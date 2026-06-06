# Live Transcription — Faza 0a: Streaming Core (backend) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the pure, unit-tested backend core of live transcription — the streaming event model, an RMS speech-segmenter, and a VAD-segmented strategy — entirely with `cargo test`, touching no UI or audio threads.

**Architecture:** A new `src-tauri/src/stt/streaming/` module. `StreamingStrategy` is a trait that consumes 16 kHz mono f32 audio and emits `StreamEvent`s over a `crossbeam-channel`. `SpeechSegmenter` (pure) cuts audio only on end-of-speech silence — so no word is ever bisected. `VadSegmentedStrategy` feeds closed segments to any `AsrEngine` (batch) and emits committed/final text. This is the foundation; Faza 0b wires it into `audio.rs`/commands, Faza 0c into the React UI.

**Tech Stack:** Rust, `crossbeam-channel` 0.5 (already a dep), `serde`, existing `AsrEngine` trait (`stt/traits.rs`), `AppError` (`error.rs`). Inline `#[cfg(test)]` tests (same pattern as `stt/cloud.rs`).

---

## Pre-step: branch

Current branch is `feat/byok-model-list` (unrelated). Start this work on a fresh branch off `main`:

```bash
cd /Users/woro/Documents/simplevoice
git checkout main
git pull --ff-only        # if a remote main exists; skip if it errors offline
git checkout -b feat/live-transcription
```

> Note: commits below are part of the TDD rhythm. Per project memory ("commit only when the user asks"), confirm with the user whether to auto-commit each task or hold. Commits are authored as the user, with NO Co-Authored-By trailer.

## File Structure

- Create: `src-tauri/src/stt/streaming/mod.rs` — `StreamEvent`, `StreamSink`, `StreamingStrategy` trait, submodule declarations.
- Create: `src-tauri/src/stt/streaming/segmenter.rs` — `SpeechSegmenter` (pure RMS end-of-speech segmentation) + tests.
- Create: `src-tauri/src/stt/streaming/vad_segmented.rs` — `VadSegmentedStrategy` (+ `MockEngine` test helper) + tests.
- Modify: `src-tauri/src/stt/mod.rs:10` — add `pub mod streaming;` after the existing `pub mod downloader;`.

Each file owns one responsibility; tests live inline at the bottom of each file.

---

### Task 1: Streaming module scaffold + event model

**Files:**
- Create: `src-tauri/src/stt/streaming/mod.rs`
- Modify: `src-tauri/src/stt/mod.rs` (add module declaration)
- Test: inline in `mod.rs`

- [ ] **Step 1: Create the module file with the event model and trait**

Create `src-tauri/src/stt/streaming/mod.rs`:

```rust
use crossbeam_channel::Sender;
use crate::error::AppError;

pub mod segmenter;
pub mod vad_segmented;

/// Events emitted by a live strategy. Serialized to the frontend in Faza 0b.
/// `Committed.full` is the authoritative committed text (single source of truth);
/// `Committed.delta` is the append-only chunk safe to auto-paste.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Tentative tail — re-rendered whole in the overlay, never pasted.
    Partial { text: String },
    /// Newly stabilized text — append-only, safe to auto-paste.
    Committed { delta: String, full: String },
    /// Utterance/session finalized.
    Final { text: String },
    /// Strategy error. `recoverable = true` => the stream keeps going.
    Error { reason: String, recoverable: bool },
}

/// Channel the strategy emits events on. Bounded channel is created by the
/// controller in Faza 0b; the strategy only holds the `Sender`.
pub type StreamSink = Sender<StreamEvent>;

/// A live transcription strategy. Runs on a dedicated worker thread, so it may
/// block (e.g. call `AsrEngine::transcribe`) directly — no async required.
pub trait StreamingStrategy: Send {
    /// Feed mono 16 kHz f32 audio (any chunk length). Emits zero or more events.
    fn push_audio(&mut self, samples: &[f32], sink: &StreamSink) -> Result<(), AppError>;
    /// End of session: flush buffered speech and emit a `Final`.
    fn finish(&mut self, sink: &StreamSink) -> Result<(), AppError>;
    /// Reset between utterances (clears committed/tentative state).
    fn reset(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_event_roundtrips_through_channel_and_serializes() {
        let (tx, rx) = crossbeam_channel::unbounded::<StreamEvent>();
        tx.send(StreamEvent::Committed { delta: "hi".into(), full: "hi".into() }).unwrap();
        let got = rx.recv().unwrap();
        assert_eq!(got, StreamEvent::Committed { delta: "hi".into(), full: "hi".into() });

        let json = serde_json::to_string(&StreamEvent::Error { reason: "boom".into(), recoverable: true }).unwrap();
        assert!(json.contains("\"kind\":\"error\""));
        assert!(json.contains("\"recoverable\":true"));
    }
}
```

