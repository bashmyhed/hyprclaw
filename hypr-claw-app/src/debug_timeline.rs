use hypr_claw_runtime::{DebugEvent, DebugObserver};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct CompositeDebugObserver {
    observers: Vec<Arc<dyn DebugObserver>>,
}

impl CompositeDebugObserver {
    pub fn new(observers: Vec<Arc<dyn DebugObserver>>) -> Self {
        Self { observers }
    }
}

impl DebugObserver for CompositeDebugObserver {
    fn on_event(&self, event: &DebugEvent) {
        for observer in &self.observers {
            observer.on_event(event);
        }
    }
}

pub struct TimelineDebugObserver {
    feed: Arc<Mutex<Vec<String>>>,
    live_print: bool,
    include_payloads: bool,
    started_at: Mutex<Option<Instant>>,
}

impl TimelineDebugObserver {
    pub fn new(feed: Arc<Mutex<Vec<String>>>, live_print: bool, include_payloads: bool) -> Self {
        Self {
            feed,
            live_print,
            include_payloads,
            started_at: Mutex::new(None),
        }
    }

    fn emit(&self, stage: &str, detail: String) {
        let elapsed = self.elapsed_ms();
        let line = format!("[{stage:<6}] {elapsed:>5}ms  {detail}");
        if let Ok(mut rows) = self.feed.lock() {
            rows.push(line.clone());
            if rows.len() > 320 {
                let drop_count = rows.len() - 320;
                rows.drain(0..drop_count);
            }
        }
        if self.live_print {
            println!("{line}");
        }
    }

    fn elapsed_ms(&self) -> u128 {
        self.started_at
            .lock()
            .ok()
            .and_then(|guard| *guard)
            .map(|started| started.elapsed().as_millis())
            .unwrap_or(0)
    }

    fn tool_summary(&self, tool_name: &str, input: &Value) -> String {
        let mut fields = Vec::new();
        for key in [
            "kind", "url", "selector", "text", "value", "key", "app", "path", "query", "target",
        ] {
            if let Some(value) = input.get(key) {
                let rendered = summarize_value(value, 44);
                if !rendered.is_empty() && rendered != "\"\"" {
                    fields.push(format!("{key}={rendered}"));
                }
            }
        }
        let detail = if fields.is_empty() {
            summarize_value(input, 96)
        } else {
            fields.join(" ")
        };
        format!("{tool_name} {}", truncate_line(&detail, 112))
            .trim()
            .to_string()
    }
}

impl DebugObserver for TimelineDebugObserver {
    fn on_event(&self, event: &DebugEvent) {
        match event {
            DebugEvent::RunStarted { user_message, .. } => {
                if let Ok(mut started_at) = self.started_at.lock() {
                    *started_at = Some(Instant::now());
                }
                self.emit(
                    "task",
                    format!("goal={}", truncate_line(&collapse_line(user_message), 112)),
                );
            }
            DebugEvent::LlmRequest {
                iteration,
                tool_count,
            } => {
                self.emit(
                    "think",
                    format!(
                        "iteration={} evaluating next step across {} tools",
                        iteration, tool_count
                    ),
                );
            }
            DebugEvent::ToolCallRequested {
                iteration,
                tool_name,
                input,
            } => {
                self.emit(
                    "tool",
                    format!(
                        "iteration={} {}",
                        iteration,
                        self.tool_summary(tool_name, input)
                    ),
                );
            }
            DebugEvent::ToolResult { tool_name, result } => {
                if self.include_payloads {
                    self.emit(
                        "state",
                        format!(
                            "{} -> {}",
                            tool_name,
                            truncate_line(&summarize_value(result, 112), 112)
                        ),
                    );
                }
            }
            DebugEvent::ToolFailure { tool_name, error } => {
                if self.include_payloads {
                    self.emit(
                        "fail",
                        format!(
                            "{tool_name} -> {}",
                            truncate_line(&collapse_line(error), 112)
                        ),
                    );
                }
            }
            DebugEvent::LlmFinal { content, .. } => {
                self.emit(
                    "done",
                    format!(
                        "response ready: {}",
                        truncate_line(&collapse_line(content), 112)
                    ),
                );
            }
            DebugEvent::RunError { message } => {
                self.emit(
                    "error",
                    truncate_line(&collapse_line(message), 112).to_string(),
                );
            }
        }
    }
}

fn truncate_line(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let mut out = value
            .chars()
            .take(max.saturating_sub(3))
            .collect::<String>();
        out.push_str("...");
        out
    }
}

fn collapse_line(raw: &str) -> String {
    raw.replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn summarize_value(value: &Value, max: usize) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => format!("{:?}", truncate_line(&collapse_line(v), max)),
        Value::Array(values) => {
            let preview = values
                .iter()
                .take(4)
                .map(|item| summarize_value(item, max / 2))
                .collect::<Vec<_>>()
                .join(", ");
            let suffix = if values.len() > 4 { ", ..." } else { "" };
            truncate_line(&format!("[{}{}]", preview, suffix), max)
        }
        Value::Object(map) => {
            let preview = map
                .iter()
                .take(5)
                .map(|(key, item)| format!("{key}={}", summarize_value(item, max / 2)))
                .collect::<Vec<_>>()
                .join(" ");
            truncate_line(&preview, max)
        }
    }
}
