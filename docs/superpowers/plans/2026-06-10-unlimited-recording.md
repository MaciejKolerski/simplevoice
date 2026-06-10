# Unlimited Recording Length Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the 90-second transcription limit: recordings up to the 90-minute safety cap get transcribed by chunking at silence boundaries, with progress UI, an elapsed-time counter, and a pre-cap warning.

**Architecture:** The recording capture is already unbounded; the limit lives in `SttController::transcribe`. We add a pure silence-aware chunker (`stt/chunker.rs`), a chunked transcription loop with a progress callback (local + cloud paths), a 90-minute safety auto-stop with an 85-minute warning in the audio consumer thread, and overlay/HUD UI for timer/progress/warning. Spec: `docs/superpowers/specs/2026-06-10-unlimited-recording-design.md`.

**Tech Stack:** Rust (Tauri 2), React 19 + TypeScript, i18next. Tests: Rust `#[cfg(test)]` unit tests (pattern: `src-tauri/src/stt/streaming/segmenter.rs:84`). Frontend has no test framework — verification is `pnpm lint` (tsc strict) + manual.

**Conventions (CLAUDE.md):** pnpm only, no emojis, comments only for non-obvious *why*, commit as the user with NO Co-Authored-By trailer.

---

## File map

| File | Change |
| --- | --- |
| `src-tauri/src/stt/chunker.rs` | **Create** — silence-aware splitter + unit tests |
| `src-tauri/src/stt/mod.rs` | Register chunker; `prepare_samples` → `pub(crate)`; remove 90 s guard; add `ChunkedTranscription` + `transcribe_with_progress` + unit tests |
| `src-tauri/src/audio.rs` | `last_samples: Arc<Vec<f32>>`; extract `auto_stop_recording`; cap + warning constants and checks |
| `src-tauri/src/lib.rs` | `end_live_session` → `pub(crate)`; `transcribe_audio` uses chunked local + cloud paths, emits `transcription-progress`; `truncation_marker` helper |
| `src/App.tsx` | HUD progress bar state + listener |
| `src/views/RecordingWindowView.tsx` | Elapsed timer, progress panel, cap warning, language sync |
| `src/i18n/language.ts` | Emit `ui-language-changed` so the overlay window follows switches |
| `src/i18n/locales/{en,pl,de}.json` | `overlay.transcribing` + `overlay.timeWarning` keys |

Backend events added: `transcription-progress { done, total }` (only when total > 1), `recording-time-warning { seconds_left }` (= 300). Frontend event added: `ui-language-changed` (emitted by `src/i18n/language.ts`, consumed by the overlay window).

---

### Task 1: Silence-aware chunker (`stt/chunker.rs`)

**Files:**
- Create: `src-tauri/src/stt/chunker.rs`
- Modify: `src-tauri/src/stt/mod.rs` (module registration, one line)

- [ ] **Step 1.1: Create the module with tests and a `todo!()` stub**

Create `src-tauri/src/stt/chunker.rs`:

```rust
//! Splits long 16 kHz mono recordings into transcription-sized chunks.
//! Cuts are placed in the quietest pause found in a 45–90 s window, so no
//! word is ever bisected; chunks that contain no speech at all are dropped
//! (this also prevents Whisper hallucinations on silence).

use std::ops::Range;

pub const SAMPLE_RATE: usize = 16_000;
pub const CHUNK_MIN_SECS: usize = 45;
pub const CHUNK_MAX_SECS: usize = 90;
const SILENCE_RMS: f32 = 0.008;
const SKIP_CHUNK_RMS: f32 = 0.008;
const HOP_MS: usize = 100;
const HOP: usize = SAMPLE_RATE * HOP_MS / 1000;
/// Minimum quiet window for a cut (spec: SILENCE_WIN_MS).
const SILENCE_WIN_MS: usize = 300;
const SILENCE_HOPS_NEEDED: usize = SILENCE_WIN_MS / HOP_MS;

pub fn split_at_silences(samples: &[f32]) -> Vec<Range<usize>> {
    todo!()
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&x| x * x).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: usize) -> Vec<f32> {
        vec![0.5; secs * SAMPLE_RATE]
    }

    fn silence(secs: usize) -> Vec<f32> {
        vec![0.0; secs * SAMPLE_RATE]
    }

    fn assert_invariants(ranges: &[Range<usize>], input_len: usize) {
        let mut prev_end = 0;
        for r in ranges {
            assert!(r.start >= prev_end, "ranges must be ordered and non-overlapping");
            assert!(r.end <= input_len);
            assert!(
                r.end - r.start <= CHUNK_MAX_SECS * SAMPLE_RATE,
                "chunk longer than CHUNK_MAX_SECS: {} samples",
                r.end - r.start
            );
            prev_end = r.end;
        }
    }

    #[test]
    fn short_input_is_a_single_chunk() {
        let input = tone(30);
        let ranges = split_at_silences(&input);
        assert_eq!(ranges, vec![0..input.len()]);
    }

    #[test]
    fn input_at_exactly_max_is_a_single_chunk() {
        let input = tone(CHUNK_MAX_SECS);
        let ranges = split_at_silences(&input);
        assert_eq!(ranges, vec![0..input.len()]);
    }

    #[test]
    fn cut_lands_inside_the_silence_gap() {
        // 60 s speech, 2 s pause, 60 s speech: the only valid cut is in the pause.
        let mut input = tone(60);
        input.extend(silence(2));
        input.extend(tone(60));
        let ranges = split_at_silences(&input);
        assert_eq!(ranges.len(), 2);
        assert_invariants(&ranges, input.len());
        assert!(ranges[0].end >= 60 * SAMPLE_RATE, "cut before the pause");
        assert!(ranges[0].end <= 62 * SAMPLE_RATE, "cut after the pause");
        assert_eq!(ranges[1].start, ranges[0].end, "no samples lost between chunks");
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[1].end, input.len());
    }

    #[test]
    fn pauseless_speech_falls_back_to_quietest_hop() {
        let input = tone(120);
        let ranges = split_at_silences(&input);
        assert_eq!(ranges.len(), 2);
        assert_invariants(&ranges, input.len());
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[1].start, ranges[0].end);
        assert_eq!(ranges[1].end, input.len());
    }

    #[test]
    fn all_silence_yields_no_chunks() {
        let input = silence(120);
        assert!(split_at_silences(&input).is_empty());
    }

    #[test]
    fn trailing_silent_chunk_is_dropped() {
        // 50 s speech then 70 s silence: the silent tail chunk must be dropped.
        let mut input = tone(50);
        input.extend(silence(70));
        let ranges = split_at_silences(&input);
        assert_eq!(ranges.len(), 1);
        assert_invariants(&ranges, input.len());
        assert_eq!(ranges[0].start, 0);
        assert!(ranges[0].end >= 50 * SAMPLE_RATE, "speech must not be cut off");
    }

    #[test]
    fn empty_input_yields_no_chunks() {
        assert!(split_at_silences(&[]).is_empty());
    }
}
```

