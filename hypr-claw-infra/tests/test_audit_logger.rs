use hypr_claw::infra::audit_logger::AuditLogger;
use hypr_claw::infra::contracts::{AuditEntry, PermissionDecision};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::thread;
use tempfile::TempDir;

fn create_entry(session: &str, tool: &str) -> AuditEntry {
    AuditEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        session: session.to_string(),
        tool: tool.to_string(),
        input: HashMap::new(),
        result: HashMap::new(),
        approval: PermissionDecision::ALLOW,
    }
}

#[test]
fn test_log_single_entry() {
    let temp = TempDir::new().unwrap();
    let log_path = temp.path().join("audit.log");
    let logger = AuditLogger::new(&log_path).unwrap();

    let entry = create_entry("session1", "tool1");
    logger.log(&entry).unwrap();

    let file = File::open(&log_path).unwrap();
    let reader = BufReader::new(file);
    let lines: Vec<_> = reader.lines().collect();

    assert_eq!(lines.len(), 1);
}

#[test]
fn test_log_multiple_entries() {
    let temp = TempDir::new().unwrap();
    let log_path = temp.path().join("audit.log");
    let logger = AuditLogger::new(&log_path).unwrap();

    for i in 0..10 {
        let entry = create_entry(&format!("session{}", i), "tool");
        logger.log(&entry).unwrap();
    }

    let file = File::open(&log_path).unwrap();
    let reader = BufReader::new(file);
    let lines: Vec<_> = reader.lines().collect();

    assert_eq!(lines.len(), 10);
}

#[test]
fn test_append_only() {
    let temp = TempDir::new().unwrap();
    let log_path = temp.path().join("audit.log");

    {
        let logger = AuditLogger::new(&log_path).unwrap();
        logger.log(&create_entry("session1", "tool1")).unwrap();
    }

    {
        let logger = AuditLogger::new(&log_path).unwrap();
        logger.log(&create_entry("session2", "tool2")).unwrap();
    }

    let file = File::open(&log_path).unwrap();
    let reader = BufReader::new(file);
    let lines: Vec<_> = reader.lines().collect();

    assert_eq!(lines.len(), 2);
}

#[test]
fn test_concurrent_logging() {
    let temp = TempDir::new().unwrap();
    let log_path = temp.path().join("audit.log");
    let logger = Arc::new(AuditLogger::new(&log_path).unwrap());

    let mut handles = vec![];

    for i in 0..10 {
        let logger_clone = Arc::clone(&logger);
        let handle = thread::spawn(move || {
            let entry = create_entry(&format!("session{}", i), "tool");
            logger_clone.log(&entry).unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let file = File::open(&log_path).unwrap();
    let reader = BufReader::new(file);
    let lines: Vec<_> = reader.lines().collect();

    assert_eq!(lines.len(), 10);
}

#[test]
fn test_json_format() {
    let temp = TempDir::new().unwrap();
    let log_path = temp.path().join("audit.log");
    let logger = AuditLogger::new(&log_path).unwrap();

    let entry = create_entry("session1", "tool1");
    logger.log(&entry).unwrap();

    let file = File::open(&log_path).unwrap();
    let reader = BufReader::new(file);
    let line = reader.lines().next().unwrap().unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(parsed["session"], "session1");
    assert_eq!(parsed["tool"], "tool1");
}

#[test]
fn test_entry_with_data() {
    let temp = TempDir::new().unwrap();
    let log_path = temp.path().join("audit.log");
    let logger = AuditLogger::new(&log_path).unwrap();

    let mut entry = create_entry("session1", "tool1");
    entry
        .input
        .insert("param".to_string(), serde_json::json!("value"));
    entry
        .result
        .insert("status".to_string(), serde_json::json!("success"));

    logger.log(&entry).unwrap();

    let file = File::open(&log_path).unwrap();
    let reader = BufReader::new(file);
    let line = reader.lines().next().unwrap().unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(parsed["input"]["param"], "value");
    assert_eq!(parsed["result"]["status"], "success");
}

#[test]
fn test_creates_parent_directory() {
    let temp = TempDir::new().unwrap();
    let log_path = temp.path().join("logs").join("nested").join("audit.log");

    let logger = AuditLogger::new(&log_path).unwrap();
    logger.log(&create_entry("session1", "tool1")).unwrap();

    assert!(log_path.exists());
}
