use crate::infra::contracts::{PermissionDecision, PermissionLevel, PermissionRequest};

const BLOCKED_PATTERNS: &[&str] = &["sudo", "rm", "chmod", "curl|sh", "|sh"];

pub struct PermissionEngine;

impl Default for PermissionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PermissionEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn check(&self, request: &PermissionRequest) -> PermissionDecision {
        if self.contains_blocked_pattern(request) {
            return PermissionDecision::DENY;
        }

        match request.permission_level {
            PermissionLevel::SAFE => PermissionDecision::ALLOW,
            PermissionLevel::REQUIRE_APPROVAL => PermissionDecision::REQUIRE_APPROVAL,
            PermissionLevel::DANGEROUS => PermissionDecision::DENY,
        }
    }

    fn contains_blocked_pattern(&self, request: &PermissionRequest) -> bool {
        let tool_name_lower = request.tool_name.to_lowercase();

        for pattern in BLOCKED_PATTERNS {
            if tool_name_lower.contains(&pattern.to_lowercase()) {
                return true;
            }
        }

        for value in request.input.values() {
            if self.value_contains_pattern(value) {
                return true;
            }
        }

        false
    }

    fn value_contains_pattern(&self, value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(s) => {
                let s_lower = s.to_lowercase().replace(" ", "");
                BLOCKED_PATTERNS
                    .iter()
                    .any(|p| s_lower.contains(&p.to_lowercase()))
            }
            serde_json::Value::Array(arr) => arr.iter().any(|v| self.value_contains_pattern(v)),
            serde_json::Value::Object(obj) => obj.values().any(|v| self.value_contains_pattern(v)),
            _ => false,
        }
    }
}
