#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Integration tests for concurrency and session safety.

use async_trait::async_trait;
use hypr_claw_runtime::LLMClientType;
use hypr_claw_runtime::*;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;

// Thread-safe mock implementations
struct ThreadSafeSessionStore {
    storage: Arc<Mutex<HashMap<String, Vec<Message>>>>,
}

impl ThreadSafeSessionStore {
    fn new() -> Self {
        Self {
            storage: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl SessionStore for ThreadSafeSessionStore {
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

struct ThreadSafeLockManager {
    locks: Arc<Mutex<HashSet<String>>>,
    call_log: Arc<Mutex<Vec<(String, String)>>>,
}

impl ThreadSafeLockManager {
    fn new() -> Self {
        Self {
            locks: Arc::new(Mutex::new(HashSet::new())),
            call_log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn get_call_log(&self) -> Vec<(String, String)> {
        self.call_log.lock().unwrap().clone()
    }
}

#[async_trait]
impl LockManager for ThreadSafeLockManager {
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

struct MockToolDispatcher;

#[async_trait]
impl ToolDispatcher for MockToolDispatcher {
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
async fn test_parallel_different_sessions() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Create agent config
    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(ThreadSafeSessionStore::new());
    let lock_mgr = Arc::new(ThreadSafeLockManager::new());
    let dispatcher = Arc::new(MockToolDispatcher);
    let registry = Arc::new(MockToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:8000".to_string(), 0));
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

    let controller = Arc::new(RuntimeController::new(
        agent_loop,
        temp_path.to_str().unwrap().to_string(),
    ));

    // Spawn parallel tasks for different users
    let controller1 = controller.clone();
    let task1 = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        controller1.execute("user1", "agent", "Message 1").await
    });

    let controller2 = controller.clone();
    let task2 = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        controller2.execute("user2", "agent", "Message 2").await
    });

    let result1 = task1.await.unwrap();
    let result2 = task2.await.unwrap();

    // Both should fail (no real LLM), but locks should be released
    assert!(result1.is_err());
    assert!(result2.is_err());

    // Verify lock sequence - each session should have acquire/release
    let call_log = lock_mgr.get_call_log();
    assert!(call_log.len() >= 4); // At least 2 acquire + 2 release

    let user1_logs: Vec<_> = call_log
        .iter()
        .filter(|(_, key)| key == "agent:user1")
        .collect();
    let user2_logs: Vec<_> = call_log
        .iter()
        .filter(|(_, key)| key == "agent:user2")
        .collect();

    assert!(user1_logs.len() >= 2);
    assert!(user2_logs.len() >= 2);
}

#[tokio::test]
async fn test_sequential_same_session() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(ThreadSafeSessionStore::new());
    let lock_mgr = Arc::new(ThreadSafeLockManager::new());
    let dispatcher = Arc::new(MockToolDispatcher);
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

    let controller = Arc::new(RuntimeController::new(
        agent_loop,
        temp_path.to_str().unwrap().to_string(),
    ));

    // Try to execute same session in parallel
    let controller1 = controller.clone();
    let task1 =
        tokio::spawn(async move { controller1.execute("user1", "agent", "Message 1").await });

    // Give first task time to acquire lock
    tokio::time::sleep(Duration::from_millis(10)).await;

    let controller2 = controller.clone();
    let task2 =
        tokio::spawn(async move { controller2.execute("user1", "agent", "Message 2").await });

    let result1 = task1.await.unwrap();
    let result2 = task2.await.unwrap();

    // At least one should fail (either LLM error or lock error)
    assert!(result1.is_err() || result2.is_err());

    // Verify locks were used correctly
    let call_log = lock_mgr.get_call_log();
    assert!(!call_log.is_empty());
}

#[tokio::test]
async fn test_lock_released_on_error() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(ThreadSafeSessionStore::new());
    let lock_mgr = Arc::new(ThreadSafeLockManager::new());
    let dispatcher = Arc::new(MockToolDispatcher);
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

    // First call fails (no real LLM)
    let result = controller.execute("user1", "agent", "Message 1").await;
    assert!(result.is_err());

    // Lock should be released - verify by checking call log
    let call_log = lock_mgr.get_call_log();
    let last_action = &call_log.last().unwrap().0;
    assert_eq!(last_action, "release");

    // Second call should be able to acquire lock
    let result2 = controller.execute("user1", "agent", "Message 2").await;
    // Will also fail, but should not fail with lock error
    assert!(result2.is_err());
    if let Err(e) = result2 {
        assert!(!matches!(e, RuntimeError::LockError(_)));
    }
}

#[tokio::test]
async fn test_no_session_corruption() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(ThreadSafeSessionStore::new());
    let lock_mgr = Arc::new(ThreadSafeLockManager::new());
    let dispatcher = Arc::new(MockToolDispatcher);
    let registry = Arc::new(MockToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:8000".to_string(), 0));
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

    let controller = Arc::new(RuntimeController::new(
        agent_loop,
        temp_path.to_str().unwrap().to_string(),
    ));

    // Execute multiple requests for different users in parallel
    let mut tasks = vec![];

    for user_id in &["user1", "user2"] {
        let controller_clone = controller.clone();
        let user_id = user_id.to_string();
        let task = tokio::spawn(async move {
            for i in 0..3 {
                tokio::time::sleep(Duration::from_millis(i * 5)).await;
                let _ = controller_clone
                    .execute(&user_id, "agent", &format!("Message {}", i + 1))
                    .await;
            }
        });
        tasks.push(task);
    }

    for task in tasks {
        task.await.unwrap();
    }

    // Verify lock manager was used correctly - should have acquire/release pairs
    let call_log = lock_mgr.get_call_log();
    assert!(!call_log.is_empty());

    // Count acquire and release calls
    let acquires = call_log
        .iter()
        .filter(|(action, _)| action == "acquire")
        .count();
    let releases = call_log
        .iter()
        .filter(|(action, _)| action == "release")
        .count();

    // Should have equal number of acquires and releases (all locks released)
    assert_eq!(acquires, releases);
}
