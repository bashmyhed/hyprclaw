//! Fuzz testing for LLM response parsing.

#![allow(clippy::unwrap_used)]

use hypr_claw_runtime::LLMResponse;
use proptest::prelude::*;
use serde_json::json;

proptest! {
    #[test]
    fn test_llm_response_missing_fields(
        content in prop::option::of(any::<String>()),
        tool_calls_present in any::<bool>(),
    ) {
        let mut response_json = json!({});

        if let Some(c) = content {
            response_json["content"] = json!(c);
        }

        if tool_calls_present {
            response_json["tool_calls"] = json!([]);
        }

        // Should not panic
        let result: Result<LLMResponse, _> = serde_json::from_value(response_json);
        // Either succeeds or fails gracefully
        let _ = result;
    }

    #[test]
    fn test_llm_response_malformed_tool_calls(
        tool_name in prop::option::of(any::<String>()),
        arguments in prop::option::of(any::<String>()),
    ) {
        let mut tool_call = json!({});

        if let Some(name) = tool_name {
            tool_call["tool_name"] = json!(name);
        }

        if let Some(args) = arguments {
            tool_call["arguments"] = json!(args);
        }

        let response_json = json!({
            "content": null,
            "tool_calls": [tool_call]
        });

        // Should not panic
        let result: Result<LLMResponse, _> = serde_json::from_value(response_json);
        let _ = result;
    }

    #[test]
    fn test_llm_response_invalid_json_structure(
        random_string in any::<String>(),
    ) {
        // Should not panic on invalid JSON
        let result: Result<LLMResponse, _> = serde_json::from_str(&random_string);
        let _ = result;
    }
}

#[test]
fn test_empty_content_and_tool_calls() {
    let response_json = json!({
        "content": null,
        "tool_calls": []
    });

    let result: Result<LLMResponse, _> = serde_json::from_value(response_json);
    // Should handle gracefully
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_tool_call_without_tool_name() {
    let response_json = json!({
        "content": null,
        "tool_calls": [{
            "arguments": "{}"
        }]
    });

    let result: Result<LLMResponse, _> = serde_json::from_value(response_json);
    // Should fail gracefully, not panic
    assert!(result.is_err());
}

#[test]
fn test_tool_call_with_invalid_arguments() {
    let response_json = json!({
        "content": null,
        "tool_calls": [{
            "tool_name": "test",
            "arguments": "not valid json"
        }]
    });

    let result: Result<LLMResponse, _> = serde_json::from_value(response_json);
    // Should handle gracefully
    let _ = result;
}

#[test]
fn test_deeply_nested_structure() {
    let mut nested = json!({"level": 0});
    for i in 1..100 {
        nested = json!({"level": i, "nested": nested});
    }

    let response_json = json!({
        "content": nested,
        "tool_calls": []
    });

    // Should not stack overflow
    let result: Result<LLMResponse, _> = serde_json::from_value(response_json);
    let _ = result;
}
