use crate::error::AppError;
use crate::stt::traits::{AsrEngine, ModelFormat};

#[cfg(feature = "onnx")]
use std::path::Path;
#[cfg(feature = "onnx")]
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

#[cfg(feature = "onnx")]
fn find_file_with_keywords(dir: &Path, contains: &[&str], extension: &str) -> Option<std::path::PathBuf> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext.to_string_lossy().to_lowercase() == extension {
                        if let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_lowercase()) {
                            if contains.iter().all(|&kw| name.contains(kw)) {
                                return Some(path);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(feature = "onnx")]
pub struct OnnxEngine {
    recognizer: OfflineRecognizer,
    model_path: String,
}

#[cfg(feature = "onnx")]
impl OnnxEngine {
    pub fn initialize(path: &Path, _use_gpu: bool) -> Result<Self, AppError> {
        let dir = path;
        if !dir.exists() {
            return Err(AppError::Model(format!("Model path does not exist: {}", dir.display())));
        }

        let mut config = OfflineRecognizerConfig::default();
        let n_threads = (num_cpus::get() as i32).max(2) / 2;
        config.model_config.num_threads = n_threads;
        config.model_config.debug = false;

        // Auto-detect tokens/vocab file
        let tokens_path = if dir.join("tokens.txt").exists() {
            Some(dir.join("tokens.txt"))
        } else if dir.join("vocab.txt").exists() {
            Some(dir.join("vocab.txt"))
        } else {
            None
        };

        if let Some(ref p) = tokens_path {
            config.model_config.tokens = Some(p.to_string_lossy().to_string());
        } else {
            return Err(AppError::Model("Missing tokens.txt or vocab.txt in model folder".to_string()));
        }

        // Helper keyword matching
        let encoder_opt = find_file_with_keywords(dir, &["encoder"], "onnx")
            .or_else(|| find_file_with_keywords(dir, &["encode"], "onnx"));
        let decoder_opt = find_file_with_keywords(dir, &["decoder"], "onnx")
            .or_else(|| find_file_with_keywords(dir, &["decode"], "onnx"));
        let joiner_opt = find_file_with_keywords(dir, &["joiner"], "onnx")
            .or_else(|| find_file_with_keywords(dir, &["join"], "onnx"));

        // 2. Detect Moonshine
        let is_moonshine_v1 = dir.join("preprocess.onnx").exists() || dir.join("preprocessor.onnx").exists();
        let is_moonshine_v2 = dir.join("merged_decoder.onnx").exists();

        // 1. Detect Parakeet TDT / Transducer (our downloaded layout)
        let is_transducer_layout = !is_moonshine_v1 && !is_moonshine_v2
            && encoder_opt.is_some() 
            && decoder_opt.is_some() 
            && (joiner_opt.is_some() || decoder_opt.as_ref().map(|p| p.file_name().map(|n| n.to_string_lossy().contains("joint")).unwrap_or(false)).unwrap_or(false));

        if is_transducer_layout {
            let encoder = encoder_opt.unwrap();
            let decoder = decoder_opt.unwrap();
            let joiner = joiner_opt.unwrap_or_else(|| decoder.clone());

            println!("Initializing Transducer (Parakeet TDT) engine from: {}", dir.display());
            config.model_config.transducer = OfflineTransducerModelConfig {
                encoder: Some(encoder.to_string_lossy().to_string()),
                decoder: Some(decoder.to_string_lossy().to_string()),
                joiner: Some(joiner.to_string_lossy().to_string()),
            };
            config.model_config.model_type = Some("nemo_transducer".to_string());
        } else if is_moonshine_v1 {
            println!("Initializing Moonshine v1 engine from: {}", dir.display());
            let preprocess = if dir.join("preprocess.onnx").exists() {
                dir.join("preprocess.onnx")
            } else {
                dir.join("preprocessor.onnx")
            };
            let encode = dir.join("encode.onnx");
            let uncached_decoder = dir.join("uncached_decode.onnx");
            let cached_decoder = dir.join("cached_decode.onnx");

            config.model_config.moonshine.preprocessor = Some(preprocess.to_string_lossy().to_string());
            config.model_config.moonshine.encoder = Some(encode.to_string_lossy().to_string());
            config.model_config.moonshine.uncached_decoder = Some(uncached_decoder.to_string_lossy().to_string());
            config.model_config.moonshine.cached_decoder = Some(cached_decoder.to_string_lossy().to_string());
            config.model_config.model_type = Some("moonshine".to_string());
        } else if is_moonshine_v2 {
            println!("Initializing Moonshine v2 engine from: {}", dir.display());
            let encoder = if dir.join("encoder.onnx").exists() {
                dir.join("encoder.onnx")
            } else {
                dir.join("encode.onnx")
            };
            let merged_decoder = dir.join("merged_decoder.onnx");

            config.model_config.moonshine.encoder = Some(encoder.to_string_lossy().to_string());
            config.model_config.moonshine.merged_decoder = Some(merged_decoder.to_string_lossy().to_string());
            config.model_config.model_type = Some("moonshine".to_string());
        } else {
            return Err(AppError::Model(
                "Unsupported or unrecognized ONNX model directory structure. Ensure it contains the necessary encoder, decoder, joiner, or Moonshine ONNX files.".to_string()
            ));
        }

        let recognizer = OfflineRecognizer::create(&config)
            .ok_or_else(|| AppError::Model("Failed to create sherpa-onnx OfflineRecognizer. Check if model files are valid ONNX models.".to_string()))?;

        Ok(Self {
            recognizer,
            model_path: dir.to_string_lossy().to_string(),
        })
    }
}

#[cfg(feature = "onnx")]
impl AsrEngine for OnnxEngine {
    fn transcribe(
        &self,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<String, AppError> {
        let stream = self.recognizer.create_stream();

        if let Some(lang) = language {
            if !lang.is_empty() && lang != "auto" {
                stream.set_option("language", lang);
                stream.set_option("tgt_lang", lang);
            }
        }

        // sherpa-onnx expects 16kHz audio samples.
        stream.accept_waveform(16000, samples);

        self.recognizer.decode(&stream);

        let result = stream.get_result()
            .ok_or_else(|| AppError::Model("Failed to extract result from sherpa-onnx stream".to_string()))?;

        Ok(result.text.trim().to_string())
    }

    fn display_name(&self) -> &str {
        &self.model_path
    }

    fn model_format(&self) -> ModelFormat {
        ModelFormat::Onnx
    }

    fn supports_language_hint(&self) -> bool {
        true
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
