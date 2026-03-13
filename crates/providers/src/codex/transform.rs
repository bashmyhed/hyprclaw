use super::types::*;
use crate::traits::Message;

pub fn normalize_model(model: &str) -> String {
    // Strip reasoning effort suffixes
    let normalized = model
        .replace("-xhigh", "")
        .replace("-high", "")
        .replace("-medium", "")
        .replace("-low", "")
        .replace("-max-high", "-max")
        .replace("-max-medium", "-max")
        .replace("-max-low", "-max");

    // Handle legacy GPT-5.0 -> GPT-5.1 mapping
    if normalized == "gpt-5-codex" {
        "gpt-5.1-codex".to_string()
    } else if normalized == "gpt-5-codex-mini" {
        "gpt-5.1-codex-mini".to_string()
    } else if normalized == "gpt-5" {
        "gpt-5.1".to_string()
    } else {
        normalized
    }
}

pub fn extract_reasoning_effort(model: &str) -> String {
    if model.contains("-xhigh") {
        "xhigh"
    } else if model.contains("-high") {
        "high"
    } else if model.contains("-low") {
        "low"
    } else {
        "medium"
    }
    .to_string()
}

pub fn build_codex_request(
    messages: &[Message],
    tools: Option<&[serde_json::Value]>,
    model: &str,
) -> CodexRequest {
    let normalized_model = normalize_model(model);
    let reasoning_effort = extract_reasoning_effort(model);

    let input: Vec<CodexMessage> = messages
        .iter()
        .map(|m| CodexMessage {
            msg_type: "message".to_string(),
            role: m.role.clone(),
            content: m.content.clone(),
        })
        .collect();

    CodexRequest {
        model: normalized_model,
        store: false,
        include: vec!["reasoning.encrypted_content".to_string()],
        input,
        reasoning: ReasoningConfig {
            effort: reasoning_effort,
            summary: "auto".to_string(),
        },
        text: TextConfig {
            verbosity: "medium".to_string(),
        },
        stream: true,
        tools: tools.map(|t| t.to_vec()),
        instructions: Some("You are a helpful assistant.".to_string()),
    }
}

#[allow(dead_code)]
pub fn parse_codex_response(
    response: CodexResponse,
) -> Result<(Option<String>, Vec<crate::traits::ToolCall>), String> {
    let output = response.output.ok_or("No output in response")?;

    let tool_calls = if let Some(calls) = output.tool_calls {
        calls
            .into_iter()
            .filter_map(|call| {
                let arguments: serde_json::Value =
                    serde_json::from_str(&call.function.arguments).ok()?;
                Some(crate::traits::ToolCall {
                    name: call.function.name,
                    arguments,
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok((output.content, tool_calls))
}
