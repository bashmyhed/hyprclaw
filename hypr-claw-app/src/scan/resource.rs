use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

const THROTTLE_SAMPLE_INTERVAL_MS: u64 = 750;

struct ThrottleCache {
    system: System,
    last_checked: Instant,
    last_value: bool,
}

impl ThrottleCache {
    fn new() -> Self {
        let mut system = System::new_with_specifics(
            RefreshKind::new()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        system.refresh_all();
        Self {
            system,
            last_checked: Instant::now()
                .checked_sub(Duration::from_millis(THROTTLE_SAMPLE_INTERVAL_MS))
                .unwrap_or_else(Instant::now),
            last_value: false,
        }
    }
}

/// Adaptive resource monitor for scan operations
#[derive(Debug, Clone)]
pub struct ResourceMonitor {
    pub cpu_limit_percent: f32,
    pub memory_limit_mb: u64,
    pub io_throttle_ms: u64,
}

impl ResourceMonitor {
    pub fn auto_calibrate() -> Self {
        let mut sys = System::new_with_specifics(
            RefreshKind::new()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        sys.refresh_all();

        std::thread::sleep(std::time::Duration::from_millis(200));
        sys.refresh_cpu();

        let _total_cpu_count = sys.cpus().len() as f32;
        let total_mem_mb = sys.total_memory() / 1024 / 1024;
        let used_mem_mb = sys.used_memory() / 1024 / 1024;
        let cpu_usage = sys.global_cpu_info().cpu_usage();

        // Safe limits: leave 40% CPU and 2GB RAM for user
        let available_cpu = (100.0 - cpu_usage).max(0.0);
        let cpu_limit = (available_cpu * 0.6).min(60.0).max(20.0);

        let available_mem = total_mem_mb.saturating_sub(used_mem_mb);
        let mem_limit = available_mem.saturating_sub(2048).min(1024).max(256);

        let io_throttle = if cpu_usage > 70.0 { 5 } else { 1 };

        Self {
            cpu_limit_percent: cpu_limit,
            memory_limit_mb: mem_limit,
            io_throttle_ms: io_throttle,
        }
    }

    pub fn adjust_worker_count(&self) -> usize {
        let base = num_cpus::get();
        if self.cpu_limit_percent < 30.0 {
            (base / 2).max(1)
        } else if self.cpu_limit_percent < 50.0 {
            (base * 3 / 4).max(1)
        } else {
            base
        }
    }

    pub fn should_throttle(&self) -> bool {
        static CACHE: OnceLock<Mutex<ThrottleCache>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(ThrottleCache::new()));
        let mut cache = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if cache.last_checked.elapsed() < Duration::from_millis(THROTTLE_SAMPLE_INTERVAL_MS) {
            return cache.last_value;
        }

        cache.system.refresh_cpu();
        cache.system.refresh_memory();

        let cpu_usage = cache.system.global_cpu_info().cpu_usage();
        let total_memory = cache.system.total_memory();
        let mem_usage_percent = if total_memory == 0 {
            0.0
        } else {
            (cache.system.used_memory() as f32 / total_memory as f32) * 100.0
        };

        cache.last_value = cpu_usage > 90.0 || mem_usage_percent > 95.0;
        cache.last_checked = Instant::now();
        cache.last_value
    }

    pub fn print_calibration(&self) {
        println!("⚙️  Resource calibration:");
        println!("  CPU limit: {:.1}%", self.cpu_limit_percent);
        println!("  Memory limit: {} MB", self.memory_limit_mb);
        println!("  Worker threads: {}", self.adjust_worker_count());
        println!("  IO throttle: {} ms", self.io_throttle_ms);
    }
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        Self::auto_calibrate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_calibrate() {
        let monitor = ResourceMonitor::auto_calibrate();
        assert!(monitor.cpu_limit_percent >= 20.0);
        assert!(monitor.cpu_limit_percent <= 100.0);
        assert!(monitor.memory_limit_mb >= 256);
        assert!(monitor.io_throttle_ms >= 1);
    }

    #[test]
    fn test_adjust_worker_count() {
        let monitor = ResourceMonitor {
            cpu_limit_percent: 25.0,
            memory_limit_mb: 512,
            io_throttle_ms: 1,
        };
        let workers = monitor.adjust_worker_count();
        assert!(workers >= 1);
        assert!(workers <= num_cpus::get());
    }

    #[test]
    fn test_should_throttle() {
        let monitor = ResourceMonitor::auto_calibrate();
        // Should return a boolean without panicking
        let _ = monitor.should_throttle();
    }

    #[test]
    fn test_default_monitor() {
        let monitor = ResourceMonitor::default();
        assert!(monitor.cpu_limit_percent > 0.0);
        assert!(monitor.memory_limit_mb > 0);
    }
}
