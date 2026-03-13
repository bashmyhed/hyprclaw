//! LLM client for HTTP communication with Python service.

use crate::interfaces::RuntimeError;
use crate::types::{LLMResponse, Message};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// Request payload for LLM service.
#[derive(Debug, Serialize)]
struct LLMRequest {
    system_prompt: String,
    messages: Vec<Message>,
    tools: Vec<serde_json::Value>,
}

/// OpenAI-compatible request format for NVIDIA/Google
#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

/// OpenAI-compatible response format
#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAIMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIToolCall {
    function: OpenAIFunction,
}

#[derive(Debug, Deserialize)]
struct OpenAIFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIModelsResponse {
    data: Vec<OpenAIModelEntry>,
}

#[derive(Debug, Deserialize)]
struct OpenAIModelEntry {
    id: String,
}

/// Circuit breaker state.
struct CircuitBreaker {
    consecutive_failures: AtomicUsize,
    breaker_open: AtomicBool,
    opened_at: Mutex<Option<Instant>>,
    failure_threshold: usize,
    cooldown_duration: Duration,
}

impl CircuitBreaker {
    fn new(failure_threshold: usize, cooldown_duration: Duration) -> Self {
        Self {
            consecutive_failures: AtomicUsize::new(0),
            breaker_open: AtomicBool::new(false),
            opened_at: Mutex::new(None),
            failure_threshold,
            cooldown_duration,
        }
    }

    fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.breaker_open.store(false, Ordering::SeqCst);
        *self.opened_at.lock() = None;
    }

    fn record_failure(&self) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        if failures >= self.failure_threshold {
            self.breaker_open.store(true, Ordering::SeqCst);
            *self.opened_at.lock() = Some(Instant::now());
        }
    }

    fn should_allow_request(&self) -> Result<(), RuntimeError> {
        if !self.breaker_open.load(Ordering::SeqCst) {
            return Ok(());
        }

        let opened_at = self.opened_at.lock();
        if let Some(opened_time) = *opened_at {
            if opened_time.elapsed() >= self.cooldown_duration {
                drop(opened_at);
                // Allow trial request
                return Ok(());
            }
        }

        Err(RuntimeError::LLMError(
            "Circuit breaker open: LLM service unavailable".to_string(),
        ))
    }
}

/// LLM client for calling Python service via HTTP.
#[derive(Clone)]
pub struct LLMClient {
    base_url: String,
    client: reqwest::Client,
    max_retries: u32,
    circuit_breaker: Arc<CircuitBreaker>,
    api_key: Option<String>,
    model: Arc<RwLock<Option<String>>>,
}

