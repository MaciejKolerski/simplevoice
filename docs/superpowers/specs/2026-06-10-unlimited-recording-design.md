# Unlimited recording length — design

Date: 2026-06-10
Status: approved by user

## Problem

Recording capture is already unbounded, but `SttController::transcribe` rejects any
input longer than 90 seconds (`src-tauri/src/stt/mod.rs:81`, `prepared.len() > 16_000 * 90`)
with the error "Recording too long (>90s)". Users cannot transcribe anything longer
than a short voice message. The cloud branch (`lib.rs`, `transcribe_audio`) bypasses
that guard but fails anyway on provider upload limits (OpenAI: 25 MB ≈ 13 min of WAV).

## Goals

- Recordings up to ~1 hour by design, hard safety stop at 90 minutes.
- Transcription works for arbitrary-length input on every engine (whisper.cpp GGML,
  Candle, sherpa-onnx, NeMo) and on the cloud path, using the engine the user already
  selected.
- Progress reporting during long transcriptions (percent in the recording overlay
  and main-window HUD).
- Elapsed-time counter in the recording overlay while recording.
- Warning in the overlay shortly before the safety stop.
- Recordings that fit in a single chunk (≤ 90 s) take the existing single-pass
  path — zero behavior change for the common case.

## Non-goals

- Live transcription mode: unchanged (it has its own segmentation and no batch limit).
- Disk-streamed capture: not needed for the 1-hour design target (~230 MB f32 RAM at
  16 kHz mono; one buffer copy after the `mem::take` fix below).
- Cross-chunk prompt continuity (feeding the previous chunk's tail as Whisper
  `initial_prompt`): explicitly deferred; noted as a future enhancement.
- Parallel chunk transcription: chunks run sequentially. Local engines are
  CPU/GPU-bound (parallelism would not help) and sequential order keeps progress
  reporting and partial-failure semantics trivial.

## Architecture

### 1. Recording safety net (`src-tauri/src/audio.rs`)

The consumer thread already counts accumulated samples. Two new thresholds, checked
there (constants in `audio.rs`):

- `RECORDING_WARNING_SECS = 5100` (85 min): emit `recording-time-warning`
  `{ seconds_left: u32 }` once per recording.
- `RECORDING_MAX_SECS = 5400` (90 min): trigger a graceful auto-stop reusing the
  existing VAD auto-stop mechanics (save WAV, emit `recording-stopped`, resume
  paused media, then normal transcription). Unlike VAD, the cap check runs
  regardless of `vad_enabled` and also in live mode — it is a forgotten-recording
  safety net, not a feature limit.

Memory fix: both stop paths (manual `stop_recording`, VAD/cap auto-stop) currently do
`s.last_samples = s.buffer.clone()`. Replace with `std::mem::take(&mut s.buffer)`
handed to `last_samples`, and write the WAV from `last_samples`. Saves a full copy
(~230 MB at 1 h). `buffer` starts empty on the next recording exactly as today
(`s.buffer.clear()` in `start_recording` remains correct on an empty Vec).

### 2. Chunker (`src-tauri/src/stt/chunker.rs`, new module)

Pure function, no I/O, unit-testable:

```rust
pub fn split_at_silences(samples: &[f32]) -> Vec<std::ops::Range<usize>>
```

Constants (in `chunker.rs`):

- `CHUNK_TARGET_SECS = 60`, `CHUNK_MIN_SECS = 45`, `CHUNK_MAX_SECS = 90`
- `SILENCE_WIN_MS = 300` — minimum quiet window for a cut
- `SILENCE_RMS = 0.008` — same RMS threshold the segmenter/VAD already use
- `SKIP_CHUNK_RMS = 0.008` (same value as `SILENCE_RMS`) — a chunk whose overall
  RMS is below this is dropped from the result (kills Whisper silence
  hallucinations for free)

Algorithm per chunk: scan RMS over 100 ms hops inside the window
`[start + CHUNK_MIN_SECS, start + CHUNK_MAX_SECS]`; cut at the center of the
quietest ≥ 300 ms run below `SILENCE_RMS`; if speech never dips below the threshold
(no pause at all in 45 s), cut at the single quietest hop in the window. Input
≤ `CHUNK_MAX_SECS` returns a single range untouched.

Invariants (unit-tested): cuts never exceed `CHUNK_MAX_SECS` per chunk; ranges are
ordered, non-overlapping, and cover the input except dropped silent chunks;
≤ 90 s input yields one chunk.

