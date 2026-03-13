//! Enhanced tool execution logging.

use serde_json::Value;
use tracing::info;

/// Log tool call details for debugging.
pub fn log_tool_call(tool_name: &str, input: &Value, session_key: &str, iteration: usize, max_iterations: usize) {
    info!("🔧 TOOL CALL:");
    info!("  Tool: '{}'", tool_name);
    info!("  Input: {}", serde_json::to_string_pretty(input).unwrap_or_else(|_| "{}".to_string()));
    info!("  Session: {}", session_key);
    info!("  Iteration: {}/{}", iteration, max_iterations);
}

/// Log tool execution result.
pub fn log_tool_result(success: bool, output: &Option<Value>, error: &Option<String>) {
    if success {
        info!("✅ TOOL SUCCESS");
        if let Some(out) = output {
            info!("  Output: {}", serde_json::to_string_pretty(out).unwrap_or_else(|_| "{}".to_string()));
        }
    } else {
        info!("❌ TOOL FAILURE");
        if let Some(err) = error {
            info!("  Error: {}", err);
        }
    }
}
