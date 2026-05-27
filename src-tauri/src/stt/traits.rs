use crate::error::AppError;

/// Wspólny interfejs dla wszystkich lokalnych backendów ASR.
/// Arc<dyn AsrEngine> trzymany w SttState.
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
    GgmlBin,        // whisper.cpp / whisper-rs, plik *.bin
    Gguf,           // whisper.cpp / whisper-rs >=0.17, plik *.gguf
    HfSafetensors,  // Hugging Face folder z model.safetensors
    HfPytorch,      // Hugging Face folder z pytorch_model.bin
    Onnx,           // folder z *.onnx (wyeksportowany przez optimum)
    Nemo,           // NVIDIA NeMo, plik *.nemo (experimental)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelInfo {
    pub path: String,
    pub format: ModelFormat,
    pub architecture: Option<String>, // "Whisper", "Wav2Vec2CTC", "FastConformer", ...
    pub hf_model_id: Option<String>,  // z config.json → _name_or_path
    pub display_name: String,
    pub filename: String,
    pub size_bytes: u64,
    pub size_formatted: String,
    pub quality_score: u8,   // 0-100, do sortowania w UI
    pub speed_score: u8,     // 0-100
    pub is_active: bool,
    pub needs_conversion: bool, // True = HF safetensors bez ONNX → pokaż przycisk "Convert"
}
