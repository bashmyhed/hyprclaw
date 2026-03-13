use crate::error::ToolError;
use crate::execution_context::ExecutionContext;
use crate::traits::PermissionTier;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn schema(&self) -> serde_json::Value;
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Write
    }

    async fn execute(
        &self,
        ctx: ExecutionContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError>;
}
