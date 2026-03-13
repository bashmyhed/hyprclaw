use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct Metrics {
    llm_requests: AtomicU64,
    llm_failures: AtomicU64,
    tool_executions: AtomicU64,
    tool_failures: AtomicU64,
    compactions: AtomicU64,
    permission_denials: AtomicU64,
    tasks_spawned: AtomicU64,
}

impl Metrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            llm_requests: AtomicU64::new(0),
            llm_failures: AtomicU64::new(0),
            tool_executions: AtomicU64::new(0),
            tool_failures: AtomicU64::new(0),
            compactions: AtomicU64::new(0),
            permission_denials: AtomicU64::new(0),
            tasks_spawned: AtomicU64::new(0),
        })
    }

    pub fn inc_llm_requests(&self) {
        self.llm_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_llm_failures(&self) {
        self.llm_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_tool_executions(&self) {
        self.tool_executions.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_tool_failures(&self) {
        self.tool_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_compactions(&self) {
        self.compactions.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_permission_denials(&self) {
        self.permission_denials.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_tasks_spawned(&self) {
        self.tasks_spawned.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            llm_requests: self.llm_requests.load(Ordering::Relaxed),
            llm_failures: self.llm_failures.load(Ordering::Relaxed),
            tool_executions: self.tool_executions.load(Ordering::Relaxed),
            tool_failures: self.tool_failures.load(Ordering::Relaxed),
            compactions: self.compactions.load(Ordering::Relaxed),
            permission_denials: self.permission_denials.load(Ordering::Relaxed),
            tasks_spawned: self.tasks_spawned.load(Ordering::Relaxed),
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            llm_requests: AtomicU64::new(0),
            llm_failures: AtomicU64::new(0),
            tool_executions: AtomicU64::new(0),
            tool_failures: AtomicU64::new(0),
            compactions: AtomicU64::new(0),
            permission_denials: AtomicU64::new(0),
            tasks_spawned: AtomicU64::new(0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub llm_requests: u64,
    pub llm_failures: u64,
    pub tool_executions: u64,
    pub tool_failures: u64,
    pub compactions: u64,
    pub permission_denials: u64,
    pub tasks_spawned: u64,
}

impl MetricsSnapshot {
    pub fn llm_success_rate(&self) -> f64 {
        if self.llm_requests == 0 {
            return 1.0;
        }
        1.0 - (self.llm_failures as f64 / self.llm_requests as f64)
    }

    pub fn tool_success_rate(&self) -> f64 {
        if self.tool_executions == 0 {
            return 1.0;
        }
        1.0 - (self.tool_failures as f64 / self.tool_executions as f64)
    }
}
