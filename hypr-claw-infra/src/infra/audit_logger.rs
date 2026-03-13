use crate::infra::contracts::AuditEntry;
use parking_lot::Mutex;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuditLoggerError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct AuditLogger {
    #[allow(dead_code)]
    log_path: PathBuf,
    file: Mutex<File>,
}

impl AuditLogger {
    pub fn new<P: AsRef<Path>>(log_path: P) -> Result<Self, AuditLoggerError> {
        let log_path = log_path.as_ref().to_path_buf();

        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        Ok(Self {
            log_path,
            file: Mutex::new(file),
        })
    }

    pub fn log(&self, entry: &AuditEntry) -> Result<(), AuditLoggerError> {
        let json = serde_json::to_string(entry)?;
        let mut file = self.file.lock();
        writeln!(file, "{}", json)?;
        file.sync_all()?;
        Ok(())
    }
}
