//! Schema versioning tests.

#![allow(clippy::unwrap_used)]

use hypr_claw_runtime::{LLMResponse, Message, Role, SCHEMA_VERSION};
use serde_json::json;

#[test]
fn test_message_correct_version() {
    let msg = Message::new(Role::User, json!("test"));
    assert_eq!(msg.schema_version, SCHEMA_VERSION);
    assert!(msg.validate_version().is_ok());
}

#[test]
fn test_message_version_mismatch() {
    let json_str = r#"{
        "schema_version": 999,
        "role": "user",
        "content": "test"
    }"#;

    let msg: Message = serde_json::from_str(json_str).unwrap();
    assert!(msg.validate_version().is_err());
}

#[test]
fn test_message_missing_version_defaults() {
    let json_str = r#"{
        "role": "user",
        "content": "test"
    }"#;

    let msg: Message = serde_json::from_str(json_str).unwrap();
    assert_eq!(msg.schema_version, SCHEMA_VERSION);
    assert!(msg.validate_version().is_ok());
}

#[test]
fn test_llm_response_final_correct_version() {
    let json_str = format!(
        r#"{{
        "type": "final",
        "schema_version": {},
        "content": "test"
    }}"#,
        SCHEMA_VERSION
    );

    let response: LLMResponse = serde_json::from_str(&json_str).unwrap();
    assert!(response.validate_version().is_ok());
}

#[test]
fn test_llm_response_tool_call_correct_version() {
    let json_str = format!(
        r#"{{
        "type": "tool_call",
        "schema_version": {},
        "tool_name": "test",
        "input": {{}}
    }}"#,
        SCHEMA_VERSION
    );

    let response: LLMResponse = serde_json::from_str(&json_str).unwrap();
    assert!(response.validate_version().is_ok());
}

#[test]
fn test_llm_response_version_mismatch() {
    let json_str = r#"{
        "type": "final",
        "schema_version": 999,
        "content": "test"
    }"#;

    let response: LLMResponse = serde_json::from_str(json_str).unwrap();
    assert!(response.validate_version().is_err());
}

#[test]
fn test_llm_response_missing_version_defaults() {
    let json_str = r#"{
        "type": "final",
        "content": "test"
    }"#;

    let response: LLMResponse = serde_json::from_str(json_str).unwrap();
    assert!(response.validate_version().is_ok());
}

#[test]
fn test_version_error_message() {
    let json_str = r#"{
        "schema_version": 2,
        "role": "user",
        "content": "test"
    }"#;

    let msg: Message = serde_json::from_str(json_str).unwrap();
    let err = msg.validate_version().unwrap_err();
    assert!(err.contains("expected 1"));
    assert!(err.contains("got 2"));
}
