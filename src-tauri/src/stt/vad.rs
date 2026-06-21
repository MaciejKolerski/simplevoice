//! Silero-VAD silence/noise trimming (B2), behind the `onnx` feature.
//!
//! This is an opt-in pre-transcription step: it runs the Silero voice-activity
//! detector (via sherpa-onnx) over a finished recording and keeps only the
//! detected speech, concatenated. Leading/trailing silence and gaps of
//! non-speech (room noise, breaths, long pauses) are dropped, which gives the
//! ASR a cleaner, shorter signal. It never *adds* audio, and on any failure the
//! caller falls back to the untrimmed samples — so a missing/incompatible model
//! degrades to "no trimming", never to lost audio.

use std::path::Path;

use sherpa_onnx::{SileroVadModelConfig, TenVadModelConfig, VadModelConfig, VoiceActivityDetector};

/// Audio sample rate the rest of the pipeline runs at (16 kHz mono f32).
const SAMPLE_RATE: i32 = 16_000;
/// Silero processes audio in fixed windows; 512 samples (32 ms @ 16 kHz) is the
/// model's native frame. Feeding it window-sized chunks keeps memory bounded on
/// long recordings (the detector emits/queues segments as it goes).
const WINDOW_SIZE: usize = 512;
/// Keep 100 ms of audio around the speech span so a word's onset/offset is never
/// clipped by the trim.
const PAD_SAMPLES: usize = 1600;

/// Run Silero VAD over `samples` (16 kHz mono f32) and return the span from the
/// first to the last detected speech (plus a little padding) — i.e. trim only
/// the leading/trailing silence, keeping everything in between. Returns `None`
/// when the model can't load, no speech is found, or there's nothing to trim, so
/// callers keep the original audio.
///
/// It deliberately does NOT concatenate speech segments: splicing out internal
/// pauses measurably hurt Whisper accuracy in testing (WER 0.000 -> 0.267 on a
/// pause-heavy clip) by clipping word boundaries and removing the prosody Whisper
/// relies on. Trimming only the outer dead air preserves the transcription.
pub fn trim_to_speech(samples: &[f32], model_path: &Path) -> Option<Vec<f32>> {
    let config = VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model_path.to_string_lossy().into_owned()),
            threshold: 0.5,
            min_silence_duration: 0.25,
            min_speech_duration: 0.10,
            window_size: WINDOW_SIZE as i32,
            max_speech_duration: 20.0,
        },
        ten_vad: TenVadModelConfig::default(),
        sample_rate: SAMPLE_RATE,
        num_threads: 1,
        provider: Some("cpu".to_string()),
        debug: false,
    };

    // 30 s internal buffer is ample: segments are drained every window, and a
    // single utterance is capped at max_speech_duration (20 s) above.
    let vad = VoiceActivityDetector::create(&config, 30.0)?;

    let mut first: Option<usize> = None;
    let mut last_end: usize = 0;

    let mut i = 0;
    while i < samples.len() {
        let end = (i + WINDOW_SIZE).min(samples.len());
        vad.accept_waveform(&samples[i..end]);
        collect_bounds(&vad, &mut first, &mut last_end);
        i = end;
    }
    // Emit any speech still buffered at end-of-input, then fold it in.
    vad.flush();
    collect_bounds(&vad, &mut first, &mut last_end);

    let first = first?;
    if last_end <= first {
        return None;
    }

    // Pad the speech span, clamp to the buffer.
    let start = first.saturating_sub(PAD_SAMPLES);
    let stop = (last_end + PAD_SAMPLES).min(samples.len());
    if start == 0 && stop == samples.len() {
        return None; // no leading/trailing silence to remove
    }
    Some(samples[start..stop].to_vec())
}

/// Fold every queued speech segment's `[start, start+n)` bounds into the running
/// first-start / last-end, then drop the segment.
fn collect_bounds(vad: &VoiceActivityDetector, first: &mut Option<usize>, last_end: &mut usize) {
    while let Some(seg) = vad.front() {
        let s = seg.start().max(0) as usize;
        let n = seg.n().max(0) as usize;
        if first.is_none() {
            *first = Some(s);
        }
        *last_end = (*last_end).max(s + n);
        vad.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn home() -> PathBuf {
        PathBuf::from(std::env::var("HOME").expect("HOME"))
    }

    fn model_path() -> PathBuf {
        std::env::var("SV_SILERO_MODEL")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                home()
                    .join("Library/Application Support/com.woro.simplevoice/models")
                    .join("silero_vad_v4.onnx")
            })
    }

    fn clip_path() -> PathBuf {
        std::env::var("SV_TEST_WAV")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/Users/woro/Documents/Simple/test/output.wav"))
    }

    /// End-to-end check on a real 16 kHz clip. Ignored by default (needs the
    /// Silero model + a wav present); run locally with:
    ///   cargo test --features onnx vad -- --ignored
    #[test]
    #[ignore = "needs silero model + a wav clip on disk"]
    fn trims_real_clip_to_nonempty_subset() {
        let reader = hound::WavReader::open(clip_path()).expect("open wav");
        let samples: Vec<f32> = reader
            .into_samples::<i16>()
            .map(|s| s.expect("sample") as f32 / 32768.0)
            .collect();
        assert!(!samples.is_empty(), "clip should have samples");

        let trimmed = trim_to_speech(&samples, &model_path()).expect("VAD should detect speech");

        assert!(!trimmed.is_empty(), "trimmed audio must be non-empty");
        assert!(
            trimmed.len() <= samples.len(),
            "trimming never adds audio (got {} > {})",
            trimmed.len(),
            samples.len()
        );
        // A clean dictation clip is mostly speech, so we should retain a large
        // fraction rather than collapsing to a few frames.
        assert!(
            trimmed.len() as f32 >= samples.len() as f32 * 0.3,
            "kept too little: {} of {}",
            trimmed.len(),
            samples.len()
        );
        eprintln!(
            "vad trim: {} -> {} samples ({:.1}%)",
            samples.len(),
            trimmed.len(),
            100.0 * trimmed.len() as f32 / samples.len() as f32
        );
    }
}
