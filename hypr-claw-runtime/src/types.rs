//! Core type definitions for Hypr-Claw runtime.

use serde::{Deserialize, Serialize};

/// Schema version for protocol compatibility.
pub const SCHEMA_VERSION: u32 = 1;

/// Message role in conversation.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Tool,
    System,
}

/// A single message in the conversation.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub role: Role,
    pub content: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

/// Structured response from LLM.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LLMResponse {
    Final {
        #[serde(default = "default_schema_version")]
        schema_version: u32,
        content: String,
    },
    ToolCall {
        #[serde(default = "default_schema_version")]
        schema_version: u32,
        tool_name: String,
        input: serde_json::Value,
    },
}

impl Message {
    /// Create a new message.
    pub fn new(role: Role, content: serde_json::Value) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            role,
            content,
            metadata: None,
        }
    }

    /// Create a new message with metadata.
    pub fn with_metadata(
        role: Role,
        content: serde_json::Value,
        metadata: serde_json::Value,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            role,
            content,
            metadata: Some(metadata),
        }
    }

    /// Validate schema version.
    pub fn validate_version(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "Schema version mismatch: expected {}, got {}",
                SCHEMA_VERSION, self.schema_version
            ));
        }
        Ok(())
    }
}

impl LLMResponse {
    /// Validate schema version.
    pub fn validate_version(&self) -> Result<(), String> {
        let version = match self {
            LLMResponse::Final { schema_version, .. } => *schema_version,
            LLMResponse::ToolCall { schema_version, .. } => *schema_version,
        };

        if version != SCHEMA_VERSION {
            return Err(format!(
                "Schema version mismatch: expected {}, got {}",
                SCHEMA_VERSION, version
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_message_creation() {
        let msg = Message::new(Role::User, json!("Hello"));
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, json!("Hello"));
        assert!(msg.metadata.is_none());
    }

    #[test]
    fn test_message_with_metadata() {
        let metadata = json!({"timestamp": "2026-02-22"});
        let msg = Message::with_metadata(Role::Assistant, json!("Hi"), metadata.clone());
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.metadata, Some(metadata));
    }

    #[test]
    fn test_message_serialization() {
        let msg = Message::new(Role::User, json!("Test"));
        let serialized = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&serialized).unwrap();
        assert_eq!(msg.role, deserialized.role);
        assert_eq!(msg.content, deserialized.content);
    }

    #[test]
    fn test_role_serialization() {
        let role = Role::User;
        let serialized = serde_json::to_string(&role).unwrap();
        assert_eq!(serialized, r#""user""#);

        let role = Role::Assistant;
        let serialized = serde_json::to_string(&role).unwrap();
        assert_eq!(serialized, r#""assistant""#);
    }

    #[test]
    fn test_llm_response_final() {
        let response = LLMResponse::Final {
            schema_version: SCHEMA_VERSION,
            content: "Done".to_string(),
        };
        let serialized = serde_json::to_string(&response).unwrap();
        let deserialized: LLMResponse = serde_json::from_str(&serialized).unwrap();

        match deserialized {
            LLMResponse::Final { content, .. } => assert_eq!(content, "Done"),
            _ => panic!("Expected Final response"),
        }
    }

    #[test]
    fn test_llm_response_tool_call() {
        let response = LLMResponse::ToolCall {
            schema_version: SCHEMA_VERSION,
            tool_name: "search".to_string(),
            input: json!({"query": "test"}),
        };
        let serialized = serde_json::to_string(&response).unwrap();
        let deserialized: LLMResponse = serde_json::from_str(&serialized).unwrap();

        match deserialized {
            LLMResponse::ToolCall {
                tool_name, input, ..
            } => {
                assert_eq!(tool_name, "search");
                assert_eq!(input, json!({"query": "test"}));
            }
            _ => panic!("Expected ToolCall response"),
        }
    }

    #[test]
    fn test_llm_response_json_format() {
        let response = LLMResponse::Final {
            schema_version: SCHEMA_VERSION,
            content: "Hello".to_string(),
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["type"], "final");
        assert_eq!(json["content"], "Hello");
    }

    #[test]
    fn test_invalid_role_deserialization() {
        let invalid_json = r#"{"role": "invalid", "content": "test"}"#;
        let result: Result<Message, _> = serde_json::from_str(invalid_json);
        assert!(result.is_err());
    }
}
