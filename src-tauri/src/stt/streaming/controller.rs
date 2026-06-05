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
