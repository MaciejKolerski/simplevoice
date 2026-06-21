# Transcription Improvement Program — live progress tracker

Source of requirements: `TRANSCRIPTION_IMPROVEMENTS.md` (repo root).
This file is the running status of all **52** items (the document has 52 distinct
items A1–H5; an earlier "49" count was an arithmetic slip). Updated as each
sub-project merges to `main`.

## User decisions (binding)
- **Autonomy:** implement ALL items across all stages, merging each sub-project to
  `main` without per-stage approval; user reviews at the end.
- **Python removal (C3/F5):** REMOVE the NeMo per-call sidecar and the on-device
  converter (`--trust-remote-code`); route `.nemo`/conversion-needing models to an
  actionable "download a prebuilt ONNX" error.
- **Platform code I cannot verify on macOS (Linux/Wayland/X11, Windows, ONNX GPU):**
  implement behind existing `cfg`, but mark **UNVERIFIED — user must confirm on that
  OS**, and list each under "Needs your verification" below. Linux/Windows-gated code
  does not even compile-check on this macOS host.

## Measurement caveat
The only eval clip (`test/output.wav`) is clean → the harness currently proves
**no regression** (baseline WER 0.000 / EXACT on 4 models), not accuracy *gains*.
Real A/B/D gains need harder fixtures (noisy/looping/accented) the user can add.

## Status legend
✅ done & merged · 🔜 next · ⏳ pending · 🚩 needs your verification/assets · ⏸ deferred

