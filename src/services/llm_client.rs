// src/services/llm_client.rs

use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use sha2::{Sha256, Digest};
use hex;

use crate::config::ModelConfig;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";


// ---------------------------------------------------------------------------
// LlmProvider trait — Strategy Pattern for Dependency Injection
// ---------------------------------------------------------------------------

/// Strategy trait for LLM completion. Enables swapping the real Anthropic API
/// with a mock provider returning canned test responses.
pub trait LlmProvider: Send + Sync {
    /// Execute a completion request.
    /// `model` is the model alias key from the config (e.g., "gemini-pro", "sonnet").
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

    /// Update the models configuration map at runtime.
    fn update_models(&mut self, _models: HashMap<String, ModelConfig>) {}

    /// Update the Gemini thinking budget at runtime.
    fn update_thinking_budget(&mut self, _budget: Option<u32>) {}
}

// ---------------------------------------------------------------------------
// RoutingLlmProvider — resolves aliases and dispatches to correct API
// ---------------------------------------------------------------------------

/// Production LLM provider that resolves model aliases from the config,
/// determines the provider (Anthropic/Gemini), and routes the call accordingly.
pub struct RoutingLlmProvider {
    models: HashMap<String, ModelConfig>,
    anthropic: AnthropicClient,
    gemini: GeminiClient,
}

impl RoutingLlmProvider {
    pub fn new(cache_root: Option<PathBuf>, models: HashMap<String, ModelConfig>) -> Self {
        Self {
            models,
            anthropic: AnthropicClient::new(cache_root.clone()),
            gemini: GeminiClient::new(cache_root, None),
        }
    }

    pub fn new_with_thinking_budget(
        cache_root: Option<PathBuf>,
        models: HashMap<String, ModelConfig>,
        thinking_budget: Option<u32>,
    ) -> Self {
        Self {
            models,
            anthropic: AnthropicClient::new(cache_root.clone()),
            gemini: GeminiClient::new(cache_root, thinking_budget),
        }
    }
}

impl LlmProvider for RoutingLlmProvider {
    fn complete(
        &self,
        model_alias: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String> {
        let model_cfg = self.models.get(model_alias).ok_or_else(|| {
            format!(
                "Unknown model alias '{}'. Available models: [{}]. Check [models] section in config.toml",
                model_alias,
                self.models.keys().cloned().collect::<Vec<_>>().join(", ")
            )
        })?;

        let actual_name = &model_cfg.name;
        match model_cfg.provider.to_ascii_lowercase().as_str() {
            "claude" | "anthropic" => self.anthropic.complete(actual_name, system_prompt, user_prompt),
            "gemini" | "google" => self.gemini.complete(actual_name, system_prompt, user_prompt),
            other => Err(format!(
                "Unknown provider '{}' for model alias '{}'. Use 'claude'/'anthropic' or 'gemini'/'google'.",
                other, model_alias
            )),
        }
    }

    fn update_models(&mut self, models: HashMap<String, ModelConfig>) {
        self.models = models;
    }

    fn update_thinking_budget(&mut self, budget: Option<u32>) {
        self.gemini.thinking_budget = budget;
    }
}

// ---------------------------------------------------------------------------
// RealLlmProvider — legacy wrapper, kept for backward compatibility
// ---------------------------------------------------------------------------

/// Production LLM provider that calls the Anthropic API with SHA-256 caching.
/// DEPRECATED: Use RoutingLlmProvider instead for multi-provider support.
pub struct RealLlmProvider {
    client: AnthropicClient,
}

impl RealLlmProvider {
    pub fn new(cache_root: Option<PathBuf>) -> Self {
        Self {
            client: AnthropicClient::new(cache_root),
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

// ---------------------------------------------------------------------------
// Shared cache helpers
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug)]
struct CachedResponse {
    model: String,
    system: String,
    user: String,
    response: String,
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

/// Strip Gemma 4 thinking channel markers from response text.
/// Gemma 4 models embed reasoning between `<|channel>thought` and `<channel|>` tokens.
/// This function removes those blocks, returning only the answer portion.
fn strip_gemma_thinking(text: &str) -> String {
    // Look for the channel-end marker that terminates thinking
    if let Some(end_pos) = text.find("<channel|>") {
        let after = &text[end_pos + "<channel|>".len()..];
        return after.trim_start().to_string();
    }
    // Also handle a variant where thinking is between <|channel>thought ... <channel|>
    if let Some(start_pos) = text.find("<|channel>thought") {
        // If there's content before the thinking marker, keep it
        let before = text[..start_pos].trim_end();
        if !before.is_empty() {
            return before.to_string();
        }
    }
    text.to_string()
}

fn check_cache(cache_dir: &Option<PathBuf>, model: &str, system: &str, user: &str) -> Option<String> {
    let dir = cache_dir.as_ref()?;
    let hash = compute_hash(model, system, user);
    let cache_path = dir.join(format!("{}.json", hash));
    if cache_path.exists() {
        if let Ok(content) = fs::read_to_string(&cache_path) {
            if let Ok(cached) = serde_json::from_str::<CachedResponse>(&content) {
                if cached.model == model && cached.system == system && cached.user == user {
                    return Some(cached.response);
                }
            }
        }
    }
    None
}

fn write_cache(cache_dir: &Option<PathBuf>, model: &str, system: &str, user: &str, response: &str) {
    if let Some(dir) = cache_dir.as_ref() {
        let hash = compute_hash(model, system, user);
        let cache_path = dir.join(format!("{}.json", hash));
        let cached = serde_json::json!({
            "model": model,
            "system": system,
            "user": user,
            "response": response
        });
        if let Ok(json) = serde_json::to_string_pretty(&cached) {
            let _ = std::fs::write(cache_path, json);
        }
    }
}

// ---------------------------------------------------------------------------
// AnthropicClient — Anthropic/Claude API
// ---------------------------------------------------------------------------

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

pub struct AnthropicClient {
    client: reqwest::blocking::Client,
    cache_dir: Option<PathBuf>,
}

impl AnthropicClient {
    pub fn new(cache_root: Option<PathBuf>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("Failed to build HTTP client");
        let cache_dir = cache_root.map(|root| {
            let dir = root.join(".llm_cache");
            let _ = fs::create_dir_all(&dir);
            dir
        });
        Self { client, cache_dir }
    }

