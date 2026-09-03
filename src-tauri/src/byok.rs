use crate::audio::AudioController;
use crate::{deliver_transcription, LastTranscription};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::ipc::{InvokeBody, Request, Response};

const KEYRING_SERVICE: &str = "simplevoice";
const MAX_PROXY_METADATA_BYTES: usize = 64 * 1024;
const MAX_TRANSCRIPTION_REQUEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_TRANSCRIPTION_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const POLL_LIMIT: usize = 120;

#[derive(Clone, Copy)]
struct ProviderSpec {
    id: &'static str,
    name: &'static str,
    package: &'static str,
    default_model: &'static str,
    credential_kind: &'static str,
    required_settings: &'static [&'static str],
    dashboard_url: &'static str,
}

const PROVIDERS: [ProviderSpec; 19] = [
    ProviderSpec {
        id: "openai",
        name: "OpenAI",
        package: "@ai-sdk/openai",
        default_model: "gpt-4o-mini-transcribe",
        credential_kind: "apiKey",
        required_settings: &[],
        dashboard_url: "https://platform.openai.com/api-keys",
    },
    ProviderSpec {
        id: "groq",
        name: "Groq",
        package: "@ai-sdk/groq",
        default_model: "whisper-large-v3-turbo",
        credential_kind: "apiKey",
        required_settings: &[],
        dashboard_url: "https://console.groq.com/keys",
    },
    ProviderSpec {
        id: "deepgram",
        name: "Deepgram",
        package: "@ai-sdk/deepgram",
        default_model: "nova-3",
        credential_kind: "apiKey",
        required_settings: &[],
        dashboard_url: "https://console.deepgram.com",
    },
    ProviderSpec {
        id: "assemblyai",
        name: "AssemblyAI",
        package: "@ai-sdk/assemblyai",
        default_model: "universal-3-5-pro",
        credential_kind: "apiKey",
        required_settings: &[],
        dashboard_url: "https://www.assemblyai.com/dashboard",
    },
    ProviderSpec {
        id: "speechmatics",
        name: "Speechmatics",
        package: "ai",
        default_model: "enhanced",
        credential_kind: "apiKey",
        required_settings: &[],
        dashboard_url: "https://portal.speechmatics.com",
    },
    ProviderSpec {
        id: "gladia",
        name: "Gladia",
        package: "@ai-sdk/gladia",
        default_model: "default",
        credential_kind: "apiKey",
        required_settings: &[],
        dashboard_url: "https://app.gladia.io",
    },
    ProviderSpec {
        id: "revai",
        name: "Rev AI",
        package: "@ai-sdk/revai",
        default_model: "machine",
        credential_kind: "apiKey",
        required_settings: &[],
        dashboard_url: "https://www.rev.ai/auth/login",
    },
    ProviderSpec {
        id: "elevenlabs",
        name: "ElevenLabs",
        package: "@ai-sdk/elevenlabs",
        default_model: "scribe_v2",
        credential_kind: "apiKey",
        required_settings: &[],
        dashboard_url: "https://elevenlabs.io/app/developers/api-keys",
    },
    ProviderSpec {
        id: "together",
        name: "Together AI",
        package: "@ai-sdk/openai",
        default_model: "openai/whisper-large-v3",
        credential_kind: "apiKey",
        required_settings: &[],
        dashboard_url: "https://api.together.ai/settings/api-keys",
    },
    ProviderSpec {
        id: "fireworks",
        name: "Fireworks AI",
        package: "@ai-sdk/openai",
        default_model: "whisper-v3-turbo",
        credential_kind: "apiKey",
        required_settings: &[],
        dashboard_url: "https://app.fireworks.ai/settings/users/api-keys",
    },
    ProviderSpec {
        id: "deepinfra",
        name: "DeepInfra",
        package: "ai",
        default_model: "openai/whisper-large-v3",
        credential_kind: "apiKey",
        required_settings: &[],
        dashboard_url: "https://deepinfra.com/dash/api_keys",
    },
    ProviderSpec {
        id: "lemonfox",
        name: "Lemonfox.ai",
        package: "@ai-sdk/openai",
        default_model: "whisper-1",
        credential_kind: "apiKey",
        required_settings: &[],
        dashboard_url: "https://www.lemonfox.ai/dashboard",
    },
    ProviderSpec {
        id: "cloudflare",
        name: "Cloudflare Workers AI",
        package: "workers-ai-provider",
        default_model: "@cf/openai/whisper-large-v3-turbo",
        credential_kind: "apiToken",
        required_settings: &["accountId"],
        dashboard_url: "https://dash.cloudflare.com",
    },
    ProviderSpec {
        id: "replicate",
        name: "Replicate",
        package: "ai",
        default_model: "openai/whisper",
        credential_kind: "apiToken",
        required_settings: &[],
        dashboard_url: "https://replicate.com/account/api-tokens",
    },
    ProviderSpec {
        id: "huggingface",
        name: "Hugging Face",
        package: "ai",
        default_model: "openai/whisper-large-v3",
        credential_kind: "apiToken",
        required_settings: &[],
        dashboard_url: "https://huggingface.co/settings/tokens",
    },
    ProviderSpec {
        id: "azure",
        name: "Microsoft Azure AI Speech",
        package: "ai",
        default_model: "standard",
        credential_kind: "subscriptionKey",
        required_settings: &["region"],
        dashboard_url: "https://portal.azure.com",
    },
    ProviderSpec {
        id: "google-cloud",
        name: "Google Cloud Speech-to-Text",
        package: "ai",
        default_model: "latest_long",
        credential_kind: "serviceAccountJson",
        required_settings: &[],
        dashboard_url: "https://console.cloud.google.com/apis/credentials",
    },
    ProviderSpec {
        id: "google-ai-studio",
        name: "Google AI Studio",
        package: "ai",
        default_model: "gemini-3.5-transcribe",
        credential_kind: "apiKey",
        required_settings: &[],
        dashboard_url: "https://aistudio.google.com/api-keys",
    },
    ProviderSpec {
        id: "aws",
        name: "Amazon Transcribe",
        package: "ai",
        default_model: "standard",
        credential_kind: "secretAccessKey",
        required_settings: &["accessKeyId", "region"],
        dashboard_url: "https://console.aws.amazon.com/iam/home#/security_credentials",
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
    credential_kind: String,
    required_settings: Vec<String>,
    dashboard_url: String,
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

async fn fetch_sdk_version(package: &'static str) -> Option<String> {
    let encoded = package.replace('/', "%2F");
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
    if json.get("name").and_then(|value| value.as_str()) != Some(package) {
        return None;
    }
    json.get("version")
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

async fn load_provider_catalog() -> Vec<CloudProviderInfo> {
    let mut seen_packages = HashSet::new();
    let packages = PROVIDERS
        .iter()
        .map(|provider| provider.package)
        .filter(|package| seen_packages.insert(*package));
    let versions = futures::future::join_all(
        packages.map(|package| async move { (package, fetch_sdk_version(package).await) }),
    )
    .await
    .into_iter()
    .collect::<HashMap<_, _>>();
    PROVIDERS
        .iter()
        .map(|spec| CloudProviderInfo {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            sdk_package: spec.package.to_string(),
            sdk_version: versions.get(spec.package).cloned().flatten(),
            default_model: spec.default_model.to_string(),
            credential_kind: spec.credential_kind.to_string(),
            required_settings: spec
                .required_settings
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            dashboard_url: spec.dashboard_url.to_string(),
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

async fn read_response_body(mut response: reqwest::Response) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_TRANSCRIPTION_RESPONSE_BYTES as u64)
    {
        return Err("errors.cloud_response_parse::response body too large".to_string());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("errors.cloud_response_parse::{error}"))?
    {
        if body.len() + chunk.len() > MAX_TRANSCRIPTION_RESPONSE_BYTES {
            return Err("errors.cloud_response_parse::response body too large".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn checked_bytes(request: reqwest::RequestBuilder) -> Result<Vec<u8>, String> {
    let response = request
        .send()
        .await
        .map_err(|error| format!("errors.cloud_request_failed::{error}"))?;
    let status = response.status();
    let body = read_response_body(response).await?;
    if !status.is_success() {
        return Err(format!(
            "errors.cloud_api_error::{} — {}",
            status,
            truncate(&String::from_utf8_lossy(&body), 400)
        ));
    }
    Ok(body)
}

async fn checked_json(request: reqwest::RequestBuilder) -> Result<serde_json::Value, String> {
    let body = checked_bytes(request).await?;
    serde_json::from_slice(&body).map_err(|error| format!("errors.cloud_response_parse::{error}"))
}

async fn optional_json(
    request: reqwest::RequestBuilder,
) -> Result<Option<serde_json::Value>, String> {
    let response = request
        .send()
        .await
        .map_err(|error| format!("errors.cloud_request_failed::{error}"))?;
    let status = response.status();
    let body = read_response_body(response).await?;
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(format!(
            "errors.cloud_api_error::{} — {}",
            status,
            truncate(&String::from_utf8_lossy(&body), 400)
        ));
    }
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| format!("errors.cloud_response_parse::{error}"))
}

fn required_setting<'a>(
    provider: ProviderSpec,
    settings: &'a HashMap<String, String>,
    name: &str,
) -> Result<&'a str, String> {
    settings
        .get(name)
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .ok_or_else(|| {
            format!(
                "errors.provider_setting_missing::{} — {name}",
                provider.name
            )
        })
}

fn validated_region(
    provider: ProviderSpec,
    settings: &HashMap<String, String>,
) -> Result<&str, String> {
    let region = required_setting(provider, settings, "region")?;
    if region.len() > 64
        || !region
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(format!(
            "errors.provider_setting_invalid::{} — region",
            provider.name
        ));
    }
    Ok(region)
}

fn validated_cloudflare_account(
    provider: ProviderSpec,
    settings: &HashMap<String, String>,
) -> Result<&str, String> {
    let account_id = required_setting(provider, settings, "accountId")?;
    if account_id.len() != 32 || !account_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "errors.provider_setting_invalid::{} — accountId",
            provider.name
        ));
    }
    Ok(account_id)
}

