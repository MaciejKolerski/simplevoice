# Etap 1a: Whisper decoder resilience (A1, A4, A5) — design

Date: 2026-06-20
Status: approved by user

First sub-project of Etap 1 (core accuracy) from the transcription-improvement
program (`TRANSCRIPTION_IMPROVEMENTS.md`). Etap 1 is split into three sub-projects:
**1a (this one)** = the trivial decoder-resilience setters A1/A4/A5; 1b = custom
dictionary (A3/D1); 1c = beam search + typed DecodeParams + UI (A2/A8). Each is
gated by re-running the Etap 0 eval harness.

## Problem

The Whisper engine (`src-tauri/src/stt/ggml_whisper.rs:61-84`) strips out
whisper.cpp's standard decoding resilience:

- `SamplingStrategy::Greedy { best_of: 1 }` + `set_temperature(0.0)` with **no
  `temperature_inc`** — the temperature-fallback ladder is disabled. On a low
  avg-logprob / high compression-ratio (loop) / no-speech result, reference
  whisper.cpp retries decoding at temperature 0.2, 0.4 … 1.0. SimpleVoice never
  retries, so on hard audio it locks onto the first (often looped/hallucinated)
  hypothesis.
- `set_no_context` is never called (defaults `false`), so with the app's own
  chunker each `state.full` carries the previous chunk's text as context — a bad
  trailing hallucination in one chunk poisons the next.
- `set_suppress_nst(false)` (`ggml_whisper.rs:75`) lets non-speech tokens
  (`[BLANK_AUDIO]`, `(music)`, `[ Silence ]`) reach the output, and there is **no
  output sanitization anywhere** to remove leftovers.

## Goals

- A1: enable the temperature-fallback ladder.
- A4: set `no_context` on the (chunked) decode path.
- A5: suppress non-speech tokens AND add a conservative output sanitizer for any
  leftover markers, applied on both the local and the cloud transcription paths.
- No WER regression on the existing Etap 0 baseline: the 4 installed models must
  stay at WER 0.000 / EXACT on `test/output.wav`.
- Zero new dependencies. The sanitizer is pure and unit-tested.

## Non-goals

- Beam search (A2), typed `DecodeParams` + "Accuracy vs Speed" UI (A8) — that is
  sub-project 1c.
- Custom dictionary / `initial_prompt` / ONNX hotwords / fuzzy correction (A3/D1) —
  sub-project 1b.
- Audio frontend (B), streaming (G), other engines' decoding params (A7).
- Demonstrating the accuracy *gain* on hard audio. The only clip available is
  clean, so this stage proves *no regression*; the resilience win shows on
  noisy/looping audio, addable later as harder fixtures (recorded as a follow-up).

## Architecture

### 1. Decoder params (`src-tauri/src/stt/ggml_whisper.rs::transcribe`)

Three changes in the existing `FullParams` setup (lines ~61-84):

- **A1:** change `SamplingStrategy::Greedy { best_of: 1 }` to `best_of: 2`, and add
  `params.set_temperature_inc(0.2);` (right after the existing
  `params.set_temperature(0.0);`). The existing `set_logprob_thold(-1.0)` and
  `set_no_speech_thold(0.6)` are the ladder's trigger thresholds and stay.
- **A4:** add `params.set_no_context(true);`.
- **A5:** change `params.set_suppress_nst(false)` to `params.set_suppress_nst(true)`.

These are `whisper-rs` 0.16 `FullParams` setters. `set_suppress_nst` and
`set_temperature` are already used here, confirming the setter style. The plan must
confirm the exact names `set_temperature_inc` and `set_no_context` against the
installed `whisper-rs` 0.16 API before writing them (adapt if a name differs); this
is the one external-API risk in the stage.

### 2. Output sanitizer (`src-tauri/src/stt/mod.rs`)

A pure, dependency-free helper, unit-tested:

```rust
pub(crate) fn sanitize_output(text: &str) -> String
```

Rules (conservative — bias toward keeping real speech):

- Remove every maximal `[...]` span. Square-bracketed spans are almost always
  Whisper non-speech markers (`[BLANK_AUDIO]`, `[ Silence ]`, `[Music]`); users do
  not dictate square brackets.
- Remove a `(...)` span **only** when its inner text, lowercased and trimmed,
  matches a small known non-speech marker set: `blank_audio`, `silence`, `music`,
  `applause`, `laughter`, `noise`, `inaudible`. Other parentheticals (e.g.
  `(see note)`) are kept — people do dictate parentheses.
- Collapse runs of whitespace to a single space and trim. (Removing a marker can
  leave double spaces.)

`sanitize_output` never errors; all-marker input returns `""`.

### 3. Applying the sanitizer

- **Local path (`stt/mod.rs::transcribe_with_progress`):** the per-chunk block
  currently does `let text = text.trim().to_string(); if !text.is_empty() { parts.push(text); }`.
  Change to `let text = sanitize_output(&text); if !text.is_empty() { parts.push(text); }`
  (sanitize trims internally). A chunk that is only markers becomes empty and is
  dropped — same handling as today's empty text. This covers every local engine,
  not just Whisper.
- **Cloud path (`src-tauri/src/lib.rs`, the cloud branch of `transcribe_audio`):**
  apply `sanitize_output` to the **joined** cloud transcription before it is
  delivered. Markers never span a chunk boundary, so sanitizing the joined result
  is equivalent to per-chunk sanitizing and is simpler to wire in. The plan must
  locate the join point in `lib.rs` (large file) and call `stt::sanitize_output`
  there.

No `AsrEngine` trait change — sanitization is a post-step above the engines.

## Error handling

Unchanged. `sanitize_output` is total (never panics, never errors). An all-marker
chunk yields empty text and is dropped, exactly as empty engine output is today.
Existing empty-result handling in `transcribe_audio` (skip clipboard/paste/sound)
stays as is.

## Testing

- **Unit (`stt/mod.rs`)**: `sanitize_output` — strips `[BLANK_AUDIO]`, `[ Silence ]`,
  `[Music]`, `(music)`, `(applause)`; keeps normal text; keeps a non-marker
  parenthetical like `(see note)`; collapses internal whitespace; all-marker input
  → `""`; embedded marker mid-sentence (`"hello [Music] world"` → `"hello world"`).
- **`cargo test`** green (existing 82 + the new sanitizer tests).
- **Baseline gate (manual, controller-run)**: re-run the eval harness across the 4
  installed models on `test/output.wav`; WER must stay **0.000 / EXACT** for every
  model (no regression). The harness already prints the hypothesis + exact-match.
- `pnpm lint` unaffected (no frontend changes).

## Out-of-scope follow-ups (recorded, not planned)

- Harder eval fixtures (noisy / looping / silent-tail audio) to *demonstrate* the
  A1/A4/A5 gain, not just prove no-regression.
- Beam search + typed `DecodeParams` + UI (1c); custom dictionary (1b).
- Moving `sanitize_output` into a dedicated `stt/text.rs` when the D-chain
  (filler/stutter removal, custom words, OpenCC) lands — at that point the
  post-processing chain gets its own module.