### 3. Transcription loop (`src-tauri/src/stt/mod.rs` + `src-tauri/src/lib.rs`)

- Delete the 90-second guard in `SttController::transcribe`.
- New method on `SttController`:

```rust
pub fn transcribe_with_progress(
    &self,
    samples: &[f32],
    language: Option<&str>,
    progress: &mut dyn FnMut(usize, usize), // (done_chunks, total_chunks)
) -> Result<String, /* same error type as the existing transcribe() */>
```

  `prepare_samples` runs once on the full input (as today), then: single chunk →
  existing direct path; multiple chunks → sequential `engine.transcribe` per chunk,
  results joined with a single space, `progress(done, total)` after each chunk.
  Existing `transcribe` delegates with a no-op callback.

- `transcribe_audio` (lib.rs): the local branch calls `transcribe_with_progress`,
  emitting `transcription-progress { done: usize, total: usize }` from the callback
  via a cloned `AppHandle` (the call already runs inside `spawn_blocking`; Tauri
  `emit` is thread-safe). The cloud branch uses the same chunker: prepare samples,
  `split_at_silences`, one `transcribe_cloud` call per chunk (60 s ≈ 1.9 MB WAV,
  far below provider caps), same join and same progress event. `prepare_samples`
  becomes `pub(crate)` so the cloud branch shares the same preprocessing (today it
  skips silence trimming).

### 4. Partial-failure semantics

If chunk N fails (engine error, cloud HTTP error) after at least one chunk
succeeded, return the text of chunks 1..N-1 and append a marker —
`" [transkrypcja przerwana na {mm:ss}]"` when `ui_language` in config.json is
`pl`, `" [transcription stopped at {mm:ss}]"` otherwise ({mm:ss} = offset of the
failed chunk). The user keeps most of their dictation instead of losing
everything. If the FIRST chunk fails, propagate the error as today. Existing empty-result handling in `transcribe_audio` (skip
clipboard/paste/sound) stays as is.

### 5. UI (`src/App.tsx`, `src/views/RecordingWindowView.tsx`)

- **Elapsed timer** (overlay): on `recording-started` start a 1 s interval; render
  `M:SS` (`H:MM:SS` past an hour) in the overlay; clear on `recording-stopped`.
  Pure frontend state, no backend involvement.
- **Progress bar** (overlay + HUD): listen to `transcription-progress`; show
  `Transkrypcja… {percent}%` with a thin bar in the overlay's transcribing state and
  next to the existing HUD spinner in the main window. Reset on
  `transcription-final` / transcribe completion. Single-chunk recordings emit no
  progress event → UI shows the current spinner, unchanged.
- **Cap warning** (overlay): on `recording-time-warning`, show "Nagranie zatrzyma
  się za 5:00" in the overlay until stop.
- All three strings go through the existing i18n files (pl + en).

### 6. Unchanged

Live mode, VAD behavior, pause/resume of system media, WAV persistence (format
ceiling ~37 h), sqlite history (TEXT/REAL, no limits), auto-paste, AppNapGuard
(already held across the whole `transcribe_audio` command, so multi-minute
inference survives App Nap).

## Error handling summary

| Failure | Behavior |
| --- | --- |
| Engine/cloud error on first chunk | Error surfaces exactly as today (AlertDialog) |
| Error on later chunk | Partial text + truncation marker, normal paste flow |
| 90 min cap reached | Graceful stop, normal save + transcription |
| Pauseless speech (no silence to cut) | Fallback cut at quietest sample in window |
| All-silent chunk | Dropped before engine call |

## Testing

- **Unit (Rust, `chunker.rs`)**: synthetic 16 kHz buffers (sine bursts + silence
  gaps): cut points land inside silence runs; invariants above; all-silence input
  → empty result; 30 s input → single untouched range.
- **Unit (Rust, `stt/mod.rs`)**: progress callback fires (n, total) exactly once
  per chunk with a mock engine; join semantics; partial-failure marker.
- **`cargo check` / `cargo test` + `pnpm lint`** must pass.
- **Manual**: multi-minute real recording on the local GGML engine — verify chunk
  boundaries don't bisect words, progress renders, paste works; a >13 min cloud
  recording would previously fail and must now succeed (verifiable only with an
  API key configured).

## Out-of-scope follow-ups (recorded, not planned)

- Whisper `initial_prompt` continuity across chunks (better punctuation/casing
  continuity).
- Disk-spill capture for multi-hour recordings.
- Cancel button for an in-flight long transcription.