fn validated_aws_access_key(
    provider: ProviderSpec,
    settings: &HashMap<String, String>,
) -> Result<&str, String> {
    let access_key = required_setting(provider, settings, "accessKeyId")?;
    if !(16..=128).contains(&access_key.len())
        || !access_key.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(format!(
            "errors.provider_setting_invalid::{} — accessKeyId",
            provider.name
        ));
    }
    Ok(access_key)
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
        .filter(|id| {
            let lower = id.to_ascii_lowercase();
            lower.contains("whisper") && !lower.contains("realtime") && !lower.contains("live")
        })
        .map(|id| CloudModelInfo {
            id: id.to_string(),
            name: id.to_string(),
        })
        .collect()
}

fn model_values(json: &serde_json::Value) -> Vec<&serde_json::Value> {
    json.as_array()
        .or_else(|| json.get("data").and_then(serde_json::Value::as_array))
        .or_else(|| json.get("models").and_then(serde_json::Value::as_array))
        .or_else(|| json.get("results").and_then(serde_json::Value::as_array))
        .or_else(|| json.get("result").and_then(serde_json::Value::as_array))
        .map(|values| values.iter().collect())
        .unwrap_or_default()
}

fn model_id(model: &serde_json::Value) -> Option<&str> {
    model
        .get("id")
        .or_else(|| model.get("model_id"))
        .or_else(|| model.get("modelId"))
        .or_else(|| model.get("model_name"))
        .or_else(|| model.get("name"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn parse_together_models(json: &serde_json::Value) -> Vec<CloudModelInfo> {
    model_values(json)
        .into_iter()
        .filter_map(model_id)
        .filter(|id| {
            let id = id.to_ascii_lowercase();
            (id.contains("whisper") || id.contains("voxtral"))
                && !id.contains("realtime")
                && !id.contains("live")
        })
        .map(|id| CloudModelInfo {
            id: id.to_string(),
            name: id.to_string(),
        })
        .collect()
}

fn parse_fireworks_models(json: &serde_json::Value) -> Vec<CloudModelInfo> {
    model_values(json)
        .into_iter()
        .filter_map(model_id)
        .filter(|id| {
            let lower = id.to_ascii_lowercase();
            lower.contains("whisper") && !lower.contains("realtime")
        })
        .map(|id| {
            let short_id = id.rsplit('/').next().unwrap_or(id);
            CloudModelInfo {
                id: short_id.to_string(),
                name: short_id.to_string(),
            }
        })
        .collect()
}

fn parse_deepinfra_models(json: &serde_json::Value) -> Vec<CloudModelInfo> {
    model_values(json)
        .into_iter()
        .filter(|model| {
            !model.get("deprecated").is_some_and(|deprecated| {
                deprecated.as_bool() == Some(true)
                    || deprecated.as_i64().is_some_and(|value| value != 0)
            })
        })
        .filter(|model| {
            ["type", "reported_type", "pipeline_tag"]
                .into_iter()
                .filter_map(|field| model.get(field))
                .filter_map(serde_json::Value::as_str)
                .any(|kind| {
                    matches!(
                        kind.to_ascii_lowercase().as_str(),
                        "automatic-speech-recognition" | "speech-recognition" | "transcription"
                    )
                })
        })
        .filter_map(model_id)
        .filter(|id| {
            let lower = id.to_ascii_lowercase();
            !lower.contains("streaming") && !lower.contains("realtime") && !lower.contains("live")
        })
        .map(|id| CloudModelInfo {
            id: id.to_string(),
            name: id.to_string(),
        })
        .collect()
}

fn parse_cloudflare_models(json: &serde_json::Value) -> Vec<CloudModelInfo> {
    model_values(json)
        .into_iter()
        .filter_map(|model| {
            let id = model_id(model)?;
            let lower = id.to_ascii_lowercase();
            if !(lower.starts_with("@cf/openai/whisper") || lower == "@cf/deepgram/nova-3")
                || lower.contains("realtime")
            {
                return None;
            }
            let name = model
                .get("display_name")
                .or_else(|| model.get("displayName"))
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(id);
            Some(CloudModelInfo {
                id: id.to_string(),
                name: name.to_string(),
            })
        })
        .collect()
}

fn parse_huggingface_models(json: &serde_json::Value) -> Vec<CloudModelInfo> {
    model_values(json)
        .into_iter()
        .filter(|model| {
            model
                .get("pipeline_tag")
                .or_else(|| model.get("pipelineTag"))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|tag| tag == "automatic-speech-recognition")
        })
        .filter_map(model_id)
        .map(|id| CloudModelInfo {
            id: id.to_string(),
            name: id.to_string(),
        })
        .collect()
}

fn parse_gemini_transcription_models(json: &serde_json::Value) -> Vec<CloudModelInfo> {
    model_values(json)
        .into_iter()
        .filter_map(|model| {
            let raw_id = model_id(model)?;
            let id = raw_id.strip_prefix("models/").unwrap_or(raw_id);
            let lower = id.to_ascii_lowercase();
            if !lower.contains("transcribe") || lower.contains("live") {
                return None;
            }
            let name = model
                .get("displayName")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(id);
            Some(CloudModelInfo {
                id: id.to_string(),
                name: name.to_string(),
            })
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
            if !(lower.starts_with("scribe_") || lower.starts_with("scribe-"))
                || lower.contains("realtime")
            {
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

fn static_models(values: &[(&str, &str)]) -> Vec<CloudModelInfo> {
    values
        .iter()
        .map(|(id, name)| CloudModelInfo {
            id: (*id).to_string(),
            name: (*name).to_string(),
        })
        .collect()
}

#[derive(Deserialize)]
struct GoogleServiceAccount {
    #[serde(rename = "type")]
    account_type: String,
    project_id: String,
    private_key: String,
    client_email: String,
}

#[derive(Serialize)]
struct GoogleJwtClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
}

async fn google_access_token(credentials_json: &str) -> Result<(String, String), String> {
    use sha2::Digest as _;

    let credentials: GoogleServiceAccount = serde_json::from_str(credentials_json)
        .map_err(|error| format!("errors.invalid_provider_credentials::{error}"))?;
    if credentials.account_type != "service_account"
        || credentials.project_id.trim().is_empty()
        || credentials.client_email.trim().is_empty()
        || credentials.private_key.trim().is_empty()
    {
        return Err(
            "errors.invalid_provider_credentials::Google Cloud service account JSON".to_string(),
        );
    }

    let fingerprint = format!("{:x}", sha2::Sha256::digest(credentials_json.as_bytes()));
    static CACHE: OnceLock<tokio::sync::Mutex<HashMap<String, (String, i64)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()));
    let now = chrono::Utc::now().timestamp();
    if let Some((token, expires_at)) = cache.lock().await.get(&fingerprint) {
        if *expires_at > now + 60 {
            return Ok((token.clone(), credentials.project_id));
        }
    }

    let claims = GoogleJwtClaims {
        iss: &credentials.client_email,
        scope: "https://www.googleapis.com/auth/cloud-platform",
        aud: "https://oauth2.googleapis.com/token",
        iat: now,
        exp: now + 3600,
    };
    let assertion = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_rsa_pem(credentials.private_key.as_bytes())
            .map_err(|error| format!("errors.invalid_provider_credentials::{error}"))?,
    )
    .map_err(|error| format!("errors.invalid_provider_credentials::{error}"))?;
    let json = checked_json(
        shared_client()
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ]),
    )
    .await?;
    let token = json
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "errors.cloud_response_parse::missing Google access token".to_string())?
        .to_string();
    let expires_in = json
        .get("expires_in")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(3600)
        .clamp(60, 3600);
    cache
        .lock()
        .await
        .insert(fingerprint, (token.clone(), now + expires_in));
    Ok((token, credentials.project_id))
}

