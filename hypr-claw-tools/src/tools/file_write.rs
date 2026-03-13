use crate::error::ToolError;
use crate::execution_context::ExecutionContext;
use crate::sandbox::PathGuard;
use crate::tools::base::{Tool, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tokio::fs;
use tokio::io::AsyncWriteExt;

#[derive(Deserialize)]
struct FileWriteInput {
    path: String,
    content: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Clone)]
pub struct FileWriteTool {
    sandbox_root: String,
}

impl FileWriteTool {
    pub fn new(sandbox_root: &str) -> Result<Self, ToolError> {
        let _ = PathGuard::new(sandbox_root)?;
        Ok(Self {
            sandbox_root: sandbox_root.to_string(),
        })
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &'static str {
        "file.write"
    }

    fn description(&self) -> &'static str {
        "Writes content to file"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"},
                "overwrite": {"type": "boolean"}
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(
        &self,
        _ctx: ExecutionContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let input: FileWriteInput =
            serde_json::from_value(input).map_err(|e| ToolError::ValidationError(e.to_string()))?;

        let path_guard = PathGuard::new(&self.sandbox_root)?;
        let validated_path = path_guard.validate_new(&input.path)?;

        if validated_path.exists() && !input.overwrite {
            return Err(ToolError::ValidationError(
                "File exists and overwrite=false".into(),
            ));
        }

        if let Some(parent) = validated_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        }

        let temp_path = validated_path.with_extension("tmp");
        let mut file = fs::File::create(&temp_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        file.write_all(input.content.as_bytes())
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        file.sync_all()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        fs::rename(&temp_path, &validated_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(ToolResult {
            success: true,
            output: Some(json!({"written": true})),
            error: None,
        })
    }
}
