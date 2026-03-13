use crate::error::ToolError;
use crate::execution_context::ExecutionContext;
use crate::sandbox::PathGuard;
use crate::tools::base::{Tool, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tokio::fs;

#[derive(Deserialize)]
struct FileReadInput {
    path: String,
}

#[derive(Clone)]
pub struct FileReadTool {
    sandbox_root: String,
}

impl FileReadTool {
    pub fn new(sandbox_root: &str) -> Result<Self, ToolError> {
        // Validate sandbox root exists
        let _ = PathGuard::new(sandbox_root)?;
        Ok(Self {
            sandbox_root: sandbox_root.to_string(),
        })
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &'static str {
        "file.read"
    }

    fn description(&self) -> &'static str {
        "Reads file contents"
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
        let input: FileReadInput =
            serde_json::from_value(input).map_err(|e| ToolError::ValidationError(e.to_string()))?;

        let path_guard = PathGuard::new(&self.sandbox_root)?;
        let validated_path = path_guard.validate(&input.path)?;

        let content = fs::read_to_string(&validated_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        String::from_utf8(content.as_bytes().to_vec())
            .map_err(|_| ToolError::ValidationError("Invalid UTF-8".into()))?;

        Ok(ToolResult {
            success: true,
            output: Some(json!({"content": content})),
            error: None,
        })
    }
}
