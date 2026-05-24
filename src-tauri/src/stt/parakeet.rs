use ort::{inputs, session::Session};
use std::sync::Mutex;

pub struct ParakeetEngine {
    session: Option<Mutex<Session>>,
}

impl ParakeetEngine {
    pub fn new() -> Self {
        Self { session: None }
    }
}

impl super::EngineAdapter for ParakeetEngine {
    fn initialize(&mut self, model_path: &str) -> Result<(), String> {
        let mut builder = Session::builder()
            .map_err(|e| format!("Failed to create ONNX session builder: {}", e))?;

        #[cfg(target_os = "windows")]
        {
            builder = builder
                .with_execution_providers([
                    ort::execution_providers::DirectMLExecutionProvider::default().build(),
                ])
                .map_err(|e| format!("Failed to register DirectML provider: {}", e))?;
            println!("[ParakeetEngine] Registered DirectML provider.");
        }

        #[cfg(target_os = "linux")]
        {
            builder = builder
                .with_execution_providers([
                    ort::execution_providers::CUDAExecutionProvider::default().build(),
                ])
                .map_err(|e| format!("Failed to register CUDA provider: {}", e))?;
            println!("[ParakeetEngine] Registered CUDA provider.");
        }

        let session = builder
            .commit_from_file(model_path)
            .map_err(|e| format!("Failed to load ONNX model from {}: {}", model_path, e))?;
        self.session = Some(Mutex::new(session));
        Ok(())
    }

    fn transcribe(&self, samples: &[f32], _language: Option<&str>) -> Result<String, String> {
        let session_mutex = self.session.as_ref().ok_or("No ONNX session loaded in ParakeetEngine")?;
        let log_probs = {
            let mut session = session_mutex.lock().map_err(|e| format!("Failed to lock Parakeet session: {}", e))?;
            let signal_len = samples.len();
            let audio_array = ndarray::Array::from_shape_vec((1, signal_len), samples.to_vec())
                .map_err(|e| format!("Failed to create input audio array: {}", e))?;
            let length_array = ndarray::Array::from_shape_vec((1,), vec![signal_len as i32])
                .map_err(|e| format!("Failed to create input length array: {}", e))?;
            let audio_tensor = ort::value::Tensor::from_array(audio_array)
                .map_err(|e| format!("Failed to create audio tensor: {}", e))?;
            let length_tensor = ort::value::Tensor::from_array(length_array)
                .map_err(|e| format!("Failed to create length tensor: {}", e))?;
            let outputs = session.run(inputs![
                "audio_signal" => &audio_tensor,
                "length" => &length_tensor,
            ]).map_err(|e| format!("ONNX inference run failed: {}", e))?;
            outputs[0]
                .try_extract_array::<f32>()
                .map_err(|e| format!("Failed to extract output array: {}", e))?
                .to_owned()
        };
        let shape = log_probs.shape();
        if shape.len() != 3 {
            return Err(format!("Unexpected output tensor shape: {:?}", shape));
        }
        let time_steps = shape[1];
        let vocab_size = shape[2];
        let vocab = [
            " ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q",
            "r", "s", "t", "u", "v", "w", "x", "y", "z", "'",
        ];
        let blank_idx = vocab_size - 1;
        let mut decoded = String::new();
        let mut last_idx = None;
        for t in 0..time_steps {
            let mut max_val = f32::NEG_INFINITY;
            let mut max_idx = 0;
            for v in 0..vocab_size {
                let val = *log_probs.get([0, t, v]).unwrap_or(&f32::NEG_INFINITY);
                if val > max_val {
                    max_val = val;
                    max_idx = v;
                }
            }
            if max_idx != blank_idx && Some(max_idx) != last_idx && max_idx < vocab.len() {
                decoded.push_str(vocab[max_idx]);
            }
            last_idx = Some(max_idx);
        }
        Ok(decoded.trim().to_string())
    }
}
