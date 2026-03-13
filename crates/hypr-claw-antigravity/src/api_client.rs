use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

use crate::accounts::AccountManager;
use crate::fingerprint::build_fingerprint_headers;
use crate::models::{HeaderStyle, ModelResolver};
use crate::request_transform::{add_thinking_config, transform_tools};

// API endpoints from constants.ts
const ANTIGRAVITY_ENDPOINT: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com";
const GEMINI_CLI_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: Function,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: Message,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

pub struct AntigravityClient {
    client: Client,
    account_manager: AccountManager,
}

impl AntigravityClient {
    pub async fn new(storage_path: PathBuf) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;

        let account_manager = AccountManager::new(storage_path).await?;

        Ok(Self {
            client,
            account_manager,
        })
    }

    pub async fn add_account(
        &mut self,
        email: Option<String>,
        refresh: String,
        project_id: String,
    ) -> Result<()> {
        self.account_manager
            .add_account(email, refresh, project_id)
            .await
    }

    pub fn account_count(&self) -> usize {
        self.account_manager.get_account_count()
    }

    pub async fn chat(&mut self, request: ChatRequest) -> Result<ChatResponse> {
        let resolved = ModelResolver::resolve(&request.model);

        // Determine which API to use
        let (endpoint, _header_style) = match resolved.quota_preference {
            HeaderStyle::Antigravity => (ANTIGRAVITY_ENDPOINT, HeaderStyle::Antigravity),
            HeaderStyle::GeminiCli => (GEMINI_CLI_ENDPOINT, HeaderStyle::GeminiCli),
        };

        // Get access token
        let access_token = self.account_manager.refresh_current_token().await?;

        // Get account for fingerprint
        let account = self.account_manager.get_current_account().await?;
        let fingerprint = account.fingerprint.as_ref().context("No fingerprint")?;

        // Build request body
        let mut body = serde_json::to_value(&request)?;

        // Update model name
        if let Some(obj) = body.as_object_mut() {
            obj.insert(
                "model".to_string(),
                Value::String(resolved.actual_model.clone()),
            );
        }

        // Add thinking configuration
        add_thinking_config(
            &mut body,
            resolved.thinking_budget,
            resolved.thinking_level.as_deref(),
        );

        // Transform tools
        transform_tools(&mut body);

        // Build headers
        let mut headers = build_fingerprint_headers(fingerprint);
        headers.push((
            "Authorization".to_string(),
            format!("Bearer {}", access_token),
        ));
        headers.push(("Content-Type".to_string(), "application/json".to_string()));

        // Build URL
        let url = format!("{}/v1/chat:generateContent", endpoint);

        // Make request
        let mut req = self.client.post(&url);
        for (key, value) in headers {
            req = req.header(key, value);
        }

        let response = req.json(&body).send().await?;

        // Handle rate limiting
        if response.status().as_u16() == 429 {
            let quota_key = ModelResolver::get_model_family(&resolved.actual_model);
            self.account_manager
                .mark_rate_limited(quota_key, 60_000)
                .await?;
            anyhow::bail!("Rate limited, rotated to next account");
        }

        let response = response.error_for_status()?;
        let chat_response: ChatResponse = response.json().await?;

        Ok(chat_response)
    }
}