- [ ] **Step 1.2: Register the module**

In `src-tauri/src/stt/mod.rs`, after `pub mod streaming;` (line 11) add:

```rust
pub(crate) mod chunker;
```

- [ ] **Step 1.3: Run tests to verify they fail**

Run: `cd /Users/woro/Documents/simplevoice/src-tauri && cargo test chunker 2>&1 | tail -20`
Expected: compile succeeds, tests FAIL with `not yet implemented` panics (the `todo!()`).

- [ ] **Step 1.4: Implement `split_at_silences`**

Replace the `todo!()` stub in `src-tauri/src/stt/chunker.rs`:

```rust
pub fn split_at_silences(samples: &[f32]) -> Vec<Range<usize>> {
    let max = CHUNK_MAX_SECS * SAMPLE_RATE;
    let min = CHUNK_MIN_SECS * SAMPLE_RATE;
    let mut ranges = Vec::new();
    let mut start = 0;

    while samples.len() - start > max {
        let window = &samples[start + min..start + max];
        let cut = start + min + find_cut(window);
        push_if_speech(&mut ranges, samples, start..cut);
        start = cut;
    }
    if start < samples.len() {
        push_if_speech(&mut ranges, samples, start..samples.len());
    }
    ranges
}

/// Offset (in samples, hop-aligned) of the best cut point inside `window`:
/// the center of the quietest run of at least SILENCE_HOPS_NEEDED hops below
/// SILENCE_RMS, or the single quietest hop when speech never pauses.
fn find_cut(window: &[f32]) -> usize {
    let hops: Vec<f32> = window.chunks(HOP).map(rms).collect();

    let mut best: Option<(f32, usize)> = None; // (avg rms, cut hop index)
    let mut i = 0;
    while i < hops.len() {
        if hops[i] < SILENCE_RMS {
            let run_start = i;
            while i < hops.len() && hops[i] < SILENCE_RMS {
                i += 1;
            }
            let run_len = i - run_start;
            if run_len >= SILENCE_HOPS_NEEDED {
                let avg: f32 = hops[run_start..i].iter().sum::<f32>() / run_len as f32;
                if best.map_or(true, |(b, _)| avg < b) {
                    best = Some((avg, run_start + run_len / 2));
                }
            }
        } else {
            i += 1;
        }
    }

    if let Some((_, hop_idx)) = best {
        return hop_idx * HOP;
    }

    let quietest = hops
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    quietest * HOP
}

fn push_if_speech(out: &mut Vec<Range<usize>>, samples: &[f32], range: Range<usize>) {
    if !range.is_empty() && rms(&samples[range.clone()]) >= SKIP_CHUNK_RMS {
        out.push(range);
    }
}
```

- [ ] **Step 1.5: Run tests to verify they pass**

Run: `cd /Users/woro/Documents/simplevoice/src-tauri && cargo test chunker 2>&1 | tail -10`
Expected: `test result: ok. 7 passed`

- [ ] **Step 1.6: Commit**

```bash
git add src-tauri/src/stt/chunker.rs src-tauri/src/stt/mod.rs
git commit -m "feat(stt): silence-aware chunker for long recordings"
```

---

### Task 2: Chunked transcription in `SttController`

**Files:**
- Modify: `src-tauri/src/stt/mod.rs` (lines 16, 78-93 + new tests)

- [ ] **Step 2.1: Add unit tests with a fake engine (they will not compile yet)**

