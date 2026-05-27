use crate::error::AppError;
use crate::stt::traits::{AsrEngine, ModelFormat};

#[cfg(feature = "onnx")]
use std::path::Path;
#[cfg(feature = "onnx")]
use ort::session::Session;

#[cfg(feature = "onnx")]
#[allow(dead_code)]
pub struct OnnxEngine {
    session: Session,
    model_path: String,
}

#[cfg(feature = "onnx")]
impl OnnxEngine {
    pub fn initialize(path: &Path, _use_gpu: bool) -> Result<Self, AppError> {
        let onnx_path = if path.is_dir() {
            let mut found_path = None;
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if entry_path.is_file() && entry_path.extension().is_some_and(|ext| ext == "onnx") {
                        found_path = Some(entry_path);
                        break;
                    }
                }
            }
            found_path.ok_or_else(|| AppError::Model("No .onnx file found in directory".to_string()))?
        } else {
            path.to_path_buf()
        };

        // Initialize ONNX Runtime session
        let session = Session::builder()
            .map_err(|e| AppError::Model(format!("Failed to create ONNX builder: {}", e)))?
            .commit_from_file(&onnx_path)
            .map_err(|e| AppError::Model(format!("Failed to load ONNX model: {}", e)))?;

        Ok(Self {
            session,
            model_path: onnx_path.to_string_lossy().to_string(),
        })
    }
}

#[cfg(feature = "onnx")]
impl AsrEngine for OnnxEngine {
    fn transcribe(
        &self,
        _samples: &[f32],
        _language: Option<&str>,
    ) -> Result<String, AppError> {
        // ONNX models (e.g. Moonshine, Parakeet, Whisper ONNX) require different pre- and post-processing.
        // This is a placeholder for ONNX-based speech-to-text.
        Ok(format!("ONNX Transcription Placeholder (model: {})", self.model_path))
    }

    fn display_name(&self) -> &str {
        "Universal ONNX Engine"
    }

    fn model_format(&self) -> ModelFormat {
        ModelFormat::Onnx
    }

    fn supports_language_hint(&self) -> bool {
        false
    }

    fn gpu_accelerated(&self) -> bool {
        false
    }
}

#[cfg(not(feature = "onnx"))]
pub struct OnnxEngine;

#[cfg(not(feature = "onnx"))]
impl OnnxEngine {
    pub fn initialize(_path: &std::path::Path, _use_gpu: bool) -> Result<Self, AppError> {
        Err(AppError::Model("ONNX support is not compiled in. Build with --features onnx.".to_string()))
    }
}

#[cfg(not(feature = "onnx"))]
impl AsrEngine for OnnxEngine {
    fn transcribe(
        &self,
        _samples: &[f32],
        _language: Option<&str>,
    ) -> Result<String, AppError> {
        Err(AppError::Model("ONNX support is not compiled in.".to_string()))
    }

    fn display_name(&self) -> &str {
        "ONNX (Not Compiled)"
    }

    fn model_format(&self) -> ModelFormat {
        ModelFormat::Onnx
    }

    fn supports_language_hint(&self) -> bool {
        false
    }

    fn gpu_accelerated(&self) -> bool {
        false
    }
}
