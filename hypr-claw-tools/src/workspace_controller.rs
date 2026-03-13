use crate::execution_context::ExecutionContext;
use crate::registry::ToolRegistryImpl;
use crate::tools::base::{Tool, ToolResult};
use crate::traits::PermissionTier;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard};
use tracing::{info, warn};

pub const AGENT_WORKSPACE_ID: u32 = 8;

pub struct WorkspaceController {
    hyprctl_path: PathBuf,
    user_workspaces: Mutex<HashMap<String, u32>>,
    agent_workspace_id: u32,
}

impl WorkspaceController {
    pub fn new() -> Self {
        Self::with_hyprctl_path_and_workspace("hyprctl", AGENT_WORKSPACE_ID)
    }

    pub fn with_hyprctl_path<P: Into<PathBuf>>(hyprctl_path: P) -> Self {
        Self::with_hyprctl_path_and_workspace(hyprctl_path, AGENT_WORKSPACE_ID)
    }

    pub fn with_hyprctl_path_and_workspace<P: Into<PathBuf>>(
        hyprctl_path: P,
        agent_workspace_id: u32,
    ) -> Self {
        Self {
            hyprctl_path: hyprctl_path.into(),
            user_workspaces: Mutex::new(HashMap::new()),
            agent_workspace_id,
        }
    }

    pub fn enter_agent_workspace(&self, session_key: &str) -> ToolResult {
        info!(
            "Workspace controller: detecting active workspace for session {}",
            session_key
        );

        let user_workspace_id = match self.active_workspace_id() {
            Ok(id) => id,
            Err(message) => {
                warn!(
                    "Workspace controller: failed to detect current workspace for session {}: {}",
                    session_key, message
                );
                return failure_result(
                    message,
                    json!({
                        "action": "enter_agent_workspace",
                        "session_key": session_key,
                        "agent_workspace_id": self.agent_workspace_id,
                    }),
                );
            }
        };

        {
            let mut workspaces = self.user_workspaces_lock();
            workspaces.insert(session_key.to_string(), user_workspace_id);
        }

        info!(
            "Workspace controller: stored user workspace {} for session {} and switching agent to workspace {}",
            user_workspace_id, session_key, self.agent_workspace_id
        );

        if let Err(message) = self.dispatch_workspace(self.agent_workspace_id) {
            warn!(
                "Workspace controller: failed to switch session {} to workspace {}: {}",
                session_key, self.agent_workspace_id, message
            );
            return failure_result(
                message,
                json!({
                    "action": "enter_agent_workspace",
                    "session_key": session_key,
                    "user_workspace_id": user_workspace_id,
                    "agent_workspace_id": self.agent_workspace_id,
                }),
            );
        }

        let current_workspace_id = match self.active_workspace_id() {
            Ok(id) => id,
            Err(message) => {
                warn!(
                    "Workspace controller: unable to confirm agent workspace for session {}: {}",
                    session_key, message
                );
                return failure_result(
                    message,
                    json!({
                        "action": "enter_agent_workspace",
                        "session_key": session_key,
                        "user_workspace_id": user_workspace_id,
                        "agent_workspace_id": self.agent_workspace_id,
                    }),
                );
            }
        };

        if current_workspace_id != self.agent_workspace_id {
            let message = format!(
                "workspace switch verification failed: expected {}, got {}",
                self.agent_workspace_id, current_workspace_id
            );
            warn!(
                "Workspace controller: session {} did not reach agent workspace: {}",
                session_key, message
            );
            return failure_result(
                message,
                json!({
                    "action": "enter_agent_workspace",
                    "session_key": session_key,
                    "user_workspace_id": user_workspace_id,
                    "agent_workspace_id": self.agent_workspace_id,
                    "current_workspace_id": current_workspace_id,
                }),
            );
        }

        info!(
            "Workspace controller: session {} moved to agent workspace {}",
            session_key, current_workspace_id
        );

        success_result(json!({
            "action": "enter_agent_workspace",
            "session_key": session_key,
            "message": "Switched to agent execution workspace",
            "user_workspace_id": user_workspace_id,
            "agent_workspace_id": self.agent_workspace_id,
            "current_workspace_id": current_workspace_id,
        }))
    }

