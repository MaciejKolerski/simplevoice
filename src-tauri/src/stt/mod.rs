use std::sync::Mutex;

pub mod traits;
pub mod factory;
pub mod ggml_whisper;
pub mod onnx_engine;
pub mod converter;
pub mod downloader;
pub mod streaming;
pub(crate) mod chunker;
pub mod text;

#[cfg(feature = "candle")]
pub mod candle;

/// Non-speech markers Whisper sometimes emits inside parentheses. Square-bracketed
/// spans are stripped unconditionally; parenthesized spans are stripped only when
/// their inner text matches one of these, so real dictated parentheticals like
/// "(see below)" are kept.
const NONSPEECH_PAREN_MARKERS: &[&str] = &[
    "blank_audio", "silence", "music", "applause", "laughter", "noise", "inaudible",
];

/// Conservatively removes leftover non-speech artifacts from transcribed text and
/// normalizes whitespace. Total and pure: never panics, never errors; text that is
/// only markers becomes empty. Applied above every engine (local and cloud) as a
/// complement to Whisper's suppress_nst.
pub(crate) fn sanitize_output(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '[' {
            if let Some(rel) = chars[i + 1..].iter().position(|&ch| ch == ']') {
                i += 1 + rel + 1;
                continue;
            }
        } else if c == '(' {
            if let Some(rel) = chars[i + 1..].iter().position(|&ch| ch == ')') {
                let inner: String = chars[i + 1..i + 1 + rel].iter().collect();
                if NONSPEECH_PAREN_MARKERS.contains(&inner.trim().to_lowercase().as_str()) {
                    i += 1 + rel + 1;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn prepare_samples(samples: &[f32]) -> Vec<f32> {
    if samples.is_empty() {
        return vec![];
    }

    let threshold = 0.015;
    let mut start = 0;
    while start < samples.len() && samples[start].abs() < threshold {
        start += 1;
    }
    let mut end = samples.len();
    while end > start && samples[end - 1].abs() < threshold {
        end -= 1;
    }

    let trimmed = if end > start + 100 {
        &samples[start..end]
    } else {
        samples
    };

    let sum_sq: f64 = trimmed.iter().map(|&x| x as f64 * x as f64).sum();
    let rms = (sum_sq / trimmed.len() as f64).sqrt().max(0.001) as f32;
    let gain = 0.70 / rms;
    trimmed.iter().map(|&s| (s * gain).clamp(-1.0, 1.0)).collect()
}

pub struct ChunkedTranscription {
    pub text: String,
    /// Present when a chunk after the first failed: (offset in seconds of the
    /// failed chunk within the prepared audio, engine error). `text` holds
    /// everything transcribed before the failure.
    pub truncated: Option<(f32, String)>,
}

pub struct SttState {
    pub active_model_path: Option<String>,
    pub loading_model_path: Option<String>,
    pub engine: Option<std::sync::Arc<dyn traits::AsrEngine>>,
    /// False after a model load; set true once the engine has been warmed (first
    /// real or dummy decode), so warm-up runs at most once per loaded model.
    pub warmed: bool,
    /// When the engine was last used for idle unloading. None until first load.
    pub last_used: Option<std::time::Instant>,
    /// `use_gpu` of the last load, so an idle-unloaded model reloads identically.
    pub loaded_gpu: bool,
    /// Operations currently holding the engine (a transcription, a live session).
    /// Idle-unload must never drop the engine out from under one.
    pub in_flight: usize,
}

/// Keeps the engine alive for one operation: while a lease exists the idle-unload
/// watcher leaves the engine alone, and dropping the lease restarts the idle
/// clock, so "idle" means "5 minutes since the last use ended", not "since it
/// started" (a long transcription could otherwise be unloaded mid-run).
pub struct EngineLease {
    state: std::sync::Arc<Mutex<SttState>>,
}

impl Drop for EngineLease {
    fn drop(&mut self) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.in_flight = s.in_flight.saturating_sub(1);
        s.last_used = Some(std::time::Instant::now());
    }
}

#[derive(Clone)]
pub struct SttController {
    pub state: std::sync::Arc<Mutex<SttState>>,
    /// Serializes model loads. A reload after an idle-unload can be requested from
    /// several places within milliseconds (recording start, live worker,
    /// transcription); without this they would each load their own copy of a
    /// multi-gigabyte model.
    load_lock: std::sync::Arc<Mutex<()>>,
}

impl SttController {
    pub fn new() -> Self {
        Self {
            state: std::sync::Arc::new(Mutex::new(SttState {
                active_model_path: None,
                loading_model_path: None,
                engine: None,
                warmed: false,
                last_used: None,
                loaded_gpu: false,
                in_flight: 0,
            })),
            load_lock: std::sync::Arc::new(Mutex::new(())),
        }
    }

    pub fn load_model(&self, model_path: &str, use_gpu: bool) -> Result<(), String> {
        let _guard = self.load_lock.lock().unwrap_or_else(|e| e.into_inner());
        // Someone may have loaded exactly this while we waited for the lock. The
        // record-start preload and the frontend's restore can ask at the same
        // moment. Loading it twice would briefly hold two copies of a
        // multi-gigabyte model (in VRAM, on the GPU path).
        {
            let s = self.state.lock().unwrap();
            if s.engine.is_some()
                && s.active_model_path.as_deref() == Some(model_path)
                && s.loaded_gpu == use_gpu
            {
                return Ok(());
            }
        }
        self.load_locked(model_path, use_gpu)
    }

    /// The actual load. Callers must hold `load_lock`.
    fn load_locked(&self, model_path: &str, use_gpu: bool) -> Result<(), String> {
        let path = std::path::Path::new(model_path);
        let engine = factory::AsrFactory::load(path, use_gpu)
            .map_err(|e| format!("Failed to load model: {}", e))?;

        let mut s = self.state.lock().unwrap();
        s.engine = Some(std::sync::Arc::from(engine));
        s.active_model_path = Some(model_path.to_string());
        s.warmed = false;
        s.loaded_gpu = use_gpu;
        s.last_used = Some(std::time::Instant::now());

        println!("Successfully loaded ASR model: {}", model_path);
        Ok(())
    }

    /// Marks a model as selected without loading it. Used at startup to restore
    /// the last model from config: recording is then allowed immediately and the
    /// engine is loaded on demand, instead of the app claiming "no model
    /// selected" until the frontend gets around to calling `load_model`.
    pub fn set_selected_model(&self, model_path: &str, use_gpu: bool) {
        let mut s = self.state.lock().unwrap();
        if s.active_model_path.is_none() {
            s.active_model_path = Some(model_path.to_string());
            s.loaded_gpu = use_gpu;
        }
    }

    /// The engine, if it is loaded right now (refreshing the idle clock).
    fn engine_now(&self) -> Option<std::sync::Arc<dyn traits::AsrEngine>> {
        let mut s = self.state.lock().unwrap();
        let engine = s.engine.clone();
        if engine.is_some() {
            s.last_used = Some(std::time::Instant::now());
        }
        engine
    }

    /// Returns the engine, reloading the selected model first when an idle-unload
    /// dropped it. `Ok(None)` means no local model is selected at all (cloud
    /// engine, or a fresh install), which is not an error at this level.
    pub fn ensure_loaded(&self) -> Result<Option<std::sync::Arc<dyn traits::AsrEngine>>, String> {
        if let Some(engine) = self.engine_now() {
            return Ok(Some(engine));
        }
        let _guard = self.load_lock.lock().unwrap_or_else(|e| e.into_inner());
        // Another caller may have reloaded it while we waited for the lock.
        if let Some(engine) = self.engine_now() {
            return Ok(Some(engine));
        }
        let selected = {
            let s = self.state.lock().unwrap();
            s.active_model_path.clone().map(|p| (p, s.loaded_gpu))
        };
        let Some((path, gpu)) = selected else {
            return Ok(None);
        };
        tracing::info!("reloading the idle-unloaded ASR model: {}", path);
        self.load_locked(&path, gpu)?;
        Ok(self.engine_now())
    }

    /// Claims the engine until the returned lease is dropped (see `EngineLease`).
    pub fn lease(&self) -> EngineLease {
        {
            let mut s = self.state.lock().unwrap();
            s.in_flight += 1;
            s.last_used = Some(std::time::Instant::now());
        }
        EngineLease { state: std::sync::Arc::clone(&self.state) }
    }

    /// Returns the active engine for a one-time warm-up the first time it is called
    /// after a model load (marking it warmed so later calls return None). Returns
    /// None when there is no local engine (e.g. a cloud provider) or it is already
    /// warmed.
    pub fn take_engine_to_warm(&self) -> Option<std::sync::Arc<dyn traits::AsrEngine>> {
        let mut s = self.state.lock().unwrap();
        if s.warmed {
            return None;
        }
        let engine = s.engine.clone();
        if engine.is_some() {
            s.warmed = true;
        }
        engine
    }

    /// Drops the loaded engine to free RAM/VRAM. `active_model_path`/`loaded_gpu`
    /// are kept so the next transcription reloads it transparently.
    pub fn unload(&self) {
        let mut s = self.state.lock().unwrap();
        s.engine = None;
        s.warmed = false;
    }

    /// Unloads the engine if it has been idle for at least `idle_secs`. Returns true
    /// if it unloaded. No-op when nothing is loaded, when it was used recently, or
    /// while an operation holds a lease on it. Decision and unload happen under one
    /// lock, so a transcription that starts in between cannot lose its engine.
    pub fn unload_if_idle(&self, idle_secs: u64) -> bool {
        let mut s = self.state.lock().unwrap();
        let idle = s.engine.is_some()
            && s.in_flight == 0
            && s.last_used.map_or(false, |t| t.elapsed().as_secs() >= idle_secs);
        if idle {
            s.engine = None;
            s.warmed = false;
        }
        idle
    }

    pub fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String, String> {
        self.transcribe_with_progress(samples, language, &mut |_, _| {})
            .map(|c| c.text)
    }

    /// Transcribes input of any length by splitting it at silence boundaries
    /// and running the active engine once per chunk. `progress(done, total)`
    /// fires after every successfully transcribed chunk.
    pub fn transcribe_with_progress(
        &self,
        samples: &[f32],
        language: Option<&str>,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<ChunkedTranscription, String> {
        let prepared = prepare_samples(samples);

        // Hold the engine for the whole run: the lease keeps the idle-unload
        // watcher from dropping it mid-transcription, and `ensure_loaded`
        // reloads it transparently if an earlier unload already did.
        let _lease = self.lease();
        let engine = self
            .ensure_loaded()?
            .ok_or("No speech-to-text model loaded. Please load an ASR model first.")?;

        let chunks = chunker::split_at_silences(&prepared);
        let total = chunks.len();
        let mut parts: Vec<String> = Vec::with_capacity(total);
        let mut truncated = None;

        for (i, range) in chunks.iter().enumerate() {
            match engine.transcribe(&prepared[range.clone()], language) {
                Ok(text) => {
                    let text = sanitize_output(&text);
                    if !text.is_empty() {
                        parts.push(text);
                    }
                }
                Err(e) => {
                    let err = format!("Transcription failed: {}", e);
                    // parts.is_empty(), not i == 0: if earlier chunks produced
                    // only empty text, a "partial" result would paste a lone
                    // truncation marker into the user's document.
                    if parts.is_empty() {
                        return Err(err);
                    }
                    truncated = Some((range.start as f32 / chunker::SAMPLE_RATE as f32, err));
                    break;
                }
            }
            progress(i + 1, total);
        }

        Ok(ChunkedTranscription {
            text: text::collapse_repeats(&parts.join(" ")),
            truncated,
        })
    }
}

/// An engine handle that resolves the controller's engine on every call instead of
/// capturing it once. Handed to the live (streaming) session so a recording started
/// while the model is idle-unloaded still transcribes: the audio tap is
/// installed immediately without losing speech, and the first re-decode reloads the
/// model. It holds a lease for the session's lifetime, so the watcher cannot unload
/// the engine underneath a live recording.
pub struct LazyEngine {
    controller: SttController,
    _lease: EngineLease,
}

impl LazyEngine {
    pub fn new(controller: SttController) -> Self {
        let lease = controller.lease();
        Self { controller, _lease: lease }
    }
}

impl traits::AsrEngine for LazyEngine {
    fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String, crate::error::AppError> {
        let engine = self
            .controller
            .ensure_loaded()
            .map_err(crate::error::AppError::Model)?
            .ok_or_else(|| {
                crate::error::AppError::Model("No speech-to-text model loaded".to_string())
            })?;
        engine.transcribe(samples, language)
    }

    fn display_name(&self) -> &str {
        "Local model (loaded on demand)"
    }

    fn model_format(&self) -> traits::ModelFormat {
        let s = self.controller.state.lock().unwrap();
        s.engine
            .as_ref()
            .map(|e| e.model_format())
            .unwrap_or(traits::ModelFormat::GgmlBin)
    }

    fn gpu_accelerated(&self) -> bool {
        let s = self.controller.state.lock().unwrap();
        s.engine.as_ref().map_or(s.loaded_gpu, |e| e.gpu_accelerated())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::stt::traits::{AsrEngine, ModelFormat};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeEngine {
        calls: AtomicUsize,
        /// 1-based call number from which transcribe starts failing.
        fail_from_call: Option<usize>,
        /// Calls with number <= this return Ok(String::new()) (failure check wins if both apply).
        empty_until_call: usize,
    }

    impl FakeEngine {
        fn ok() -> Self {
            Self { calls: AtomicUsize::new(0), fail_from_call: None, empty_until_call: 0 }
        }
        fn failing_from(n: usize) -> Self {
            Self { calls: AtomicUsize::new(0), fail_from_call: Some(n), empty_until_call: 0 }
        }
    }

    impl AsrEngine for FakeEngine {
        fn transcribe(&self, _samples: &[f32], _language: Option<&str>) -> Result<String, AppError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(n) = self.fail_from_call {
                if call >= n {
                    return Err(AppError::Command("boom".into()));
                }
            }
            if call <= self.empty_until_call {
                return Ok(String::new());
            }
            Ok(format!("part{}", call))
        }
        fn display_name(&self) -> &str {
            "fake"
        }
        fn model_format(&self) -> ModelFormat {
            ModelFormat::GgmlBin
        }
    }

    fn controller_with(engine: FakeEngine) -> SttController {
        let c = SttController::new();
        c.state.lock().unwrap().engine = Some(std::sync::Arc::new(engine));
        c
    }

    /// 0.5 amplitude: above prepare_samples' trim threshold, so length is stable.
    fn speech(secs: usize) -> Vec<f32> {
        vec![0.5; secs * 16_000]
    }

    #[test]
    fn no_engine_errors() {
        let c = SttController::new();
        let err = c.transcribe(&speech(1), None).unwrap_err();
        assert!(err.contains("No speech-to-text model loaded"));
    }

    /// Backdates `last_used` so the controller looks idle for `secs`.
    fn make_idle_for(c: &SttController, secs: u64) {
        let mut s = c.state.lock().unwrap();
        s.last_used = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(secs));
    }

    #[test]
    fn unloads_only_after_the_idle_period() {
        let c = controller_with(FakeEngine::ok());
        make_idle_for(&c, 60);
        assert!(!c.unload_if_idle(300), "60 s idle must not unload");
        make_idle_for(&c, 400);
        assert!(c.unload_if_idle(300), "400 s idle must unload");
        assert!(c.state.lock().unwrap().engine.is_none());
        assert!(!c.unload_if_idle(300), "nothing left to unload");
    }

    #[test]
    fn a_lease_keeps_the_engine_loaded_however_long_it_is_held() {
        // A transcription longer than the idle period used to be able to lose its
        // engine to the watcher half-way through.
        let c = controller_with(FakeEngine::ok());
        let lease = c.lease();
        make_idle_for(&c, 4_000);
        assert!(!c.unload_if_idle(300), "must not unload while in use");
        assert!(c.state.lock().unwrap().engine.is_some());
        // Dropping the lease restarts the idle clock, so the next tick is a no-op.
        drop(lease);
        assert!(!c.unload_if_idle(300));
        // Only idling again from that point unloads.
        make_idle_for(&c, 400);
        assert!(c.unload_if_idle(300));
    }

    #[test]
    fn ensure_loaded_is_none_when_no_model_is_selected() {
        let c = SttController::new();
        assert!(c.ensure_loaded().unwrap().is_none());
    }

    #[test]
    fn ensure_loaded_returns_the_live_engine_and_refreshes_idle() {
        let c = controller_with(FakeEngine::ok());
        make_idle_for(&c, 4_000);
        assert!(c.ensure_loaded().unwrap().is_some());
        // Touching the engine counts as use: the watcher must not unload it now.
        assert!(!c.unload_if_idle(300));
    }

    #[test]
    fn transcription_after_an_idle_unload_reports_the_missing_model_path() {
        // Engine dropped by the watcher and no path to reload from (only possible
        // when nothing was ever loaded) -> a clear error, not a panic.
        let c = controller_with(FakeEngine::ok());
        make_idle_for(&c, 400);
        assert!(c.unload_if_idle(300));
        let err = c.transcribe(&speech(1), None).unwrap_err();
        assert!(err.contains("No speech-to-text model loaded"));
    }

    #[test]
    fn loading_the_already_loaded_model_is_a_no_op() {
        // The path does not exist, so a real load would fail. Reaching Ok proves
        // the redundant reload was skipped instead of tearing the engine down.
        let c = controller_with(FakeEngine::ok());
        {
            let mut s = c.state.lock().unwrap();
            s.active_model_path = Some("/models/whisper.bin".into());
            s.loaded_gpu = true;
        }
        assert!(c.load_model("/models/whisper.bin", true).is_ok());
        assert!(c.state.lock().unwrap().engine.is_some());
        // A different GPU setting is a real change and must not be skipped.
        assert!(c.load_model("/models/whisper.bin", false).is_err());
    }

    #[test]
    fn set_selected_model_marks_a_model_without_loading_it() {
        let c = SttController::new();
        c.set_selected_model("/models/whisper.bin", true);
        let s = c.state.lock().unwrap();
        assert_eq!(s.active_model_path.as_deref(), Some("/models/whisper.bin"));
        assert!(s.loaded_gpu);
        assert!(s.engine.is_none(), "restoring the choice must not load anything");
    }

    #[test]
    fn set_selected_model_never_overrides_a_loaded_model() {
        let c = controller_with(FakeEngine::ok());
        c.state.lock().unwrap().active_model_path = Some("/models/loaded.bin".into());
        c.set_selected_model("/models/from-config.bin", false);
        assert_eq!(
            c.state.lock().unwrap().active_model_path.as_deref(),
            Some("/models/loaded.bin")
        );
    }

    #[test]
    fn short_input_single_engine_call() {
        let c = controller_with(FakeEngine::ok());
        let mut progress: Vec<(usize, usize)> = Vec::new();
        let out = c
            .transcribe_with_progress(&speech(30), None, &mut |d, t| progress.push((d, t)))
            .unwrap();
        assert_eq!(out.text, "part1");
        assert!(out.truncated.is_none());
        assert_eq!(progress, vec![(1, 1)]);
    }

    #[test]
    fn long_input_is_chunked_and_joined_with_progress() {
        // 120 s of pauseless speech -> 2 chunks (45 s fallback cut + 75 s).
        let c = controller_with(FakeEngine::ok());
        let mut progress: Vec<(usize, usize)> = Vec::new();
        let out = c
            .transcribe_with_progress(&speech(120), None, &mut |d, t| progress.push((d, t)))
            .unwrap();
        assert_eq!(out.text, "part1 part2");
        assert!(out.truncated.is_none());
        assert_eq!(progress, vec![(1, 2), (2, 2)]);
    }

    #[test]
    fn over_90s_no_longer_errors() {
        let c = controller_with(FakeEngine::ok());
        assert_eq!(c.transcribe(&speech(91), None).unwrap(), "part1 part2");
    }

    #[test]
    fn first_chunk_failure_propagates_the_error() {
        let c = controller_with(FakeEngine::failing_from(1));
        let err = c.transcribe(&speech(120), None).unwrap_err();
        assert!(err.contains("Transcription failed"));
    }

    #[test]
    fn later_chunk_failure_returns_partial_text() {
        let c = controller_with(FakeEngine::failing_from(2));
        let mut progress: Vec<(usize, usize)> = Vec::new();
        let out = c
            .transcribe_with_progress(&speech(120), None, &mut |d, t| progress.push((d, t)))
            .unwrap();
        assert_eq!(out.text, "part1");
        let (secs, err) = out.truncated.expect("must report truncation");
        assert!((44.0..=46.0).contains(&secs), "failed chunk starts at ~45s, got {}", secs);
        assert!(err.contains("boom"));
        assert_eq!(progress, vec![(1, 2)], "no progress for the failed chunk");
    }

    #[test]
    fn empty_input_returns_empty_text_without_engine_calls() {
        let c = controller_with(FakeEngine::ok());
        let out = c.transcribe(&[], None).unwrap();
        assert_eq!(out, "");
    }

    #[test]
    fn empty_first_chunk_then_failure_is_an_error() {
        let c = controller_with(FakeEngine {
            calls: AtomicUsize::new(0),
            fail_from_call: Some(2),
            empty_until_call: 1,
        });
        let err = c.transcribe(&speech(120), None).unwrap_err();
        assert!(err.contains("Transcription failed"));
    }

    #[test]
    fn sanitize_strips_square_bracket_markers() {
        assert_eq!(sanitize_output("hello [BLANK_AUDIO] world"), "hello world");
        assert_eq!(sanitize_output("[ Silence ]"), "");
        assert_eq!(sanitize_output("[Music] hi"), "hi");
    }

    #[test]
    fn sanitize_strips_known_paren_markers_only() {
        assert_eq!(sanitize_output("hi (music) there"), "hi there");
        assert_eq!(sanitize_output("(applause)"), "");
        assert_eq!(sanitize_output("note (see below) please"), "note (see below) please");
    }

    #[test]
    fn sanitize_collapses_whitespace_and_trims() {
        assert_eq!(sanitize_output("  a   b  "), "a b");
    }

    #[test]
    fn sanitize_keeps_plain_text_and_real_parens() {
        assert_eq!(sanitize_output("To jest nagranie"), "To jest nagranie");
        assert_eq!(sanitize_output("koszt (netto) wynosi"), "koszt (netto) wynosi");
    }
}
