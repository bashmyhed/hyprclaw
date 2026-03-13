//! Abstract interfaces for runtime dependencies.

use crate::types::Message;
use async_trait::async_trait;
use thiserror::Error;

/// Runtime errors.
#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("Session error: {0}")]
    SessionError(String),

    #[error("Lock error: {0}")]
    LockError(String),

    #[error("Tool error: {0}")]
    ToolError(String),

    #[error("LLM error: {0}")]
    LLMError(String),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

/// Session persistence interface.
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Load message history for a session.
    async fn load(&self, session_key: &str) -> Result<Vec<Message>, RuntimeError>;

    /// Save message history for a session.
    async fn save(&self, session_key: &str, messages: &[Message]) -> Result<(), RuntimeError>;
}

/// Session locking interface.
#[async_trait]
pub trait LockManager: Send + Sync {
    /// Acquire lock for a session.
    async fn acquire(&self, session_key: &str) -> Result<(), RuntimeError>;

    /// Release lock for a session.
    async fn release(&self, session_key: &str);
}

/// Tool execution interface.
#[async_trait]
pub trait ToolDispatcher: Send + Sync {
    /// Execute a tool and return result.
    async fn execute(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        session_key: &str,
    ) -> Result<serde_json::Value, RuntimeError>;
}

/// Tool discovery interface.
pub trait ToolRegistry: Send + Sync {
    /// Get list of active tool names for an agent.
    fn get_active_tools(&self, agent_id: &str) -> Vec<String>;

    /// Get full tool schemas for an agent in OpenAI function format.
    fn get_tool_schemas(&self, agent_id: &str) -> Vec<serde_json::Value>;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    // Mock implementations for testing
    struct MockSessionStore {
        storage: Arc<Mutex<std::collections::HashMap<String, Vec<Message>>>>,
    }

    impl MockSessionStore {
        fn new() -> Self {
            Self {
                storage: Arc::new(Mutex::new(std::collections::HashMap::new())),
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
        locks: Arc<Mutex<std::collections::HashSet<String>>>,
    }

    impl MockLockManager {
        fn new() -> Self {
            Self {
                locks: Arc::new(Mutex::new(std::collections::HashSet::new())),
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

    struct MockToolRegistry {
        tools: Arc<Mutex<std::collections::HashMap<String, Vec<String>>>>,
    }

    impl MockToolRegistry {
        fn new() -> Self {
            Self {
                tools: Arc::new(Mutex::new(std::collections::HashMap::new())),
            }
        }
    }

    impl ToolRegistry for MockToolRegistry {
        fn get_active_tools(&self, agent_id: &str) -> Vec<String> {
            let tools = self.tools.lock().unwrap();
            tools.get(agent_id).cloned().unwrap_or_default()
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

    #[tokio::test]
    async fn test_session_store_load_empty() {
        let store = MockSessionStore::new();
        let messages = store.load("test:user1").await.unwrap();
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn test_session_store_save_and_load() {
        let store = MockSessionStore::new();
        let messages = vec![Message::new(crate::types::Role::User, json!("Hello"))];

        store.save("test:user1", &messages).await.unwrap();
        let loaded = store.load("test:user1").await.unwrap();

        assert_eq!(loaded.len(), 1);
    }

    #[tokio::test]
    async fn test_lock_manager_acquire() {
        let manager = MockLockManager::new();
        let result = manager.acquire("test:user1").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_lock_manager_release() {
        let manager = MockLockManager::new();
        manager.acquire("test:user1").await.unwrap();
        manager.release("test:user1").await;

        // Should be able to acquire again
        let result = manager.acquire("test:user1").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_lock_manager_double_acquire() {
        let manager = MockLockManager::new();
        manager.acquire("test:user1").await.unwrap();
        let result = manager.acquire("test:user1").await;

        assert!(result.is_err());
        match result {
            Err(RuntimeError::LockError(msg)) => assert!(msg.contains("already held")),
            _ => panic!("Expected LockError"),
        }
    }

    #[tokio::test]
    async fn test_tool_dispatcher_execute() {
        let dispatcher = MockToolDispatcher;
        let result = dispatcher
            .execute("search", &json!({"query": "test"}), "test:user1")
            .await
            .unwrap();

        assert_eq!(result["status"], "success");
        assert_eq!(result["tool"], "search");
    }

    #[test]
    fn test_tool_registry_empty() {
        let registry = MockToolRegistry::new();
        let tools = registry.get_active_tools("agent1");
        assert!(tools.is_empty());
    }

    #[test]
    fn test_runtime_error_display() {
        let err = RuntimeError::SessionError("test error".to_string());
        assert_eq!(err.to_string(), "Session error: test error");

        let err = RuntimeError::LockError("lock failed".to_string());
        assert_eq!(err.to_string(), "Lock error: lock failed");
    }
}