- [ ] **Step 2: Register the module**

In `src-tauri/src/stt/mod.rs`, after line `pub mod downloader;` (currently line 10), add:

```rust
pub mod streaming;
```

- [ ] **Step 3: Run the test to verify it fails to compile then passes**

Run: `cd src-tauri && cargo test --lib streaming::tests::stream_event_roundtrips -- --nocapture`
Expected: first build is slow (whisper/candle/onnx deps); then PASS. If `segmenter`/`vad_segmented` modules don't exist yet, the build fails — create empty stubs to unblock:

In `segmenter.rs` and `vad_segmented.rs` put a temporary `// placeholder` (replaced in Tasks 2-3). Re-run; Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/stt/streaming/mod.rs src-tauri/src/stt/streaming/segmenter.rs src-tauri/src/stt/streaming/vad_segmented.rs src-tauri/src/stt/mod.rs
git commit -m "feat(live): streaming event model + strategy trait scaffold"
```

---

### Task 2: SpeechSegmenter (pure RMS end-of-speech segmentation)

**Files:**
- Create/replace: `src-tauri/src/stt/streaming/segmenter.rs`
- Test: inline

- [ ] **Step 1: Write the failing tests**

Replace the contents of `src-tauri/src/stt/streaming/segmenter.rs` with:

```rust
/// Pure, side-effect-free speech segmenter over 16 kHz mono f32.
/// Accumulates speech; when trailing silence after speech exceeds the
/// configured duration, it closes a segment. Because cuts only ever land in
/// silence, no word is bisected.
pub enum SegmenterEvent {
    /// Nothing to emit yet (leading silence, or still mid-speech).
    None,
    /// A complete speech segment, closed on an end-of-speech pause.
    SegmentClosed(Vec<f32>),
}

pub struct SpeechSegmenter {
    threshold: f32,
    silence_samples_needed: usize,
    current: Vec<f32>,
    has_spoken: bool,
    silence_samples: usize,
}

impl SpeechSegmenter {
    pub fn new(threshold: f32, silence_ms: u32, sample_rate: u32) -> Self {
        let silence_samples_needed = (silence_ms as f32 / 1000.0 * sample_rate as f32) as usize;
        Self {
            threshold,
            silence_samples_needed,
            current: Vec::new(),
            has_spoken: false,
            silence_samples: 0,
        }
    }

    /// Feed one audio chunk. Returns `SegmentClosed` when a pause closes a segment.
    pub fn push(&mut self, chunk: &[f32]) -> SegmenterEvent {
        let rms = rms(chunk);
        if rms >= self.threshold {
            self.has_spoken = true;
            self.silence_samples = 0;
            self.current.extend_from_slice(chunk);
            SegmenterEvent::None
        } else if self.has_spoken {
            // Trailing silence after speech: keep the natural tail in the segment.
            self.current.extend_from_slice(chunk);
            self.silence_samples += chunk.len();
            if self.silence_samples >= self.silence_samples_needed {
                let seg = std::mem::take(&mut self.current);
                self.has_spoken = false;
                self.silence_samples = 0;
                SegmenterEvent::SegmentClosed(seg)
            } else {
                SegmenterEvent::None
            }
        } else {
            // Leading silence: ignore.
            SegmenterEvent::None
        }
    }

