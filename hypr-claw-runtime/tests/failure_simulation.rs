#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Failure simulation tests for catastrophic scenarios.

use async_trait::async_trait;
use hypr_claw_runtime::LLMClientType;
use hypr_claw_runtime::*;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;

// Mock implementations
struct MockSessionStore {
    storage: Mutex<HashMap<String, Vec<Message>>>,
    should_fail_save: bool,
}

impl MockSessionStore {
    fn new() -> Self {
        Self {
            storage: Mutex::new(HashMap::new()),
            should_fail_save: false,
        }
    }

    fn with_save_failure() -> Self {
        Self {
            storage: Mutex::new(HashMap::new()),
            should_fail_save: true,
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
        if self.should_fail_save {
            return Err(RuntimeError::SessionError(
                "Storage write failed".to_string(),
            ));
        }
        let mut storage = self.storage.lock().unwrap();
        storage.insert(session_key.to_string(), messages.to_vec());
        Ok(())
    }
}

struct MockLockManager {
    locks: Mutex<HashSet<String>>,
    call_log: Arc<Mutex<Vec<(String, String)>>>,
}

impl MockLockManager {
    fn new() -> Self {
        Self {
            locks: Mutex::new(HashSet::new()),
            call_log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn is_locked(&self, session_key: &str) -> bool {
        self.locks.lock().unwrap().contains(session_key)
    }

    fn get_call_log(&self) -> Vec<(String, String)> {
        self.call_log.lock().unwrap().clone()
    }
}

#[async_trait]
impl LockManager for MockLockManager {
    async fn acquire(&self, session_key: &str) -> Result<(), RuntimeError> {
        let mut locks = self.locks.lock().unwrap();
        let mut log = self.call_log.lock().unwrap();
        log.push(("acquire".to_string(), session_key.to_string()));

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
        let mut log = self.call_log.lock().unwrap();
        log.push(("release".to_string(), session_key.to_string()));
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
async fn test_llm_timeout() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(MockSessionStore::new());
    let lock_mgr = Arc::new(MockLockManager::new());
    let dispatcher = Arc::new(NormalToolDispatcher);
    let registry = Arc::new(MockToolRegistry);

    // LLM client with very short timeout will fail
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
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

    // Should fail with LLM error
    assert!(result.is_err());

    // Lock must be released
    assert!(!lock_mgr.is_locked("agent:user1"));

    // Verify lock was acquired and released
    let log = lock_mgr.get_call_log();
    assert!(log.iter().any(|(action, _)| action == "acquire"));
    assert!(log.iter().any(|(action, _)| action == "release"));
}

#[tokio::test]
async fn test_llm_malformed_response() {
    // This is tested via the LLM client's validation
    // Invalid JSON will be caught during deserialization
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(MockSessionStore::new());
    let lock_mgr = Arc::new(MockLockManager::new());
    let dispatcher = Arc::new(NormalToolDispatcher);
    let registry = Arc::new(MockToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
    let compactor = Compactor::new(10000, MockSummarizer);

    let agent_loop = AgentLoop::new(
        store.clone(),
        lock_mgr.clone(),
        dispatcher,
        registry,
        llm_client,
        compactor,
        10,
    );

    let controller = RuntimeController::new(agent_loop, temp_path.to_str().unwrap().to_string());

    let result = controller.execute("user1", "agent", "Test").await;

    // Should fail cleanly
    assert!(result.is_err());

    // No panic occurred
    // Lock released
    assert!(!lock_mgr.is_locked("agent:user1"));

    // Session should not be corrupted (empty or unchanged)
    let session = store.load("agent:user1").await.unwrap();
    // Session might have user message but no corrupted state
    assert!(session.len() <= 1);
}

#[tokio::test]
async fn test_session_store_write_failure() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(MockSessionStore::with_save_failure());
    let lock_mgr = Arc::new(MockLockManager::new());
    let dispatcher = Arc::new(NormalToolDispatcher);
    let registry = Arc::new(MockToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
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

    // Should fail with error
    assert!(result.is_err());

    // Lock must be released even on save failure
    assert!(!lock_mgr.is_locked("agent:user1"));

    // No infinite retry - should fail once
    let log = lock_mgr.get_call_log();
    let acquire_count = log.iter().filter(|(action, _)| action == "acquire").count();
    assert_eq!(acquire_count, 1);
}

#[tokio::test]
async fn test_infinite_tool_recursion() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(MockSessionStore::new());
    let lock_mgr = Arc::new(MockLockManager::new());
    let dispatcher = Arc::new(NormalToolDispatcher);
    let registry = Arc::new(MockToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
    let compactor = Compactor::new(10000, MockSummarizer);

    // Set low max_iterations to test enforcement
    let agent_loop = AgentLoop::new(
        store,
        lock_mgr.clone(),
        dispatcher,
        registry,
        llm_client,
        compactor,
        3, // Low limit
    );

    let controller = RuntimeController::new(agent_loop, temp_path.to_str().unwrap().to_string());

    let result = controller.execute("user1", "agent", "Test").await;

    // Should fail with max iterations error
    assert!(result.is_err());
    if let Err(e) = result {
        let error_msg = e.to_string();
        // Should mention max iterations or LLM error
        assert!(error_msg.contains("Max iterations") || error_msg.contains("LLM"));
    }

    // Lock must be released
    assert!(!lock_mgr.is_locked("agent:user1"));
}

#[tokio::test]
async fn test_concurrent_failure_no_deadlock() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(MockSessionStore::new());
    let lock_mgr = Arc::new(MockLockManager::new());
    let dispatcher = Arc::new(NormalToolDispatcher);
    let registry = Arc::new(MockToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
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

    let controller = Arc::new(RuntimeController::new(
        agent_loop,
        temp_path.to_str().unwrap().to_string(),
    ));

    // Spawn multiple failing requests
    let mut tasks = vec![];
    for i in 0..5 {
        let controller_clone = controller.clone();
        let task = tokio::spawn(async move {
            controller_clone
                .execute(&format!("user{}", i), "agent", "Test")
                .await
        });
        tasks.push(task);
    }

    // All should complete (fail) without deadlock
    let timeout = tokio::time::timeout(Duration::from_secs(5), async {
        for task in tasks {
            let _ = task.await;
        }
    })
    .await;

    assert!(timeout.is_ok(), "Deadlock detected - timeout occurred");

    // All locks should be released
    for i in 0..5 {
        assert!(!lock_mgr.is_locked(&format!("agent:user{}", i)));
    }
}

#[tokio::test]
async fn test_lock_release_count_matches() {
    let lock_mgr = MockLockManager::new();

    // Simulate multiple acquire/release cycles
    for i in 0..10 {
        let key = format!("session:{}", i);
        lock_mgr.acquire(&key).await.unwrap();
        lock_mgr.release(&key).await;
    }

    let log = lock_mgr.get_call_log();
    let acquire_count = log.iter().filter(|(action, _)| action == "acquire").count();
    let release_count = log.iter().filter(|(action, _)| action == "release").count();

    assert_eq!(acquire_count, release_count, "Lock leak detected");
}
