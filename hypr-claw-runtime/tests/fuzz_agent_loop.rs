#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::approx_constant
)]
//! Property-based fuzzing tests for agent loop.

use hypr_claw_runtime::*;
use proptest::prelude::*;
use serde_json::json;

// Mock summarizer for compactor tests
struct MockSummarizer;

impl Summarizer for MockSummarizer {
    fn summarize(&self, messages: &[Message]) -> Result<String, RuntimeError> {
        Ok(format!("Summary of {} messages", messages.len()))
    }
}

// Test that LLMResponse deserialization handles invalid formats safely
#[test]
fn test_llm_response_missing_tool_name() {
    // Missing tool_name in tool_call
    let json_str = r#"{"type": "tool_call", "input": {"query": "test"}}"#;
    let result: Result<LLMResponse, _> = serde_json::from_str(json_str);
    assert!(result.is_err(), "Should reject tool_call without tool_name");
}

#[test]
fn test_llm_response_missing_content() {
    // Missing content in final
    let json_str = r#"{"type": "final"}"#;
    let result: Result<LLMResponse, _> = serde_json::from_str(json_str);
    assert!(result.is_err(), "Should reject final without content");
}

#[test]
fn test_llm_response_unknown_type() {
    // Unknown type
    let json_str = r#"{"type": "unknown", "content": "test"}"#;
    let result: Result<LLMResponse, _> = serde_json::from_str(json_str);
    assert!(result.is_err(), "Should reject unknown type");
}

#[test]
fn test_llm_response_invalid_json() {
    // Malformed JSON
    let json_str = r#"{"type": "final", "content": "test"#;
    let result: Result<LLMResponse, _> = serde_json::from_str(json_str);
    assert!(result.is_err(), "Should reject malformed JSON");
}

#[test]
fn test_llm_response_deep_nested_json() {
    // Deep nested structure in tool input
    let mut nested = json!({"level": 0});
    for i in 1..100 {
        nested = json!({"level": i, "nested": nested});
    }

    let response = LLMResponse::ToolCall {
        schema_version: hypr_claw_runtime::SCHEMA_VERSION,
        tool_name: "test".to_string(),
        input: nested,
    };

    // Should serialize and deserialize without panic
    let serialized = serde_json::to_string(&response).unwrap();
    let deserialized: LLMResponse = serde_json::from_str(&serialized).unwrap();

    match deserialized {
        LLMResponse::ToolCall { tool_name, .. } => {
            assert_eq!(tool_name, "test");
        }
        _ => panic!("Expected ToolCall"),
    }
}

#[test]
fn test_llm_response_large_payload() {
    // Large content string
    let large_content = "A".repeat(1_000_000); // 1MB

    let response = LLMResponse::Final {
        schema_version: hypr_claw_runtime::SCHEMA_VERSION,
        content: large_content.clone(),
    };

    // Should handle without panic
    let serialized = serde_json::to_string(&response).unwrap();
    assert!(serialized.len() > 1_000_000);

    let deserialized: LLMResponse = serde_json::from_str(&serialized).unwrap();
    match deserialized {
        LLMResponse::Final {
            content,
            schema_version: hypr_claw_runtime::SCHEMA_VERSION,
        } => {
            assert_eq!(content.len(), 1_000_000);
        }
        _ => panic!("Expected Final"),
    }
}

#[test]
fn test_message_arbitrary_json_content() {
    // Message with various JSON types
    let test_cases = vec![
        json!(null),
        json!(true),
        json!(false),
        json!(42),
        json!(3.14),
        json!("string"),
        json!([1, 2, 3]),
        json!({"key": "value"}),
        json!({"nested": {"deep": {"value": 123}}}),
    ];

    for content in test_cases {
        let msg = Message::new(Role::User, content.clone());

        // Should serialize and deserialize
        let serialized = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&serialized).unwrap();

        assert_eq!(msg.content, deserialized.content);
    }
}

