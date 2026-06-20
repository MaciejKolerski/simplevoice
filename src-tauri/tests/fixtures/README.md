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
