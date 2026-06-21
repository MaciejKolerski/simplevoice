use std::path::Path;
use crate::error::AppError;
use crate::stt::traits::{AsrEngine, ModelFormat, ModelInfo};
use crate::stt::ggml_whisper::GgmlWhisperEngine;

pub struct AsrFactory;

impl AsrFactory {
    /// Detects the format and creates the corresponding engine.
    pub fn load(path: &Path, use_gpu: bool) -> Result<Box<dyn AsrEngine>, AppError> {
        let format = Self::detect_format(path)?;
        match format {
            ModelFormat::GgmlBin | ModelFormat::Gguf => {
                let engine = GgmlWhisperEngine::initialize(&path.to_string_lossy(), use_gpu)?;
                Ok(Box::new(engine))
            }
            ModelFormat::HfSafetensors | ModelFormat::HfPytorch => {
                #[cfg(feature = "candle")]
                {
                    let info = Self::detect(path, None)?;
                    let arch = info.architecture.as_deref().unwrap_or("");
                    if arch == "WhisperForConditionalGeneration" || arch == "Whisper" {
                        let engine = super::candle::whisper::CandleWhisperEngine::initialize(path, use_gpu)?;
                        Ok(Box::new(engine))
                    } else if is_ctc_arch(arch) {
                        let engine = super::candle::wav2vec::Wav2VecEngine::initialize(path, use_gpu)?;
                        Ok(Box::new(engine))
                    } else {
                        Err(AppError::Model(format!(
                            "Architecture {:?} not supported natively by Candle. Try converting to ONNX.",
                            info.architecture
                        )))
                    }
                }
                #[cfg(not(feature = "candle"))]
                {
                    Err(AppError::Model("Candle support is not compiled in. Build with --features candle.".to_string()))
                }
            }
            ModelFormat::Onnx => {
                let engine = super::onnx_engine::OnnxEngine::initialize(path, use_gpu)?;
                Ok(Box::new(engine))
            }
            ModelFormat::Nemo => Err(AppError::Model(
                "NeMo .nemo models are no longer supported on-device. Download a \
                 prebuilt ONNX model (e.g. Parakeet) from the model list instead."
                    .to_string(),
            )),
        }
    }

