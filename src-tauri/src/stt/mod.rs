use std::sync::Mutex;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub mod cloud;
pub mod parakeet;
pub mod sherpa;

pub trait EngineAdapter: Send + Sync {
    fn initialize(&mut self, model_path: &str) -> Result<(), String>;
    fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String, String>;
}

pub struct WhisperEngine {
    context: Option<WhisperContext>,
}

impl WhisperEngine {
    pub fn new() -> Self {
        Self { context: None }
    }
}

impl EngineAdapter for WhisperEngine {
    fn initialize(&mut self, model_path: &str) -> Result<(), String> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu = false;
        params.flash_attn = false;

        let ctx = WhisperContext::new_with_params(model_path, params)
            .map_err(|e| {
                format!(
                    "Failed to initialize Whisper context from {}: {}",
                    model_path, e
                )
            })?;
        self.context = Some(ctx);
        Ok(())
    }

    fn transcribe(&self, samples: &[f32], _language: Option<&str>) -> Result<String, String> {
        let ctx = self
            .context
            .as_ref()
            .ok_or("No model context loaded in WhisperEngine")?;

        let mut state = ctx
            .create_state()
            .map_err(|e| format!("Failed to create Whisper state: {}", e))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_temperature(0.0);
        let n_threads = (num_cpus::get() as i32).max(2) / 2;
        params.set_n_threads(n_threads);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_suppress_blank(false);
        params.set_suppress_nst(false);
        params.set_no_timestamps(false);
        params.set_logprob_thold(-1.0);
        params.set_no_speech_thold(0.6);

        state
            .full(params, samples)
            .map_err(|e| format!("Whisper inference run failed: {}", e))?;

        let mut text = String::new();
        let num_segments = state.full_n_segments();

        for i in 0..num_segments {
            if let Some(segment) = state.get_segment(i) {
                if let Ok(segment_text) = segment.to_str() {
                    text.push_str(segment_text);
                }
            }
        }

        Ok(text.trim().to_string())
    }
}

pub struct SttState {
    pub active_model_path: Option<String>,
    pub loading_model_path: Option<String>,
    pub engine: Option<std::sync::Arc<Box<dyn EngineAdapter>>>,
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

    pub fn load_model(&self, model_path: &str) -> Result<(), String> {
        {
            let s = self.state.lock().unwrap();

            if let Some(ref active) = s.active_model_path {
                if active == model_path {
                    return Ok(());
                }
            }
        }

        let path = std::path::Path::new(model_path);

        let engine: Box<dyn EngineAdapter> = if path.is_dir() {
            let engine = sherpa::SherpaEngine::new(model_path)?;
            Box::new(engine)
        } else if model_path.ends_with(".onnx") {
            let mut parakeet = parakeet::ParakeetEngine::new();
            parakeet.initialize(model_path)?;
            Box::new(parakeet)
        } else {
            let mut whisper = WhisperEngine::new();
            whisper.initialize(model_path)?;
            Box::new(whisper)
        };

        let mut s = self.state.lock().unwrap();
        s.engine = Some(std::sync::Arc::new(engine));
        s.active_model_path = Some(model_path.to_string());

        println!("Successfully loaded model: {}", model_path);
        Ok(())
    }

    pub fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String, String> {
        let engine_arc = {
            let s = self.state.lock().unwrap();
            s.engine.clone()
        };

        let engine = engine_arc.ok_or("No speech-to-text model loaded. Please load a model first.")?;
        engine.transcribe(samples, language)
    }
}
