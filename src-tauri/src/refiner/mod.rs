use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    temperature: f32,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    system: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Deserialize)]
struct AnthropicContentBlock {
    text: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiConfig {
    temperature: f32,
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "generationConfig")]
    generation_config: GeminiConfig,
}

#[derive(Deserialize)]
struct GeminiCandidatePart {
    text: String,
}

#[derive(Deserialize)]
struct GeminiCandidateContent {
    parts: Vec<GeminiCandidatePart>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiCandidateContent,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

pub async fn refine_text(
    text: &str,
    provider: &str,
    model: &str,
    api_key: &str,
    prompt: &str,
) -> Result<String, String> {
    let client = reqwest::Client::new();

    if provider == "openai" {
        let request = OpenAIRequest {
            model: model.to_string(),
            messages: vec![
                OpenAIMessage {
                    role: "system".to_string(),
                    content: prompt.to_string(),
                },
                OpenAIMessage {
                    role: "user".to_string(),
                    content: text.to_string(),
                },
            ],
            temperature: 0.2,
        };

        let response = client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(api_key)
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("Failed to send request to OpenAI: {}", e))?;

        if !response.status().is_success() {
            let err_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("OpenAI returned API error: {}", err_text));
        }

        let parsed = response
            .json::<OpenAIResponse>()
            .await
            .map_err(|e| format!("Failed to parse OpenAI JSON: {}", e))?;

        let refined = parsed
            .choices
            .first()
            .ok_or("OpenAI returned no chat choices")?
            .message
            .content
            .trim()
            .to_string();

        Ok(refined)
    } else if provider == "anthropic" {
        let request = AnthropicRequest {
            model: model.to_string(),
            system: prompt.to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: text.to_string(),
            }],
            max_tokens: 4096,
            temperature: 0.2,
        };

        let response = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("Failed to send request to Anthropic: {}", e))?;

        if !response.status().is_success() {
            let err_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("Anthropic returned API error: {}", err_text));
        }

        let parsed = response
            .json::<AnthropicResponse>()
            .await
            .map_err(|e| format!("Failed to parse Anthropic JSON: {}", e))?;

        let refined = parsed
            .content
            .first()
            .ok_or("Anthropic returned no content blocks")?
            .text
            .trim()
            .to_string();

        Ok(refined)
    } else if provider == "gemini" {
        let request = GeminiRequest {
            contents: vec![GeminiContent {
                parts: vec![GeminiPart {
                    text: format!(
                        "Instructions: {}\n\nInput Text to process and format:\n{}",
                        prompt, text
                    ),
                }],
            }],
            generation_config: GeminiConfig { temperature: 0.2 },
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model, api_key
        );

        let response = client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("Failed to send request to Gemini: {}", e))?;

        if !response.status().is_success() {
            let err_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("Gemini returned API error: {}", err_text));
        }

        let parsed = response
            .json::<GeminiResponse>()
            .await
            .map_err(|e| format!("Failed to parse Gemini JSON: {}", e))?;

        let refined = parsed
            .candidates
            .first()
            .ok_or("Gemini returned no candidates")?
            .content
            .parts
            .first()
            .ok_or("Gemini candidate contains no parts")?
            .text
            .trim()
            .to_string();

        Ok(refined)
    } else {
        Err(format!("Unknown LLM provider: {}", provider))
    }
}
