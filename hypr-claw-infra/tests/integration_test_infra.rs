use hypr_claw::infra::audit_logger::AuditLogger;
use hypr_claw::infra::contracts::{
    AuditEntry, PermissionDecision, PermissionLevel, PermissionRequest,
};
use hypr_claw::infra::lock_manager::LockManager;
use hypr_claw::infra::permission_engine::PermissionEngine;
use hypr_claw::infra::session_store::SessionStore;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn test_full_session_workflow() {
    let temp = TempDir::new().unwrap();

    // Initialize infrastructure
    let session_store = Arc::new(SessionStore::new(temp.path().join("sessions")).unwrap());
    let lock_manager = Arc::new(LockManager::new(Duration::from_secs(5)));
    let permission_engine = Arc::new(PermissionEngine::new());
    let audit_logger = Arc::new(AuditLogger::new(temp.path().join("audit.log")).unwrap());

    let session_key = "user123_session";

    // Acquire lock
    let _lock = lock_manager.acquire(session_key).unwrap();

    // Check permission
    let mut input = HashMap::new();
    input.insert("file".to_string(), json!("/tmp/test.txt"));

    let perm_request = PermissionRequest {
        session_key: session_key.to_string(),
        tool_name: "read_file".to_string(),
        input: input.clone(),
        permission_level: PermissionLevel::SAFE,
    };

    let decision = permission_engine.check(&perm_request);
    assert_eq!(decision, PermissionDecision::ALLOW);

    // Log to session
    let message = json!({"role": "user", "content": "read file"});
    session_store.append(session_key, &message).unwrap();

    // Audit log
    let audit_entry = AuditEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        session: session_key.to_string(),
        tool: "read_file".to_string(),
        input: input.clone(),
        result: HashMap::new(),
        approval: decision,
    };
    audit_logger.log(&audit_entry).unwrap();

    // Verify session
    let messages = session_store.load(session_key).unwrap();
    assert_eq!(messages.len(), 1);
}

#[test]
fn test_concurrent_sessions() {
    let temp = TempDir::new().unwrap();

    let session_store = Arc::new(SessionStore::new(temp.path().join("sessions")).unwrap());
    let lock_manager = Arc::new(LockManager::new(Duration::from_secs(5)));
    let permission_engine = Arc::new(PermissionEngine::new());

    let mut handles = vec![];

    for i in 0..5 {
        let store = Arc::clone(&session_store);
        let locks = Arc::clone(&lock_manager);
        let perms = Arc::clone(&permission_engine);

        let handle = thread::spawn(move || {
            let session_key = format!("session{}", i);

            let _lock = locks.acquire(&session_key).unwrap();

            let mut input = HashMap::new();
            input.insert("data".to_string(), json!(format!("data{}", i)));

            let perm_request = PermissionRequest {
                session_key: session_key.clone(),
                tool_name: "process".to_string(),
                input,
                permission_level: PermissionLevel::SAFE,
            };

            let decision = perms.check(&perm_request);
            assert_eq!(decision, PermissionDecision::ALLOW);

            let message = json!({"id": i});
            store.append(&session_key, &message).unwrap();
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    for i in 0..5 {
        let messages = session_store.load(&format!("session{}", i)).unwrap();
        assert_eq!(messages.len(), 1);
    }
}

#[test]
fn test_permission_blocks_dangerous_operation() {
    let temp = TempDir::new().unwrap();

    let permission_engine = PermissionEngine::new();
    let audit_logger = AuditLogger::new(temp.path().join("audit.log")).unwrap();

    let mut input = HashMap::new();
    input.insert("cmd".to_string(), json!("sudo rm -rf /"));

    let perm_request = PermissionRequest {
        session_key: "session1".to_string(),
        tool_name: "shell".to_string(),
        input: input.clone(),
        permission_level: PermissionLevel::SAFE,
    };

    let decision = permission_engine.check(&perm_request);
    assert_eq!(decision, PermissionDecision::DENY);

    let audit_entry = AuditEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        session: "session1".to_string(),
        tool: "shell".to_string(),
        input,
        result: HashMap::new(),
        approval: decision,
    };

    audit_logger.log(&audit_entry).unwrap();
}

#[test]
fn test_lock_prevents_concurrent_session_access() {
    let lock_manager = Arc::new(LockManager::new(Duration::from_millis(100)));

    let lock1 = lock_manager.acquire("session1").unwrap();

    let manager_clone = Arc::clone(&lock_manager);
    let handle = thread::spawn(move || {
        let result = manager_clone.acquire("session1");
        assert!(result.is_err()); // Should timeout
    });

    thread::sleep(Duration::from_millis(150));
    drop(lock1);

    handle.join().unwrap();
}
