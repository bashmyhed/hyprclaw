use crate::{types::*, ContextCompactor};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::fs;

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Context not found: {0}")]
    NotFound(String),
}

pub struct ContextManager {
    base_path: PathBuf,
}

impl ContextManager {
    pub fn new<P: AsRef<Path>>(base_path: P) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
        }
    }

    pub async fn initialize(&self) -> Result<(), MemoryError> {
        fs::create_dir_all(&self.base_path).await?;
        tracing::info!("Context manager initialized at {:?}", self.base_path);
        Ok(())
    }

    pub async fn load(&self, session_id: &str) -> Result<ContextData, MemoryError> {
        let path = self.context_path(session_id);

        if !path.exists() {
            tracing::info!("Creating new context for session: {}", session_id);
            return Ok(ContextData {
                session_id: session_id.to_string(),
                ..Default::default()
            });
        }

        let content = fs::read_to_string(&path).await?;
        let context: ContextData = serde_json::from_str(&content)?;

        tracing::info!("Loaded context for session: {}", session_id);
        Ok(context)
    }

    pub async fn save(&self, context: &ContextData) -> Result<(), MemoryError> {
        let mut compacted_context = context.clone();
        ContextCompactor::compact(&mut compacted_context);
        let path = self.context_path(&compacted_context.session_id);

        // Atomic write: write to temp file, then rename
        let temp_path = path.with_extension("tmp");
        let content = serde_json::to_string_pretty(&compacted_context)?;

        fs::write(&temp_path, content).await?;
        fs::rename(&temp_path, &path).await?;

        tracing::debug!(
            "Saved context for session: {}",
            compacted_context.session_id
        );
        Ok(())
    }

    pub async fn delete(&self, session_id: &str) -> Result<(), MemoryError> {
        let path = self.context_path(session_id);
        if path.exists() {
            fs::remove_file(&path).await?;
            tracing::info!("Deleted context for session: {}", session_id);
        }
        Ok(())
    }

    pub async fn list_sessions(&self) -> Result<Vec<String>, MemoryError> {
        let mut sessions = Vec::new();
        let mut entries = fs::read_dir(&self.base_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".json") {
                    sessions.push(name.trim_end_matches(".json").to_string());
                }
            }
        }

        Ok(sessions)
    }

    fn context_path(&self, session_id: &str) -> PathBuf {
        self.base_path.join(format!("{}.json", session_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_context_lifecycle() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ContextManager::new(temp_dir.path());

        manager.initialize().await.unwrap();

        let mut context = manager.load("test_session").await.unwrap();
        context.facts.push("Test fact".to_string());

        manager.save(&context).await.unwrap();

        let loaded = manager.load("test_session").await.unwrap();
        assert_eq!(loaded.facts.len(), 1);
        assert_eq!(loaded.facts[0], "Test fact");

        manager.delete("test_session").await.unwrap();
    }
}
