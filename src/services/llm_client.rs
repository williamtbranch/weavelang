// src/services/llm_client.rs

use std::env;
use std::sync::{Arc, Mutex};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<AnthropicMessage>,
    temperature: f32,
}

#[derive(Deserialize, Debug)]
struct AnthropicContent {
    text: String,
}

#[derive(Deserialize, Debug)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Deserialize, Debug)]
struct AnthropicErrorResponse {
    error: AnthropicErrorDetail,
}

#[derive(Deserialize, Debug)]
struct AnthropicErrorDetail {
    message: String,
}

pub struct LlmClient {
    api_key: String,
    client: reqwest::blocking::Client,
}

impl LlmClient {
    pub fn new() -> Result<Self, String> {
        let api_key = env::var("ANTHROPIC_API_KEY")
            .map_err(|_| "ANTHROPIC_API_KEY not found in environment (.env)".to_string())?;

        let client = reqwest::blocking::Client::new();

        Ok(Self { api_key, client })
    }

    pub fn complete(
        &self, 
        model: &str, 
        system_prompt: &str, 
        user_prompt: &str
    ) -> Result<String, String> {
        
        let request_body = AnthropicRequest {
            model: model.to_string(),
            max_tokens: 4096,
            system: system_prompt.to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            }],
            temperature: 0.0,
        };

        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_str(&self.api_key).map_err(|e| e.to_string())?);
        headers.insert("anthropic-version", HeaderValue::from_static(ANTHROPIC_VERSION));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let response = self.client.post(ANTHROPIC_API_URL)
            .headers(headers)
            .json(&request_body)
            .send()
            .map_err(|e| format!("Request failed: {}", e))?;

        if !response.status().is_success() {
            let error_text = response.text().unwrap_or_default();
            if let Ok(err_json) = serde_json::from_str::<AnthropicErrorResponse>(&error_text) {
                return Err(format!("API Error: {}", err_json.error.message));
            }
            return Err(format!("API Error (Raw): {}", error_text));
        }

        // --- ENCODING FIX ---
        // Force UTF-8 decoding of the HTTP response.
        let response_text = response.text_with_charset("utf-8")
            .map_err(|e| format!("Failed to read response text: {}", e))?;

        let response_body: AnthropicResponse = serde_json::from_str(&response_text)
            .map_err(|e| format!("Failed to parse response JSON: {}", e))?;

        if let Some(first_block) = response_body.content.first() {
            Ok(first_block.text.clone())
        } else {
            Err("Response contained no content".to_string())
        }
    }
}

#[derive(Clone)]
pub struct LlmService {
    internal: Arc<Mutex<LlmClient>>,
}

impl LlmService {
    pub fn new() -> Result<Self, String> {
        let client = LlmClient::new()?;
        Ok(Self {
            internal: Arc::new(Mutex::new(client)),
        })
    }

    pub fn complete(&self, model: &str, system: &str, user: &str) -> Result<String, String> {
        let guard = self.internal.lock().map_err(|_| "Failed to lock LLM Client")?;
        guard.complete(model, system, user)
    }
}