fn aws_credentials(
    provider: ProviderSpec,
    settings: &HashMap<String, String>,
    secret_access_key: &str,
) -> Result<(String, String, String), String> {
    let access_key_id = validated_aws_access_key(provider, settings)?.to_string();
    let region = validated_region(provider, settings)?.to_string();
    if secret_access_key.trim().len() < 16 {
        return Err(format!(
            "errors.invalid_provider_credentials::{}",
            provider.name
        ));
    }
    Ok((access_key_id, secret_access_key.trim().to_string(), region))
}

async fn list_aws_models(
    provider: ProviderSpec,
    settings: &HashMap<String, String>,
    secret_access_key: &str,
) -> Result<Vec<CloudModelInfo>, String> {
    let (access_key_id, secret_access_key, region) =
        aws_credentials(provider, settings, secret_access_key)?;
    let config = aws_sdk_transcribe::Config::builder()
        .region(aws_sdk_transcribe::config::Region::new(region))
        .credentials_provider(aws_sdk_transcribe::config::Credentials::new(
            access_key_id,
            secret_access_key,
            None,
            None,
            "simplevoice-byok",
        ))
        .build();
    let output = aws_sdk_transcribe::Client::from_conf(config)
        .list_language_models()
        .status_equals(aws_sdk_transcribe::types::ModelStatus::Completed)
        .max_results(100)
        .send()
        .await
        .map_err(|error| format!("errors.cloud_api_error::{error}"))?;
    let mut models = static_models(&[("standard", "Amazon Transcribe standard")]);
    models.extend(output.models().iter().filter_map(|model| {
        let name = model.model_name()?;
        let language = model.language_code()?.as_str();
        Some(CloudModelInfo {
            id: format!("custom::{name}::{language}"),
            name: format!("{name} ({language})"),
        })
    }));
    Ok(models)
}

#[tauri::command]
pub async fn list_cloud_models(
    provider: String,
    settings: Option<HashMap<String, String>>,
) -> Result<Vec<CloudModelInfo>, String> {
    let spec = provider_spec(provider.trim())
        .ok_or_else(|| format!("errors.provider_no_transcription::{provider}"))?;
    let settings = settings.unwrap_or_default();
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
        "assemblyai" => {
            checked_json(
                shared_client()
                    .get("https://api.assemblyai.com/v2/transcript")
                    .query(&[("limit", "1")])
                    .header("Authorization", &key),
            )
            .await?;
            static_models(&[
                ("universal-3-5-pro", "Universal-3.5 Pro"),
                ("universal-2", "Universal-2"),
            ])
        }
        "speechmatics" => {
            checked_json(
                shared_client()
                    .get("https://asr.api.speechmatics.com/v2/jobs/")
                    .query(&[("limit", "1")])
                    .bearer_auth(&key),
            )
            .await?;
            static_models(&[
                ("enhanced", "Enhanced transcription"),
                ("standard", "Standard transcription"),
            ])
        }
        "gladia" => {
            checked_json(
                shared_client()
                    .get("https://api.gladia.io/v2/pre-recorded")
                    .query(&[("page", "1"), ("limit", "1")])
                    .header("x-gladia-key", &key),
            )
            .await?;
            static_models(&[("default", "Gladia pre-recorded transcription")])
        }
        "revai" => {
            checked_json(
                shared_client()
                    .get("https://api.rev.ai/speechtotext/v1/account")
                    .bearer_auth(&key),
            )
            .await?;
            static_models(&[("machine", "Rev AI machine transcription")])
        }
        "elevenlabs" => {
            let json = checked_json(
                shared_client()
                    .get("https://api.elevenlabs.io/v1/models")
                    .header("xi-api-key", &key),
            )
            .await?;
            parse_elevenlabs_models(&json)
        }
        "together" => {
            let json = checked_json(
                shared_client()
                    .get("https://api.together.ai/v1/models")
                    .bearer_auth(&key),
            )
            .await?;
            parse_together_models(&json)
        }
        "fireworks" => {
            let direct = optional_json(
                shared_client()
                    .get("https://audio-turbo.us-virginia-1.direct.fireworks.ai/v1/models")
                    .bearer_auth(&key),
            )
            .await?;
            let json = match direct {
                Some(json) => json,
                None => {
                    checked_json(
                        shared_client()
                            .get("https://api.fireworks.ai/inference/v1/models")
                            .bearer_auth(&key),
                    )
                    .await?
                }
            };
            parse_fireworks_models(&json)
        }
        "deepinfra" => {
            let json = checked_json(
                shared_client()
                    .get("https://api.deepinfra.com/models/list")
                    .bearer_auth(&key),
            )
            .await?;
            parse_deepinfra_models(&json)
        }
        "lemonfox" => {
            let json = optional_json(
                shared_client()
                    .get("https://api.lemonfox.ai/v1/models")
                    .bearer_auth(&key),
            )
            .await?;
            json.as_ref()
                .map(parse_openai_models)
                .filter(|models| !models.is_empty())
                .unwrap_or_else(|| static_models(&[("whisper-1", "Whisper-1")]))
        }
        "cloudflare" => {
            let account_id = validated_cloudflare_account(spec, &settings)?;
            let json = checked_json(
                shared_client()
                    .get(format!(
                        "https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/models/search"
                    ))
                    .query(&[
                        ("task", "Automatic Speech Recognition"),
                        ("hide_experimental", "true"),
                        ("include_deprecated", "false"),
                        ("per_page", "100"),
                    ])
                    .bearer_auth(&key),
            )
            .await?;
            parse_cloudflare_models(&json)
        }
        "replicate" => {
            let json = checked_json(
                shared_client()
                    .get("https://api.replicate.com/v1/models/openai/whisper")
                    .bearer_auth(&key),
            )
            .await?;
            let version = json
                .pointer("/latest_version/id")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    "errors.cloud_response_parse::Replicate model version is missing".to_string()
                })?;
            vec![CloudModelInfo {
                id: format!("openai/whisper:{version}"),
                name: "OpenAI Whisper on Replicate".to_string(),
            }]
        }
        "huggingface" => {
            checked_json(
                shared_client()
                    .get("https://huggingface.co/api/whoami-v2")
                    .bearer_auth(&key),
            )
            .await?;
            let json = checked_json(
                shared_client()
                    .get("https://huggingface.co/api/models")
                    .query(&[
                        ("inference_provider", "hf-inference"),
                        ("pipeline_tag", "automatic-speech-recognition"),
                        ("sort", "trendingScore"),
                        ("direction", "-1"),
                        ("limit", "50"),
                    ])
                    .bearer_auth(&key),
            )
            .await?;
            parse_huggingface_models(&json)
        }
        "azure" => {
            let region = validated_region(spec, &settings)?;
            checked_bytes(
                shared_client()
                    .post(format!(
                        "https://{region}.api.cognitive.microsoft.com/sts/v1.0/issueToken"
                    ))
                    .header("Ocp-Apim-Subscription-Key", &key),
            )
            .await?;
            static_models(&[
                ("standard", "Azure Speech standard"),
                ("enhanced", "Azure Speech enhanced"),
            ])
        }
        "google-cloud" => {
            google_access_token(&key).await?;
            static_models(&[
                ("latest_long", "Latest long-form model"),
                ("latest_short", "Latest short-form model"),
                ("video", "Video model"),
                ("phone_call", "Phone call model"),
                ("command_and_search", "Command and search model"),
                ("default", "Default model"),
            ])
        }
        "google-ai-studio" => {
            let json = checked_json(
                shared_client()
                    .get("https://generativelanguage.googleapis.com/v1beta/models")
                    .header("x-goog-api-key", &key),
            )
            .await?;
            parse_gemini_transcription_models(&json)
        }
        "aws" => list_aws_models(spec, &settings, &key).await?,
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
    #[serde(default)]
    settings: HashMap<String, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProxyResponseMetadata {
    status: u16,
    status_text: String,
    headers: Vec<(String, String)>,
}