Append to `src-tauri/src/stt/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::stt::traits::{AsrEngine, ModelFormat};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeEngine {
        calls: AtomicUsize,
        /// 1-based call number from which transcribe starts failing.
        fail_from_call: Option<usize>,
    }

    impl FakeEngine {
        fn ok() -> Self {
            Self { calls: AtomicUsize::new(0), fail_from_call: None }
        }
        fn failing_from(n: usize) -> Self {
            Self { calls: AtomicUsize::new(0), fail_from_call: Some(n) }
        }
    }

    impl AsrEngine for FakeEngine {
        fn transcribe(&self, _samples: &[f32], _language: Option<&str>) -> Result<String, AppError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(n) = self.fail_from_call {
                if call >= n {
                    return Err(AppError::Command("boom".into()));
                }
            }
            Ok(format!("part{}", call))
        }
        fn display_name(&self) -> &str {
            "fake"
        }
        fn model_format(&self) -> ModelFormat {
            ModelFormat::GgmlBin
        }
    }

    fn controller_with(engine: FakeEngine) -> SttController {
        let c = SttController::new();
        c.state.lock().unwrap().engine = Some(std::sync::Arc::new(engine));
        c
    }

    /// 0.5 amplitude: above prepare_samples' trim threshold, so length is stable.
    fn speech(secs: usize) -> Vec<f32> {
        vec![0.5; secs * 16_000]
    }

    #[test]
    fn no_engine_errors() {
        let c = SttController::new();
        let err = c.transcribe(&speech(1), None).unwrap_err();
        assert!(err.contains("No speech-to-text model loaded"));
    }

    #[test]
    fn short_input_single_engine_call() {
        let c = controller_with(FakeEngine::ok());
        let mut progress: Vec<(usize, usize)> = Vec::new();
        let out = c
            .transcribe_with_progress(&speech(30), None, &mut |d, t| progress.push((d, t)))
            .unwrap();
        assert_eq!(out.text, "part1");
        assert!(out.truncated.is_none());
        assert_eq!(progress, vec![(1, 1)]);
    }

    #[test]
    fn long_input_is_chunked_and_joined_with_progress() {
        // 120 s of pauseless speech -> 2 chunks (45 s fallback cut + 75 s).
        let c = controller_with(FakeEngine::ok());
        let mut progress: Vec<(usize, usize)> = Vec::new();
        let out = c
            .transcribe_with_progress(&speech(120), None, &mut |d, t| progress.push((d, t)))
            .unwrap();
        assert_eq!(out.text, "part1 part2");
        assert!(out.truncated.is_none());
        assert_eq!(progress, vec![(1, 2), (2, 2)]);
    }

    #[test]
    fn over_90s_no_longer_errors() {
        let c = controller_with(FakeEngine::ok());
        assert_eq!(c.transcribe(&speech(91), None).unwrap(), "part1 part2");
    }

    #[test]
    fn first_chunk_failure_propagates_the_error() {
        let c = controller_with(FakeEngine::failing_from(1));
        let err = c.transcribe(&speech(120), None).unwrap_err();
        assert!(err.contains("Transcription failed"));
    }

    #[test]
    fn later_chunk_failure_returns_partial_text() {
        let c = controller_with(FakeEngine::failing_from(2));
        let mut progress: Vec<(usize, usize)> = Vec::new();
        let out = c
            .transcribe_with_progress(&speech(120), None, &mut |d, t| progress.push((d, t)))
            .unwrap();
        assert_eq!(out.text, "part1");
        let (secs, err) = out.truncated.expect("must report truncation");
        assert!((44.0..=46.0).contains(&secs), "failed chunk starts at ~45s, got {}", secs);
        assert!(err.contains("boom"));
        assert_eq!(progress, vec![(1, 2)], "no progress for the failed chunk");
    }

    #[test]
    fn empty_input_returns_empty_text_without_engine_calls() {
        let c = controller_with(FakeEngine::ok());
        let out = c.transcribe(&[], None).unwrap();
        assert_eq!(out, "");
    }
}
```

- [ ] **Step 2.2: Run tests to verify they fail to compile**

Run: `cd /Users/woro/Documents/simplevoice/src-tauri && cargo test --lib stt::tests 2>&1 | tail -10`
Expected: compile error — `transcribe_with_progress` and `ChunkedTranscription` do not exist.

- [ ] **Step 2.3: Implement chunked transcription**

In `src-tauri/src/stt/mod.rs`:

a) Line 16 — change visibility AND fix the RMS accumulator. With the 90 s guard
gone this function sees up to 86 M samples; a sequential f32 sum loses addends
once the running total is large (RMS underestimated ~2x at 1 h, gain then
over-amplifies and clips). Use f64 for the accumulation:

```rust
pub(crate) fn prepare_samples(samples: &[f32]) -> Vec<f32> {
```

and replace lines 37-39:

```rust
    let sum_sq: f32 = trimmed.iter().map(|&x| x * x).sum();
    let rms = (sum_sq / trimmed.len() as f32).sqrt().max(0.001);
    let gain = 0.70 / rms;
```

with:

```rust
    let sum_sq: f64 = trimmed.iter().map(|&x| x as f64 * x as f64).sum();
    let rms = (sum_sq / trimmed.len() as f64).sqrt().max(0.001) as f32;
    let gain = 0.70 / rms;
```

(`chunker::rms` needs no change — it only ever sees ≤90 s slices and 1600-sample hops.)

b) Replace the whole `transcribe` method (lines 78-93, including the 90 s guard) with:

```rust
    pub fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String, String> {
        self.transcribe_with_progress(samples, language, &mut |_, _| {})
            .map(|c| c.text)
    }

    /// Transcribes input of any length by splitting it at silence boundaries
    /// and running the active engine once per chunk. `progress(done, total)`
    /// fires after every successfully transcribed chunk.
    pub fn transcribe_with_progress(
        &self,
        samples: &[f32],
        language: Option<&str>,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<ChunkedTranscription, String> {
        let prepared = prepare_samples(samples);

        let engine_arc = {
            let s = self.state.lock().unwrap();
            s.engine.clone()
        };
        let engine =
            engine_arc.ok_or("No speech-to-text model loaded. Please load an ASR model first.")?;

        let chunks = chunker::split_at_silences(&prepared);
        let total = chunks.len();
        let mut parts: Vec<String> = Vec::with_capacity(total);
        let mut truncated = None;

        for (i, range) in chunks.iter().enumerate() {
            match engine.transcribe(&prepared[range.clone()], language) {
                Ok(text) => {
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        parts.push(text);
                    }
                }
                Err(e) => {
                    let err = format!("Transcription failed: {}", e);
                    // parts.is_empty(), not i == 0: if earlier chunks produced
                    // only empty text, a "partial" result would paste a lone
                    // truncation marker into the user's document.
                    if parts.is_empty() {
                        return Err(err);
                    }
                    truncated = Some((range.start as f32 / chunker::SAMPLE_RATE as f32, err));
                    break;
                }
            }
            progress(i + 1, total);
        }

        Ok(ChunkedTranscription {
            text: parts.join(" "),
            truncated,
        })
    }
```

c) Add above `pub struct SttState` (line 43):

```rust
pub struct ChunkedTranscription {
    pub text: String,
    /// Present when a chunk after the first failed: (offset in seconds of the
    /// failed chunk within the prepared audio, engine error). `text` holds
    /// everything transcribed before the failure.
    pub truncated: Option<(f32, String)>,
}
```

- [ ] **Step 2.4: Run tests to verify they pass**

Run: `cd /Users/woro/Documents/simplevoice/src-tauri && cargo test --lib 2>&1 | tail -10`
Expected: all tests pass, including the pre-existing streaming/cloud tests.

- [ ] **Step 2.5: Commit**

```bash
git add src-tauri/src/stt/mod.rs
git commit -m "feat(stt): chunked transcription with progress replaces the 90s limit"
```

