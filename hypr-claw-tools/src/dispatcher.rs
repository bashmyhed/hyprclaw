use crate::error::ToolError;
use crate::execution_context::ExecutionContext;
use crate::registry::ToolRegistryImpl;
use crate::tools::ToolResult;
use crate::traits::{AuditLogger, PermissionDecision, PermissionEngine, PermissionRequest};
use serde_json::json;
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use tracing::{error, info, warn};

pub struct ToolDispatcherImpl {
    registry: Arc<ToolRegistryImpl>,
    permission: Arc<dyn PermissionEngine>,
    audit: Arc<dyn AuditLogger>,
    timeout_ms: u64,
}

impl ToolDispatcherImpl {
    pub fn new(
        registry: Arc<ToolRegistryImpl>,
        permission: Arc<dyn PermissionEngine>,
        audit: Arc<dyn AuditLogger>,
        timeout_ms: u64,
    ) -> Self {
        Self {
            registry,
            permission,
            audit,
            timeout_ms,
        }
    }

    pub async fn dispatch(
        &self,
        session_key: String,
        tool_name: String,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        info!(
            "Dispatching tool: {} for session: {}",
            tool_name, session_key
        );

        // 1. Lookup tool
        let tool = self
            .registry
            .get(&tool_name)
            .ok_or_else(|| ToolError::ValidationError(format!("Tool not found: {}", tool_name)))?;

        // 2. Validate input against schema
        let schema = tool.schema();
        if !self.validate_input(&input, &schema) {
            return Err(ToolError::ValidationError(
                "Input does not match schema".into(),
            ));
        }

        // 3. Build permission request
        let perm_request = PermissionRequest {
            session_key: session_key.clone(),
            tool_name: tool_name.clone(),
            input: input.clone(),
            permission_tier: tool.permission_tier(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        // 4. Check permission
        let permission_tier = perm_request.permission_tier;
        let decision = self.permission.check(perm_request).await;

        let result = match &decision {
            PermissionDecision::Deny(reason) => {
                warn!("Permission denied: {}", reason);
                Err(ToolError::PermissionDenied(reason.clone()))
            }
            PermissionDecision::RequireApproval(msg) => Ok(ToolResult {
                success: false,
                output: Some(json!({"approval_required": true, "message": msg})),
                error: Some("Approval required".into()),
            }),
            PermissionDecision::Allow => {
                let ctx = ExecutionContext::new(session_key.clone(), self.timeout_ms);
                self.execute_with_protection(tool, ctx, input.clone()).await
            }
        };

        // 10. Always audit (isolated from result)
        self.log_audit_isolated(
            &session_key,
            &tool_name,
            &input,
            &result,
            &decision,
            permission_tier,
        )
        .await;

        result
    }

    fn validate_input(&self, input: &serde_json::Value, _schema: &serde_json::Value) -> bool {
        // Validate JSON structure
        if input.is_null() {
            return false;
        }

        // Check for excessively large payloads
        if let Ok(serialized) = serde_json::to_string(input) {
            if serialized.len() > 1_000_000 {
                // 1MB limit
                return false;
            }
        }

        true
    }

    async fn execute_with_protection(
        &self,
        tool: Arc<dyn crate::tools::Tool>,
        ctx: ExecutionContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let timeout_ms = ctx.timeout_ms;

        // Execute with timeout and panic isolation
        let exec_future = async move { tool.execute(ctx, input).await };

        // Spawn task to isolate panics
        let handle = tokio::spawn(exec_future);

        match timeout(Duration::from_millis(timeout_ms), handle).await {
            Ok(Ok(result)) => result,
            Ok(Err(join_err)) => {
                if join_err.is_panic() {
                    error!("Tool execution panicked");
                    Err(ToolError::Internal)
                } else {
                    error!("Tool execution cancelled");
                    Err(ToolError::Internal)
                }
            }
            Err(_) => {
                warn!("Tool execution timed out after {}ms", timeout_ms);
                Err(ToolError::Timeout)
            }
        }
    }

    async fn log_audit_isolated(
        &self,
        session_key: &str,
        tool_name: &str,
        input: &serde_json::Value,
        result: &Result<ToolResult, ToolError>,
        decision: &PermissionDecision,
        permission_tier: crate::traits::PermissionTier,
    ) {
        // Audit logging must never fail the operation
        let log_entry = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "session": session_key,
            "tool": tool_name,
            "input": input,
            "approval_decision": match decision {
                PermissionDecision::Allow => "ALLOW",
                PermissionDecision::Deny(_) => "DENY",
                PermissionDecision::RequireApproval(_) => "REQUIRE_APPROVAL",
            },
            "permission_tier": format!("{permission_tier:?}"),
            "result": match result {
                Ok(r) => json!({"success": r.success, "output": &r.output, "error": &r.error}),
                Err(e) => json!({"error": e.to_string()}),
            }
        });

        // Fire and forget - don't await
        let audit = self.audit.clone();
        tokio::spawn(async move {
            let _ = audit.log(log_entry).await;
        });
    }
}
