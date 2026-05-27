use std::sync::Mutex;

pub mod cloud;
pub mod traits;
pub mod factory;
pub mod ggml_whisper;
pub mod onnx_engine;
pub mod nemo_engine;

#[cfg(feature = "candle")]
pub mod candle;

fn prepare_samples(samples: &[f32]) -> Vec<f32> {
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

    let sum_sq: f32 = trimmed.iter().map(|&x| x * x).sum();
    let rms = (sum_sq / trimmed.len() as f32).sqrt().max(0.001);
    let gain = 0.70 / rms;
    trimmed.iter().map(|&s| (s * gain).clamp(-1.0, 1.0)).collect()
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
        let prepared = prepare_samples(samples);

        if prepared.len() > 16_000 * 90 {
            return Err("Recording too long (>90s). Please use shorter clips.".to_string());
        }

        let engine_arc = {
            let s = self.state.lock().unwrap();
            s.engine.clone()
        };

        let engine = engine_arc.ok_or("No speech-to-text model loaded. Please load an ASR model first.")?;
        engine.transcribe(&prepared, language)
            .map_err(|e| format!("Transcription failed: {}", e))
    }
}
