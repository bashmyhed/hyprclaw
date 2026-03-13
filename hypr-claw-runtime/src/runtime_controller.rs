//! Runtime controller - main entry point for agent execution.

use crate::agent_config::load_agent_config;
use crate::agent_loop::AgentLoop;
use crate::compactor::Summarizer;
use crate::gateway::resolve_session;
use crate::interfaces::{LockManager, RuntimeError, SessionStore, ToolDispatcher, ToolRegistry};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{debug, error, info};

/// Main runtime controller.
pub struct RuntimeController<S, L, D, R, Sum>
where
    S: SessionStore,
    L: LockManager,
    D: ToolDispatcher,
    R: ToolRegistry,
    Sum: Summarizer,
{
    agent_loop: AgentLoop<S, L, D, R, Sum>,
    config_dir: String,
    concurrency_limiter: Arc<Semaphore>,
}

impl<S, L, D, R, Sum> RuntimeController<S, L, D, R, Sum>
where
    S: SessionStore,
    L: LockManager,
    D: ToolDispatcher,
    R: ToolRegistry,
    Sum: Summarizer,
{
    /// Create a new runtime controller.
    ///
    /// # Arguments
    /// * `agent_loop` - Configured agent loop instance
    /// * `config_dir` - Directory containing agent configs
    pub fn new(agent_loop: AgentLoop<S, L, D, R, Sum>, config_dir: String) -> Self {
        Self::with_max_concurrent_sessions(agent_loop, config_dir, 100)
    }

    /// Create a new runtime controller with custom concurrency limit.
    ///
    /// # Arguments
    /// * `agent_loop` - Configured agent loop instance
    /// * `config_dir` - Directory containing agent configs
    /// * `max_concurrent_sessions` - Maximum concurrent sessions allowed
    pub fn with_max_concurrent_sessions(
        agent_loop: AgentLoop<S, L, D, R, Sum>,
        config_dir: String,
        max_concurrent_sessions: usize,
    ) -> Self {
        Self {
            agent_loop,
            config_dir,
            concurrency_limiter: Arc::new(Semaphore::new(max_concurrent_sessions)),
        }
    }

    /// Execute agent for a user message.
    ///
    /// # Arguments
    /// * `user_id` - User identifier
    /// * `agent_id` - Agent identifier
    /// * `user_message` - User's input message
    ///
    /// # Returns
    /// Agent's response
    pub async fn execute(
        &self,
        user_id: &str,
        agent_id: &str,
        user_message: &str,
    ) -> Result<String, RuntimeError> {
        // Acquire concurrency permit
        let _permit =
            self.concurrency_limiter.acquire().await.map_err(|e| {
                RuntimeError::SessionError(format!("Concurrency limit error: {}", e))
            })?;

        // Resolve session
        info!("Processing request: user={}, agent={}", user_id, agent_id);
        let session_key = resolve_session(user_id, agent_id)?;

        // Load agent config
        let config_path = format!("{}/{}.yaml", self.config_dir, agent_id);
        debug!("Loading agent config from: {}", config_path);
        let agent_config = load_agent_config(&config_path)?;

        // Execute agent loop
        let response = self
            .agent_loop
            .run(
                &session_key,
                &agent_config.id,
                &agent_config.soul,
                user_message,
            )
            .await
            .map_err(|e| {
                error!("Runtime execution failed: {}", e);
                RuntimeError::LLMError(format!("Runtime execution failed: {}", e))
            })?;

        info!(
            "Request completed successfully for session: {}",
            session_key
        );
        Ok(response)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::compactor::Compactor;
    use crate::types::Message;
    use crate::{LLMClient, LLMClientType};
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::HashMap;
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
        locks: Mutex<std::collections::HashSet<String>>,
    }

    impl MockLockManager {
        fn new() -> Self {
            Self {
                locks: Mutex::new(std::collections::HashSet::new()),
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

    impl crate::compactor::Summarizer for MockSummarizer {
        fn summarize(&self, messages: &[Message]) -> Result<String, RuntimeError> {
            Ok(format!("Summary of {} messages", messages.len()))
        }
    }

    #[tokio::test]
    async fn test_agent_config_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let store = Arc::new(MockSessionStore::new());
        let lock_mgr = Arc::new(MockLockManager::new());
        let dispatcher = Arc::new(MockToolDispatcher);
        let registry = Arc::new(MockToolRegistry);
        let llm_client =
            LLMClientType::Standard(LLMClient::new("http://localhost:8000".to_string(), 0));
        let compactor = Compactor::new(10000, MockSummarizer);

        let agent_loop = AgentLoop::new(
            store, lock_mgr, dispatcher, registry, llm_client, compactor, 10,
        );

        let controller =
            RuntimeController::new(agent_loop, temp_dir.path().to_str().unwrap().to_string());

        let result = controller.execute("user1", "nonexistent_agent", "Hi").await;
        assert!(result.is_err());
        match result {
            Err(RuntimeError::ConfigError(msg)) => {
                assert!(msg.contains("Config file not found"));
            }
            _ => panic!("Expected ConfigError"),
        }
    }

    #[tokio::test]
    async fn test_empty_user_id() {
        let temp_dir = TempDir::new().unwrap();
        let store = Arc::new(MockSessionStore::new());
        let lock_mgr = Arc::new(MockLockManager::new());
        let dispatcher = Arc::new(MockToolDispatcher);
        let registry = Arc::new(MockToolRegistry);
        let llm_client =
            LLMClientType::Standard(LLMClient::new("http://localhost:8000".to_string(), 0));
        let compactor = Compactor::new(10000, MockSummarizer);

        let agent_loop = AgentLoop::new(
            store, lock_mgr, dispatcher, registry, llm_client, compactor, 10,
        );

        let controller =
            RuntimeController::new(agent_loop, temp_dir.path().to_str().unwrap().to_string());

        let result = controller.execute("", "agent", "Hi").await;
        assert!(result.is_err());
        match result {
            Err(RuntimeError::SessionError(msg)) => {
                assert!(msg.contains("non-empty"));
            }
            _ => panic!("Expected SessionError"),
        }
    }

    #[tokio::test]
    async fn test_empty_agent_id() {
        let temp_dir = TempDir::new().unwrap();
        let store = Arc::new(MockSessionStore::new());
        let lock_mgr = Arc::new(MockLockManager::new());
        let dispatcher = Arc::new(MockToolDispatcher);
        let registry = Arc::new(MockToolRegistry);
        let llm_client =
            LLMClientType::Standard(LLMClient::new("http://localhost:8000".to_string(), 0));
        let compactor = Compactor::new(10000, MockSummarizer);

        let agent_loop = AgentLoop::new(
            store, lock_mgr, dispatcher, registry, llm_client, compactor, 10,
        );

        let controller =
            RuntimeController::new(agent_loop, temp_dir.path().to_str().unwrap().to_string());

        let result = controller.execute("user1", "", "Hi").await;
        assert!(result.is_err());
        match result {
            Err(RuntimeError::SessionError(msg)) => {
                assert!(msg.contains("non-empty"));
            }
            _ => panic!("Expected SessionError"),
        }
    }

    #[test]
    fn test_controller_creation() {
        let store = Arc::new(MockSessionStore::new());
        let lock_mgr = Arc::new(MockLockManager::new());
        let dispatcher = Arc::new(MockToolDispatcher);
        let registry = Arc::new(MockToolRegistry);
        let llm_client =
            LLMClientType::Standard(LLMClient::new("http://localhost:8000".to_string(), 0));
        let compactor = Compactor::new(10000, MockSummarizer);

        let agent_loop = AgentLoop::new(
            store, lock_mgr, dispatcher, registry, llm_client, compactor, 10,
        );

        let controller = RuntimeController::new(agent_loop, "/tmp/agents".to_string());
        assert_eq!(controller.config_dir, "/tmp/agents");
    }
}
