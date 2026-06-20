# Eval Harness (Etap 0: H1 + H2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build an offline transcription evaluation harness (WER/CER/latency/RTF over a clip manifest) plus golden tests for the fragile model-detection code, so every later accuracy change is measurable and regressions are caught.

**Architecture:** A pure, dependency-free `eval` library module holds the metrics and report types (fully unit-tested without audio/models). A thin `bin/eval.rs` driver feeds the real `SttController::transcribe_with_progress` pipeline from a JSON manifest and writes a results JSON. A behavior-preserving refactor extracts ONNX layout detection into a pure, testable function; `factory::detect_format` gets golden tests.

**Tech Stack:** Rust (edition 2021), `hound` (WAV read, already a dep), `serde`/`serde_json` (already deps), `whisper-rs`/`sherpa-onnx` engines (already present), `tempfile` (new dev-dependency).

## Global Constraints

- **Zero new runtime dependencies.** Only one new `[dev-dependencies]` entry: `tempfile = "3"`. `strsim` is NOT added here (it belongs to D1).
- Rust edition 2021. Follow existing patterns in `stt/factory.rs`, `audio.rs`, `bin/test_whisper.rs`, `tests/long_audio.rs`.
- Comments: default to none; explain *why* in 1-2 lines only when needed. No emojis anywhere.
- Library crate name is `simplevoice_app_lib`; binaries link it (`use simplevoice_app_lib::...`).
- All unit tests must pass under default features (`default = ["candle", "onnx"]`): `cargo test`.
- The `eval` module is pure: no audio capture, no model loading, no I/O in metric functions.
- WAV inputs are asserted 16 kHz mono (matching `tests/long_audio.rs`).
- The only production-code change outside new files is a behavior-preserving extraction in `onnx_engine.rs`.
- Run all Rust commands from `src-tauri/`.

---

### Task 1: Eval metrics module

**Files:**
- Create: `src-tauri/src/eval.rs`
- Modify: `src-tauri/src/lib.rs` (add module declaration near the other top-level `mod` lines, lines 1-10)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `simplevoice_app_lib::eval::normalize(text: &str) -> Vec<String>`
  - `simplevoice_app_lib::eval::edit_distance<T: Eq>(a: &[T], b: &[T]) -> usize`
  - `simplevoice_app_lib::eval::word_error_rate(reference: &str, hypothesis: &str) -> f64`
  - `simplevoice_app_lib::eval::char_error_rate(reference: &str, hypothesis: &str) -> f64`
  - `simplevoice_app_lib::eval::mean(xs: &[f64]) -> f64`
  - `simplevoice_app_lib::eval::median(xs: &[f64]) -> f64`

- [ ] **Step 1: Create `src-tauri/src/eval.rs` with metric implementations + unit tests**