fn has_safe_path_suffix(path: &str, prefix: &str, trailing: &str) -> bool {
    let Some(value) = path
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(trailing))
    else {
        return false;
    };
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn endpoint_is_allowed(
    provider: &str,
    method: &str,
    url: &reqwest::Url,
    settings: &HashMap<String, String>,
) -> bool {
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    let host = url.host_str().unwrap_or_default();
    let path = url.path().trim_end_matches('/');
    let is_get = method.eq_ignore_ascii_case("GET");
    let is_post = method.eq_ignore_ascii_case("POST");
    match provider {
        "openai" => is_post && host == "api.openai.com" && path == "/v1/audio/transcriptions",
        "groq" => is_post && host == "api.groq.com" && path == "/openai/v1/audio/transcriptions",
        "deepgram" => is_post && host == "api.deepgram.com" && path == "/v1/listen",
        "assemblyai" => {
            host == "api.assemblyai.com"
                && ((is_post && matches!(path, "/v2/upload" | "/v2/transcript"))
                    || (is_get && has_safe_path_suffix(path, "/v2/transcript/", "")))
        }
        "gladia" => {
            host == "api.gladia.io"
                && ((is_post && matches!(path, "/v2/upload" | "/v2/pre-recorded"))
                    || (is_get
                        && (has_safe_path_suffix(path, "/v2/pre-recorded/", "")
                            || has_safe_path_suffix(path, "/v2/transcription/", ""))))
        }
        "revai" => {
            host == "api.rev.ai"
                && ((is_post && path == "/speechtotext/v1/jobs")
                    || (is_get
                        && (has_safe_path_suffix(path, "/speechtotext/v1/jobs/", "")
                            || has_safe_path_suffix(
                                path,
                                "/speechtotext/v1/jobs/",
                                "/transcript",
                            ))))
        }
        "elevenlabs" => is_post && host == "api.elevenlabs.io" && path == "/v1/speech-to-text",
        "together" => is_post && host == "api.together.ai" && path == "/v1/audio/transcriptions",
        "fireworks" => {
            is_post
                && host == "audio-turbo.us-virginia-1.direct.fireworks.ai"
                && path == "/v1/audio/transcriptions"
        }
        "lemonfox" => is_post && host == "api.lemonfox.ai" && path == "/v1/audio/transcriptions",
        "cloudflare" => {
            let Some(account_id) = settings.get("accountId").map(String::as_str) else {
                return false;
            };
            let prefix = format!("/client/v4/accounts/{account_id}/ai/run/");
            let model_path = path.strip_prefix(&prefix).unwrap_or_default();
            is_post
                && host == "api.cloudflare.com"
                && account_id.len() == 32
                && account_id.bytes().all(|byte| byte.is_ascii_hexdigit())
                && (model_path.starts_with("@cf/openai/whisper")
                    || model_path == "@cf/deepgram/nova-3")
                && !model_path.contains("realtime")
                && !model_path.contains("live")
                && !path.contains("..")
        }
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
    if !matches!(
        metadata.method.to_ascii_uppercase().as_str(),
        "GET" | "POST"
    ) {
        return Err("errors.invalid_byok_request".to_string());
    }
    let url = reqwest::Url::parse(&metadata.url)
        .map_err(|_| "errors.invalid_byok_request".to_string())?;
    if !endpoint_is_allowed(spec.id, &metadata.method, &url, &metadata.settings) {
        return Err("errors.invalid_byok_request".to_string());
    }

    let key = api_key(spec.id)?;
    let mut outbound = if metadata.method.eq_ignore_ascii_case("GET") {
        shared_client().get(url.clone())
    } else {
        shared_client().post(url.clone()).body(body)
    };
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
        "assemblyai" => outbound.header("Authorization", key),
        "gladia" => outbound.header("x-gladia-key", key),
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
    let response_body = read_response_body(response).await?;
    encode_proxy_response(
        ProxyResponseMetadata {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or_default().to_string(),
            headers: response_headers,
        },
        response_body,
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptionRequestMetadata {
    provider: String,
    model_id: String,
    language: Option<String>,
    media_type: String,
    #[serde(default)]
    settings: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderTranscriptionSegment {
    text: String,
    start_second: f64,
    end_second: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderTranscriptionResult {
    text: String,
    language: Option<String>,
    duration_in_seconds: Option<f64>,
    segments: Vec<ProviderTranscriptionSegment>,
}

impl ProviderTranscriptionResult {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            language: None,
            duration_in_seconds: None,
            segments: Vec::new(),
        }
    }
}

fn decode_transcription_metadata(
    request: &Request<'_>,
) -> Result<TranscriptionRequestMetadata, String> {
    let encoded = request
        .headers()
        .get("x-simplevoice-byok-transcription")
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

fn valid_model_id(model_id: &str) -> bool {
    !model_id.is_empty()
        && model_id.len() <= 300
        && model_id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':' | b'@')
        })
        && !model_id.contains("..")
        && !model_id.starts_with('/')
}

fn model_url(base: &str, model_id: &str, suffix: Option<&str>) -> Result<reqwest::Url, String> {
    if !valid_model_id(model_id) {
        return Err("errors.invalid_byok_request".to_string());
    }
    let mut url =
        reqwest::Url::parse(base).map_err(|_| "errors.invalid_byok_request".to_string())?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| "errors.invalid_byok_request".to_string())?;
        segments.pop_if_empty();
        for segment in model_id.split('/') {
            segments.push(segment);
        }
        if let Some(suffix) = suffix {
            segments.push(suffix);
        }
    }
    Ok(url)
}

fn locale_for_language(language: &str) -> &str {
    match language {
        "af" => "af-ZA",
        "ar" => "ar-SA",
        "bg" => "bg-BG",
        "cs" => "cs-CZ",
        "da" => "da-DK",
        "de" => "de-DE",
        "el" => "el-GR",
        "en" => "en-US",
        "es" => "es-ES",
        "fa" => "fa-IR",
        "fi" => "fi-FI",
        "fr" => "fr-FR",
        "he" => "he-IL",
        "hi" => "hi-IN",
        "hr" => "hr-HR",
        "hu" => "hu-HU",
        "id" => "id-ID",
        "it" => "it-IT",
        "ja" => "ja-JP",
        "ko" => "ko-KR",
        "ms" => "ms-MY",
        "nl" => "nl-NL",
        "no" => "no-NO",
        "pl" => "pl-PL",
        "pt" => "pt-BR",
        "ro" => "ro-RO",
        "ru" => "ru-RU",
        "sk" => "sk-SK",
        "sr" => "sr-RS",
        "sv" => "sv-SE",
        "sw" => "sw-KE",
        "th" => "th-TH",
        "tl" => "tl-PH",
        "tr" => "tr-TR",
        "uk" => "uk-UA",
        "vi" => "vi-VN",
        "zh" => "zh-CN",
        value => value,
    }
}

fn gemini_locale_for_language(language: &str) -> &str {
    match language {
        "ar" => "ar-EG",
        "es" => "es-419",
        "no" => "nb-NO",
        "tl" => "fil-PH",
        "zh" => "cmn-Hans-CN",
        value => locale_for_language(value),
    }
}

fn azure_locale_for_language(language: &str) -> &str {
    match language {
        "no" => "nb-NO",
        "tl" => "fil-PH",
        value => locale_for_language(value),
    }
}

fn google_cloud_locale_for_language(language: &str) -> &str {
    match language {
        "tl" => "fil-PH",
        value => locale_for_language(value),
    }
}

fn wav_pcm_bytes(audio: &[u8]) -> Result<Vec<u8>, String> {
    let reader = hound::WavReader::new(std::io::Cursor::new(audio))
        .map_err(|error| format!("errors.audio_encode_failed::{error}"))?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.sample_rate != crate::stt::chunker::SAMPLE_RATE as u32
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err("errors.audio_encode_failed::expected mono 16 kHz PCM WAV".to_string());
    }
    let samples = reader
        .into_samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("errors.audio_encode_failed::{error}"))?;
    let mut pcm = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        pcm.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(pcm)
}

fn json_segments(value: &serde_json::Value) -> Vec<ProviderTranscriptionSegment> {
    value
        .get("segments")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|segment| {
            let text = segment
                .get("text")
                .or_else(|| segment.get("word"))?
                .as_str()?
                .trim();
            if text.is_empty() {
                return None;
            }
            let start_second = segment
                .get("start")
                .or_else(|| segment.get("start_time"))
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0);
            let end_second = segment
                .get("end")
                .or_else(|| segment.get("end_time"))
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(start_second);
            Some(ProviderTranscriptionSegment {
                text: text.to_string(),
                start_second,
                end_second,
            })
        })
        .collect()
}

