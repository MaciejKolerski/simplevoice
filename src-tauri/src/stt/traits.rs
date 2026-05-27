use crate::error::AppError;

/// Common interface for all local ASR engines.
/// Arc<dyn AsrEngine> is held in SttState.
pub trait AsrEngine: Send + Sync {
    fn transcribe(
        &self,
        samples: &[f32],          // PCM 16 kHz mono
        language: Option<&str>,   // None = auto-detect
    ) -> Result<String, AppError>;

    fn display_name(&self) -> &str;
    fn model_format(&self) -> ModelFormat;
    fn supports_language_hint(&self) -> bool { true }
    fn gpu_accelerated(&self) -> bool { false }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelFormat {
    GgmlBin,        // whisper.cpp / whisper-rs, *.bin file
    Gguf,           // whisper.cpp / whisper-rs >=0.17, *.gguf file
    HfSafetensors,  // Hugging Face directory with model.safetensors
    HfPytorch,      // Hugging Face directory with pytorch_model.bin
    Onnx,           // Directory with *.onnx (exported via Optimum)
    Nemo,           // NVIDIA NeMo, *.nemo file (experimental)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelInfo {
    pub path: String,
    pub format: ModelFormat,
    pub architecture: Option<String>, // "Whisper", "Wav2Vec2CTC", "FastConformer", ...
    pub hf_model_id: Option<String>,  // From config.json -> _name_or_path
    pub display_name: String,
    pub filename: String,
    pub size_bytes: u64,
    pub size_formatted: String,
    pub quality_score: u8,   // 0-100, for sorting in UI
    pub speed_score: u8,     // 0-100
    pub is_active: bool,
    pub needs_conversion: bool, // True = HF safetensors without ONNX -> show "Convert" button
}