impl LLMClient {
    /// Create a new LLM client.
    ///
    /// # Arguments
    /// * `base_url` - Base URL of the LLM service
    /// * `max_retries` - Maximum number of retries on failure
    pub fn new(base_url: String, max_retries: u32) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            base_url,
            client,
            max_retries,
            circuit_breaker: Arc::new(CircuitBreaker::new(5, Duration::from_secs(30))),
            api_key: None,
            model: Arc::new(RwLock::new(None)),
        }
    }

    /// Create a new LLM client with API key for authentication.
    pub fn with_api_key(base_url: String, max_retries: u32, api_key: String) -> Self {
        let mut client = Self::new(base_url, max_retries);
        client.api_key = Some(api_key);
        client
    }

    /// Create a new LLM client with API key and model.
    pub fn with_api_key_and_model(
        base_url: String,
        max_retries: u32,
        api_key: String,
        model: String,
    ) -> Self {
        let client = Self::with_api_key(base_url, max_retries, api_key);
        client.set_model(&model).ok();
        client
    }

    /// Set active model for OpenAI-compatible providers.
    pub fn set_model(&self, model: &str) -> Result<(), RuntimeError> {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            return Err(RuntimeError::LLMError("Model cannot be empty".to_string()));
        }
        *self.model.write() = Some(trimmed.to_string());
        Ok(())
    }

    /// Get currently active model.
    pub fn current_model(&self) -> Option<String> {
        self.model.read().clone()
    }

    /// List models from provider OpenAI-compatible `/models` endpoint.
    pub async fn list_models(&self) -> Result<Vec<String>, RuntimeError> {
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let mut req_builder = self.client.get(&url);

        if let Some(api_key) = &self.api_key {
            req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
        }

        let response = req_builder.send().await.map_err(|e| {
            RuntimeError::LLMError(format!("Failed to query models endpoint: {}", e))
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read models response".to_string());
            return Err(RuntimeError::LLMError(format!(
                "Failed to list models: HTTP {}. Details: {}",
                status, body
            )));
        }

        let body = response.text().await.map_err(|e| {
            RuntimeError::LLMError(format!("Failed to read models response: {}", e))
        })?;

        let mut models: Vec<String> = Vec::new();

        if let Ok(parsed) = serde_json::from_str::<OpenAIModelsResponse>(&body) {
            models.extend(parsed.data.into_iter().map(|m| m.id));
        } else if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(items) = value.get("data").and_then(|v| v.as_array()) {
                for item in items {
                    if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                        models.push(id.to_string());
                    }
                }
            }
        }

        models.sort();
        models.dedup();

        if models.is_empty() {
            if let Some(current) = self.current_model() {
                return Ok(vec![current]);
            }
            return Err(RuntimeError::LLMError(
                "Models endpoint returned no model IDs".to_string(),
            ));
        }

        Ok(models)
    }

    fn model_generation_profile(&self, model: &str) -> (Option<f32>, Option<f32>, Option<u32>) {
        let m = model.to_lowercase();
        // GLM-4.7 guidance for terminal/agentic workloads.
        if m == "z-ai/glm4.7" || m.ends_with("/glm4.7") {
            return (Some(0.7), Some(1.0), Some(16_384));
        }
        // Stable defaults for tool-driven automation.
        (Some(0.2), Some(0.95), Some(4_096))
    }

    fn retry_delay_for_error(&self, attempt: u32, err: &RuntimeError) -> Duration {
        let msg = err.to_string();
        let lower = msg.to_lowercase();
        if lower.contains("rate limit")
            || lower.contains("too many requests")
            || lower.contains("resource_exhausted")
            || lower.contains("429")
        {
            if let Some(seconds) = extract_retry_seconds(&msg) {
                return Duration::from_secs(seconds.min(90));
            }
            return Duration::from_secs((2_u64.saturating_pow(attempt + 1)).min(30));
        }
        Duration::from_millis((250_u64.saturating_mul(2_u64.saturating_pow(attempt))).min(5000))
    }

    fn should_retry_error(&self, err: &RuntimeError) -> bool {
        let msg = err.to_string().to_lowercase();

        if msg.contains("authentication failed")
            || msg.contains("401 unauthorized")
            || msg.contains("invalid endpoint (404")
            || msg.contains("invalid_argument")
            || msg.contains("400 bad request")
            || msg.contains("models endpoint returned no model ids")
        {
            return false;
        }

        // Do not keep retrying on hard quota exhaustion.
        if msg.contains("generaterequestsperday")
            || msg.contains("perday")
            || msg.contains("daily quota")
            || msg.contains("quota exhausted")
        {
            return false;
        }

        true
    }

    /// Call LLM service with retry logic.
    ///
    /// # Arguments
    /// * `system_prompt` - System prompt for the LLM
    /// * `messages` - Conversation history
    /// * `tool_schemas` - Available tool schemas in OpenAI function format
    ///
    /// # Returns
    /// Normalized LLMResponse
    ///
    /// # Errors
    /// Returns error if all retries fail
    pub async fn call(
        &self,
        system_prompt: &str,
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
    ) -> Result<LLMResponse, RuntimeError> {
        let _timer = crate::metrics::MetricTimer::new("llm_request_latency");

        // Check circuit breaker
        self.circuit_breaker.should_allow_request()?;

        // CRITICAL: Validate tools are not empty
        if tool_schemas.is_empty() {
            return Err(RuntimeError::LLMError(
                "Cannot call LLM with empty tool schemas. Agent must have tools registered."
                    .to_string(),
            ));
        }

        let mut last_error = None;
        let mut attempts_used = 0u32;

        for attempt in 0..=self.max_retries {
            attempts_used = attempt + 1;
            debug!("LLM call attempt {}/{}", attempt + 1, self.max_retries + 1);

            match self.call_once(system_prompt, messages, tool_schemas).await {
                Ok(response) => {
                    self.circuit_breaker.record_success();
                    return Ok(response);
                }
                Err(e) => {
                    warn!("LLM call failed (attempt {}): {}", attempt + 1, e);
                    let retryable = self.should_retry_error(&e);
                    let delay = self.retry_delay_for_error(attempt, &e);
                    last_error = Some(e);
                    if retryable && attempt < self.max_retries {
                        tokio::time::sleep(delay).await;
                    } else {
                        break;
                    }
                }
            }
        }

        self.circuit_breaker.record_failure();

        Err(RuntimeError::LLMError(format!(
            "LLM call failed after {} attempts: {}",
            attempts_used,
            last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "Unknown error".to_string())
        )))
    }

    async fn call_once(
        &self,
        system_prompt: &str,
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
    ) -> Result<LLMResponse, RuntimeError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let active_model = self.current_model();

        let mut req_builder = self.client.post(&url);

        // Use OpenAI format if model is specified (NVIDIA/Google), otherwise use custom format
        if let Some(model) = &active_model {
            // Convert to OpenAI format
            let mut openai_messages = Vec::new();
            let mut last_tool_call_id: Option<String> = None;

            // Add system message if present
            if !system_prompt.is_empty() {
                openai_messages.push(serde_json::json!({
                    "role": "system",
                    "content": system_prompt
                }));
            }

            // Add conversation messages
            for msg in messages {
                let role_str = match msg.role {
                    crate::types::Role::User => "user",
                    crate::types::Role::Assistant => "assistant",
                    crate::types::Role::Tool => "tool",
                    crate::types::Role::System => "system",
                };

                // Encode tool call turns in strict OpenAI format for provider compatibility.
                if matches!(msg.role, crate::types::Role::Assistant)
                    && msg
                        .metadata
                        .as_ref()
                        .and_then(|m| m.get("tool_call"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                {
                    let tool_name = msg
                        .metadata
                        .as_ref()
                        .and_then(|m| m.get("tool_name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown_tool");
                    let tool_input = msg
                        .metadata
                        .as_ref()
                        .and_then(|m| m.get("input"))
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({}));
                    let call_id = format!("call_{}", openai_messages.len());
                    last_tool_call_id = Some(call_id.clone());
                    openai_messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": serde_json::Value::Null,
                        "tool_calls": [{
                            "id": call_id,
                            "type": "function",
                            "function": {
                                "name": tool_name,
                                "arguments": serde_json::to_string(&tool_input).unwrap_or_else(|_| "{}".to_string())
                            }
                        }]
                    }));
                    continue;
                }

                if matches!(msg.role, crate::types::Role::Tool) {
                    let tool_call_id = last_tool_call_id
                        .clone()
                        .unwrap_or_else(|| "call_0".to_string());
                    openai_messages.push(serde_json::json!({
                        "role": role_str,
                        "tool_call_id": tool_call_id,
                        "content": msg.content.to_string()
                    }));
                    continue;
                }

                let content_value = if let Some(s) = msg.content.as_str() {
                    serde_json::Value::String(s.to_string())
                } else {
                    serde_json::Value::String(msg.content.to_string())
                };
                openai_messages.push(serde_json::json!({
                    "role": role_str,
                    "content": content_value
                }));
            }

            let (temperature, top_p, max_tokens) = self.model_generation_profile(model);
            let openai_request = OpenAIRequest {
                model: model.clone(),
                messages: openai_messages,
                tools: Some(tool_schemas.to_vec()),
                tool_choice: Some("auto".to_string()),
                temperature,
                top_p,
                max_tokens,
            };

            debug!("llm url={}", url);
            debug!("llm api_key_present={}", self.api_key.is_some());
            debug!("llm tools_count={}", tool_schemas.len());

            req_builder = req_builder.json(&openai_request);
        } else {
            // Use custom format for local/Python service
            let request = LLMRequest {
                system_prompt: system_prompt.to_string(),
                messages: messages.to_vec(),
                tools: tool_schemas.to_vec(),
            };

            debug!("llm url={}", url);
            debug!("llm tools_count={}", tool_schemas.len());

            req_builder = req_builder.json(&request);
        }

        // Add Authorization header if API key is present
        if let Some(api_key) = &self.api_key {
            req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
        }

        let response = req_builder.send().await.map_err(|e| {
            if e.is_connect() || e.is_timeout() {
                RuntimeError::LLMError("Network connection failed".to_string())
            } else {
                RuntimeError::LLMError(format!("HTTP request failed: {}", e))
            }
        })?;

        let status = response.status();
        if !status.is_success() {
            // Try to get error details from response body
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error response".to_string());

            let error_msg = match status.as_u16() {
                401 => {
                    if self.api_key.is_some() {
                        format!(
                            "Authentication failed. Check your API key. Details: {}",
                            error_body
                        )
                    } else {
                        format!(
                            "Authentication required (401 Unauthorized). Details: {}",
                            error_body
                        )
                    }
                }
                404 => format!("Invalid endpoint (404 Not Found). Details: {}", error_body),
                429 => {
                    if self.api_key.is_some() {
                        format!("Rate limit exceeded. Details: {}", error_body)
                    } else {
                        format!(
                            "Rate limit exceeded (429 Too Many Requests). Details: {}",
                            error_body
                        )
                    }
                }
                500..=599 => {
                    if self.api_key.is_some() {
                        format!("LLM service error. Details: {}", error_body)
                    } else {
                        format!("Server error: {}. Details: {}", status, error_body)
                    }
                }
                _ => format!("HTTP error: {}. Details: {}", status, error_body),
            };
            return Err(RuntimeError::LLMError(error_msg));
        }

        let llm_response: LLMResponse =
            if active_model.is_some() {
                // Parse OpenAI format response
                let openai_response: OpenAIResponse = response.json().await.map_err(|e| {
                    RuntimeError::LLMError(format!("Failed to parse response: {}", e))
                })?;

                // Convert to our format
                if let Some(choice) = openai_response.choices.first() {
                    if let Some(tool_calls) = &choice.message.tool_calls {
                        if let Some(tool_call) = tool_calls.first() {
                            LLMResponse::ToolCall {
                                schema_version: crate::types::SCHEMA_VERSION,
                                tool_name: tool_call.function.name.clone(),
                                input: serde_json::from_str(&tool_call.function.arguments)
                                    .unwrap_or(serde_json::json!({})),
                            }
                        } else {
                            let content = choice.message.content.clone().unwrap_or_default();
                            if let Some((tool_name, input)) = parse_inline_tool_call(&content) {
                                LLMResponse::ToolCall {
                                    schema_version: crate::types::SCHEMA_VERSION,
                                    tool_name,
                                    input,
                                }
                            } else {
                                LLMResponse::Final {
                                    schema_version: crate::types::SCHEMA_VERSION,
                                    content,
                                }
                            }
                        }
                    } else {
                        let content = choice.message.content.clone().unwrap_or_default();
                        if let Some((tool_name, input)) = parse_inline_tool_call(&content) {
                            LLMResponse::ToolCall {
                                schema_version: crate::types::SCHEMA_VERSION,
                                tool_name,
                                input,
                            }
                        } else {
                            LLMResponse::Final {
                                schema_version: crate::types::SCHEMA_VERSION,
                                content,
                            }
                        }
                    }
                } else {
                    return Err(RuntimeError::LLMError("No choices in response".to_string()));
                }
            } else {
                // Parse custom format response
                response.json().await.map_err(|e| {
                    RuntimeError::LLMError(format!("Failed to parse response: {}", e))
                })?
            };

        self.validate_response(&llm_response)?;

        Ok(llm_response)
    }

    fn validate_response(&self, response: &LLMResponse) -> Result<(), RuntimeError> {
        match response {
            LLMResponse::Final { content, .. } => {
                if content.is_empty() {
                    return Err(RuntimeError::LLMError(
                        "Final response has empty content".to_string(),
                    ));
                }
            }
            LLMResponse::ToolCall { tool_name, .. } => {
                if tool_name.is_empty() {
                    return Err(RuntimeError::LLMError(
                        "Tool call missing tool_name".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}

fn extract_retry_seconds(msg: &str) -> Option<u64> {
    let mut num = String::new();
    for ch in msg.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            num.push(ch);
            continue;
        }
        if matches!(ch, 's' | 'S') && !num.is_empty() {
            if let Ok(v) = num.parse::<f64>() {
                if v > 0.0 {
                    return Some(v.ceil() as u64);
                }
            }
        }
        num.clear();
    }
    None
}

fn parse_inline_tool_call(content: &str) -> Option<(String, serde_json::Value)> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let (Some(start), Some(end)) = (trimmed.find("<tool_call>"), trimmed.find("</tool_call>")) {
        if end > start + "<tool_call>".len() {
            let inner = &trimmed[start + "<tool_call>".len()..end];
            if let Some(parsed) = parse_tool_call_payload(inner.trim()) {
                return Some(parsed);
            }
        }
    }

    parse_tool_call_payload(trimmed)
}

fn parse_tool_call_payload(payload: &str) -> Option<(String, serde_json::Value)> {
    if payload.is_empty() {
        return None;
    }

    if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(payload) {
        if let Some((tool_name, input)) = extract_tool_from_json(&json_value) {
            return Some((tool_name, input));
        }
    }

    if let Some(start) = payload.find('{') {
        let (name_part, json_part) = payload.split_at(start);
        let candidate = name_part.trim().trim_matches('"').trim_matches('\'');
        if looks_like_tool_name(candidate) {
            if let Ok(input) = serde_json::from_str::<serde_json::Value>(json_part) {
                return Some((candidate.to_string(), input));
            }
        }
    }

    if looks_like_tool_name(payload) {
        return Some((payload.to_string(), serde_json::json!({})));
    }

    None
}

fn extract_tool_from_json(value: &serde_json::Value) -> Option<(String, serde_json::Value)> {
    let obj = value.as_object()?;

    let tool_name = obj
        .get("tool_name")
        .or_else(|| obj.get("name"))
        .or_else(|| obj.get("tool"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|name| looks_like_tool_name(name))?
        .to_string();

    let input = obj
        .get("input")
        .cloned()
        .or_else(|| obj.get("arguments").cloned())
        .unwrap_or_else(|| serde_json::json!({}));

    Some((tool_name, input))
}

fn looks_like_tool_name(candidate: &str) -> bool {
    if candidate.len() < 3 || candidate.len() > 128 {
        return false;
    }
    candidate
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        && (candidate.contains('.') || candidate.contains('_'))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::types::Role;
    use serde_json::json;

    // Mock server tests would use wiremock or similar
    // For now, we test the validation logic

    #[test]
    fn test_validate_final_response() {
        let client = LLMClient::new("http://localhost:8000".to_string(), 1);

        let response = LLMResponse::Final {
            schema_version: crate::types::SCHEMA_VERSION,
            content: "Hello".to_string(),
        };
        assert!(client.validate_response(&response).is_ok());
    }

    #[test]
    fn test_validate_empty_final_response() {
        let client = LLMClient::new("http://localhost:8000".to_string(), 1);

        let response = LLMResponse::Final {
            schema_version: crate::types::SCHEMA_VERSION,
            content: "".to_string(),
        };
        let result = client.validate_response(&response);
        assert!(result.is_err());
        match result {
            Err(RuntimeError::LLMError(msg)) => {
                assert!(msg.contains("empty content"));
            }
            _ => panic!("Expected LLMError"),
        }
    }

    #[test]
    fn test_validate_tool_call_response() {
        let client = LLMClient::new("http://localhost:8000".to_string(), 1);

        let response = LLMResponse::ToolCall {
            schema_version: crate::types::SCHEMA_VERSION,
            tool_name: "search".to_string(),
            input: json!({"query": "test"}),
        };
        assert!(client.validate_response(&response).is_ok());
    }

    #[test]
    fn test_validate_empty_tool_name() {
        let client = LLMClient::new("http://localhost:8000".to_string(), 1);

        let response = LLMResponse::ToolCall {
            schema_version: crate::types::SCHEMA_VERSION,
            tool_name: "".to_string(),
            input: json!({"query": "test"}),
        };
        let result = client.validate_response(&response);
        assert!(result.is_err());
        match result {
            Err(RuntimeError::LLMError(msg)) => {
                assert!(msg.contains("missing tool_name"));
            }
            _ => panic!("Expected LLMError"),
        }
    }

    #[test]
    fn test_client_creation() {
        let client = LLMClient::new("http://localhost:8000".to_string(), 2);
        assert_eq!(client.base_url, "http://localhost:8000");
        assert_eq!(client.max_retries, 2);
    }

    #[test]
    fn test_request_serialization() {
        let messages = vec![Message::new(Role::User, json!("Hello"))];
        let request = LLMRequest {
            system_prompt: "You are helpful".to_string(),
            messages,
            tools: vec![json!({
                "type": "function",
                "function": {
                    "name": "search",
                    "description": "Search for information",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {"type": "string"}
                        },
                        "required": ["query"]
                    }
                }
            })],
        };

        let serialized = serde_json::to_string(&request).unwrap();
        assert!(serialized.contains("system_prompt"));
        assert!(serialized.contains("messages"));
        assert!(serialized.contains("tools"));
    }

    #[test]
    fn test_parse_inline_tool_call_tag_name_only() {
        let parsed = parse_inline_tool_call("<tool_call>desktop.capture_screen</tool_call>")
            .expect("inline tool call should parse");
        assert_eq!(parsed.0, "desktop.capture_screen");
        assert_eq!(parsed.1, json!({}));
    }

    #[test]
    fn test_parse_inline_tool_call_json_payload() {
        let parsed = parse_inline_tool_call(
            r#"{"tool_name":"fs.list","input":{"path":"/home/rick/Downloads"}}"#,
        )
        .expect("json tool call should parse");
        assert_eq!(parsed.0, "fs.list");
        assert_eq!(parsed.1, json!({"path":"/home/rick/Downloads"}));
    }

    #[test]
    fn test_parse_inline_tool_call_non_tool_text() {
        let parsed = parse_inline_tool_call("Hello, how can I help?");
        assert!(parsed.is_none());
    }

    #[test]
    fn test_extract_retry_seconds_supports_fractional_values() {
        let msg = "Please retry in 44.155866472s.";
        assert_eq!(extract_retry_seconds(msg), Some(45));
    }

    #[test]
    fn test_should_retry_error_skips_daily_quota_exhaustion() {
        let client = LLMClient::new("http://localhost:8000".to_string(), 2);
        let err = RuntimeError::LLMError(
            "Rate limit exceeded. quotaId: GenerateRequestsPerDayPerProjectPerModel-FreeTier"
                .to_string(),
        );
        assert!(!client.should_retry_error(&err));
    }
}
