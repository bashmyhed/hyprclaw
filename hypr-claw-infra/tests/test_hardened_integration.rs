use hypr_claw::infra::*;
use std::collections::HashMap;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn test_full_hardened_workflow() {
    let dir = tempdir().unwrap();

    // Initialize all hardened components
    let lock_manager = lock_manager::LockManager::new(Duration::from_secs(5));
    let permission_engine = permission_engine::PermissionEngine::new();
    let session_store = session_store::SessionStore::new(dir.path().join("sessions")).unwrap();
    let audit_logger =
        audit_logger_chained::AuditLogger::new(dir.path().join("audit.log")).unwrap();
    let rate_limiter = rate_limiter::RateLimiter::new(
        rate_limiter::RateLimitConfig::new(10, Duration::from_secs(60)),
        rate_limiter::RateLimitConfig::new(5, Duration::from_secs(60)),
        rate_limiter::RateLimitConfig::new(100, Duration::from_secs(60)),
    );

    let session_key = "test_session";
    let tool_name = "read_file";

    // 1. Check rate limit
    rate_limiter.check_all(session_key, tool_name).unwrap();

    // 2. Acquire lock
    let _lock = lock_manager.acquire(session_key).unwrap();

    // 3. Check permission
    let mut input = HashMap::new();
    input.insert("file".to_string(), serde_json::json!("/tmp/test.txt"));

    let perm_request = contracts::PermissionRequest {
        session_key: session_key.to_string(),
        tool_name: tool_name.to_string(),
        input: input.clone(),
        permission_level: contracts::PermissionLevel::SAFE,
    };

    let decision = permission_engine.check(&perm_request);
    assert_eq!(decision, contracts::PermissionDecision::ALLOW);

    // 4. Log to audit (with hash chain)
    let audit_entry = contracts::AuditEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        session: session_key.to_string(),
        tool: tool_name.to_string(),
        input: input.clone(),
        result: HashMap::new(),
        approval: decision,
    };
    audit_logger.log(&audit_entry).unwrap();

    // 5. Store session data
    let result = serde_json::json!({"status": "success"});
    session_store.append(session_key, &result).unwrap();

    // 6. Verify audit chain integrity
    audit_logger.verify_integrity().unwrap();

    // 7. Verify session data
    let messages = session_store.load(session_key).unwrap();
    assert_eq!(messages.len(), 1);
}

#[test]
fn test_hardened_concurrent_workflow() {
    use std::sync::Arc;
    use std::thread;

    let dir = tempdir().unwrap();

    let lock_manager = Arc::new(lock_manager::LockManager::new(Duration::from_secs(5)));
    let session_store =
        Arc::new(session_store::SessionStore::new(dir.path().join("sessions")).unwrap());
    let audit_logger =
        Arc::new(audit_logger_chained::AuditLogger::new(dir.path().join("audit.log")).unwrap());
    let rate_limiter = Arc::new(rate_limiter::RateLimiter::new(
        rate_limiter::RateLimitConfig::new(100, Duration::from_secs(60)),
        rate_limiter::RateLimitConfig::new(100, Duration::from_secs(60)),
        rate_limiter::RateLimitConfig::new(1000, Duration::from_secs(60)),
    ));

    let mut handles = vec![];

    for i in 0..10 {
        let lock_mgr = Arc::clone(&lock_manager);
        let store = Arc::clone(&session_store);
        let _logger = Arc::clone(&audit_logger);
        let limiter = Arc::clone(&rate_limiter);

        let handle = thread::spawn(move || {
            let session_key = format!("session_{}", i);

            // Rate limit check
            limiter.check_session(&session_key).unwrap();

            // Acquire lock for session
            let _lock = lock_mgr.acquire(&session_key).unwrap();

            // Store session
            let msg = serde_json::json!({"thread": i});
            store.append(&session_key, &msg).unwrap();
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Log audit entries sequentially (audit logger is already thread-safe)
    for i in 0..10 {
        let entry = contracts::AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session: format!("session_{}", i),
            tool: "test_tool".to_string(),
            input: HashMap::new(),
            result: HashMap::new(),
            approval: contracts::PermissionDecision::ALLOW,
        };
        audit_logger.log(&entry).unwrap();
    }

    // Verify audit chain
    audit_logger.verify_integrity().unwrap();
}

#[test]
fn test_hardened_integrity_validation() {
    let dir = tempdir().unwrap();

    // Create some data
    let session_store = session_store::SessionStore::new(dir.path().join("sessions")).unwrap();
    let audit_logger =
        audit_logger_chained::AuditLogger::new(dir.path().join("audit.log")).unwrap();
    let memory_store = memory_store::MemoryStore::new(dir.path().join("memory.db")).unwrap();

    session_store
        .append("test", &serde_json::json!({"test": true}))
        .unwrap();

    let entry = contracts::AuditEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        session: "test".to_string(),
        tool: "test".to_string(),
        input: HashMap::new(),
        result: HashMap::new(),
        approval: contracts::PermissionDecision::ALLOW,
    };
    audit_logger.log(&entry).unwrap();

    memory_store.save_memory("test_key", "test_value").unwrap();

    // Run integrity validation
    let result = integrity::IntegrityValidator::validate_all(
        &dir.path().join("audit.log"),
        &dir.path().join("sessions"),
        &dir.path().join("memory.db"),
    );

    assert!(result.is_ok());
}

#[test]
fn test_hardened_rate_limit_enforcement() {
    let _dir = tempdir().unwrap();

    let rate_limiter = rate_limiter::RateLimiter::new(
        rate_limiter::RateLimitConfig::new(3, Duration::from_secs(60)),
        rate_limiter::RateLimitConfig::new(100, Duration::from_secs(60)),
        rate_limiter::RateLimitConfig::new(1000, Duration::from_secs(60)),
    );

    let session_key = "limited_session";

    // First 3 should succeed
    assert!(rate_limiter.check_session(session_key).is_ok());
    assert!(rate_limiter.check_session(session_key).is_ok());
    assert!(rate_limiter.check_session(session_key).is_ok());

    // 4th should fail
    assert!(rate_limiter.check_session(session_key).is_err());
}

#[test]
fn test_hardened_crypto_security() {
    let dir = tempdir().unwrap();
    let key = [42u8; 32];

    let store = credential_store::CredentialStore::new(dir.path().join("creds"), &key).unwrap();

    // Store secret
    store.store_secret("api_key", "sk-secret123").unwrap();

    // Retrieve secret
    let retrieved = store.get_secret("api_key").unwrap();
    assert_eq!(retrieved, "sk-secret123");

    // Try with wrong key - should fail
    let wrong_key = [99u8; 32];
    let store2 =
        credential_store::CredentialStore::new(dir.path().join("creds"), &wrong_key).unwrap();
    let result = store2.get_secret("api_key");
    assert!(result.is_err());
}
