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
