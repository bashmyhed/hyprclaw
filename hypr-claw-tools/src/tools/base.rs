use crate::error::ToolError;
use crate::execution_context::ExecutionContext;
use crate::traits::PermissionTier;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolResult {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub output: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub for_llm: Option<Value>,
    #[serde(default)]
    pub for_user: Option<String>,
    #[serde(default)]
    pub silent: bool,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub is_async: bool,
    #[serde(default)]
    pub media: Vec<String>,
    #[serde(default)]
    pub recovery_hint: Option<String>,
    #[serde(default)]
    pub error_kind: Option<String>,
}

impl ToolResult {
    pub fn success(output: Value) -> Self {
        Self {
            success: true,
            output: Some(output),
            ..Self::default()
        }
    }

    pub fn success_with_user(output: Value, for_user: impl Into<String>) -> Self {
        Self {
            success: true,
            output: Some(output.clone()),
            for_llm: Some(output),
            for_user: Some(for_user.into()),
            ..Self::default()
        }
    }

    pub fn approval_required(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            success: false,
            output: Some(json!({"approval_required": true, "message": message})),
            error: Some("Approval required".to_string()),
            for_user: Some(message),
            is_error: true,
            error_kind: Some("approval_required".to_string()),
            ..Self::default()
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            success: false,
            error: Some(message.clone()),
            for_llm: Some(json!({"error": message})),
            is_error: true,
            ..Self::default()
        }
    }

    pub fn llm_payload(&self) -> Value {
        self.for_llm
            .clone()
            .or_else(|| self.output.clone())
            .unwrap_or_else(|| json!({}))
    }

    pub fn user_message(&self) -> Option<&str> {
        if self.silent {
            None
        } else {
            self.for_user.as_deref()
        }
    }

    pub fn effective_error_message(&self) -> Option<String> {
        self.error.clone().or_else(|| {
            self.is_error
                .then(|| "Tool execution failed".to_string())
        })
    }

    pub fn is_effective_success(&self) -> bool {
        self.success && !self.is_error && self.error.is_none()
    }
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