---

### Task 3: Recording safety cap, warning, and the buffer-copy fix (`audio.rs`)

No unit tests here — this is thread/OS-integration code exercised by `cargo check`,
the existing test suite (compilation of all call sites), and the manual
verification in Task 7.

**Files:**
- Modify: `src-tauri/src/audio.rs` (lines 22, 89, 204-329, 387-416)
- Modify: `src-tauri/src/lib.rs:278` (`end_live_session` visibility)

- [ ] **Step 3.1: Make `end_live_session` callable from audio.rs**

In `src-tauri/src/lib.rs` line 278:

```rust
pub(crate) fn end_live_session(app: &tauri::AppHandle) {
```

- [ ] **Step 3.2: Switch `last_samples` to `Arc<Vec<f32>>`**

In `src-tauri/src/audio.rs`:

Line 22 (`AudioState` field):

```rust
    pub last_samples: Arc<Vec<f32>>,
```

Line 89 (`AudioController::new` init):

```rust
                last_samples: Arc::new(Vec::new()),
```

- [ ] **Step 3.3: Add the cap constants**

After the imports (below line 5) in `src-tauri/src/audio.rs`:

```rust
/// Safety net for forgotten recordings (the design target is ~1 h sessions).
/// Checked in the consumer thread regardless of VAD or live mode.
pub(crate) const RECORDING_MAX_SECS: usize = 5400;
/// Warn the user this long before the cap (emits `recording-time-warning`).
pub(crate) const RECORDING_WARNING_SECS: usize = 5100;
```

- [ ] **Step 3.4: Extract `auto_stop_recording` and wire in cap + warning**

In `src-tauri/src/audio.rs`, add this free function after `save_wav_file` (after line 73):

```rust
/// Completes an automatic stop (VAD silence or the max-duration cap) from the
/// consumer thread. Consumes the held state guard so the lock is released
/// before any blocking work; WAV save and notifications run on a new thread,
/// mirroring the manual stop path. The is_recording guard makes it a no-op
/// when a manual stop won the race in between two consumer iterations —
/// without it, this would overwrite last_samples (the real recording) with
/// the ≤1024-sample residue drained after the manual stop.
fn auto_stop_recording(
    mut s: std::sync::MutexGuard<'_, AudioState>,
    state: &Arc<Mutex<AudioState>>,
    app_handle: &tauri::AppHandle,
) {
    if !s.is_recording {
        return;
    }
    s.is_recording = false;
    s.is_saving = true;
    if let Some(wrapper) = s.stream.take() {
        let _ = wrapper.0.pause();
    }

    let paused_apps: Vec<String> = s.paused_media_apps.drain(..).collect();

    let samples = Arc::new(std::mem::take(&mut s.buffer));
    s.last_samples = Arc::clone(&samples);
    let start_time = s.recording_start.take().unwrap_or_else(chrono::Local::now);

    // Resume media before dropping the lock
    if !paused_apps.is_empty() {
        crate::media_control::resume_system_media(&paused_apps);
    }

    drop(s);

    // Refresh overlay visibility only AFTER releasing the audio-state lock:
    // update_recording_window_visibility re-locks it (is_recording / is_saving /
    // is_transcribing), so calling it while `s` was held would deadlock the
    // audio thread. is_saving is still true here, so the overlay stays up
    // through transcription (keeps App Nap away on macOS).
    #[cfg(target_os = "macos")]
    crate::update_recording_window_visibility(app_handle);

    let state_save_clone = Arc::clone(state);
    let app_handle_save_clone = app_handle.clone();
    std::thread::spawn(move || {
        let saved_path = save_wav_file(&app_handle_save_clone, &samples, start_time)
            .ok()
            .flatten();

        {
            let mut s = state_save_clone.lock().unwrap();
            s.is_saving = false;
            s.is_transcribing = true;
        }

        let _ = crate::rebuild_tray_menu(&app_handle_save_clone);
        crate::play_backend_sound(&app_handle_save_clone, "stop");

        let payload = saved_path.unwrap_or_else(|| "Recording stopped".to_string());
        let _ = app_handle_save_clone.emit("recording-stopped", payload);

        // A max-duration auto-stop can fire while a live session is active;
        // finish it so `transcription-final` is emitted. No-op when inactive
        // (the VAD path never runs in live mode).
        crate::end_live_session(&app_handle_save_clone);
    });
}
```

Then replace the consumer-thread body (the `std::thread::spawn(move || { ... });` block at lines 204-329) with:

```rust
        std::thread::spawn(move || {
            let mut local_buf = vec![0.0; 1024];
            let mut has_spoken = false;
            let mut silence_samples = 0;
            let mut warned_about_cap = false;

            loop {
                // Check state at start of loop iteration
                let (is_recording, vad_enabled, vad_threshold, vad_silence_duration_ms) = {
                    let s = state_clone.lock().unwrap();
                    (
                        s.is_recording,
                        s.vad_enabled,
                        s.vad_threshold,
                        s.vad_silence_duration_ms,
                    )
                };

                if !is_recording {
                    // Drain any remaining samples directly into buffer
                    let mut s = state_clone.lock().unwrap();
                    while !consumer.is_empty() {
                        let read = consumer.pop_slice(&mut local_buf);
                        s.buffer.extend_from_slice(&local_buf[..read]);
                    }
                    break;
                }

                // Read from consumer
                let read = consumer.pop_slice(&mut local_buf);
                if read > 0 {
                    // Compute RMS of the newly read samples for visualizer
                    let mut sum_sq = 0.0;
                    for &sample in &local_buf[..read] {
                        sum_sq += sample * sample;
                    }
                    let rms = (sum_sq / read as f32).sqrt();
                    let _ = app_handle_clone.emit("audio-amplitude", rms);

                    let mut should_warn = false;
                    {
                        let mut s = state_clone.lock().unwrap();
                        s.buffer.extend_from_slice(&local_buf[..read]);
                        let buffer_len = s.buffer.len();

                        // Read live state under the same lock as the fan-out so the two stay
                        // consistent with set_live_session / clear_live_session.
                        let live_active = s.live_mode_active;

                        // Live fan-out: hand the chunk to the streaming session. Non-blocking;
                        // the bounded channel returns Full rather than stalling the audio path.
                        if let Some(tx) = &s.stream_tx {
                            let _ = tx.try_send(local_buf[..read].to_vec());
                        }

                        if buffer_len >= RECORDING_MAX_SECS * 16_000 {
                            auto_stop_recording(s, &state_clone, &app_handle_clone);
                            break;
                        }

                        if vad_enabled && !live_active {
                            if rms >= vad_threshold {
                                has_spoken = true;
                                silence_samples = 0;
                            } else if has_spoken {
                                silence_samples += read;
                                let timeout_samples =
                                    (vad_silence_duration_ms as f32 / 1000.0 * 16000.0) as usize;
                                if silence_samples >= timeout_samples {
                                    auto_stop_recording(s, &state_clone, &app_handle_clone);
                                    break;
                                }
                            }
                        }

                        if !warned_about_cap && buffer_len >= RECORDING_WARNING_SECS * 16_000 {
                            warned_about_cap = true;
                            should_warn = true;
                        }
                    }
                    if should_warn {
                        let _ = app_handle_clone.emit(
                            "recording-time-warning",
                            serde_json::json!({
                                "seconds_left": (RECORDING_MAX_SECS - RECORDING_WARNING_SECS) as u32
                            }),
                        );
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
```

