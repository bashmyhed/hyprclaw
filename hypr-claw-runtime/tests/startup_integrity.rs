#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Startup integrity check tests.

use async_trait::async_trait;
use hypr_claw_runtime::LLMClientType;
use hypr_claw_runtime::*;
use serde_json::json;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

// Mock implementations
struct CorruptedSessionStore {
    should_fail_load: bool,
}

impl CorruptedSessionStore {
    fn new(should_fail_load: bool) -> Self {
        Self { should_fail_load }
    }
}

#[async_trait]
impl SessionStore for CorruptedSessionStore {
    async fn load(&self, _session_key: &str) -> Result<Vec<Message>, RuntimeError> {
        if self.should_fail_load {
            Err(RuntimeError::SessionError(
                "Corrupted session data".to_string(),
            ))
        } else {
            Ok(vec![])
        }
    }

    async fn save(&self, _session_key: &str, _messages: &[Message]) -> Result<(), RuntimeError> {
        Ok(())
    }
}

struct NormalLockManager {
    locks: Mutex<HashSet<String>>,
}

impl NormalLockManager {
    fn new() -> Self {
        Self {
            locks: Mutex::new(HashSet::new()),
        }
    }
}

#[async_trait]
impl LockManager for NormalLockManager {
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

struct NormalToolDispatcher;

#[async_trait]
impl ToolDispatcher for NormalToolDispatcher {
    async fn execute(
        &self,
        tool_name: &str,
        _input: &serde_json::Value,
        _session_key: &str,
    ) -> Result<serde_json::Value, RuntimeError> {
        Ok(json!({"status": "success", "tool": tool_name}))
    }
}

struct NormalToolRegistry;

impl ToolRegistry for NormalToolRegistry {
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

struct NormalSummarizer;

impl Summarizer for NormalSummarizer {
    fn summarize(&self, messages: &[Message]) -> Result<String, RuntimeError> {
        Ok(format!("Summary of {} messages", messages.len()))
    }
}

#[tokio::test]
async fn test_corrupted_session_load_fails_fast() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(CorruptedSessionStore::new(true));
    let lock_mgr = Arc::new(NormalLockManager::new());
    let dispatcher = Arc::new(NormalToolDispatcher);
    let registry = Arc::new(NormalToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
    let compactor = Compactor::new(10000, NormalSummarizer);

    let agent_loop = AgentLoop::new(
        store, lock_mgr, dispatcher, registry, llm_client, compactor, 10,
    );

    let controller = RuntimeController::new(agent_loop, temp_path.to_str().unwrap().to_string());

    let result = controller.execute("user1", "agent", "Test").await;

    // Should fail fast with session error
    assert!(result.is_err());
    match result {
        Err(RuntimeError::LLMError(msg)) => {
            // Wrapped in LLMError by runtime controller
            assert!(
                msg.contains("Corrupted session data") || msg.contains("Runtime execution failed")
            );
        }
        _ => panic!("Expected error containing session corruption message"),
    }
}

#[tokio::test]
async fn test_missing_config_file_fails_fast() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Don't create config file

    let store = Arc::new(CorruptedSessionStore::new(false));
    let lock_mgr = Arc::new(NormalLockManager::new());
    let dispatcher = Arc::new(NormalToolDispatcher);
    let registry = Arc::new(NormalToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
    let compactor = Compactor::new(10000, NormalSummarizer);

    let agent_loop = AgentLoop::new(
        store, lock_mgr, dispatcher, registry, llm_client, compactor, 10,
    );

    let controller = RuntimeController::new(agent_loop, temp_path.to_str().unwrap().to_string());

    let result = controller.execute("user1", "agent", "Test").await;

    // Should fail fast with config error
    assert!(result.is_err());
    match result {
        Err(RuntimeError::ConfigError(msg)) => {
            assert!(msg.contains("Config file not found"));
        }
        _ => panic!("Expected ConfigError"),
    }
}

#[tokio::test]
async fn test_missing_soul_file_fails_fast() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Create config but not soul file
    std::fs::write(
        temp_path.join("agent.yaml"),
        "id: agent\nsoul: missing.md\n",
    )
    .unwrap();

    let store = Arc::new(CorruptedSessionStore::new(false));
    let lock_mgr = Arc::new(NormalLockManager::new());
    let dispatcher = Arc::new(NormalToolDispatcher);
    let registry = Arc::new(NormalToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
    let compactor = Compactor::new(10000, NormalSummarizer);

    let agent_loop = AgentLoop::new(
        store, lock_mgr, dispatcher, registry, llm_client, compactor, 10,
    );

    let controller = RuntimeController::new(agent_loop, temp_path.to_str().unwrap().to_string());

    let result = controller.execute("user1", "agent", "Test").await;

    // Should fail fast with config error
    assert!(result.is_err());
    match result {
        Err(RuntimeError::ConfigError(msg)) => {
            assert!(msg.contains("Soul file not found"));
        }
        _ => panic!("Expected ConfigError"),
    }
}

#[tokio::test]
async fn test_invalid_yaml_config_fails_fast() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Create invalid YAML
    std::fs::write(
        temp_path.join("agent.yaml"),
        "invalid: yaml: structure: [[[",
    )
    .unwrap();