```rust
//! Offline evaluation metrics for the transcription harness (Etap 0 / H1).
//! Pure and dependency-free: no audio, no model loading. Unit-tested directly.

/// Lowercases (Unicode-aware), drops punctuation (keeping intra-word apostrophes
/// and hyphens), collapses whitespace, and splits into word tokens. Pure-punctuation
/// tokens are dropped so a stray "-" never counts as a word.
pub fn normalize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' || c == '\u{2019}' || c == '-' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .filter(|t| t.chars().any(|c| c.is_alphanumeric()))
        .map(|t| t.to_string())
        .collect()
}

/// Classic Levenshtein edit distance with two rolling rows (O(min) memory).
/// Generic so the same routine scores word slices and char slices.
pub fn edit_distance<T: Eq>(a: &[T], b: &[T]) -> usize {
    let n = b.len();
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];
    for (i, ai) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, bj) in b.iter().enumerate() {
            let cost = if ai == bj { 0 } else { 1 };
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Word Error Rate = word edits / reference word count. Empty reference yields
/// 0.0 against empty hypothesis, 1.0 otherwise.
pub fn word_error_rate(reference: &str, hypothesis: &str) -> f64 {
    let r = normalize(reference);
    let h = normalize(hypothesis);
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    edit_distance(&r, &h) as f64 / r.len() as f64
}

/// Character Error Rate over the normalized, space-joined text.
pub fn char_error_rate(reference: &str, hypothesis: &str) -> f64 {
    let r: Vec<char> = normalize(reference).join(" ").chars().collect();
    let h: Vec<char> = normalize(hypothesis).join(" ").chars().collect();
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    edit_distance(&r, &h) as f64 / r.len() as f64
}

pub fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

pub fn median(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_lowercases_strips_punctuation_and_collapses_space() {
        assert_eq!(normalize("Hello,  World!"), vec!["hello", "world"]);
        assert_eq!(normalize("  spaced   out  "), vec!["spaced", "out"]);
    }

    #[test]
    fn normalize_keeps_intra_word_apostrophe_and_hyphen() {
        assert_eq!(normalize("don't stop"), vec!["don't", "stop"]);
        assert_eq!(normalize("state-of-the-art"), vec!["state-of-the-art"]);
    }

    #[test]
    fn normalize_drops_standalone_punctuation_tokens() {
        assert_eq!(normalize("a - b"), vec!["a", "b"]);
    }

    #[test]
    fn normalize_preserves_unicode_diacritics_lowercased() {
        assert_eq!(normalize("Łódź ÖL"), vec!["łódź", "öl"]);
    }

    #[test]
    fn edit_distance_basic_cases() {
        assert_eq!(edit_distance::<u8>(&[], &[]), 0);
        assert_eq!(edit_distance(b"abc", b"abc"), 0);
        assert_eq!(edit_distance(b"abc", b"abd"), 1); // substitution
        assert_eq!(edit_distance(b"abc", b"abcd"), 1); // insertion
        assert_eq!(edit_distance(b"abc", b"ab"), 1); // deletion
        assert_eq!(edit_distance(b"", b"abc"), 3);
    }

    #[test]
    fn wer_is_edits_over_reference_words() {
        // 4 reference words, one substituted -> 0.25
        assert!((word_error_rate("the quick brown fox", "the quick green fox") - 0.25).abs() < 1e-9);
        assert_eq!(word_error_rate("same words here", "same words here"), 0.0);
    }

    #[test]
    fn wer_empty_reference_edge_cases() {
        assert_eq!(word_error_rate("", ""), 0.0);
        assert_eq!(word_error_rate("", "extra"), 1.0);
    }

    #[test]
    fn cer_counts_character_edits() {
        // "kitten" vs "sitting": 3 char edits over 6 reference chars.
        assert!((char_error_rate("kitten", "sitting") - 0.5).abs() < 1e-9);
    }

    #[test]
    fn mean_and_median() {
        assert_eq!(mean(&[1.0, 2.0, 3.0]), 2.0);
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), 2.5);
        assert_eq!(mean(&[]), 0.0);
        assert_eq!(median(&[]), 0.0);
    }
}
```

- [ ] **Step 2: Register the module in `src-tauri/src/lib.rs`**

Add this line next to the other top-level module declarations (after `mod error;`, before `pub mod stt;` at line 10):

```rust
pub mod eval;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test --lib eval::`
Expected: PASS — all 9 `eval::tests::*` pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/eval.rs src-tauri/src/lib.rs
git commit -m "feat(eval): pure WER/CER/edit-distance metrics module"
```

---

### Task 2: Eval report types, scoring, and table rendering

**Files:**
- Modify: `src-tauri/src/eval.rs` (append types + functions above the `#[cfg(test)]` block; add tests inside it)

