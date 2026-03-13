use thiserror::Error;

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Operation timed out")]
    Timeout,

    #[error("Sandbox violation: {0}")]
    SandboxViolation(String),

    #[error("Internal error")]
    Internal,
}
