use crate::error::ToolError;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB

pub struct PathGuard {
    sandbox_root: PathBuf,
}

impl PathGuard {
    pub fn new(sandbox_root: impl AsRef<Path>) -> Result<Self, ToolError> {
        let root = fs::canonicalize(sandbox_root)
            .map_err(|e| ToolError::SandboxViolation(format!("Invalid sandbox root: {}", e)))?;
        Ok(Self { sandbox_root: root })
    }

    pub fn validate(&self, path: impl AsRef<Path>) -> Result<PathBuf, ToolError> {
        let path = path.as_ref();

        // Reject absolute paths immediately
        if path.is_absolute() {
            return Err(ToolError::SandboxViolation(
                "Absolute paths not allowed".into(),
            ));
        }

        // Check for path traversal before canonicalization
        let path_str = path.to_string_lossy();
        if path_str.contains("..") {
            return Err(ToolError::SandboxViolation(
                "Path traversal detected".into(),
            ));
        }

        let full_path = self.sandbox_root.join(path);

        // Canonicalize to resolve symlinks
        let canonical = fs::canonicalize(&full_path).map_err(|_| {
            ToolError::SandboxViolation("Path does not exist or is inaccessible".into())
        })?;

        // Verify boundary after canonicalization (TOCTOU protection)
        if !canonical.starts_with(&self.sandbox_root) {
            return Err(ToolError::SandboxViolation("Path escapes sandbox".into()));
        }

        // Check file size
        if let Ok(metadata) = fs::metadata(&canonical) {
            if metadata.is_file() && metadata.len() > MAX_FILE_SIZE {
                return Err(ToolError::SandboxViolation("File too large".into()));
            }
        }

        Ok(canonical)
    }

    pub fn validate_new(&self, path: impl AsRef<Path>) -> Result<PathBuf, ToolError> {
        let path = path.as_ref();

        // Reject absolute paths
        if path.is_absolute() {
            return Err(ToolError::SandboxViolation(
                "Absolute paths not allowed".into(),
            ));
        }

        // Check for traversal
        let path_str = path.to_string_lossy();
        if path_str.contains("..") {
            return Err(ToolError::SandboxViolation(
                "Path traversal detected".into(),
            ));
        }

        let full_path = self.sandbox_root.join(path);

        // Validate parent directory
        if let Some(parent) = full_path.parent() {
            if parent.exists() {
                let canonical_parent = fs::canonicalize(parent)
                    .map_err(|_| ToolError::SandboxViolation("Invalid parent directory".into()))?;

                if !canonical_parent.starts_with(&self.sandbox_root) {
                    return Err(ToolError::SandboxViolation("Path escapes sandbox".into()));
                }
            }
        }

        // Ensure the path itself doesn't escape via symlink
        // Check each component of the full path, starting from the sandbox root
        let mut current = self.sandbox_root.clone();

        // Get the relative path from sandbox_root to full_path
        if let Ok(relative) = full_path.strip_prefix(&self.sandbox_root) {
            for component in relative.components() {
                current.push(component);
                if current.exists() {
                    if let Ok(canonical) = fs::canonicalize(&current) {
                        if !canonical.starts_with(&self.sandbox_root) {
                            return Err(ToolError::SandboxViolation(
                                "Symlink escapes sandbox".into(),
                            ));
                        }
                    }
                }
            }
        }

        Ok(full_path)
    }
}
