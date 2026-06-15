use reqwest::multipart;
use std::io::Cursor;
use base64::Engine;

/// Keep model ids that look like speech-to-text models.
fn asr_model_filter(id: &str) -> bool {
    let lower = id.to_lowercase();
    ["whisper", "transcribe", "asr"]
        .iter()
        .any(|kw| lower.contains(kw))
}

/// Apply the ASR keyword filter, but if it removes everything, return the full
/// list (protects unusual custom/self-hosted servers whose ids don't match).
fn apply_asr_filter(all: Vec<String>) -> Vec<String> {
    let filtered: Vec<String> = all.iter().filter(|id| asr_model_filter(id)).cloned().collect();
    if filtered.is_empty() {
        all
    } else {
        filtered
    }
}

fn sort_dedup(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

/// Parse model ids from an OpenAI-style `{ "data": [ { "id": ... } ] }` body.
fn parse_openai_models(json: &serde_json::Value) -> Vec<String> {
    json.get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Parse Gemini models, keeping only those that support `generateContent`
/// (the method the transcription path uses) and stripping the `models/` prefix.
fn parse_gemini_models(json: &serde_json::Value) -> Vec<String> {
    json.get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|m| {
                    m.get("supportedGenerationMethods")
                        .and_then(|v| v.as_array())
                        .map(|methods| {
                            methods.iter().any(|x| x.as_str() == Some("generateContent"))
                        })
                        .unwrap_or(false)
                })
                .filter_map(|m| m.get("name").and_then(|v| v.as_str()))
                .map(|name| name.strip_prefix("models/").unwrap_or(name).to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Trim and cap an error body so it is safe to surface in the UI.
fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() > max {
        s.chars().take(max).collect::<String>() + "…"
    } else {
        s.to_string()
    }
}

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
            .map_err(|e| format!("errors.audio_encode_failed::{}", e))?;
        for &sample in samples {
            let sample_i16 = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            writer
                .write_sample(sample_i16)
                .map_err(|e| format!("errors.audio_encode_failed::{}", e))?;
        }
        writer
            .finalize()
            .map_err(|e| format!("errors.audio_encode_failed::{}", e))?;
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
        return Err("errors.provider_no_transcription::Anthropic Claude".to_string());
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
            .map_err(|e| format!("errors.cloud_request_failed::{}", e))?;

        if !response.status().is_success() {
            let err_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("errors.cloud_api_error::{}", err_text));
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
            .map_err(|e| format!("errors.cloud_response_parse::{}", e))?;

        let transcribed_text = result
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .and_then(|c| c.content.as_ref())
            .and_then(|c| c.parts.as_ref())
            .and_then(|p| p.first())
            .and_then(|p| p.text.as_deref())
            .ok_or_else(|| "errors.cloud_extract_text".to_string())?;

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
            .map_err(|e| format!("errors.cloud_request_failed::{}", e))?;

        if !response.status().is_success() {
            let err_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("errors.cloud_api_error::{}", err_text));
        }

        #[derive(serde::Deserialize)]
        struct ApiResponse {
            text: String,
        }

        let result = response
            .json::<ApiResponse>()
            .await
            .map_err(|e| format!("errors.cloud_response_parse::{}", e))?;

        return Ok(result.text.trim().to_string());
    }

    // Default OpenAI/custom preset flow (using multipart/form-data)
    let part = multipart::Part::bytes(wav_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("errors.audio_encode_failed::{}", e))?;

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
        .map_err(|e| format!("errors.cloud_request_failed::{}", e))?;

    if !response.status().is_success() {
        let err_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("errors.cloud_api_error::{}", err_text));
    }

    #[derive(serde::Deserialize)]
    struct ApiResponse {
        text: String,
    }

    let result = response
        .json::<ApiResponse>()
        .await
        .map_err(|e| format!("errors.cloud_response_parse::{}", e))?;

    Ok(result.text.trim().to_string())
}

