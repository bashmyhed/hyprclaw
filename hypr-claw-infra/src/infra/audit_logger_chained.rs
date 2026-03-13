use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::infra::contracts::AuditEntry;

#[derive(Error, Debug)]
pub enum AuditLoggerError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Chain integrity violation: {0}")]
    IntegrityViolation(String),
}

#[derive(Serialize, Deserialize, Clone)]
struct ChainedAuditEntry {
    entry_hash: String,
    prev_hash: String,
    #[serde(flatten)]
    entry: AuditEntry,
}

pub struct AuditLogger {
    log_path: PathBuf,
    file: Mutex<File>,
    last_hash: Mutex<String>,
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

        // Verify existing chain and get last hash
        let last_hash = Self::verify_and_get_last_hash(&log_path)?;

        Ok(Self {
            log_path,
            file: Mutex::new(file),
            last_hash: Mutex::new(last_hash),
        })
    }

    pub fn log(&self, entry: &AuditEntry) -> Result<(), AuditLoggerError> {
        let prev_hash = self.last_hash.lock().clone();

        // Compute entry hash
        let entry_json = serde_json::to_string(entry)?;
        let mut hasher = Sha256::new();
        hasher.update(&prev_hash);
        hasher.update(&entry_json);
        let entry_hash = format!("{:x}", hasher.finalize());

        let chained_entry = ChainedAuditEntry {
            entry_hash: entry_hash.clone(),
            prev_hash,
            entry: entry.clone(),
        };

        let json = serde_json::to_string(&chained_entry)?;
        let mut file = self.file.lock();
        writeln!(file, "{}", json)?;
        file.sync_all()?;

        // Update last hash
        *self.last_hash.lock() = entry_hash;

        Ok(())
    }

    pub fn verify_integrity(&self) -> Result<(), AuditLoggerError> {
        Self::verify_and_get_last_hash(&self.log_path)?;
        Ok(())
    }

    fn verify_and_get_last_hash(log_path: &Path) -> Result<String, AuditLoggerError> {
        if !log_path.exists() {
            return Ok("genesis".to_string());
        }

        let file = File::open(log_path)?;
        let reader = BufReader::new(file);

        let mut prev_hash = "genesis".to_string();
        let mut line_num = 0;

        for line in reader.lines() {
            line_num += 1;
            let line = line?;

            if line.trim().is_empty() {
                continue;
            }

            let chained_entry: ChainedAuditEntry = serde_json::from_str(&line).map_err(|e| {
                AuditLoggerError::IntegrityViolation(format!(
                    "Line {}: Invalid JSON: {}",
                    line_num, e
                ))
            })?;

            // Verify prev_hash matches
            if chained_entry.prev_hash != prev_hash {
                return Err(AuditLoggerError::IntegrityViolation(format!(
                    "Line {}: Hash chain broken. Expected prev_hash '{}', got '{}'",
                    line_num, prev_hash, chained_entry.prev_hash
                )));
            }

            // Recompute entry hash
            let entry_json = serde_json::to_string(&chained_entry.entry)?;
            let mut hasher = Sha256::new();
            hasher.update(&prev_hash);
            hasher.update(&entry_json);
            let computed_hash = format!("{:x}", hasher.finalize());

            if computed_hash != chained_entry.entry_hash {
                return Err(AuditLoggerError::IntegrityViolation(format!(
                    "Line {}: Hash mismatch. Expected '{}', got '{}'",
                    line_num, computed_hash, chained_entry.entry_hash
                )));
            }

            prev_hash = chained_entry.entry_hash;
        }

        Ok(prev_hash)
    }
}
