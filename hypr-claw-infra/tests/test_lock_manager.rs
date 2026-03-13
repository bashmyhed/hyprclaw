use hypr_claw::infra::lock_manager::LockManager;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn test_acquire_and_release() {
    let manager = LockManager::new(Duration::from_secs(5));

    let lock1 = manager.acquire("session1").unwrap();
    drop(lock1);

    let lock2 = manager.acquire("session1").unwrap();
    drop(lock2);
}

#[test]
fn test_concurrent_same_session_blocks() {
    let manager = Arc::new(LockManager::new(Duration::from_millis(100)));
    let manager_clone = Arc::clone(&manager);

    let lock = manager.acquire("session1").unwrap();

    let handle = thread::spawn(move || {
        let result = manager_clone.acquire("session1");
        assert!(result.is_err()); // Should timeout
    });

    thread::sleep(Duration::from_millis(150));
    drop(lock);

    handle.join().unwrap();
}

#[test]
fn test_different_sessions_independent() {
    let manager = Arc::new(LockManager::new(Duration::from_secs(5)));

    let lock1 = manager.acquire("session1").unwrap();
    let lock2 = manager.acquire("session2").unwrap();

    drop(lock1);
    drop(lock2);
}

#[test]
fn test_timeout_behavior() {
    let manager = Arc::new(LockManager::new(Duration::from_millis(50)));

    let _lock = manager.acquire("session1").unwrap();

    let start = std::time::Instant::now();
    let result = manager.acquire("session1");
    let elapsed = start.elapsed();

    assert!(result.is_err());
    assert!(elapsed >= Duration::from_millis(50));
    assert!(elapsed < Duration::from_millis(200));
}

#[test]
fn test_lock_released_on_drop() {
    let manager = LockManager::new(Duration::from_secs(5));

    {
        let _lock = manager.acquire("session1").unwrap();
    } // Lock dropped here

    let lock2 = manager.acquire("session1").unwrap();
    drop(lock2);
}

#[test]
fn test_parallel_different_sessions() {
    let manager = Arc::new(LockManager::new(Duration::from_secs(5)));
    let mut handles = vec![];

    for i in 0..10 {
        let manager_clone = Arc::clone(&manager);
        let handle = thread::spawn(move || {
            let session_key = format!("session{}", i);
            let _lock = manager_clone.acquire(&session_key).unwrap();
            thread::sleep(Duration::from_millis(10));
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_lock_released_in_panic() {
    let manager = Arc::new(LockManager::new(Duration::from_secs(5)));
    let manager_clone = Arc::clone(&manager);

    let handle = thread::spawn(move || {
        let _lock = manager_clone.acquire("session1").unwrap();
        panic!("Simulated panic");
    });

    let _ = handle.join();

    // Lock should be released despite panic
    let lock = manager.acquire("session1").unwrap();
    drop(lock);
}
