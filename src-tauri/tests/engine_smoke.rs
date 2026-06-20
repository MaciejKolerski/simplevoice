//! Smoke test: load a real model and transcribe a short clip, asserting non-empty
//! output. Ignored by default; needs local files passed via env.
//!
//! SV_TEST_MODEL=/path/to/ggml-tiny.bin \
//! SV_TEST_WAV=/path/to/short.wav \
//! cargo test --test engine_smoke -- --ignored --nocapture

use simplevoice_app_lib::stt::SttController;

#[test]
#[ignore = "needs SV_TEST_MODEL and SV_TEST_WAV pointing at local files"]
fn loads_model_and_produces_text() {
    let model = std::env::var("SV_TEST_MODEL").expect("SV_TEST_MODEL not set");
    let wav = std::env::var("SV_TEST_WAV").expect("SV_TEST_WAV not set");

    let mut reader = hound::WavReader::open(&wav).expect("open wav");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "test expects 16 kHz input");
    assert_eq!(spec.channels, 1, "test expects mono input");
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.expect("wav sample") as f32 / i16::MAX as f32)
        .collect();

    let controller = SttController::new();
    controller.load_model(&model, false).expect("load model");
    let text = controller.transcribe(&samples, None).expect("transcription");
    assert!(!text.trim().is_empty(), "transcription must not be empty");
}
