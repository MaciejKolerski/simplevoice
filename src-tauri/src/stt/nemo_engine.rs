use std::path::Path;
use std::process::Command;
use crate::error::AppError;
use crate::stt::traits::{AsrEngine, ModelFormat};

pub struct NemoEngine {
    model_path: String,
}

impl NemoEngine {
    pub fn initialize(path: &Path, _use_gpu: bool) -> Result<Self, AppError> {
        Ok(Self {
            model_path: path.to_string_lossy().to_string(),
        })
    }
}

impl AsrEngine for NemoEngine {
    fn transcribe(
        &self,
        samples: &[f32],
        _language: Option<&str>,
    ) -> Result<String, AppError> {
        // Create a temporary WAV file
        let temp_dir = std::env::temp_dir();
        let temp_wav_path = temp_dir.join(format!("nemo_temp_{}.wav", chrono::Utc::now().timestamp_micros()));

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        {
            let mut writer = hound::WavWriter::create(&temp_wav_path, spec)
                .map_err(|e| AppError::Model(format!("Failed to create temporary WAV file: {}", e)))?;
            for &sample in samples {
                writer.write_sample(sample)
                    .map_err(|e| AppError::Model(format!("Failed to write WAV sample: {}", e)))?;
            }
            writer.finalize()
                .map_err(|e| AppError::Model(format!("Failed to finalize WAV file: {}", e)))?;
        }

        // Run python to transcribe using NeMo
        let python_cmd = "import sys, nemo.collections.asr as nemo_asr; \
                          model = nemo_asr.ASRModel.restore_from(sys.argv[1]); \
                          res = model.transcribe([sys.argv[2]]); \
                          text = res[0] if isinstance(res, list) else res; \
                          print(text[0] if isinstance(text, list) else text)";

        let mut output = Command::new("python3")
            .arg("-c")
            .arg(python_cmd)
            .arg(&self.model_path)
            .arg(&temp_wav_path)
            .output();

        if output.is_err() {
            output = Command::new("python")
                .arg("-c")
                .arg(python_cmd)
                .arg(&self.model_path)
                .arg(&temp_wav_path)
                .output();
        }

        // Remove temporary WAV file
        let _ = std::fs::remove_file(&temp_wav_path);

        let output = output.map_err(|e| AppError::Model(format!(
            "Failed to execute Python process. Please ensure Python 3 and nemo_toolkit are installed and in your PATH. Error: {}", e
        )))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Model(format!(
                "Python NeMo transcription script exited with error: {}", stderr
            )));
        }

        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(result)
    }

    fn display_name(&self) -> &str {
        "NVIDIA NeMo (Python Sidecar)"
    }

    fn model_format(&self) -> ModelFormat {
        ModelFormat::Nemo
    }

    fn supports_language_hint(&self) -> bool {
        false
    }

    fn gpu_accelerated(&self) -> bool {
        true
    }
}
