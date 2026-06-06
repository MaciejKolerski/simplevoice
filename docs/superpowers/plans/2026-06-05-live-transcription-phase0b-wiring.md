# Live Transcription — Faza 0b: Backend Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Wire the Faza 0a streaming core into the running backend — a `StreamingController` worker that maps `StreamEvent`s to Tauri events, an audio fan-out tap, and recording-flow routing — all **dormant by default** behind a `live_transcription_enabled` config flag, so batch behavior is byte-for-byte unchanged until Faza 0c flips the flag from the UI.

**Architecture:** `StreamingController` (managed state) owns a worker thread fed by a bounded `Sender<Vec<f32>>` installed on `AudioState.stream_tx`. The existing `audio.rs` consumer thread fans out each drained chunk to that sender (non-blocking `try_send`) and skips VAD auto-stop while `live_mode_active`. On record start/stop, lib.rs helpers (`begin_live_session`/`end_live_session`) start/finish the controller only when the flag is set and a local engine is loaded. The worker emits `transcription-partial|committed|final|error` events.

**Tech Stack:** Rust, `crossbeam-channel` (bounded), Tauri `Emitter`, existing `VadSegmentedStrategy`/`StreamEvent` from Faza 0a. Pure event-mapping is unit-tested; thread/Tauri glue is compile- + lint-verified, with documented manual `pnpm tauri dev` steps.

---

## File Structure

- Create: `src-tauri/src/stt/streaming/controller.rs` — `StreamingController`, `event_payload` mapping (+ unit test).
- Modify: `src-tauri/src/stt/streaming/mod.rs` — `pub mod controller;` + re-export `StreamingController`.
- Modify: `src-tauri/src/audio.rs` — `AudioState.{stream_tx, live_mode_active}`, init, `set_stream_tx`/`set_live_mode`, consumer fan-out + auto-stop guard.
- Modify: `src-tauri/src/lib.rs` — `is_live_transcription_enabled`, `begin_live_session`/`end_live_session`, routing in `start_recording`/`stop_recording`/`toggle_recording`, `.manage(StreamingController::new())`.

---

### Task 1: StreamingController + event mapping

**Files:**
- Create: `src-tauri/src/stt/streaming/controller.rs`
- Modify: `src-tauri/src/stt/streaming/mod.rs`

- [ ] **Step 1: Write `controller.rs` with a unit-tested pure mapping**

Create `src-tauri/src/stt/streaming/controller.rs`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender};
use tauri::{AppHandle, Emitter};

use super::{StreamEvent, StreamingStrategy};

/// Pure mapping from a strategy event to a Tauri (event-name, JSON payload).
/// Kept free of `AppHandle` so it is unit-testable.
pub fn event_payload(ev: &StreamEvent) -> (&'static str, serde_json::Value) {
    match ev {
        StreamEvent::Partial { text } => (
            "transcription-partial",
            serde_json::json!({ "text": text }),
        ),
        StreamEvent::Committed { delta, full } => (
            "transcription-committed",
            serde_json::json!({ "delta": delta, "full": full }),
        ),
        StreamEvent::Final { text } => (
            "transcription-final",
            serde_json::json!({ "text": text }),
        ),
        StreamEvent::Error { reason, recoverable } => (
            "transcription-error",
            serde_json::json!({ "reason": reason, "recoverable": recoverable }),
        ),
    }
}

struct Session {
    audio_tx: Sender<Vec<f32>>,
    stop: std::sync::Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

/// Owns the live worker thread. Managed Tauri state.
pub struct StreamingController {
    session: Mutex<Option<Session>>,
}

impl StreamingController {
    pub fn new() -> Self {
        Self { session: Mutex::new(None) }
    }

    pub fn is_active(&self) -> bool {
        self.session.lock().unwrap().is_some()
    }

