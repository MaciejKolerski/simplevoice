use std::path::Path;
use std::sync::Mutex;
use candle_core::{Device, Tensor, IndexOp, D};
use candle_nn::VarBuilder;
use candle_transformers::models::whisper::{Config, model::Whisper, audio};
use tokenizers::Tokenizer;
use crate::error::AppError;
use crate::stt::traits::{AsrEngine, ModelFormat};

const MEL_FILTERS_80: &[u8] = include_bytes!("melfilters.bytes");
const MEL_FILTERS_128: &[u8] = include_bytes!("melfilters128.bytes");

const LANGUAGES: [(&str, &str); 99] = [
    ("en", "english"),
    ("zh", "chinese"),
    ("de", "german"),
    ("es", "spanish"),
    ("ru", "russian"),
    ("ko", "korean"),
    ("fr", "french"),
    ("ja", "japanese"),
    ("pt", "portuguese"),
    ("tr", "turkish"),
    ("pl", "polish"),
    ("ca", "catalan"),
    ("nl", "dutch"),
    ("ar", "arabic"),
    ("sv", "swedish"),
    ("it", "italian"),
    ("id", "indonesian"),
    ("hi", "hindi"),
    ("fi", "finnish"),
    ("vi", "vietnamese"),
    ("he", "hebrew"),
    ("uk", "ukrainian"),
    ("el", "greek"),
    ("ms", "malay"),
    ("cs", "czech"),
    ("ro", "romanian"),
    ("da", "danish"),
    ("hu", "hungarian"),
    ("ta", "tamil"),
    ("no", "norwegian"),
    ("th", "thai"),
    ("ur", "urdu"),
    ("hr", "croatian"),
    ("bg", "bulgarian"),
    ("lt", "lithuanian"),
    ("la", "latin"),
    ("mi", "maori"),
    ("ml", "malayalam"),
    ("cy", "welsh"),
    ("sk", "slovak"),
    ("te", "telugu"),
    ("fa", "persian"),
    ("lv", "latvian"),
    ("bn", "bengali"),
    ("sr", "serbian"),
    ("az", "azerbaijani"),
    ("sl", "slovenian"),
    ("kn", "kannada"),
    ("et", "estonian"),
    ("mk", "macedonian"),
    ("br", "breton"),
    ("eu", "basque"),
    ("is", "icelandic"),
    ("hy", "armenian"),
    ("ne", "nepali"),
    ("mn", "mongolian"),
    ("bs", "bosnian"),
    ("kk", "kazakh"),
    ("sq", "albanian"),
    ("sw", "swahili"),
    ("gl", "galician"),
    ("sn", "shona"),
    ("yo", "yoruba"),
    ("so", "somali"),
    ("af", "afrikaans"),
    ("oc", "occitan"),
    ("ka", "georgian"),
    ("be", "belarusian"),
    ("tg", "tajik"),
    ("sd", "sindhi"),
    ("gu", "gujarati"),
    ("am", "amharic"),
    ("yi", "yiddish"),
    ("lo", "lao"),
    ("uz", "uzbek"),
    ("fo", "faroese"),
    ("ht", "haitian creole"),
    ("ps", "pashto"),
    ("tk", "turkmen"),
    ("nn", "nynorsk"),
    ("mt", "maltese"),
    ("sa", "sanskrit"),
    ("lb", "luxembourgish"),
    ("my", "myanmar"),
    ("bo", "tibetan"),
    ("tl", "tagalog"),
    ("mg", "malagasy"),
    ("as", "assamese"),
    ("tt", "tatar"),
    ("haw", "hawaiian"),
    ("ln", "lingala"),
    ("ha", "hausa"),
    ("ba", "bashkir"),
    ("jw", "javanese"),
    ("su", "sundanese"),
    ("pa", "punjabi"),
    ("mr", "marathi"),
    ("gu", "gujarati"),
    ("kn", "kannada"),
];

pub struct CandleWhisperEngine {
    model: Mutex<Whisper>,
    tokenizer: Tokenizer,
    device: Device,
    config: Config,
}

impl CandleWhisperEngine {
    pub fn initialize(model_dir: &Path, use_gpu: bool) -> Result<Self, AppError> {
        let device = super::get_device(use_gpu)
            .map_err(|e| AppError::Model(format!("Device error: {}", e)))?;

        let config_path = model_dir.join("config.json");
        let config_str = std::fs::read_to_string(config_path)
            .map_err(|e| AppError::Model(format!("Failed to read config.json: {}", e)))?;
        let config: Config = serde_json::from_str(&config_str)
            .map_err(|e| AppError::Model(format!("Failed to parse config.json: {}", e)))?;

        let tokenizer_path = model_dir.join("tokenizer.json");
        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| AppError::Model(format!("Failed to load tokenizer: {}", e)))?;

