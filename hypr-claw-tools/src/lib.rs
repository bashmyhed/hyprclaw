pub mod audit_adapter;
pub mod dispatcher;
pub mod error;
pub mod execution_context;
pub mod fast_window_state;
pub mod os_capabilities;
pub mod os_tools;
pub mod permission_adapter;
pub mod registry;
pub mod runtime_health;
pub mod sandbox;
pub mod tools;
pub mod traits;
pub mod workspace_controller;

pub use dispatcher::ToolDispatcherImpl;
pub use error::ToolError;
pub use execution_context::ExecutionContext;
pub use fast_window_state::DesktopFastWindowStateTool;
pub use registry::ToolRegistryImpl;
pub use runtime_health::{probe_runtime_health, BackendStatus, RuntimeHealthSnapshot};
pub use tools::{Tool, ToolResult};
pub use traits::{
    AuditLogger, PermissionDecision, PermissionEngine, PermissionRequest, PermissionTier,
};
pub use workspace_controller::{
    register_workspace_tools, HyprWorkspaceEnterAgentTool, HyprWorkspaceReturnUserTool,
    WorkspaceController, AGENT_WORKSPACE_ID,
};
