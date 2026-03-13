//! Hyprland control - workspace and window management

use super::{OsError, OsResult};
use tokio::process::Command;

fn validate_workspace_id(id: u32) -> OsResult<()> {
    if id == 0 {
        return Err(OsError::InvalidArgument(
            "workspace id must be greater than 0".to_string(),
        ));
    }
    Ok(())
}

fn validate_window_selector(window_id: &str) -> OsResult<()> {
    if window_id.is_empty() {
        return Err(OsError::InvalidArgument(
            "window selector cannot be empty".to_string(),
        ));
    }
    if !window_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ':' | '_' | '-' | '.'))
    {
        return Err(OsError::InvalidArgument(
            "window selector contains invalid characters".to_string(),
        ));
    }
    Ok(())
}

fn validate_exec_command(command: &str) -> OsResult<()> {
    if command.trim().is_empty() {
        return Err(OsError::InvalidArgument(
            "command cannot be empty".to_string(),
        ));
    }
    if command.contains('\n') || command.contains('\0') {
        return Err(OsError::InvalidArgument(
            "command contains invalid control characters".to_string(),
        ));
    }
    Ok(())
}

/// Switch to a workspace
pub async fn workspace_switch(id: u32) -> OsResult<()> {
    validate_workspace_id(id)?;
    let output = Command::new("hyprctl")
        .args(&["dispatch", "workspace", &id.to_string()])
        .output()
        .await?;

    if !output.status.success() {
        return Err(OsError::OperationFailed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

/// Move window to workspace
pub async fn workspace_move_window(window_id: &str, workspace_id: u32) -> OsResult<()> {
    validate_workspace_id(workspace_id)?;
    validate_window_selector(window_id)?;
    let output = Command::new("hyprctl")
        .args(&[
            "dispatch",
            "movetoworkspace",
            &format!("{},{}", workspace_id, window_id),
        ])
        .output()
        .await?;

    if !output.status.success() {
        return Err(OsError::OperationFailed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

/// Focus a window
pub async fn window_focus(window_id: &str) -> OsResult<()> {
    validate_window_selector(window_id)?;
    let output = Command::new("hyprctl")
        .args(&["dispatch", "focuswindow", window_id])
        .output()
        .await?;

    if !output.status.success() {
        return Err(OsError::OperationFailed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

/// Close a window
pub async fn window_close(window_id: &str) -> OsResult<()> {
    validate_window_selector(window_id)?;
    let output = Command::new("hyprctl")
        .args(&["dispatch", "closewindow", window_id])
        .output()
        .await?;

    if !output.status.success() {
        return Err(OsError::OperationFailed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

/// Execute a program in Hyprland
pub async fn exec(command: &str) -> OsResult<()> {
    validate_exec_command(command)?;
    let output = Command::new("hyprctl")
        .args(&["dispatch", "exec", command])
        .output()
        .await?;

    if !output.status.success() {
        return Err(OsError::OperationFailed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

/// Move window to a workspace (alias for workspace move)
pub async fn window_move(window_id: &str, workspace_id: u32) -> OsResult<()> {
    workspace_move_window(window_id, workspace_id).await
}

/// Get current workspace info
pub async fn get_active_workspace() -> OsResult<u32> {
    let output = Command::new("hyprctl")
        .args(&["activeworkspace", "-j"])
        .output()
        .await?;

    if !output.status.success() {
        return Err(OsError::OperationFailed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| OsError::OperationFailed(e.to_string()))?;

    let id = json["id"]
        .as_u64()
        .ok_or_else(|| OsError::OperationFailed("Invalid workspace ID".to_string()))?;

    Ok(id as u32)
}
