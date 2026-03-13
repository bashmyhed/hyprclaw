use crate::error::ToolError;
use crate::execution_context::ExecutionContext;
use crate::registry::ToolRegistryImpl;
use crate::tools::{Tool, ToolResult};
use crate::traits::{AuditLogger, PermissionDecision, PermissionEngine, PermissionRequest};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use tracing::{error, warn};

pub struct ToolDispatchRequest {
    pub session_key: String,
    pub tool_name: String,
    pub input: Value,
}

pub struct ToolDispatchContext {
    pub request: ToolDispatchRequest,
    pub tool: Option<Arc<dyn Tool>>,
    pub permission_tier: Option<crate::traits::PermissionTier>,
    pub permission_decision: Option<PermissionDecision>,
}

impl ToolDispatchContext {
    pub fn new(request: ToolDispatchRequest) -> Self {
        Self {
            request,
            tool: None,
            permission_tier: None,
            permission_decision: None,
        }
    }
}

pub struct VisibilityStage<'a> {
    registry: &'a Arc<ToolRegistryImpl>,
}

impl<'a> VisibilityStage<'a> {
    pub fn new(registry: &'a Arc<ToolRegistryImpl>) -> Self {
        Self { registry }
    }

    pub fn run(&self, ctx: &mut ToolDispatchContext) -> Result<(), ToolError> {
        let tool = self.registry.get(&ctx.request.tool_name).ok_or_else(|| {
            ToolError::ValidationError(format!("Tool not found: {}", ctx.request.tool_name))
        })?;
        ctx.permission_tier = Some(tool.permission_tier());
        ctx.tool = Some(tool);
        Ok(())
    }
}

pub struct InputValidationStage;

impl InputValidationStage {
    pub fn run(&self, ctx: &ToolDispatchContext) -> Result<(), ToolError> {
        if ctx.request.input.is_null() {
            return Err(ToolError::ValidationError(
                "Input does not match schema".into(),
            ));
        }

        if let Ok(serialized) = serde_json::to_string(&ctx.request.input) {
            if serialized.len() > 1_000_000 {
                return Err(ToolError::ValidationError(
                    "Input does not match schema".into(),
                ));
            }
        }

        Ok(())
    }
}

pub struct SafetyPolicyStage;

impl SafetyPolicyStage {
    pub fn run(&self, ctx: &ToolDispatchContext) -> Result<(), ToolError> {
        if ctx.tool.is_none() {
            return Err(ToolError::Internal);
        }
        Ok(())
    }
}

pub struct PermissionStage<'a> {
    permission: &'a Arc<dyn PermissionEngine>,
}

impl<'a> PermissionStage<'a> {
    pub fn new(permission: &'a Arc<dyn PermissionEngine>) -> Self {
        Self { permission }
    }

    pub async fn run(&self, ctx: &mut ToolDispatchContext) -> Result<(), ToolError> {
        let permission_tier = ctx.permission_tier.ok_or(ToolError::Internal)?;
        let request = PermissionRequest {
            session_key: ctx.request.session_key.clone(),
            tool_name: ctx.request.tool_name.clone(),
            input: ctx.request.input.clone(),
            permission_tier,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        ctx.permission_decision = Some(self.permission.check(request).await);
        Ok(())
    }
}

pub struct ExecutionStage {
    timeout_ms: u64,
}

impl ExecutionStage {
    pub fn new(timeout_ms: u64) -> Self {
        Self { timeout_ms }
    }

    pub async fn run(&self, ctx: &ToolDispatchContext) -> Result<ToolResult, ToolError> {
        match ctx
            .permission_decision
            .as_ref()
            .ok_or(ToolError::Internal)?
        {
            PermissionDecision::Deny(reason) => Err(ToolError::PermissionDenied(reason.clone())),
            PermissionDecision::RequireApproval(msg) => Ok(ToolResult::approval_required(msg)),
            PermissionDecision::Allow => {
                let tool = ctx.tool.clone().ok_or(ToolError::Internal)?;
                let exec_ctx =
                    ExecutionContext::new(ctx.request.session_key.clone(), self.timeout_ms);
                let input = ctx.request.input.clone();
                let timeout_ms = exec_ctx.timeout_ms;
                let exec_future = async move { tool.execute(exec_ctx, input).await };
                let handle = tokio::spawn(exec_future);

                match timeout(Duration::from_millis(timeout_ms), handle).await {
                    Ok(Ok(result)) => result,
                    Ok(Err(join_err)) => {
                        if join_err.is_panic() {
                            error!("Tool execution panicked");
                        } else {
                            error!("Tool execution cancelled");
                        }
                        Err(ToolError::Internal)
                    }
                    Err(_) => {
                        warn!("Tool execution timed out after {}ms", timeout_ms);
                        Err(ToolError::Timeout)
                    }
                }
            }
        }
    }
}

pub struct ResultNormalizationStage;

impl ResultNormalizationStage {
    pub fn run(&self, result: ToolResult) -> ToolResult {
        let mut result = result;

        if result.is_error && result.error.is_none() {
            result.error = Some("Tool execution failed".to_string());
        }
        if result.is_effective_success() && result.for_llm.is_none() {
            result.for_llm = result.output.clone();
        }

        result
    }
}

pub struct RecoveryClassificationStage;

impl RecoveryClassificationStage {
    pub fn run(&self, result: ToolResult) -> ToolResult {
        let mut result = result;

        if result.error_kind.is_none() {
            result.error_kind = result
                .error
                .as_ref()
                .map(|_| "execution_failed".to_string());
        }

        if result.recovery_hint.is_none() && !result.is_effective_success() {
            result.recovery_hint = Some("choose_alternative_tool_or_retry".to_string());
        }

        result
    }
}

pub struct AuditStage<'a> {
    audit: &'a Arc<dyn AuditLogger>,
}

