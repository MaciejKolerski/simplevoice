use std::sync::Mutex;

pub mod cloud;
pub mod traits;
pub mod factory;
pub mod ggml_whisper;
pub mod onnx_engine;
pub mod nemo_engine;
pub mod converter;
pub mod downloader;
pub mod streaming;
pub(crate) mod chunker;

#[cfg(feature = "candle")]
pub mod candle;

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

        println!("Successfully loaded ASR model: {}", model_path);
        Ok(())
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
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        parts.push(text);
                    }
                }
                Err(e) => {
                    let err = format!("Transcription failed: {}", e);
                    if i == 0 {
                        return Err(err);
                    }
                    truncated = Some((range.start as f32 / chunker::SAMPLE_RATE as f32, err));
                    break;
                }
            }
            progress(i + 1, total);
        }

        Ok(ChunkedTranscription {
            text: parts.join(" "),
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
    }

    impl FakeEngine {
        fn ok() -> Self {
            Self { calls: AtomicUsize::new(0), fail_from_call: None }
        }
        fn failing_from(n: usize) -> Self {
            Self { calls: AtomicUsize::new(0), fail_from_call: Some(n) }
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
}
