use crate::infra::distributed::*;
use crate::infra::lock_manager::{
    LockManager as LocalLockManager, SessionLock as LocalSessionLock,
};
use crate::infra::session_store::SessionStore as LocalSessionStore;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// Local implementation of DistributedLockManager (single-node)
pub struct LocalDistributedLockManager {
    inner: Arc<LocalLockManager>,
}

impl LocalDistributedLockManager {
    pub fn new(timeout: Duration) -> Self {
        Self {
            inner: Arc::new(LocalLockManager::new(timeout)),
        }
    }
}

impl DistributedLockManager for LocalDistributedLockManager {
    fn acquire(
        &self,
        session_key: &str,
        _timeout: Duration,
    ) -> Result<Box<dyn DistributedLock>, DistributedError> {
        let lock = self
            .inner
            .acquire(session_key)
            .map_err(|e| DistributedError::Timeout(e.to_string()))?;
        Ok(Box::new(LocalDistributedLock { inner: lock }))
    }

    fn try_acquire(
        &self,
        session_key: &str,
    ) -> Result<Option<Box<dyn DistributedLock>>, DistributedError> {
        // Try with zero timeout
        match self.inner.acquire(session_key) {
            Ok(lock) => Ok(Some(Box::new(LocalDistributedLock { inner: lock }))),
            Err(_) => Ok(None),
        }
    }

    fn is_locked(&self, _session_key: &str) -> Result<bool, DistributedError> {
        // Local implementation doesn't expose lock state query
        Ok(false)
    }
}

struct LocalDistributedLock {
    inner: LocalSessionLock,
}

impl DistributedLock for LocalDistributedLock {
    fn session_key(&self) -> &str {
        self.inner.session_key()
    }

    fn extend(&mut self, _duration: Duration) -> Result<(), DistributedError> {
        // Local locks don't need extension
        Ok(())
    }

    fn release(self: Box<Self>) -> Result<(), DistributedError> {
        // Drop handles release
        Ok(())
    }
}

/// Local implementation of DistributedSessionStore (single-node)
pub struct LocalDistributedSessionStore {
    inner: Arc<LocalSessionStore>,
}

impl LocalDistributedSessionStore {
    pub fn new(base_path: &str) -> Result<Self, DistributedError> {
        let store = LocalSessionStore::new(base_path)
            .map_err(|e| DistributedError::Storage(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(store),
        })
    }
}

impl DistributedSessionStore for LocalDistributedSessionStore {
    fn load(&self, session_key: &str) -> Result<Vec<Value>, DistributedError> {
        self.inner
            .load(session_key)
            .map_err(|e| DistributedError::Storage(e.to_string()))
    }

    fn append(&self, session_key: &str, message: &Value) -> Result<(), DistributedError> {
        self.inner
            .append(session_key, message)
            .map_err(|e| DistributedError::Storage(e.to_string()))
    }

    fn save(&self, session_key: &str, messages: &[Value]) -> Result<(), DistributedError> {
        self.inner
            .save(session_key, messages)
            .map_err(|e| DistributedError::Storage(e.to_string()))
    }

    fn delete(&self, _session_key: &str) -> Result<(), DistributedError> {
        // Not implemented in local store yet
        Err(DistributedError::Storage(
            "delete not implemented".to_string(),
        ))
    }

    fn list_sessions(&self) -> Result<Vec<String>, DistributedError> {
        // Not implemented in local store yet
        Err(DistributedError::Storage(
            "list not implemented".to_string(),
        ))
    }
}
