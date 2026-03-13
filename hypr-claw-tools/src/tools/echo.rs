use crate::error::ToolError;
use crate::execution_context::ExecutionContext;
use crate::tools::base::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn description(&self) -> &'static str {
        "Echoes input back"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "message": {"type": "string"}
            },
            "required": ["message"]
        })
    }

    async fn execute(
        &self,
        _ctx: ExecutionContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult {
            success: true,
            output: Some(input),
            error: None,
        })
    }
}
