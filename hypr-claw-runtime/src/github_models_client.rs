//! GitHub Models API client adapter.

use crate::interfaces::RuntimeError;
use crate::tool_call_normalizer::normalize_response;
use crate::types::{LLMResponse, Message};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const GITHUB_MODELS_ENDPOINT: &str = "https://models.github.ai/inference";

#[derive(Serialize)]
struct GitHubRequest {
    model: String,
    messages: Vec<GitHubMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
}

#[derive(Serialize, Deserialize)]
struct GitHubMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct GitHubResponse {
    choices: Vec<GitHubChoice>,
}

#[derive(Deserialize)]
struct GitHubChoice {
    message: GitHubResponseMessage,
}

#[derive(Deserialize)]
struct GitHubResponseMessage {
    content: Option<String>,
    tool_calls: Option<Vec<GitHubToolCall>>,
}

#[derive(Deserialize)]
struct GitHubToolCall {
    function: GitHubFunction,
}

#[derive(Deserialize)]
struct GitHubFunction {
    name: String,
    arguments: String,
}

pub struct GitHubModelsClient {
    client: reqwest::Client,
    token: String,
    model: String,
}

impl GitHubModelsClient {
    pub fn new(token: String, model: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            client,
            token,
            model,
        }
    }

    pub async fn call(
        &self,
        system_prompt: &str,
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
    ) -> Result<LLMResponse, RuntimeError> {
        let mut gh_messages = vec![GitHubMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        }];

        for msg in messages {
            let role = match msg.role {
                crate::types::Role::User => "user",
                crate::types::Role::Assistant => "assistant",
                crate::types::Role::Tool => "tool",
                crate::types::Role::System => "system",
            };

            let content = if let Some(s) = msg.content.as_str() {
                s.to_string()
            } else {
                msg.content.to_string()
            };

            gh_messages.push(GitHubMessage {
                role: role.to_string(),
                content,
            });
        }

        let request = GitHubRequest {
            model: self.model.clone(),
            messages: gh_messages,
            tools: if tool_schemas.is_empty() {
                None
            } else {
                Some(tool_schemas.to_vec())
            },
        };

        let response = self
            .client
            .post(format!("{}/chat/completions", GITHUB_MODELS_ENDPOINT))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| RuntimeError::LLMError(format!("GitHub API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(RuntimeError::LLMError(format!(
                "GitHub API error {}: {}",
                status, error_text
            )));
        }

        let gh_response: GitHubResponse = response
            .json()
            .await
            .map_err(|e| RuntimeError::LLMError(format!("Failed to parse response: {}", e)))?;

        let choice = gh_response
            .choices
            .first()
            .ok_or_else(|| RuntimeError::LLMError("No choices in response".to_string()))?;

        let llm_response = if let Some(tool_calls) = &choice.message.tool_calls {
            if let Some(tool_call) = tool_calls.first() {
                LLMResponse::ToolCall {
                    schema_version: crate::types::SCHEMA_VERSION,
                    tool_name: tool_call.function.name.clone(),
                    input: serde_json::from_str(&tool_call.function.arguments)
                        .unwrap_or(serde_json::json!({})),
                }
            } else {
                LLMResponse::Final {
                    schema_version: crate::types::SCHEMA_VERSION,
                    content: choice.message.content.clone().unwrap_or_default(),
                }
            }
        } else {
            LLMResponse::Final {
                schema_version: crate::types::SCHEMA_VERSION,
                content: choice.message.content.clone().unwrap_or_default(),
            }
        };

        Ok(normalize_response(llm_response))
    }
}
