use reqwest::multipart;
use std::io::Cursor;

fn pcm_to_wav_bytes(samples: &[f32]) -> Result<Vec<u8>, String> {
    let mut buffer = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer = hound::WavWriter::new(&mut buffer, spec)
            .map_err(|e| format!("Failed to initialize WAV writer: {}", e))?;
        for &sample in samples {
            let sample_i16 = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            writer.write_sample(sample_i16)
                .map_err(|e| format!("Failed to write WAV sample: {}", e))?;
        }
        writer.finalize().map_err(|e| format!("Failed to finalize WAV: {}", e))?;
    }
    Ok(buffer.into_inner())
}

pub async fn transcribe_cloud(
    samples: &[f32],
    api_key: &str,
    model: Option<&str>,
    base_url: Option<&str>,
) -> Result<String, String> {
    let wav_bytes = pcm_to_wav_bytes(samples)?;
    
    let client = reqwest::Client::new();
    let part = multipart::Part::bytes(wav_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
        
    let model_name = model.unwrap_or("").trim();
    let model_str = if model_name.is_empty() { "whisper-1" } else { model_name };
    
    let form = multipart::Form::new()
        .part("file", part)
        .text("model", model_str.to_string());
        
    let base_url_trimmed = base_url.unwrap_or("").trim();
    let endpoint = if base_url_trimmed.is_empty() {
        "https://api.openai.com/v1/audio/transcriptions".to_string()
    } else {
        format!("{}/audio/transcriptions", base_url_trimmed.trim_end_matches('/'))
    };
        
    let response = client
        .post(&endpoint)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Failed to send request to cloud ASR: {}", e))?;
        
    if !response.status().is_success() {
        let err_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("Cloud ASR API returned error: {}", err_text));
    }
    
    #[derive(serde::Deserialize)]
    struct ApiResponse {
        text: String,
    }
    
    let result = response
        .json::<ApiResponse>()
        .await
        .map_err(|e| format!("Failed to parse cloud response: {}", e))?;
        
    Ok(result.text.trim().to_string())
}
