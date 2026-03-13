use crate::error::ToolError;
use crate::pipeline::{
    AuditStage, ExecutionStage, InputValidationStage, PermissionStage, RecoveryClassificationStage,
    ResultNormalizationStage, SafetyPolicyStage, ToolDispatchContext, ToolDispatchRequest,
    VisibilityStage,
};
use crate::registry::ToolRegistryImpl;
use crate::tools::ToolResult;
use crate::traits::{AuditLogger, PermissionEngine};
use std::sync::Arc;
use tracing::{info, warn};

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

        let mut ctx = ToolDispatchContext::new(ToolDispatchRequest {
            session_key,
            tool_name,
            input,
        });

        let result = self.dispatch_inner(&mut ctx).await;
        AuditStage::new(&self.audit).run(&ctx, &result).await;
        result
    }

    async fn dispatch_inner(&self, ctx: &mut ToolDispatchContext) -> Result<ToolResult, ToolError> {
        VisibilityStage::new(&self.registry).run(ctx)?;
        InputValidationStage.run(ctx)?;
        SafetyPolicyStage.run(ctx)?;
        PermissionStage::new(&self.permission).run(ctx).await?;

        let result = ExecutionStage::new(self.timeout_ms).run(ctx).await;
        match result {
            Ok(result) => {
                let result = ResultNormalizationStage.run(result);
                Ok(RecoveryClassificationStage.run(result))
            }
            Err(err) => {
                if matches!(err, ToolError::PermissionDenied(_)) {
                    warn!("Permission denied during pipeline execution: {}", err);
                }
                Err(err)
            }
        }
    }
}
