# Etap 0: Evaluation harness (WER/latency) + engine tests — design

Date: 2026-06-20
Status: approved by user

This is the first sub-project of the larger transcription-improvement program
described in `TRANSCRIPTION_IMPROVEMENTS.md` (49 items, A1–H5). The roadmap there
marks this stage (H1 + H2) as PRIORITY #1: every later accuracy change (A/B/D)
must be measurable, or it ships blind and regressions go unnoticed.

## Problem

There is no way to measure transcription quality or speed in the repo. No WER/CER
computation, no reference corpus, no engine benchmark, no golden tests. Concretely:

- Accuracy work (Whisper decoder params, resampler, VAD, custom dictionary) would
  be implemented blind — no proof of gain, no regression gate across
  languages/models.
- The most fragile, most-edited code has zero tests: `factory.rs::detect_format`
  (magic-byte sniff, directory-layout detection, `.part` handling) and the ONNX
  layout detection in `onnx_engine.rs` (transducer-vs-Moonshine ordering). A silent
  break there mis-detects a model with no failing test.

Existing test coverage is limited to pure logic: `chunker.rs`, `cloud.rs` helpers,
`streaming/`, and `stt/mod.rs` (chunked-transcription flow with a `FakeEngine`).
`tests/long_audio.rs` is an env-gated, `#[ignore]`d integration test that loads a
real model — the pattern this harness extends.

## Goals

- Offline harness that, given a manifest of clips (WAV + reference text), runs each
  clip through the real transcription pipeline and reports per-clip and aggregate
  **WER**, **CER**, **latency**, and **RTF** (real-time factor).
- The harness drives `SttController::transcribe_with_progress` — the same path the
  app uses (`prepare_samples` → `chunker::split_at_silences` → engine) — so it guards
  the whole pipeline against regression, not just an engine in isolation.
- A machine-readable results JSON per run, usable as a baseline for before/after
  comparison when implementing Etap 1+.
- Pure metric logic (normalize, WER, CER, edit distance, median/mean) lives in the
  library and is fully unit-tested **without any audio or model** — so `cargo test`
  is green in CI-style runs and the metrics themselves are trustworthy.
- Engine-detection tests (H2): `factory.rs::detect_format` and a newly-extracted
  pure `detect_onnx_layout` covering the fragile detection ordering.
- A first real WER/latency baseline produced this session on the user's clips
  (the user has reference-labeled clips).
- Zero new runtime dependencies. One new dev-dependency (`tempfile`).

## Non-goals

- No automatic LibriSpeech (or any corpus) download. Clips are data, not code; the
  user points the harness at their own manifest. A `fixtures/README.md` documents
  how to assemble one.
- No `--baseline` auto-comparison / regression-failing mode in this stage. The run
  writes a results JSON; comparing two runs is a trivial follow-up, deferred.
- No engine behavior changes. The only production code edit is a pure, behavior-
  preserving refactor in `onnx_engine.rs` to make detection testable.
- No new ML runtime, no CJK-specific CER modes, no frontend changes.

## Architecture

### 1. Metrics + report library module (`src-tauri/src/eval.rs`, new)

Always compiled, no new runtime deps. Registered with `pub mod eval;` in `lib.rs`
(top-level module list, alongside `pub mod stt;`). Two concerns:

**Metrics (pure):**

```rust
pub fn normalize(text: &str) -> Vec<String>;
pub fn edit_distance<T: Eq>(a: &[T], b: &[T]) -> usize; // classic DP
pub fn word_error_rate(reference: &str, hypothesis: &str) -> f64;
pub fn char_error_rate(reference: &str, hypothesis: &str) -> f64;
pub fn mean(xs: &[f64]) -> f64;
pub fn median(xs: &[f64]) -> f64;
```

- `normalize`: Unicode NFC, lowercase, strip punctuation (keep alphanumerics,
  intra-word `'` and `-`), collapse whitespace, split into tokens. This is the
  single most important correctness detail — WER is meaningless without it, and it
  is the most-tested function.
- `word_error_rate` = `edit_distance(normalize(ref), normalize(hyp)) / ref_word_count`,
  guarded for empty reference (`ref` empty + `hyp` empty → 0.0; `ref` empty +
  `hyp` non-empty → 1.0).