fn valid_job_id(job_id: &str) -> bool {
    !job_id.is_empty()
        && job_id.len() <= 200
        && job_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn rev_transcript_result(
    value: &serde_json::Value,
) -> (String, Vec<ProviderTranscriptionSegment>, Option<f64>) {
    let mut text_parts = Vec::new();
    let mut segments = Vec::new();
    let mut duration = None::<f64>;
    for monologue in value
        .get("monologues")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(elements) = monologue
            .get("elements")
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        let text = elements
            .iter()
            .filter_map(|element| element.get("value"))
            .filter_map(serde_json::Value::as_str)
            .collect::<String>();
        let start_second = elements
            .iter()
            .filter_map(|element| element.get("ts"))
            .find_map(serde_json::Value::as_f64);
        let end_second = elements
            .iter()
            .rev()
            .filter_map(|element| element.get("end_ts"))
            .find_map(serde_json::Value::as_f64);
        if let Some(end_second) = end_second {
            duration = Some(duration.unwrap_or_default().max(end_second));
        }
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        text_parts.push(text.to_string());
        if let (Some(start_second), Some(end_second)) = (start_second, end_second) {
            segments.push(ProviderTranscriptionSegment {
                text: text.to_string(),
                start_second,
                end_second,
            });
        }
    }
    (text_parts.join(" "), segments, duration)
}