    pub fn return_user_workspace(&self, session_key: &str) -> ToolResult {
        let user_workspace_id = {
            let workspaces = self.user_workspaces_lock();
            workspaces.get(session_key).copied()
        };

        let Some(user_workspace_id) = user_workspace_id else {
            let message = "no stored user workspace for this session".to_string();
            warn!(
                "Workspace controller: cannot restore workspace for session {}: {}",
                session_key, message
            );
            return failure_result(
                message,
                json!({
                    "action": "return_user_workspace",
                    "session_key": session_key,
                }),
            );
        };

        info!(
            "Workspace controller: restoring session {} to workspace {}",
            session_key, user_workspace_id
        );

        if let Err(message) = self.dispatch_workspace(user_workspace_id) {
            warn!(
                "Workspace controller: failed restoring session {} to workspace {}: {}",
                session_key, user_workspace_id, message
            );
            return failure_result(
                message,
                json!({
                    "action": "return_user_workspace",
                    "session_key": session_key,
                    "user_workspace_id": user_workspace_id,
                }),
            );
        }

        let current_workspace_id = match self.active_workspace_id() {
            Ok(id) => id,
            Err(message) => {
                warn!(
                    "Workspace controller: unable to confirm restored workspace for session {}: {}",
                    session_key, message
                );
                return failure_result(
                    message,
                    json!({
                        "action": "return_user_workspace",
                        "session_key": session_key,
                        "user_workspace_id": user_workspace_id,
                    }),
                );
            }
        };

        if current_workspace_id != user_workspace_id {
            let message = format!(
                "workspace restore verification failed: expected {}, got {}",
                user_workspace_id, current_workspace_id
            );
            warn!(
                "Workspace controller: session {} restore verification failed: {}",
                session_key, message
            );
            return failure_result(
                message,
                json!({
                    "action": "return_user_workspace",
                    "session_key": session_key,
                    "user_workspace_id": user_workspace_id,
                    "current_workspace_id": current_workspace_id,
                }),
            );
        }

        {
            let mut workspaces = self.user_workspaces_lock();
            workspaces.remove(session_key);
        }

        info!(
            "Workspace controller: session {} returned to workspace {}",
            session_key, user_workspace_id
        );

        success_result(json!({
            "action": "return_user_workspace",
            "session_key": session_key,
            "message": "Returned to user workspace",
            "user_workspace_id": user_workspace_id,
            "current_workspace_id": current_workspace_id,
            "restored": true,
        }))
    }

    fn user_workspaces_lock(&self) -> MutexGuard<'_, HashMap<String, u32>> {
        match self.user_workspaces.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("Workspace controller: recovering from poisoned workspace state lock");
                poisoned.into_inner()
            }
        }
    }

    fn active_workspace_id(&self) -> Result<u32, String> {
        let output = self.run_hyprctl(&["activeworkspace", "-j"])?;
        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("failed to parse active workspace JSON: {}", error))?;
        let workspace_id = value
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| "active workspace JSON did not contain a numeric id".to_string())?;
        u32::try_from(workspace_id)
            .map_err(|_| format!("workspace id {} does not fit into u32", workspace_id))
    }

    fn dispatch_workspace(&self, workspace_id: u32) -> Result<(), String> {
        self.run_hyprctl(&["dispatch", "workspace", &workspace_id.to_string()])?;
        Ok(())
    }

    fn run_hyprctl(&self, args: &[&str]) -> Result<std::process::Output, String> {
        let output = Command::new(&self.hyprctl_path)
            .args(args)
            .output()
            .map_err(|error| {
                format!(
                    "failed to execute {} {:?}: {}",
                    display_command(&self.hyprctl_path),
                    args,
                    error
                )
            })?;

        if output.status.success() {
            return Ok(output);
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let details = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            "no output".to_string()
        };

        Err(format!(
            "{} {:?} failed with status {:?}: {}",
            display_command(&self.hyprctl_path),
            args,
            output.status.code(),
            details
        ))
    }
}

impl Default for WorkspaceController {
    fn default() -> Self {
        Self::new()
    }
}

pub struct HyprWorkspaceEnterAgentTool {
    controller: Arc<WorkspaceController>,
}

impl HyprWorkspaceEnterAgentTool {
    pub fn new(controller: Arc<WorkspaceController>) -> Self {
        Self { controller }
    }
}

#[async_trait]
impl Tool for HyprWorkspaceEnterAgentTool {
    fn name(&self) -> &'static str {
        "hypr_workspace_enter_agent"
    }

    fn description(&self) -> &'static str {
        "Switch to the agent execution workspace."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }

    async fn execute(
        &self,
        ctx: ExecutionContext,
        _input: Value,
    ) -> Result<ToolResult, crate::error::ToolError> {
        Ok(self.controller.enter_agent_workspace(&ctx.session_key))
    }
}

pub struct HyprWorkspaceReturnUserTool {
    controller: Arc<WorkspaceController>,
}

impl HyprWorkspaceReturnUserTool {
    pub fn new(controller: Arc<WorkspaceController>) -> Self {
        Self { controller }
    }
}

#[async_trait]
impl Tool for HyprWorkspaceReturnUserTool {
    fn name(&self) -> &'static str {
        "hypr_workspace_return_user"
    }

    fn description(&self) -> &'static str {
        "Return to the user's original workspace."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }

    async fn execute(
        &self,
        ctx: ExecutionContext,
        _input: Value,
    ) -> Result<ToolResult, crate::error::ToolError> {
        Ok(self.controller.return_user_workspace(&ctx.session_key))
    }
}