- `char_error_rate`: same, over `chars()` of the joined normalized text (useful for
  word-boundary-free languages and as a secondary signal).
- `edit_distance` is generic over `T: Eq` so the same DP serves words and chars.
  Two-row DP, O(min(a,b)) memory.

**Report (pure data + formatting):**

```rust
pub struct EvalClip   { pub wav: String, pub reference: String, pub language: Option<String> }
pub struct EvalManifest { pub clips: Vec<EvalClip> }
pub struct ClipResult { pub name: String, pub wer: f64, pub cer: f64,
                        pub audio_secs: f64, pub elapsed_ms: u128, pub rtf: f64 }
pub struct EvalReport { pub results: Vec<ClipResult>, /* aggregates */ }

pub fn score_clip(name, reference, hypothesis, audio_secs, elapsed) -> ClipResult;
```

`EvalClip`/`EvalManifest` derive `serde::Deserialize`; `ClipResult`/`EvalReport`
derive `serde::Serialize` (both already available). `score_clip` is pure and
unit-tested. `EvalReport` exposes a `render_table() -> String` (human stdout) and is
serialized to JSON for the results file.

### 2. Harness binary (`src-tauri/src/bin/eval.rs`, new)

Thin driver, modeled on `bin/test_whisper.rs` + `tests/long_audio.rs`. No logic that
isn't covered by unit tests in `eval.rs`. Inputs via env (matching existing
convention):

- `SV_EVAL_MANIFEST` — path to the manifest JSON (required).
- `SIMPLEVOICE_MODEL` — model path passed to `SttController::load_model` (required).
- `SV_EVAL_GPU` — `1`/`true` → `use_gpu = true` (default false).
- `SV_EVAL_OUT` — results JSON path (default: `eval-results.json` next to manifest).

Flow:

1. Parse manifest JSON. WAV paths are resolved relative to the manifest file's
   directory (portable fixtures).
2. `SttController::new()`, `load_model(model, use_gpu)`.
3. Per clip: read WAV with `hound` (assert 16 kHz mono, as `long_audio.rs` does;
   convert `i16`/`f32` samples to `f32` PCM), compute `audio_secs`; `Instant::now()`;
   `controller.transcribe_with_progress(&samples, clip.language.as_deref(), &mut noop)`;
   measure `elapsed`; `score_clip(...)`; collect.
4. Print `report.render_table()` to stdout; write `EvalReport` to `SV_EVAL_OUT`.

### 3. Testability refactor (`src-tauri/src/stt/onnx_engine.rs`)

Extract the layout-detection branch of `initialize()` into a pure function that does
not depend on the `onnx` feature (it is path logic only, no sherpa types):

```rust
pub enum OnnxLayout {
    Transducer { encoder: PathBuf, decoder: PathBuf, joiner: PathBuf },
    MoonshineV1,
    MoonshineV2,
    Unsupported,
}
pub fn detect_onnx_layout(dir: &Path) -> OnnxLayout;
```

`detect_onnx_layout` reproduces today's exact precedence and conditions:
transducer (encoder + decoder + (joiner | decoder-named-`joint`), and NOT a
Moonshine layout) wins; else `preprocess.onnx`/`preprocessor.onnx` → `MoonshineV1`;
else `merged_decoder.onnx` → `MoonshineV2`; else `Unsupported`. `find_file_with_keywords`
moves with it (or becomes non-feature-gated). The `#[cfg(feature = "onnx")]`
`initialize()` calls `detect_onnx_layout` and matches on the enum — same runtime
behavior, now testable without sherpa or a model. This is the only production-code
change, and it is behavior-preserving.

### 4. Engine-detection tests (H2)

- `factory.rs` (`#[cfg(test)] mod tests`): `detect_format` on
  - crafted `.bin` bytes: `[0,0,b'g',b'g']` → `GgmlBin`, `[b'G',b'G',0,0]` →
    `GgmlBin`, `[0,0,0,0]` → `Err` (bad magic);
  - extension routing: `.gguf` → `Gguf`, `.onnx` → `Onnx`, `.nemo` → `Nemo`;
  - synthetic dir trees (via `tempfile::tempdir`): `model.safetensors` →
    `HfSafetensors`, `pytorch_model.bin` → `HfPytorch`, a `*.onnx` file → `Onnx`,
    a `*.part` file → `Err("Incomplete download")`, empty dir → `Err`.