        // We support both model.safetensors and model.safetensors.index.json (multi-file weights)
        let weights_path = model_dir.join("model.safetensors");
        let vb = if weights_path.exists() {
            unsafe {
                VarBuilder::from_mmaped_safetensors(&[weights_path], candle_core::DType::F32, &device)
                    .map_err(|e| AppError::Model(format!("Failed to load weights: {}", e)))?
            }
        } else {
            // If model.safetensors doesn't exist, we might have multiple safetensors files.
            // Let's find all *.safetensors files in the directory.
            let mut safetensors_files = Vec::new();
            if let Ok(entries) = std::fs::read_dir(model_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().is_some_and(|ext| ext == "safetensors") {
                        safetensors_files.push(path);
                    }
                }
            }
            if safetensors_files.is_empty() {
                return Err(AppError::Model("No model.safetensors or safetensors files found".to_string()));
            }
            unsafe {
                VarBuilder::from_mmaped_safetensors(&safetensors_files, candle_core::DType::F32, &device)
                    .map_err(|e| AppError::Model(format!("Failed to load weights from multiple files: {}", e)))?
            }
        };

        let model = Whisper::load(&vb, config.clone())
            .map_err(|e| AppError::Model(format!("Failed to load whisper model: {}", e)))?;

        println!("[Candle Whisper] Model initialized successfully on device: {:?}", device);

        Ok(Self {
            model: Mutex::new(model),
            tokenizer,
            device,
            config,
        })
    }

    fn detect_language(&self, model: &mut Whisper, mel: &Tensor) -> Result<u32, AppError> {
        let (_bsize, _, seq_len) = mel.dims3().map_err(|e| AppError::Model(e.to_string()))?;
        let mel = mel.narrow(
            2,
            0,
            usize::min(seq_len, self.config.max_source_positions),
        ).map_err(|e| AppError::Model(e.to_string()))?;

        let language_token_ids = LANGUAGES
            .iter()
            .map(|(t, _)| {
                self.tokenizer.token_to_id(&format!("<|{t}|>"))
                    .ok_or_else(|| AppError::Model(format!("Language token <|{}|> not found in vocabulary", t)))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let sot_token = self.tokenizer.token_to_id("<|startoftranscript|>")
            .or_else(|| self.tokenizer.token_to_id("<|sot|>"))
            .ok_or_else(|| AppError::Model("Start of transcript token not found".to_string()))?;

        let audio_features = model.encoder.forward(&mel, true)
            .map_err(|e| AppError::Model(format!("Encoder forward failed during language detection: {}", e)))?;

        let tokens = Tensor::new(&[[sot_token]], &self.device)
            .map_err(|e| AppError::Model(e.to_string()))?;
        let language_token_ids_tensor = Tensor::new(language_token_ids.as_slice(), &self.device)
            .map_err(|e| AppError::Model(e.to_string()))?;

        let ys = model.decoder.forward(&tokens, &audio_features, true)
            .map_err(|e| AppError::Model(format!("Decoder forward failed during language detection: {}", e)))?;

        let logits = model.decoder.final_linear(&ys.i((..1, 0..1))?)
            .map_err(|e| AppError::Model(e.to_string()))?
            .i(0)?
            .i(0)?;

        let logits = logits.index_select(&language_token_ids_tensor, 0)
            .map_err(|e| AppError::Model(e.to_string()))?;

        let probs = candle_nn::ops::softmax(&logits, D::Minus1)
            .map_err(|e| AppError::Model(e.to_string()))?;

        let probs = probs.to_vec1::<f32>()
            .map_err(|e| AppError::Model(e.to_string()))?;

        let mut probs = LANGUAGES.iter().zip(probs.iter()).collect::<Vec<_>>();
        probs.sort_by(|(_, p1), (_, p2)| p2.total_cmp(p1));

        let detected_lang_code = probs[0].0 .0;
        let detected_token = self.tokenizer.token_to_id(&format!("<|{detected_lang_code}|>"))
            .ok_or_else(|| AppError::Model(format!("Token for detected language <|{}|> not found", detected_lang_code)))?;

        println!("Auto-detected language: {} ({}) with probability {:.4}", probs[0].0 .1, detected_lang_code, probs[0].1);
        Ok(detected_token)
    }

    fn decode_segment(&self, model: &mut Whisper, mel: &Tensor, language_token: Option<u32>) -> Result<String, AppError> {
        let audio_features = model.encoder.forward(mel, true)
            .map_err(|e| AppError::Model(format!("Encoder forward failed: {}", e)))?;
        
        let sample_len = self.config.max_target_positions / 2;
        
        let sot_token = self.tokenizer.token_to_id("<|startoftranscript|>")
            .or_else(|| self.tokenizer.token_to_id("<|sot|>"))
            .unwrap_or(50258);
        
        let transcribe_token = self.tokenizer.token_to_id("<|transcribe|>")
            .unwrap_or(50359);
            
        let eot_token = self.tokenizer.token_to_id("<|endoftext|>")
            .or_else(|| self.tokenizer.token_to_id("<|eot|>"))
            .unwrap_or(50257);
            
        let no_timestamps_token = self.tokenizer.token_to_id("<|notimestamps|>")
            .unwrap_or(50363);

        let mut tokens = vec![sot_token];
        if let Some(lang_tok) = language_token {
            tokens.push(lang_tok);
        }
        tokens.push(transcribe_token);
        tokens.push(no_timestamps_token);

        let loop_start = std::time::Instant::now();
        for i in 0..sample_len {
            let tokens_t = Tensor::new(tokens.as_slice(), &self.device)
                .map_err(|e| AppError::Model(e.to_string()))?;
            let tokens_t = tokens_t.unsqueeze(0)
                .map_err(|e| AppError::Model(e.to_string()))?;
            
            let ys = model.decoder.forward(&tokens_t, &audio_features, i == 0)
                .map_err(|e| AppError::Model(format!("Decoder forward failed: {}", e)))?;
            
            let (_, seq_len, _) = ys.dims3().map_err(|e| AppError::Model(e.to_string()))?;
            let logits = model.decoder.final_linear(&ys.i((..1, seq_len - 1..))?)?
                .i(0)?
                .i(0)?;
                
            let logits_v: Vec<f32> = logits.to_vec1()?;
            
            let next_token = logits_v
                .iter()
                .enumerate()
                .max_by(|(_, u), (_, v)| u.total_cmp(v))
                .map(|(idx, _)| idx as u32)
                .unwrap();
                
            tokens.push(next_token);
            if next_token == eot_token || tokens.len() > self.config.max_target_positions {
                break;
            }
        }
        println!("[Candle Whisper] decode_segment finished in {:.2?}", loop_start.elapsed());
        
        let text = self.tokenizer.decode(&tokens, true)
            .map_err(|e| AppError::Model(format!("Decoding failed: {}", e)))?;
        Ok(text)
    }
}

impl AsrEngine for CandleWhisperEngine {
    fn transcribe(
        &self,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<String, AppError> {
        let start_time = std::time::Instant::now();
        println!("[Candle Whisper] Starting transcription of {} samples ({:.2}s)...", samples.len(), samples.len() as f32 / 16000.0);
        
        let mel_bytes = match self.config.num_mel_bins {
            80 => MEL_FILTERS_80,
            128 => MEL_FILTERS_128,
            n => {
                println!("[Candle Whisper] Error: Unsupported num_mel_bins {}", n);
                return Err(AppError::Model(format!("Unsupported num_mel_bins: {}", n)));
            }
        };
        
        let mel_filters: Vec<f32> = mel_bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
            .collect();

        println!("[Candle Whisper] Computing Mel Spectrogram...");
        let mel = audio::pcm_to_mel(&self.config, samples, &mel_filters);
        
        let mel_len = mel.len();
        let mel_t = Tensor::from_vec(
            mel,
            (1, self.config.num_mel_bins, mel_len / self.config.num_mel_bins),
            &self.device,
        )?;

        let mut model = self.model.lock().unwrap();

        // Check if model is multilingual and handle language token selection
        let language_token = if self.config.vocab_size >= 51865 {
            match language {
                Some(lang) if lang != "auto" && !lang.trim().is_empty() => {
                    let token_name = format!("<|{}|>", lang);
                    let id = self.tokenizer.token_to_id(&token_name);
                    println!("[Candle Whisper] Using language: {} (token id: {:?})", lang, id);
                    id
                }
                _ => {
                    println!("[Candle Whisper] Auto-detecting language...");
                    let id = self.detect_language(&mut model, &mel_t).ok();
                    println!("[Candle Whisper] Auto-detection returned token id: {:?}", id);
                    id
                }
            }
        } else {
            println!("[Candle Whisper] Model is English-only, skipping language tokens.");
            None
        };

        // Slice mel spectrogram into 30-second segments (3000 frames) and decode
        let (_, _, content_frames) = mel_t.dims3().map_err(|e| AppError::Model(e.to_string()))?;
        let n_frames = 3000;
        let mut seek = 0;
        let mut full_text = String::new();
        let num_segments = (content_frames + n_frames - 1) / n_frames;
        let mut segment_idx = 0;

        println!("[Candle Whisper] Starting decoding loop ({} segments)...", num_segments);
        while seek < content_frames {
            segment_idx += 1;
            let segment_size = usize::min(content_frames - seek, n_frames);
            println!("[Candle Whisper] Decoding segment {}/{} (seek: {}, size: {})...", segment_idx, num_segments, seek, segment_size);
            
            let mel_segment = mel_t.narrow(2, seek, segment_size)
                .map_err(|e| AppError::Model(e.to_string()))?;
            
            let segment_text = self.decode_segment(&mut model, &mel_segment, language_token)?;
            println!("[Candle Whisper] Segment {} text: {:?}", segment_idx, segment_text);
            
            if !segment_text.trim().is_empty() {
                if !full_text.is_empty() {
                    full_text.push(' ');
                }
                full_text.push_str(segment_text.trim());
            }
            
            seek += segment_size;
        }

        println!("[Candle Whisper] Transcription completed in {:.2?}!", start_time.elapsed());
        Ok(full_text)
    }

    fn display_name(&self) -> &str {
        "Whisper (Candle)"
    }

    fn model_format(&self) -> ModelFormat {
        ModelFormat::HfSafetensors
    }

    fn supports_language_hint(&self) -> bool {
        self.config.vocab_size >= 51865
    }

    fn gpu_accelerated(&self) -> bool {
        !matches!(self.device, Device::Cpu)
    }
}