pub fn register_workspace_tools(registry: &mut ToolRegistryImpl) -> Arc<WorkspaceController> {
    let controller = Arc::new(WorkspaceController::new());
    registry.register(Arc::new(HyprWorkspaceEnterAgentTool::new(
        controller.clone(),
    )));
    registry.register(Arc::new(HyprWorkspaceReturnUserTool::new(
        controller.clone(),
    )));
    controller
}

fn display_command(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn success_result(output: Value) -> ToolResult {
    ToolResult {
        success: true,
        output: Some(output),
        error: None,
    }
}

fn failure_result(message: String, output: Value) -> ToolResult {
    ToolResult {
        success: false,
        output: Some(output),
        error: Some(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn install_fake_hyprctl(dir: &Path, initial_workspace: u32) -> PathBuf {
        let state_file = dir.join("workspace_state");
        let log_file = dir.join("workspace_log");
        let script_path = dir.join("hyprctl");

        fs::write(&state_file, initial_workspace.to_string()).unwrap();
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh
STATE_FILE=\"{state}\"
LOG_FILE=\"{log}\"
printf '%s\n' \"$*\" >> \"$LOG_FILE\"
if [ \"$1\" = \"activeworkspace\" ] && [ \"$2\" = \"-j\" ]; then
  current=$(cat \"$STATE_FILE\")
  printf '{{\"id\":%s}}\n' \"$current\"
  exit 0
fi
if [ \"$1\" = \"dispatch\" ] && [ \"$2\" = \"workspace\" ]; then
  printf '%s' \"$3\" > \"$STATE_FILE\"
  exit 0
fi
echo \"unsupported args: $*\" >&2
exit 1
",
                state = state_file.display(),
                log = log_file.display(),
            ),
        )
        .unwrap();

        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();

        script_path
    }

    #[tokio::test]
    async fn enter_tool_switches_to_agent_workspace_and_returns_previous_workspace() {
        let temp_dir = tempfile::tempdir().unwrap();
        let hyprctl_path = install_fake_hyprctl(temp_dir.path(), 3);
        let controller = Arc::new(WorkspaceController::with_hyprctl_path(hyprctl_path));
        let tool = HyprWorkspaceEnterAgentTool::new(controller);
        let ctx = ExecutionContext::new("session-a".into(), 5000);

        let result = tool.execute(ctx, json!({})).await.unwrap();

        assert!(result.success);
        assert_eq!(
            result
                .output
                .as_ref()
                .and_then(|value| value.get("user_workspace_id")),
            Some(&json!(3))
        );
        assert_eq!(
            result
                .output
                .as_ref()
                .and_then(|value| value.get("current_workspace_id")),
            Some(&json!(AGENT_WORKSPACE_ID))
        );
    }

    #[tokio::test]
    async fn return_tool_restores_workspace_for_same_session() {
        let temp_dir = tempfile::tempdir().unwrap();
        let hyprctl_path = install_fake_hyprctl(temp_dir.path(), 5);
        let controller = Arc::new(WorkspaceController::with_hyprctl_path(hyprctl_path));
        let enter_tool = HyprWorkspaceEnterAgentTool::new(controller.clone());
        let return_tool = HyprWorkspaceReturnUserTool::new(controller);
        let ctx = ExecutionContext::new("session-b".into(), 5000);

        let enter_result = enter_tool.execute(ctx.clone(), json!({})).await.unwrap();
        assert!(enter_result.success);

        let return_result = return_tool.execute(ctx, json!({})).await.unwrap();
        assert!(return_result.success);
        assert_eq!(
            return_result
                .output
                .as_ref()
                .and_then(|value| value.get("current_workspace_id")),
            Some(&json!(5))
        );
    }

    #[tokio::test]
    async fn return_tool_fails_when_no_workspace_was_stored() {
        let temp_dir = tempfile::tempdir().unwrap();
        let hyprctl_path = install_fake_hyprctl(temp_dir.path(), 4);
        let controller = Arc::new(WorkspaceController::with_hyprctl_path(hyprctl_path));
        let tool = HyprWorkspaceReturnUserTool::new(controller);
        let ctx = ExecutionContext::new("session-c".into(), 5000);

        let result = tool.execute(ctx, json!({})).await.unwrap();

        assert!(!result.success);
        assert_eq!(
            result.error.as_deref(),
            Some("no stored user workspace for this session")
        );
    }

    #[test]
    fn register_workspace_tools_adds_both_tools() {
        let mut registry = ToolRegistryImpl::new();

        register_workspace_tools(&mut registry);

        assert!(registry.get("hypr_workspace_enter_agent").is_some());
        assert!(registry.get("hypr_workspace_return_user").is_some());
    }
}