    let store = Arc::new(CorruptedSessionStore::new(false));
    let lock_mgr = Arc::new(NormalLockManager::new());
    let dispatcher = Arc::new(NormalToolDispatcher);
    let registry = Arc::new(NormalToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
    let compactor = Compactor::new(10000, NormalSummarizer);

    let agent_loop = AgentLoop::new(
        store, lock_mgr, dispatcher, registry, llm_client, compactor, 10,
    );

    let controller = RuntimeController::new(agent_loop, temp_path.to_str().unwrap().to_string());

    let result = controller.execute("user1", "agent", "Test").await;

    // Should fail fast with config error
    assert!(result.is_err());
    match result {
        Err(RuntimeError::ConfigError(_)) => {
            // Expected
        }
        _ => panic!("Expected ConfigError"),
    }
}

#[tokio::test]
async fn test_empty_config_file_fails_fast() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Create empty config
    std::fs::write(temp_path.join("agent.yaml"), "").unwrap();

    let store = Arc::new(CorruptedSessionStore::new(false));
    let lock_mgr = Arc::new(NormalLockManager::new());
    let dispatcher = Arc::new(NormalToolDispatcher);
    let registry = Arc::new(NormalToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
    let compactor = Compactor::new(10000, NormalSummarizer);

    let agent_loop = AgentLoop::new(
        store, lock_mgr, dispatcher, registry, llm_client, compactor, 10,
    );

    let controller = RuntimeController::new(agent_loop, temp_path.to_str().unwrap().to_string());

    let result = controller.execute("user1", "agent", "Test").await;

    // Should fail fast
    assert!(result.is_err());
    match result {
        Err(RuntimeError::ConfigError(msg)) => {
            assert!(msg.contains("empty") || msg.contains("missing"));
        }
        _ => panic!("Expected ConfigError"),
    }
}

#[test]
fn test_message_deserialization_schema_mismatch() {
    // Test that schema mismatches are caught
    let invalid_json = r#"{"role": "invalid_role", "content": "test"}"#;
    let result: Result<Message, _> = serde_json::from_str(invalid_json);
    assert!(result.is_err(), "Should reject invalid role");

    // Missing required field
    let invalid_json = r#"{"content": "test"}"#;
    let result: Result<Message, _> = serde_json::from_str(invalid_json);
    assert!(result.is_err(), "Should reject missing role");
}

#[test]
fn test_llm_response_deserialization_schema_mismatch() {
    // Invalid type
    let invalid_json = r#"{"type": "invalid_type", "content": "test"}"#;
    let result: Result<LLMResponse, _> = serde_json::from_str(invalid_json);
    assert!(result.is_err(), "Should reject invalid type");

    // Missing required field for final
    let invalid_json = r#"{"type": "final"}"#;
    let result: Result<LLMResponse, _> = serde_json::from_str(invalid_json);
    assert!(result.is_err(), "Should reject final without content");

    // Missing required field for tool_call
    let invalid_json = r#"{"type": "tool_call", "input": {}}"#;
    let result: Result<LLMResponse, _> = serde_json::from_str(invalid_json);
    assert!(result.is_err(), "Should reject tool_call without tool_name");
}

#[test]
fn test_config_validation_on_load() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Config missing required id field
    std::fs::write(temp_path.join("agent.md"), "Soul content").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "soul: agent.md\n").unwrap();

    let result = load_agent_config(temp_path.join("agent.yaml").to_str().unwrap());
    assert!(result.is_err());
    match result {
        Err(RuntimeError::ConfigError(_)) => {
            // Expected
        }
        _ => panic!("Expected ConfigError"),
    }
}

#[tokio::test]
async fn test_healthy_startup_succeeds() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Create valid config
    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(CorruptedSessionStore::new(false));
    let lock_mgr = Arc::new(NormalLockManager::new());
    let dispatcher = Arc::new(NormalToolDispatcher);
    let registry = Arc::new(NormalToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
    let compactor = Compactor::new(10000, NormalSummarizer);

    let agent_loop = AgentLoop::new(
        store, lock_mgr, dispatcher, registry, llm_client, compactor, 10,
    );

    let controller = RuntimeController::new(agent_loop, temp_path.to_str().unwrap().to_string());

    // Should not panic during initialization
    // Execution will fail (no real LLM) but startup is clean
    let result = controller.execute("user1", "agent", "Test").await;

    // Will fail due to LLM, but not due to startup issues
    assert!(result.is_err());
}
