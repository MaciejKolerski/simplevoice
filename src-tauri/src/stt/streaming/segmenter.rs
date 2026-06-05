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
