use serde_json::Value;

#[derive(Debug, Clone)]
pub enum DebugEvent {
    RunStarted {
        session_key: String,
        agent_id: String,
        user_message: String,
    },
    LlmRequest {
        iteration: usize,
        tool_count: usize,
    },
    LlmFinal {
        iteration: usize,
        content: String,
    },
    ToolCallRequested {
        iteration: usize,
        tool_name: String,
        input: Value,
    },
    ToolResult {
        tool_name: String,
        result: Value,
    },
    ToolFailure {
        tool_name: String,
        error: String,
    },
    RunError {
        message: String,
    },
}

pub trait DebugObserver: Send + Sync {
    fn on_event(&self, event: &DebugEvent);
}

pub struct NoopDebugObserver;

impl DebugObserver for NoopDebugObserver {
    fn on_event(&self, _event: &DebugEvent) {}
}
