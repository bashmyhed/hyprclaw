use crate::traits::*;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

pub struct OpenAICompatibleProvider {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
}

impl OpenAICompatibleProvider {
    pub fn new(base_url: String, api_key: Option<String>, model: String) -> Self {
        Self {
            client: Client::new(),
            base_url,
            api_key,
            model,
        }
    }
}

#[async_trait]
impl LLMProvider for OpenAICompatibleProvider {
    async fn generate(
        &self,
        messages: &[Message],
        tools: Option<&[serde_json::Value]>,
    ) -> Result<GenerateResponse, ProviderError> {
        let url = format!("{}/chat/completions", self.base_url);

        let mut body = json!({
            "model": self.model,
            "messages": messages,
        });

        if let Some(tools) = tools {
            body["tools"] = json!(tools);
        }

        let mut request = self.client.post(&url).json(&body);

        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::Api(format!("{}: {}", status, text)));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        // Parse OpenAI-compatible response
        let choice = json["choices"]
            .get(0)
            .ok_or_else(|| ProviderError::Parse("No choices in response".to_string()))?;

        let message = &choice["message"];
        let content = message["content"].as_str().map(|s| s.to_string());
        let finish_reason = choice["finish_reason"]
            .as_str()
            .unwrap_or("stop")
            .to_string();

        let tool_calls = if let Some(calls) = message["tool_calls"].as_array() {
            calls
                .iter()
                .filter_map(|call| {
                    let name = call["function"]["name"].as_str()?.to_string();
                    let arguments: serde_json::Value =
                        serde_json::from_str(call["function"]["arguments"].as_str()?).ok()?;
                    Some(ToolCall { name, arguments })
                })
                .collect()
        } else {
            Vec::new()
        };

        Ok(GenerateResponse {
            content,
            tool_calls,
            finish_reason,
        })
    }

    fn name(&self) -> &str {
        "OpenAI Compatible"
    }
}
