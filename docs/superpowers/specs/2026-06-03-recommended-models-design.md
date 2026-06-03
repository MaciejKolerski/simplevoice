# Expand recommended local models — design

## Goal

The Models page (`Local` tab) currently advertises only **2** downloadable
recommended models (Parakeet TDT v3, Whisper Tiny). Grow this to a diverse,
production-ready set of **~11** so users have meaningful choice across speed,
accuracy, size, engine family, version, and language.

## Scope

- **Single file touched:** `src/views/ModelsView.tsx` — replace the
  `RECOMMENDED_MODELS` array only.
- **No logic changes.** Row rendering, the `download_model` flow, and
  `isModelDownloaded` already handle an arbitrary number of entries:
  - single-file models are de-duplicated by **filename**, so multiple Whisper
    entries sharing `ggerganov/whisper.cpp` are distinguished correctly;
  - multi-file models (Parakeet) are de-duplicated by **repo folder name**, so
    v2 and v3 are distinguished correctly.
- `format` is cosmetic (drives the badge via `FORMAT_LABELS`); the real format
  is re-detected by `scan_models` after download.

## The set (ordered lightest → heaviest)

All `repo_id`/filenames verified to exist on Hugging Face; sizes are exact
(from the HF tree API).

| # | Name | repo_id | files | format | size |
|---|------|---------|-------|--------|------|
| 1 | Whisper Tiny (GGML) | ggerganov/whisper.cpp | ggml-tiny.bin | ggml_bin | 74 MB |
| 2 | Whisper Tiny English (GGML) | ggerganov/whisper.cpp | ggml-tiny.en.bin | ggml_bin | 74 MB |
| 3 | Whisper Base (GGML) | ggerganov/whisper.cpp | ggml-base.bin | ggml_bin | 141 MB |
| 4 | Whisper Small (GGML) | ggerganov/whisper.cpp | ggml-small.bin | ggml_bin | 465 MB |
| 5 | Whisper Small English (GGML) | ggerganov/whisper.cpp | ggml-small.en.bin | ggml_bin | 465 MB |
| 6 | Parakeet TDT v2 (ONNX) | csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8 | encoder/decoder/joiner.int8.onnx, tokens.txt | onnx | 631 MB |
| 7 | Parakeet TDT v3 (ONNX) | csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8 | encoder/decoder/joiner.int8.onnx, tokens.txt | onnx | 639 MB |
| 8 | Whisper Medium (GGML) | ggerganov/whisper.cpp | ggml-medium.bin | ggml_bin | 1.4 GB |
| 9 | Whisper Large v3 Turbo (GGML) | ggerganov/whisper.cpp | ggml-large-v3-turbo.bin | ggml_bin | 1.5 GB |
| 10 | Whisper Large v2 (GGML) | ggerganov/whisper.cpp | ggml-large-v2.bin | ggml_bin | 2.9 GB |
| 11 | Whisper Large v3 (GGML) | ggerganov/whisper.cpp | ggml-large-v3.bin | ggml_bin | 2.9 GB |

Diversity axes: engine family (Whisper / NVIDIA Parakeet), size (74 MB → 2.9 GB),
version (large v1→v2→v3→turbo; Parakeet v2→v3), language (multilingual vs
English-only `.en`).

## Consistency fixes to the two pre-existing entries

- Whisper Tiny: `format` `gguf` → `ggml_bin` (badge becomes "GGML", matching the
  name and the other Whisper rows); size `75 MB` → `74 MB` (exact).
- Parakeet v3: size `600 MB` → `639 MB` (exact).

## Verification

- `pnpm lint` must pass.
- Manual: each row shows a correct badge/size; "Get" downloads succeed; once a
  file is present it moves out of "Available for Download".
