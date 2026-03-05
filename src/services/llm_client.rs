// src/services/llm_client.rs

use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use sha2::{Sha256, Digest};
use hex;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

// ---------------------------------------------------------------------------
// LlmProvider trait — Strategy Pattern for Dependency Injection
// ---------------------------------------------------------------------------

/// Strategy trait for LLM completion. Enables swapping the real Anthropic API
/// with a mock provider returning canned test responses.
pub trait LlmProvider: Send + Sync {
    /// Execute a completion request.
    fn complete(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String>;

    /// Set the current prompt/stage context.  The `MockLlmProvider` uses this
    /// to resolve which canned response file to read.  The real provider
    /// ignores it.
    fn set_context(&mut self, _prompt_name: &str) {}
}

// ---------------------------------------------------------------------------
// RealLlmProvider — wraps the existing Anthropic API client
// ---------------------------------------------------------------------------

/// Production LLM provider that calls the Anthropic API with SHA-256 caching.
pub struct RealLlmProvider {
    client: LlmClient,
}

impl RealLlmProvider {
    pub fn new(cache_root: Option<PathBuf>) -> Self {
        Self {
            client: LlmClient::new(cache_root),
        }
    }
}

impl LlmProvider for RealLlmProvider {
    fn complete(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String> {
        self.client.complete(model, system_prompt, user_prompt)
    }
}

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

#[derive(Serialize, Deserialize, Debug)]
struct CachedResponse {
    model: String,
    system: String,
    user: String,
    response: String,
}

pub struct LlmClient {
    client: reqwest::blocking::Client,
    cache_dir: Option<PathBuf>,
}

impl LlmClient {
    pub fn new(cache_root: Option<PathBuf>) -> Self {
        let client = reqwest::blocking::Client::new();

        let cache_dir = cache_root.map(|root| {
            let dir = root.join(".llm_cache");
            let _ = fs::create_dir_all(&dir);
            dir
        });

        Self { client, cache_dir }
    }

    fn get_cache_path(&self, hash: &str) -> Option<PathBuf> {
        self.cache_dir.as_ref().map(|dir| dir.join(format!("{}.json", hash)))
    }

    fn compute_hash(model: &str, system: &str, user: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(model.as_bytes());
        hasher.update(b"||");
        hasher.update(system.as_bytes());
        hasher.update(b"||");
        hasher.update(user.as_bytes());
        hex::encode(hasher.finalize())
    }

    pub fn complete(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String> {
        // Fetch the key at call time so the service can be initialised before the key is stored.
        let api_key = crate::services::secrets::get_anthropic_key()?;

        // 1. Check Cache
        let hash = Self::compute_hash(model, system_prompt, user_prompt);
        if let Some(cache_path) = self.get_cache_path(&hash) {
            if cache_path.exists() {
                if let Ok(content) = fs::read_to_string(&cache_path) {
                    if let Ok(cached) = serde_json::from_str::<CachedResponse>(&content) {
                        // Double check for collisions (unlikely but safe)
                        if cached.model == model && cached.system == system_prompt && cached.user == user_prompt {
                            return Ok(cached.response);
                        }
                    }
                }
            }
        }

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
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&api_key).map_err(|e| e.to_string())?,
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .headers(headers)
            .json(&request_body)
            .send()
            .map_err(|e| format!("Request failed: {e}"))?;

        if !response.status().is_success() {
            let error_text = response.text().unwrap_or_default();
            if let Ok(err_json) = serde_json::from_str::<AnthropicErrorResponse>(&error_text) {
                return Err(format!("API Error: {}", err_json.error.message));
            }
            return Err(format!("API Error (Raw): {error_text}"));
        }

        // Force UTF-8 decoding of the HTTP response.
        let response_text = response
            .text_with_charset("utf-8")
            .map_err(|e| format!("Failed to read response text: {e}"))?;

        let response_body: AnthropicResponse = serde_json::from_str(&response_text)
            .map_err(|e| format!("Failed to parse response JSON: {e}"))?;

        let result_text = if let Some(first_block) = response_body.content.first() {
            first_block.text.clone()
        } else {
            return Err("Response contained no content".to_string());
        };

        // 2. Write Cache
        if self.cache_dir.is_some() {
            let hash = Self::compute_hash(model, system_prompt, user_prompt);
            if let Some(cache_path) = self.get_cache_path(&hash) {
                let cached = serde_json::json!({
                    "system": system_prompt,
                    "user": user_prompt,
                    "response": result_text
                });
                if let Ok(json) = serde_json::to_string_pretty(&cached) {
                    let _ = std::fs::write(cache_path, json);
                }
            }
        }

        Ok(result_text)
    }
}

#[derive(Clone)]
pub struct LlmService {
    internal: Arc<Mutex<Box<dyn LlmProvider>>>,
}

impl LlmService {
    /// Create a new LlmService using the production Anthropic API provider.
    /// Always succeeds — the API key is fetched lazily at call time, not here.
    pub fn new(project_root: Option<PathBuf>) -> Self {
        let provider = RealLlmProvider::new(project_root);
        Self {
            internal: Arc::new(Mutex::new(Box::new(provider))),
        }
    }

    /// Create a new LlmService from any provider (real or mock).
    pub fn from_provider(provider: Box<dyn LlmProvider>) -> Self {
        Self {
            internal: Arc::new(Mutex::new(provider)),
        }
    }

    pub fn complete(&self, model: &str, system: &str, user: &str) -> Result<String, String> {
        let guard = self
            .internal
            .lock()
            .map_err(|_| "Failed to lock LLM Client")?;
        guard.complete(model, system, user)
    }

    /// Set the prompt/stage context on the underlying provider.
    /// Used by `LlmStageService` and `llm_segmenter` before issuing calls,
    /// so `MockLlmProvider` knows which canned file to read.
    pub fn set_context(&self, prompt_name: &str) {
        if let Ok(mut guard) = self.internal.lock() {
            guard.set_context(prompt_name);
        }
    }
}
