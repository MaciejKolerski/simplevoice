use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};
use crate::error::AppError;
use crate::stt::traits::{AsrEngine, ModelFormat};

/// Whisper beam width: 0 = greedy (fast preset), >=1 = beam search (accurate preset).
/// Process-global so the "Accuracy vs Speed" setting applies without re-threading a
/// param through every engine call; set from config before each transcription (A2/A8).
pub static WHISPER_BEAM_SIZE: AtomicI32 = AtomicI32::new(0);

/// Whisper initial prompt (custom dictionary, A3): biases decoding toward these
/// words/names so proper nouns and jargon transcribe correctly. Empty = no prompt.
/// Set from config before each transcription.
pub static WHISPER_INITIAL_PROMPT: Mutex<String> = Mutex::new(String::new());

pub struct GgmlWhisperEngine {
    _context: WhisperContext,
    state: Mutex<WhisperState>,
    gpu_active: bool,
}

impl GgmlWhisperEngine {
    pub fn initialize(model_path: &str, use_gpu: bool) -> Result<Self, AppError> {
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
                    return Ok(Self {
                        _context: ctx,
                        state: Mutex::new(state),
                        gpu_active: true,
                    });
                }
            }
        }

        let mut params = WhisperContextParameters::default();
        params.use_gpu = false;
        params.flash_attn = false;

        let ctx = WhisperContext::new_with_params(model_path, params)
            .map_err(|e| AppError::Model(format!("Failed to initialize Whisper context: {}", e)))?;
        let state = ctx.create_state()
            .map_err(|e| AppError::Model(format!("Failed to create Whisper state: {}", e)))?;

        Ok(Self {
            _context: ctx,
            state: Mutex::new(state),
            gpu_active: false,
        })
    }
}

impl AsrEngine for GgmlWhisperEngine {
    fn transcribe(
        &self,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<String, AppError> {
        let mut state_guard = self.state.lock().map_err(|e| AppError::Model(format!("State lock error: {}", e)))?;
        let state = &mut *state_guard;

        let beam = WHISPER_BEAM_SIZE.load(Ordering::Relaxed);
        let strategy = if beam >= 1 {
            SamplingStrategy::BeamSearch { beam_size: beam, patience: -1.0 }
        } else {
            SamplingStrategy::Greedy { best_of: 2 }
        };
        let mut params = FullParams::new(strategy);
        params.set_temperature(0.0);
        params.set_temperature_inc(0.2);
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
        params.set_suppress_nst(true);
        params.set_no_timestamps(true);
        params.set_logprob_thold(-1.0);
        params.set_no_speech_thold(0.6);
        params.set_no_context(true);

        match language {
            Some(lang) if !lang.trim().is_empty() && lang != "auto" => params.set_language(Some(lang)),
            _ => params.set_language(None),
        }
        params.set_translate(false);

        let initial_prompt = WHISPER_INITIAL_PROMPT.lock().unwrap().clone();
        if !initial_prompt.is_empty() {
            params.set_initial_prompt(&initial_prompt);
        }

        state.full(params, samples)
            .map_err(|e| AppError::Model(format!("Whisper inference run failed: {}", e)))?;

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

    fn display_name(&self) -> &str {
        "Whisper GGML"
    }

    fn model_format(&self) -> ModelFormat {
        ModelFormat::GgmlBin
    }

    fn supports_language_hint(&self) -> bool {
        true
    }

    fn gpu_accelerated(&self) -> bool {
        self.gpu_active
    }
}
