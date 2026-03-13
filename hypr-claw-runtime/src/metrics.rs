//! Metrics instrumentation for runtime observability.

use std::time::Instant;

/// Record LLM request latency.
pub fn record_llm_latency(duration_ms: f64) {
    metrics::histogram!("llm_request_latency", duration_ms);
}

/// Record tool execution latency.
pub fn record_tool_latency(duration_ms: f64) {
    metrics::histogram!("tool_execution_latency", duration_ms);
}

/// Record session duration.
pub fn record_session_duration(duration_ms: f64) {
    metrics::histogram!("session_duration", duration_ms);
}

/// Record lock wait duration.
pub fn record_lock_wait(duration_ms: f64) {
    metrics::histogram!("lock_wait_duration", duration_ms);
}

/// Increment compaction counter.
pub fn increment_compaction_count() {
    metrics::counter!("compaction_count", 1);
}

/// RAII timer for automatic metric recording.
pub struct MetricTimer {
    start: Instant,
    metric_name: &'static str,
}

impl MetricTimer {
    pub fn new(metric_name: &'static str) -> Self {
        Self {
            start: Instant::now(),
            metric_name,
        }
    }
}

impl Drop for MetricTimer {
    fn drop(&mut self) {
        let duration_ms = self.start.elapsed().as_secs_f64() * 1000.0;
        match self.metric_name {
            "llm_request_latency" => record_llm_latency(duration_ms),
            "tool_execution_latency" => record_tool_latency(duration_ms),
            "session_duration" => record_session_duration(duration_ms),
            "lock_wait_duration" => record_lock_wait(duration_ms),
            _ => {}
        }
    }
}
