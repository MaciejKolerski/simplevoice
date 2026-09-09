use rubato::{FftFixedInOut, Resampler};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioProcessingSettings {
    pub microphone_gain_db: f32,
}

impl AudioProcessingSettings {
    pub fn validate(self) -> Result<Self, String> {
        if !self.microphone_gain_db.is_finite()
            || !(-20.0..=30.0).contains(&self.microphone_gain_db)
        {
            return Err("errors.invalid_audio_processing".into());
        }
        Ok(self)
    }

    pub fn from_config(config: &serde_json::Value) -> Result<Self, String> {
        if !config.is_object() {
            return Err("errors.invalid_config".into());
        }
        Self {
            microphone_gain_db: serde_json::from_value(
                config
                    .get("microphone_gain_db")
                    .cloned()
                    .unwrap_or(0.into()),
            )
            .map_err(|_| "errors.invalid_audio_processing".to_string())?,
        }
        .validate()
    }
}

#[derive(Clone, Copy, Default, Serialize)]
pub struct MicrophoneLevel {
    pub input_peak: f32,
    pub rms: f32,
    pub peak: f32,
    pub clipped: bool,
    pub limited: bool,
}

struct SampleRateConverter {
    inner: Option<FftFixedInOut<f32>>,
    pending: VecDeque<f32>,
    skip: usize,
    input_count: usize,
    output_count: usize,
    source_rate: usize,
    target_rate: usize,
}

impl SampleRateConverter {
    fn new(source_rate: usize, target_rate: usize) -> Result<Self, String> {
        let inner = if source_rate == target_rate {
            None
        } else {
            Some(
                FftFixedInOut::new(source_rate, target_rate, source_rate / 100, 1)
                    .map_err(|error| error.to_string())?,
            )
        };
        let skip = inner.as_ref().map_or(0, Resampler::output_delay);
        Ok(Self {
            inner,
            pending: VecDeque::new(),
            skip,
            input_count: 0,
            output_count: 0,
            source_rate,
            target_rate,
        })
    }

    fn push(&mut self, samples: &[f32], finish: bool) -> Result<Vec<f32>, String> {
        self.input_count += samples.len();
        let Some(inner) = &mut self.inner else {
            return Ok(samples.to_vec());
        };
        self.pending.extend(samples);
        let target_count = (self.input_count * self.target_rate).div_ceil(self.source_rate);
        let mut output = Vec::new();
        let needed = inner.input_frames_next();
        while self.pending.len() >= needed || (finish && self.output_count < target_count) {
            if self.pending.len() < needed {
                self.pending.resize(needed, 0.0);
            }
            let input: Vec<f32> = self.pending.drain(..needed).collect();
            let block = inner
                .process(&[input], None)
                .map_err(|error| error.to_string())?;
            let skip = self.skip.min(block[0].len());
            self.skip -= skip;
            let available = &block[0][skip..];
            let count = available
                .len()
                .min(target_count.saturating_sub(self.output_count));
            output.extend_from_slice(&available[..count]);
            self.output_count += count;
        }
        Ok(output)
    }
}

const SAMPLE_RATE: usize = 16_000;
const LOOKAHEAD_SAMPLES: usize = SAMPLE_RATE * 5 / 1000;
const PEAK_HOLD_SAMPLES: usize = SAMPLE_RATE * 10 / 1000;
const GAIN_FILTER_SAMPLES: usize = LOOKAHEAD_SAMPLES / 2 + 1;
const LIMITER_CEILING: f64 = 0.8912509381337456;

struct GainAverage {
    values: [f64; GAIN_FILTER_SAMPLES],
    position: usize,
    sum: f64,
}

impl GainAverage {
    fn new() -> Self {
        Self {
            values: [1.0; GAIN_FILTER_SAMPLES],
            position: 0,
            sum: GAIN_FILTER_SAMPLES as f64,
        }
    }

    fn push(&mut self, gain: f64) -> f64 {
        self.sum += gain - self.values[self.position];
        self.values[self.position] = gain;
        self.position = (self.position + 1) % GAIN_FILTER_SAMPLES;
        if self.position == 0 {
            // Recompute periodically so rounding drift cannot accumulate over long recordings.
            self.sum = self.values.iter().sum();
        }
        (self.sum / GAIN_FILTER_SAMPLES as f64).clamp(0.0, 1.0)
    }
}

struct LookaheadLimiter {
    delay: VecDeque<f64>,
    peaks: VecDeque<(u64, f64)>,
    position: u64,
    envelope_gain: f64,
    release_coefficient: f64,
    smoothing_first: GainAverage,
    smoothing_second: GainAverage,
}

