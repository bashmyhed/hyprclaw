use crate::traits::{Tool, ToolError, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;

pub struct FileReadTool {
    sandbox_path: PathBuf,
}

impl FileReadTool {
    pub fn new(sandbox_path: PathBuf) -> Self {
        Self { sandbox_path }
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read a file from the sandbox directory"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to file (relative to sandbox)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| ToolError::Validation("Missing 'path' field".to_string()))?;

        let full_path = self.sandbox_path.join(path);

        // Security: ensure path is within sandbox
        if !full_path.starts_with(&self.sandbox_path) {
            return Err(ToolError::PermissionDenied(
                "Path outside sandbox".to_string(),
            ));
        }

        let content = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        Ok(ToolResult {
            success: true,
            output: json!({ "content": content }),
            error: None,
        })
    }
}

pub struct FileWriteTool {
    sandbox_path: PathBuf,
}

impl FileWriteTool {
    pub fn new(sandbox_path: PathBuf) -> Self {
        Self { sandbox_path }
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Write content to a file in the sandbox directory"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to file (relative to sandbox)"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| ToolError::Validation("Missing 'path' field".to_string()))?;

        let content = args["content"]
            .as_str()
            .ok_or_else(|| ToolError::Validation("Missing 'content' field".to_string()))?;

        let full_path = self.sandbox_path.join(path);

        // Security: ensure path is within sandbox
        if !full_path.starts_with(&self.sandbox_path) {
            return Err(ToolError::PermissionDenied(
                "Path outside sandbox".to_string(),
            ));
        }

        tokio::fs::write(&full_path, content)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        Ok(ToolResult {
            success: true,
            output: json!({ "path": path, "bytes_written": content.len() }),
            error: None,
        })
    }
}

pub struct FileListTool {
    sandbox_path: PathBuf,
}

impl FileListTool {
    pub fn new(sandbox_path: PathBuf) -> Self {
        Self { sandbox_path }
    }
}

#[async_trait]
impl Tool for FileListTool {
    fn name(&self) -> &str {
        "file_list"
    }

    fn description(&self) -> &str {
        "List files in the sandbox directory"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path (relative to sandbox, default: root)",
                    "default": "."
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        let path = args["path"].as_str().unwrap_or(".");
        let full_path = self.sandbox_path.join(path);

        // Security: ensure path is within sandbox
        if !full_path.starts_with(&self.sandbox_path) {
            return Err(ToolError::PermissionDenied(
                "Path outside sandbox".to_string(),
            ));
        }

        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&full_path)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?
        {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
            entries.push(json!({ "name": name, "is_dir": is_dir }));
        }

        Ok(ToolResult {
            success: true,
            output: json!({ "entries": entries }),
            error: None,
        })
    }
}
