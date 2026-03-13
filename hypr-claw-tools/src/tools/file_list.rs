use crate::error::ToolError;
use crate::execution_context::ExecutionContext;
use crate::sandbox::PathGuard;
use crate::tools::base::{Tool, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tokio::fs;

const MAX_ENTRIES: usize = 1000;

#[derive(Deserialize)]
struct FileListInput {
    path: String,
}

#[derive(Clone)]
pub struct FileListTool {
    sandbox_root: String,
}

impl FileListTool {
    pub fn new(sandbox_root: &str) -> Result<Self, ToolError> {
        let _ = PathGuard::new(sandbox_root)?;
        Ok(Self {
            sandbox_root: sandbox_root.to_string(),
        })
    }
}

#[async_trait]
impl Tool for FileListTool {
    fn name(&self) -> &'static str {
        "file.list"
    }

    fn description(&self) -> &'static str {
        "Lists directory contents"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            },
            "required": ["path"]
        })
    }

    async fn execute(
        &self,
        _ctx: ExecutionContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let input: FileListInput =
            serde_json::from_value(input).map_err(|e| ToolError::ValidationError(e.to_string()))?;

        let path_guard = PathGuard::new(&self.sandbox_root)?;
        let validated_path = path_guard.validate(&input.path)?;

        let mut entries = Vec::new();
        let mut dir = fs::read_dir(&validated_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
        {
            if entries.len() >= MAX_ENTRIES {
                break;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }

            entries.push(name);
        }

        Ok(ToolResult {
            success: true,
            output: Some(json!({"entries": entries})),
            error: None,
        })
    }
}
