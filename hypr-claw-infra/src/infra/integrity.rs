use rusqlite::Connection;
use std::path::Path;
use thiserror::Error;

use crate::infra::audit_logger_chained::AuditLogger;
use crate::infra::session_store::SessionStore;

#[derive(Error, Debug)]
pub enum IntegrityError {
    #[error("Audit chain corrupted: {0}")]
    AuditChain(String),

    #[error("Session data corrupted: {0}")]
    SessionData(String),

    #[error("Database corrupted: {0}")]
    Database(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct IntegrityValidator;

impl IntegrityValidator {
    /// Perform full integrity check on startup
    pub fn validate_all(
        audit_log_path: &Path,
        session_store_path: &Path,
        memory_db_path: &Path,
    ) -> Result<(), IntegrityError> {
        // 1. Verify audit chain
        Self::verify_audit_chain(audit_log_path)?;

        // 2. Validate session JSON
        Self::verify_session_store(session_store_path)?;

        // 3. SQLite integrity check
        Self::verify_database(memory_db_path)?;

        Ok(())
    }

    fn verify_audit_chain(log_path: &Path) -> Result<(), IntegrityError> {
        if !log_path.exists() {
            return Ok(()); // No audit log yet
        }

        let logger =
            AuditLogger::new(log_path).map_err(|e| IntegrityError::AuditChain(e.to_string()))?;

        logger
            .verify_integrity()
            .map_err(|e| IntegrityError::AuditChain(e.to_string()))?;

        Ok(())
    }

    fn verify_session_store(store_path: &Path) -> Result<(), IntegrityError> {
        if !store_path.exists() {
            return Ok(()); // No sessions yet
        }

        let store = SessionStore::new(store_path)
            .map_err(|e| IntegrityError::SessionData(e.to_string()))?;

        // List all session files
        let entries = std::fs::read_dir(store_path)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                let session_key = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| IntegrityError::SessionData("Invalid filename".to_string()))?;

                // Try to load session
                store.load(session_key).map_err(|e| {
                    IntegrityError::SessionData(format!("Session '{}': {}", session_key, e))
                })?;
            }
        }

        Ok(())
    }

    fn verify_database(db_path: &Path) -> Result<(), IntegrityError> {
        if !db_path.exists() {
            return Ok(()); // No database yet
        }

        let conn =
            Connection::open(db_path).map_err(|e| IntegrityError::Database(e.to_string()))?;

        // Run SQLite integrity check
        let result: String = conn
            .pragma_query_value(None, "integrity_check", |row| row.get(0))
            .map_err(|e| IntegrityError::Database(e.to_string()))?;

        if result != "ok" {
            return Err(IntegrityError::Database(format!(
                "Integrity check failed: {}",
                result
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_validate_empty_system() {
        let dir = tempdir().unwrap();
        let audit_path = dir.path().join("audit.log");
        let session_path = dir.path().join("sessions");
        let db_path = dir.path().join("memory.db");

        let result = IntegrityValidator::validate_all(&audit_path, &session_path, &db_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_corrupted_audit_chain() {
        let dir = tempdir().unwrap();
        let audit_path = dir.path().join("audit.log");

        // Create valid audit log
        use crate::infra::contracts::{AuditEntry, PermissionDecision};
        use std::collections::HashMap;

        {
            let logger = AuditLogger::new(&audit_path).unwrap();
            let entry = AuditEntry {
                timestamp: chrono::Utc::now().to_rfc3339(),
                session: "test".to_string(),
                tool: "test".to_string(),
                input: HashMap::new(),
                result: HashMap::new(),
                approval: PermissionDecision::ALLOW,
            };
            logger.log(&entry).unwrap();
        }

        // Corrupt it
        let content = fs::read_to_string(&audit_path).unwrap();
        fs::write(&audit_path, content.replace("test", "XXXX")).unwrap();

        let result = IntegrityValidator::validate_all(
            &audit_path,
            &dir.path().join("sessions"),
            &dir.path().join("memory.db"),
        );

        assert!(matches!(result, Err(IntegrityError::AuditChain(_))));
    }

    #[test]
    fn test_corrupted_session_json() {
        // Session store is designed to be resilient to corrupted lines
        // This test verifies that the validator doesn't fail on recoverable corruption
        let dir = tempdir().unwrap();
        let session_path = dir.path().join("sessions");
        fs::create_dir_all(&session_path).unwrap();

        // Create file with some valid and some invalid JSON
        let session_file = session_path.join("test_session.jsonl");
        fs::write(
            &session_file,
            "{\"valid\": true}\n{invalid}\n{\"also_valid\": true}\n",
        )
        .unwrap();

        // Should succeed - session store skips corrupted lines
        let result = IntegrityValidator::validate_all(
            &dir.path().join("audit.log"),
            &session_path,
            &dir.path().join("memory.db"),
        );

        assert!(result.is_ok());
    }
}
