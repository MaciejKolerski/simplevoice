# Microphone gain and peak protection

The microphone volume control applies to the shared capture pipeline used by
recordings, local and cloud transcription, live transcription, and the microphone
level test. Its controls are in Settings > Recording.

## Signal path

1. Select a capture format with at least 16-bit precision at 16 kHz when available:
   prefer float32, then other floating-point formats, then wider integer formats.
   Prefer fewer channels at equal precision. Fall back to the device default if
   no such configuration exists, letting Rubato handle its sample rate.
2. Convert the device's sample format to floating point and downmix to mono.
3. Sanitize invalid input and resample to 16 kHz with Rubato.
4. Remove DC offset and apply the requested gain in floating point.
5. Apply the lookahead limiter, then deliver audio to meters, VAD, storage, and
   transcription.

The saved WAV remains mono, 16 kHz, 16-bit PCM. DSP runs on the audio consumer,
outside the microphone callback. Gain and limiting have the same timing for every
input sample rate and callback size because their time base is the 16 kHz stream.

Capture precision must be selected before opening the stream. ALSA/PipeWire can
enumerate 8-bit formats before float32. Selecting the first mono format quantizes
quiet speech before the application receives it; converting those samples to
float and increasing gain also increases the audible quantization error.

## Gain and limiter behavior

| Parameter | Value |
| --- | --- |
| User gain | −20 to +30 dB; default 0 dB |
| Gain conversion | `10^(gain_db / 20)` |
| Gain-change smoothing | 10 ms exponential time constant |
| Limiter ceiling | −1 dBFS PCM sample peak |
| Lookahead delay | 5 ms / 80 output samples |
| Additional peak hold | 10 ms |
| Gain smoothing | Two 41-sample moving averages |
| Limiter release | 120 ms exponential time constant |

The initial gain is applied from the first captured sample. Subsequent slider
changes are smoothed to avoid amplitude discontinuities. Quiet audio below the
limiter threshold retains the requested linear gain and its waveform.

A monotonic peak queue covers the lookahead delay and additional hold. Reduction
is calculated for each sample; two positive-weight moving averages turn it into a
smooth attack. Their combined span equals the audio delay. Every gain value that
contributes to an output sample has seen that sample's peak, so smoothing cannot
allow an impulse past the ceiling. A final numerical guard absorbs rounding
error. The hold and release prevent the gain from following individual low voice
waveform cycles. Queue and smoothing storage are bounded and allocated at startup.

Lookahead does not add leading silence or extend saved recordings. Finalization
drains buffered real samples, including recordings shorter than 5 ms. Manual and
automatic stops flush DSP state before handing off the final recording. Repeated
finalization returns no additional samples.

## Research basis

OBS's [Gain filter](https://obsproject.com/kb/gain-filter) uses a linear amplitude
multiplier. Its [limiter implementation](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-filters/limiter-filter.c)
tracks the signal envelope per sample with attack and release timing. The
[compressor documentation](https://obsproject.com/kb/compressor-filter) describes
the separate roles of level reduction and output gain; its
[limiter documentation](https://obsproject.com/kb/limiter-filter) places peak
protection at the end of the chain.

Simplevoice uses an independently implemented lookahead limiter and adds gain
smoothing. Its settings and algorithm are not an exact reproduction of OBS.

## Regression coverage

`cargo test --locked --lib audio::input_config_tests` checks that device format
enumeration order cannot select 8-bit capture over higher precision, including
integer-only devices, stereo-only high-resolution input, mono preference at equal
precision, and fallback when only low-resolution formats support 16 kHz.

`cargo test --locked --lib audio_processing::tests` covers:

- Identical output for different capture partitions, including one-sample calls.
- Linear +30 dB boost with sufficient headroom and preservation of quiet audio.
- Smoothed slider changes across the full gain range.
- Tonal purity during sustained limiting at 60, 83, 137, 440, 1000, and 3100 Hz.
- Finite, bounded output at 8, 16, 22.05, 44.1, 48, 88.2, 96, 192, and 384 kHz
  input rates, including invalid samples and full-scale transients.
- Exact output duration, very short recordings, tail preservation, repeatable
  finalization, limiter recovery, and bounded buffer capacity.

In the deterministic 60 Hz test at +30 dB, with 37-sample input chunks, the
normalized residual outside the fitted fundamental measured −22.61 dB with the
previous block limiter and −102.92 dB with this implementation. This is a synthetic
DSP regression measurement, not a microphone recording or an OBS comparison.

The ceiling is defined for PCM sample peaks. The tests do not establish an
oversampled true-peak specification, subjective voice quality, or transcription
accuracy. Digital gain also amplifies input noise and cannot recover audio that
was already distorted by the microphone, interface, or driver.