impl<'a> AuditStage<'a> {
    pub fn new(audit: &'a Arc<dyn AuditLogger>) -> Self {
        Self { audit }
    }

    pub async fn run(&self, ctx: &ToolDispatchContext, result: &Result<ToolResult, ToolError>) {
        let permission_tier = ctx.permission_tier;
        let decision = ctx.permission_decision.as_ref();
        let log_entry = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "session": ctx.request.session_key,
            "tool": ctx.request.tool_name,
            "input": ctx.request.input,
            "approval_decision": match decision {
                Some(PermissionDecision::Allow) => "ALLOW",
                Some(PermissionDecision::Deny(_)) => "DENY",
                Some(PermissionDecision::RequireApproval(_)) => "REQUIRE_APPROVAL",
                None => "UNKNOWN",
            },
            "permission_tier": permission_tier.map(|tier| format!("{tier:?}")).unwrap_or_else(|| "UNKNOWN".to_string()),
            "result": match result {
                Ok(r) => json!({
                    "success": r.success,
                    "output": &r.output,
                    "error": &r.error,
                    "for_llm": &r.for_llm,
                    "for_user": &r.for_user,
                    "silent": r.silent,
                    "is_error": r.is_error,
                    "is_async": r.is_async,
                    "media": &r.media,
                    "recovery_hint": &r.recovery_hint,
                    "error_kind": &r.error_kind
                }),
                Err(e) => json!({"error": e.to_string()}),
            }
        });

        let audit = self.audit.clone();
        tokio::spawn(async move {
            let _ = audit.log(log_entry).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::EchoTool;
    use async_trait::async_trait;

    struct MockPermissionEngine;

    #[async_trait]
    impl PermissionEngine for MockPermissionEngine {
        async fn check(&self, _request: PermissionRequest) -> PermissionDecision {
            PermissionDecision::Allow
        }
    }

    #[test]
    fn validation_stage_rejects_null_input() {
        let ctx = ToolDispatchContext::new(ToolDispatchRequest {
            session_key: "s".into(),
            tool_name: "echo".into(),
            input: Value::Null,
        });
        let stage = InputValidationStage;
        let result = stage.run(&ctx);
        assert!(matches!(result, Err(ToolError::ValidationError(_))));
    }

    #[tokio::test]
    async fn visibility_permission_execution_flow_succeeds() {
        let mut registry = ToolRegistryImpl::new();
        registry.register(Arc::new(EchoTool));
        let registry = Arc::new(registry);
        let permission: Arc<dyn PermissionEngine> = Arc::new(MockPermissionEngine);

        let mut ctx = ToolDispatchContext::new(ToolDispatchRequest {
            session_key: "s".into(),
            tool_name: "echo".into(),
            input: json!({"message": "x"}),
        });

        VisibilityStage::new(&registry).run(&mut ctx).unwrap();
        InputValidationStage.run(&ctx).unwrap();
        SafetyPolicyStage.run(&ctx).unwrap();
        PermissionStage::new(&permission)
            .run(&mut ctx)
            .await
            .unwrap();
        let result = ExecutionStage::new(1000).run(&ctx).await.unwrap();
        let result = ResultNormalizationStage.run(result);

        assert!(result.is_effective_success());
        assert_eq!(result.llm_payload(), json!({"message": "x"}));
    }

    #[test]
    fn recovery_classification_sets_defaults() {
        let stage = RecoveryClassificationStage;
        let result = stage.run(ToolResult::failure("boom"));
        assert_eq!(result.error_kind.as_deref(), Some("execution_failed"));
        assert_eq!(
            result.recovery_hint.as_deref(),
            Some("choose_alternative_tool_or_retry")
        );
    }
}
