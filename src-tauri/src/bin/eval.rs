//! Offline transcription evaluation harness (Etap 0 / H1).
//!
//! SV_EVAL_MANIFEST=/path/to/manifest.json \
//! SIMPLEVOICE_MODEL=/path/to/model \
//! cargo run --bin eval
//!
//! Optional: SV_EVAL_GPU=1, SV_EVAL_OUT=/path/to/results.json

use simplevoice_app_lib::eval::{score_clip, EvalManifest, EvalReport};
use simplevoice_app_lib::stt::SttController;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn main() {
    if let Err(e) = run() {
        eprintln!("eval error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let manifest_path = std::env::var("SV_EVAL_MANIFEST")
        .map_err(|_| "SV_EVAL_MANIFEST not set".to_string())?;
    let model = std::env::var("SIMPLEVOICE_MODEL")
        .map_err(|_| "SIMPLEVOICE_MODEL not set".to_string())?;
    let use_gpu = matches!(std::env::var("SV_EVAL_GPU").as_deref(), Ok("1") | Ok("true"));

    let manifest_path = PathBuf::from(&manifest_path);
    let base_dir = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let out_path = std::env::var("SV_EVAL_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| base_dir.join("eval-results.json"));

    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read manifest {}: {}", manifest_path.display(), e))?;
    let manifest: EvalManifest = serde_json::from_str(&manifest_text)
        .map_err(|e| format!("parse manifest: {}", e))?;

    let controller = SttController::new();
    controller.load_model(&model, use_gpu)?;

    println!("{:<32} {:>6} {:>6} {:>8} {:>9} {:>6}", "clip", "WER", "CER", "audio", "elapsed", "RTF");
    let mut results = Vec::new();
    for clip in &manifest.clips {
        let wav_path = base_dir.join(&clip.wav);
        let samples = match read_wav_16k_mono(&wav_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("skip {}: {}", clip.wav, e);
                continue;
            }
        };
        let audio_secs = samples.len() as f64 / 16_000.0;
        let started = Instant::now();
        let hyp = match controller.transcribe_with_progress(&samples, clip.language.as_deref(), &mut |_, _| {}) {
            Ok(c) => c.text,
            Err(e) => {
                eprintln!("skip {}: transcription failed: {}", clip.wav, e);
                continue;
            }
        };
        let elapsed = started.elapsed();
        let r = score_clip(&clip.wav, &clip.reference, &hyp, audio_secs, elapsed);
        println!(
            "{:<32} {:>6.3} {:>6.3} {:>7.2}s {:>7}ms {:>6.2}",
            r.name, r.wer, r.cer, r.audio_secs, r.elapsed_ms, r.rtf
        );
        results.push(r);
    }

    if results.is_empty() {
        return Err("no clip produced a result".to_string());
    }

    let report = EvalReport::from_results(results);
    println!("\n{}", report.render_table());

    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&out_path, json).map_err(|e| format!("write {}: {}", out_path.display(), e))?;
    println!("wrote {}", out_path.display());
    Ok(())
}

fn read_wav_16k_mono(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 {
        return Err(format!("expected 16 kHz, got {} Hz", spec.sample_rate));
    }
    if spec.channels != 1 {
        return Err(format!("expected mono, got {} channels", spec.channels));
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?,
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?,
    };
    Ok(samples)
}
