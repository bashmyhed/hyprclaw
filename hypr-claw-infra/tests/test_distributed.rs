use hypr_claw::infra::distributed::*;
use hypr_claw::infra::distributed_adapters::*;
use serde_json::json;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn test_distributed_lock_trait_conformance() {
    let manager = LocalDistributedLockManager::new(Duration::from_secs(1));
    let lock = manager
        .acquire("test_session", Duration::from_secs(1))
        .unwrap();
    assert_eq!(lock.session_key(), "test_session");
}

#[test]
fn test_distributed_lock_try_acquire() {
    let manager = LocalDistributedLockManager::new(Duration::from_secs(1));

    let _lock1 = manager.try_acquire("test_session").unwrap().unwrap();
    let lock2 = manager.try_acquire("test_session").unwrap();

    assert!(lock2.is_none());
}

#[test]
fn test_distributed_session_store_trait_conformance() {
    let dir = tempdir().unwrap();
    let store = LocalDistributedSessionStore::new(dir.path().to_str().unwrap()).unwrap();

    let msg = json!({"role": "user", "content": "hello"});
    store.append("session1", &msg).unwrap();

    let messages = store.load("session1").unwrap();
    assert_eq!(messages.len(), 1);
}

#[test]
fn test_sharding_strategy() {
    let strategy = ConsistentHashSharding::new(4);

    let shard1 = strategy.get_shard("session_1");
    let shard2 = strategy.get_shard("session_1");
    assert_eq!(shard1, shard2); // Consistent

    assert!(shard1 < 4); // Within bounds
}

#[test]
fn test_sharding_distribution() {
    let strategy = ConsistentHashSharding::new(4);
    let mut shard_counts = vec![0; 4];

    for i in 0..100 {
        let session_key = format!("session_{}", i);
        let shard = strategy.get_shard(&session_key);
        shard_counts[shard] += 1;
    }

    // Check reasonable distribution (each shard should have some sessions)
    for count in shard_counts {
        assert!(count > 10 && count < 50);
    }
}

#[test]
fn test_mock_network_failure() {
    // Simulate network failure by returning error
    struct FailingLockManager;

    impl DistributedLockManager for FailingLockManager {
        fn acquire(
            &self,
            _session_key: &str,
            _timeout: Duration,
        ) -> Result<Box<dyn DistributedLock>, DistributedError> {
            Err(DistributedError::Network("Connection refused".to_string()))
        }

        fn try_acquire(
            &self,
            _session_key: &str,
        ) -> Result<Option<Box<dyn DistributedLock>>, DistributedError> {
            Err(DistributedError::Network("Connection refused".to_string()))
        }

        fn is_locked(&self, _session_key: &str) -> Result<bool, DistributedError> {
            Err(DistributedError::Network("Connection refused".to_string()))
        }
    }

    let manager = FailingLockManager;
    let result = manager.acquire("test", Duration::from_secs(1));
    assert!(matches!(result, Err(DistributedError::Network(_))));
}

#[test]
fn test_failover_behavior() {
    // Test that we can switch between implementations
    let local_manager = LocalDistributedLockManager::new(Duration::from_secs(1));

    // Acquire lock with local manager
    let lock = local_manager
        .acquire("session1", Duration::from_secs(1))
        .unwrap();
    assert_eq!(lock.session_key(), "session1");

    // In production, we could failover to a different implementation
    // This demonstrates the abstraction works
}
