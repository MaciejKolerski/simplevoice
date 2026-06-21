//! LocalAgreement-2 live strategy: re-decode the growing utterance buffer every
//! `min_chunk_ms` and commit only words that have stabilized across two decodes.
//! Works with any batch `AsrEngine` (uses `transcribe()` + whitespace split — no
//! word timestamps required). The audio buffer is only ever reset at a whole
//! end-of-speech pause or a hard cap, never mid-word, so a word is never split.

use std::sync::Arc;

use crate::error::AppError;
use crate::stt::traits::AsrEngine;

use super::segmenter::{SegmenterEvent, SpeechSegmenter};
use super::stabilizer::Stabilizer;
use super::words::{join_words, split_words};
use super::{StreamEvent, StreamSink, StreamingStrategy};

const SAMPLE_RATE: usize = 16_000;
const SAMPLES_PER_MS: usize = SAMPLE_RATE / 1000;

pub struct LocalAgreementStrategy {
    engine: Arc<dyn AsrEngine>,
    language: Option<String>,
    segmenter: SpeechSegmenter,
    stab: Stabilizer,
    /// Finalized words from previous utterances this session.
    session: Vec<String>,
    /// Last emitted committed text, for computing the append-only delta.
    last_full: String,
    min_chunk_samples: usize,
    cap_samples: usize,
    since_decode: usize,
}

impl LocalAgreementStrategy {
    pub fn new(
        engine: Arc<dyn AsrEngine>,
        threshold: f32,
        silence_ms: u32,
        min_chunk_ms: u32,
        language: Option<String>,
        cap_secs: u32,
    ) -> Self {
        Self {
            engine,
            language,
            segmenter: SpeechSegmenter::new(threshold, silence_ms, SAMPLE_RATE as u32),
            stab: Stabilizer::new(),
            session: Vec::new(),
            last_full: String::new(),
            min_chunk_samples: (min_chunk_ms as usize) * SAMPLES_PER_MS,
            cap_samples: (cap_secs.clamp(5, 120) as usize) * SAMPLE_RATE,
            since_decode: 0,
        }
    }

    fn full_text(&self) -> String {
        let mut all = self.session.clone();
        all.extend_from_slice(self.stab.committed());
        join_words(&all)
    }

    fn decode(&self, audio: &[f32]) -> Result<Vec<String>, AppError> {
        let text = self.engine.transcribe(audio, self.language.as_deref())?;
        Ok(split_words(&text))
    }

    /// Emit a Committed event for any newly appended committed text, plus a
    /// Partial for the tentative tail. Committed text is append-only, so the
    /// delta is just the new suffix (sliced at a space => UTF-8 safe).
    fn emit_state(&mut self, sink: &StreamSink, tentative: &[String]) {
        let full = self.full_text();
        if full.len() > self.last_full.len() {
            let delta = full[self.last_full.len()..].to_string();
            self.last_full = full.clone();
            let _ = sink.send(StreamEvent::Committed { delta, full });
        }
        let _ = sink.send(StreamEvent::Partial { text: join_words(tentative) });
    }

    /// Finalize a closed utterance: decode it once more, commit everything, move
    /// the words into the session accumulator, and reset for the next utterance.
    fn finalize_utterance(&mut self, audio: &[f32], sink: &StreamSink) {
        if !audio.is_empty() {
            match self.decode(audio) {
                Ok(hyp) => {
                    self.stab.observe(&hyp);
                }
                Err(e) => {
                    let _ = sink.send(StreamEvent::Error {
                        reason: e.to_string(),
                        recoverable: true,
                    });
                }
            }
        }
        self.stab.flush();
        self.session.extend(self.stab.committed().iter().cloned());
        self.stab = Stabilizer::new();
        self.since_decode = 0;
        self.emit_state(sink, &[]);
    }
}

