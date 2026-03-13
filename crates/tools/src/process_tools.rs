use crate::traits::{Tool, ToolError, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct ProcessListTool;

#[async_trait]
impl Tool for ProcessListTool {
    fn name(&self) -> &str {
        "process_list"
    }

    fn description(&self) -> &str {
        "List running processes (read-only)"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of processes to return",
                    "default": 10
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        let limit = args["limit"].as_u64().unwrap_or(10) as usize;

        let output = tokio::process::Command::new("ps")
            .args(&["aux", "--sort=-pcpu"])
            .output()
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        if !output.status.success() {
            return Err(ToolError::Execution("ps command failed".to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let processes: Vec<&str> = stdout.lines().take(limit + 1).collect();

        Ok(ToolResult {
            success: true,
            output: json!({ "processes": processes }),
            error: None,
        })
    }
}
