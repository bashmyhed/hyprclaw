use crate::traits::{Tool, ToolError, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echo back the input message"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "Message to echo"
                }
            },
            "required": ["message"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        let message = args["message"]
            .as_str()
            .ok_or_else(|| ToolError::Validation("Missing 'message' field".to_string()))?;

        Ok(ToolResult {
            success: true,
            output: json!({ "message": message }),
            error: None,
        })
    }
}

pub struct SystemInfoTool;

#[async_trait]
impl Tool for SystemInfoTool {
    fn name(&self) -> &str {
        "system_info"
    }

    fn description(&self) -> &str {
        "Get current system information"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _args: serde_json::Value) -> Result<ToolResult, ToolError> {
        let info = json!({
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "family": std::env::consts::FAMILY,
        });

        Ok(ToolResult {
            success: true,
            output: info,
            error: None,
        })
    }
}