**Interfaces:**
- Consumes: `word_error_rate`, `char_error_rate`, `mean`, `median` from Task 1.
- Produces:
  - `EvalClip { wav: String, reference: String, language: Option<String> }` (`Deserialize`)
  - `EvalManifest { clips: Vec<EvalClip> }` (`Deserialize`)
  - `ClipResult { name, wer, cer, audio_secs: f64, elapsed_ms: u128, rtf: f64 }` (`Serialize`)
  - `Aggregate { clips, mean_wer, median_wer, mean_cer, median_cer, median_latency_ms, median_rtf }` (`Serialize`)
  - `EvalReport { results: Vec<ClipResult>, aggregate: Aggregate }` (`Serialize`)
  - `score_clip(name: &str, reference: &str, hypothesis: &str, audio_secs: f64, elapsed: std::time::Duration) -> ClipResult`
  - `EvalReport::from_results(results: Vec<ClipResult>) -> EvalReport`
  - `EvalReport::render_table(&self) -> String`

- [ ] **Step 1: Append the report types and functions to `src-tauri/src/eval.rs`**

Insert immediately before the `#[cfg(test)] mod tests {` line:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct EvalClip {
    pub wav: String,
    pub reference: String,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvalManifest {
    pub clips: Vec<EvalClip>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipResult {
    pub name: String,
    pub wer: f64,
    pub cer: f64,
    pub audio_secs: f64,
    pub elapsed_ms: u128,
    pub rtf: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Aggregate {
    pub clips: usize,
    pub mean_wer: f64,
    pub median_wer: f64,
    pub mean_cer: f64,
    pub median_cer: f64,
    pub median_latency_ms: f64,
    pub median_rtf: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalReport {
    pub results: Vec<ClipResult>,
    pub aggregate: Aggregate,
}

/// Scores one clip: WER/CER against the reference plus latency and real-time
/// factor (processing time / audio duration).
pub fn score_clip(
    name: &str,
    reference: &str,
    hypothesis: &str,
    audio_secs: f64,
    elapsed: std::time::Duration,
) -> ClipResult {
    let elapsed_ms = elapsed.as_millis();
    let rtf = if audio_secs > 0.0 {
        (elapsed_ms as f64 / 1000.0) / audio_secs
    } else {
        0.0
    };
    ClipResult {
        name: name.to_string(),
        wer: word_error_rate(reference, hypothesis),
        cer: char_error_rate(reference, hypothesis),
        audio_secs,
        elapsed_ms,
        rtf,
    }
}

impl EvalReport {
    pub fn from_results(results: Vec<ClipResult>) -> Self {
        let wers: Vec<f64> = results.iter().map(|r| r.wer).collect();
        let cers: Vec<f64> = results.iter().map(|r| r.cer).collect();
        let lats: Vec<f64> = results.iter().map(|r| r.elapsed_ms as f64).collect();
        let rtfs: Vec<f64> = results.iter().map(|r| r.rtf).collect();
        let aggregate = Aggregate {
            clips: results.len(),
            mean_wer: mean(&wers),
            median_wer: median(&wers),
            mean_cer: mean(&cers),
            median_cer: median(&cers),
            median_latency_ms: median(&lats),
            median_rtf: median(&rtfs),
        };
        Self { results, aggregate }
    }

    pub fn render_table(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let a = &self.aggregate;
        let _ = writeln!(s, "clips:            {}", a.clips);
        let _ = writeln!(s, "WER     mean {:.3}   median {:.3}", a.mean_wer, a.median_wer);
        let _ = writeln!(s, "CER     mean {:.3}   median {:.3}", a.mean_cer, a.median_cer);
        let _ = writeln!(s, "latency median {:.0} ms", a.median_latency_ms);
        let _ = writeln!(s, "RTF     median {:.2}", a.median_rtf);
        s
    }
}
```

- [ ] **Step 2: Add tests inside the existing `#[cfg(test)] mod tests` block**

Append these tests after the existing ones in the `tests` module:

```rust
    #[test]
    fn score_clip_computes_rtf_and_metrics() {
        let r = score_clip(
            "clip1",
            "the quick brown fox",
            "the quick green fox",
            10.0,
            std::time::Duration::from_millis(5000),
        );
        assert_eq!(r.name, "clip1");
        assert!((r.wer - 0.25).abs() < 1e-9);
        assert_eq!(r.elapsed_ms, 5000);
        assert!((r.rtf - 0.5).abs() < 1e-9);
    }

    #[test]
    fn manifest_deserializes_with_optional_language() {
        let json = r#"{"clips":[
            {"wav":"a.wav","reference":"hello","language":"en"},
            {"wav":"b.wav","reference":"czesc"}
        ]}"#;
        let m: EvalManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.clips.len(), 2);
        assert_eq!(m.clips[0].language.as_deref(), Some("en"));
        assert_eq!(m.clips[1].language, None);
    }

    #[test]
    fn report_aggregates_and_serializes() {
        let results = vec![
            score_clip("a", "a b", "a b", 2.0, std::time::Duration::from_millis(1000)),
            score_clip("b", "a b", "a c", 2.0, std::time::Duration::from_millis(3000)),
        ];
        let report = EvalReport::from_results(results);
        assert_eq!(report.aggregate.clips, 2);
        assert!((report.aggregate.median_latency_ms - 2000.0).abs() < 1e-9);
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"median_wer\""));
    }
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test --lib eval::`
Expected: PASS — the 3 new tests plus the 9 from Task 1.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/eval.rs
git commit -m "feat(eval): manifest/report types, score_clip, table rendering"
```

---

### Task 3: Extract pure `detect_onnx_layout` (refactor + golden tests)

**Files:**
- Modify: `src-tauri/src/stt/onnx_engine.rs` (un-gate `find_file_with_keywords`, add `OnnxLayout` + `detect_onnx_layout`, rewrite the detection branch of `initialize`, add tests)
- Modify: `src-tauri/Cargo.toml` (add `[dev-dependencies] tempfile = "3"`)

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `OnnxLayout` enum: `Transducer { encoder: PathBuf, decoder: PathBuf, joiner: PathBuf }`, `MoonshineV1`, `MoonshineV2`, `Unsupported`
  - `detect_onnx_layout(dir: &Path) -> OnnxLayout` (available regardless of the `onnx` feature)

- [ ] **Step 1: Add the `tempfile` dev-dependency to `src-tauri/Cargo.toml`**

After the `[features]` block (before `[profile.dev]`), add:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Replace the top imports and feature-gated `find_file_with_keywords` in `onnx_engine.rs`**

Replace lines 1-28 (the current `use` lines and the `#[cfg(feature = "onnx")] fn find_file_with_keywords`) with:

```rust
use crate::error::AppError;
use crate::stt::traits::{AsrEngine, ModelFormat};

use std::path::{Path, PathBuf};

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
```

- [ ] **Step 3: Rewrite the detection branch inside `initialize` to consume `detect_onnx_layout`**

In `#[cfg(feature = "onnx")] impl OnnxEngine::initialize`, delete the old block that computed `encoder_opt`/`decoder_opt`/`joiner_opt`/`is_moonshine_v1`/`is_moonshine_v2`/`is_transducer_layout` and the following `if is_transducer_layout { ... } else if is_moonshine_v1 { ... } else if is_moonshine_v2 { ... } else { ... }` chain (currently lines ~64-126). Replace it with:

```rust
        match detect_onnx_layout(dir) {
            OnnxLayout::Transducer { encoder, decoder, joiner } => {
                println!("Initializing Transducer (Parakeet TDT) engine from: {}", dir.display());
                config.model_config.transducer = OfflineTransducerModelConfig {
                    encoder: Some(encoder.to_string_lossy().to_string()),
                    decoder: Some(decoder.to_string_lossy().to_string()),
                    joiner: Some(joiner.to_string_lossy().to_string()),
                };
                config.model_config.model_type = Some("nemo_transducer".to_string());
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
```

Note: the `let dir = path;`, the `dir.exists()` check, the `OfflineRecognizerConfig::default()` setup, the threads/tokens detection, and the final `OfflineRecognizer::create(&config)` all stay exactly as they are — only the detection/branch block in the middle is replaced.

- [ ] **Step 4: Add golden tests at the bottom of `onnx_engine.rs`**

Append (after the `#[cfg(not(feature = "onnx"))]` impls, at end of file):

```rust
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
    fn detects_moonshine_v1_and_v2() {
        let d1 = tempfile::tempdir().unwrap();
        touch(d1.path(), "preprocess.onnx");
        assert_eq!(detect_onnx_layout(d1.path()), OnnxLayout::MoonshineV1);

        let d2 = tempfile::tempdir().unwrap();
        touch(d2.path(), "merged_decoder.onnx");
        assert_eq!(detect_onnx_layout(d2.path()), OnnxLayout::MoonshineV2);
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
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib onnx_engine::`
Expected: PASS — 5 tests. Then confirm the refactor still compiles the engine: `cargo build --lib`
Expected: builds with no errors (default features include `onnx`).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/stt/onnx_engine.rs src-tauri/Cargo.toml
git commit -m "refactor(onnx): extract pure detect_onnx_layout + golden tests"
```

---

### Task 4: Golden tests for `factory::detect_format`

**Files:**
- Modify: `src-tauri/src/stt/factory.rs` (append a `#[cfg(test)] mod tests` at end of file)

**Interfaces:**
- Consumes: `AsrFactory::detect_format(path: &Path) -> Result<ModelFormat, AppError>` and `ModelFormat` (existing).
- Produces: nothing (test-only).

- [ ] **Step 1: Append golden tests to `src-tauri/src/stt/factory.rs`**

Add at the end of the file:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they pass (characterization of existing behavior)**

Run: `cargo test --lib factory::tests`
Expected: PASS — 6 tests pinning current `detect_format` behavior.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/stt/factory.rs
git commit -m "test(factory): golden tests for detect_format"
```

---

### Task 5: Fixtures README + example manifest

**Files:**
- Create: `src-tauri/tests/fixtures/README.md`
- Create: `src-tauri/tests/fixtures/manifest.example.json`

**Interfaces:**
- Consumes: the manifest schema (`EvalManifest`/`EvalClip`) from Task 2 — documentation only.
- Produces: nothing executable.

- [ ] **Step 1: Create `src-tauri/tests/fixtures/manifest.example.json`**

```json
{
  "clips": [
    { "wav": "clips/sample-en.wav", "reference": "the exact words spoken in this clip", "language": "en" },
    { "wav": "clips/sample-pl.wav", "reference": "dokladny tekst wypowiedziany w nagraniu", "language": "pl" },
    { "wav": "clips/sample-de.wav", "reference": "der genaue gesprochene text", "language": "de" }
  ]
}
```

- [ ] **Step 2: Create `src-tauri/tests/fixtures/README.md`**

````markdown
# Evaluation fixtures

The eval harness (`cargo run --bin eval`) measures WER/CER/latency against a
manifest of clips. Audio is data, not code: keep large WAV corpora out of git and
point the harness at a local manifest.

## Manifest schema

```json
{
  "clips": [
    { "wav": "clips/example.wav", "reference": "ground truth transcript", "language": "en" }
  ]
}
```

- `wav` — path to a 16 kHz, mono, 16-bit (or float) WAV, resolved **relative to the
  manifest file's directory**.
- `reference` — the exact human-verified transcript (the WER ground truth).
- `language` — optional ISO code passed to the engine; omit for auto-detect.

See `manifest.example.json` for a starting point.

## Running

```bash
cd src-tauri
SV_EVAL_MANIFEST=/path/to/manifest.json \
SIMPLEVOICE_MODEL=/path/to/ggml-model.bin \
cargo run --bin eval
```

Optional environment variables:

- `SV_EVAL_GPU=1` — load the model on GPU.
- `SV_EVAL_OUT=/path/to/results.json` — results location (default:
  `eval-results.json` next to the manifest).

## Assembling a corpus

A useful set covers several conditions:

- A clean-speech subset (e.g. a few LibriSpeech `test-clean` clips with their
  reference transcripts).
- A few noisy / accented / quiet clips (the conditions that expose audio-frontend
  regressions).
- At least one clip per non-English language you care about (e.g. `pl`, `de`).

Convert anything to 16 kHz mono first, for example:

```bash
ffmpeg -i input.wav -ar 16000 -ac 1 clips/output.wav
```
````

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/fixtures/README.md src-tauri/tests/fixtures/manifest.example.json
git commit -m "docs(eval): fixtures README + example manifest"
```

---

### Task 6: Harness binary `bin/eval.rs`

**Files:**
- Create: `src-tauri/src/bin/eval.rs`

**Interfaces:**
- Consumes: `simplevoice_app_lib::eval::{EvalManifest, EvalReport, score_clip}` (Tasks 1-2); `simplevoice_app_lib::stt::SttController` (existing); `hound`, `serde_json` (existing deps).
- Produces: the `eval` binary (`cargo run --bin eval`).

- [ ] **Step 1: Create `src-tauri/src/bin/eval.rs`**

```rust
//! Offline transcription evaluation harness (Etap 0 / H1).
//!
//! SV_EVAL_MANIFEST=/path/to/manifest.json \
//! SIMPLEVOICE_MODEL=/path/to/model \
//! cargo run --bin eval
//!
//! Optional: SV_EVAL_GPU=1, SV_EVAL_OUT=/path/to/results.json

use simplevoice_app_lib::eval::{score_clip, EvalManifest, EvalReport};
use simplevoice_app_lib::stt::SttController;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn main() {
    if let Err(e) = run() {
        eprintln!("eval error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let manifest_path = std::env::var("SV_EVAL_MANIFEST")
        .map_err(|_| "SV_EVAL_MANIFEST not set".to_string())?;
    let model = std::env::var("SIMPLEVOICE_MODEL")
        .map_err(|_| "SIMPLEVOICE_MODEL not set".to_string())?;
    let use_gpu = matches!(std::env::var("SV_EVAL_GPU").as_deref(), Ok("1") | Ok("true"));

    let manifest_path = PathBuf::from(&manifest_path);
    let base_dir = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let out_path = std::env::var("SV_EVAL_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| base_dir.join("eval-results.json"));

    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read manifest {}: {}", manifest_path.display(), e))?;
    let manifest: EvalManifest = serde_json::from_str(&manifest_text)
        .map_err(|e| format!("parse manifest: {}", e))?;

    let controller = SttController::new();
    controller.load_model(&model, use_gpu)?;

    println!("{:<32} {:>6} {:>6} {:>8} {:>9} {:>6}", "clip", "WER", "CER", "audio", "elapsed", "RTF");
    let mut results = Vec::new();
    for clip in &manifest.clips {
        let wav_path = base_dir.join(&clip.wav);
        let samples = match read_wav_16k_mono(&wav_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("skip {}: {}", clip.wav, e);
                continue;
            }
        };
        let audio_secs = samples.len() as f64 / 16_000.0;
        let started = Instant::now();
        let hyp = match controller.transcribe_with_progress(&samples, clip.language.as_deref(), &mut |_, _| {}) {
            Ok(c) => c.text,
            Err(e) => {
                eprintln!("skip {}: transcription failed: {}", clip.wav, e);
                continue;
            }
        };
        let elapsed = started.elapsed();
        let r = score_clip(&clip.wav, &clip.reference, &hyp, audio_secs, elapsed);
        println!(
            "{:<32} {:>6.3} {:>6.3} {:>7.2}s {:>7}ms {:>6.2}",
            r.name, r.wer, r.cer, r.audio_secs, r.elapsed_ms, r.rtf
        );
        results.push(r);
    }

    if results.is_empty() {
        return Err("no clip produced a result".to_string());
    }

    let report = EvalReport::from_results(results);
    println!("\n{}", report.render_table());

    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&out_path, json).map_err(|e| format!("write {}: {}", out_path.display(), e))?;
    println!("wrote {}", out_path.display());
    Ok(())
}

fn read_wav_16k_mono(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 {
        return Err(format!("expected 16 kHz, got {} Hz", spec.sample_rate));
    }
    if spec.channels != 1 {
        return Err(format!("expected mono, got {} channels", spec.channels));
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?,
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?,
    };
    Ok(samples)
}
```

- [ ] **Step 2: Verify the binary compiles**

Run: `cargo build --bin eval`
Expected: builds with no errors.

- [ ] **Step 3: Verify missing-env error path**

Run: `cargo run --bin eval` (with neither env var set)
Expected: prints `eval error: SV_EVAL_MANIFEST not set` and exits non-zero.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/bin/eval.rs
git commit -m "feat(eval): harness binary driving the real transcription pipeline"
```

---

### Task 7 (optional): env-gated engine smoke test

**Files:**
- Create: `src-tauri/tests/engine_smoke.rs`

**Interfaces:**
- Consumes: `simplevoice_app_lib::stt::SttController` (existing).
- Produces: nothing (ignored integration test).

- [ ] **Step 1: Create `src-tauri/tests/engine_smoke.rs`**

```rust
//! Smoke test: load a real model and transcribe a short clip, asserting non-empty
//! output. Ignored by default; needs local files passed via env.
//!
//! SV_TEST_MODEL=/path/to/ggml-tiny.bin \
//! SV_TEST_WAV=/path/to/short.wav \
//! cargo test --test engine_smoke -- --ignored --nocapture

use simplevoice_app_lib::stt::SttController;

#[test]
#[ignore = "needs SV_TEST_MODEL and SV_TEST_WAV pointing at local files"]
fn loads_model_and_produces_text() {
    let model = std::env::var("SV_TEST_MODEL").expect("SV_TEST_MODEL not set");
    let wav = std::env::var("SV_TEST_WAV").expect("SV_TEST_WAV not set");

    let mut reader = hound::WavReader::open(&wav).expect("open wav");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "test expects 16 kHz input");
    assert_eq!(spec.channels, 1, "test expects mono input");
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.expect("wav sample") as f32 / i16::MAX as f32)
        .collect();

    let controller = SttController::new();
    controller.load_model(&model, false).expect("load model");
    let text = controller.transcribe(&samples, None).expect("transcription");
    assert!(!text.trim().is_empty(), "transcription must not be empty");
}
```

- [ ] **Step 2: Verify it compiles and is ignored by default**

Run: `cargo test --test engine_smoke`
Expected: compiles; reports `0 passed; 0 failed; 1 ignored`.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/engine_smoke.rs
git commit -m "test(eval): env-gated engine smoke test"
```

---

## Final verification (whole stage)

- [ ] **Run the full unit suite**

Run: `cargo test`
Expected: all tests pass (eval metrics + report, onnx layout, factory detect_format, plus the existing suite); ignored integration tests stay ignored.

- [ ] **Produce the real WER/latency baseline**

Create a manifest (e.g. `/Users/woro/Documents/Simple/test/manifest.json`) pointing
at the user's clip with the user-supplied reference text:

```json
{ "clips": [ { "wav": "output.wav", "reference": "<USER-SUPPLIED GROUND TRUTH>", "language": "<code or omit>" } ] }
```

Run:

```bash
cd src-tauri
SV_EVAL_MANIFEST=/Users/woro/Documents/Simple/test/manifest.json \
SIMPLEVOICE_MODEL=/path/to/model \
cargo run --bin eval
```

Expected: a per-clip + aggregate WER/CER/latency/RTF table and a written
`eval-results.json`. This baseline is the gate for Etap 1.

- [ ] **Frontend check (no-op confirmation)**

Run: `pnpm lint`
Expected: passes (no frontend files changed).
