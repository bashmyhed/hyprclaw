use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionTier {
    Read,
    Write,
    Execute,
    SystemCritical,
}

/// Permission decision result
#[derive(Debug, Clone)]
pub enum PermissionDecision {
    Allow,
    Deny(String),
    RequireApproval(String),
}

/// Permission request
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub session_key: String,
    pub tool_name: String,
    pub input: Value,
    pub permission_tier: PermissionTier,
    pub timestamp: String,
}

/// Permission engine trait
#[async_trait]
pub trait PermissionEngine: Send + Sync {
    async fn check(&self, request: PermissionRequest) -> PermissionDecision;
}

/// Audit logger trait
#[async_trait]
pub trait AuditLogger: Send + Sync {
    async fn log(&self, entry: Value);
}
