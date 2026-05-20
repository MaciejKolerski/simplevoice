use std::sync::Mutex;
use whisper_rs::{WhisperContext, WhisperContextParameters, FullParams, SamplingStrategy};

pub mod cloud;
pub mod parakeet;
pub mod sherpa;

pub trait EngineAdapter: Send + Sync {
    fn initialize(&mut self, model_path: &str) -> Result<(), String>;
    fn transcribe(&self, samples: &[f32]) -> Result<String, String>;
    fn shutdown(&mut self) -> Result<(), String>;
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
        let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
            .map_err(|e| format!("Failed to initialize Whisper context from {}: {}", model_path, e))?;
        self.context = Some(ctx);
        Ok(())
    }

    fn transcribe(&self, samples: &[f32]) -> Result<String, String> {
        let ctx = self.context.as_ref().ok_or("No model context loaded in WhisperEngine")?;
        
        let mut state = ctx.create_state()
            .map_err(|e| format!("Failed to create Whisper state: {}", e))?;
            
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_temperature(0.0);
        params.set_initial_prompt("The following is a transcription of voice instructions, notes, thoughts, and programming code snippets.");

        state.full(params, samples)
            .map_err(|e| format!("Whisper inference run failed: {}", e))?;

        let mut text = String::new();
        let num_segments = state.full_n_segments()
            .map_err(|e| format!("Failed to query number of segments: {}", e))?;
            
        for i in 0..num_segments {
            if let Ok(segment_text) = state.full_get_segment_text(i) {
                text.push_str(&segment_text);
            }
        }

        Ok(text.trim().to_string())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.context = None;
        Ok(())
    }
}

pub struct SttState {
    pub active_model_path: Option<String>,
    pub engine: Option<Box<dyn EngineAdapter>>,
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
                engine: None,
            })),
        }
    }

    pub fn load_model(&self, model_path: &str) -> Result<(), String> {
        let path = std::path::Path::new(model_path);
        
        let engine: Box<dyn EngineAdapter> = if path.is_dir() {
            let engine = sherpa::SherpaEngine::new(model_path)?;
            Box::new(engine)
        } else if model_path.ends_with(".onnx") {
            Box::new(parakeet::ParakeetEngine::new(model_path))
        } else {
            let mut whisper = WhisperEngine::new();
            whisper.initialize(model_path)?;
            Box::new(whisper)
        };

        let mut s = self.state.lock().unwrap();
        if let Some(mut old_engine) = s.engine.take() {
            let _ = old_engine.shutdown();
        }
        
        s.engine = Some(engine);
        s.active_model_path = Some(model_path.to_string());
        
        println!("Successfully loaded model: {}", model_path);
        Ok(())
    }

    pub fn transcribe(&self, samples: &[f32]) -> Result<String, String> {
        let s = self.state.lock().unwrap();
        let engine = s.engine.as_ref().ok_or("No speech-to-text model loaded. Please load a model first.")?;
        engine.transcribe(samples)
    }
}
