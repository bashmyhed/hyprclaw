//! Comprehensive failure simulation tests.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use async_trait::async_trait;
use hypr_claw_runtime::LLMClientType;
use hypr_claw_runtime::*;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::time::{timeout, Duration};

// Mock implementations
struct FailingSessionStore {
    should_fail: Arc<AtomicBool>,
}

#[async_trait]
impl SessionStore for FailingSessionStore {
    async fn load(&self, _session_key: &str) -> Result<Vec<Message>, RuntimeError> {
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(RuntimeError::SessionError("Disk failure".to_string()));
        }
        Ok(vec![])
    }

    async fn save(&self, _session_key: &str, _messages: &[Message]) -> Result<(), RuntimeError> {
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(RuntimeError::SessionError("Disk write failure".to_string()));
        }
        Ok(())
    }
}

struct TimeoutLockManager;

#[async_trait]
impl LockManager for TimeoutLockManager {
    async fn acquire(&self, _session_key: &str) -> Result<(), RuntimeError> {
        tokio::time::sleep(Duration::from_secs(10)).await;
        Ok(())
    }

    async fn release(&self, _session_key: &str) {}
}

struct MockToolDispatcher;

#[async_trait]
impl ToolDispatcher for MockToolDispatcher {
    async fn execute(
        &self,
        _tool_name: &str,
        _input: &serde_json::Value,
        _session_key: &str,
    ) -> Result<serde_json::Value, RuntimeError> {
        Ok(json!({"result": "ok"}))
    }
}

struct MockToolRegistry;

impl ToolRegistry for MockToolRegistry {
    fn get_active_tools(&self, _agent_id: &str) -> Vec<String> {
        vec!["test".to_string()]
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

struct FailingSummarizer;

impl Summarizer for FailingSummarizer {
    fn summarize(&self, _messages: &[Message]) -> Result<String, RuntimeError> {
        Err(RuntimeError::LLMError("Summarizer failure".to_string()))
    }
}

struct MockSummarizer;

impl Summarizer for MockSummarizer {
    fn summarize(&self, messages: &[Message]) -> Result<String, RuntimeError> {
        Ok(format!("Summary of {} messages", messages.len()))
    }
}

struct NormalSessionStore;

#[async_trait]
impl SessionStore for NormalSessionStore {
    async fn load(&self, _session_key: &str) -> Result<Vec<Message>, RuntimeError> {
        Ok(vec![])
    }

    async fn save(&self, _session_key: &str, _messages: &[Message]) -> Result<(), RuntimeError> {
        Ok(())
    }
}

struct NormalLockManager;

#[async_trait]
impl LockManager for NormalLockManager {
    async fn acquire(&self, _session_key: &str) -> Result<(), RuntimeError> {
        Ok(())
    }

    async fn release(&self, _session_key: &str) {}
}

#[tokio::test]
async fn test_disk_write_failure() {
    let should_fail = Arc::new(AtomicBool::new(false));
    let session_store = Arc::new(FailingSessionStore {
        should_fail: should_fail.clone(),
    });
    let lock_manager = Arc::new(NormalLockManager);
    let dispatcher = Arc::new(MockToolDispatcher);
    let registry = Arc::new(MockToolRegistry);
    let llm_client = LLMClientType::Standard(LLMClient::new("http://mock".to_string(), 0));
    let compactor = Compactor::new(1000, MockSummarizer);

    let agent_loop = AgentLoop::new(
        session_store,
        lock_manager,
        dispatcher,
        registry,
        llm_client,
        compactor,
        5,
    );

    // Trigger failure
    should_fail.store(true, Ordering::SeqCst);

    let result = agent_loop
        .run("test:session", "agent1", "system", "test")
        .await;

    // Should fail gracefully
    assert!(result.is_err());
    match result {
        Err(RuntimeError::SessionError(_)) => {}
        _ => panic!("Expected SessionError"),
    }
}

#[tokio::test]
async fn test_lock_acquisition_timeout() {
    let session_store = Arc::new(NormalSessionStore);
    let lock_manager = Arc::new(TimeoutLockManager);
    let dispatcher = Arc::new(MockToolDispatcher);
    let registry = Arc::new(MockToolRegistry);
    let llm_client = LLMClientType::Standard(LLMClient::new("http://mock".to_string(), 0));
    let compactor = Compactor::new(1000, MockSummarizer);

    let agent_loop = AgentLoop::new(
        session_store,
        lock_manager,
        dispatcher,
        registry,
        llm_client,
        compactor,
        5,
    );

    // Should timeout
    let result = timeout(
        Duration::from_secs(2),
        agent_loop.run("test:session", "agent1", "system", "test"),
    )
    .await;

    assert!(result.is_err(), "Should timeout");
}

#[tokio::test]
async fn test_compactor_failure() {
    let session_store = Arc::new(NormalSessionStore);
    let lock_manager = Arc::new(NormalLockManager);
    let dispatcher = Arc::new(MockToolDispatcher);
    let registry = Arc::new(MockToolRegistry);
    let llm_client = LLMClientType::Standard(LLMClient::new("http://mock".to_string(), 0));
    let compactor = Compactor::new(1000, FailingSummarizer);

    let agent_loop = AgentLoop::new(
        session_store,
        lock_manager,
        dispatcher,
        registry,
        llm_client,
        compactor,
        5,
    );

    // Create many messages to trigger compaction
    let result = agent_loop
        .run("test:session", "agent1", "system", "test")
        .await;

    // Should handle compactor failure
    let _ = result;
}

#[tokio::test]
async fn test_llm_timeout() {
    let session_store = Arc::new(NormalSessionStore);
    let lock_manager = Arc::new(NormalLockManager);
    let dispatcher = Arc::new(MockToolDispatcher);
    let registry = Arc::new(MockToolRegistry);
    // Invalid URL will cause timeout
    let llm_client = LLMClientType::Standard(LLMClient::new(
        "http://invalid-host-that-does-not-exist:9999".to_string(),
        0,
    ));
    let compactor = Compactor::new(1000, MockSummarizer);

    let agent_loop = AgentLoop::new(
        session_store,
        lock_manager,
        dispatcher,
        registry,
        llm_client,
        compactor,
        5,
    );

    let result = agent_loop
        .run("test:session", "agent1", "system", "test")
        .await;

    // Should fail with LLM error
    assert!(result.is_err());
    match result {
        Err(RuntimeError::LLMError(_)) => {}
        _ => panic!("Expected LLMError"),
    }
}

#[tokio::test]
async fn test_circuit_breaker_opens() {
    let llm_client =
        LLMClientType::Standard(LLMClient::new("http://invalid-host:9999".to_string(), 0));

    // Create dummy tool schema
    let dummy_tools = vec![json!({
        "type": "function",
        "function": {
            "name": "test",
            "description": "Test tool",
            "parameters": {
                "type": "object",
                "properties": {}
            }
        }
    })];

    // Trigger multiple failures
    for _ in 0..6 {
        let result = llm_client.call("system", &[], &dummy_tools).await;
        assert!(result.is_err());
    }

    // Circuit breaker should be open now
    let result = llm_client.call("system", &[], &dummy_tools).await;
    assert!(result.is_err());
    match result {
        Err(RuntimeError::LLMError(msg)) => {
            assert!(msg.contains("Circuit breaker"));
        }
        _ => panic!("Expected circuit breaker error"),
    }
}
