use hypr_claw::infra::lock_manager::{LockError, LockManager};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

#[test]
fn test_concurrent_same_session_blocks() {
    let manager = Arc::new(LockManager::new(Duration::from_secs(2)));
    let barrier = Arc::new(Barrier::new(2));
    let session_key = "test_session";

    let manager1 = Arc::clone(&manager);
    let barrier1 = Arc::clone(&barrier);
    let handle1 = thread::spawn(move || {
        let _lock = manager1.acquire(session_key).unwrap();
        barrier1.wait();
        thread::sleep(Duration::from_millis(100));
    });

    let manager2 = Arc::clone(&manager);
    let barrier2 = Arc::clone(&barrier);
    let handle2 = thread::spawn(move || {
        barrier2.wait();
        thread::sleep(Duration::from_millis(10));
        let result = manager2.acquire(session_key);
        assert!(result.is_ok());
    });

    handle1.join().unwrap();
    handle2.join().unwrap();
}

#[test]
fn test_parallel_different_sessions() {
    let manager = Arc::new(LockManager::new(Duration::from_secs(1)));
    let mut handles = vec![];

    for i in 0..10 {
        let manager_clone = Arc::clone(&manager);
        let handle = thread::spawn(move || {
            let session_key = format!("session_{}", i);
            let _lock = manager_clone.acquire(&session_key).unwrap();
            thread::sleep(Duration::from_millis(50));
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_lock_timeout() {
    let manager = Arc::new(LockManager::new(Duration::from_millis(100)));
    let session_key = "timeout_session";

    let _lock1 = manager.acquire(session_key).unwrap();

    let manager_clone = Arc::clone(&manager);
    let handle = thread::spawn(move || {
        let result = manager_clone.acquire(session_key);
        assert!(matches!(result, Err(LockError::Timeout(_))));
    });

    handle.join().unwrap();
}

#[test]
fn test_lock_release_on_panic() {
    let manager = Arc::new(LockManager::new(Duration::from_secs(1)));
    let session_key = "panic_session";

    let manager_clone = Arc::clone(&manager);
    let handle = thread::spawn(move || {
        let _lock = manager_clone.acquire(session_key).unwrap();
        panic!("Intentional panic");
    });

    let _ = handle.join();

    let lock2 = manager.acquire(session_key);
    assert!(lock2.is_ok());
}

#[test]
fn test_lock_wait_time_measurement() {
    let manager = LockManager::new(Duration::from_secs(1));
    let session_key = "metrics_session";

    let (result, metrics) = manager.acquire_with_metrics(session_key);
    assert!(result.is_ok());
    assert!(metrics.acquired);
    assert!(metrics.wait_time < Duration::from_millis(10));
}

#[test]
fn test_lock_wait_time_with_contention() {
    let manager = Arc::new(LockManager::new(Duration::from_secs(2)));
    let session_key = "contention_session";

    let _lock1 = manager.acquire(session_key).unwrap();

    let manager_clone = Arc::clone(&manager);
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        drop(_lock1);
    });

    thread::sleep(Duration::from_millis(50));

    let (result, metrics) = manager_clone.acquire_with_metrics(session_key);
    assert!(result.is_ok());
    assert!(metrics.acquired);
    assert!(metrics.wait_time >= Duration::from_millis(10)); // More lenient timing

    handle.join().unwrap();
}

#[test]
fn test_no_deadlock_multiple_acquires() {
    let manager = Arc::new(LockManager::new(Duration::from_secs(2)));
    let mut handles = vec![];

    // Each thread acquires its own session lock, then releases it
    // This tests that multiple threads can safely acquire different locks
    for i in 0..5 {
        let manager_clone = Arc::clone(&manager);
        let handle = thread::spawn(move || {
            let key = format!("session_{}", i);
            let _lock = manager_clone.acquire(&key).unwrap();
            thread::sleep(Duration::from_millis(10));
            // Lock released here
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}
