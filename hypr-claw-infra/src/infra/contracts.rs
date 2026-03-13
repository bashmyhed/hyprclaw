use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionLevel {
    #[allow(non_camel_case_types)]
    SAFE,
    #[allow(non_camel_case_types)]
    REQUIRE_APPROVAL,
    #[allow(non_camel_case_types)]
    DANGEROUS,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionDecision {
    #[allow(non_camel_case_types)]
    ALLOW,
    #[allow(non_camel_case_types)]
    DENY,
    #[allow(non_camel_case_types)]
    REQUIRE_APPROVAL,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub session_key: String,
    pub tool_name: String,
    pub input: HashMap<String, serde_json::Value>,
    pub permission_level: PermissionLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub session: String,
    pub tool: String,
    pub input: HashMap<String, serde_json::Value>,
    pub result: HashMap<String, serde_json::Value>,
    pub approval: PermissionDecision,
}
