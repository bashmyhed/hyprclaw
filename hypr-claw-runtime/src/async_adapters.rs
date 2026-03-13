use crate::interfaces::{
    LockManager as LockManagerTrait, RuntimeError, SessionStore as SessionStoreTrait,
};
use crate::types::Message;
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

/// Async wrapper for sync SessionStore
pub struct AsyncSessionStore {
    inner: Arc<hypr_claw::infra::session_store::SessionStore>,
}

impl AsyncSessionStore {
    pub fn new(inner: Arc<hypr_claw::infra::session_store::SessionStore>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl SessionStoreTrait for AsyncSessionStore {
    async fn load(&self, session_key: &str) -> Result<Vec<Message>, RuntimeError> {
        let inner = self.inner.clone();
        let key = session_key.to_string();

        tokio::task::spawn_blocking(move || {
            inner
                .load(&key)
                .map(|values| {
                    values
                        .into_iter()
                        .filter_map(|v| serde_json::from_value(v).ok())
                        .collect()
                })
                .map_err(|e| RuntimeError::SessionError(e.to_string()))
        })
        .await
        .map_err(|e| RuntimeError::SessionError(e.to_string()))?
    }

    async fn save(&self, session_key: &str, messages: &[Message]) -> Result<(), RuntimeError> {
        let inner = self.inner.clone();
        let key = session_key.to_string();
        let msgs: Vec<serde_json::Value> = messages
            .iter()
            .filter_map(|m| serde_json::to_value(m).ok())
            .collect();

        tokio::task::spawn_blocking(move || {
            inner
                .save(&key, &msgs)
                .map_err(|e| RuntimeError::SessionError(e.to_string()))
        })
        .await
        .map_err(|e| RuntimeError::SessionError(e.to_string()))?
    }
}

/// Async wrapper for sync LockManager
pub struct AsyncLockManager {
    inner: Arc<hypr_claw::infra::lock_manager::LockManager>,
    locks: Arc<Mutex<HashMap<String, hypr_claw::infra::lock_manager::SessionLock>>>,
}

impl AsyncLockManager {
    pub fn new(inner: Arc<hypr_claw::infra::lock_manager::LockManager>) -> Self {
        Self {
            inner,
            locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl LockManagerTrait for AsyncLockManager {
    async fn acquire(&self, session_key: &str) -> Result<(), RuntimeError> {
        let inner = self.inner.clone();
        let key = session_key.to_string();

        let lock = tokio::task::spawn_blocking(move || {
            inner
                .acquire(&key)
                .map_err(|e| RuntimeError::LockError(e.to_string()))
        })
        .await
        .map_err(|e| RuntimeError::LockError(e.to_string()))??;

        self.locks.lock().insert(session_key.to_string(), lock);
        Ok(())
    }

    async fn release(&self, session_key: &str) {
        self.locks.lock().remove(session_key);
    }
}