    pub fn complete(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String> {
        let api_key = crate::services::secrets::get_anthropic_key()?;

        // Check cache
        if let Some(cached) = check_cache(&self.cache_dir, model, system_prompt, user_prompt) {
            return Ok(cached);
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
            .map_err(|e| format!("Anthropic request failed (network): {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let status_code = status.as_u16();
            let error_text = response.text().unwrap_or_default();
            let detail = if let Ok(err_json) = serde_json::from_str::<AnthropicErrorResponse>(&error_text) {
                err_json.error.message
            } else {
                error_text
            };
            // Provide user-friendly error context based on HTTP status
            let hint = match status_code {
                401 => " (invalid API key — check 'set key anthropic <key>')",
                403 => " (forbidden — check API key permissions)",
                429 => " (rate limit reached — wait and retry, or reduce batch size)",
                404 => " (model not found — check the model name in config.toml)",
                _ => "",
            };
            return Err(format!(
                "Anthropic API Error (HTTP {}): {}{} [model: {}]",
                status_code, detail, hint, model
            ));
        }

        let response_text = response
            .text_with_charset("utf-8")
            .map_err(|e| format!("Failed to read Anthropic response: {e}"))?;

        let response_body: AnthropicResponse = serde_json::from_str(&response_text)
            .map_err(|e| format!("Failed to parse Anthropic JSON: {e}"))?;

        let result_text = if let Some(first_block) = response_body.content.first() {
            first_block.text.clone()
        } else {
            return Err("Anthropic response contained no content blocks".to_string());
        };

        write_cache(&self.cache_dir, model, system_prompt, user_prompt, &result_text);
        Ok(result_text)
    }
}

// ---------------------------------------------------------------------------
// GeminiClient — Google Gemini API
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Serialize, Deserialize, Debug)]
struct GeminiContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(default)]
    parts: Vec<GeminiPart>,
}

#[derive(Serialize, Deserialize, Debug)]
struct GeminiPart {
    #[serde(default)]
    text: String,
    /// True when this part contains model thinking/reasoning (not the answer).
    #[serde(default, skip_serializing)]
    thought: bool,
    /// Present in thinking-model responses; we ignore it.
    #[serde(rename = "thoughtSignature", default, skip_serializing)]
    _thought_signature: Option<String>,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    temperature: f32,
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,
    #[serde(rename = "thinkingConfig", skip_serializing_if = "Option::is_none")]
    thinking_config: Option<GeminiThinkingConfig>,
}

#[derive(Serialize)]
struct GeminiThinkingConfig {
    #[serde(rename = "thinkingBudget")]
    thinking_budget: u32,
}

#[derive(Deserialize, Debug)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    error: Option<GeminiErrorDetail>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Deserialize, Debug)]
struct GeminiUsageMetadata {
    #[serde(rename = "promptTokenCount", default)]
    prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount", default)]
    candidates_token_count: u32,
    #[serde(rename = "totalTokenCount", default)]
    total_token_count: u32,
    #[serde(rename = "thoughtsTokenCount", default)]
    thoughts_token_count: u32,
}

#[derive(Deserialize, Debug)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct GeminiErrorDetail {
    message: String,
    #[allow(dead_code)]
    code: Option<u16>,
}

pub struct GeminiClient {
    client: reqwest::blocking::Client,
    cache_dir: Option<PathBuf>,
    /// Thinking budget for models that support it (Gemini 2.5+).
    /// - `None` → dynamic thinking (model decides how much to think — recommended default)
    /// - `Some(0)` → thinking disabled
    /// - `Some(n)` → explicit budget of n tokens
    thinking_budget: Option<u32>,
}

impl GeminiClient {
    pub fn new(cache_root: Option<PathBuf>, thinking_budget: Option<u32>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("Failed to build HTTP client");
        let cache_dir = cache_root.map(|root| {
            let dir = root.join(".llm_cache");
            let _ = fs::create_dir_all(&dir);
            dir
        });
        Self { client, cache_dir, thinking_budget }
    }

