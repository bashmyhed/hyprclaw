use crate::infra::permission_engine::PermissionEngine;
use async_trait::async_trait;
use hypr_claw_tools::{
    PermissionDecision, PermissionEngine as PermissionEngineTrait, PermissionRequest,
    PermissionTier,
};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;
use tokio::time::{timeout, Duration};

#[async_trait]
impl PermissionEngineTrait for PermissionEngine {
    async fn check(&self, request: PermissionRequest) -> PermissionDecision {
        // Convert to infra types
        let input_map: HashMap<String, serde_json::Value> = request
            .input
            .as_object()
            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        let infra_request = crate::infra::contracts::PermissionRequest {
            session_key: request.session_key,
            tool_name: request.tool_name,
            input: input_map,
            permission_level: match request.permission_tier {
                PermissionTier::Read | PermissionTier::Write | PermissionTier::Execute => {
                    crate::infra::contracts::PermissionLevel::SAFE
                }
                PermissionTier::SystemCritical => {
                    crate::infra::contracts::PermissionLevel::REQUIRE_APPROVAL
                }
            },
        };

        let decision = self.check(&infra_request);

        match decision {
            crate::infra::contracts::PermissionDecision::ALLOW => PermissionDecision::Allow,
            crate::infra::contracts::PermissionDecision::DENY => {
                PermissionDecision::Deny("Permission denied".to_string())
            }
            crate::infra::contracts::PermissionDecision::REQUIRE_APPROVAL => {
                if is_full_auto_mode_enabled() {
                    return PermissionDecision::Allow;
                }
                let description = format!(
                    "{} with input {}",
                    infra_request.tool_name,
                    serde_json::to_string(&infra_request.input).unwrap_or_default()
                );
                if prompt_user_approval(&description).await {
                    PermissionDecision::Allow
                } else {
                    PermissionDecision::Deny("Approval denied or timed out".to_string())
                }
            }
        }
    }
}

fn is_full_auto_mode_enabled() -> bool {
    Path::new("./data/full_auto_mode.flag").exists()
}

async fn prompt_user_approval(description: &str) -> bool {
    print!("Approve action: {} [y/N] ", description);
    let _ = io::stdout().flush();

    let task = tokio::task::spawn_blocking(|| {
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        input
    });

    match timeout(Duration::from_secs(30), task).await {
        Ok(Ok(input)) => input.trim().eq_ignore_ascii_case("y"),
        _ => false,
    }
}