    /// Flush buffered speech (e.g. on manual stop). Returns `None` if empty.
    pub fn flush(&mut self) -> Option<Vec<f32>> {
        self.has_spoken = false;
        self.silence_samples = 0;
        if self.current.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.current))
        }
    }
}

fn rms(chunk: &[f32]) -> f32 {
    if chunk.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = chunk.iter().map(|&s| s * s).sum();
    (sum_sq / chunk.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loud(n: usize) -> Vec<f32> { vec![0.5; n] }
    fn quiet(n: usize) -> Vec<f32> { vec![0.0; n] }

    fn closed_len(ev: SegmenterEvent) -> usize {
        match ev {
            SegmenterEvent::SegmentClosed(s) => s.len(),
            SegmenterEvent::None => panic!("expected SegmentClosed"),
        }
    }

    #[test]
    fn leading_silence_is_ignored() {
        // threshold 0.01, 100 ms silence @ 16 kHz = 1600 samples needed
        let mut seg = SpeechSegmenter::new(0.01, 100, 16_000);
        assert!(matches!(seg.push(&quiet(1600)), SegmenterEvent::None));
        assert!(seg.flush().is_none());
    }

    #[test]
    fn no_close_while_speaking() {
        let mut seg = SpeechSegmenter::new(0.01, 100, 16_000);
        assert!(matches!(seg.push(&loud(1600)), SegmenterEvent::None));
        assert!(matches!(seg.push(&loud(1600)), SegmenterEvent::None));
    }

    #[test]
    fn speech_then_silence_closes_segment_including_tail() {
        let mut seg = SpeechSegmenter::new(0.01, 100, 16_000);
        assert!(matches!(seg.push(&loud(1600)), SegmenterEvent::None));
        // 1600 samples of silence reaches the 1600-sample threshold -> closes.
        let len = closed_len(seg.push(&quiet(1600)));
        assert_eq!(len, 3200); // speech (1600) + trailing silence (1600)
    }

    #[test]
    fn flush_returns_buffered_speech_when_pause_too_short() {
        let mut seg = SpeechSegmenter::new(0.01, 100, 16_000);
        seg.push(&loud(1600));
        seg.push(&quiet(800)); // below the 1600 threshold -> no close
        let flushed = seg.flush().expect("speech buffered");
        assert_eq!(flushed.len(), 2400);
        assert!(seg.flush().is_none()); // empty after flush
    }
}
```

- [ ] **Step 2: Run tests to verify they fail (red) or compile-fail, then implement is already inline**

Run: `cd src-tauri && cargo test --lib streaming::segmenter -- --nocapture`
Expected: PASS (implementation and tests are written together here; this is a pure module so they pass immediately). If any assert fails, the segmenter logic — not the test — is wrong; fix the logic.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/stt/streaming/segmenter.rs
git commit -m "feat(live): RMS speech segmenter (cuts only in silence)"
```

---

### Task 3: VadSegmentedStrategy

**Files:**
- Create/replace: `src-tauri/src/stt/streaming/vad_segmented.rs`
- Test: inline (with a `MockEngine`)

- [ ] **Step 1: Write the strategy and its failing tests**

Replace the contents of `src-tauri/src/stt/streaming/vad_segmented.rs` with:

```rust
use std::sync::Arc;
use crate::error::AppError;
use crate::stt::traits::AsrEngine;
use super::segmenter::{SegmenterEvent, SpeechSegmenter};
use super::{StreamEvent, StreamSink, StreamingStrategy};

/// Simplest live strategy: segment on end-of-speech silence (no word ever
/// bisected), then batch-decode the segment with any `AsrEngine`. Emits one
/// `Committed` per utterance and a `Final` on `finish`.
pub struct VadSegmentedStrategy {
    engine: Arc<dyn AsrEngine>,
    segmenter: SpeechSegmenter,
    language: Option<String>,
    committed: String,
}

impl VadSegmentedStrategy {
    pub fn new(
        engine: Arc<dyn AsrEngine>,
        threshold: f32,
        silence_ms: u32,
        language: Option<String>,
    ) -> Self {
        Self {
            engine,
            segmenter: SpeechSegmenter::new(threshold, silence_ms, 16_000),
            language,
            committed: String::new(),
        }
    }

    fn transcribe_segment(&mut self, seg: &[f32], sink: &StreamSink) {
        match self.engine.transcribe(seg, self.language.as_deref()) {
            Ok(text) => {
                let t = text.trim();
                if t.is_empty() {
                    return;
                }
                let delta = if self.committed.is_empty() {
                    t.to_string()
                } else {
                    format!(" {}", t)
                };
                self.committed.push_str(&delta);
                let _ = sink.send(StreamEvent::Committed {
                    delta,
                    full: self.committed.clone(),
                });
            }
            Err(e) => {
                let _ = sink.send(StreamEvent::Error {
                    reason: e.to_string(),
                    recoverable: true,
                });
            }
        }
    }
}

impl StreamingStrategy for VadSegmentedStrategy {
    fn push_audio(&mut self, samples: &[f32], sink: &StreamSink) -> Result<(), AppError> {
        if let SegmenterEvent::SegmentClosed(seg) = self.segmenter.push(samples) {
            self.transcribe_segment(&seg, sink);
        }
        Ok(())
    }

    fn finish(&mut self, sink: &StreamSink) -> Result<(), AppError> {
        if let Some(seg) = self.segmenter.flush() {
            self.transcribe_segment(&seg, sink);
        }
        let _ = sink.send(StreamEvent::Final { text: self.committed.clone() });
        Ok(())
    }

    fn reset(&mut self) {
        self.committed.clear();
        let _ = self.segmenter.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::traits::ModelFormat;

    struct MockEngine {
        text: String,
    }
    impl AsrEngine for MockEngine {
        fn transcribe(&self, _s: &[f32], _l: Option<&str>) -> Result<String, AppError> {
            Ok(self.text.clone())
        }
        fn display_name(&self) -> &str { "mock" }
        fn model_format(&self) -> ModelFormat { ModelFormat::GgmlBin }
    }

    struct ErrEngine;
    impl AsrEngine for ErrEngine {
        fn transcribe(&self, _s: &[f32], _l: Option<&str>) -> Result<String, AppError> {
            Err(AppError::Model("boom".into()))
        }
        fn display_name(&self) -> &str { "err" }
        fn model_format(&self) -> ModelFormat { ModelFormat::GgmlBin }
    }

    fn loud(n: usize) -> Vec<f32> { vec![0.5; n] }
    fn quiet(n: usize) -> Vec<f32> { vec![0.0; n] }

    // Drives the segmenter to close exactly one segment.
    fn feed_one_utterance(s: &mut VadSegmentedStrategy, sink: &StreamSink) {
        s.push_audio(&loud(1600), sink).unwrap();
        s.push_audio(&quiet(1600), sink).unwrap(); // closes the segment
    }

    #[test]
    fn closed_segment_emits_committed() {
        let engine = Arc::new(MockEngine { text: "hello".into() });
        let mut s = VadSegmentedStrategy::new(engine, 0.01, 100, None);
        let (tx, rx) = crossbeam_channel::unbounded();
        feed_one_utterance(&mut s, &tx);
        assert_eq!(
            rx.try_recv().unwrap(),
            StreamEvent::Committed { delta: "hello".into(), full: "hello".into() }
        );
    }

    #[test]
    fn two_segments_accumulate_with_space_delta() {
        let engine = Arc::new(MockEngine { text: "world".into() });
        let mut s = VadSegmentedStrategy::new(engine, 0.01, 100, None);
        let (tx, rx) = crossbeam_channel::unbounded();
        feed_one_utterance(&mut s, &tx); // committed = "world"
        feed_one_utterance(&mut s, &tx); // committed = "world world"
        let _first = rx.try_recv().unwrap();
        assert_eq!(
            rx.try_recv().unwrap(),
            StreamEvent::Committed { delta: " world".into(), full: "world world".into() }
        );
    }

    #[test]
    fn finish_emits_final_with_full_committed() {
        let engine = Arc::new(MockEngine { text: "hello".into() });
        let mut s = VadSegmentedStrategy::new(engine, 0.01, 100, None);
        let (tx, rx) = crossbeam_channel::unbounded();
        feed_one_utterance(&mut s, &tx);
        let _committed = rx.try_recv().unwrap();
        s.finish(&tx).unwrap();
        assert_eq!(rx.try_recv().unwrap(), StreamEvent::Final { text: "hello".into() });
    }

    #[test]
    fn engine_error_emits_recoverable_error() {
        let engine = Arc::new(ErrEngine);
        let mut s = VadSegmentedStrategy::new(engine, 0.01, 100, None);
        let (tx, rx) = crossbeam_channel::unbounded();
        feed_one_utterance(&mut s, &tx);
        assert_eq!(
            rx.try_recv().unwrap(),
            StreamEvent::Error { reason: "Model error: boom".into(), recoverable: true }
        );
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cd src-tauri && cargo test --lib streaming::vad_segmented -- --nocapture`
Expected: PASS for all four tests. If `engine_error_emits_recoverable_error` fails on the reason string, adjust the expected to match `AppError::Model`'s `Display` (`"Model error: boom"` per `error.rs`).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/stt/streaming/vad_segmented.rs
git commit -m "feat(live): VAD-segmented strategy (batch decode per utterance)"
```

---

### Task 4: Full module green + frontend lint unaffected

**Files:** none (verification only)

- [ ] **Step 1: Run the whole streaming test module**

Run: `cd src-tauri && cargo test --lib streaming -- --nocapture`
Expected: all tests across `mod.rs`, `segmenter.rs`, `vad_segmented.rs` PASS.

- [ ] **Step 2: Confirm the frontend type-check is untouched**

Run: `cd /Users/woro/Documents/simplevoice && pnpm lint`
Expected: PASS (no frontend changes in Faza 0a).

- [ ] **Step 3: Commit (if anything was adjusted)**

```bash
git add -A && git commit -m "test(live): streaming core green" || echo "nothing to commit"
```

---

## Self-Review

- **Spec coverage:** Implements LIVE_TRANSCRIPTION.md §3.3 (`StreamEvent`/`StreamSink`/`StreamingStrategy`), §3.6 `VadSegmentedStrategy`, §3.8 RMS-VAD baseline (Silero deferred to a later refinement, per §3.8 fallback), §3.5 "cut only in silence" invariant, §6.1 unit tests (segmenter boundaries, committed/delta accumulation, error path). Not in 0a (by decomposition): audio tap (§3.7), controller/commands (§3.9–3.10), frontend (§3.11) — these are Faza 0b/0c.
- **Placeholder scan:** No TBD/TODO; every step has runnable code and an exact command. Task 1 Step 3 intentionally creates brief stubs only to satisfy the `mod` declarations, immediately replaced by Tasks 2-3.
- **Type consistency:** `StreamEvent::Committed { delta, full }` used identically in `mod.rs`, `vad_segmented.rs`, and all tests. `SegmenterEvent::{None, SegmentClosed}`, `SpeechSegmenter::{new, push, flush}`, `VadSegmentedStrategy::new(engine, threshold, silence_ms, language)` consistent across tasks. `AsrEngine` mock implements exactly the required trait methods (`transcribe`, `display_name`, `model_format`; defaults cover the rest).

## Next plans (after 0a is green)

- **Faza 0b — Wiring:** `StreamingController` (worker thread + bounded channel + Tauri event forwarder), `AudioState.{stream_tx, live_mode_active}` fan-out in `audio.rs` consumer loop, commands (`set_live_transcription_enabled`, `start/stop_live_session`), `toggle_recording` routing behind the config flag, `is_recording_allowed` guard reuse.
- **Faza 0c — Frontend:** live-text zone in `RecordingWindowView.tsx` (committed/tentative `useState`), event listeners, settings toggle in `SettingsView.tsx`, `ConfigContext` field, i18n keys (`en`/`de`/`pl`).
