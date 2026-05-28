use simplevoice_app_lib::stt::candle::whisper::CandleWhisperEngine;
use simplevoice_app_lib::stt::traits::AsrEngine;
use std::path::Path;

fn main() {
    println!("Testing CandleWhisperEngine with language auto-detect...");

    let home = std::env::var("HOME").unwrap_or_default();
    let model_dir_str = std::env::var("WHISPER_MODEL_DIR").unwrap_or_else(|_| {
        if !home.is_empty() {
            format!("{}/Library/Application Support/com.woro.simplevoice/models/whisper-tiny-hf", home)
        } else {
            "./whisper-tiny-hf".to_string()
        }
    });
    let model_dir = Path::new(&model_dir_str);
    if !model_dir.exists() {
        println!("Error: Model directory does not exist at {:?}", model_dir);
        return;
    }

    println!("Initializing engine...");
    let engine: CandleWhisperEngine = match CandleWhisperEngine::initialize(model_dir, false) {
        Ok(eng) => eng,
        Err(e) => {
            println!("Initialization failed: {:?}", e);
            return;
        }
    };
    println!("Engine initialized successfully!");

    println!("Creating 3 seconds of dummy audio (16kHz)...");
    let samples = vec![0.0f32; 16000 * 3];

    println!("Transcribing with auto-detect...");
    match engine.transcribe(&samples, None) {
        Ok(text) => {
            println!("Transcription success! Result text: {:?}", text);
        }
        Err(e) => {
            println!("Transcription failed: {:?}", e);
        }
    }
}
