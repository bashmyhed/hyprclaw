use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    pub session_id: String,
    pub user_id: String,
    pub soul_config: SoulConfig,
    pub environment: EnvironmentSnapshot,
    pub persistent_context: PersistentContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoulConfig {
    pub id: String,
    pub system_prompt: String,
    pub allowed_tools: Vec<String>,
    pub autonomy_mode: AutonomyMode,
    pub max_iterations: usize,
    pub risk_tolerance: RiskTolerance,
    pub verbosity: VerbosityLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AutonomyMode {
    Auto,
    Confirm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskTolerance {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerbosityLevel {
    Minimal,
    Normal,
    Verbose,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentSnapshot {
    pub workspace: String,
    pub timestamp: i64,
    pub system_info: SystemInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub running_processes: Vec<String>,
    pub memory_usage_mb: u64,
    pub disk_usage_percent: f32,
    pub battery_percent: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentContext {
    pub system_state: serde_json::Value,
    pub facts: Vec<String>,
    pub recent_history: Vec<HistoryEntry>,
    pub long_term_summary: String,
    pub active_tasks: Vec<TaskState>,
    pub tool_stats: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub timestamp: i64,
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub progress: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: serde_json::Value,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub requires_approval: bool,
    pub completed: bool,
}
