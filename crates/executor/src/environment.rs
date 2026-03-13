use serde::{Deserialize, Serialize};
use std::env;
use sysinfo::{Disks, System};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentSnapshot {
    pub timestamp: i64,
    pub workspace: String,
    pub system: SystemSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSnapshot {
    pub running_processes: Vec<ProcessInfo>,
    pub memory_usage_mb: u64,
    pub memory_total_mb: u64,
    pub disk_usage_percent: f32,
    pub battery_percent: Option<u8>,
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub name: String,
    pub pid: u32,
}

impl EnvironmentSnapshot {
    pub fn capture() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();

        let workspace = env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "/".to_string());

        let running_processes = sys
            .processes()
            .iter()
            .take(20) // Top 20 processes
            .map(|(pid, process)| ProcessInfo {
                name: process.name().to_string(),
                pid: pid.as_u32(),
            })
            .collect();

        let memory_usage_mb = sys.used_memory() / 1024 / 1024;
        let memory_total_mb = sys.total_memory() / 1024 / 1024;

        let disks = Disks::new_with_refreshed_list();
        let disk_usage_percent = disks
            .iter()
            .next()
            .map(|disk| {
                let total = disk.total_space();
                let available = disk.available_space();
                if total > 0 {
                    ((total - available) as f64 / total as f64 * 100.0) as f32
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);

        // Battery info (Linux-specific, optional)
        let battery_percent = Self::read_battery_percent();

        Self {
            timestamp: chrono::Utc::now().timestamp(),
            workspace,
            system: SystemSnapshot {
                running_processes,
                memory_usage_mb,
                memory_total_mb,
                disk_usage_percent,
                battery_percent,
                uptime_seconds: System::uptime(),
            },
        }
    }

    fn read_battery_percent() -> Option<u8> {
        #[cfg(target_os = "linux")]
        {
            std::fs::read_to_string("/sys/class/power_supply/BAT0/capacity")
                .ok()
                .and_then(|s| s.trim().parse().ok())
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    pub fn to_concise_string(&self) -> String {
        format!(
            "Workspace: {}\nMemory: {}/{} MB\nDisk: {:.1}%\nProcesses: {}\nBattery: {}",
            self.workspace,
            self.system.memory_usage_mb,
            self.system.memory_total_mb,
            self.system.disk_usage_percent,
            self.system.running_processes.len(),
            self.system
                .battery_percent
                .map(|p| format!("{}%", p))
                .unwrap_or_else(|| "N/A".to_string())
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environment_capture() {
        let snapshot = EnvironmentSnapshot::capture();
        assert!(!snapshot.workspace.is_empty());
        assert!(snapshot.system.memory_total_mb > 0);
        assert!(!snapshot.system.running_processes.is_empty());
    }

    #[test]
    fn test_concise_string() {
        let snapshot = EnvironmentSnapshot::capture();
        let concise = snapshot.to_concise_string();
        assert!(concise.contains("Workspace:"));
        assert!(concise.contains("Memory:"));
    }
}