impl StreamingStrategy for LocalAgreementStrategy {
    fn push_audio(&mut self, samples: &[f32], sink: &StreamSink) -> Result<(), AppError> {
        match self.segmenter.push(samples) {
            SegmenterEvent::SegmentClosed(seg) => {
                self.finalize_utterance(&seg, sink);
            }
            SegmenterEvent::None => {
                self.since_decode += samples.len();
                if self.segmenter.pending().len() >= self.cap_samples {
                    // Bound re-decode cost on a pauseless monologue: force-finalize.
                    if let Some(seg) = self.segmenter.flush() {
                        self.finalize_utterance(&seg, sink);
                    }
                } else if self.since_decode >= self.min_chunk_samples {
                    self.since_decode = 0;
                    let pending = self.segmenter.pending();
                    if !pending.is_empty() {
                        match self.decode(pending) {
                            Ok(hyp) => {
                                let tentative = self.stab.observe(&hyp);
                                self.emit_state(sink, &tentative);
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
            }
        }
        Ok(())
    }

    fn finish(&mut self, sink: &StreamSink) -> Result<(), AppError> {
        if let Some(seg) = self.segmenter.flush() {
            self.finalize_utterance(&seg, sink);
        } else {
            self.stab.flush();
            self.session.extend(self.stab.committed().iter().cloned());
        }
        let _ = sink.send(StreamEvent::Final { text: join_words(&self.session) });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::traits::ModelFormat;

    /// Simulates a progressive decoder: returns the first N words of `script`,
    /// where N grows with the buffer length (one extra word per `step` samples).
    struct ProgressiveEngine {
        script: Vec<String>,
        step: usize,
    }
    impl AsrEngine for ProgressiveEngine {
        fn transcribe(&self, samples: &[f32], _l: Option<&str>) -> Result<String, AppError> {
            let n = (samples.len() / self.step).clamp(0, self.script.len());
            Ok(self.script[..n].join(" "))
        }
        fn display_name(&self) -> &str { "progressive" }
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

    fn drain(rx: &crossbeam_channel::Receiver<StreamEvent>) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            out.push(ev);
        }
        out
    }

    fn last_committed_full(events: &[StreamEvent]) -> Option<String> {
        events.iter().rev().find_map(|e| match e {
            StreamEvent::Committed { full, .. } => Some(full.clone()),
            _ => None,
        })
    }

    #[test]
    fn commits_words_live_while_speaking_then_finalizes() {
        // min_chunk = 1600 samples; one new word per 1600 samples of buffer.
        let engine = Arc::new(ProgressiveEngine {
            script: vec!["alpha".into(), "beta".into(), "gamma".into(), "delta".into()],
            step: 1600,
        });
        let mut s = LocalAgreementStrategy::new(engine, 0.01, 100, 100, None, 20);
        let (tx, rx) = crossbeam_channel::unbounded();

        // Feed 4 loud chunks of 1600 samples each (no pause -> stays one utterance).
        for _ in 0..4 {
            s.push_audio(&loud(1600), &tx).unwrap();
        }
        let events = drain(&rx);
        // By now several words have stabilized and been committed live.
        let full = last_committed_full(&events).expect("some words committed live");
        assert!(full.starts_with("alpha"), "got: {full}");
        assert!(full.contains("beta"), "expected live commit of earlier words, got: {full}");

        // Finish flushes the rest.
        s.finish(&tx).unwrap();
        let tail = drain(&rx);
        let final_text = tail.iter().rev().find_map(|e| match e {
            StreamEvent::Final { text } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(final_text.as_deref(), Some("alpha beta gamma delta"));
    }

    #[test]
    fn committed_deltas_concatenate_to_the_full_text() {
        let engine = Arc::new(ProgressiveEngine {
            script: vec!["one".into(), "two".into(), "three".into()],
            step: 1600,
        });
        let mut s = LocalAgreementStrategy::new(engine, 0.01, 100, 100, None, 20);
        let (tx, rx) = crossbeam_channel::unbounded();
        for _ in 0..3 {
            s.push_audio(&loud(1600), &tx).unwrap();
        }
        s.finish(&tx).unwrap();

        let events = drain(&rx);
        let mut assembled = String::new();
        for e in &events {
            if let StreamEvent::Committed { delta, full } = e {
                assembled.push_str(delta);
                assert_eq!(&assembled, full, "delta stream must equal full text");
            }
        }
        assert_eq!(assembled, "one two three");
    }

    #[test]
    fn pause_finalizes_an_utterance_and_text_accumulates_across_two() {
        let engine = Arc::new(ProgressiveEngine {
            script: vec!["hello".into()],
            step: 1600,
        });
        let mut s = LocalAgreementStrategy::new(engine, 0.01, 100, 100, None, 20);
        let (tx, rx) = crossbeam_channel::unbounded();

        // Utterance 1: one word, then a 1600-sample silence closes it (100ms @16k).
        s.push_audio(&loud(1600), &tx).unwrap();
        s.push_audio(&quiet(1600), &tx).unwrap(); // SegmentClosed -> finalize

        // Utterance 2: same word again.
        s.push_audio(&loud(1600), &tx).unwrap();
        s.finish(&tx).unwrap();

        let events = drain(&rx);
        let final_text = events.iter().rev().find_map(|e| match e {
            StreamEvent::Final { text } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(final_text.as_deref(), Some("hello hello"));
    }

    #[test]
    fn engine_error_is_recoverable() {
        let mut s = LocalAgreementStrategy::new(Arc::new(ErrEngine), 0.01, 100, 100, None, 20);
        let (tx, rx) = crossbeam_channel::unbounded();
        s.push_audio(&loud(1600), &tx).unwrap();
        let events = drain(&rx);
        assert!(events.iter().any(|e| matches!(
            e,
            StreamEvent::Error { recoverable: true, .. }
        )));
    }
}
