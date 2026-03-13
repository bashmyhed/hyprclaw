use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContext {
    pub session_key: String,
    pub timeout_ms: u64,
    pub audit_ref: String,
    pub permission_ref: String,
}

impl ExecutionContext {
    pub fn new(session_key: String, timeout_ms: u64) -> Self {
        Self {
            session_key,
            timeout_ms,
            audit_ref: uuid::Uuid::new_v4().to_string(),
            permission_ref: uuid::Uuid::new_v4().to_string(),
        }
    }
}
