use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct DesktopNotificationObserver {
    enabled: AtomicBool,
}

impl DesktopNotificationObserver {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    fn notify(&self, title: &str, body: &str) {
        if !self.is_enabled() {
            return;
        }
        let title = truncate_single_line(title, 96);
        let body = truncate_multiline(body, 360);
        let _ = Command::new("notify-send")
            .args(["-a", "Hypr-Claw", &title, &body])
            .spawn();
    }
}

impl hypr_claw_runtime::DebugObserver for DesktopNotificationObserver {
    fn on_event(&self, event: &hypr_claw_runtime::DebugEvent) {
        let (title, body) = match event {
            hypr_claw_runtime::DebugEvent::RunStarted {
                session_key,
                agent_id,
                user_message,
            } => (
                "Hypr-Claw Run".to_string(),
                format!(
                    "session={} agent={}\n{}",
                    session_key,
                    agent_id,
                    truncate_multiline(user_message, 240)
                ),
            ),
            hypr_claw_runtime::DebugEvent::LlmRequest {
                iteration,
                tool_count,
            } => (
                "Hypr-Claw LLM Request".to_string(),
                format!("iteration={} tools={}", iteration, tool_count),
            ),
            hypr_claw_runtime::DebugEvent::LlmFinal { iteration, content } => (
                "Hypr-Claw LLM Final".to_string(),
                format!(
                    "iteration={}\n{}",
                    iteration,
                    truncate_multiline(content, 260)
                ),
            ),
            hypr_claw_runtime::DebugEvent::ToolCallRequested {
                iteration,
                tool_name,
                input,
            } => (
                format!("Hypr-Claw Tool {}", tool_name),
                format!(
                    "iteration={}\n{}",
                    iteration,
                    truncate_multiline(&json_pretty(input), 260)
                ),
            ),
            hypr_claw_runtime::DebugEvent::ToolResult { tool_name, result } => (
                format!("Hypr-Claw Tool OK {}", tool_name),
                truncate_multiline(&json_pretty(result), 260),
            ),
            hypr_claw_runtime::DebugEvent::ToolFailure { tool_name, error } => (
                format!("Hypr-Claw Tool Fail {}", tool_name),
                truncate_multiline(error, 260),
            ),
            hypr_claw_runtime::DebugEvent::RunError { message } => (
                "Hypr-Claw Run Error".to_string(),
                truncate_multiline(message, 260),
            ),
        };
        self.notify(&title, &body);
    }
}

fn json_pretty(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn truncate_single_line(value: &str, max_len: usize) -> String {
    let collapsed = value.replace('\n', " ");
    if collapsed.chars().count() <= max_len {
        collapsed
    } else {
        let mut truncated = collapsed.chars().take(max_len.saturating_sub(1)).collect::<String>();
        truncated.push('…');
        truncated
    }
}

fn truncate_multiline(value: &str, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        value.to_string()
    } else {
        let mut truncated = value.chars().take(max_len.saturating_sub(1)).collect::<String>();
        truncated.push('…');
        truncated
    }
}
