//! End-to-end check of chunked transcription on a real model and recording.
//! Ignored by default: needs a local GGML model and a WAV, passed via env.
//!
//! SIMPLEVOICE_MODEL=/path/to/ggml-model.bin \
//! SIMPLEVOICE_WAV=/path/to/recording.wav \
//! cargo test --test long_audio -- --ignored --nocapture

use simplevoice_app_lib::stt::SttController;

#[test]
#[ignore = "needs SIMPLEVOICE_MODEL and SIMPLEVOICE_WAV pointing at local files"]
fn chunked_transcription_of_a_long_recording() {
    let model = std::env::var("SIMPLEVOICE_MODEL").expect("SIMPLEVOICE_MODEL not set");
    let wav = std::env::var("SIMPLEVOICE_WAV").expect("SIMPLEVOICE_WAV not set");

    let mut reader = hound::WavReader::open(&wav).expect("open wav");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "test expects 16 kHz input");
    assert_eq!(spec.channels, 1, "test expects mono input");
    let base: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.expect("wav sample") as f32 / i16::MAX as f32)
        .collect();

    // Tile the recording past the old 90 s limit (target >= 3 minutes).
    let copies = (200 * 16_000 / base.len()).max(2) + 1;
    let mut samples = Vec::with_capacity(base.len() * copies);
    for _ in 0..copies {
        samples.extend_from_slice(&base);
    }
    let secs = samples.len() / 16_000;
    println!("input: {} copies, {}:{:02} total", copies, secs / 60, secs % 60);

    let controller = SttController::new();
    controller.load_model(&model, true).expect("load model");

    let mut progress: Vec<(usize, usize)> = Vec::new();
    let started = std::time::Instant::now();
    let out = controller
        .transcribe_with_progress(&samples, None, &mut |done, total| {
            println!("progress {}/{} at {:?}", done, total, started.elapsed());
            progress.push((done, total));
        })
        .expect("transcription");

    println!("text ({} chars): {}", out.text.len(), out.text);

    assert!(out.truncated.is_none(), "no chunk may fail");
    let total = progress.last().expect("progress fired").1;
    assert!(total >= 2, "long input must be chunked, got {} chunk(s)", total);
    let expected: Vec<(usize, usize)> = (1..=total).map(|d| (d, total)).collect();
    assert_eq!(progress, expected, "every chunk must report progress in order");
    assert!(!out.text.trim().is_empty(), "text must not be empty");
}
