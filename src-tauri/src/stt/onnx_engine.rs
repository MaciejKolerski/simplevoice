use crate::error::AppError;
use crate::stt::traits::{AsrEngine, ModelFormat};

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Per-transcription hotwords (one phrase per line) that bias the ONNX
/// transducer decoder toward the user's custom-dictionary phrases (A7/A3). Set
/// from the delivery layer before each transcribe; read in `transcribe` to build
/// a hotword-aware stream. Lives outside the `onnx` feature gate so the setter
/// compiles even when ONNX is off.
pub(crate) static ONNX_HOTWORDS: Mutex<String> = Mutex::new(String::new());

/// Set the ONNX transducer hotwords (one phrase per line). Public so the eval
/// harness can exercise contextual biasing; the app sets it from the delivery
/// layer per transcription.
pub fn set_onnx_hotwords(hotwords: String) {
    if let Ok(mut g) = ONNX_HOTWORDS.lock() {
        *g = hotwords;
    }
}

#[cfg(feature = "onnx")]
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

/// Pure layout detection for a downloaded ONNX model directory. Lives outside the
/// `onnx` feature gate (path logic only) so the fragile transducer-vs-Moonshine
/// precedence can be unit-tested without sherpa or a real model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnnxLayout {
    Transducer {
        encoder: PathBuf,
        decoder: PathBuf,
        joiner: PathBuf,
    },
    MoonshineV1,
    MoonshineV2,
    Unsupported,
}

