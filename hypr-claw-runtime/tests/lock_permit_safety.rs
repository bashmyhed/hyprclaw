#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Lock and permit safety audit tests.

use async_trait::async_trait;
use hypr_claw_runtime::LLMClientType;
use hypr_claw_runtime::*;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

// Thread-safe mock with counters
struct AuditSessionStore {
    storage: Mutex<HashMap<String, Vec<Message>>>,
}

impl AuditSessionStore {
    fn new() -> Self {
        Self {
            storage: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl SessionStore for AuditSessionStore {
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

struct AuditLockManager {
    locks: Mutex<HashSet<String>>,
    acquire_count: Mutex<usize>,
    release_count: Mutex<usize>,
}

impl AuditLockManager {
    fn new() -> Self {
        Self {
            locks: Mutex::new(HashSet::new()),
            acquire_count: Mutex::new(0),
            release_count: Mutex::new(0),
        }
    }

    fn get_acquire_count(&self) -> usize {
        *self.acquire_count.lock().unwrap()
    }

    fn get_release_count(&self) -> usize {
        *self.release_count.lock().unwrap()
    }

    fn get_active_locks(&self) -> usize {
        self.locks.lock().unwrap().len()
    }
}

#[async_trait]
impl LockManager for AuditLockManager {
    async fn acquire(&self, session_key: &str) -> Result<(), RuntimeError> {
        let mut locks = self.locks.lock().unwrap();
        let mut count = self.acquire_count.lock().unwrap();
        *count += 1;

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
        let mut count = self.release_count.lock().unwrap();
        *count += 1;
        locks.remove(session_key);
    }
}

struct AuditToolDispatcher;

#[async_trait]
impl ToolDispatcher for AuditToolDispatcher {
    async fn execute(
        &self,
        tool_name: &str,
        _input: &serde_json::Value,
        _session_key: &str,
    ) -> Result<serde_json::Value, RuntimeError> {
        Ok(json!({"status": "success", "tool": tool_name}))
    }
}

struct FailingToolDispatcher;

#[async_trait]
impl ToolDispatcher for FailingToolDispatcher {
    async fn execute(
        &self,
        _tool_name: &str,
        _input: &serde_json::Value,
        _session_key: &str,
    ) -> Result<serde_json::Value, RuntimeError> {
        Err(RuntimeError::ToolError("Tool execution failed".to_string()))
    }
}

struct AuditToolRegistry;

impl ToolRegistry for AuditToolRegistry {
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

struct AuditSummarizer;

impl Summarizer for AuditSummarizer {
    fn summarize(&self, messages: &[Message]) -> Result<String, RuntimeError> {
        Ok(format!("Summary of {} messages", messages.len()))
    }
}

#[tokio::test]
async fn test_lock_released_on_normal_completion() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(AuditSessionStore::new());
    let lock_mgr = Arc::new(AuditLockManager::new());
    let dispatcher = Arc::new(AuditToolDispatcher);
    let registry = Arc::new(AuditToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
    let compactor = Compactor::new(10000, AuditSummarizer);

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

    let _ = controller.execute("user1", "agent", "Test").await;

    // Verify lock was acquired and released
    assert_eq!(lock_mgr.get_acquire_count(), 1);
    assert_eq!(lock_mgr.get_release_count(), 1);
    assert_eq!(lock_mgr.get_active_locks(), 0);
}

#[tokio::test]
async fn test_lock_released_on_llm_failure() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(AuditSessionStore::new());
    let lock_mgr = Arc::new(AuditLockManager::new());
    let dispatcher = Arc::new(AuditToolDispatcher);
    let registry = Arc::new(AuditToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
    let compactor = Compactor::new(10000, AuditSummarizer);

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
    assert!(result.is_err());

    // Lock must be released even on LLM failure
    assert_eq!(lock_mgr.get_acquire_count(), 1);
    assert_eq!(lock_mgr.get_release_count(), 1);
    assert_eq!(lock_mgr.get_active_locks(), 0);
}

#[tokio::test]
async fn test_lock_released_on_tool_failure() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(AuditSessionStore::new());
    let lock_mgr = Arc::new(AuditLockManager::new());
    let dispatcher = Arc::new(FailingToolDispatcher);
    let registry = Arc::new(AuditToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
    let compactor = Compactor::new(10000, AuditSummarizer);

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
    assert!(result.is_err());

    // Lock must be released even on tool failure
    assert_eq!(lock_mgr.get_acquire_count(), 1);
    assert_eq!(lock_mgr.get_release_count(), 1);
    assert_eq!(lock_mgr.get_active_locks(), 0);
}

#[tokio::test]
async fn test_multiple_requests_no_lock_leak() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(AuditSessionStore::new());
    let lock_mgr = Arc::new(AuditLockManager::new());
    let dispatcher = Arc::new(AuditToolDispatcher);
    let registry = Arc::new(AuditToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
    let compactor = Compactor::new(10000, AuditSummarizer);

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

    // Execute multiple requests
    for i in 0..10 {
        let _ = controller
            .execute(&format!("user{}", i), "agent", "Test")
            .await;
    }

    // All locks should be released
    assert_eq!(lock_mgr.get_acquire_count(), 10);
    assert_eq!(lock_mgr.get_release_count(), 10);
    assert_eq!(lock_mgr.get_active_locks(), 0);
}

#[tokio::test]
async fn test_concurrent_requests_no_lock_leak() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(AuditSessionStore::new());
    let lock_mgr = Arc::new(AuditLockManager::new());
    let dispatcher = Arc::new(AuditToolDispatcher);
    let registry = Arc::new(AuditToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
    let compactor = Compactor::new(10000, AuditSummarizer);

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

    // Spawn concurrent requests
    let mut tasks = vec![];
    for i in 0..10 {
        let controller_clone = controller.clone();
        let task = tokio::spawn(async move {
            controller_clone
                .execute(&format!("user{}", i), "agent", "Test")
                .await
        });
        tasks.push(task);
    }

    // Wait for all to complete
    for task in tasks {
        let _ = task.await;
    }

    // All locks should be released
    assert_eq!(lock_mgr.get_acquire_count(), 10);
    assert_eq!(lock_mgr.get_release_count(), 10);
    assert_eq!(lock_mgr.get_active_locks(), 0);
}

#[tokio::test]
async fn test_lock_balance_after_mixed_success_failure() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    std::fs::write(temp_path.join("agent.md"), "You are helpful.").unwrap();
    std::fs::write(temp_path.join("agent.yaml"), "id: agent\nsoul: agent.md\n").unwrap();

    let store = Arc::new(AuditSessionStore::new());
    let lock_mgr = Arc::new(AuditLockManager::new());
    let dispatcher = Arc::new(AuditToolDispatcher);
    let registry = Arc::new(AuditToolRegistry);
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://localhost:9999".to_string(), 0));
    let compactor = Compactor::new(10000, AuditSummarizer);

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

    // Execute requests for different users sequentially to avoid lock conflicts
    for i in 0..20 {
        let _ = controller
            .execute(&format!("user{}", i), "agent", "Test")
            .await;
    }

    // Acquire and release counts must match
    assert_eq!(
        lock_mgr.get_acquire_count(),
        lock_mgr.get_release_count(),
        "Lock leak detected: acquire != release"
    );
    assert_eq!(lock_mgr.get_active_locks(), 0, "Active locks remain");
}

#[tokio::test]
async fn test_lock_manager_invariant() {
    let lock_mgr = AuditLockManager::new();

    // Simulate various operations
    for i in 0..100 {
        let key = format!("session:{}", i);
        let _ = lock_mgr.acquire(&key).await;
        lock_mgr.release(&key).await;
    }

    // Invariant: acquire_count == release_count
    assert_eq!(lock_mgr.get_acquire_count(), lock_mgr.get_release_count());
    assert_eq!(lock_mgr.get_active_locks(), 0);
}

#[tokio::test]
async fn test_no_resource_leak_on_repeated_operations() {
    let lock_mgr = AuditLockManager::new();

    // Repeated acquire/release cycles
    for _ in 0..1000 {
        let _ = lock_mgr.acquire("test_session").await;
        lock_mgr.release("test_session").await;
    }

    // Should have no active locks
    assert_eq!(lock_mgr.get_active_locks(), 0);
    assert_eq!(lock_mgr.get_acquire_count(), 1000);
    assert_eq!(lock_mgr.get_release_count(), 1000);
}
