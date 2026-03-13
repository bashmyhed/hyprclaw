use crate::infra::audit_logger::AuditLogger;
use crate::infra::contracts::AuditEntry;
use async_trait::async_trait;
use hypr_claw_tools::AuditLogger as AuditLoggerTrait;
use std::collections::HashMap;

#[async_trait]
impl AuditLoggerTrait for AuditLogger {
    async fn log(&self, entry: serde_json::Value) {
        // Convert JSON to AuditEntry
        let input_map: HashMap<String, serde_json::Value> = entry
            .get("input")
            .and_then(|v| v.as_object())
            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        let result_map: HashMap<String, serde_json::Value> = entry
            .get("result")
            .and_then(|v| v.as_object())
            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        let audit_entry = AuditEntry {
            timestamp: entry
                .get("timestamp")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            session: entry
                .get("session")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            tool: entry
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            input: input_map,
            result: result_map,
            approval: crate::infra::contracts::PermissionDecision::ALLOW,
        };

        let _ = self.log(&audit_entry);
    }
}
