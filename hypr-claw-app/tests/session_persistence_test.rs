//! Session persistence validation tests.

#![allow(clippy::unwrap_used)]

use async_trait::async_trait;
use hypr_claw_runtime::{Message, SessionStore};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_session_persistence_across_restarts() {
    // Setup
    let test_dir = "./test_sessions_persistence";
    std::fs::create_dir_all(test_dir).unwrap();

    let session_key = "test:persistence_user";

    // First execution - create and save session manually
    {
        let session_store =
            Arc::new(hypr_claw::infra::session_store::SessionStore::new(test_dir).unwrap());

        let async_session = Arc::new(hypr_claw_runtime::AsyncSessionStore::new(session_store));

        // Create some test messages
        let messages = vec![
            Message::new(hypr_claw_runtime::Role::User, serde_json::json!("Hello")),
            Message::new(
                hypr_claw_runtime::Role::Assistant,
                serde_json::json!("Hi there!"),
            ),
        ];

        // Save session
        async_session.save(session_key, &messages).await.unwrap();
    }

    // Simulate restart - create new runtime with same session store
    {
        let session_store =
            Arc::new(hypr_claw::infra::session_store::SessionStore::new(test_dir).unwrap());

        let async_session = Arc::new(hypr_claw_runtime::AsyncSessionStore::new(session_store));

        // Load session
        let messages: Vec<Message> = async_session.load(session_key).await.unwrap();

        // Verify session was persisted
        assert!(
            !messages.is_empty(),
            "Session should contain messages after restart"
        );
        assert_eq!(messages.len(), 2, "Should have 2 messages");

        // Verify no corruption
        for msg in &messages {
            // Each message should be valid
            let _ = serde_json::to_string(msg).expect("Message should be serializable");
        }
    }

    // Cleanup
    std::fs::remove_dir_all(test_dir).unwrap();
}

#[tokio::test]
async fn test_session_not_corrupted_on_error() {
    let test_dir = "./test_sessions_error";
    std::fs::create_dir_all(test_dir).unwrap();

    let session_key = "test:error_user";

    {
        let session_store =
            Arc::new(hypr_claw::infra::session_store::SessionStore::new(test_dir).unwrap());
        let lock_manager = Arc::new(hypr_claw::infra::lock_manager::LockManager::new(
            Duration::from_secs(30),
        ));

        let async_session = Arc::new(hypr_claw_runtime::AsyncSessionStore::new(session_store));
        let async_locks = Arc::new(hypr_claw_runtime::AsyncLockManager::new(lock_manager));

        let dispatcher = Arc::new(MockDispatcher);
        let registry = Arc::new(MockRegistry);
        // Invalid URL will cause error
        let llm_client = hypr_claw_runtime::LLMClient::new("http://invalid:9999".to_string(), 0);
        let compactor = hypr_claw_runtime::Compactor::new(1000, MockSummarizer);

        let agent_loop = hypr_claw_runtime::AgentLoop::new(
            async_session.clone(),
            async_locks,
            dispatcher,
            registry,
            hypr_claw_runtime::LLMClientType::Standard(llm_client),
            compactor,
            5,
        );

        // This will fail
        let result = agent_loop.run(session_key, "agent", "system", "test").await;
        assert!(result.is_err(), "Should fail with invalid LLM URL");

        // Session should still be loadable (not corrupted)
        let messages: Vec<Message> = async_session.load(session_key).await.unwrap();

        // Verify all messages are valid
        for msg in &messages {
            let _ = serde_json::to_string(msg).expect("Messages should be serializable");
        }
    }

    std::fs::remove_dir_all(test_dir).unwrap();
}

// Mock implementations
struct MockDispatcher;

#[async_trait]
impl hypr_claw_runtime::ToolDispatcher for MockDispatcher {
    async fn execute(
        &self,
        _tool_name: &str,
        _input: &serde_json::Value,
        _session_key: &str,
    ) -> Result<serde_json::Value, hypr_claw_runtime::RuntimeError> {
        Ok(serde_json::json!({"result": "ok"}))
    }
}

struct MockRegistry;

impl hypr_claw_runtime::ToolRegistry for MockRegistry {
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

struct MockSummarizer;

impl hypr_claw_runtime::Summarizer for MockSummarizer {
    fn summarize(
        &self,
        messages: &[hypr_claw_runtime::Message],
    ) -> Result<String, hypr_claw_runtime::RuntimeError> {
        Ok(format!("Summary of {} messages", messages.len()))
    }
}
