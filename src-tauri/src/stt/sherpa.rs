use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig};
use std::path::Path;

pub struct SherpaEngine {
    recognizer: OfflineRecognizer,
}

impl SherpaEngine {
    pub fn new(model_dir: &str) -> Result<Self, String> {
        let dir = Path::new(model_dir);
        if !dir.exists() {
            return Err(format!("Model directory does not exist: {}", model_dir));
        }

        let mut config = OfflineRecognizerConfig::default();
        config.model_config.num_threads = 4;
        config.model_config.debug = false;

        // Auto-detect tokens.txt
        let tokens_path = dir.join("tokens.txt");
        if tokens_path.exists() {
            config.model_config.tokens = Some(tokens_path.to_string_lossy().to_string());
        } else {
            return Err("Missing tokens.txt file in model folder".to_string());
        }

        // Detect Moonshine (v1 or v2)
        let is_moonshine_v1 =
            dir.join("preprocess.onnx").exists() || dir.join("preprocessor.onnx").exists();
        let is_moonshine_v2 = dir.join("merged_decoder.onnx").exists();

        // Detect Transducer (Parakeet TDT)
        let is_transducer = dir.join("joiner.onnx").exists() || dir.join("join.onnx").exists();

        // Detect Canary
        let is_canary = dir.join("encoder.onnx").exists()
            && dir.join("decoder.onnx").exists()
            && !is_transducer
            && !is_moonshine_v1
            && !is_moonshine_v2;

        if is_moonshine_v1 {
            println!("Initializing Moonshine v1 engine from: {}", model_dir);
            let preprocess = if dir.join("preprocess.onnx").exists() {
                dir.join("preprocess.onnx")
            } else {
                dir.join("preprocessor.onnx")
            };
            let encode = dir.join("encode.onnx");
            let uncached_decoder = dir.join("uncached_decode.onnx");
            let cached_decoder = dir.join("cached_decode.onnx");

            if !encode.exists() || !uncached_decoder.exists() || !cached_decoder.exists() {
                return Err("Moonshine v1 model folder is missing encode.onnx, uncached_decode.onnx, or cached_decode.onnx".to_string());
            }

            config.model_config.moonshine.preprocessor =
                Some(preprocess.to_string_lossy().to_string());
            config.model_config.moonshine.encoder = Some(encode.to_string_lossy().to_string());
            config.model_config.moonshine.uncached_decoder =
                Some(uncached_decoder.to_string_lossy().to_string());
            config.model_config.moonshine.cached_decoder =
                Some(cached_decoder.to_string_lossy().to_string());
            config.model_config.model_type = Some("moonshine".to_string());
        } else if is_moonshine_v2 {
            println!("Initializing Moonshine v2 engine from: {}", model_dir);
            let encoder = if dir.join("encoder.onnx").exists() {
                dir.join("encoder.onnx")
            } else {
                dir.join("encode.onnx")
            };
            let merged_decoder = dir.join("merged_decoder.onnx");

            if !encoder.exists() || !merged_decoder.exists() {
                return Err(
                    "Moonshine v2 model folder is missing encoder.onnx or merged_decoder.onnx"
                        .to_string(),
                );
            }

            config.model_config.moonshine.encoder = Some(encoder.to_string_lossy().to_string());
            config.model_config.moonshine.merged_decoder =
                Some(merged_decoder.to_string_lossy().to_string());
            config.model_config.model_type = Some("moonshine".to_string());
        } else if is_transducer {
            println!(
                "Initializing Transducer (Parakeet TDT) engine from: {}",
                model_dir
            );
            let encoder = dir.join("encoder.onnx");
            let decoder = dir.join("decoder.onnx");
            let joiner = if dir.join("joiner.onnx").exists() {
                dir.join("joiner.onnx")
            } else {
                dir.join("join.onnx")
            };

            if !encoder.exists() || !decoder.exists() || !joiner.exists() {
                return Err(
                    "Transducer model folder is missing encoder.onnx, decoder.onnx, or joiner.onnx"
                        .to_string(),
                );
            }

            config.model_config.transducer.encoder = Some(encoder.to_string_lossy().to_string());
            config.model_config.transducer.decoder = Some(decoder.to_string_lossy().to_string());
            config.model_config.transducer.joiner = Some(joiner.to_string_lossy().to_string());
            config.model_config.model_type = Some("transducer".to_string());
        } else if is_canary {
            println!("Initializing Canary-Qwen engine from: {}", model_dir);
            let encoder = dir.join("encoder.onnx");
            let decoder = dir.join("decoder.onnx");

            config.model_config.canary.encoder = Some(encoder.to_string_lossy().to_string());
            config.model_config.canary.decoder = Some(decoder.to_string_lossy().to_string());
            config.model_config.canary.src_lang = Some("en".to_string());
            config.model_config.canary.tgt_lang = Some("en".to_string());
            config.model_config.canary.use_pnc = true;
            config.model_config.model_type = Some("canary".to_string());
        } else {
            return Err("Unsupported or unrecognized ONNX model directory structure. Ensure it contains the necessary encoder, decoder, joiner, or Moonshine ONNX files.".to_string());
        }

        let recognizer = OfflineRecognizer::create(&config)
            .ok_or_else(|| "Failed to create sherpa-onnx OfflineRecognizer. Check if model files are valid ONNX models.".to_string())?;

        Ok(Self { recognizer })
    }
}

impl super::EngineAdapter for SherpaEngine {
    fn initialize(&mut self, _model_path: &str) -> Result<(), String> {
        Ok(())
    }

    fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String, String> {
        let stream = self.recognizer.create_stream();

        if let Some(lang) = language {
            if !lang.is_empty() && lang != "auto" {
                // set_option is the standard way to set runtime parameters in sherpa-onnx
                stream.set_option("language", lang);
                stream.set_option("tgt_lang", lang);
            }
        }

        // sherpa-onnx expectations: 16kHz audio samples.

        stream.accept_waveform(16000, samples);

        self.recognizer.decode(&stream);

        let result = stream
            .get_result()
            .ok_or_else(|| "Failed to extract result from sherpa-onnx stream".to_string())?;

        Ok(result.text.trim().to_string())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        Ok(())
    }
}