- `onnx_engine.rs` (`#[cfg(test)] mod tests`): `detect_onnx_layout` on synthetic
  dir trees — transducer (`encoder.onnx`+`decoder.onnx`+`joiner.onnx`) →
  `Transducer`; transducer files **plus** `preprocess.onnx` → `MoonshineV1` (the
  transducer guard requires NOT a Moonshine layout, so Moonshine wins when both
  coexist — this is the exact ordering the test must pin); `preprocess.onnx` only →
  `MoonshineV1`; `merged_decoder.onnx` → `MoonshineV2`; empty → `Unsupported`. Plus a
  `find_file_with_keywords` case.
- Optional env-gated smoke test (`#[ignore]`, like `long_audio.rs`): `SV_TEST_MODEL`
  → load ggml-tiny → transcribe a short fixture → assert non-empty output.

### 5. Fixtures + manifest (`src-tauri/tests/fixtures/`)

- `tests/fixtures/README.md`: manifest schema, the 16 kHz-mono WAV requirement, and
  guidance for assembling a corpus (LibriSpeech test-clean subset + a few
  noisy/accented/quiet clips + one pl + one de).
- `tests/fixtures/manifest.example.json`: the schema with placeholder entries.
- The user's real clips/manifest live outside the repo (or git-ignored) and are
  passed via `SV_EVAL_MANIFEST`; the repo only carries the schema + docs.

### 6. Dependencies (`src-tauri/Cargo.toml`)

- Add `[dev-dependencies] tempfile = "3"` (test-only, no runtime weight).
- No runtime dependencies added. `strsim` is intentionally **not** pulled in here —
  it belongs to D1 (custom-dictionary fuzzy matching); the harness uses its own
  small generic `edit_distance`.

## Error handling summary

| Failure | Behavior |
| --- | --- |
| `SV_EVAL_MANIFEST` / `SIMPLEVOICE_MODEL` unset | Clear error, exit non-zero |
| Manifest unparseable | Clear error, exit non-zero |
| Model load fails | Error surfaces (existing `load_model` message), exit non-zero |
| A clip's WAV missing/unreadable | Warn, skip that clip, continue |
| A clip not 16 kHz mono | Per-clip error recorded, skip, continue |
| No clip succeeded | Exit non-zero (nothing to baseline) |
| Reference empty / hypothesis empty | Defined WER (0.0 / 1.0 per the rules above) |

## Testing

- **Unit (Rust, `eval.rs`)**: `normalize` (casing, punctuation, whitespace, NFC,
  intra-word apostrophe/hyphen); `edit_distance` (empty, identical, substitution,
  insertion, deletion); `word_error_rate` / `char_error_rate` on known pairs incl.
  empty-reference edge cases; `median`/`mean`; `score_clip` field math (rtf,
  elapsed); `EvalManifest` deserialization + `EvalReport` serialization round-trip.
- **Unit (Rust, `factory.rs`)**: `detect_format` cases above.
- **Unit (Rust, `onnx_engine.rs`)**: `detect_onnx_layout` + `find_file_with_keywords`
  cases above.
- **`cargo test`** (default features) and **`cargo build --bin eval`** must pass.
- **Manual / real baseline (this session)**: with the user's clips,
  `SV_EVAL_MANIFEST=… SIMPLEVOICE_MODEL=… cargo run --bin eval` produces a WER/latency
  table and `eval-results.json`. This is the gate for Etap 1.
- `pnpm lint` unaffected (no frontend changes).

## Out-of-scope follow-ups (recorded, not planned)

- `--baseline <file>` mode that diffs two runs and fails on WER regression beyond a
  threshold (turns the harness into a CI gate).
- Bundling/scripting a LibriSpeech test-clean subset fetch.
- CER tokenization tuned per language family (CJK).
- Reusing `eval::edit_distance` from D1's fuzzy custom-word matcher (or swapping to
  `strsim` once D1 introduces it).