pub async fn list_models(
    provider: &str,
    base_url: Option<&str>,
    api_key: &str,
) -> Result<Vec<String>, String> {
    let provider_str = provider.trim().to_lowercase();
    if provider_str == "anthropic" {
        return Err("errors.provider_no_model_listing::Anthropic".to_string());
    }
    let base_trimmed = base_url.unwrap_or("").trim();
    let client = reqwest::Client::new();

    if provider_str == "gemini" {
        let base = if base_trimmed.is_empty() {
            "https://generativelanguage.googleapis.com/v1beta"
        } else {
            base_trimmed
        };
        let endpoint = format!("{}/models", base.trim_end_matches('/'));
        let response = client
            .get(&endpoint)
            .header("x-goog-api-key", api_key)
            .send()
            .await
            .map_err(|e| format!("errors.cloud_request_failed::{}", e))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("errors.cloud_api_error::{} — {}", status, truncate(&body, 300)));
        }
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("errors.cloud_response_parse::{}", e))?;
        return Ok(sort_dedup(parse_gemini_models(&json)));
    }

    // OpenAI / OpenRouter / custom (OpenAI-compatible)
    let base = if base_trimmed.is_empty() {
        match provider_str.as_str() {
            "openrouter" => "https://openrouter.ai/api/v1",
            _ => "https://api.openai.com/v1",
        }
    } else {
        base_trimmed
    };
    let endpoint = format!("{}/models", base.trim_end_matches('/'));
    let response = client
        .get(&endpoint)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|e| format!("errors.cloud_request_failed::{}", e))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("errors.cloud_api_error::{} — {}", status, truncate(&body, 300)));
    }
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("errors.cloud_response_parse::{}", e))?;
    Ok(sort_dedup(apply_asr_filter(parse_openai_models(&json))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn asr_filter_matches_keywords() {
        assert!(asr_model_filter("whisper-1"));
        assert!(asr_model_filter("gpt-4o-transcribe"));
        assert!(asr_model_filter("openai/whisper-large-v3"));
        assert!(asr_model_filter("some-ASR-model"));
        assert!(!asr_model_filter("gpt-4o"));
        assert!(!asr_model_filter("text-embedding-3-small"));
    }

    #[test]
    fn parses_openai_data_ids() {
        let j = json!({"data":[{"id":"whisper-1"},{"id":"gpt-4o"}]});
        assert_eq!(parse_openai_models(&j), vec!["whisper-1", "gpt-4o"]);
    }

    #[test]
    fn openai_missing_data_is_empty() {
        let j = json!({"object":"list"});
        assert!(parse_openai_models(&j).is_empty());
    }

    #[test]
    fn gemini_keeps_generatecontent_and_strips_prefix() {
        let j = json!({"models":[
            {"name":"models/gemini-1.5-flash","supportedGenerationMethods":["generateContent","countTokens"]},
            {"name":"models/embedding-001","supportedGenerationMethods":["embedContent"]}
        ]});
        assert_eq!(parse_gemini_models(&j), vec!["gemini-1.5-flash"]);
    }

    #[test]
    fn asr_filter_empty_fallback_returns_all() {
        let all = vec!["model-a".to_string(), "model-b".to_string()];
        assert_eq!(apply_asr_filter(all.clone()), all);
    }

    #[test]
    fn asr_filter_keeps_only_matches_when_present() {
        let all = vec!["whisper-1".to_string(), "gpt-4o".to_string()];
        assert_eq!(apply_asr_filter(all), vec!["whisper-1".to_string()]);
    }

    #[test]
    fn sort_dedup_orders_and_unifies() {
        let v = vec!["b".to_string(), "a".to_string(), "a".to_string()];
        assert_eq!(sort_dedup(v), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_at_exact_max_unchanged() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_long_ascii_appends_ellipsis() {
        assert_eq!(truncate("hello", 3), "hel…");
    }

    #[test]
    fn truncate_trims_whitespace() {
        assert_eq!(truncate("  hi  ", 10), "hi");
    }

    #[test]
    fn truncate_is_unicode_safe() {
        assert_eq!(truncate("héllo wörld", 4), "héll…");
    }

    #[test]
    fn truncate_zero_max_is_just_ellipsis() {
        assert_eq!(truncate("abc", 0), "…");
    }

    #[test]
    fn sort_dedup_handles_non_adjacent_duplicates() {
        let v = vec!["a".to_string(), "b".to_string(), "a".to_string()];
        assert_eq!(sort_dedup(v), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn gemini_skips_missing_name_and_keeps_unprefixed() {
        let j = json!({"models":[
            {"supportedGenerationMethods":["generateContent"]},
            {"name":"gemini-pro","supportedGenerationMethods":["generateContent"]}
        ]});
        assert_eq!(parse_gemini_models(&j), vec!["gemini-pro"]);
    }
}
