use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SessionStoreError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid session key")]
    InvalidKey,
}

pub struct SessionStore {
    base_path: PathBuf,
}

impl SessionStore {
    pub fn new<P: AsRef<Path>>(base_path: P) -> Result<Self, SessionStoreError> {
        let base_path = base_path.as_ref().to_path_buf();
        fs::create_dir_all(&base_path)?;
        Ok(Self { base_path })
    }

    fn session_path(&self, session_key: &str) -> Result<PathBuf, SessionStoreError> {
        if session_key.is_empty() || session_key.contains("..") || session_key.contains('/') {
            return Err(SessionStoreError::InvalidKey);
        }
        Ok(self.base_path.join(format!("{}.jsonl", session_key)))
    }

    pub fn load(&self, session_key: &str) -> Result<Vec<Value>, SessionStoreError> {
        let path = self.session_path(session_key)?;

        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&path)?;
        let reader = BufReader::new(file);
        let mut messages = Vec::new();

        for line in reader.lines() {
            match line {
                Ok(l) if !l.trim().is_empty() => {
                    match serde_json::from_str(&l) {
                        Ok(msg) => messages.push(msg),
                        Err(_) => continue, // Skip corrupted lines
                    }
                }
                _ => continue,
            }
        }

        Ok(messages)
    }

    pub fn append(&self, session_key: &str, message: &Value) -> Result<(), SessionStoreError> {
        let path = self.session_path(session_key)?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;

        let json = serde_json::to_string(message)?;
        writeln!(file, "{}", json)?;
        file.sync_all()?;

        Ok(())
    }

    pub fn save(&self, session_key: &str, messages: &[Value]) -> Result<(), SessionStoreError> {
        let path = self.session_path(session_key)?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let temp_path = path.with_extension("tmp");

        {
            let mut file = File::create(&temp_path)?;
            for msg in messages {
                let json = serde_json::to_string(msg)?;
                writeln!(file, "{}", json)?;
            }
            file.sync_all()?;
        }

        fs::rename(&temp_path, &path)?;

        Ok(())
    }
}
