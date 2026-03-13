//! Tool call normalization module.

use crate::types::LLMResponse;
use tracing::warn;

/// Normalize LLM response to ensure consistent tool call structure.
pub fn normalize_response(response: LLMResponse) -> LLMResponse {
    match response {
        LLMResponse::ToolCall {
            schema_version,
            tool_name,
            mut input,
        } => {
            // Ensure input is an object
            if !input.is_object() {
                warn!("Tool call input is not an object, converting to empty object");
                input = serde_json::json!({});
            }

            // Trim and validate tool name
            let tool_name = tool_name.trim().to_string();
            if tool_name.is_empty() {
                warn!("Tool call has empty tool name after normalization");
            }

            LLMResponse::ToolCall {
                schema_version,
                tool_name,
                input,
            }
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_normalize_valid_tool_call() {
        let response = LLMResponse::ToolCall {
            schema_version: 1,
            tool_name: "  desktop.mouse_click  ".to_string(),
            input: json!({"button": "left"}),
        };

        let normalized = normalize_response(response);

        match normalized {
            LLMResponse::ToolCall { tool_name, .. } => {
                assert_eq!(tool_name, "desktop.mouse_click");
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_normalize_non_object_input() {
        let response = LLMResponse::ToolCall {
            schema_version: 1,
            tool_name: "test".to_string(),
            input: json!("string"),
        };

        let normalized = normalize_response(response);

        match normalized {
            LLMResponse::ToolCall { input, .. } => {
                assert!(input.is_object());
            }
            _ => panic!("Expected ToolCall"),
        }
    }
}