    /// Detects the model format without loading it into memory.
    pub fn detect_format(path: &Path) -> Result<ModelFormat, AppError> {
        if path.is_file() {
            match path.extension().and_then(|e| e.to_str()) {
                Some("gguf")  => return Ok(ModelFormat::Gguf),
                Some("onnx")  => return Ok(ModelFormat::Onnx),
                Some("nemo")  => return Ok(ModelFormat::Nemo),
                Some("bin")   => {
                    if let Ok(mut file) = std::fs::File::open(path) {
                        use std::io::Read;
                        let mut header = [0u8; 4];
                        if file.read_exact(&mut header).is_ok() {
                            if &header[2..4] == b"gg" || &header[0..2] == b"GG" {
                                return Ok(ModelFormat::GgmlBin);
                            }
                        }
                    }
                    return Err(AppError::Model("Invalid GGML model file (bad magic number)".to_string()));
                }
                _             => {}
            }
        }
        if path.is_dir() {
            // A directory still holding `.part` files is a download in progress
            // (or a paused/interrupted one). Treat it as not-yet-a-model so a
            // half-finished multi-file model is never listed as installed.
            let has_partial = std::fs::read_dir(path)
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .any(|e| e.path().extension().is_some_and(|ext| ext == "part"))
                })
                .unwrap_or(false);
            if has_partial {
                return Err(AppError::Model(format!(
                    "Incomplete download at: {}",
                    path.display()
                )));
            }
            if path.join("model.safetensors").exists()
                || path.join("model.safetensors.index.json").exists()
            {
                return Ok(ModelFormat::HfSafetensors);
            }
            if path.join("pytorch_model.bin").exists() {
                return Ok(ModelFormat::HfPytorch);
            }
            // Check for ONNX directories (Moonshine, etc.)
            let has_onnx = if let Ok(sub_entries) = std::fs::read_dir(path) {
                sub_entries
                    .filter_map(|e| e.ok())
                    .any(|e| e.path().extension().is_some_and(|ext| ext == "onnx"))
            } else {
                false
            };
            if has_onnx {
                return Ok(ModelFormat::Onnx);
            }
        }
        Err(AppError::Model(format!("Unrecognized model format at: {}", path.display())))
    }

    /// Reads model metadata without loading weights.
    pub fn detect(path: &Path, active_path: Option<&str>) -> Result<ModelInfo, AppError> {
        let format = Self::detect_format(path)?;
        let filename = path.file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("")
            .to_string();
        
        let path_str = path.to_string_lossy().to_string();
        let is_active = Some(path_str.as_str()) == active_path;

        let size_bytes = if path.is_file() {
            path.metadata()?.len()
        } else {
            // Directory: sum the size of all files
            let mut sum = 0;
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    if entry.path().is_file() {
                        sum += entry.metadata().map(|m| m.len()).unwrap_or(0);
                    }
                }
            }
            sum
        };

        let size_formatted = if size_bytes >= 1_073_741_824 {
            format!("{:.2} GB", size_bytes as f64 / 1_073_741_824.0)
        } else {
            format!("{:.0} MB", size_bytes as f64 / 1_048_576.0)
        };

        let mut architecture = None;
        let mut hf_model_id = None;
        let mut needs_conversion = false;

        if path.is_dir() {
            let config_path = path.join("config.json");
            if config_path.exists() {
                if let Ok(config_content) = std::fs::read_to_string(config_path) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&config_content) {
                        architecture = json["architectures"][0].as_str().map(|s| s.to_string());
                        hf_model_id = json["_name_or_path"].as_str().map(|s| s.to_string());
                    }
                }
            }

            if format == ModelFormat::HfSafetensors || format == ModelFormat::HfPytorch {
                let has_onnx = if let Ok(entries) = std::fs::read_dir(path) {
                    entries.flatten().any(|e| e.path().extension().is_some_and(|ext| ext == "onnx"))
                } else {
                    false
                };

                let is_natively_supported = match architecture.as_deref() {
                    Some("WhisperForConditionalGeneration") | Some("Whisper") => true,
                    Some(a) if is_ctc_arch(a) => true,
                    _ => false,
                };

                if !has_onnx && !is_natively_supported {
                    needs_conversion = true;
                }
            }
        }

        let filename_lower = filename.to_lowercase();
        let (display_name, quality_score, speed_score) = match format {
            ModelFormat::GgmlBin | ModelFormat::Gguf => {
                let (q, s, name) = if filename_lower.contains("large") || size_bytes > 2_000_000_000 {
                    (95, 40, "Whisper Large")
                } else if filename_lower.contains("medium") || size_bytes > 1_000_000_000 {
                    (85, 60, "Whisper Medium")
                } else if filename_lower.contains("small") || size_bytes > 400_000_000 {
                    (75, 80, "Whisper Small")
                } else if filename_lower.contains("base") || size_bytes > 140_000_000 {
                    (65, 90, "Whisper Base")
                } else {
                    (50, 98, "Whisper Tiny")
                };
                (format!("{} ({})", name, filename), q, s)
            }
            ModelFormat::HfSafetensors | ModelFormat::HfPytorch => {
                let name = hf_model_id.clone().unwrap_or_else(|| filename.clone());
                (format!("HF: {}", name), 85, 75)
            }
            ModelFormat::Onnx => {
                let (name, q, s) = if filename_lower.contains("moonshine") || path.join("preprocess.onnx").exists() {
                    ("Moonshine ASR", 90, 85)
                } else if filename_lower.contains("canary") {
                    ("NVIDIA Canary-Qwen", 94, 60)
                } else if filename_lower.contains("parakeet") || path.join("joiner.onnx").exists() {
                    ("NVIDIA Parakeet TDT", 88, 92)
                } else {
                    ("ONNX Model", 80, 70)
                };
                (format!("{} ({})", name, filename), q, s)
            }
            ModelFormat::Nemo => {
                (format!("NVIDIA NeMo ({})", filename), 90, 60)
            }
        };

        Ok(ModelInfo {
            path: path_str,
            format,
            architecture,
            hf_model_id,
            display_name,
            filename,
            size_bytes,
            size_formatted,
            quality_score,
            speed_score,
            is_active,
            needs_conversion,
        })
    }
}