fn find_file_with_keywords(dir: &Path, contains: &[&str], extension: &str) -> Option<PathBuf> {
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

/// Derive the text `bpe.vocab` sherpa-onnx needs for hotword tokenization from a
/// SentencePiece `bpe.model` (a protobuf). Models ship the binary `bpe.model` but
/// not the `<piece> <score>`-per-line vocab; this minimal proto reader extracts
/// each `SentencePiece { piece = 1 (string), score = 2 (float) }` from the
/// top-level `ModelProto { pieces = 1 (repeated) }`. Returns the vocab text, or
/// None on a malformed proto. No external deps (avoids re-introducing Python).
fn bpe_vocab_from_model(model_bytes: &[u8]) -> Option<String> {
    fn read_varint(b: &[u8], i: &mut usize) -> Option<u64> {
        let mut result: u64 = 0;
        let mut shift = 0u32;
        loop {
            let byte = *b.get(*i)?;
            *i += 1;
            result |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Some(result);
            }
            shift += 7;
            if shift >= 64 {
                return None;
            }
        }
    }
    fn skip(b: &[u8], i: &mut usize, wire: u64) -> Option<()> {
        match wire {
            0 => {
                read_varint(b, i)?;
            }
            1 => *i += 8,
            2 => {
                let len = read_varint(b, i)? as usize;
                *i += len;
            }
            5 => *i += 4,
            _ => return None,
        }
        if *i > b.len() {
            None
        } else {
            Some(())
        }
    }

    let mut out = String::new();
    let mut i = 0;
    while i < model_bytes.len() {
        let tag = read_varint(model_bytes, &mut i)?;
        let (field, wire) = (tag >> 3, tag & 0x7);
        if field == 1 && wire == 2 {
            // A `SentencePiece` submessage.
            let len = read_varint(model_bytes, &mut i)? as usize;
            let end = i.checked_add(len)?;
            if end > model_bytes.len() {
                return None;
            }
            let mut piece: Option<String> = None;
            let mut score: f32 = 0.0;
            while i < end {
                let t = read_varint(model_bytes, &mut i)?;
                match (t >> 3, t & 0x7) {
                    (1, 2) => {
                        let l = read_varint(model_bytes, &mut i)? as usize;
                        let s = model_bytes.get(i..i.checked_add(l)?)?;
                        piece = Some(String::from_utf8_lossy(s).into_owned());
                        i += l;
                    }
                    (2, 5) => {
                        let b = model_bytes.get(i..i + 4)?;
                        score = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                        i += 4;
                    }
                    (_, w) => skip(model_bytes, &mut i, w)?,
                }
            }
            if let Some(p) = piece {
                out.push_str(&p);
                out.push(' ');
                out.push_str(&score.to_string());
                out.push('\n');
            }
        } else {
            skip(model_bytes, &mut i, wire)?;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Reproduces the exact precedence used by `OnnxEngine::initialize`: a transducer
/// layout (encoder + decoder + joiner-or-`joint` decoder) wins only when the
/// directory is NOT a Moonshine layout; otherwise Moonshine v1 (preprocess) then
/// Moonshine v2 (merged_decoder); otherwise unsupported.
pub fn detect_onnx_layout(dir: &Path) -> OnnxLayout {
    let encoder_opt = find_file_with_keywords(dir, &["encoder"], "onnx")
        .or_else(|| find_file_with_keywords(dir, &["encode"], "onnx"));
    let decoder_opt = find_file_with_keywords(dir, &["decoder"], "onnx")
        .or_else(|| find_file_with_keywords(dir, &["decode"], "onnx"));
    let joiner_opt = find_file_with_keywords(dir, &["joiner"], "onnx")
        .or_else(|| find_file_with_keywords(dir, &["join"], "onnx"));

    let is_moonshine_v1 = dir.join("preprocess.onnx").exists() || dir.join("preprocessor.onnx").exists();
    let is_moonshine_v2 = dir.join("merged_decoder.onnx").exists();

    let is_transducer = !is_moonshine_v1
        && !is_moonshine_v2
        && encoder_opt.is_some()
        && decoder_opt.is_some()
        && (joiner_opt.is_some()
            || decoder_opt
                .as_ref()
                .map(|p| p.file_name().map(|n| n.to_string_lossy().contains("joint")).unwrap_or(false))
                .unwrap_or(false));

    if is_transducer {
        let encoder = encoder_opt.unwrap();
        let decoder = decoder_opt.unwrap();
        let joiner = joiner_opt.unwrap_or_else(|| decoder.clone());
        OnnxLayout::Transducer { encoder, decoder, joiner }
    } else if is_moonshine_v1 {
        OnnxLayout::MoonshineV1
    } else if is_moonshine_v2 {
        OnnxLayout::MoonshineV2
    } else {
        OnnxLayout::Unsupported
    }
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

        match detect_onnx_layout(dir) {
            OnnxLayout::Transducer { encoder, decoder, joiner } => {
                println!("Initializing ONNX transducer engine from: {}", dir.display());
                config.model_config.transducer = OfflineTransducerModelConfig {
                    encoder: Some(encoder.to_string_lossy().to_string()),
                    decoder: Some(decoder.to_string_lossy().to_string()),
                    joiner: Some(joiner.to_string_lossy().to_string()),
                };
                // A7: modified beam search over the transducer lattice (vs default
                // greedy) recovers accuracy on harder audio at a small cost.
                config.decoding_method = Some("modified_beam_search".to_string());
                config.max_active_paths = 4;
                // A7/A3: contextual biasing. hotwords_score boosts phrases passed
                // per-stream via create_stream_with_hotwords (only used when the
                // custom dictionary is non-empty); requires modified_beam_search,
                // set above. 0 phrases => no effect.
                // Boost applied to hotword token paths. `SV_HOTWORDS_SCORE` lets
                // the eval harness / power users tune it; 2.0 is sherpa's typical
                // default and biases rare/OOV terms without over-triggering.
                config.hotwords_score = std::env::var("SV_HOTWORDS_SCORE")
                    .ok()
                    .and_then(|s| s.parse::<f32>().ok())
                    .unwrap_or(2.0);
                // Two transducer families need different handling:
                //  - k2/icefall BPE zipformers ship a SentencePiece `bpe.model`.
                //    sherpa needs the TEXT `bpe.vocab` to ENCODE hotwords, so derive
                //    it once from bpe.model and cache it, then enable BPE modeling.
                //  - NeMo Parakeet ships neither; it needs model_type=nemo_transducer
                //    (and can't encode hotwords — sherpa logs "Encode hotwords failed,
                //    skipping"). Forcing nemo_transducer on a k2 model fails with
                //    "'vocab_size' does not exist in the metadata".
                let bpe_vocab = dir.join("bpe.vocab");
                let bpe_model = dir.join("bpe.model");
                if !bpe_vocab.exists() && bpe_model.exists() {
                    if let Ok(bytes) = std::fs::read(&bpe_model) {
                        if let Some(vocab) = bpe_vocab_from_model(&bytes) {
                            let _ = std::fs::write(&bpe_vocab, vocab);
                        }
                    }
                }
                if bpe_vocab.exists() {
                    config.model_config.modeling_unit = Some("bpe".to_string());
                    config.model_config.bpe_vocab = Some(bpe_vocab.to_string_lossy().to_string());
                } else {
                    config.model_config.model_type = Some("nemo_transducer".to_string());
                }
            }
            OnnxLayout::MoonshineV1 => {
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
            }
            OnnxLayout::MoonshineV2 => {
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
            }
            OnnxLayout::Unsupported => {
                return Err(AppError::Model(
                    "Unsupported or unrecognized ONNX model directory structure. Ensure it contains the necessary encoder, decoder, joiner, or Moonshine ONNX files.".to_string()
                ));
            }
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
        // A7/A3: bias toward the custom-dictionary phrases when present.
        let hotwords = ONNX_HOTWORDS
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let stream = if hotwords.trim().is_empty() {
            self.recognizer.create_stream()
        } else {
            self.recognizer.create_stream_with_hotwords(&hotwords)
        };

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

#[cfg(test)]
mod tests {
    use super::{detect_onnx_layout, find_file_with_keywords, OnnxLayout};
    use std::fs::File;

    fn touch(dir: &std::path::Path, name: &str) {
        File::create(dir.join(name)).unwrap();
    }

    #[test]
    fn detects_transducer_layout() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "encoder.onnx");
        touch(d.path(), "decoder.onnx");
        touch(d.path(), "joiner.onnx");
        match detect_onnx_layout(d.path()) {
            OnnxLayout::Transducer { encoder, decoder, joiner } => {
                assert!(encoder.ends_with("encoder.onnx"));
                assert!(decoder.ends_with("decoder.onnx"));
                assert!(joiner.ends_with("joiner.onnx"));
            }
            other => panic!("expected Transducer, got {:?}", other),
        }
    }

    #[test]
    fn moonshine_wins_over_transducer_when_preprocess_present() {
        // The transducer guard requires NOT a Moonshine layout, so a directory with
        // both transducer files and preprocess.onnx must resolve to MoonshineV1.
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "encoder.onnx");
        touch(d.path(), "decoder.onnx");
        touch(d.path(), "joiner.onnx");
        touch(d.path(), "preprocess.onnx");
        assert_eq!(detect_onnx_layout(d.path()), OnnxLayout::MoonshineV1);
    }

    #[test]
    fn detects_moonshine_v1() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "preprocess.onnx");
        assert_eq!(detect_onnx_layout(d.path()), OnnxLayout::MoonshineV1);
    }

    #[test]
    fn detects_moonshine_v2() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "merged_decoder.onnx");
        assert_eq!(detect_onnx_layout(d.path()), OnnxLayout::MoonshineV2);
    }

    #[test]
    fn empty_dir_is_unsupported() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(detect_onnx_layout(d.path()), OnnxLayout::Unsupported);
    }

    #[test]
    fn find_file_with_keywords_matches_extension_and_all_keywords() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "model.encoder.int8.onnx");
        touch(d.path(), "notes.txt");
        assert!(find_file_with_keywords(d.path(), &["encoder"], "onnx").is_some());
        assert!(find_file_with_keywords(d.path(), &["decoder"], "onnx").is_none());
    }

    #[test]
    fn bpe_vocab_from_model_extracts_piece_and_score() {
        // ModelProto { pieces: [ SentencePiece { piece: "▁the", score: -1.5 } ] }.
        // submessage: field 1 (piece, len-delim) "▁the" = E2 96 81 74 68 65;
        //             field 2 (score, fixed32) -1.5 = 00 00 C0 BF (f32 LE).
        let sub: Vec<u8> = vec![
            0x0A, 0x06, 0xE2, 0x96, 0x81, 0x74, 0x68, 0x65, 0x15, 0x00, 0x00, 0xC0, 0xBF,
        ];
        let mut proto = vec![0x0A, sub.len() as u8];
        proto.extend_from_slice(&sub);
        assert_eq!(
            super::bpe_vocab_from_model(&proto).as_deref(),
            Some("▁the -1.5\n")
        );
        // Malformed (truncated) input must not panic.
        assert!(super::bpe_vocab_from_model(&[0x0A, 0x06, 0xE2]).is_none());
    }
}
