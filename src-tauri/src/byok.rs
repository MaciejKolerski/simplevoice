use crate::audio::AudioController;
use crate::{deliver_transcription, LastTranscription};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::ipc::{InvokeBody, Request, Response};

const KEYRING_SERVICE: &str = "simplevoice";
const MAX_PROXY_METADATA_BYTES: usize = 64 * 1024;
const MAX_TRANSCRIPTION_REQUEST_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
struct ProviderSpec {
    id: &'static str,
    name: &'static str,
    package: &'static str,
    default_model: &'static str,
}

const PROVIDERS: [ProviderSpec; 4] = [
    ProviderSpec {
        id: "openai",
        name: "OpenAI",
        package: "@ai-sdk/openai",
        default_model: "gpt-4o-mini-transcribe",
    },
    ProviderSpec {
        id: "groq",
        name: "Groq",
        package: "@ai-sdk/groq",
        default_model: "whisper-large-v3-turbo",
    },
    ProviderSpec {
        id: "deepgram",
        name: "Deepgram",
        package: "@ai-sdk/deepgram",
        default_model: "nova-3",
    },
    ProviderSpec {
        id: "elevenlabs",
        name: "ElevenLabs",
        package: "@ai-sdk/elevenlabs",
        default_model: "scribe_v2",
    },
];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudProviderInfo {
    id: String,
    name: String,
    sdk_package: String,
    sdk_version: Option<String>,
    default_model: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CloudModelInfo {
    pub id: String,
    pub name: String,
}

fn provider_spec(id: &str) -> Option<ProviderSpec> {
    PROVIDERS.iter().copied().find(|provider| provider.id == id)
}

pub(crate) fn is_supported_provider(id: &str) -> bool {
    provider_spec(id).is_some()
}

fn shared_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

fn truncate(value: &str, max_chars: usize) -> String {
    let value = value.trim();
    if value.chars().count() <= max_chars {
        value.to_string()
    } else {
        value.chars().take(max_chars).collect::<String>() + "…"
    }
}

async fn fetch_sdk_version(spec: ProviderSpec) -> Option<String> {
    let encoded = spec.package.replace('/', "%2F");
    let url = format!("https://registry.npmjs.org/{encoded}/latest");
    let response = shared_client()
        .get(url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let json: serde_json::Value = response.json().await.ok()?;
    if json.get("name").and_then(|value| value.as_str()) != Some(spec.package) {
        return None;
    }
    json.get("version")
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

async fn load_provider_catalog() -> Vec<CloudProviderInfo> {
    let versions =
        futures::future::join_all(PROVIDERS.iter().copied().map(fetch_sdk_version)).await;
    PROVIDERS
        .iter()
        .zip(versions)
        .map(|(spec, sdk_version)| CloudProviderInfo {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            sdk_package: spec.package.to_string(),
            sdk_version,
            default_model: spec.default_model.to_string(),
        })
        .collect()
}

#[tauri::command]
pub async fn list_cloud_providers() -> Vec<CloudProviderInfo> {
    static CATALOG: tokio::sync::OnceCell<Vec<CloudProviderInfo>> =
        tokio::sync::OnceCell::const_new();
    CATALOG.get_or_init(load_provider_catalog).await.clone()
}

fn api_key(provider: &str) -> Result<String, String> {
    let spec = provider_spec(provider)
        .ok_or_else(|| format!("errors.provider_no_transcription::{provider}"))?;
    let entry = keyring::Entry::new(KEYRING_SERVICE, &format!("api_key_{}", spec.id))
        .map_err(|error| format!("errors.keyring_access::{error}"))?;
    match entry.get_password() {
        Ok(key) if !key.trim().is_empty() => Ok(key),
        Ok(_) | Err(keyring::Error::NoEntry) => {
            Err(format!("errors.api_key_missing::{}", spec.name))
        }
        Err(error) => Err(format!("errors.keyring_access::{error}")),
    }
}

async fn checked_json(request: reqwest::RequestBuilder) -> Result<serde_json::Value, String> {
    let response = request
        .send()
        .await
        .map_err(|error| format!("errors.cloud_request_failed::{error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("errors.cloud_response_parse::{error}"))?;
    if !status.is_success() {
        return Err(format!(
            "errors.cloud_api_error::{} — {}",
            status,
            truncate(&body, 400)
        ));
    }
    serde_json::from_str(&body).map_err(|error| format!("errors.cloud_response_parse::{error}"))
}

fn sorted_models(mut models: Vec<CloudModelInfo>, default_model: &str) -> Vec<CloudModelInfo> {
    let mut seen = HashSet::with_capacity(models.len());
    models.retain(|model| seen.insert(model.id.clone()));
    models.sort_by(|a, b| {
        let a_default = a.id == default_model;
        let b_default = b.id == default_model;
        b_default
            .cmp(&a_default)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
    models
}

fn parse_openai_models(json: &serde_json::Value) -> Vec<CloudModelInfo> {
    json.get("data")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|model| model.get("id").and_then(|value| value.as_str()))
        .filter(|id| {
            let lower = id.to_lowercase();
            (*id == "whisper-1" || lower.contains("transcribe"))
                && !lower.contains("realtime")
                && !lower.contains("live-transcribe")
        })
        .map(|id| CloudModelInfo {
            id: id.to_string(),
            name: id.to_string(),
        })
        .collect()
}

fn parse_groq_models(json: &serde_json::Value) -> Vec<CloudModelInfo> {
    json.get("data")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter(|model| model.get("active").and_then(|value| value.as_bool()) != Some(false))
        .filter_map(|model| model.get("id").and_then(|value| value.as_str()))
        .filter(|id| id.to_lowercase().contains("whisper"))
        .map(|id| CloudModelInfo {
            id: id.to_string(),
            name: id.to_string(),
        })
        .collect()
}

fn parse_deepgram_models(json: &serde_json::Value) -> Vec<CloudModelInfo> {
    json.get("stt")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter(|model| model.get("batch").and_then(|value| value.as_bool()) != Some(false))
        .filter_map(|model| {
            let id = model
                .get("canonical_name")
                .or_else(|| model.get("name"))?
                .as_str()?
                .trim();
            if id.is_empty() {
                return None;
            }
            let name = model
                .get("name")
                .and_then(|value| value.as_str())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(id);
            Some(CloudModelInfo {
                id: id.to_string(),
                name: name.to_string(),
            })
        })
        .collect()
}

fn parse_elevenlabs_models(json: &serde_json::Value) -> Vec<CloudModelInfo> {
    json.as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| {
            let id = model.get("model_id")?.as_str()?.trim();
            let lower = id.to_lowercase();
            if !lower.contains("scribe") || lower.contains("realtime") {
                return None;
            }
            let name = model
                .get("name")
                .and_then(|value| value.as_str())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(id);
            Some(CloudModelInfo {
                id: id.to_string(),
                name: name.to_string(),
            })
        })
        .collect()
}

async fn list_deepgram_models(key: &str) -> Result<Vec<CloudModelInfo>, String> {
    let projects = checked_json(
        shared_client()
            .get("https://api.deepgram.com/v1/projects")
            .header("Authorization", format!("Token {key}")),
    )
    .await?;
    let project_ids: Vec<String> = projects
        .get("projects")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|project| {
            project
                .get("project_id")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .take(8)
        .collect();

    let requests = project_ids.into_iter().map(|project_id| async move {
        checked_json(
            shared_client()
                .get(format!(
                    "https://api.deepgram.com/v1/projects/{project_id}/models"
                ))
                .query(&[("include_outdated", "false")])
                .header("Authorization", format!("Token {key}")),
        )
        .await
    });

    let mut models = Vec::new();
    let mut first_error = None;
    for result in futures::future::join_all(requests).await {
        match result {
            Ok(json) => models.extend(parse_deepgram_models(&json)),
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    if models.is_empty() {
        if let Some(error) = first_error {
            return Err(error);
        }
    }
    Ok(models)
}

#[tauri::command]
pub async fn list_cloud_models(provider: String) -> Result<Vec<CloudModelInfo>, String> {
    let spec = provider_spec(provider.trim())
        .ok_or_else(|| format!("errors.provider_no_transcription::{provider}"))?;
    let key = api_key(spec.id)?;
    let models = match spec.id {
        "openai" => {
            let json = checked_json(
                shared_client()
                    .get("https://api.openai.com/v1/models")
                    .bearer_auth(&key),
            )
            .await?;
            parse_openai_models(&json)
        }
        "groq" => {
            let json = checked_json(
                shared_client()
                    .get("https://api.groq.com/openai/v1/models")
                    .bearer_auth(&key),
            )
            .await?;
            parse_groq_models(&json)
        }
        "deepgram" => list_deepgram_models(&key).await?,
        "elevenlabs" => {
            let json = checked_json(
                shared_client()
                    .get("https://api.elevenlabs.io/v1/models")
                    .header("xi-api-key", &key),
            )
            .await?;
            parse_elevenlabs_models(&json)
        }
        _ => Vec::new(),
    };
    let models = sorted_models(models, spec.default_model);
    if models.is_empty() {
        return Err(format!("errors.no_transcription_models::{}", spec.name));
    }
    Ok(models)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProxyRequestMetadata {
    provider: String,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProxyResponseMetadata {
    status: u16,
    status_text: String,
    headers: Vec<(String, String)>,
}

fn endpoint_is_allowed(provider: &str, url: &reqwest::Url) -> bool {
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    let host = url.host_str().unwrap_or_default();
    let path = url.path().trim_end_matches('/');
    match provider {
        "openai" => host == "api.openai.com" && path == "/v1/audio/transcriptions",
        "groq" => host == "api.groq.com" && path == "/openai/v1/audio/transcriptions",
        "deepgram" => host == "api.deepgram.com" && path == "/v1/listen",
        "elevenlabs" => host == "api.elevenlabs.io" && path == "/v1/speech-to-text",
        _ => false,
    }
}

fn decode_proxy_metadata(request: &Request<'_>) -> Result<ProxyRequestMetadata, String> {
    let encoded = request
        .headers()
        .get("x-simplevoice-byok-request")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| "errors.invalid_byok_request".to_string())?;
    if encoded.len() > MAX_PROXY_METADATA_BYTES {
        return Err("errors.invalid_byok_request".to_string());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "errors.invalid_byok_request".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "errors.invalid_byok_request".to_string())
}

fn encode_proxy_response(
    metadata: ProxyResponseMetadata,
    body: Vec<u8>,
) -> Result<Response, String> {
    let metadata = serde_json::to_vec(&metadata)
        .map_err(|error| format!("errors.cloud_response_parse::{error}"))?;
    let metadata_len = u32::try_from(metadata.len())
        .map_err(|_| "errors.cloud_response_parse::response metadata too large".to_string())?;
    let mut envelope = Vec::with_capacity(4 + metadata.len() + body.len());
    envelope.extend_from_slice(&metadata_len.to_be_bytes());
    envelope.extend_from_slice(&metadata);
    envelope.extend_from_slice(&body);
    Ok(Response::new(envelope))
}

#[tauri::command]
pub async fn byok_http_request(request: Request<'_>) -> Result<Response, String> {
    let metadata = decode_proxy_metadata(&request)?;
    let body = match request.body() {
        InvokeBody::Raw(bytes) => bytes.clone(),
        InvokeBody::Json(_) => return Err("errors.invalid_byok_request".to_string()),
    };
    if body.len() > MAX_TRANSCRIPTION_REQUEST_BYTES || metadata.headers.len() > 32 {
        return Err("errors.invalid_byok_request".to_string());
    }
    let spec = provider_spec(metadata.provider.trim())
        .ok_or_else(|| format!("errors.provider_no_transcription::{}", metadata.provider))?;
    if !metadata.method.eq_ignore_ascii_case("POST") {
        return Err("errors.invalid_byok_request".to_string());
    }
    let url = reqwest::Url::parse(&metadata.url)
        .map_err(|_| "errors.invalid_byok_request".to_string())?;
    if !endpoint_is_allowed(spec.id, &url) {
        return Err("errors.invalid_byok_request".to_string());
    }

    let key = api_key(spec.id)?;
    let mut outbound = shared_client().post(url.clone()).body(body);
    for (name, value) in metadata.headers {
        if !matches!(
            name.to_ascii_lowercase().as_str(),
            "accept" | "content-type"
        ) {
            continue;
        }
        let Ok(name) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        let Ok(value) = reqwest::header::HeaderValue::from_str(&value) else {
            continue;
        };
        outbound = outbound.header(name, value);
    }
    outbound = match spec.id {
        "deepgram" => outbound.header("Authorization", format!("Token {key}")),
        "elevenlabs" => outbound.header("xi-api-key", key),
        _ => outbound.bearer_auth(key),
    };

    let response = outbound
        .send()
        .await
        .map_err(|error| format!("errors.cloud_request_failed::{error}"))?;
    let status = response.status();
    let response_headers = response
        .headers()
        .iter()
        .filter(|(name, _)| {
            !matches!(
                name.as_str(),
                "connection"
                    | "content-encoding"
                    | "content-length"
                    | "set-cookie"
                    | "transfer-encoding"
            )
        })
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect();
    let response_body = response
        .bytes()
        .await
        .map_err(|error| format!("errors.cloud_response_parse::{error}"))?
        .to_vec();
    encode_proxy_response(
        ProxyResponseMetadata {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or_default().to_string(),
            headers: response_headers,
        },
        response_body,
    )
}

struct PreparedTranscription {
    id: u64,
    samples: Arc<Vec<f32>>,
    chunks: Vec<Range<usize>>,
}

#[derive(Default)]
pub struct CloudTranscriptionState {
    next_id: AtomicU64,
    prepared: Mutex<Option<PreparedTranscription>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudTranscriptionPlan {
    session_id: String,
    chunk_count: usize,
}

#[derive(Deserialize)]
pub struct CloudChunkResult {
    text: Option<String>,
    error: Option<String>,
}

fn pcm_to_wav_bytes(samples: &[f32]) -> Result<Vec<u8>, String> {
    let mut cursor = std::io::Cursor::new(Vec::with_capacity(samples.len() * 2 + 64));
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: crate::stt::chunker::SAMPLE_RATE as u32,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|error| format!("errors.audio_encode_failed::{error}"))?;
        for sample in samples {
            writer
                .write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .map_err(|error| format!("errors.audio_encode_failed::{error}"))?;
        }
        writer
            .finalize()
            .map_err(|error| format!("errors.audio_encode_failed::{error}"))?;
    }
    Ok(cursor.into_inner())
}

fn parse_session_id(session_id: &str) -> Result<u64, String> {
    session_id
        .parse()
        .map_err(|_| "errors.cloud_session_expired".to_string())
}

#[tauri::command]
pub async fn prepare_cloud_transcription(
    audio_controller: tauri::State<'_, AudioController>,
    state: tauri::State<'_, CloudTranscriptionState>,
) -> Result<CloudTranscriptionPlan, String> {
    let samples = {
        let audio = audio_controller
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        Arc::clone(&audio.last_samples)
    };
    if samples.is_empty() {
        return Err("errors.no_recording_audio".to_string());
    }
    let (prepared, chunks) = tauri::async_runtime::spawn_blocking(move || {
        let prepared = crate::stt::prepare_samples(&samples);
        let chunks = crate::stt::chunker::split_at_silences(&prepared);
        (Arc::new(prepared), chunks)
    })
    .await
    .map_err(|error| format!("errors.audio_encode_failed::{error}"))?;
    let id = state
        .next_id
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);
    let chunk_count = chunks.len();
    *state
        .prepared
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(PreparedTranscription {
        id,
        samples: prepared,
        chunks,
    });
    Ok(CloudTranscriptionPlan {
        session_id: id.to_string(),
        chunk_count,
    })
}

#[tauri::command]
pub async fn get_cloud_transcription_chunk(
    session_id: String,
    index: usize,
    state: tauri::State<'_, CloudTranscriptionState>,
) -> Result<Response, String> {
    let id = parse_session_id(&session_id)?;
    let (samples, range) = {
        let prepared = state
            .prepared
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let prepared = prepared
            .as_ref()
            .filter(|prepared| prepared.id == id)
            .ok_or_else(|| "errors.cloud_session_expired".to_string())?;
        let range = prepared
            .chunks
            .get(index)
            .cloned()
            .ok_or_else(|| "errors.cloud_session_expired".to_string())?;
        (Arc::clone(&prepared.samples), range)
    };
    let bytes = tauri::async_runtime::spawn_blocking(move || pcm_to_wav_bytes(&samples[range]))
        .await
        .map_err(|error| format!("errors.audio_encode_failed::{error}"))??;
    Ok(Response::new(bytes))
}

#[tauri::command]
pub async fn complete_cloud_transcription(
    session_id: String,
    results: Vec<CloudChunkResult>,
    language: Option<String>,
    state: tauri::State<'_, CloudTranscriptionState>,
    audio_controller: tauri::State<'_, AudioController>,
    last_transcription: tauri::State<'_, LastTranscription>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let id = parse_session_id(&session_id)?;
    let prepared = state
        .prepared
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
        .filter(|prepared| prepared.id == id)
        .ok_or_else(|| "errors.cloud_session_expired".to_string())?;
    if results.len() != prepared.chunks.len() {
        return Err("errors.cloud_session_expired".to_string());
    }
    let results = results
        .into_iter()
        .map(|result| match (result.text, result.error) {
            (Some(text), _) => Ok(text.trim().to_string()),
            (_, Some(error)) => Err(format!("errors.cloud_api_error::{}", truncate(&error, 600))),
            _ => Ok(String::new()),
        })
        .collect();
    let (parts, truncated_at) = crate::join_cloud_results(results, &prepared.chunks)?;
    let mut text =
        crate::stt::text::collapse_repeats(&crate::stt::sanitize_output(&parts.join(" ")));
    if let Some(seconds) = truncated_at {
        text.push_str(&crate::truncation_marker(&app_handle, seconds));
    }
    Ok(deliver_transcription(
        text,
        language.as_deref(),
        last_transcription.inner(),
        audio_controller.inner(),
        &app_handle,
    )
    .await)
}

#[tauri::command]
pub fn cancel_cloud_transcription(
    session_id: String,
    state: tauri::State<'_, CloudTranscriptionState>,
) {
    let Ok(id) = parse_session_id(&session_id) else {
        return;
    };
    let mut prepared = state
        .prepared
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if prepared.as_ref().is_some_and(|prepared| prepared.id == id) {
        *prepared = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openai_filter_is_strictly_transcription_only() {
        let models = parse_openai_models(&json!({"data": [
            {"id": "gpt-4o"},
            {"id": "tts-1"},
            {"id": "whisper-1"},
            {"id": "gpt-4o-mini-transcribe"},
            {"id": "gpt-live-transcribe"},
            {"id": "gpt-realtime-transcribe"}
        ]}));
        assert_eq!(
            models.into_iter().map(|model| model.id).collect::<Vec<_>>(),
            vec!["whisper-1", "gpt-4o-mini-transcribe"]
        );
    }

    #[test]
    fn groq_filter_drops_chat_and_inactive_models() {
        let models = parse_groq_models(&json!({"data": [
            {"id": "llama-3", "active": true},
            {"id": "whisper-large-v3", "active": false},
            {"id": "whisper-large-v3-turbo", "active": true}
        ]}));
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "whisper-large-v3-turbo");
    }

    #[test]
    fn deepgram_uses_only_batch_stt_models() {
        let models = parse_deepgram_models(&json!({
            "stt": [
                {"name": "nova-3", "canonical_name": "nova-3", "batch": true},
                {"name": "flux", "canonical_name": "flux-general-en", "batch": false}
            ],
            "tts": [{"name": "aura", "canonical_name": "aura-2"}]
        }));
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "nova-3");
    }

    #[test]
    fn elevenlabs_drops_tts_and_realtime_models() {
        let models = parse_elevenlabs_models(&json!([
            {"model_id": "eleven_multilingual_v2", "name": "Multilingual"},
            {"model_id": "scribe_v2_realtime", "name": "Scribe realtime"},
            {"model_id": "scribe_v2", "name": "Scribe v2"}
        ]));
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "scribe_v2");
    }

    #[test]
    fn proxy_rejects_non_provider_endpoints() {
        assert!(endpoint_is_allowed(
            "openai",
            &reqwest::Url::parse("https://api.openai.com/v1/audio/transcriptions").unwrap()
        ));
        assert!(!endpoint_is_allowed(
            "openai",
            &reqwest::Url::parse("https://example.com/v1/audio/transcriptions").unwrap()
        ));
        assert!(!endpoint_is_allowed(
            "openai",
            &reqwest::Url::parse("http://api.openai.com/v1/audio/transcriptions").unwrap()
        ));
    }

    #[test]
    fn wav_encoder_writes_a_valid_mono_16khz_file() {
        let bytes = pcm_to_wav_bytes(&[0.0, 0.5, -0.5]).unwrap();
        let reader = hound::WavReader::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().sample_rate, 16_000);
        assert_eq!(reader.duration(), 3);
    }
}
