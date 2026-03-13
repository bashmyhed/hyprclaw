use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Execution error: {0}")]
    Execution(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: serde_json::Value,
    pub error: Option<String>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> serde_json::Value;

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolError>;
}
