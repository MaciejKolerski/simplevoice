//! Splits long 16 kHz mono recordings into transcription-sized chunks.
//! Cuts are placed in the quietest pause found in a 45–90 s window, so words
//! are not bisected when a pause exists; chunks that contain no speech at all are dropped
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
                if best.is_none_or(|(b, _)| avg < b) {
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

    #[test]
    fn input_just_above_max_splits_into_two() {
        let input = tone(CHUNK_MAX_SECS + 1);
        let ranges = split_at_silences(&input);
        assert_eq!(ranges.len(), 2);
        assert_invariants(&ranges, input.len());
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[1].end, input.len());
    }

    #[test]
    fn mid_recording_silent_chunk_is_dropped() {
        // 50 s speech, 120 s silence, 50 s speech: the silent middle chunk
        // disappears, so the surviving ranges are non-contiguous.
        let mut input = tone(50);
        input.extend(silence(120));
        input.extend(tone(50));
        let ranges = split_at_silences(&input);
        assert_invariants(&ranges, input.len());
        assert!(ranges.len() >= 2, "speech on both sides must survive");
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges.last().unwrap().end, input.len());
        let covered: usize = ranges.iter().map(|r| r.end - r.start).sum();
        assert!(covered < input.len(), "the silent middle must be dropped");
    }
}
