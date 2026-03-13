//! LLM client type wrapper - supports both standard HTTP and Codex providers.

use crate::codex_adapter::CodexAdapter;
use crate::interfaces::RuntimeError;
use crate::llm_client::LLMClient;
use crate::types::{LLMResponse, Message};

/// Enum wrapper for different LLM client types.
pub enum LLMClientType {
    Standard(LLMClient),
    Codex(CodexAdapter),
}

impl LLMClientType {
    /// Call LLM with unified interface.
    pub async fn call(
        &self,
        system_prompt: &str,
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
    ) -> Result<LLMResponse, RuntimeError> {
        match self {
            Self::Standard(client) => client.call(system_prompt, messages, tool_schemas).await,
            Self::Codex(adapter) => adapter.call(system_prompt, messages, tool_schemas).await,
        }
    }

    /// Update active model for providers backed by standard OpenAI-compatible client.
    pub fn set_model(&self, model: &str) -> Result<(), RuntimeError> {
        match self {
            Self::Standard(client) => client.set_model(model),
            Self::Codex(_) => Err(RuntimeError::LLMError(
                "Model switching is not supported for this provider".to_string(),
            )),
        }
    }

    /// Get currently active model, when available.
    pub fn current_model(&self) -> Option<String> {
        match self {
            Self::Standard(client) => client.current_model(),
            Self::Codex(_) => None,
        }
    }

    /// Query provider model catalog when supported.
    pub async fn list_models(&self) -> Result<Vec<String>, RuntimeError> {
        match self {
            Self::Standard(client) => client.list_models().await,
            Self::Codex(_) => Err(RuntimeError::LLMError(
                "Model listing is not supported for this provider".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enum_variants_exist() {
        // Just verify the enum compiles and variants are accessible
        let _standard = LLMClientType::Standard(LLMClient::new("http://test".to_string(), 3));

        // Can't easily test Codex without OAuth, but verify it compiles
        // Test passes if we reach here
    }
}
