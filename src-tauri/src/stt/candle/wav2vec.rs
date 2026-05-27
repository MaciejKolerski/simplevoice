use crate::error::AppError;
use crate::stt::traits::{AsrEngine, ModelFormat};

pub struct Wav2VecEngine;

impl Wav2VecEngine {
    pub fn initialize(_model_dir: &std::path::Path, _use_gpu: bool) -> Result<Self, AppError> {
        Ok(Self)
    }
}

impl AsrEngine for Wav2VecEngine {
    fn transcribe(
        &self,
        _samples: &[f32],
        _language: Option<&str>,
    ) -> Result<String, AppError> {
        Err(AppError::Model(
            "Wav2Vec2 / MMS is not supported natively by Candle. Please convert the model to ONNX format and load it as an ONNX model.".to_string()
        ))
    }

    fn display_name(&self) -> &str {
        "Wav2Vec2 (Candle)"
    }

    fn model_format(&self) -> ModelFormat {
        ModelFormat::HfSafetensors
    }

    fn supports_language_hint(&self) -> bool {
        false
    }

    fn gpu_accelerated(&self) -> bool {
        false
    }
}