(The inner `{ }` scope around the lock is new: it releases the mutex before the
warning event is emitted. The VAD logic itself is unchanged — its stop block now
calls `auto_stop_recording` instead of inlining the same code.)

- [ ] **Step 3.5: Fix the manual stop path**

In `stop_recording` (lines 411-415), replace:

```rust
            let mut s = self.state.lock().unwrap();
            s.last_samples = s.buffer.clone();
            let samples = std::mem::take(&mut s.buffer);
            let start_time = s.recording_start.take().unwrap_or_else(chrono::Local::now);
            (samples, start_time)
```

with:

```rust
            let mut s = self.state.lock().unwrap();
            let samples = Arc::new(std::mem::take(&mut s.buffer));
            s.last_samples = Arc::clone(&samples);
            let start_time = s.recording_start.take().unwrap_or_else(chrono::Local::now);
            (samples, start_time)
```

(`save_wav_file(app_handle, &samples, start_time)` below still compiles —
`&Arc<Vec<f32>>` deref-coerces to `&[f32]`.)

- [ ] **Step 3.6: Fix `last_samples` consumers in lib.rs and verify compile**

In `src-tauri/src/lib.rs`, `transcribe_audio` (lines 1711-1714), replace:

```rust
    let final_samples = samples.unwrap_or_else(|| {
        let s = audio_controller.state.lock().unwrap();
        s.last_samples.clone()
    });
```

with:

```rust
    let final_samples: std::sync::Arc<Vec<f32>> = match samples {
        Some(v) => std::sync::Arc::new(v),
        None => {
            let s = audio_controller.state.lock().unwrap();
            std::sync::Arc::clone(&s.last_samples)
        }
    };
```

(`has_last_recording_samples` at line 1797 needs no change — `is_empty()`
resolves through the Arc.) The two `final_samples` consumers inside
`transcribe_audio` are rewritten in Task 4; for this commit to compile, adjust
only the local branch closure argument from `&final_samples` (unchanged text —
deref coercion handles `&Arc<Vec<f32>>` → `&[f32]`) and the cloud call's
`&final_samples` likewise stays textually identical. Verify:

Run: `cd /Users/woro/Documents/simplevoice/src-tauri && cargo check 2>&1 | tail -5`
Expected: `Finished` with no errors.

- [ ] **Step 3.7: Run the full test suite**

Run: `cd /Users/woro/Documents/simplevoice/src-tauri && cargo test --lib 2>&1 | tail -5`
Expected: all tests pass.

- [ ] **Step 3.8: Commit**

```bash
git add src-tauri/src/audio.rs src-tauri/src/lib.rs
git commit -m "feat(audio): 90-min safety cap with warning; drop a full buffer copy on stop"
```

---

### Task 4: Progress events, truncation marker, and chunked cloud path (`lib.rs`)

**Files:**
- Modify: `src-tauri/src/lib.rs` (`transcribe_audio`, lines 1709-1741, plus two new helpers)

- [ ] **Step 4.1: Add the marker helpers**

In `src-tauri/src/lib.rs`, immediately after `is_pause_audio_enabled` (after line 322):

```rust
fn config_ui_language(app_handle: &tauri::AppHandle) -> String {
    let Ok(dir) = app_handle.path().app_local_data_dir() else {
        return String::new();
    };
    let Ok(content) = std::fs::read_to_string(dir.join("config.json")) else {
        return String::new();
    };
    serde_json::from_str::<serde_json::Value>(&content)
        .ok()
        .and_then(|v| v.get("ui_language").and_then(|l| l.as_str()).map(str::to_string))
        .unwrap_or_default()
}

/// Marker appended to a partially transcribed recording when a chunk after
/// the first one failed (spec: partial text beats losing the whole dictation).
fn truncation_marker(app_handle: &tauri::AppHandle, secs: f32) -> String {
    marker_for(&config_ui_language(app_handle), secs)
}

/// Pure so the locale selection and mm:ss formatting are unit-testable.
fn marker_for(lang: &str, secs: f32) -> String {
    let total = secs.max(0.0) as u32;
    let (mm, ss) = (total / 60, total % 60);
    match lang {
        "pl" => format!(" [transkrypcja przerwana na {}:{:02}]", mm, ss),
        "de" => format!(" [Transkription abgebrochen bei {}:{:02}]", mm, ss),
        _ => format!(" [transcription stopped at {}:{:02}]", mm, ss),
    }
}
```

Add unit tests at the very end of `src-tauri/src/lib.rs` (the file has no test
module yet):