    /// Start a live session. Returns the `Sender` to install on
    /// `AudioState.stream_tx`. Finalizes any prior session first.
    pub fn start(
        &self,
        app: AppHandle,
        mut strategy: Box<dyn StreamingStrategy>,
    ) -> Sender<Vec<f32>> {
        self.finish();

        let (audio_tx, audio_rx) = bounded::<Vec<f32>>(16);
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();

        let handle = std::thread::spawn(move || {
            let (sink, sink_rx) = bounded::<StreamEvent>(64);
            loop {
                if stop_thread.load(Ordering::Relaxed) {
                    break;
                }
                match audio_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(chunk) => {
                        let _ = strategy.push_audio(&chunk, &sink);
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                drain_and_emit(&app, &sink_rx);
            }
            let _ = strategy.finish(&sink);
            drain_and_emit(&app, &sink_rx);
        });

        let mut g = self.session.lock().unwrap();
        *g = Some(Session { audio_tx: audio_tx.clone(), stop, handle: Some(handle) });
        audio_tx
    }

    /// Finish the active session (graceful): signal stop, drop the audio
    /// sender so the worker unblocks, then join.
    pub fn finish(&self) {
        let session = self.session.lock().unwrap().take();
        if let Some(mut s) = session {
            s.stop.store(true, Ordering::Relaxed);
            drop(s.audio_tx);
            if let Some(h) = s.handle.take() {
                let _ = h.join();
            }
        }
    }
}

fn drain_and_emit(app: &AppHandle, rx: &Receiver<StreamEvent>) {
    while let Ok(ev) = rx.try_recv() {
        let (name, payload) = event_payload(&ev);
        let _ = app.emit(name, payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_committed_to_event_and_payload() {
        let (name, payload) = event_payload(&StreamEvent::Committed {
            delta: " world".into(),
            full: "hello world".into(),
        });
        assert_eq!(name, "transcription-committed");
        assert_eq!(payload, serde_json::json!({ "delta": " world", "full": "hello world" }));
    }

    #[test]
    fn maps_error_with_recoverable_flag() {
        let (name, payload) = event_payload(&StreamEvent::Error {
            reason: "boom".into(),
            recoverable: true,
        });
        assert_eq!(name, "transcription-error");
        assert_eq!(payload, serde_json::json!({ "reason": "boom", "recoverable": true }));
    }
}
```

- [ ] **Step 2: Register and re-export in `mod.rs`**

In `src-tauri/src/stt/streaming/mod.rs`, add after `pub mod vad_segmented;`:

```rust
pub mod controller;
pub use controller::StreamingController;
```

- [ ] **Step 3: Run tests**

Run: `cd src-tauri && cargo test --lib streaming -- --nocapture`
Expected: 9 prior tests + 2 new (`maps_committed_to_event_and_payload`, `maps_error_with_recoverable_flag`) all PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/stt/streaming/controller.rs src-tauri/src/stt/streaming/mod.rs
git commit -m "feat(live): streaming controller (worker thread + Tauri event mapping)"
```

---

### Task 2: Audio fan-out tap

**Files:**
- Modify: `src-tauri/src/audio.rs`

- [ ] **Step 1: Add the import and new `AudioState` fields**

In `src-tauri/src/audio.rs`, change the top imports (line 1-4) to add crossbeam's `Sender`:

```rust
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::Sender;
use ringbuf::{storage::Heap, traits::*, SharedRb};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
```

In `struct AudioState` (after `pub cached_devices: Vec<String>,`), add:

```rust
    /// When `Some`, the consumer thread fans out drained chunks to a live
    /// streaming session. Installed/cleared by the StreamingController wiring.
    pub stream_tx: Option<Sender<Vec<f32>>>,
    /// When true, VAD does NOT auto-stop the recording (the live segmenter owns
    /// utterance boundaries; the session ends on manual stop).
    pub live_mode_active: bool,
```

In `AudioController::new()` (the `AudioState { ... }` literal), add after `cached_devices: Vec::new(),`:

```rust
                stream_tx: None,
                live_mode_active: false,
```

- [ ] **Step 2: Add controller helpers**

In `impl AudioController`, add after `set_selected_device` (around line 116):

```rust
    pub fn set_stream_tx(&self, tx: Option<Sender<Vec<f32>>>) {
        self.state.lock().unwrap().stream_tx = tx;
    }

    pub fn set_live_mode(&self, active: bool) {
        self.state.lock().unwrap().live_mode_active = active;
    }
```

- [ ] **Step 3: Read `live_mode_active` in the consumer loop and fan out chunks**

In `start_recording`, the loop-top state read (currently lines 182-190) becomes:

```rust
                let (is_recording, vad_enabled, vad_threshold, vad_silence_duration_ms, live_mode_active) = {
                    let s = state_clone.lock().unwrap();
                    (
                        s.is_recording,
                        s.vad_enabled,
                        s.vad_threshold,
                        s.vad_silence_duration_ms,
                        s.live_mode_active,
                    )
                };
```

Inside the `if read > 0 {` block, immediately after `s.buffer.extend_from_slice(&local_buf[..read]);` (line 214), add the fan-out (non-blocking; the bounded channel drops the oldest pressure via `Full`, never blocks the audio thread):

```rust
                    if let Some(tx) = &s.stream_tx {
                        let _ = tx.try_send(local_buf[..read].to_vec());
                    }
```

- [ ] **Step 4: Guard VAD auto-stop in live mode**

Change `if vad_enabled {` (line 216) to:

```rust
                    if vad_enabled && !live_mode_active {
```

- [ ] **Step 5: Verify it compiles and the streaming tests still pass**

Run: `cd src-tauri && cargo test --lib streaming -- --nocapture`
Expected: PASS (compiles with the new fields; streaming tests unaffected).
Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: `Finished` with no errors (warnings about unused `set_stream_tx`/`set_live_mode` are acceptable until Task 3 uses them).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/audio.rs
git commit -m "feat(live): audio fan-out tap + live-mode VAD guard"
```

---

### Task 3: lib.rs recording-flow routing (dormant behind the flag)

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the config-flag reader (mirrors `is_sound_feedback_enabled`)**

In `src-tauri/src/lib.rs`, directly after the `is_sound_feedback_enabled` function (ends line 207), add:

```rust
/// Reads `live_transcription_enabled` from config.json. Defaults to false, so
/// every live code path is dormant unless the user opts in (Faza 0c UI).
fn is_live_transcription_enabled(app_handle: &tauri::AppHandle) -> bool {
    let app_local_data = match app_handle.path().app_local_data_dir() {
        Ok(dir) => dir,
        Err(_) => return false,
    };
    let config_path = app_local_data.join("config.json");
    let content = match std::fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    serde_json::from_str::<serde_json::Value>(&content)
        .ok()
        .and_then(|v| v.get("live_transcription_enabled").and_then(|b| b.as_bool()))
        .unwrap_or(false)
}
```

- [ ] **Step 2: Add the begin/end session helpers**

Immediately below `is_live_transcription_enabled`, add:

```rust
/// Starts a live session if the flag is set and a local engine is loaded.
/// No-op otherwise (cloud/no-model/flag-off => classic batch behavior).
fn begin_live_session(app: &tauri::AppHandle) {
    if !is_live_transcription_enabled(app) {
        return;
    }
    let config = app.state::<AppConfig>();
    let engine_is_local = config.active.lock().unwrap().engine == "local";
    if !engine_is_local {
        return;
    }
    let stt = app.state::<SttController>();
    let engine = { stt.state.lock().unwrap().engine.clone() };
    let Some(engine) = engine else { return };

    let audio = app.state::<AudioController>();
    let (threshold, silence_ms) = {
        let s = audio.state.lock().unwrap();
        (s.vad_threshold, s.vad_silence_duration_ms)
    };

    let strategy = Box::new(crate::stt::streaming::vad_segmented::VadSegmentedStrategy::new(
        engine, threshold, silence_ms, None,
    ));
    let streaming = app.state::<crate::stt::streaming::StreamingController>();
    let tx = streaming.start(app.clone(), strategy);
    audio.set_stream_tx(Some(tx));
    audio.set_live_mode(true);
}

/// Finishes the active live session (emits `transcription-final`) and clears
/// the audio tap. No-op if no session is active.
fn end_live_session(app: &tauri::AppHandle) {
    let audio = app.state::<AudioController>();
    audio.set_stream_tx(None);
    audio.set_live_mode(false);
    let streaming = app.state::<crate::stt::streaming::StreamingController>();
    streaming.finish();
}
```

- [ ] **Step 3: Route the recording commands**

In `start_recording` (the `#[tauri::command]`, ends line 639), add `begin_live_session(&app_handle);` just before `Ok(())`:

```rust
    controller.start_recording(app_handle.clone(), pause_audio)?;
    play_backend_sound(&app_handle, "start");
    let _ = rebuild_tray_menu(&app_handle);
    update_recording_window_visibility(&app_handle);
    begin_live_session(&app_handle);
    Ok(())
```

In `stop_recording` (ends line 654), add `end_live_session(&app_handle);` right after the `let res = ...` line:

```rust
    let res = controller.stop_recording(&app_handle);
    end_live_session(&app_handle);
    if res.is_ok() {
        play_backend_sound(&app_handle, "stop");
        update_recording_window_visibility(&app_handle);
    }
    let _ = rebuild_tray_menu(&app_handle);
    res
```

- [ ] **Step 4: Route the toggle (shortcut/tray) path**

In `toggle_recording` (lines 969-1001): in the stop branch, after `update_recording_window_visibility(app);` inside the `if let Ok(wav_path)` block, add `end_live_session(app);`. In the start branch, after `update_recording_window_visibility(app);` inside the `if controller.start_recording(...).is_ok()` block, add `begin_live_session(app);`. The branch becomes:

```rust
    if controller.is_recording() {
        if let Ok(wav_path) = controller.stop_recording(app) {
            play_backend_sound(app, "stop");
            let payload = wav_path.unwrap_or_else(|| "Recording stopped".to_string());
            let _ = app.emit("recording-stopped", payload);
            update_recording_window_visibility(app);
            end_live_session(app);
        }
    } else {
        let stt = app.state::<SttController>();
        let config = app.state::<AppConfig>();
        match is_recording_allowed(&config, &stt) {
            Ok(_) => {
                let pause_audio = is_pause_audio_enabled(app);
                if controller.start_recording(app.clone(), pause_audio).is_ok() {
                    play_backend_sound(app, "start");
                    let _ = app.emit("recording-started", ());
                    update_recording_window_visibility(app);
                    begin_live_session(app);
                }
            }
            Err(reason) => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
                let _ = app.emit("recording-failed-to-start", reason);
            }
        }
    }
    let _ = rebuild_tray_menu(app);
```

- [ ] **Step 5: Register the managed state**

In the builder chain, after `.manage(stt::downloader::DownloadRegistry::default())` (line 2354), add:

```rust
        .manage(crate::stt::streaming::StreamingController::new())
```

- [ ] **Step 6: Build + lint**

Run: `cd src-tauri && cargo build 2>&1 | tail -8`
Expected: `Finished` with no errors.
Run: `cd /Users/woro/Documents/simplevoice && pnpm lint`
Expected: PASS (no frontend change).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(live): route recording flow to live session behind config flag"
```

---

### Task 4: Verify dormant-by-default + full green

**Files:** none (verification)

- [ ] **Step 1: Streaming tests + full build green**

Run: `cd src-tauri && cargo test --lib streaming -- --nocapture`
Expected: 11 PASS.
Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: `Finished`.

- [ ] **Step 2: Confirm default behavior unchanged (flag off)**

Manual reasoning check (document in commit/PR): with no `live_transcription_enabled` key in config.json, `begin_live_session` returns immediately; `stream_tx` stays `None`; the consumer never fans out; `live_mode_active` stays false so VAD auto-stop is unchanged. Batch path is byte-for-byte identical.

- [ ] **Step 3 (optional manual): live smoke test**

With a local Whisper model loaded, set `"live_transcription_enabled": true` in `<app_local_data>/config.json`, run `pnpm tauri dev`, start recording, speak with a pause; watch the dev console / a temporary `listen('transcription-committed', ...)` for events. (Full UI lands in Faza 0c.)

---

## Self-Review

- **Spec coverage:** Implements LIVE_TRANSCRIPTION.md §3.7 (fan-out tap, `stream_tx`, `live_mode_active`, VAD guard), §3.9 (StreamingController lifecycle, graceful finish via stop+drop+join, routing through `toggle_recording`/`start`/`stop`), §3.10 event names (`transcription-partial|committed|final|error`). Deferred: incremental auto-paste (Faza 2), settings UI + overlay + flag toggle (Faza 0c), model-loading-race i18n error (covered indirectly: `begin_live_session` no-ops when engine is `None`).
- **Placeholder scan:** No TBD/TODO; every step shows the exact code and command. `event_payload` is unit-tested; thread/Tauri glue is compile+lint verified (cannot `cargo test` an `AppHandle` worker without a Tauri runtime) with a documented manual smoke test.
- **Type consistency:** `StreamingController::{new, start, finish, is_active}`, `set_stream_tx(Option<Sender<Vec<f32>>>)`, `set_live_mode(bool)`, `VadSegmentedStrategy::new(engine, threshold, silence_ms, None)` match Faza 0a signatures. `event_payload` returns the same event names the frontend will listen for in Faza 0c.

## Next plan

- **Faza 0c — Frontend:** settings toggle writing `live_transcription_enabled` via `save_config`; `RecordingWindowView` committed/tentative `useState` listening to the four events; `App.tsx` skips `transcribe_audio` when live is enabled (final text arrives via `transcription-final`); i18n keys in `en`/`de`/`pl`.
