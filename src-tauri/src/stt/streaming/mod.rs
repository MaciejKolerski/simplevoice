use crossbeam_channel::Sender;
use crate::error::AppError;

pub mod segmenter;
pub mod vad_segmented;
pub mod words;
pub mod stabilizer;
pub mod local_agreement;
pub mod controller;
pub use controller::StreamingController;

/// Events emitted by a live strategy. Serialized to the frontend in Faza 0b.
/// `Committed.full` is the authoritative committed text (single source of truth);
/// `Committed.delta` is the append-only chunk safe to auto-paste.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Tentative tail - re-rendered whole in the overlay, never pasted.
    Partial { text: String },
    /// Newly stabilized text - append-only, safe to auto-paste.
    Committed { delta: String, full: String },
    /// Utterance/session finalized.
    Final { text: String },
    /// Strategy error. `recoverable = true` => the stream keeps going.
    Error { reason: String, recoverable: bool },
}

/// Channel the strategy emits events on. The bounded channel is created by the
/// controller in Faza 0b; the strategy only holds the `Sender`.
pub type StreamSink = Sender<StreamEvent>;

/// A live transcription strategy. Runs on a dedicated worker thread, so it may
/// block (e.g. call `AsrEngine::transcribe`) directly - no async required.
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