```rust
#[cfg(test)]
mod tests {
    use super::marker_for;

    #[test]
    fn truncation_marker_localization_and_format() {
        assert_eq!(marker_for("pl", 45.0), " [transkrypcja przerwana na 0:45]");
        assert_eq!(marker_for("de", 305.0), " [Transkription abgebrochen bei 5:05]");
        assert_eq!(marker_for("en", 3661.0), " [transcription stopped at 61:01]");
        assert_eq!(marker_for("", -1.0), " [transcription stopped at 0:00]");
    }
}
```

- [ ] **Step 4.2: Rewrite the `text` block in `transcribe_audio`**

Replace lines 1716-1741 (`let text = { ... };`) with:

```rust
    let text = {
        if engine == "openai-cloud" {
            let provider_name = provider.unwrap_or_else(|| "openai".to_string());
            let key = get_secure_api_key(provider_name.clone())?;
            if key.trim().is_empty() {
                return Err(format!("ASR API Key for {} is missing or empty. Please set it in models/engines settings.", provider_name));
            }
            // Same preprocessing + chunking as the local path (per spec): keeps
            // every upload far below provider size caps (OpenAI: 25 MB), yields
            // progress events, and puts chunk offsets and the truncation marker
            // on one timeline with the local engines.
            let prepared = crate::stt::prepare_samples(&final_samples);
            let chunks = crate::stt::chunker::split_at_silences(&prepared);
            let total = chunks.len();
            let mut parts: Vec<String> = Vec::with_capacity(total);
            let mut truncated_at: Option<f32> = None;
            for (i, range) in chunks.iter().enumerate() {
                match crate::stt::cloud::transcribe_cloud(
                    &prepared[range.clone()],
                    &key,
                    Some(&provider_name),
                    model.as_deref(),
                    base_url.as_deref(),
                    language.as_deref(),
                )
                .await
                {
                    Ok(part) => {
                        let part = part.trim().to_string();
                        if !part.is_empty() {
                            parts.push(part);
                        }
                    }
                    Err(e) => {
                        // Same rule as transcribe_with_progress: only return a
                        // partial result when there is actual text to keep.
                        if parts.is_empty() {
                            return Err(e.to_string());
                        }
                        eprintln!(
                            "[transcribe_audio] cloud chunk {}/{} failed: {}",
                            i + 1,
                            total,
                            e
                        );
                        truncated_at = Some(range.start as f32 / 16_000.0);
                        break;
                    }
                }
                if total > 1 {
                    let _ = app_handle.emit(
                        "transcription-progress",
                        serde_json::json!({ "done": i + 1, "total": total }),
                    );
                }
            }
            let mut joined = parts.join(" ");
            if let Some(secs) = truncated_at {
                joined.push_str(&truncation_marker(&app_handle, secs));
            }
            joined
        } else {
            let app_for_progress = app_handle.clone();
            let language_for_engine = language.clone();
            let samples_for_engine = std::sync::Arc::clone(&final_samples);
            let result: Result<crate::stt::ChunkedTranscription, String> =
                tauri::async_runtime::spawn_blocking(move || {
                    controller.transcribe_with_progress(
                        &samples_for_engine,
                        language_for_engine.as_deref(),
                        &mut |done, total| {
                            if total > 1 {
                                let _ = app_for_progress.emit(
                                    "transcription-progress",
                                    serde_json::json!({ "done": done, "total": total }),
                                );
                            }
                        },
                    )
                })
                .await
                .map_err(|e| e.to_string())?;
            let chunked = result?;
            let mut joined = chunked.text;
            if let Some((secs, err)) = chunked.truncated {
                eprintln!(
                    "[transcribe_audio] transcription truncated at {:.0}s: {}",
                    secs, err
                );
                joined.push_str(&truncation_marker(&app_handle, secs));
            }
            joined
        }
    };
```

- [ ] **Step 4.3: Compile and test**

Run: `cd /Users/woro/Documents/simplevoice/src-tauri && cargo check 2>&1 | tail -5 && cargo test --lib 2>&1 | tail -5`
Expected: clean compile, all tests pass.

- [ ] **Step 4.4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(stt): progress events, partial-failure marker, chunked cloud transcription"
```

---

### Task 5: HUD progress bar (`App.tsx`)

**Files:**
- Modify: `src/App.tsx` (state at ~line 41, listeners at ~155-360, HUD at ~450-460)

- [ ] **Step 5.1: Add progress state and listener**

a) Next to the other state hooks (after line 41 `isTranscribing`):

```tsx
  const [transcriptionProgress, setTranscriptionProgress] = useState<{
    done: number;
    total: number;
  } | null>(null);
```

b) In `handleStarted` (line ~156) add:

```tsx
      setTranscriptionProgress(null);
```

c) In `handleStopped`'s `finally` block (line ~254-257) add:

```tsx
        setTranscriptionProgress(null);
```

d) Register the listener next to the others (after `unlistenFinal`, line ~341):

```tsx
    const unlistenProgress = listen<{ done: number; total: number }>(
      "transcription-progress",
      (event) => setTranscriptionProgress(event.payload),
    );
```

and add the matching cleanup in the effect's return:

```tsx
      unlistenProgress.then((f) => f());
```

- [ ] **Step 5.2: Render the bar in the HUD**

In the transcribing branch of the HUD (after the `{t("hud.transcribingHint")}` paragraph, line ~458), add:

```tsx
                  {transcriptionProgress && transcriptionProgress.total > 1 && (
                    <div className="w-full mt-4">
                      <div className="h-1.5 w-full rounded-full bg-white/10 overflow-hidden">
                        <div
                          className="h-full rounded-full bg-white/80 transition-all duration-300"
                          style={{
                            width: `${Math.round((transcriptionProgress.done / transcriptionProgress.total) * 100)}%`,
                          }}
                        />
                      </div>
                      <p className="text-muted text-xs mt-2 tabular-nums">
                        {Math.round((transcriptionProgress.done / transcriptionProgress.total) * 100)}%
                      </p>
                    </div>
                  )}
