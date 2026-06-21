use std::sync::Mutex;

pub mod cloud;
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
}

#[derive(Clone)]
pub struct SttController {
    pub state: std::sync::Arc<Mutex<SttState>>,
}

impl SttController {
    pub fn new() -> Self {
        Self {
            state: std::sync::Arc::new(Mutex::new(SttState {
                active_model_path: None,
                loading_model_path: None,
                engine: None,
                warmed: false,
            })),
        }
    }

    pub fn load_model(&self, model_path: &str, use_gpu: bool) -> Result<(), String> {
        let path = std::path::Path::new(model_path);
        let engine = factory::AsrFactory::load(path, use_gpu)
            .map_err(|e| format!("Failed to load model: {}", e))?;

        let mut s = self.state.lock().unwrap();
        s.engine = Some(std::sync::Arc::from(engine));
        s.active_model_path = Some(model_path.to_string());
        s.warmed = false;

        println!("Successfully loaded ASR model: {}", model_path);
        Ok(())
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

        let engine_arc = {
            let s = self.state.lock().unwrap();
            s.engine.clone()
        };
        let engine =
            engine_arc.ok_or("No speech-to-text model loaded. Please load an ASR model first.")?;

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
