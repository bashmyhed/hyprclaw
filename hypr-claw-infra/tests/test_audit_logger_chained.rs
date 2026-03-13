use hypr_claw::infra::audit_logger_chained::{AuditLogger, AuditLoggerError};
use hypr_claw::infra::contracts::{AuditEntry, PermissionDecision};
use std::collections::HashMap;
use tempfile::tempdir;

#[test]
fn test_valid_chain_verification() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("audit.log");
    let logger = AuditLogger::new(&log_path).unwrap();

    for i in 0..5 {
        let entry = AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session: format!("session_{}", i),
            tool: "test_tool".to_string(),
            input: HashMap::new(),
            result: HashMap::new(),
            approval: PermissionDecision::ALLOW,
        };
        logger.log(&entry).unwrap();
    }

    // Verify integrity
    logger.verify_integrity().unwrap();
}

#[test]
fn test_tampered_entry_detection() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("audit.log");
    let logger = AuditLogger::new(&log_path).unwrap();

    for i in 0..3 {
        let entry = AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session: format!("session_{}", i),
            tool: "test_tool".to_string(),
            input: HashMap::new(),
            result: HashMap::new(),
            approval: PermissionDecision::ALLOW,
        };
        logger.log(&entry).unwrap();
    }

    // Tamper with the file
    let content = std::fs::read_to_string(&log_path).unwrap();
    let tampered = content.replace("session_1", "session_X");
    std::fs::write(&log_path, tampered).unwrap();

    // Verification should fail
    let result = logger.verify_integrity();
    assert!(matches!(
        result,
        Err(AuditLoggerError::IntegrityViolation(_))
    ));
}

#[test]
fn test_deleted_entry_detection() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("audit.log");
    let logger = AuditLogger::new(&log_path).unwrap();

    for i in 0..5 {
        let entry = AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session: format!("session_{}", i),
            tool: "test_tool".to_string(),
            input: HashMap::new(),
            result: HashMap::new(),
            approval: PermissionDecision::ALLOW,
        };
        logger.log(&entry).unwrap();
    }

    // Delete middle entry
    let content = std::fs::read_to_string(&log_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    let modified = format!("{}\n{}\n{}\n{}\n", lines[0], lines[1], lines[3], lines[4]);
    std::fs::write(&log_path, modified).unwrap();

    // Verification should fail
    let result = logger.verify_integrity();
    assert!(matches!(
        result,
        Err(AuditLoggerError::IntegrityViolation(_))
    ));
}

#[test]
fn test_reordered_entry_detection() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("audit.log");
    let logger = AuditLogger::new(&log_path).unwrap();

    for i in 0..4 {
        let entry = AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session: format!("session_{}", i),
            tool: "test_tool".to_string(),
            input: HashMap::new(),
            result: HashMap::new(),
            approval: PermissionDecision::ALLOW,
        };
        logger.log(&entry).unwrap();
    }

    // Swap two entries
    let content = std::fs::read_to_string(&log_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    let modified = format!("{}\n{}\n{}\n{}\n", lines[0], lines[2], lines[1], lines[3]);
    std::fs::write(&log_path, modified).unwrap();

    // Verification should fail
    let result = logger.verify_integrity();
    assert!(matches!(
        result,
        Err(AuditLoggerError::IntegrityViolation(_))
    ));
}

#[test]
fn test_hash_mismatch_detection() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("audit.log");
    let logger = AuditLogger::new(&log_path).unwrap();

    let entry = AuditEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        session: "session_1".to_string(),
        tool: "test_tool".to_string(),
        input: HashMap::new(),
        result: HashMap::new(),
        approval: PermissionDecision::ALLOW,
    };
    logger.log(&entry).unwrap();

    // Modify entry_hash
    let content = std::fs::read_to_string(&log_path).unwrap();
    let tampered = content.replace("entry_hash", "entry_XXXX");
    std::fs::write(&log_path, tampered).unwrap();

    // Verification should fail
    let result = logger.verify_integrity();
    assert!(matches!(
        result,
        Err(AuditLoggerError::IntegrityViolation(_))
    ));
}

#[test]
fn test_startup_verification_failure() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("audit.log");

    // Create logger and add entries
    {
        let logger = AuditLogger::new(&log_path).unwrap();
        for i in 0..3 {
            let entry = AuditEntry {
                timestamp: chrono::Utc::now().to_rfc3339(),
                session: format!("session_{}", i),
                tool: "test_tool".to_string(),
                input: HashMap::new(),
                result: HashMap::new(),
                approval: PermissionDecision::ALLOW,
            };
            logger.log(&entry).unwrap();
        }
    }

    // Tamper with file
    let content = std::fs::read_to_string(&log_path).unwrap();
    let tampered = content.replace("session_1", "session_X");
    std::fs::write(&log_path, tampered).unwrap();

    // Creating new logger should fail on verification
    let result = AuditLogger::new(&log_path);
    assert!(matches!(
        result,
        Err(AuditLoggerError::IntegrityViolation(_))
    ));
}

#[test]
fn test_empty_log_initialization() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("audit.log");

    let logger = AuditLogger::new(&log_path).unwrap();
    logger.verify_integrity().unwrap();
}

#[test]
fn test_chain_continuity() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("audit.log");

    // Create logger and add entries
    {
        let logger = AuditLogger::new(&log_path).unwrap();
        for i in 0..3 {
            let entry = AuditEntry {
                timestamp: chrono::Utc::now().to_rfc3339(),
                session: format!("session_{}", i),
                tool: "test_tool".to_string(),
                input: HashMap::new(),
                result: HashMap::new(),
                approval: PermissionDecision::ALLOW,
            };
            logger.log(&entry).unwrap();
        }
    }

    // Reopen logger and add more entries
    {
        let logger = AuditLogger::new(&log_path).unwrap();
        for i in 3..6 {
            let entry = AuditEntry {
                timestamp: chrono::Utc::now().to_rfc3339(),
                session: format!("session_{}", i),
                tool: "test_tool".to_string(),
                input: HashMap::new(),
                result: HashMap::new(),
                approval: PermissionDecision::ALLOW,
            };
            logger.log(&entry).unwrap();
        }
    }

    // Verify entire chain
    let logger = AuditLogger::new(&log_path).unwrap();
    logger.verify_integrity().unwrap();
}