```

- [ ] **Step 5.3: Lint**

Run: `cd /Users/woro/Documents/simplevoice && pnpm lint`
Expected: no output (tsc strict passes).

- [ ] **Step 5.4: Commit**

```bash
git add src/App.tsx
git commit -m "feat(ui): transcription progress bar in the HUD"
```

---

### Task 6: Overlay timer, progress percent, and cap warning (`RecordingWindowView.tsx` + i18n)

**Files:**
- Modify: `src/views/RecordingWindowView.tsx`
- Modify: `src/i18n/language.ts` (broadcast language changes to the overlay window)
- Modify: `src/i18n/locales/en.json`, `src/i18n/locales/pl.json`, `src/i18n/locales/de.json` (each: new `overlay` section after `hud`, line ~130)

- [ ] **Step 6.1: Add the i18n keys**

In each locale file, after the closing `}` of the `"hud"` section (line ~130), add a sibling section:

`en.json`:
```json
  "overlay": {
    "transcribing": "Transcribing… {{percent}}%",
    "timeWarning": "Recording will stop in {{time}}"
  },
```

`pl.json`:
```json
  "overlay": {
    "transcribing": "Transkrypcja… {{percent}}%",
    "timeWarning": "Nagranie zatrzyma się za {{time}}"
  },
```

`de.json`:
```json
  "overlay": {
    "transcribing": "Transkription… {{percent}}%",
    "timeWarning": "Aufnahme stoppt in {{time}}"
  },
```

- [ ] **Step 6.1b: Broadcast language changes to the overlay window**

Each Tauri window has its own JS context and i18n instance; `changeLanguage` in
`src/i18n/language.ts` only updates the calling (main) window. Emit a
cross-window event, mirroring the existing `live-overlay-mode-changed` pattern.

In `src/i18n/language.ts`, add to the imports:

```ts
import { emit } from "@tauri-apps/api/event";
```

and extend `changeLanguage`:

```ts
export async function changeLanguage(lang: Language): Promise<void> {
  await i18n.changeLanguage(lang);
  await persistLanguage(lang);
  await pushTrayLabels();
  emit("ui-language-changed", lang).catch(() => {});
}
```

- [ ] **Step 6.2: Add timer/progress/warning to the overlay**

In `src/views/RecordingWindowView.tsx`:

a) Imports (top of file):

```tsx
import { useTranslation } from "react-i18next";
import i18n from "../i18n";
```

b) Inside the component, next to the existing state (after line 29, the closing `);` of the `overlayMode` useState):

```tsx
  const { t } = useTranslation();
  const [elapsed, setElapsed] = useState<number | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [warningSecs, setWarningSecs] = useState<number | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const startedAtRef = useRef<number>(0);
```

c) In the mount effect (after the `is_recording_window_locked_cmd` invoke, line ~52), add the language sync and a timer helper:

```tsx
    // This window skips applyPersistedLanguage (it would re-push tray labels),
    // so sync the overlay strings to the configured UI language directly.
    invoke<string>("load_config")
      .then((str) => {
        const lang = JSON.parse(str || "{}").ui_language;
        if (typeof lang === "string" && i18n.language !== lang) {
          i18n.changeLanguage(lang).catch(() => {});
        }
      })
      .catch(() => {});

    // Follow live language switches made in the main window's settings.
    const unlistenLanguage = listen<string>("ui-language-changed", (event) => {
      if (event.payload && i18n.language !== event.payload) {
        i18n.changeLanguage(event.payload).catch(() => {});
      }
    });

    const stopTimer = () => {
      if (timerRef.current) {
        clearInterval(timerRef.current);
        timerRef.current = null;
      }
    };
```

d) Extend the existing handlers in the same effect:

`recording-started` (line ~54) becomes:

```tsx
    const unlistenStarted = listen("recording-started", () => {
      statusRef.current = "recording";
      setCommitted("");
      setTentative("");
      setProgress(null);
      setWarningSecs(null);
      startedAtRef.current = Date.now();
      setElapsed(0);
      stopTimer();
      timerRef.current = setInterval(() => {
        setElapsed(Math.floor((Date.now() - startedAtRef.current) / 1000));
      }, 1000);
    });
```

`recording-stopped` (line ~60) becomes:

```tsx
    const unlistenStopped = listen("recording-stopped", () => {
      statusRef.current = "transcribing";
      amplitudeRef.current = 0;
      stopTimer();
      setElapsed(null);
      setWarningSecs(null);
    });
```

`transcribing-status` (line ~65) becomes:

```tsx
    const unlistenTranscribing = listen<boolean>("transcribing-status", (event) => {
      statusRef.current = event.payload ? "transcribing" : "idle";
      if (!event.payload) {
        amplitudeRef.current = 0;
        setProgress(null);
      }
    });
```

e) Register two new listeners next to the others:

```tsx
    const unlistenProgress = listen<{ done: number; total: number }>(
      "transcription-progress",
      (event) => setProgress(event.payload),
    );
    const unlistenTimeWarning = listen<{ seconds_left: number }>(
      "recording-time-warning",
      (event) => setWarningSecs(event.payload.seconds_left),
    );
```

f) Extend the cleanup return with:

```tsx
      unlistenLanguage.then((f) => f());
      unlistenProgress.then((f) => f());
      unlistenTimeWarning.then((f) => f());
      stopTimer();
```

g) Add the format helper above the `return` (next to the `RECENT_WORDS` logic, line ~203):

```tsx
  const formatElapsed = (secs: number) => {
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    const s = secs % 60;
    return h > 0
      ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
      : `${m}:${String(s).padStart(2, "0")}`;
  };
```

h) In the JSX, inside the pill `<div data-tauri-drag-region ...>` after the `<canvas>` (line ~229) — the elapsed timer:

```tsx
        {elapsed !== null && (
          <span className="ml-2 text-[11px] tabular-nums text-white/70">
            {formatElapsed(elapsed)}
          </span>
        )}
