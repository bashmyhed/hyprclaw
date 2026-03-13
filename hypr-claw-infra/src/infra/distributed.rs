use serde_json::Value;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DistributedError {
    #[error("Lock timeout: {0}")]
    Timeout(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Serialization error: {0}")]
    Serialization(String),
}

/// Distributed lock manager trait for horizontal scaling
///
/// Implementations:
/// - Redis: Use SETNX with TTL for distributed locking
/// - Database: Use row-level locks with SELECT FOR UPDATE
/// - Etcd: Use lease-based locking
pub trait DistributedLockManager: Send + Sync {
    /// Acquire a distributed lock with timeout
    fn acquire(
        &self,
        session_key: &str,
        timeout: Duration,
    ) -> Result<Box<dyn DistributedLock>, DistributedError>;

    /// Try to acquire lock without blocking
    fn try_acquire(
        &self,
        session_key: &str,
    ) -> Result<Option<Box<dyn DistributedLock>>, DistributedError>;

    /// Check if a lock is currently held
    fn is_locked(&self, session_key: &str) -> Result<bool, DistributedError>;
}

/// Distributed lock handle with RAII semantics
pub trait DistributedLock: Send {
    /// Get the session key this lock protects
    fn session_key(&self) -> &str;

    /// Extend the lock TTL (for long-running operations)
    fn extend(&mut self, duration: Duration) -> Result<(), DistributedError>;

    /// Explicitly release the lock (also released on drop)
    fn release(self: Box<Self>) -> Result<(), DistributedError>;
}

/// Distributed session storage trait for horizontal scaling
///
/// Implementations:
/// - Redis: Use Redis Streams or sorted sets
/// - S3: Use object storage with session key prefix
/// - Database: Use JSONB column with session_key index
pub trait DistributedSessionStore: Send + Sync {
    /// Load entire session history
    fn load(&self, session_key: &str) -> Result<Vec<Value>, DistributedError>;

    /// Append message to session
    fn append(&self, session_key: &str, message: &Value) -> Result<(), DistributedError>;

    /// Save entire session (atomic replace)
    fn save(&self, session_key: &str, messages: &[Value]) -> Result<(), DistributedError>;

    /// Delete session
    fn delete(&self, session_key: &str) -> Result<(), DistributedError>;

    /// List all session keys (for admin/cleanup)
    fn list_sessions(&self) -> Result<Vec<String>, DistributedError>;
}

/// Sharding strategy for horizontal scaling
pub trait ShardingStrategy: Send + Sync {
    /// Determine which shard a session belongs to
    fn get_shard(&self, session_key: &str) -> usize;

    /// Get total number of shards
    fn shard_count(&self) -> usize;
}

/// Consistent hash-based sharding
pub struct ConsistentHashSharding {
    shard_count: usize,
}

impl ConsistentHashSharding {
    pub fn new(shard_count: usize) -> Self {
        Self { shard_count }
    }
}

impl ShardingStrategy for ConsistentHashSharding {
    fn get_shard(&self, session_key: &str) -> usize {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        session_key.hash(&mut hasher);
        (hasher.finish() as usize) % self.shard_count
    }

    fn shard_count(&self) -> usize {
        self.shard_count
    }
}
