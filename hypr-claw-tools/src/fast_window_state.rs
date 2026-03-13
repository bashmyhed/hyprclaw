use crate::error::ToolError;
use crate::execution_context::ExecutionContext;
use crate::tools::base::{Tool, ToolResult};
use crate::traits::PermissionTier;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::process::Command;

pub struct DesktopFastWindowStateTool;

#[derive(Debug, Deserialize)]
struct HyprClientWorkspace {
    id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct HyprClient {
    #[serde(default)]
    class: String,
    #[serde(default)]
    title: String,
    workspace: Option<HyprClientWorkspace>,
    mapped: Option<bool>,
    hidden: Option<bool>,
    #[serde(rename = "focusHistoryID")]
    focus_history_id: Option<i64>,
    focused: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowSummary {
    class: String,
    title: String,
    workspace: i64,
}

#[async_trait]
impl Tool for DesktopFastWindowStateTool {
    fn name(&self) -> &'static str {
        "desktop.fast_window_state"
    }

    fn description(&self) -> &'static str {
        "Read active and visible Hyprland windows from fast metadata without screenshot or OCR"
    }

    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        _ctx: ExecutionContext,
        _input: Value,
    ) -> Result<ToolResult, ToolError> {
        let output = Command::new("hyprctl")
            .args(["clients", "-j"])
            .output()
            .map_err(|error| {
                ToolError::ExecutionFailed(format!(
                    "failed to execute hyprctl clients -j: {}",
                    error
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let details = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                "no output".to_string()
            };

            return Err(ToolError::ExecutionFailed(format!(
                "hyprctl clients -j failed with status {:?}: {}",
                output.status.code(),
                details
            )));
        }

        let clients: Vec<HyprClient> = serde_json::from_slice(&output.stdout).map_err(|error| {
            ToolError::ExecutionFailed(format!(
                "failed to parse hyprctl clients -j output: {}",
                error
            ))
        })?;

        let state = build_fast_window_state(clients);

        Ok(ToolResult {
            success: true,
            output: Some(state),
            error: None,
            ..ToolResult::default()
        })
    }
}

fn build_fast_window_state(clients: Vec<HyprClient>) -> Value {
    let active_window = clients
        .iter()
        .find(|client| client.focused == Some(true))
        .or_else(|| {
            clients
                .iter()
                .find(|client| client.focus_history_id == Some(0))
        })
        .map(window_summary);

    let visible_windows: Vec<WindowSummary> = clients
        .iter()
        .filter(|client| client.mapped.unwrap_or(true) && !client.hidden.unwrap_or(false))
        .map(window_summary)
        .collect();

    let windows_json: Vec<Value> = visible_windows.iter().map(window_summary_json).collect();

    let active_workspace = active_window
        .as_ref()
        .map(|window| window.workspace)
        .or_else(|| visible_windows.first().map(|window| window.workspace));

    let active_window_json = active_window
        .as_ref()
        .map(window_summary_json)
        .unwrap_or(Value::Null);

    json!({
        "active_window": active_window_json,
        "windows": windows_json,
        "window_count": visible_windows.len(),
        "active_workspace": active_workspace,
        "source": "hyprctl clients -j"
    })
}

fn window_summary(client: &HyprClient) -> WindowSummary {
    WindowSummary {
        class: client.class.clone(),
        title: client.title.clone(),
        workspace: client
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.id)
            .unwrap_or_default(),
    }
}

fn window_summary_json(window: &WindowSummary) -> Value {
    json!({
        "class": window.class,
        "title": window.title,
        "workspace": window.workspace
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_fast_window_state_prefers_focused_client_and_visible_windows() {
        let clients = vec![
            HyprClient {
                class: "firefox".to_string(),
                title: "YouTube".to_string(),
                workspace: Some(HyprClientWorkspace { id: Some(5) }),
                mapped: Some(true),
                hidden: Some(false),
                focus_history_id: Some(0),
                focused: Some(true),
            },
            HyprClient {
                class: "Alacritty".to_string(),
                title: "Terminal".to_string(),
                workspace: Some(HyprClientWorkspace { id: Some(5) }),
                mapped: Some(true),
                hidden: Some(false),
                focus_history_id: Some(1),
                focused: Some(false),
            },
            HyprClient {
                class: "Slack".to_string(),
                title: "Hidden".to_string(),
                workspace: Some(HyprClientWorkspace { id: Some(7) }),
                mapped: Some(true),
                hidden: Some(true),
                focus_history_id: Some(2),
                focused: Some(false),
            },
        ];

        let result = build_fast_window_state(clients);

        assert_eq!(result["active_window"]["class"], "firefox");
        assert_eq!(result["active_window"]["title"], "YouTube");
        assert_eq!(result["active_window"]["workspace"], 5);
        assert_eq!(result["window_count"], 2);
        assert_eq!(result["windows"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn build_fast_window_state_uses_focus_history_when_focused_flag_missing() {
        let clients = vec![
            HyprClient {
                class: "code".to_string(),
                title: "main.rs".to_string(),
                workspace: Some(HyprClientWorkspace { id: Some(3) }),
                mapped: Some(true),
                hidden: Some(false),
                focus_history_id: Some(0),
                focused: None,
            },
            HyprClient {
                class: "firefox".to_string(),
                title: "Docs".to_string(),
                workspace: Some(HyprClientWorkspace { id: Some(4) }),
                mapped: Some(true),
                hidden: Some(false),
                focus_history_id: Some(1),
                focused: None,
            },
        ];

        let result = build_fast_window_state(clients);

        assert_eq!(result["active_window"]["class"], "code");
        assert_eq!(result["active_workspace"], 3);
    }

    #[test]
    fn build_fast_window_state_returns_null_active_window_when_none_match() {
        let clients = vec![HyprClient {
            class: "firefox".to_string(),
            title: "Background".to_string(),
            workspace: Some(HyprClientWorkspace { id: Some(2) }),
            mapped: Some(true),
            hidden: Some(false),
            focus_history_id: Some(4),
            focused: Some(false),
        }];

        let result = build_fast_window_state(clients);

        assert!(result["active_window"].is_null());
        assert_eq!(result["window_count"], 1);
        assert_eq!(result["active_workspace"], 2);
    }
}
