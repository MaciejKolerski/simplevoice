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
            writer
                .write_sample(sample_i16)
                .map_err(|e| format!("Failed to write WAV sample: {}", e))?;
        }
        writer
            .finalize()
            .map_err(|e| format!("Failed to finalize WAV: {}", e))?;
    }
    Ok(buffer.into_inner())
}

pub async fn transcribe_cloud(
    samples: &[f32],
    api_key: &str,
    provider: Option<&str>,
    model: Option<&str>,
    base_url: Option<&str>,
    language: Option<&str>,
) -> Result<String, String> {
    let wav_bytes = pcm_to_wav_bytes(samples)?;
    let client = reqwest::Client::new();
    let provider_str = provider.unwrap_or("").trim().to_lowercase();

    if provider_str == "anthropic" {
        return Err("Anthropic Claude does not support audio transcription. Please select a different provider.".to_string());
    }

    // Google Gemini preset handling
    if provider_str == "gemini" {
        let base_url_trimmed = base_url.unwrap_or("").trim();
        let base = if base_url_trimmed.is_empty() {
            "https://generativelanguage.googleapis.com/v1beta"
        } else {
            base_url_trimmed
        };
        let model_name = model.unwrap_or("").trim();
        let model_str = if model_name.is_empty() {
            "gemini-1.5-flash"
        } else {
            model_name
        };
        let endpoint = format!("{}/models/{}:generateContent", base.trim_end_matches('/'), model_str);

        // Encode WAV to base64
        let base64_data = base64::engine::general_purpose::STANDARD.encode(&wav_bytes);

        // Construct prompt text
        let prompt_text = if let Some(lang) = language {
            if !lang.is_empty() && lang != "auto" {
                format!(
                    "Transcribe this audio. Please transcribe the audio into text precisely. The language is: {}. Do not add any introduction, explanation, formatting or extra text. Output ONLY the transcription of the speech.",
                    lang
                )
            } else {
                "Transcribe this audio. Please transcribe the audio into text precisely. Do not add any introduction, explanation, formatting or extra text. Output ONLY the transcription of the speech.".to_string()
            }
        } else {
            "Transcribe this audio. Please transcribe the audio into text precisely. Do not add any introduction, explanation, formatting or extra text. Output ONLY the transcription of the speech.".to_string()
        };

        // Construct JSON payload
        let payload = serde_json::json!({
            "contents": [{
                "role": "user",
                "parts": [
                    { "text": prompt_text },
                    {
                        "inline_data": {
                            "mime_type": "audio/wav",
                            "data": base64_data
                        }
                    }
                ]
            }]
        });

        let response = client
            .post(&endpoint)
            .header("x-goog-api-key", api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Failed to send request to Gemini ASR: {}", e))?;

        if !response.status().is_success() {
            let err_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("Gemini API returned error: {}", err_text));
        }

        #[derive(serde::Deserialize)]
        struct GeminiResponse {
            candidates: Option<Vec<GeminiCandidate>>,
        }
        #[derive(serde::Deserialize)]
        struct GeminiCandidate {
            content: Option<GeminiContent>,
        }
        #[derive(serde::Deserialize)]
        struct GeminiContent {
            parts: Option<Vec<GeminiPart>>,
        }
        #[derive(serde::Deserialize)]
        struct GeminiPart {
            text: Option<String>,
        }

        let result = response
            .json::<GeminiResponse>()
            .await
            .map_err(|e| format!("Failed to parse Gemini response: {}", e))?;

        let transcribed_text = result
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .and_then(|c| c.content.as_ref())
            .and_then(|c| c.parts.as_ref())
            .and_then(|p| p.first())
            .and_then(|p| p.text.as_deref())
            .ok_or_else(|| "Failed to extract transcription text from Gemini response. Make sure the API key is valid and the model is correct.".to_string())?;

        return Ok(transcribed_text.trim().to_string());
    }

    // OpenRouter preset handling
    if provider_str == "openrouter" {
        let base_url_trimmed = base_url.unwrap_or("").trim();
        let base = if base_url_trimmed.is_empty() {
            "https://openrouter.ai/api/v1"
        } else {
            base_url_trimmed
        };
        let model_name = model.unwrap_or("").trim();
        let model_str = if model_name.is_empty() {
            "openai/whisper-large-v3"
        } else {
            model_name
        };
        let endpoint = format!("{}/audio/transcriptions", base.trim_end_matches('/'));

        // Encode WAV to base64
        let base64_data = base64::engine::general_purpose::STANDARD.encode(&wav_bytes);

        // Construct JSON payload
        let mut payload = serde_json::json!({
            "model": model_str,
            "input_audio": {
                "data": base64_data,
                "format": "wav"
            }
        });

        // Add language if specified
        if let Some(lang) = language {
            if !lang.is_empty() && lang != "auto" {
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("language".to_string(), serde_json::Value::String(lang.to_string()));
                }
            }
        }

        let response = client
            .post(&endpoint)
            .bearer_auth(api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Failed to send request to OpenRouter ASR: {}", e))?;

        if !response.status().is_success() {
            let err_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("OpenRouter ASR API returned error: {}", err_text));
        }

        #[derive(serde::Deserialize)]
        struct ApiResponse {
            text: String,
        }

        let result = response
            .json::<ApiResponse>()
            .await
            .map_err(|e| format!("Failed to parse OpenRouter response: {}", e))?;

        return Ok(result.text.trim().to_string());
    }

    // Default OpenAI/custom preset flow (using multipart/form-data)
    let part = multipart::Part::bytes(wav_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;

    let model_name = model.unwrap_or("").trim();
    let model_str = if model_name.is_empty() {
        "whisper-1"
    } else {
        model_name
    };

    let mut form = multipart::Form::new()
        .part("file", part)
        .text("model", model_str.to_string());

    if let Some(lang) = language {
        if !lang.is_empty() && lang != "auto" {
            form = form.text("language", lang.to_string());
        }
    }

    let base_url_trimmed = base_url.unwrap_or("").trim();
    let endpoint = if base_url_trimmed.is_empty() {
        "https://api.openai.com/v1/audio/transcriptions".to_string()
    } else {
        format!(
            "{}/audio/transcriptions",
            base_url_trimmed.trim_end_matches('/')
        )
    };

    let response = client
        .post(&endpoint)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Failed to send request to cloud ASR: {}", e))?;

    if !response.status().is_success() {
        let err_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
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
