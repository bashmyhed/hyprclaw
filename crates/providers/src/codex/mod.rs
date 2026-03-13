mod constants;
mod oauth;
mod server;
mod transform;
pub mod types;

use crate::traits::*;
use async_trait::async_trait;
use constants::*;
use oauth::*;
use reqwest::Client;
use server::OAuthServer;
use std::sync::Arc;
use tokio::sync::RwLock;
use transform::*;
use types::*;

pub struct CodexProvider {
    client: Client,
    tokens: Arc<RwLock<Option<OAuthTokens>>>,
    model: String,
}

impl CodexProvider {
    pub fn new(model: String) -> Self {
        Self {
            client: Client::new(),
            tokens: Arc::new(RwLock::new(None)),
            model,
        }
    }

    pub async fn authenticate(&self) -> Result<OAuthTokens, Box<dyn std::error::Error>> {
        println!("\n[Codex] Starting OAuth authentication...");

        let pkce = generate_pkce();
        let state = generate_state();
        let auth_url = build_authorization_url(&pkce, &state);

        println!("[Codex] Opening browser for authentication...");
        println!("If browser doesn't open, visit:\n{}\n", auth_url);

        if let Err(e) = open::that(&auth_url) {
            eprintln!("[Codex] Failed to open browser: {}", e);
        }

        let server = OAuthServer::new(state);
        let code = server.wait_for_callback().await?;

        println!("[OAuth] Exchanging authorization code for tokens...");
        let tokens = exchange_code_for_tokens(&code, &pkce.verifier).await?;

        println!("[Codex] Authentication successful!");
        println!("[Codex] Account ID: {}", tokens.account_id);

        *self.tokens.write().await = Some(tokens.clone());
        Ok(tokens)
    }

    pub async fn restore_tokens(&self, tokens: OAuthTokens) {
        *self.tokens.write().await = Some(tokens);
    }

    pub async fn get_tokens(&self) -> Option<OAuthTokens> {
        self.tokens.read().await.clone()
    }

    async fn ensure_valid_token(&self) -> Result<(), Box<dyn std::error::Error>> {
        let guard = self.tokens.read().await;

        if let Some(tokens) = guard.as_ref() {
            if is_token_expired(tokens) {
                let refresh_token = tokens.refresh_token.clone();
                drop(guard);
                println!("[Codex] Token expired, refreshing...");
                let new_tokens = refresh_access_token(&refresh_token).await?;
                *self.tokens.write().await = Some(new_tokens);
                println!("[Codex] Token refreshed successfully");
            }
        } else {
            return Err("No tokens available. Please authenticate first.".into());
        }

        Ok(())
    }

    async fn make_request(
        &self,
        messages: &[Message],
        tools: Option<&[serde_json::Value]>,
    ) -> Result<GenerateResponse, ProviderError> {
        self.ensure_valid_token()
            .await
            .map_err(|e| ProviderError::Api(e.to_string()))?;

        let tokens = self.tokens.read().await;
        let tokens = tokens
            .as_ref()
            .ok_or_else(|| ProviderError::Api("No tokens".to_string()))?;

        let request_body = build_codex_request(messages, tools, &self.model);

        let response = self
            .client
            .post(CODEX_RESPONSES_URL)
            .header("Authorization", format!("Bearer {}", tokens.access_token))
            .header(HEADER_ACCOUNT_ID, &tokens.account_id)
            .header(HEADER_OPENAI_BETA, VALUE_OPENAI_BETA)
            .header(HEADER_ORIGINATOR, VALUE_ORIGINATOR)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::Api(format!("{}: {}", status, text)));
        }

        // Parse SSE stream - look for response.done event
        let text = response
            .text()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        for line in text.lines() {
            if line.starts_with("data: ") {
                let data = &line[6..];
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                    let event_type = event.get("type").and_then(|t| t.as_str());

                    // Look for response.done or response.completed
                    if event_type == Some("response.done")
                        || event_type == Some("response.completed")
                    {
                        if let Some(response_obj) = event.get("response") {
                            // Extract content from output array
                            if let Some(output) =
                                response_obj.get("output").and_then(|o| o.as_array())
                            {
                                let mut content = String::new();

                                for item in output {
                                    if let Some(item_type) =
                                        item.get("type").and_then(|t| t.as_str())
                                    {
                                        if item_type == "message" {
                                            // Extract text content
                                            if let Some(content_array) =
                                                item.get("content").and_then(|c| c.as_array())
                                            {
                                                for content_item in content_array {
                                                    if let Some(text) = content_item
                                                        .get("text")
                                                        .and_then(|t| t.as_str())
                                                    {
                                                        content.push_str(text);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                if !content.is_empty() {
                                    return Ok(GenerateResponse {
                                        content: Some(content),
                                        tool_calls: Vec::new(),
                                        finish_reason: "stop".to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        Err(ProviderError::Parse(
            "No response.done event found".to_string(),
        ))
    }
}

#[async_trait]
impl LLMProvider for CodexProvider {
    async fn generate(
        &self,
        messages: &[Message],
        tools: Option<&[serde_json::Value]>,
    ) -> Result<GenerateResponse, ProviderError> {
        self.make_request(messages, tools).await
    }

    fn name(&self) -> &str {
        "OpenAI Codex"
    }
}
