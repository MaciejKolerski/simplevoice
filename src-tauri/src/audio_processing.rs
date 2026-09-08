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

pub struct AudioProcessor {
    resampler: SampleRateConverter,
    limiter_gain: f32,
    dc_x: f32,
    dc_y: f32,
}

impl AudioProcessor {
    pub fn new(source_rate: u32) -> Result<Self, String> {
        if !(8_000..=384_000).contains(&source_rate) {
            return Err("errors.unsupported_audio_rate".into());
        }
        Ok(Self {
            resampler: SampleRateConverter::new(source_rate as usize, 16_000)?,
            limiter_gain: 1.0,
            dc_x: 0.0,
            dc_y: 0.0,
        })
    }

    pub fn process(
        &mut self,
        input: &[f32],
        settings: AudioProcessingSettings,
        finish: bool,
    ) -> Result<(Vec<f32>, MicrophoneLevel), String> {
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
        let mut output = self.resampler.push(&input, finish)?;
        let gain = 10.0_f32.powf(settings.microphone_gain_db / 20.0);
        for sample in &mut output {
            let filtered = *sample - self.dc_x + 0.995 * self.dc_y;
            self.dc_x = *sample;
            self.dc_y = filtered;
            *sample = filtered * gain;
        }
        let peak = output
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        let required = (0.98 / peak.max(0.000001)).min(1.0);
        let release = 1.0 - (-(output.len() as f32) / 3200.0).exp();
        self.limiter_gain = required.min(self.limiter_gain + (1.0 - self.limiter_gain) * release);
        level.limited = self.limiter_gain < 0.999;
        for sample in &mut output {
            *sample *= self.limiter_gain;
            level.peak = level.peak.max(sample.abs());
            level.rms += *sample * *sample;
        }
        level.rms = (level.rms / output.len().max(1) as f32).sqrt();
        Ok((output, level))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: usize, count: usize, amplitude: f32) -> Vec<f32> {
        (0..count)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * amplitude)
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
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