```

i) Between the pill and the live-text panel (before `{hasText && (`, line ~231) — the transcription-progress panel (spec: localized label + thin bar in the overlay's transcribing state) and the cap warning. Progress shows only during batch transcription and the warning only during recording, and live text never co-exists with batch progress, so the only stacking risk is warning + live text — which is why the live-text panel shrinks below (step j):

```tsx
      {progress && progress.total > 1 && (
        <div className="mt-2 w-[184px] rounded-2xl border border-white/10 bg-[#0d0d0e]/80 backdrop-blur-xl px-3 py-2 shadow-[0_12px_40px_rgba(0,0,0,0.6)]">
          <p className="text-[11px] leading-snug text-white/85 tabular-nums">
            {t("overlay.transcribing", {
              percent: Math.round((progress.done / progress.total) * 100),
            })}
          </p>
          <div className="mt-1.5 h-1 w-full rounded-full bg-white/10 overflow-hidden">
            <div
              className="h-full rounded-full bg-white/80 transition-all duration-300"
              style={{ width: `${Math.round((progress.done / progress.total) * 100)}%` }}
            />
          </div>
        </div>
      )}
      {warningSecs !== null && (
        <div className="mt-2 rounded-2xl border border-amber-500/40 bg-[#0d0d0e]/80 backdrop-blur-xl px-3 py-1.5 shadow-[0_12px_40px_rgba(0,0,0,0.6)]">
          <p className="text-[11px] leading-snug text-amber-400/95">
            {t("overlay.timeWarning", { time: formatElapsed(warningSecs) })}
          </p>
        </div>
      )}
```

j) The overlay window is a fixed 200x180 px panel; with the warning and the
live-text panel stacked (live mode in the 85-90 min window), `max-h-[120px]`
overflows the window and clips the newest words. Shrink the live panel while
the warning is visible — change the `hasText` panel's className (line ~232)
from:

```tsx
        <div className="mt-2 flex w-[184px] max-h-[120px] flex-col justify-end overflow-hidden rounded-2xl border border-white/10 bg-[#0d0d0e]/80 backdrop-blur-xl px-3 py-2 text-left shadow-[0_12px_40px_rgba(0,0,0,0.6)]">
```

to:

```tsx
        <div
          className={`mt-2 flex w-[184px] ${warningSecs !== null ? "max-h-[76px]" : "max-h-[120px]"} flex-col justify-end overflow-hidden rounded-2xl border border-white/10 bg-[#0d0d0e]/80 backdrop-blur-xl px-3 py-2 text-left shadow-[0_12px_40px_rgba(0,0,0,0.6)]`}
        >
```

- [ ] **Step 6.3: Lint**

Run: `cd /Users/woro/Documents/simplevoice && pnpm lint`
Expected: no output.

- [ ] **Step 6.4: Commit**

```bash
git add src/views/RecordingWindowView.tsx src/i18n/locales/en.json src/i18n/locales/pl.json src/i18n/locales/de.json
git commit -m "feat(ui): overlay elapsed timer, chunk progress, and pre-cap warning"
```

---

### Task 7: Full verification

- [ ] **Step 7.1: Full backend test suite and lint**

Run:
```bash
cd /Users/woro/Documents/simplevoice/src-tauri && cargo test --lib 2>&1 | tail -5
cd /Users/woro/Documents/simplevoice && pnpm lint
```
Expected: all Rust tests pass; lint silent.

- [ ] **Step 7.2: Manual verification in the dev app**

Run: `cd /Users/woro/Documents/simplevoice && pnpm tauri dev`

Checklist (local GGML engine active, live mode OFF):
1. Record ~10 s → behavior identical to before (no progress bar, single chunk), paste works.
2. Record ~2.5 min of speech with natural pauses → recording does NOT error;
   overlay shows a running `M:SS` timer while recording; during transcription
   the overlay shows a percent and the HUD (main window) shows the progress
   bar; final text reads naturally across chunk boundaries (no bisected words);
   paste + history entry work.
3. While recording, watch the overlay timer pass 1:00 without drift.
4. Switch UI language in settings → the overlay warning string language follows
   (verifiable by temporarily lowering `RECORDING_WARNING_SECS` to e.g. 10 and
   `RECORDING_MAX_SECS` to 20, recording for 25 s, observing the warning at
   10 s and the graceful auto-stop at 20 s with normal transcription; revert
   the constants afterwards).
5. Live mode ON quick regression: start/stop a short live dictation — committed
   words stream, final text pastes, no batch progress bar appears.
6. (Optional — only if a cloud API key is configured) Switch the engine to
   cloud and record >2 min: chunked uploads succeed and progress events appear.
   Previously a >13 min cloud recording failed on the provider's 25 MB cap;
   chunking removes that ceiling.

- [ ] **Step 7.3: Update the architecture doc**

`SIMPLEVOICE.md` documents the audio/STT architecture. Add to the STT section a
sentence: long recordings are chunked at silence boundaries
(`stt/chunker.rs`, 45–90 s chunks) and transcribed sequentially with
`transcription-progress` events; recording auto-stops at the 90-minute safety
cap (`audio.rs::RECORDING_MAX_SECS`).

- [ ] **Step 7.4: Final commit**

```bash
git add SIMPLEVOICE.md
git commit -m "docs: chunked long-recording transcription in architecture notes"
```

---

## Plan deviations from spec (intentional, minor)

- `CHUNK_TARGET_SECS = 60` from the spec is not a separate constant: the
  algorithm cuts at the quietest pause in the 45–90 s window, so the "target"
  is emergent; a constant that nothing reads would be dead weight.
- The truncation marker is built in `lib.rs` (which can read `ui_language`
  from config.json), not inside `SttController` — the STT layer returns
  structured data (`ChunkedTranscription.truncated`) instead of a localized
  string. Same user-visible behavior as specified. The marker additionally
  gets a German variant (the spec said pl/en, but the app ships a de locale).
- An all-silent input ≤ 90 s yields zero chunks, not one: the spec's
  silent-chunk drop (`SKIP_CHUNK_RMS`) takes precedence over its "≤ 90 s input
  yields one chunk" invariant (the spec demands both; they conflict only for
  pure silence, where dropping is the useful behavior — `transcribe` then
  returns empty text and the existing empty-result handling skips paste).
- Manual cloud verification uses a >2 min multi-chunk recording as evidence
  for the spec's >13 min case: the provider cap constrains per-upload size,
  and every chunk is ≤ 90 s regardless of total length, so exercising any
  multi-chunk upload proves the mechanism.
