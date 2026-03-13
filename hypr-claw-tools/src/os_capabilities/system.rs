//! System operations - wallpaper, power, system info

use super::{OsError, OsResult};
use std::path::Path;
use sysinfo::System;
use tokio::fs;
use tokio::process::Command;
use tokio::task;

async fn command_exists(command: &str) -> bool {
    Command::new("which")
        .arg(command)
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}

async fn run_checked(command: &str, args: &[&str]) -> OsResult<()> {
    let output = Command::new(command).args(args).output().await?;
    if output.status.success() {
        return Ok(());
    }
    Err(OsError::OperationFailed(
        String::from_utf8_lossy(&output.stderr).trim().to_string(),
    ))
}

/// Set wallpaper using any available backend.
pub async fn wallpaper_set(image_path: &str) -> OsResult<()> {
    if !Path::new(image_path).exists() {
        return Err(OsError::NotFound(image_path.to_string()));
    }

    if command_exists("swww").await && run_checked("swww", &["img", image_path]).await.is_ok() {
        return Ok(());
    }

    if command_exists("hyprctl").await {
        let preload = run_checked("hyprctl", &["hyprpaper", "preload", image_path]).await;
        let wallpaper_arg = format!(",{image_path}");
        let set_wallpaper = run_checked(
            "hyprctl",
            &["hyprpaper", "wallpaper", wallpaper_arg.as_str()],
        )
        .await;
        if preload.is_ok() && set_wallpaper.is_ok() {
            return Ok(());
        }
    }

    if command_exists("caelestia").await {
        if run_checked("caelestia", &["wallpaper", "-f", image_path])
            .await
            .is_ok()
            || run_checked("caelestia", &["wallpaper", "-h", image_path])
                .await
                .is_ok()
        {
            return Ok(());
        }
    }

    Err(OsError::OperationFailed(
        "No wallpaper backend succeeded (tried: swww, hyprpaper via hyprctl, caelestia)"
            .to_string(),
    ))
}

/// Get battery level
pub async fn battery_level() -> OsResult<u8> {
    let power_supply = Path::new("/sys/class/power_supply");
    if !power_supply.exists() {
        return Err(OsError::OperationFailed(
            "power supply information unavailable".to_string(),
        ));
    }

    let mut entries = fs::read_dir(power_supply).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("BAT") {
            continue;
        }
        let capacity_file = entry.path().join("capacity");
        if capacity_file.exists() {
            let raw = fs::read_to_string(capacity_file).await?;
            let parsed = raw
                .trim()
                .parse::<u8>()
                .map_err(|e| OsError::OperationFailed(e.to_string()))?;
            return Ok(parsed);
        }
    }

    Err(OsError::OperationFailed(
        "battery capacity not found".to_string(),
    ))
}

/// Get memory info
pub async fn memory_info() -> OsResult<MemoryInfo> {
    task::spawn_blocking(|| {
        let mut system = System::new_all();
        system.refresh_all();

        Ok(MemoryInfo {
            total_mb: system.total_memory() / 1024,
            used_mb: system.used_memory() / 1024,
            available_mb: system.available_memory() / 1024,
        })
    })
    .await
    .map_err(|e| OsError::OperationFailed(e.to_string()))?
}

/// Shutdown system
pub async fn shutdown() -> OsResult<()> {
    let output = Command::new("systemctl")
        .args(&["poweroff"])
        .output()
        .await?;

    if !output.status.success() {
        return Err(OsError::OperationFailed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

/// Reboot system
pub async fn reboot() -> OsResult<()> {
    let output = Command::new("systemctl").args(&["reboot"]).output().await?;

    if !output.status.success() {
        return Err(OsError::OperationFailed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MemoryInfo {
    pub total_mb: u64,
    pub used_mb: u64,
    pub available_mb: u64,
}