## Done (19 / 52)
_B5 and G3 are now fully done: ring-overflow counter (`note_ring_overflow`) and live-drop counter (`note_live_drop`) merged on top of the earlier downmix/coalesce halves. The `transcription-buffering` UI event stays for the frontend batch._
- ✅ **H1** offline eval harness (WER/CER/latency/RTF + hypothesis/exact-match)
- ✅ **H2** golden tests: `detect_format`, `detect_onnx_layout`, `find_file_with_keywords`, smoke test
- ✅ **A1** Whisper temperature-fallback (`best_of:2` + `set_temperature_inc(0.2)`)
- ✅ **A4** `set_no_context(true)`
- ✅ **A5** `set_suppress_nst(true)` + `stt::sanitize_output` (local + cloud)
- ✅ **D2-core** repetition/loop collapse (`stt::text::collapse_repeats`, local + cloud). _Filler-word lists (uh/um, per-language, tri-state) still pending — needs the config-threading batch (see D2-fillers below)._
- ✅ **C4** shared cloud `reqwest::Client` (OnceLock) with connect(10s)/read(120s) timeouts
- ✅ **H4** `save_wav_file` returns Err on write failure (not silent `Ok(None)`); emits `recording-save-failed`, transcription still proceeds. _Frontend toast pending → folds into E7._
- ✅ **B5-downmix** `downmix` keeps the trailing partial frame (was `chunks_exact`, silently dropped). _Ring-overflow signal (the other half of B5) still pending → folds into the H5 observability pass._
- ✅ **G2** bounded timed-join on streaming `finish()` (5s budget then detach) — no longer blocks shutdown / next recording.
- ✅ **C1** engine warm-up on record start (`SttController::take_engine_to_warm` + dummy decode off the start path; once per loaded model, no-op for cloud) — cuts first-dictation GPU/session-init latency.
- ✅ **C3 + F5** removed on-device Python: deleted `nemo_engine.rs` (per-call NeMo sidecar) and gutted `converter.rs` to a stub; `factory` routes `.nemo` to an actionable "download a prebuilt ONNX" error. _Frontend convert-button + i18n `convert`/`converting` keys still present (command stubbed so it doesn't break) → remove in the frontend batch._
- ✅ **B7** prefer native 16 kHz input config (`choose_input_config`, fallback to default) — resampler passthrough when device supports 16k. 🚩 _needs real-recording verification (capture path not exercised by the harness)._
- ✅ **A7** Parakeet transducer: `decoding_method="modified_beam_search"` + `max_active_paths=4`. Verified: Parakeet baseline stayed 0.000/EXACT, output segmentation changed (beam active).
- ✅ **G3-coalesce** worker drains the live-audio backlog into one decode + channel widened 16→64. 🚩 _coalescing not runtime-verified (no live mic here); drop-counter/event half of G3 → H5 pass._
- 🔶 **F4-timeout** download client gets `connect_timeout(15s)` (no total timeout — large files). _Retry/backoff loop (the other half of F4) still pending → needs real download testing._

## Planned sequencing of the remaining 39 (verifiable-first)

> **G5 reclassified → frontend batch.** The ASR language lives in frontend
> `localStorage` (`asr_language`), not `config.json`, so the backend cannot read it
> for the live session. G5 needs the frontend to also persist `asr_language` to
> `config.json` (then `begin_live_session` reads it like `live_min_chunk_ms`).
> Forcing `ui_language` would be wrong (user dictates pl, UI is en).

### Batch D-text (pure logic, fully macOS-verifiable, high value)
- ⏳ **D2-fillers** per-language filler-word removal (uh/um/eh…), config tri-state — needs `TranscribeOptions` config threading
- ⏳ **D4** OpenCC (zh-Hans/Hant) + optional sentence-casing (dep: `ferrous-opencc`)
- ⏳ **D5** voice formatting commands ("new line", "comma", per-language)
- ⏳ **D1** custom-dictionary fuzzy corrector (dep: `strsim`/`natural`)
- ⏳ **D4** OpenCC (zh-Hans/Hant) + optional sentence-casing (dep: `ferrous-opencc`)
- ⏳ **D5** voice formatting commands ("new line", "comma", per-language)
- ⏳ **D1** custom-dictionary fuzzy corrector (dep: `strsim`/`natural`)

### Batch A-accuracy (decoder/model, mostly verifiable)
- ⏳ **A3** custom dictionary as `initial_prompt` (Whisper) + ONNX hotwords  — pairs with D1
- ⏳ **A2** beam search (preset fast/accurate)
- ⏳ **A8** typed `DecodeParams` + Settings "Accuracy vs Speed"
- 🔶 **A7** ONNX decoding params — beam search ✅ done (verified); hotwords (with D1 dictionary) + EN-only language-routing gate still pending
- ⏳ **A6** Parakeet V3 recommended + calibrated metadata + fix `supports_language_hint` — pairs with F3

### Batch E-delivery (macOS-verifiable parts)
- ⏳ **E1** save/restore clipboard after auto-paste
- ⏳ **E7** surface silent paste failures (event → UI toast)
- ⏳ **E2** output mode (paste / clipboard-only / type) + paste-method
- ⏳ **E3** append trailing space + auto-submit (Enter)
- ⏳ **E6** paste delays / modifier-hold + configurable
- 🚩 **E4** X11 fallback (xdotool/ydotool) — UNVERIFIED (Linux)
- 🚩 **E5** Wayland fallback (240-char/GNOME) — UNVERIFIED (Linux)

### Batch C-perf + F-models (reliability)
- ✅ **C4** cloud: shared `reqwest::Client` + timeout
- ⏳ **C5** cloud: bounded chunk parallelism
- ✅ **C1** model warm-up on record start
- ⏳ **C2** push-to-talk mode
- ⏳ **C6** idle-unload model
- ✅ **C3** remove NeMo per-call sidecar (route to ONNX) — per decision
- ⏳ **F1** SHA-256 verification of downloads (dep: `sha2`)
- ⏳ **F2** atomic multi-file install + completeness manifest
- ⏳ **F3** curated backend model registry (`stt/registry.rs`) — pairs with A6
- 🔶 **F4** connect timeout ✅ done; retry/backoff loop pending (needs download testing)
- ✅ **F5** remove on-device converter (backend) — per decision; UI removal → frontend batch
- 🚩 **F6** ONNX GPU provider selector — UNVERIFIED (CoreML/DirectML/CUDA)

### Batch B-audio (input quality)
- ✅ **B5** downmix remainder + ring-overflow counter (warn-once)
- ⏳ **B8** DC-block + clipping detect + peak-aware normalize
- ⏳ **B1** rubato anti-aliasing resampler (dep: `rubato`)
- ⏳ **B3** pre-roll / look-back buffer
- ⏳ **B4** configurable VAD threshold/silence + lower consumer latency
- ✅ **B7** request/enumerate 16 kHz on device (🚩 needs real-recording check)
- 🚩 **B2** Silero VAD via sherpa-onnx — needs `silero_vad_v4.onnx` ASSET (user provides/OK to fetch)
- ⏳ **B6** chunker: VAD-driven cuts + overlap

### Batch G-streaming (live mode hardening)
- 🚩 **G5** thread user language into live session — needs frontend to persist `asr_language` to `config.json` (→ frontend batch; see note above)
- ✅ **G2** bounded timeout on `finish()`
- ⏳ **G1** committed-prefix trimming (fix O(n²))
- ✅ **G3** coalesce + live drop-counter (warn-once); `transcription-buffering` UI event → frontend batch
- ⏳ **G4** decouple ingest from decode (skip-stale)
- ⏳ **G7** configurable knobs + CJK character mode
- ⏸ **G6** native transducer streaming (sherpa OnlineRecognizer) — XL/High risk, last

### Batch H-foundation (observability)
- ✅ **H4** fix silent `save_wav_file` failure
- ✅ **H3** device-disconnect watchdog (consumer auto-stops after 5s of no audio + `err_fn` emits `recording-error`/`device_lost`) 🚩 needs real device-unplug check
- ⏳ **H5** structured logging (`tracing` + rotating file)

### Deferred (your input needed)
- ⏸ **D3** optional LLM cleanup + Apple Intelligence — XL, needs your API keys/provider; last, behind a flag

## Needs your verification / assets (running list)
_(filled as 🚩 items land)_
