use std::sync::Mutex;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

pub mod cloud;
pub mod parakeet;
pub mod sherpa;

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

pub trait EngineAdapter: Send + Sync {
    fn initialize(&mut self, model_path: &str, use_gpu: bool) -> Result<(), String>;
    fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String, String>;
}


pub struct WhisperEngine {
    context: Option<WhisperContext>,
    state: Option<Mutex<WhisperState>>,
}

impl WhisperEngine {
    pub fn new() -> Self {
        Self {
            context: None,
            state: None,
        }
    }
}

impl EngineAdapter for WhisperEngine {
    fn initialize(&mut self, model_path: &str, use_gpu: bool) -> Result<(), String> {
        // On macOS always try Metal first (safe, no Vulkan crashes). GPU flag is mainly for Linux.
        let try_gpu = use_gpu || cfg!(target_os = "macos");
        if try_gpu {
            if let Ok(result) = std::panic::catch_unwind(|| {
                let mut params = WhisperContextParameters::default();
                params.use_gpu = true;
                params.flash_attn = cfg!(target_os = "macos");

                WhisperContext::new_with_params(model_path, params)
                    .and_then(|ctx| ctx.create_state().map(|state| (ctx, state)))
            }) {
                if let Ok((ctx, state)) = result {
                    self.context = Some(ctx);
                    self.state = Some(Mutex::new(state));
                    return Ok(());
                }
            }
        }

        let mut params = WhisperContextParameters::default();
        params.use_gpu = false;
        params.flash_attn = false;

        let ctx = WhisperContext::new_with_params(model_path, params)
            .map_err(|e| format!("Failed to initialize Whisper context: {}", e))?;
        let state = ctx.create_state()
            .map_err(|e| format!("Failed to create Whisper state: {}", e))?;

        self.context = Some(ctx);
        self.state = Some(Mutex::new(state));
        Ok(())
    }

    fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String, String> {
        let _ctx = self.context.as_ref().ok_or("No model context loaded in WhisperEngine")?;
        let state_mutex = self.state.as_ref().ok_or("No Whisper state initialized")?;
        let mut state_guard = state_mutex.lock().map_err(|e| format!("State lock error: {}", e))?;
        let state = &mut *state_guard;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_temperature(0.0);
        // Optimize thread count per platform for fastest transcription.
        // On macOS (Metal) use ~half the cores (preprocessing bottleneck), clamp to 2-6.
        // On other platforms use 4-8.
        let n_threads = if cfg!(target_os = "macos") {
            ((num_cpus::get() as i32) / 2).clamp(2, 6)
        } else {
            (num_cpus::get() as i32).clamp(4, 8)
        };
        params.set_n_threads(n_threads);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_suppress_blank(true);
        params.set_suppress_nst(false);
        params.set_no_timestamps(true);
        params.set_logprob_thold(-1.0);
        params.set_no_speech_thold(0.6);

        match language {
            Some(lang) if !lang.trim().is_empty() && lang != "auto" => params.set_language(Some(lang)),
            _ => params.set_language(None),
        }
        params.set_translate(false);

        state.full(params, samples)
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

    pub fn load_model(&self, model_path: &str, use_gpu: bool) -> Result<(), String> {
        let path = std::path::Path::new(model_path);

        let engine: Box<dyn EngineAdapter> = if path.is_dir() {
            let engine = sherpa::SherpaEngine::new(model_path)?;
            Box::new(engine)
        } else if model_path.ends_with(".onnx") {
            let mut parakeet = parakeet::ParakeetEngine::new();
            parakeet.initialize(model_path, use_gpu)?;
            Box::new(parakeet)
        } else {
            let mut whisper = WhisperEngine::new();
            whisper.initialize(model_path, use_gpu)?;
            Box::new(whisper)
        };

        let mut s = self.state.lock().unwrap();
        s.engine = Some(std::sync::Arc::new(engine));
        s.active_model_path = Some(model_path.to_string());

        println!("Successfully loaded model: {}", model_path);
        Ok(())
    }

    pub fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String, String> {
        let prepared = prepare_samples(samples);

        if prepared.len() > 16_000 * 90 {
            return Err("Recording too long (>90s). Use shorter clips or a smaller/faster model (e.g. Moonshine).".to_string());
        }

        let engine_arc = {
            let s = self.state.lock().unwrap();
            s.engine.clone()
        };

        let engine = engine_arc.ok_or("No speech-to-text model loaded. Please load a model first.")?;
        engine.transcribe(&prepared, language)
    }
}