fn is_ctc_arch(arch: &str) -> bool {
    matches!(arch,
        "Wav2Vec2ForCTC" | "HubertForCTC" | "UniSpeechSatForCTC" |
        "WavLMForCTC"    | "MCTCTForCTC"  | "SEWForCTC"
    )
}

#[cfg(test)]
mod tests {
    use super::AsrFactory;
    use crate::stt::traits::ModelFormat;
    use std::fs::{self, File};
    use std::io::Write;

    fn write_bytes(path: &std::path::Path, bytes: &[u8]) {
        let mut f = File::create(path).unwrap();
        f.write_all(bytes).unwrap();
    }

    #[test]
    fn bin_with_valid_ggml_magic_is_ggml() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("model.bin");
        write_bytes(&p, &[0, 0, b'g', b'g']);
        assert_eq!(AsrFactory::detect_format(&p).unwrap(), ModelFormat::GgmlBin);

        let p2 = d.path().join("model2.bin");
        write_bytes(&p2, &[b'G', b'G', 0, 0]);
        assert_eq!(AsrFactory::detect_format(&p2).unwrap(), ModelFormat::GgmlBin);
    }

    #[test]
    fn bin_with_bad_magic_is_error() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bad.bin");
        write_bytes(&p, &[0, 0, 0, 0]);
        assert!(AsrFactory::detect_format(&p).is_err());
    }

    #[test]
    fn extension_routing_for_single_files() {
        let d = tempfile::tempdir().unwrap();
        for (name, expected) in [
            ("m.gguf", ModelFormat::Gguf),
            ("m.onnx", ModelFormat::Onnx),
            ("m.nemo", ModelFormat::Nemo),
        ] {
            let p = d.path().join(name);
            File::create(&p).unwrap();
            assert_eq!(AsrFactory::detect_format(&p).unwrap(), expected, "for {}", name);
        }
    }

    #[test]
    fn directory_layouts_are_detected() {
        let d = tempfile::tempdir().unwrap();

        let safet = d.path().join("safet");
        fs::create_dir(&safet).unwrap();
        File::create(safet.join("model.safetensors")).unwrap();
        assert_eq!(AsrFactory::detect_format(&safet).unwrap(), ModelFormat::HfSafetensors);

        let pyt = d.path().join("pyt");
        fs::create_dir(&pyt).unwrap();
        File::create(pyt.join("pytorch_model.bin")).unwrap();
        assert_eq!(AsrFactory::detect_format(&pyt).unwrap(), ModelFormat::HfPytorch);

        let onnx = d.path().join("onnx");
        fs::create_dir(&onnx).unwrap();
        File::create(onnx.join("encoder.onnx")).unwrap();
        assert_eq!(AsrFactory::detect_format(&onnx).unwrap(), ModelFormat::Onnx);
    }

    #[test]
    fn partial_download_directory_is_error() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("dl");
        fs::create_dir(&dir).unwrap();
        File::create(dir.join("encoder.onnx.part")).unwrap();
        let err = AsrFactory::detect_format(&dir).unwrap_err();
        assert!(format!("{}", err).contains("Incomplete download"));
    }

    #[test]
    fn empty_directory_is_unrecognized() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("empty");
        fs::create_dir(&dir).unwrap();
        assert!(AsrFactory::detect_format(&dir).is_err());
    }
}