impl LookaheadLimiter {
    fn new() -> Self {
        Self {
            delay: VecDeque::with_capacity(LOOKAHEAD_SAMPLES + 1),
            peaks: VecDeque::with_capacity(LOOKAHEAD_SAMPLES + PEAK_HOLD_SAMPLES + 1),
            position: 0,
            envelope_gain: 1.0,
            release_coefficient: (-1.0 / (SAMPLE_RATE as f64 * 0.120)).exp(),
            smoothing_first: GainAverage::new(),
            smoothing_second: GainAverage::new(),
        }
    }

    fn push(&mut self, sample: f64) -> Option<(f32, bool)> {
        let position = self.position;
        self.position += 1;
        let oldest = position.saturating_sub((LOOKAHEAD_SAMPLES + PEAK_HOLD_SAMPLES) as u64);
        while self.peaks.front().is_some_and(|&(index, _)| index < oldest) {
            self.peaks.pop_front();
        }
        let peak = sample.abs();
        while self.peaks.back().is_some_and(|&(_, value)| value <= peak) {
            self.peaks.pop_back();
        }
        self.peaks.push_back((position, peak));
        let window_peak = self.peaks.front().unwrap().1;
        let required_gain = LIMITER_CEILING / window_peak.max(LIMITER_CEILING);
        self.envelope_gain =
            required_gain.min(1.0 - (1.0 - self.envelope_gain) * self.release_coefficient);

        // Both positive-weight averages span exactly the audio delay. The peak window
        // keeps every contributing gain safe for the delayed sample, even for impulses.
        let gain = self.smoothing_first.push(self.envelope_gain);
        let gain = self.smoothing_second.push(gain);
        self.delay.push_back(sample);
        if self.delay.len() <= LOOKAHEAD_SAMPLES {
            return None;
        }
        let delayed = self.delay.pop_front().unwrap();
        // This bound only absorbs floating-point roundoff; the envelope does the limiting.
        let gain = gain.min(LIMITER_CEILING / delayed.abs().max(LIMITER_CEILING));
        Some(((delayed * gain) as f32, gain < 0.999))
    }

    fn drain(&mut self, mut emit: impl FnMut(f32, bool)) {
        if self.delay.is_empty() {
            return;
        }
        for _ in 0..LOOKAHEAD_SAMPLES {
            if let Some((sample, limited)) = self.push(0.0) {
                emit(sample, limited);
            }
        }
        self.delay.clear();
        self.peaks.clear();
    }
}

pub struct AudioProcessor {
    resampler: SampleRateConverter,
    limiter: LookaheadLimiter,
    microphone_gain: Option<f64>,
    gain_smoothing: f64,
    dc_x: f64,
    dc_y: f64,
    finished: bool,
}

impl AudioProcessor {
    pub fn new(source_rate: u32) -> Result<Self, String> {
        if !(8_000..=384_000).contains(&source_rate) {
            return Err("errors.unsupported_audio_rate".into());
        }
        Ok(Self {
            resampler: SampleRateConverter::new(source_rate as usize, SAMPLE_RATE)?,
            limiter: LookaheadLimiter::new(),
            microphone_gain: None,
            gain_smoothing: 1.0 - (-1.0 / (SAMPLE_RATE as f64 * 0.010)).exp(),
            dc_x: 0.0,
            dc_y: 0.0,
            finished: false,
        })
    }

