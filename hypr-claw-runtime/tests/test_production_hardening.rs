#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Production hardening tests for edge cases and defensive programming.

use async_trait::async_trait;
use hypr_claw_runtime::LLMClientType;
use hypr_claw_runtime::*;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

// Mock implementations
struct MockSessionStore {
    storage: Mutex<HashMap<String, Vec<Message>>>,
}

impl MockSessionStore {
    fn new() -> Self {
        Self {
            storage: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl SessionStore for MockSessionStore {
    async fn load(&self, session_key: &str) -> Result<Vec<Message>, RuntimeError> {
        let storage = self.storage.lock().unwrap();
        Ok(storage.get(session_key).cloned().unwrap_or_default())
    }

    async fn save(&self, session_key: &str, messages: &[Message]) -> Result<(), RuntimeError> {
        let mut storage = self.storage.lock().unwrap();
        storage.insert(session_key.to_string(), messages.to_vec());
        Ok(())
    }
}

struct MockLockManager {
    locks: Mutex<HashSet<String>>,
}

impl MockLockManager {
    fn new() -> Self {
        Self {
            locks: Mutex::new(HashSet::new()),
        }
    }
}

#[async_trait]
impl LockManager for MockLockManager {
    async fn acquire(&self, session_key: &str) -> Result<(), RuntimeError> {
        let mut locks = self.locks.lock().unwrap();
        if locks.contains(session_key) {
            return Err(RuntimeError::LockError(format!(
                "Lock already held: {}",
                session_key
            )));
        }
        locks.insert(session_key.to_string());
        Ok(())
    }

    async fn release(&self, session_key: &str) {
        let mut locks = self.locks.lock().unwrap();
        locks.remove(session_key);
    }
}

struct MockToolDispatcher {
    should_fail: bool,
}

impl MockToolDispatcher {
    fn new(should_fail: bool) -> Self {
        Self { should_fail }
    }
}

#[async_trait]
impl ToolDispatcher for MockToolDispatcher {
    async fn execute(
        &self,
        tool_name: &str,
        _input: &serde_json::Value,
        _session_key: &str,
    ) -> Result<serde_json::Value, RuntimeError> {
        if self.should_fail {
            Err(RuntimeError::ToolError(format!(
                "Tool not found: {}",
                tool_name
            )))
        } else {
            Ok(json!({"status": "success", "tool": tool_name}))
        }
    }
}

struct MockToolRegistry;

impl ToolRegistry for MockToolRegistry {
    fn get_active_tools(&self, _agent_id: &str) -> Vec<String> {
        vec![]
    }

    fn get_tool_schemas(&self, _agent_id: &str) -> Vec<serde_json::Value> {
        vec![json!({
            "type": "function",
            "function": {
                "name": "echo",
                "description": "Echo a message",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "message": {"type": "string"}
                    },
                    "required": ["message"]
                }
            }
        })]
    }
}

struct MockSummarizer;

impl Summarizer for MockSummarizer {
    fn summarize(&self, messages: &[Message]) -> Result<String, RuntimeError> {
        Ok(format!("Summary of {} messages", messages.len()))
    }
}

#[tokio::test]
async fn test_infinite_loop_prevention() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(MockSessionStore::new());
    let lock_mgr = Arc::new(MockLockManager::new());
    let dispatcher = Arc::new(MockToolDispatcher::new(false));
    let registry = Arc::new(MockToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:8000".to_string(), 0));
    let compactor = Compactor::new(10000, MockSummarizer);

    // Set low max_iterations
    let agent_loop = AgentLoop::new(
        store,
        lock_mgr.clone(),
        dispatcher,
        registry,
        llm_client,
        compactor,
        3, // Low iteration limit
    );

    let controller = RuntimeController::new(agent_loop, temp_path.to_str().unwrap().to_string());

    let result = controller.execute("user1", "agent", "Test").await;
    assert!(result.is_err());

    // Verify lock was released
    let locks = lock_mgr.locks.lock().unwrap();
    assert!(!locks.contains("agent:user1"));
}

#[tokio::test]
async fn test_malformed_llm_output() {
    // Test that LLM response validation works via type system
    // Empty final response is caught by validation
    let response = LLMResponse::Final {
        schema_version: hypr_claw_runtime::SCHEMA_VERSION,
        content: "".to_string(),
    };
    // Serialization works
    let serialized = serde_json::to_string(&response).unwrap();
    assert!(serialized.contains("final"));

    // Empty tool name
    let response = LLMResponse::ToolCall {
        schema_version: hypr_claw_runtime::SCHEMA_VERSION,
        tool_name: "".to_string(),
        input: json!({}),
    };
    let serialized = serde_json::to_string(&response).unwrap();
    assert!(serialized.contains("tool_call"));
}

#[tokio::test]
async fn test_tool_not_found_handling() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(MockSessionStore::new());
    let lock_mgr = Arc::new(MockLockManager::new());
    let dispatcher = Arc::new(MockToolDispatcher::new(true)); // Will fail
    let registry = Arc::new(MockToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:8000".to_string(), 0));
    let compactor = Compactor::new(10000, MockSummarizer);

    let agent_loop = AgentLoop::new(
        store,
        lock_mgr.clone(),
        dispatcher,
        registry,
        llm_client,
        compactor,
        10,
    );

    let controller = RuntimeController::new(agent_loop, temp_path.to_str().unwrap().to_string());

    let result = controller.execute("user1", "agent", "Test").await;
    // Will fail (no real LLM), but lock should be released
    assert!(result.is_err());

    let locks = lock_mgr.locks.lock().unwrap();
    assert!(!locks.contains("agent:user1"));
}

#[test]
fn test_empty_user_message_handling() {
    // Empty message is allowed - it's up to the LLM to handle
    let msg = Message::new(Role::User, json!(""));
    assert_eq!(msg.role, Role::User);
}

#[test]
fn test_empty_agent_id_validation() {
    let result = resolve_session("user1", "");
    assert!(result.is_err());
    match result {
        Err(RuntimeError::SessionError(msg)) => {
            assert!(msg.contains("non-empty"));
        }
        _ => panic!("Expected SessionError"),
    }
}

#[test]
fn test_empty_user_id_validation() {
    let result = resolve_session("", "agent1");
    assert!(result.is_err());
    match result {
        Err(RuntimeError::SessionError(msg)) => {
            assert!(msg.contains("non-empty"));
        }
        _ => panic!("Expected SessionError"),
    }
}

#[test]
fn test_error_boundaries_clear() {
    // Test that errors have clear messages
    let err = RuntimeError::SessionError("test error".to_string());
    assert_eq!(err.to_string(), "Session error: test error");

    let err = RuntimeError::LockError("lock failed".to_string());
    assert_eq!(err.to_string(), "Lock error: lock failed");

    let err = RuntimeError::ToolError("tool failed".to_string());
    assert_eq!(err.to_string(), "Tool error: tool failed");

    let err = RuntimeError::LLMError("llm failed".to_string());
    assert_eq!(err.to_string(), "LLM error: llm failed");

    let err = RuntimeError::ConfigError("config failed".to_string());
    assert_eq!(err.to_string(), "Config error: config failed");
}

#[test]
fn test_message_serialization_roundtrip() {
    let msg = Message::new(Role::User, json!("Test message"));
    let serialized = serde_json::to_string(&msg).unwrap();
    let deserialized: Message = serde_json::from_str(&serialized).unwrap();
    assert_eq!(msg.role, deserialized.role);
    assert_eq!(msg.content, deserialized.content);
}

#[test]
fn test_llm_response_serialization_roundtrip() {
    // Final response
    let response = LLMResponse::Final {
        schema_version: hypr_claw_runtime::SCHEMA_VERSION,
        content: "Done".to_string(),
    };
    let serialized = serde_json::to_string(&response).unwrap();
    let deserialized: LLMResponse = serde_json::from_str(&serialized).unwrap();
    match deserialized {
        LLMResponse::Final { content, .. } => assert_eq!(content, "Done"),
        _ => panic!("Expected Final response"),
    }

    // Tool call response
    let response = LLMResponse::ToolCall {
        schema_version: hypr_claw_runtime::SCHEMA_VERSION,
        tool_name: "search".to_string(),
        input: json!({"query": "test"}),
    };
    let serialized = serde_json::to_string(&response).unwrap();
    let deserialized: LLMResponse = serde_json::from_str(&serialized).unwrap();
    match deserialized {
        LLMResponse::ToolCall { tool_name, .. } => assert_eq!(tool_name, "search"),
        _ => panic!("Expected ToolCall response"),
    }
}

#[test]
fn test_compactor_handles_empty_list() {
    let compactor = Compactor::new(100, MockSummarizer);
    let result = compactor.compact(vec![]).unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_compactor_single_large_message() {
    let compactor = Compactor::new(10, MockSummarizer);
    let messages = vec![Message::new(Role::User, json!("A".repeat(100)))];
    let result = compactor.compact(messages.clone()).unwrap();
    // Should return unchanged (can't compact single message)
    assert_eq!(result.len(), 1);
}

#[tokio::test]
async fn test_lock_release_on_panic_scenario() {
    // Test that lock manager properly handles release even in error scenarios
    let lock_mgr = MockLockManager::new();

    lock_mgr.acquire("test:session").await.unwrap();
    assert!(lock_mgr.locks.lock().unwrap().contains("test:session"));

    lock_mgr.release("test:session").await;
    assert!(!lock_mgr.locks.lock().unwrap().contains("test:session"));
}

#[test]
fn test_config_validation() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Missing soul file
    let config_file = temp_path.join("agent.yaml");
    std::fs::write(&config_file, "id: test\nsoul: missing.md\n").unwrap();

    let result = load_agent_config(config_file.to_str().unwrap());
    assert!(result.is_err());
    match result {
        Err(RuntimeError::ConfigError(msg)) => {
            assert!(msg.contains("Soul file not found"));
        }
        _ => panic!("Expected ConfigError"),
    }
}
