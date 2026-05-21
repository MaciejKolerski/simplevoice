use ort::{inputs, session::Session};

pub fn transcribe_parakeet(samples: &[f32], model_path: &str) -> Result<String, String> {
    // 1. Initialize ONNX session
    let mut session = Session::builder()
        .map_err(|e| format!("Failed to create ONNX session builder: {}", e))?
        .commit_from_file(model_path)
        .map_err(|e| format!("Failed to load ONNX model from {}: {}", model_path, e))?;

    // 2. Prepare inputs
    let signal_len = samples.len();

    // Convert to ndarray as expected by ort
    let audio_array = ndarray::Array::from_shape_vec((1, signal_len), samples.to_vec())
        .map_err(|e| format!("Failed to create input audio array: {}", e))?;

    let length_array = ndarray::Array::from_shape_vec((1,), vec![signal_len as i32])
        .map_err(|e| format!("Failed to create input length array: {}", e))?;

    let audio_tensor = ort::value::Tensor::from_array(audio_array)
        .map_err(|e| format!("Failed to create audio tensor: {}", e))?;
    let length_tensor = ort::value::Tensor::from_array(length_array)
        .map_err(|e| format!("Failed to create length tensor: {}", e))?;

    // 3. Run session
    let outputs = session
        .run(inputs![
            "audio_signal" => &audio_tensor,
            "length" => &length_tensor,
        ])
        .map_err(|e| format!("ONNX inference run failed: {}", e))?;

    // 4. CTC Greedy Decoding
    let log_probs = outputs[0]
        .try_extract_array::<f32>()
        .map_err(|e| format!("Failed to extract output array: {}", e))?;

    let shape = log_probs.shape(); // [batch, time, vocab_size]
    if shape.len() != 3 {
        return Err(format!("Unexpected output tensor shape: {:?}", shape));
    }

    let time_steps = shape[1];
    let vocab_size = shape[2];

    // Standard English alphabet vocabulary for Nvidia Parakeet
    let vocab = [
        " ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q",
        "r", "s", "t", "u", "v", "w", "x", "y", "z", "'",
    ];
    let blank_idx = vocab_size - 1; // Last index is CTC blank token

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
pub struct ParakeetEngine {
    model_path: String,
}

impl ParakeetEngine {
    pub fn new(model_path: &str) -> Self {
        Self {
            model_path: model_path.to_string(),
        }
    }
}

impl super::EngineAdapter for ParakeetEngine {
    fn initialize(&mut self, _model_path: &str, _use_gpu: bool) -> Result<(), String> {
        Ok(())
    }

    fn transcribe(&self, samples: &[f32], _language: Option<&str>) -> Result<String, String> {
        transcribe_parakeet(samples, &self.model_path)
    }

}