    pub fn complete(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String> {
        let api_key = crate::services::secrets::get_google_key()?;

        // Check cache
        if let Some(cached) = check_cache(&self.cache_dir, model, system_prompt, user_prompt) {
            return Ok(cached);
        }

        // Build URL: models/{model}:generateContent?key=...
        // The model name may or may not have the "models/" prefix
        let model_path = if model.starts_with("models/") {
            model.to_string()
        } else {
            format!("models/{}", model)
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/{}:generateContent?key={}",
            model_path, api_key
        );

        // Only enable thinking for models that support it (Gemini 2.5+ series).
        // Gemma models and older Gemini models do not support thinkingBudget.
        let model_lower = model.to_lowercase();
        let supports_thinking = model_lower.contains("gemini-2.5")
            || model_lower.contains("gemini-3");

        // Thinking config:
        //   - Non-thinking model → None (omit entirely)
        //   - Thinking model + self.thinking_budget is None → None (dynamic thinking, model decides)
        //   - Thinking model + Some(0) → None (thinking disabled / dynamic)
        //   - Thinking model + Some(n) → explicit budget
        let thinking_config = if supports_thinking {
            match self.thinking_budget {
                Some(n) if n > 0 => Some(GeminiThinkingConfig { thinking_budget: n }),
                _ => None, // dynamic thinking — model decides how much to think
            }
        } else {
            None
        };

        let request_body = GeminiRequest {
            contents: vec![GeminiContent {
                role: Some("user".to_string()),
                parts: vec![GeminiPart {
                    text: user_prompt.to_string(),
                    thought: false,
                    _thought_signature: None,
                }],
            }],
            system_instruction: if system_prompt.is_empty() {
                None
            } else {
                Some(GeminiContent {
                    role: None,
                    parts: vec![GeminiPart {
                        text: system_prompt.to_string(),
                        thought: false,
                        _thought_signature: None,
                    }],
                })
            },
            generation_config: Some(GeminiGenerationConfig {
                temperature: 0.0,
                max_output_tokens: 32768,
                thinking_config,
            }),
        };

        const MAX_RETRIES: u32 = 3;
        let mut last_err = String::new();

        for attempt in 1..=MAX_RETRIES {
            let response = self
                .client
                .post(&url)
                .header(CONTENT_TYPE, "application/json")
                .json(&request_body)
                .send()
                .map_err(|e| format!("Gemini request failed (network): {e}"))?;

            let status = response.status();
            if !status.is_success() {
                let status_code = status.as_u16();
                let error_text = response.text().unwrap_or_default();
                let detail = if let Ok(err_json) = serde_json::from_str::<GeminiResponse>(&error_text) {
                    err_json
                        .error
                        .map(|e| e.message)
                        .unwrap_or(error_text.clone())
                } else {
                    error_text
                };
                let hint = match status_code {
                    400 => " (bad request — check model name in config.toml)",
                    401 | 403 => " (auth error — check 'set key google <key>')",
                    429 => " (rate limit / quota exceeded — wait and retry, or reduce batch size)",
                    404 => " (model not found — check the model name in config.toml)",
                    _ => "",
                };
                return Err(format!(
                    "Gemini API Error (HTTP {}): {}{} [model: {}]",
                    status_code, detail, hint, model
                ));
            }

            let response_text = response
                .text()
                .map_err(|e| format!("Failed to read Gemini response: {e}"))?;

            let response_body: GeminiResponse = serde_json::from_str(&response_text)
                .map_err(|e| format!("Failed to parse Gemini JSON: {e}"))?;

            // Log usage metadata (token counts including thinking)
            if let Some(ref usage) = response_body.usage_metadata {
                eprintln!(
                    "[Gemini] model={} | prompt={}tok, answer={}tok, thinking={}tok, total={}tok",
                    model,
                    usage.prompt_token_count,
                    usage.candidates_token_count,
                    usage.thoughts_token_count,
                    usage.total_token_count,
                );
            }

            if let Some(err) = response_body.error {
                return Err(format!("Gemini API Error: {} [model: {}]", err.message, model));
            }

            let first_candidate = response_body
                .candidates
                .and_then(|c| c.into_iter().next());

            let finish_reason: String = first_candidate
                .as_ref()
                .and_then(|c| c.finish_reason.as_deref())
                .unwrap_or("UNKNOWN")
                .to_string();

            // Separate thinking parts from answer parts
            let (thinking_text, answer_text): (Option<String>, Option<String>) = first_candidate
                .and_then(|c| c.content)
                .map(|c| {
                    let thought_parts: String = c.parts.iter()
                        .filter(|p| p.thought)
                        .map(|p| p.text.as_str())
                        .collect::<Vec<_>>()
                        .join("");

                    let mut answer = c.parts
                        .into_iter()
                        .filter(|p| !p.thought)
                        .map(|p| p.text)
                        .collect::<Vec<_>>()
                        .join("");

                    // Gemma 4 models embed thinking in channel tokens within the text itself.
                    answer = strip_gemma_thinking(&answer);

                    let thinking = if thought_parts.is_empty() { None } else { Some(thought_parts) };
                    let answer = if answer.is_empty() { None } else { Some(answer) };
                    (thinking, answer)
                })
                .unwrap_or((None, None));

            match answer_text {
                Some(text) => {
                    // Build a response string that includes thinking + usage as a
                    // log-friendly header.  Downstream parsers only look for `->`,
                    // `MAPPINGS:`, `VALIDATION:` etc., so they naturally skip this.
                    let mut full_response = String::new();

                    if let Some(ref usage) = response_body.usage_metadata {
                        full_response.push_str(&format!(
                            "--- USAGE: prompt={}tok  answer={}tok  thinking={}tok  total={}tok ---\n",
                            usage.prompt_token_count,
                            usage.candidates_token_count,
                            usage.thoughts_token_count,
                            usage.total_token_count,
                        ));
                    }

                    if let Some(ref thought) = thinking_text {
                        full_response.push_str("--- THINKING ---\n");
                        full_response.push_str(thought);
                        full_response.push_str("\n--- END THINKING ---\n");
                    }

                    full_response.push_str(&text);

                    // Cache only the clean answer (no thinking prefix)
                    write_cache(&self.cache_dir, model, system_prompt, user_prompt, &text);
                    return Ok(full_response);
                }
                None => {
                    last_err = format!(
                        "Gemini response contained no content (finishReason: {}, model: {})",
                        finish_reason, model
                    );
                    if attempt < MAX_RETRIES {
                        eprintln!(
                            "[llm_client] Empty Gemini response (attempt {}/{}), retrying in 2s…",
                            attempt, MAX_RETRIES
                        );
                        std::thread::sleep(std::time::Duration::from_secs(2));
                    }
                }
            }
        }

        Err(last_err)
    }
}

// ---------------------------------------------------------------------------
// LlmService — thread-safe wrapper around an LlmProvider
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct LlmService {
    internal: Arc<Mutex<Box<dyn LlmProvider>>>,
}

impl LlmService {
    /// Create a new LlmService with the routing provider that supports both
    /// Anthropic and Gemini APIs.  Model aliases are resolved from the config.
    pub fn new_routing(project_root: Option<PathBuf>, models: HashMap<String, ModelConfig>) -> Self {
        let provider = RoutingLlmProvider::new(project_root, models);
        Self {
            internal: Arc::new(Mutex::new(Box::new(provider))),
        }
    }

    /// Create a new LlmService with an explicit thinking budget for Gemini.
    /// `thinking_budget`:
    ///   - `None` → dynamic thinking (model decides — recommended)
    ///   - `Some(0)` → dynamic thinking
    ///   - `Some(n)` → explicit budget of n tokens
    pub fn new_routing_with_thinking(
        project_root: Option<PathBuf>,
        models: HashMap<String, ModelConfig>,
        thinking_budget: Option<u32>,
    ) -> Self {
        let provider = RoutingLlmProvider::new_with_thinking_budget(project_root, models, thinking_budget);
        Self {
            internal: Arc::new(Mutex::new(Box::new(provider))),
        }
    }

    /// Create a new LlmService using only the Anthropic API provider (legacy).
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

    /// Update the models configuration at runtime (e.g., after settings change).
    pub fn update_models(&self, models: HashMap<String, ModelConfig>) {
        if let Ok(mut guard) = self.internal.lock() {
            guard.update_models(models);
        }
    }

    /// Update the Gemini thinking budget at runtime.
    pub fn update_thinking_budget(&self, budget: Option<u32>) {
        if let Ok(mut guard) = self.internal.lock() {
            guard.update_thinking_budget(budget);
        }
    }
}
