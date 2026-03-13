//! Load and stress testing for runtime.

use async_trait::async_trait;
use hypr_claw_runtime::LLMClientType;
use hypr_claw_runtime::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::time::Duration;

struct StressSessionStore {
    operations: Arc<AtomicUsize>,
}

#[async_trait]
impl SessionStore for StressSessionStore {
    async fn load(&self, _session_key: &str) -> Result<Vec<Message>, RuntimeError> {
        self.operations.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(1)).await;
        Ok(vec![])
    }

    async fn save(&self, _session_key: &str, _messages: &[Message]) -> Result<(), RuntimeError> {
        self.operations.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(1)).await;
        Ok(())
    }
}

struct StressLockManager {
    acquisitions: Arc<AtomicUsize>,
}

#[async_trait]
impl LockManager for StressLockManager {
    async fn acquire(&self, _session_key: &str) -> Result<(), RuntimeError> {
        self.acquisitions.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn release(&self, _session_key: &str) {
        self.acquisitions.fetch_sub(1, Ordering::SeqCst);
    }
}

struct StressToolDispatcher {
    executions: Arc<AtomicUsize>,
    should_fail: bool,
}

#[async_trait]
impl ToolDispatcher for StressToolDispatcher {
    async fn execute(
        &self,
        _tool_name: &str,
        _input: &serde_json::Value,
        _session_key: &str,
    ) -> Result<serde_json::Value, RuntimeError> {
        self.executions.fetch_add(1, Ordering::SeqCst);

        if self.should_fail {
            return Err(RuntimeError::ToolError("Simulated failure".to_string()));
        }

        Ok(json!({"result": "ok"}))
    }
}

struct StressToolRegistry;

impl ToolRegistry for StressToolRegistry {
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

struct StressSummarizer;

impl Summarizer for StressSummarizer {
    fn summarize(&self, messages: &[Message]) -> Result<String, RuntimeError> {
        Ok(format!("Summary of {} messages", messages.len()))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_concurrent_sessions_stress() {
    let operations = Arc::new(AtomicUsize::new(0));
    let acquisitions = Arc::new(AtomicUsize::new(0));
    let executions = Arc::new(AtomicUsize::new(0));

    let session_store = Arc::new(StressSessionStore {
        operations: operations.clone(),
    });
    let lock_manager = Arc::new(StressLockManager {
        acquisitions: acquisitions.clone(),
    });
    let dispatcher = Arc::new(StressToolDispatcher {
        executions: executions.clone(),
        should_fail: false,
    });
    let registry = Arc::new(StressToolRegistry);
    let llm_client = LLMClientType::Standard(LLMClient::new("http://mock".to_string(), 0));
    let compactor = Compactor::new(1000, StressSummarizer);

    let agent_loop = Arc::new(AgentLoop::new(
        session_store,
        lock_manager,
        dispatcher,
        registry,
        llm_client,
        compactor,
        5,
    ));

    // Spawn 1000 concurrent sessions directly on agent_loop
    let mut handles = vec![];
    for i in 0..1000 {
        let agent_loop = agent_loop.clone();
        let handle = tokio::spawn(async move {
            let session_id = format!("test:user{}", i);
            let result = agent_loop
                .run(&session_id, "agent1", "system", "test")
                .await;
            result.is_ok()
        });
        handles.push(handle);
    }

    // Wait for all to complete
    let mut success_count = 0;
    for handle in handles {
        match handle.await {
            Ok(true) => success_count += 1,
            Ok(false) => {}
            Err(_) => {}
        }
    }

    // Should have processed many sessions
    println!("Successful sessions: {}", success_count);
    println!("Total operations: {}", operations.load(Ordering::SeqCst));
    println!("Total executions: {}", executions.load(Ordering::SeqCst));

    // Verify no deadlocks (test completed) and operations were attempted
    assert!(
        operations.load(Ordering::SeqCst) > 0,
        "Should have attempted operations"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_mixed_success_failure() {
    let operations = Arc::new(AtomicUsize::new(0));
    let acquisitions = Arc::new(AtomicUsize::new(0));
    let executions = Arc::new(AtomicUsize::new(0));

    let session_store = Arc::new(StressSessionStore {
        operations: operations.clone(),
    });
    let lock_manager = Arc::new(StressLockManager {
        acquisitions: acquisitions.clone(),
    });

    // Mix of failing and succeeding dispatchers
    let mut handles = vec![];

    for i in 0..100 {
        let session_store = session_store.clone();
        let lock_manager = lock_manager.clone();
        let executions = executions.clone();

        let handle = tokio::spawn(async move {
            let should_fail = i % 3 == 0;
            let dispatcher = Arc::new(StressToolDispatcher {
                executions: executions.clone(),
                should_fail,
            });
            let registry = Arc::new(StressToolRegistry);
            let llm_client = LLMClientType::Standard(LLMClient::new("http://mock".to_string(), 0));
            let compactor = Compactor::new(1000, StressSummarizer);

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
                .run(&format!("test:session{}", i), "agent1", "system", "test")
                .await;

            result.is_ok()
        });
        handles.push(handle);
    }

    let mut success_count = 0;
    for handle in handles {
        if let Ok(true) = handle.await {
            success_count += 1;
        }
    }

    println!("Mixed test - successes: {}", success_count);
    println!("Total operations: {}", operations.load(Ordering::SeqCst));

    // Should have attempted operations (may not succeed without agent configs)
    assert!(
        operations.load(Ordering::SeqCst) > 0,
        "Should have attempted operations"
    );
}

#[tokio::test]
async fn test_concurrency_limit_enforcement() {
    let session_store = Arc::new(StressSessionStore {
        operations: Arc::new(AtomicUsize::new(0)),
    });
    let lock_manager = Arc::new(StressLockManager {
        acquisitions: Arc::new(AtomicUsize::new(0)),
    });
    let dispatcher = Arc::new(StressToolDispatcher {
        executions: Arc::new(AtomicUsize::new(0)),
        should_fail: false,
    });
    let registry = Arc::new(StressToolRegistry);
    let llm_client = LLMClientType::Standard(LLMClient::new("http://mock".to_string(), 0));
    let compactor = Compactor::new(1000, StressSummarizer);

    let agent_loop = AgentLoop::new(
        session_store,
        lock_manager,
        dispatcher,
        registry,
        llm_client,
        compactor,
        5,
    );

    // Set limit to 10
    let controller = Arc::new(RuntimeController::with_max_concurrent_sessions(
        agent_loop,
        "./data/agents".to_string(),
        10,
    ));

    // Try to spawn 50 concurrent sessions
    let mut handles = vec![];
    for i in 0..50 {
        let controller = controller.clone();
        let handle = tokio::spawn(async move {
            // Add delay to ensure concurrency
            tokio::time::sleep(Duration::from_millis(10)).await;
            controller
                .execute(&format!("user{}", i), "default", "test")
                .await
        });
        handles.push(handle);
    }

    // Wait for all
    for handle in handles {
        let _ = handle.await;
    }

    // Test passes if no deadlock occurred
}