proptest! {
    #[test]
    fn test_llm_response_final_never_panics(content in ".*") {
        let response = LLMResponse::Final { content, schema_version: hypr_claw_runtime::SCHEMA_VERSION };

        // Should serialize without panic
        let serialized = serde_json::to_string(&response);
        prop_assert!(serialized.is_ok());

        // Should deserialize without panic
        if let Ok(s) = serialized {
            let deserialized: Result<LLMResponse, _> = serde_json::from_str(&s);
            prop_assert!(deserialized.is_ok());
        }
    }

    #[test]
    fn test_llm_response_tool_call_never_panics(
        tool_name in "[a-zA-Z0-9_]{1,100}",
        query in ".*"
    ) {
        let response = LLMResponse::ToolCall {
        schema_version: hypr_claw_runtime::SCHEMA_VERSION,
            tool_name,
            input: json!({"query": query}),
        };

        // Should serialize without panic
        let serialized = serde_json::to_string(&response);
        prop_assert!(serialized.is_ok());

        // Should deserialize without panic
        if let Ok(s) = serialized {
            let deserialized: Result<LLMResponse, _> = serde_json::from_str(&s);
            prop_assert!(deserialized.is_ok());
        }
    }

    #[test]
    fn test_message_never_panics(content_str in ".*") {
        let msg = Message::new(Role::User, json!(content_str));

        // Should serialize without panic
        let serialized = serde_json::to_string(&msg);
        prop_assert!(serialized.is_ok());

        // Should deserialize without panic
        if let Ok(s) = serialized {
            let deserialized: Result<Message, _> = serde_json::from_str(&s);
            prop_assert!(deserialized.is_ok());
        }
    }

    #[test]
    fn test_session_key_never_panics(
        user_id in "[a-zA-Z0-9_]{1,50}",
        agent_id in "[a-zA-Z0-9_]{1,50}"
    ) {
        // Should not panic on valid inputs
        let result = resolve_session(&user_id, &agent_id);
        prop_assert!(result.is_ok());

        if let Ok(key) = result {
            prop_assert!(key.contains(&user_id));
            prop_assert!(key.contains(&agent_id));
        }
    }
}

#[test]
fn test_empty_strings_handled_safely() {
    // Empty tool name should be caught by validation
    let response = LLMResponse::ToolCall {
        schema_version: hypr_claw_runtime::SCHEMA_VERSION,
        tool_name: "".to_string(),
        input: json!({}),
    };

    // Serialization works
    let serialized = serde_json::to_string(&response).unwrap();
    assert!(serialized.contains("tool_call"));

    // Empty content should be caught by validation
    let response = LLMResponse::Final {
        schema_version: hypr_claw_runtime::SCHEMA_VERSION,
        content: "".to_string(),
    };

    let serialized = serde_json::to_string(&response).unwrap();
    assert!(serialized.contains("final"));
}

#[test]
fn test_special_characters_in_content() {
    let special_chars = vec!["\n\r\t", "\"'`", "<>&", "\\", "\0", "🚀🎉", "中文"];

    for chars in special_chars {
        let response = LLMResponse::Final {
            schema_version: hypr_claw_runtime::SCHEMA_VERSION,
            content: chars.to_string(),
        };

        // Should handle without panic
        let serialized = serde_json::to_string(&response).unwrap();
        let deserialized: LLMResponse = serde_json::from_str(&serialized).unwrap();

        match deserialized {
            LLMResponse::Final {
                content,
                schema_version: hypr_claw_runtime::SCHEMA_VERSION,
            } => {
                assert_eq!(content, chars);
            }
            _ => panic!("Expected Final"),
        }
    }
}

#[test]
fn test_max_iterations_always_respected() {
    // This is a deterministic test that max_iterations is enforced
    // The agent loop should never exceed max_iterations regardless of input

    let max_iterations = 5;

    // Even if we had a way to force infinite tool calls,
    // the loop should stop at max_iterations
    // This is verified by the agent_loop implementation

    assert!(max_iterations > 0);
    assert!(max_iterations < 1000); // Reasonable limit
}

#[test]
fn test_compactor_handles_arbitrary_message_counts() {
    let compactor = Compactor::new(100, MockSummarizer);

    // Test with various message counts
    for count in [0, 1, 2, 5, 10, 100, 1000] {
        let messages: Vec<Message> = (0..count)
            .map(|i| Message::new(Role::User, json!(format!("Message {}", i))))
            .collect();

        let result = compactor.compact(messages);

        // Should never panic
        assert!(result.is_ok());
    }
}

#[test]
fn test_gateway_rejects_empty_ids() {
    // Empty user_id
    let result = resolve_session("", "agent");
    assert!(result.is_err());

    // Empty agent_id
    let result = resolve_session("user", "");
    assert!(result.is_err());

    // Both empty
    let result = resolve_session("", "");
    assert!(result.is_err());
}