async fn wait_for_rev_job(
    base_url: &str,
    job_id: &str,
    key: &str,
    completed_status: &str,
) -> Result<serde_json::Value, String> {
    if !valid_job_id(job_id) {
        return Err("errors.cloud_response_parse::invalid Rev AI job id".to_string());
    }
    let job_url = format!("{base_url}/{job_id}");
    for _ in 0..POLL_LIMIT {
        let job = checked_json(shared_client().get(&job_url).bearer_auth(key)).await?;
        let status = job
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if status == completed_status {
            return Ok(job);
        }
        if matches!(status, "failed" | "deleted" | "expired") {
            return Err(format!("errors.cloud_api_error::Rev AI job {status}"));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err("errors.cloud_request_failed::Rev AI job timed out".to_string())
}

async fn identify_revai_language(key: &str, audio: Vec<u8>) -> Result<String, String> {
    let form = reqwest::multipart::Form::new().part(
        "media",
        reqwest::multipart::Part::bytes(audio)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|error| format!("errors.audio_encode_failed::{error}"))?,
    );
    let submitted = checked_json(
        shared_client()
            .post("https://api.rev.ai/languageid/v1/jobs")
            .bearer_auth(key)
            .multipart(form),
    )
    .await?;
    let job_id = submitted
        .get("id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| valid_job_id(value))
        .ok_or_else(|| "errors.cloud_response_parse::missing Rev AI language job id".to_string())?;
    wait_for_rev_job(
        "https://api.rev.ai/languageid/v1/jobs",
        job_id,
        key,
        "completed",
    )
    .await?;
    let result = checked_json(
        shared_client()
            .get(format!(
                "https://api.rev.ai/languageid/v1/jobs/{job_id}/result"
            ))
            .bearer_auth(key)
            .header("Accept", "application/vnd.rev.languageid.v1.0+json"),
    )
    .await?;
    let _ = shared_client()
        .delete(format!("https://api.rev.ai/languageid/v1/jobs/{job_id}"))
        .bearer_auth(key)
        .send()
        .await;
    result
        .get("top_language")
        .and_then(serde_json::Value::as_str)
        .filter(|language| !language.is_empty() && language.len() <= 20)
        .map(str::to_string)
        .ok_or_else(|| "errors.cloud_response_parse::missing Rev AI language".to_string())
}

async fn transcribe_revai(
    model_id: &str,
    language: Option<&str>,
    key: &str,
    audio: Vec<u8>,
) -> Result<ProviderTranscriptionResult, String> {
    if model_id != "machine" {
        return Err("errors.invalid_byok_request".to_string());
    }
    let selected_language = match language {
        Some(language) => language.to_string(),
        None => identify_revai_language(key, audio.clone()).await?,
    };
    let config = serde_json::json!({
        "transcriber": model_id,
        "language": &selected_language,
        "skip_diarization": true
    });
    let form = reqwest::multipart::Form::new()
        .part(
            "media",
            reqwest::multipart::Part::bytes(audio)
                .file_name("audio.wav")
                .mime_str("audio/wav")
                .map_err(|error| format!("errors.audio_encode_failed::{error}"))?,
        )
        .text("config", config.to_string());
    let submitted = checked_json(
        shared_client()
            .post("https://api.rev.ai/speechtotext/v1/jobs")
            .bearer_auth(key)
            .multipart(form),
    )
    .await?;
    let job_id = submitted
        .get("id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| valid_job_id(value))
        .ok_or_else(|| "errors.cloud_response_parse::missing Rev AI job id".to_string())?;
    let job = wait_for_rev_job(
        "https://api.rev.ai/speechtotext/v1/jobs",
        job_id,
        key,
        "transcribed",
    )
    .await?;
    let transcript = checked_json(
        shared_client()
            .get(format!(
                "https://api.rev.ai/speechtotext/v1/jobs/{job_id}/transcript"
            ))
            .bearer_auth(key)
            .header("Accept", "application/vnd.rev.transcript.v1.0+json"),
    )
    .await?;
    let _ = shared_client()
        .delete(format!("https://api.rev.ai/speechtotext/v1/jobs/{job_id}"))
        .bearer_auth(key)
        .send()
        .await;
    let (text, segments, parsed_duration) = rev_transcript_result(&transcript);
    Ok(ProviderTranscriptionResult {
        text,
        language: job
            .get("language")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or(Some(selected_language)),
        duration_in_seconds: job
            .get("duration_seconds")
            .and_then(serde_json::Value::as_f64)
            .or(parsed_duration),
        segments,
    })
}

async fn transcribe_speechmatics(
    model_id: &str,
    language: Option<&str>,
    key: &str,
    audio: Vec<u8>,
) -> Result<ProviderTranscriptionResult, String> {
    if !matches!(model_id, "standard" | "enhanced") {
        return Err("errors.invalid_byok_request".to_string());
    }
    let mut config = serde_json::json!({
        "type": "transcription",
        "transcription_config": {
            "language": language.unwrap_or("auto"),
            "model": model_id,
            "enable_entities": false
        }
    });
    if language.is_none() {
        config["language_identification_config"] = serde_json::json!({
            "low_confidence_action": "allow"
        });
    }
    let form = reqwest::multipart::Form::new()
        .part(
            "data_file",
            reqwest::multipart::Part::bytes(audio)
                .file_name("audio.wav")
                .mime_str("audio/wav")
                .map_err(|error| format!("errors.audio_encode_failed::{error}"))?,
        )
        .text("config", config.to_string());
    let submitted = checked_json(
        shared_client()
            .post("https://asr.api.speechmatics.com/v2/jobs/")
            .bearer_auth(key)
            .multipart(form),
    )
    .await?;
    let job_id = submitted
        .get("id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| valid_job_id(value))
        .ok_or_else(|| "errors.cloud_response_parse::missing Speechmatics job id".to_string())?;
    let job_url = format!("https://asr.api.speechmatics.com/v2/jobs/{job_id}");
    for _ in 0..POLL_LIMIT {
        let status_json = checked_json(shared_client().get(&job_url).bearer_auth(key)).await?;
        let status = status_json
            .pointer("/job/status")
            .or_else(|| status_json.get("status"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if status == "done" {
            let body = checked_bytes(
                shared_client()
                    .get(format!("{job_url}/transcript"))
                    .query(&[("format", "txt")])
                    .bearer_auth(key),
            )
            .await?;
            let _ = shared_client()
                .delete(&job_url)
                .bearer_auth(key)
                .send()
                .await;
            return Ok(ProviderTranscriptionResult {
                text: String::from_utf8_lossy(&body).trim().to_string(),
                language: language.map(str::to_string),
                duration_in_seconds: None,
                segments: Vec::new(),
            });
        }
        if matches!(status, "rejected" | "deleted" | "expired" | "failed") {
            return Err(format!("errors.cloud_api_error::Speechmatics job {status}"));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err("errors.cloud_request_failed::Speechmatics transcription timed out".to_string())
}

async fn transcribe_deepinfra(
    model_id: &str,
    language: Option<&str>,
    key: &str,
    audio: Vec<u8>,
) -> Result<ProviderTranscriptionResult, String> {
    let url = model_url("https://api.deepinfra.com/v1/inference/", model_id, None)?;
    let mut form = reqwest::multipart::Form::new()
        .part(
            "audio",
            reqwest::multipart::Part::bytes(audio)
                .file_name("audio.wav")
                .mime_str("audio/wav")
                .map_err(|error| format!("errors.audio_encode_failed::{error}"))?,
        )
        .text("task", "transcribe");
    if let Some(language) = language {
        form = form.text("language", language.to_string());
    }
    let json = checked_json(shared_client().post(url).bearer_auth(key).multipart(form)).await?;
    let text = json
        .get("text")
        .or_else(|| json.get("transcription"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    Ok(ProviderTranscriptionResult {
        text,
        language: json
            .get("language")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| language.map(str::to_string)),
        duration_in_seconds: json.get("duration").and_then(serde_json::Value::as_f64),
        segments: json_segments(&json),
    })
}

fn replicate_output(json: &serde_json::Value) -> ProviderTranscriptionResult {
    let output = json.get("output").unwrap_or(json);
    if let Some(text) = output.as_str() {
        return ProviderTranscriptionResult::text(text.trim());
    }
    let text = output
        .get("transcription")
        .or_else(|| output.get("text"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    ProviderTranscriptionResult {
        text,
        language: output
            .get("detected_language")
            .or_else(|| output.get("language"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        duration_in_seconds: output.get("duration").and_then(serde_json::Value::as_f64),
        segments: json_segments(output),
    }
}

async fn transcribe_replicate(
    model_id: &str,
    language: Option<&str>,
    key: &str,
    audio: Vec<u8>,
) -> Result<ProviderTranscriptionResult, String> {
    let Some((model, version)) = model_id.split_once(':') else {
        return Err("errors.invalid_byok_request".to_string());
    };
    if model != "openai/whisper"
        || version.len() != 64
        || !version.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("errors.invalid_byok_request".to_string());
    }
    let data = base64::engine::general_purpose::STANDARD.encode(audio);
    let mut input = serde_json::json!({
        "audio": format!("data:audio/wav;base64,{data}"),
        "model": "large-v3",
        "transcription": "plain text",
        "translate": false,
        "temperature": 0
    });
    if let Some(language) = language {
        input["language"] = serde_json::Value::String(language.to_string());
    }
    let mut json = checked_json(
        shared_client()
            .post("https://api.replicate.com/v1/predictions")
            .bearer_auth(key)
            .header("Prefer", "wait=60")
            .json(&serde_json::json!({"version": version, "input": input})),
    )
    .await?;
    for _ in 0..POLL_LIMIT {
        match json
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
        {
            "succeeded" => return Ok(replicate_output(&json)),
            "failed" | "canceled" => {
                let detail = json
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("Replicate prediction failed");
                return Err(format!("errors.cloud_api_error::{}", truncate(detail, 400)));
            }
            _ => {}
        }
        let id = json
            .get("id")
            .and_then(serde_json::Value::as_str)
            .filter(|value| valid_job_id(value))
            .ok_or_else(|| {
                "errors.cloud_response_parse::missing Replicate prediction id".to_string()
            })?;
        tokio::time::sleep(Duration::from_secs(1)).await;
        json = checked_json(
            shared_client()
                .get(format!("https://api.replicate.com/v1/predictions/{id}"))
                .bearer_auth(key),
        )
        .await?;
    }
    Err("errors.cloud_request_failed::Replicate transcription timed out".to_string())
}

async fn transcribe_huggingface(
    model_id: &str,
    key: &str,
    audio: Vec<u8>,
) -> Result<ProviderTranscriptionResult, String> {
    let url = model_url(
        "https://router.huggingface.co/hf-inference/models/",
        model_id,
        None,
    )?;
    let json = checked_json(
        shared_client()
            .post(url)
            .bearer_auth(key)
            .header("Content-Type", "audio/wav")
            .header("x-wait-for-model", "true")
            .body(audio),
    )
    .await?;
    let text = json
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    Ok(ProviderTranscriptionResult {
        text,
        language: json
            .get("language")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        duration_in_seconds: None,
        segments: json_segments(&json),
    })
}

fn azure_text(json: &serde_json::Value) -> String {
    if let Some(values) = json
        .get("combinedPhrases")
        .and_then(serde_json::Value::as_array)
    {
        return values
            .iter()
            .filter_map(|value| value.get("text").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>()
            .join(" ");
    }
    json.get("DisplayText")
        .or_else(|| json.get("text"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

async fn transcribe_azure(
    provider: ProviderSpec,
    model_id: &str,
    language: Option<&str>,
    settings: &HashMap<String, String>,
    key: &str,
    audio: Vec<u8>,
) -> Result<ProviderTranscriptionResult, String> {
    if !matches!(model_id, "standard" | "enhanced") {
        return Err("errors.invalid_byok_request".to_string());
    }
    let region = validated_region(provider, settings)?;
    let locales: Vec<&str> = language
        .map(azure_locale_for_language)
        .into_iter()
        .collect();
    let definition = serde_json::json!({
        "locales": locales,
        "enhancedMode": {"enabled": model_id == "enhanced"}
    });
    let form = reqwest::multipart::Form::new()
        .part(
            "definition",
            reqwest::multipart::Part::text(definition.to_string())
                .mime_str("application/json")
                .map_err(|error| format!("errors.cloud_request_failed::{error}"))?,
        )
        .part(
            "audio",
            reqwest::multipart::Part::bytes(audio)
                .file_name("audio.wav")
                .mime_str("audio/wav")
                .map_err(|error| format!("errors.audio_encode_failed::{error}"))?,
        );
    let json = checked_json(
        shared_client()
            .post(format!(
                "https://{region}.api.cognitive.microsoft.com/speechtotext/transcriptions:transcribe"
            ))
            .query(&[("api-version", "2025-10-15")])
            .header("Ocp-Apim-Subscription-Key", key)
            .multipart(form),
    )
    .await?;
    let detected_language = json
        .get("combinedPhrases")
        .and_then(serde_json::Value::as_array)
        .and_then(|phrases| phrases.first())
        .or_else(|| {
            json.get("phrases")
                .and_then(serde_json::Value::as_array)
                .and_then(|phrases| phrases.first())
        })
        .and_then(|phrase| phrase.get("locale"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| language.map(azure_locale_for_language).map(str::to_string));
    Ok(ProviderTranscriptionResult {
        text: azure_text(&json).trim().to_string(),
        language: detected_language,
        duration_in_seconds: json
            .get("durationMilliseconds")
            .and_then(serde_json::Value::as_f64)
            .map(|value| value / 1000.0),
        segments: Vec::new(),
    })
}

async fn transcribe_google_cloud(
    model_id: &str,
    language: Option<&str>,
    credentials_json: &str,
    audio: Vec<u8>,
) -> Result<ProviderTranscriptionResult, String> {
    if !matches!(
        model_id,
        "latest_long" | "latest_short" | "video" | "phone_call" | "command_and_search" | "default"
    ) {
        return Err("errors.invalid_byok_request".to_string());
    }
    let (token, _) = google_access_token(credentials_json).await?;
    let pcm = wav_pcm_bytes(&audio)?;
    let duration = pcm.len() as f64 / 2.0 / crate::stt::chunker::SAMPLE_RATE as f64;
    let primary_language = language
        .map(google_cloud_locale_for_language)
        .unwrap_or("en-US");
    let mut config = serde_json::json!({
        "encoding": "LINEAR16",
        "sampleRateHertz": crate::stt::chunker::SAMPLE_RATE,
        "audioChannelCount": 1,
        "languageCode": primary_language,
        "model": model_id,
        "enableAutomaticPunctuation": true
    });
    if language.is_none() {
        config["alternativeLanguageCodes"] = serde_json::json!(["pl-PL", "de-DE", "fr-FR"]);
    }
    let json = checked_json(
        shared_client()
            .post("https://speech.googleapis.com/v1/speech:recognize")
            .bearer_auth(token)
            .json(&serde_json::json!({
                "config": config,
                "audio": {"content": base64::engine::general_purpose::STANDARD.encode(pcm)}
            })),
    )
    .await?;
    let text = json
        .get("results")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|result| result.pointer("/alternatives/0/transcript"))
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>()
        .join(" ");
    let detected_language = json
        .get("results")
        .and_then(serde_json::Value::as_array)
        .and_then(|results| results.first())
        .and_then(|result| result.get("languageCode"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(primary_language);
    Ok(ProviderTranscriptionResult {
        text: text.trim().to_string(),
        language: Some(detected_language.to_string()),
        duration_in_seconds: Some(duration),
        segments: Vec::new(),
    })
}

async fn transcribe_gemini(
    model_id: &str,
    language: Option<&str>,
    key: &str,
    audio: Vec<u8>,
) -> Result<ProviderTranscriptionResult, String> {
    if !model_id.to_ascii_lowercase().contains("transcribe") {
        return Err("errors.invalid_byok_request".to_string());
    }
    let language_codes: Vec<&str> = language
        .map(gemini_locale_for_language)
        .into_iter()
        .collect();
    let json = checked_json(
        shared_client()
            .post("https://generativelanguage.googleapis.com/v1beta/interactions")
            .header("x-goog-api-key", key)
            .json(&serde_json::json!({
                "model": model_id,
                "store": false,
                "input": [{
                    "type": "audio",
                    "data": base64::engine::general_purpose::STANDARD.encode(audio),
                    "mime_type": "audio/wav"
                }],
                "generation_config": {
                    "transcription_config": {
                        "language_codes": language_codes,
                        "mode": {"type": "verbatim"}
                    }
                }
            })),
    )
    .await?;
    let text = json
        .get("steps")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|step| step.get("type").and_then(serde_json::Value::as_str) == Some("model_output"))
        .filter_map(|step| step.get("content").and_then(serde_json::Value::as_array))
        .flatten()
        .filter_map(|content| content.get("text").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>()
        .join("");
    Ok(ProviderTranscriptionResult {
        text: text.trim().to_string(),
        language: language.map(gemini_locale_for_language).map(str::to_string),
        duration_in_seconds: None,
        segments: Vec::new(),
    })
}

async fn transcribe_cloudflare_nova(
    provider: ProviderSpec,
    model_id: &str,
    settings: &HashMap<String, String>,
    key: &str,
    audio: Vec<u8>,
) -> Result<ProviderTranscriptionResult, String> {
    if model_id != "@cf/deepgram/nova-3" {
        return Err("errors.invalid_byok_request".to_string());
    }
    let account_id = validated_cloudflare_account(provider, settings)?;
    let url = model_url(
        &format!("https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/run/"),
        model_id,
        None,
    )?;
    let json = checked_json(
        shared_client()
            .post(url)
            .bearer_auth(key)
            .header("Content-Type", "audio/wav")
            .body(audio),
    )
    .await?;
    let result = json.get("result").unwrap_or(&json);
    let alternative = result.pointer("/results/channels/0/alternatives/0");
    let text = alternative
        .and_then(|value| value.get("transcript"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let segments = alternative
        .and_then(|value| value.get("words"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|word| {
            Some(ProviderTranscriptionSegment {
                text: word.get("word")?.as_str()?.to_string(),
                start_second: word.get("start")?.as_f64()?,
                end_second: word.get("end")?.as_f64()?,
            })
        })
        .collect();
    Ok(ProviderTranscriptionResult {
        text,
        language: None,
        duration_in_seconds: None,
        segments,
    })
}

async fn transcribe_aws(
    provider: ProviderSpec,
    model_id: &str,
    language: Option<&str>,
    settings: &HashMap<String, String>,
    secret_access_key: &str,
    audio: Vec<u8>,
) -> Result<ProviderTranscriptionResult, String> {
    use aws_sdk_transcribestreaming::types::{
        AudioEvent, AudioStream, LanguageCode, MediaEncoding, TranscriptResultStream,
    };

    let (access_key_id, secret_access_key, region) =
        aws_credentials(provider, settings, secret_access_key)?;
    let config = aws_sdk_transcribestreaming::Config::builder()
        .region(aws_sdk_transcribestreaming::config::Region::new(region))
        .credentials_provider(aws_sdk_transcribestreaming::config::Credentials::new(
            access_key_id,
            secret_access_key,
            None,
            None,
            "simplevoice-byok",
        ))
        .build();
    let client = aws_sdk_transcribestreaming::Client::from_conf(config);
    let pcm = wav_pcm_bytes(&audio)?;
    let duration = pcm.len() as f64 / 2.0 / crate::stt::chunker::SAMPLE_RATE as f64;
    let chunks = pcm
        .chunks(6400)
        .map(<[u8]>::to_vec)
        .collect::<Vec<_>>()
        .into_iter();
    let input_stream = futures::stream::unfold(
        (chunks, false),
        |(mut chunks, delay_before_chunk)| async move {
            let chunk = chunks.next()?;
            if delay_before_chunk {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            let event = Ok::<_, aws_sdk_transcribestreaming::types::error::AudioStreamError>(
                AudioStream::AudioEvent(
                    AudioEvent::builder()
                        .audio_chunk(aws_sdk_transcribestreaming::primitives::Blob::new(chunk))
                        .build(),
                ),
            );
            Some((event, (chunks, true)))
        },
    );
    let mut request = client
        .start_stream_transcription()
        .media_sample_rate_hertz(crate::stt::chunker::SAMPLE_RATE as i32)
        .media_encoding(MediaEncoding::Pcm)
        .audio_stream(input_stream.into());

    let mut selected_language = language.map(locale_for_language).map(str::to_string);
    if let Some(custom) = model_id.strip_prefix("custom::") {
        let Some((model_name, model_language)) = custom.rsplit_once("::") else {
            return Err("errors.invalid_byok_request".to_string());
        };
        if !valid_model_id(model_name) || !valid_model_id(model_language) {
            return Err("errors.invalid_byok_request".to_string());
        }
        selected_language = Some(model_language.to_string());
        request = request
            .language_code(LanguageCode::from(model_language))
            .language_model_name(model_name);
    } else if model_id == "standard" {
        if let Some(language) = selected_language.as_deref() {
            request = request.language_code(LanguageCode::from(language));
        } else {
            request = request
                .identify_language(true)
                .language_options("en-US,pl-PL,de-DE,fr-FR,es-ES");
        }
    } else {
        return Err("errors.invalid_byok_request".to_string());
    }

    let mut output = request
        .send()
        .await
        .map_err(|error| format!("errors.cloud_api_error::{error}"))?;
    let mut text_parts = Vec::new();
    let mut segments = Vec::new();
    while let Some(event) = output
        .transcript_result_stream
        .recv()
        .await
        .map_err(|error| format!("errors.cloud_api_error::{error}"))?
    {
        if let TranscriptResultStream::TranscriptEvent(event) = event {
            for result in event
                .transcript
                .and_then(|transcript| transcript.results)
                .unwrap_or_default()
                .into_iter()
                .filter(|result| !result.is_partial)
            {
                if selected_language.is_none() {
                    selected_language = result
                        .language_code()
                        .map(|value| value.as_str().to_string());
                }
                if let Some(text) = result
                    .alternatives()
                    .first()
                    .and_then(|alternative| alternative.transcript())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    text_parts.push(text.to_string());
                    segments.push(ProviderTranscriptionSegment {
                        text: text.to_string(),
                        start_second: result.start_time(),
                        end_second: result.end_time(),
                    });
                }
            }
        }
    }
    Ok(ProviderTranscriptionResult {
        text: text_parts.join(" "),
        language: selected_language,
        duration_in_seconds: Some(duration),
        segments,
    })
}

#[tauri::command]
pub async fn byok_transcribe(request: Request<'_>) -> Result<Response, String> {
    let metadata = decode_transcription_metadata(&request)?;
    let audio = match request.body() {
        InvokeBody::Raw(bytes) => bytes.clone(),
        InvokeBody::Json(_) => return Err("errors.invalid_byok_request".to_string()),
    };
    if audio.is_empty()
        || audio.len() > MAX_TRANSCRIPTION_REQUEST_BYTES
        || metadata.media_type != "audio/wav"
        || !valid_model_id(metadata.model_id.trim())
    {
        return Err("errors.invalid_byok_request".to_string());
    }
    let spec = provider_spec(metadata.provider.trim())
        .ok_or_else(|| format!("errors.provider_no_transcription::{}", metadata.provider))?;
    let key = api_key(spec.id)?;
    let model_id = metadata.model_id.trim();
    let language = metadata.language.as_deref().filter(|value| {
        !value.is_empty()
            && value.len() <= 20
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    });
    let result = match spec.id {
        "revai" => transcribe_revai(model_id, language, &key, audio).await?,
        "speechmatics" => transcribe_speechmatics(model_id, language, &key, audio).await?,
        "deepinfra" => transcribe_deepinfra(model_id, language, &key, audio).await?,
        "replicate" => transcribe_replicate(model_id, language, &key, audio).await?,
        "huggingface" => transcribe_huggingface(model_id, &key, audio).await?,
        "azure" => {
            transcribe_azure(spec, model_id, language, &metadata.settings, &key, audio).await?
        }
        "google-cloud" => transcribe_google_cloud(model_id, language, &key, audio).await?,
        "google-ai-studio" => transcribe_gemini(model_id, language, &key, audio).await?,
        "cloudflare" => {
            transcribe_cloudflare_nova(spec, model_id, &metadata.settings, &key, audio).await?
        }
        "aws" => transcribe_aws(spec, model_id, language, &metadata.settings, &key, audio).await?,
        _ => return Err("errors.invalid_byok_request".to_string()),
    };
    let bytes = serde_json::to_vec(&result)
        .map_err(|error| format!("errors.cloud_response_parse::{error}"))?;
    Ok(Response::new(bytes))
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
    provider: String,
    audio_controller: tauri::State<'_, AudioController>,
    state: tauri::State<'_, CloudTranscriptionState>,
) -> Result<CloudTranscriptionPlan, String> {
    let spec = provider_spec(provider.trim())
        .ok_or_else(|| format!("errors.provider_no_transcription::{provider}"))?;
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
    let provider_id = spec.id;
    let (prepared, chunks) = tauri::async_runtime::spawn_blocking(move || {
        let prepared = crate::stt::prepare_samples(&samples);
        let chunks = match provider_id {
            "replicate" => crate::stt::chunker::split_at_silences_with_limit(&prepared, 10, 20),
            "google-cloud" => crate::stt::chunker::split_at_silences_with_limit(&prepared, 30, 55),
            _ => crate::stt::chunker::split_at_silences(&prepared),
        };
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
            {"id": "whisper-realtime", "active": true},
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
            {"model_id": "describe_v1", "name": "Description"},
            {"model_id": "scribe_v2_realtime", "name": "Scribe realtime"},
            {"model_id": "scribe_v2", "name": "Scribe v2"}
        ]));
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "scribe_v2");
    }

    #[test]
    fn proxy_rejects_non_provider_endpoints() {
        let settings = HashMap::new();
        assert!(endpoint_is_allowed(
            "openai",
            "POST",
            &reqwest::Url::parse("https://api.openai.com/v1/audio/transcriptions").unwrap(),
            &settings,
        ));
        assert!(!endpoint_is_allowed(
            "openai",
            "POST",
            &reqwest::Url::parse("https://example.com/v1/audio/transcriptions").unwrap(),
            &settings,
        ));
        assert!(!endpoint_is_allowed(
            "openai",
            "POST",
            &reqwest::Url::parse("http://api.openai.com/v1/audio/transcriptions").unwrap(),
            &settings,
        ));
        assert!(!endpoint_is_allowed(
            "openai",
            "GET",
            &reqwest::Url::parse("https://api.openai.com/v1/audio/transcriptions").unwrap(),
            &settings,
        ));
        assert!(endpoint_is_allowed(
            "together",
            "POST",
            &reqwest::Url::parse("https://api.together.ai/v1/audio/transcriptions").unwrap(),
            &settings,
        ));
        assert!(endpoint_is_allowed(
            "gladia",
            "GET",
            &reqwest::Url::parse("https://api.gladia.io/v2/transcription/job_123").unwrap(),
            &settings,
        ));
    }

    #[test]
    fn provider_catalog_contains_every_supported_byok_provider() {
        assert_eq!(PROVIDERS.len(), 19);
        assert_eq!(
            PROVIDERS
                .iter()
                .map(|provider| provider.id)
                .collect::<Vec<_>>(),
            vec![
                "openai",
                "groq",
                "deepgram",
                "assemblyai",
                "speechmatics",
                "gladia",
                "revai",
                "elevenlabs",
                "together",
                "fireworks",
                "deepinfra",
                "lemonfox",
                "cloudflare",
                "replicate",
                "huggingface",
                "azure",
                "google-cloud",
                "google-ai-studio",
                "aws",
            ]
        );
    }

    #[test]
    fn dynamic_catalog_filters_keep_only_batch_transcription_models() {
        let together = parse_together_models(&json!({"data": [
            {"id": "openai/whisper-large-v3"},
            {"id": "meta-llama/Llama-4"},
            {"id": "whisper-realtime"}
        ]}));
        assert_eq!(together.len(), 1);
        assert_eq!(together[0].id, "openai/whisper-large-v3");

        let cloudflare = parse_cloudflare_models(&json!({"result": [
            {"name": "@cf/openai/whisper-large-v3-turbo", "display_name": "Whisper"},
            {"name": "@cf/example/nova-3-tts", "display_name": "Nova TTS"},
            {"name": "@cf/meta/llama-4", "display_name": "Llama"}
        ]}));
        assert_eq!(cloudflare.len(), 1);
        assert_eq!(cloudflare[0].id, "@cf/openai/whisper-large-v3-turbo");

        let gemini = parse_gemini_transcription_models(&json!({"models": [
            {"name": "models/gemini-3.5-transcribe", "displayName": "Gemini Transcribe"},
            {"name": "models/gemini-3.5-transcribe-live"},
            {"name": "models/gemini-3.7-flash"}
        ]}));
        assert_eq!(gemini.len(), 1);
        assert_eq!(gemini[0].id, "gemini-3.5-transcribe");

        let deepinfra = parse_deepinfra_models(&json!({"data": [
            {"model_name": "openai/whisper-large-v3", "type": "audio", "reported_type": "automatic-speech-recognition"},
            {"model_name": "nvidia/parakeet-streaming", "type": "speech-recognition"},
            {"model_name": "hexgrad/Kokoro-82M", "type": "text-to-speech"},
            {"model_name": "legacy/whisper", "type": "speech-recognition", "deprecated": 1},
            {"model_name": "meta-llama/Llama-4", "type": "text-generation"}
        ]}));
        assert_eq!(deepinfra.len(), 1);
        assert_eq!(deepinfra[0].id, "openai/whisper-large-v3");

        let huggingface = parse_huggingface_models(&json!([
            {"id": "openai/whisper-large-v3", "pipeline_tag": "automatic-speech-recognition"},
            {"id": "hexgrad/Kokoro-82M", "pipeline_tag": "text-to-speech"},
            {"id": "unknown/no-capability"}
        ]));
        assert_eq!(huggingface.len(), 1);
        assert_eq!(huggingface[0].id, "openai/whisper-large-v3");
    }

    #[test]
    fn rev_transcript_parser_preserves_punctuation_and_timings() {
        let (text, segments, duration) = rev_transcript_result(&json!({
            "monologues": [{
                "elements": [
                    {"type": "text", "value": "Hello", "ts": 0.2, "end_ts": 0.6},
                    {"type": "punct", "value": ","},
                    {"type": "text", "value": " world", "ts": 0.7, "end_ts": 1.1},
                    {"type": "punct", "value": "."}
                ]
            }]
        }));
        assert_eq!(text, "Hello, world.");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start_second, 0.2);
        assert_eq!(segments[0].end_second, 1.1);
        assert_eq!(duration, Some(1.1));
    }

    #[test]
    fn providers_use_their_required_language_codes() {
        assert_eq!(gemini_locale_for_language("no"), "nb-NO");
        assert_eq!(gemini_locale_for_language("tl"), "fil-PH");
        assert_eq!(gemini_locale_for_language("zh"), "cmn-Hans-CN");
        assert_eq!(gemini_locale_for_language("pl"), "pl-PL");
        assert_eq!(azure_locale_for_language("no"), "nb-NO");
        assert_eq!(azure_locale_for_language("tl"), "fil-PH");
        assert_eq!(google_cloud_locale_for_language("tl"), "fil-PH");
        assert_eq!(locale_for_language("tl"), "tl-PH");
    }

    #[test]
    fn cloudflare_proxy_requires_the_matching_account_path() {
        let mut settings = HashMap::new();
        settings.insert(
            "accountId".to_string(),
            "0123456789abcdef0123456789abcdef".to_string(),
        );
        assert!(endpoint_is_allowed(
            "cloudflare",
            "POST",
            &reqwest::Url::parse(
                "https://api.cloudflare.com/client/v4/accounts/0123456789abcdef0123456789abcdef/ai/run/@cf/openai/whisper-large-v3-turbo",
            )
            .unwrap(),
            &settings,
        ));
        assert!(!endpoint_is_allowed(
            "cloudflare",
            "POST",
            &reqwest::Url::parse(
                "https://api.cloudflare.com/client/v4/accounts/ffffffffffffffffffffffffffffffff/ai/run/@cf/openai/whisper-large-v3-turbo",
            )
            .unwrap(),
            &settings,
        ));
        assert!(!endpoint_is_allowed(
            "cloudflare",
            "POST",
            &reqwest::Url::parse(
                "https://api.cloudflare.com/client/v4/accounts/0123456789abcdef0123456789abcdef/ai/run/@cf/meta/llama-4",
            )
            .unwrap(),
            &settings,
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
