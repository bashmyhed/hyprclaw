//! Codex provider adapter - bridges runtime and providers crate type systems.

use crate::interfaces::RuntimeError;
use crate::types::{LLMResponse, Message as RuntimeMessage, Role, SCHEMA_VERSION};
use hypr_claw_memory::types::OAuthTokens;
use hypr_claw_providers::codex::{types::OAuthTokens as CodexTokens, CodexProvider};
use hypr_claw_providers::traits::{LLMProvider, Message as ProviderMessage};
use std::sync::Arc;

/// Adapter that wraps CodexProvider and provides runtime-compatible interface.
pub struct CodexAdapter {
    provider: Arc<CodexProvider>,
}

impl CodexAdapter {
    /// Create adapter with optional existing tokens.
    pub async fn new(model: String, tokens: Option<OAuthTokens>) -> Result<Self, RuntimeError> {
        let provider = CodexProvider::new(model);

        if let Some(tokens) = tokens {
            let codex_tokens = CodexTokens {
                access_token: tokens.access_token,
                refresh_token: tokens.refresh_token,
                expires_at: tokens.expires_at,
                account_id: tokens.account_id,
            };
            provider.restore_tokens(codex_tokens).await;
        }

        Ok(Self {
            provider: Arc::new(provider),
        })
    }

    /// Run OAuth flow and return tokens for persistence.
    pub async fn authenticate(model: String) -> Result<OAuthTokens, RuntimeError> {
        let provider = CodexProvider::new(model);
        let tokens = provider
            .authenticate()
            .await
            .map_err(|e| RuntimeError::LLMError(format!("OAuth failed: {}", e)))?;

        Ok(OAuthTokens {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: tokens.expires_at,
            account_id: tokens.account_id,
        })
    }

    /// Get current tokens for persistence.
    pub async fn get_tokens(&self) -> Option<OAuthTokens> {
        self.provider.get_tokens().await.map(|t| OAuthTokens {
            access_token: t.access_token,
            refresh_token: t.refresh_token,
            expires_at: t.expires_at,
            account_id: t.account_id,
        })
    }

    /// Call LLM with runtime types (same signature as LLMClient::call).
    pub async fn call(
        &self,
        system_prompt: &str,
        messages: &[RuntimeMessage],
        _tool_schemas: &[serde_json::Value],
    ) -> Result<LLMResponse, RuntimeError> {
        let provider_messages = self.convert_messages(system_prompt, messages)?;

        // NOTE: Codex API doesn't support OpenAI-style function calling
        // Tools must be handled via prompt engineering (bridge prompt approach)
        // For now, Codex only returns text responses

        let response = self
            .provider
            .generate(&provider_messages, None)
            .await
            .map_err(|e| RuntimeError::LLMError(e.to_string()))?;

        self.convert_response(response)
    }

    /// Convert runtime messages to provider messages.
    fn convert_messages(
        &self,
        system_prompt: &str,
        messages: &[RuntimeMessage],
    ) -> Result<Vec<ProviderMessage>, RuntimeError> {
        let mut provider_messages = Vec::new();

        // Codex doesn't support system messages - prepend to first user message instead
        let mut system_prefix = if !system_prompt.is_empty() {
            format!("[System Instructions: {}]\n\n", system_prompt)
        } else {
            String::new()
        };

        // Convert runtime messages
        for msg in messages {
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
                Role::System => {
                    // Skip system messages, already handled
                    continue;
                }
            }
            .to_string();

            // Convert JSON content to string
            let mut content = match &msg.content {
                serde_json::Value::String(s) => s.clone(),
                other => serde_json::to_string(other)?,
            };

            // Prepend system instructions to first user message
            if role == "user" && !system_prefix.is_empty() {
                content = format!("{}{}", system_prefix, content);
                system_prefix.clear();
            }

            provider_messages.push(ProviderMessage { role, content });
        }

        Ok(provider_messages)
    }

    /// Convert provider response to runtime response.
    fn convert_response(
        &self,
        response: hypr_claw_providers::traits::GenerateResponse,
    ) -> Result<LLMResponse, RuntimeError> {
        // Codex doesn't support tool calling - always return Final response
        Ok(LLMResponse::Final {
            schema_version: SCHEMA_VERSION,
            content: response.content.unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_message_conversion() {
        let adapter = CodexAdapter {
            provider: Arc::new(CodexProvider::new("test".to_string())),
        };

        let runtime_messages = vec![
            RuntimeMessage {
                schema_version: SCHEMA_VERSION,
                role: Role::User,
                content: json!("Hello"),
                metadata: None,
            },
            RuntimeMessage {
                schema_version: SCHEMA_VERSION,
                role: Role::Assistant,
                content: json!({"text": "Hi there"}),
                metadata: None,
            },
        ];

        let result = adapter.convert_messages("You are helpful", &runtime_messages);
        assert!(result.is_ok());

        let provider_messages = result.expect("Should have messages");
        assert_eq!(provider_messages.len(), 2); // No system message, prepended to user
        assert_eq!(provider_messages[0].role, "user");
        assert!(provider_messages[0].content.contains("You are helpful"));
        assert!(provider_messages[0].content.contains("Hello"));
        assert_eq!(provider_messages[1].role, "assistant");
    }

    #[test]
    fn test_response_conversion_final() {
        let adapter = CodexAdapter {
            provider: Arc::new(CodexProvider::new("test".to_string())),
        };

        let provider_response = hypr_claw_providers::traits::GenerateResponse {
            content: Some("Test response".to_string()),
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
        };

        let result = adapter.convert_response(provider_response);
        assert!(result.is_ok());

        if let Ok(LLMResponse::Final { content, .. }) = result {
            assert_eq!(content, "Test response");
        } else {
            panic!("Expected Final response");
        }
    }

    #[test]
    fn test_response_conversion_tool_call() {
        let adapter = CodexAdapter {
            provider: Arc::new(CodexProvider::new("test".to_string())),
        };

        // Codex doesn't support tool calling - even with tool_calls in response,
        // it should return Final response
        let provider_response = hypr_claw_providers::traits::GenerateResponse {
            content: None,
            tool_calls: vec![hypr_claw_providers::traits::ToolCall {
                name: "echo".to_string(),
                arguments: json!({"message": "test"}),
            }],
            finish_reason: "tool_calls".to_string(),
        };

        let result = adapter.convert_response(provider_response);
        assert!(result.is_ok());

        // Codex adapter always returns Final, never ToolCall
        if let Ok(LLMResponse::Final { content, .. }) = result {
            assert_eq!(content, ""); // No content provided
        } else {
            panic!("Expected Final response from Codex adapter");
        }
    }
}