    pub fn process(
        &mut self,
        input: &[f32],
        settings: AudioProcessingSettings,
        finish: bool,
    ) -> Result<(Vec<f32>, MicrophoneLevel), String> {
        let settings = settings.validate()?;
        if self.finished {
            return if input.is_empty() {
                Ok((Vec::new(), MicrophoneLevel::default()))
            } else {
                Err("Audio processor has already been finalized".into())
            };
        }
        let mut level = MicrophoneLevel::default();
        let input: Vec<f32> = input
            .iter()
            .map(|&sample| {
                level.clipped |= !sample.is_finite() || sample.abs() >= 1.0;
                let sample = if sample.is_finite() {
                    sample.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
                level.input_peak = level.input_peak.max(sample.abs());
                sample
            })
            .collect();
        let resampled = self.resampler.push(&input, finish)?;
        let mut output = Vec::with_capacity(resampled.len() + LOOKAHEAD_SAMPLES);
        let mut energy = 0.0_f64;
        let mut emit = |sample: f32, limited: bool| {
            level.limited |= limited;
            level.peak = level.peak.max(sample.abs());
            energy += (sample as f64).powi(2);
            output.push(sample);
        };
        let target_gain = 10.0_f64.powf(settings.microphone_gain_db as f64 / 20.0);
        for sample in resampled {
            let gain = self.microphone_gain.get_or_insert(target_gain);
            *gain += (target_gain - *gain) * self.gain_smoothing;
            let filtered = sample as f64 - self.dc_x + 0.995 * self.dc_y;
            self.dc_x = sample as f64;
            self.dc_y = filtered;
            if let Some((sample, limited)) = self.limiter.push(filtered * *gain) {
                emit(sample, limited);
            }
        }
        if finish {
            self.limiter.drain(&mut emit);
            self.finished = true;
        }
        level.rms = (energy / output.len().max(1) as f64).sqrt() as f32;
        Ok((output, level))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: usize, count: usize, amplitude: f32) -> Vec<f32> {
        (0..count)
            .map(|i| {
                (i as f64 * 440.0 * std::f64::consts::TAU / rate as f64).sin() as f32 * amplitude
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    fn render(input: &[f32], rate: u32, gain: f32, chunk_size: usize) -> Vec<f32> {
        let mut processor = AudioProcessor::new(rate).unwrap();
        let settings = AudioProcessingSettings {
            microphone_gain_db: gain,
        };
        let mut output = Vec::new();
        for chunk in input.chunks(chunk_size) {
            output.extend(processor.process(chunk, settings, false).unwrap().0);
        }
        output.extend(processor.process(&[], settings, true).unwrap().0);
        output
    }

    #[test]
    fn boost_is_independent_of_capture_chunk_boundaries() {
        for rate in [16_000, 44_100, 48_000] {
            let input: Vec<f32> = (0..rate)
                .map(|i| {
                    let time = i as f64 / rate as f64;
                    let envelope = 0.02 + 0.2 * (time * 13.0).sin().powi(2);
                    (envelope * (time * 173.0 * std::f64::consts::TAU).sin()) as f32
                })
                .collect();
            let reference = render(&input, rate, 30.0, input.len());
            for chunk_size in [1, 37, 137, 160, 320, 1024] {
                let output = render(&input, rate, 30.0, chunk_size);
                assert_eq!(output.len(), reference.len());
                let error = output
                    .iter()
                    .zip(&reference)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0_f32, f32::max);
                assert!(
                    error < 0.000002,
                    "{rate} Hz, {chunk_size}-sample chunks: maximum difference {error}"
                );
            }
        }
    }

    #[test]
    fn sustained_limiting_preserves_tonal_purity() {
        for frequency in [60.0, 83.0, 137.0, 440.0, 1000.0, 3100.0] {
            let input: Vec<f32> = (0..48_000)
                .map(|i| {
                    (0.5 * (i as f64 * frequency * std::f64::consts::TAU / 16_000.0).sin()) as f32
                })
                .collect();
            let output = render(&input, 16_000, 30.0, 37);
            let measured = &output[32_000..];
            let mut sine = 0.0;
            let mut cosine = 0.0;
            for (i, &sample) in measured.iter().enumerate() {
                let phase = i as f64 * frequency * std::f64::consts::TAU / 16_000.0;
                sine += sample as f64 * phase.sin();
                cosine += sample as f64 * phase.cos();
            }
            sine *= 2.0 / measured.len() as f64;
            cosine *= 2.0 / measured.len() as f64;
            let residual: f64 = measured
                .iter()
                .enumerate()
                .map(|(i, &sample)| {
                    let phase = i as f64 * frequency * std::f64::consts::TAU / 16_000.0;
                    (sample as f64 - sine * phase.sin() - cosine * phase.cos()).powi(2)
                })
                .sum();
            let energy: f64 = measured.iter().map(|&sample| (sample as f64).powi(2)).sum();
            let distortion = (residual / energy).sqrt();
            eprintln!(
                "{frequency} Hz: residual {:.2} dB",
                20.0 * distortion.log10()
            );
            assert!(distortion < 0.0002, "{frequency} Hz: residual {distortion}");
        }
    }

    #[test]
    fn moving_the_gain_slider_does_not_create_amplitude_steps() {
        let mut processor = AudioProcessor::new(16_000).unwrap();
        let mut output = Vec::new();
        for gain in [0.0, 30.0, -20.0, 0.0] {
            let input: Vec<f32> = (0..4000)
                .map(|i| {
                    (0.005 * (i as f64 * 100.0 * std::f64::consts::TAU / 16_000.0).cos()) as f32
                })
                .collect();
            output.extend(
                processor
                    .process(
                        &input,
                        AudioProcessingSettings {
                            microphone_gain_db: gain,
                        },
                        false,
                    )
                    .unwrap()
                    .0,
            );
        }
        output.extend(
            processor
                .process(&[], AudioProcessingSettings::default(), true)
                .unwrap()
                .0,
        );
        let largest_step = output
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(largest_step < 0.012, "sample discontinuity: {largest_step}");
        assert_eq!(output.len(), 16_000);
    }

    #[test]
    fn maximum_boost_is_linear_when_there_is_headroom() {
        let input = tone(48_000, 48_000, 0.003);
        let normal = render(&input, 48_000, 0.0, 137);
        let boosted = render(&input, 48_000, 30.0, 137);
        for (normal, boosted) in normal.iter().zip(&boosted) {
            assert!((boosted - normal * 31.622776).abs() < 0.0000002);
        }
        assert!(rms(&boosted) > 0.06);
    }

    #[test]
    fn short_recordings_preserve_the_first_and_last_samples() {
        let settings = AudioProcessingSettings {
            microphone_gain_db: 30.0,
        };
        for count in [1, 7, 79, 80, 81, 160, 337] {
            let mut input = vec![0.0; count];
            input[0] = 0.9;
            input[count - 1] = -0.9;
            let mut processor = AudioProcessor::new(16_000).unwrap();
            let mut output = processor.process(&input, settings, false).unwrap().0;
            assert_eq!(output.len(), count.saturating_sub(80));
            output.extend(processor.process(&[], settings, true).unwrap().0);
            assert_eq!(output.len(), count);
            assert!(output[0].abs() > 0.5);
            assert!(output[count - 1] < -0.5);
            assert!(output.iter().all(|sample| sample.abs() <= 0.891251));
            assert!(processor.process(&[], settings, true).unwrap().0.is_empty());
            assert!(processor.process(&[0.1], settings, false).is_err());
        }
    }

    #[test]
    fn peaks_are_bounded_across_device_rates_and_gain_extremes() {
        for rate in [
            8_000, 16_000, 22_050, 44_100, 48_000, 88_200, 96_000, 192_000, 384_000,
        ] {
            let mut random = 0x5eed_u32;
            let input: Vec<f32> = (0..rate / 5 + 13)
                .map(|i| {
                    random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                    match i % 11 {
                        0 => 1.0,
                        1 => -1.0,
                        2 => f32::NAN,
                        3 => f32::INFINITY,
                        _ => (random as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32,
                    }
                })
                .collect();
            for gain in [-20.0, 0.0, 30.0] {
                let output = render(&input, rate, gain, 113);
                assert_eq!(output.len(), (input.len() * 16_000).div_ceil(rate as usize));
                assert!(
                    output
                        .iter()
                        .all(|sample| sample.is_finite() && sample.abs() <= 0.891251),
                    "rate {rate}, gain {gain}"
                );
            }
        }
    }

    #[test]
    fn limiter_releases_after_a_transient_without_expanding_its_buffers() {
        let mut limiter = LookaheadLimiter::new();
        let delay_capacity = limiter.delay.capacity();
        let peak_capacity = limiter.peaks.capacity();
        let mut output = Vec::new();
        for i in 0..32_000 {
            let sample = if i < 16_000 {
                30.0 * (1.0 - i as f64 / 16_000.0)
            } else {
                0.001
            };
            if let Some((sample, _)) = limiter.push(sample) {
                output.push(sample);
            }
            assert_eq!(limiter.delay.capacity(), delay_capacity);
            assert_eq!(limiter.peaks.capacity(), peak_capacity);
        }
        limiter.drain(|sample, _| output.push(sample));
        assert_eq!(output.len(), 32_000);
        assert!(output.iter().all(|sample| sample.abs() <= 0.891251));
        assert!((output.last().unwrap() - 0.001).abs() < 0.000001);
    }

    #[test]
    fn silence_stays_silent_at_maximum_boost() {
        let mut processor = AudioProcessor::new(48_000).unwrap();
        let (output, level) = processor
            .process(
                &vec![0.0; 48_000],
                AudioProcessingSettings {
                    microphone_gain_db: 30.0,
                },
                true,
            )
            .unwrap();
        assert_eq!(output.len(), 16_000);
        assert!(output.iter().all(|&sample| sample == 0.0));
        assert!(!level.clipped && !level.limited);
        assert_eq!(level.rms, 0.0);
    }

    #[test]
    fn invalid_gain_is_rejected_before_consuming_audio() {
        for gain in [f32::NAN, f32::INFINITY, -21.0, 31.0] {
            let mut processor = AudioProcessor::new(16_000).unwrap();
            assert!(processor
                .process(
                    &[0.1],
                    AudioProcessingSettings {
                        microphone_gain_db: gain
                    },
                    false
                )
                .is_err());
            let (output, _) = processor
                .process(&[0.1], AudioProcessingSettings::default(), true)
                .unwrap();
            assert_eq!(output.len(), 1);
            assert!((output[0] - 0.1).abs() < 0.000001);
        }
    }

    #[test]
    fn streaming_preserves_duration_and_the_last_partial_frame() {
        for rate in [16_000, 22_050, 44_100, 48_000, 96_000] {
            let input = tone(rate, rate / 3 + 7, 0.1);
            let mut processor = AudioProcessor::new(rate as u32).unwrap();
            let mut output = Vec::new();
            for chunk in input.chunks(137) {
                output.extend(
                    processor
                        .process(chunk, AudioProcessingSettings::default(), false)
                        .unwrap()
                        .0,
                );
            }
            output.extend(
                processor
                    .process(&[], AudioProcessingSettings::default(), true)
                    .unwrap()
                    .0,
            );
            assert_eq!(
                output.len(),
                (input.len() * 16_000).div_ceil(rate),
                "rate {rate}"
            );
            assert!(
                rms(&output[output.len() - 100..]) > 0.04,
                "lost the ending at {rate} Hz"
            );
            assert!(output.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
            assert!(processor
                .process(&[], AudioProcessingSettings::default(), true)
                .unwrap()
                .0
                .is_empty());
        }
    }

    #[test]
    fn gain_changes_output_and_limiter_preserves_peak_shape() {
        let input = tone(48_000, 48_000, 0.02);
        let render = |gain| {
            let settings = AudioProcessingSettings {
                microphone_gain_db: gain,
            };
            AudioProcessor::new(48_000)
                .unwrap()
                .process(&input, settings, true)
                .unwrap()
        };
        let (normal, _) = render(0.0);
        let (louder, _) = render(6.0);
        let (quieter, _) = render(-6.0);
        assert!((rms(&louder) / rms(&normal) - 10.0_f32.powf(6.0 / 20.0)).abs() < 0.001);
        assert!((rms(&quieter) / rms(&normal) - 10.0_f32.powf(-6.0 / 20.0)).abs() < 0.001);
        let mut live = AudioProcessor::new(48_000).unwrap();
        let (before, _) = live
            .process(&input, AudioProcessingSettings::default(), false)
            .unwrap();
        let (after, _) = live
            .process(
                &input,
                AudioProcessingSettings {
                    microphone_gain_db: 6.0,
                },
                false,
            )
            .unwrap();
        assert!(
            (rms(&after[1000..]) / rms(&before[1000..]) - 10.0_f32.powf(6.0 / 20.0)).abs() < 0.01
        );
        let (limited, level) = AudioProcessor::new(48_000)
            .unwrap()
            .process(
                &tone(48_000, 48_000, 0.5),
                AudioProcessingSettings {
                    microphone_gain_db: 30.0,
                },
                true,
            )
            .unwrap();
        assert!(level.limited);
        assert!(level.peak <= 0.980001);
        assert!(limited.iter().filter(|s| s.abs() > 0.97).count() < limited.len() / 4);
    }

    #[test]
    fn quiet_audio_is_preserved() {
        let input = tone(48_000, 48_000, 0.0001);
        let (output, _) = AudioProcessor::new(48_000)
            .unwrap()
            .process(&input, AudioProcessingSettings::default(), true)
            .unwrap();
        assert_eq!(output.len(), 16_000);
        for chunk in output.chunks(1000) {
            assert!(rms(chunk) > 0.00005);
        }
    }

    #[test]
    fn settings_and_invalid_samples_are_bounded() {
        for invalid in [
            serde_json::json!([]),
            serde_json::json!({"microphone_gain_db": 31}),
            serde_json::json!({"microphone_gain_db": -21}),
            serde_json::json!({"microphone_gain_db": "loud"}),
        ] {
            assert!(AudioProcessingSettings::from_config(&invalid).is_err());
        }
        assert!(AudioProcessingSettings {
            microphone_gain_db: f32::NAN,
        }
        .validate()
        .is_err());
        let input = [f32::NAN, f32::INFINITY, 0.5, -0.5].repeat(200);
        let (output, level) = AudioProcessor::new(48_000)
            .unwrap()
            .process(&input, AudioProcessingSettings::default(), true)
            .unwrap();
        assert!(level.clipped);
        assert!(output.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    }
}
