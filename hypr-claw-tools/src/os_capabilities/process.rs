//! Process management - spawn, kill, list processes

use super::{OsError, OsResult};
use sysinfo::System;
use tokio::process::Command;
use tokio::task;

/// Spawn a process
pub async fn spawn(command: &str, args: &[&str]) -> OsResult<u32> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Err(OsError::InvalidArgument(
            "command cannot be empty".to_string(),
        ));
    }

    let (program, normalized_args): (String, Vec<String>) =
        if args.is_empty() && trimmed.contains(' ') {
            let parts = trimmed
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<String>>();
            if parts.is_empty() {
                return Err(OsError::InvalidArgument(
                    "command cannot be empty".to_string(),
                ));
            }
            let program = parts[0].clone();
            let tail = parts.into_iter().skip(1).collect::<Vec<String>>();
            (program, tail)
        } else {
            (
                trimmed.to_string(),
                args.iter()
                    .map(|arg| (*arg).to_string())
                    .collect::<Vec<String>>(),
            )
        };

    let child = Command::new(program)
        .args(normalized_args.iter().map(String::as_str))
        .spawn()?;

    Ok(child
        .id()
        .ok_or_else(|| OsError::OperationFailed("Failed to get process ID".to_string()))?)
}

/// Kill a process by PID
pub async fn kill(pid: u32) -> OsResult<()> {
    let output = Command::new("kill")
        .args(&["-9", &pid.to_string()])
        .output()
        .await?;

    if !output.status.success() {
        return Err(OsError::OperationFailed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

/// List running processes
pub async fn list() -> OsResult<Vec<ProcessInfo>> {
    task::spawn_blocking(|| {
        let mut system = System::new_all();
        system.refresh_all();

        let processes: Vec<ProcessInfo> = system
            .processes()
            .iter()
            .map(|(pid, process)| ProcessInfo {
                pid: pid.as_u32(),
                name: process.name().to_string(),
                cpu_usage: process.cpu_usage(),
                memory: process.memory(),
            })
            .collect();

        Ok(processes)
    })
    .await
    .map_err(|e| OsError::OperationFailed(e.to_string()))?
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory: u64,
}
