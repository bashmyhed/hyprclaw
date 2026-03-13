use crate::infra::permission_engine::PermissionEngine;
use async_trait::async_trait;
use hypr_claw_tools::{
    PermissionDecision, PermissionEngine as PermissionEngineTrait, PermissionRequest,
    PermissionTier,
};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;
use tokio::time::{timeout, Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApprovalReview {
    permission_level: crate::infra::contracts::PermissionLevel,
    risk_label: &'static str,
    reasons: Vec<String>,
    input_summary: String,
}

#[async_trait]
impl PermissionEngineTrait for PermissionEngine {
    async fn check(&self, request: PermissionRequest) -> PermissionDecision {
        let review = classify_permission_request(&request);

        // Convert to infra types
        let input_map: HashMap<String, serde_json::Value> = request
            .input
            .as_object()
            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        let infra_request = crate::infra::contracts::PermissionRequest {
            session_key: request.session_key,
            tool_name: request.tool_name,
            input: input_map,
            permission_level: review.permission_level,
        };

        let decision = self.check(&infra_request);

        match decision {
            crate::infra::contracts::PermissionDecision::ALLOW => PermissionDecision::Allow,
            crate::infra::contracts::PermissionDecision::DENY => {
                PermissionDecision::Deny("Permission denied".to_string())
            }
            crate::infra::contracts::PermissionDecision::REQUIRE_APPROVAL => {
                if is_full_auto_mode_enabled() {
                    return PermissionDecision::Allow;
                }
                if prompt_user_approval(&infra_request.tool_name, &review).await {
                    PermissionDecision::Allow
                } else {
                    PermissionDecision::Deny("Approval denied or timed out".to_string())
                }
            }
        }
    }
}

fn is_full_auto_mode_enabled() -> bool {
    Path::new("./data/full_auto_mode.flag").exists()
}

fn classify_permission_request(request: &PermissionRequest) -> ApprovalReview {
    use crate::infra::contracts::PermissionLevel;

    let mut permission_level = match request.permission_tier {
        PermissionTier::Read => PermissionLevel::SAFE,
        PermissionTier::Write => PermissionLevel::SAFE,
        PermissionTier::Execute => PermissionLevel::SAFE,
        PermissionTier::SystemCritical => PermissionLevel::REQUIRE_APPROVAL,
    };
    let mut reasons = Vec::new();

    let tool_name = request.tool_name.as_str();

    if matches!(request.permission_tier, PermissionTier::SystemCritical) {
        reasons.push("system-critical operation".to_string());
    }

    match tool_name {
        "browser.action" => {
            let kind = request
                .input
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let risky_kind = matches!(
                kind,
                "type" | "fill" | "press" | "click" | "select" | "upload" | "download"
            );
            if risky_kind {
                permission_level = PermissionLevel::REQUIRE_APPROVAL;
                reasons.push(format!(
                    "browser action `{kind}` can submit or modify web content"
                ));
            }
            if input_contains_action_words(&request.input) {
                permission_level = PermissionLevel::REQUIRE_APPROVAL;
                reasons.push("browser action looks like send/submit/delete/confirm".to_string());
            }
        }
        "desktop.type_text" => {
            permission_level = PermissionLevel::REQUIRE_APPROVAL;
            reasons.push("typing text can send messages or alter app state".to_string());
        }
        "desktop.key_press" => {
            let key = request
                .input
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if matches!(key.as_str(), "return" | "enter" | "delete" | "backspace") {
                permission_level = PermissionLevel::REQUIRE_APPROVAL;
                reasons.push(format!("key press `{key}` can confirm or delete content"));
            }
        }
        "desktop.key_combo" => {
            let keys = request
                .input
                .get("keys")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|value| value.as_str())
                        .map(|value| value.to_ascii_lowercase())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let joined = keys.join("+");
            if joined.contains("enter")
                || joined.contains("return")
                || joined.contains("delete")
                || joined.contains("backspace")
                || joined.contains("alt+f4")
            {
                permission_level = PermissionLevel::REQUIRE_APPROVAL;
                reasons.push(format!(
                    "key combo `{joined}` can submit, delete, or close state"
                ));
            }
        }
        "fs.write" => {
            if let Some(path) = request.input.get("path").and_then(|v| v.as_str()) {
                if is_sensitive_path(path) {
                    permission_level = PermissionLevel::REQUIRE_APPROVAL;
                    reasons.push(format!(
                        "write targets sensitive path `{}`",
                        truncate(path, 64)
                    ));
                }
            } else {
                permission_level = PermissionLevel::REQUIRE_APPROVAL;
                reasons.push("filesystem write without path context".to_string());
            }
        }
        "proc.kill" | "system.shutdown" | "system.reboot" | "hypr.exec" => {
            permission_level = PermissionLevel::REQUIRE_APPROVAL;
            reasons.push("process or system state change".to_string());
        }
        _ => {}
    }

    if matches!(
        request.permission_tier,
        PermissionTier::Write | PermissionTier::Execute
    ) && input_contains_sensitive_terms(&request.input)
    {
        permission_level = PermissionLevel::REQUIRE_APPROVAL;
        reasons.push("input references messaging, credentials, install, or deletion".to_string());
    }

    let risk_label = match permission_level {
        PermissionLevel::SAFE => "low",
        PermissionLevel::REQUIRE_APPROVAL => "high",
        PermissionLevel::DANGEROUS => "critical",
    };

    ApprovalReview {
        permission_level,
        risk_label,
        reasons,
        input_summary: summarize_value(&request.input, 160),
    }
}

async fn prompt_user_approval(tool_name: &str, review: &ApprovalReview) -> bool {
    println!();
    println!("Approval Required");
    println!("  Risk    : {}", review.risk_label);
    println!("  Tool    : {}", tool_name);
    if review.reasons.is_empty() {
        println!("  Why     : manual confirmation requested");
    } else {
        println!("  Why     : {}", review.reasons.join("; "));
    }
    println!("  Input   : {}", review.input_summary);
    println!("  Timeout : 30s");
    print!("Approve this action? [y/N] ");
    let _ = io::stdout().flush();

    let task = tokio::task::spawn_blocking(|| {
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        input
    });

    match timeout(Duration::from_secs(30), task).await {
        Ok(Ok(input)) => input.trim().eq_ignore_ascii_case("y"),
        _ => false,
    }
}

fn input_contains_sensitive_terms(value: &serde_json::Value) -> bool {
    const TERMS: &[&str] = &[
        "password", "token", "otp", "secret", "login", "sign in", "send", "submit", "delete",
        "remove", "install", "message", "email", "whatsapp", "telegram", "gmail",
    ];
    match value {
        serde_json::Value::String(s) => {
            let lowered = s.to_ascii_lowercase();
            TERMS.iter().any(|term| lowered.contains(term))
        }
        serde_json::Value::Array(items) => items.iter().any(input_contains_sensitive_terms),
        serde_json::Value::Object(map) => map.values().any(input_contains_sensitive_terms),
        _ => false,
    }
}

fn input_contains_action_words(value: &serde_json::Value) -> bool {
    const TERMS: &[&str] = &[
        "send", "submit", "delete", "remove", "confirm", "purchase", "pay",
    ];
    match value {
        serde_json::Value::String(s) => {
            let lowered = s.to_ascii_lowercase();
            TERMS.iter().any(|term| lowered.contains(term))
        }
        serde_json::Value::Array(items) => items.iter().any(input_contains_action_words),
        serde_json::Value::Object(map) => map.values().any(input_contains_action_words),
        _ => false,
    }
}

fn is_sensitive_path(path: &str) -> bool {
    let lowered = path.to_ascii_lowercase();
    [
        "/.ssh/",
        "/.gnupg/",
        "/.config/",
        "/.local/share/keyrings",
        ".env",
        "id_rsa",
        "id_ed25519",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn summarize_value(value: &serde_json::Value, max: usize) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(v) => v.to_string(),
        serde_json::Value::Number(v) => v.to_string(),
        serde_json::Value::String(v) => truncate(&v.replace('\n', " "), max),
        serde_json::Value::Array(items) => truncate(
            &items
                .iter()
                .take(4)
                .map(|item| summarize_value(item, max / 2))
                .collect::<Vec<_>>()
                .join(", "),
            max,
        ),
        serde_json::Value::Object(map) => truncate(
            &map.iter()
                .take(6)
                .map(|(key, item)| format!("{key}={}", summarize_value(item, max / 2)))
                .collect::<Vec<_>>()
                .join(" "),
            max,
        ),
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let mut out = value
            .chars()
            .take(max.saturating_sub(3))
            .collect::<String>();
        out.push_str("...");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(
        tool_name: &str,
        input: serde_json::Value,
        permission_tier: PermissionTier,
    ) -> PermissionRequest {
        PermissionRequest {
            session_key: "s".to_string(),
            tool_name: tool_name.to_string(),
            input,
            permission_tier,
            timestamp: "2026-03-13T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn browser_action_typing_requires_approval() {
        let review = classify_permission_request(&request(
            "browser.action",
            json!({"kind": "type", "selector": "#composer", "value": "hello"}),
            PermissionTier::Execute,
        ));
        assert_eq!(
            review.permission_level,
            crate::infra::contracts::PermissionLevel::REQUIRE_APPROVAL
        );
    }

    #[test]
    fn read_only_screen_observation_stays_safe() {
        let review = classify_permission_request(&request(
            "desktop.read_screen_state",
            json!({"include_ocr": false}),
            PermissionTier::Read,
        ));
        assert_eq!(
            review.permission_level,
            crate::infra::contracts::PermissionLevel::SAFE
        );
    }

    #[test]
    fn sensitive_file_write_requires_approval() {
        let review = classify_permission_request(&request(
            "fs.write",
            json!({"path": "/home/rick/.ssh/config", "content": "Host demo"}),
            PermissionTier::Write,
        ));
        assert_eq!(
            review.permission_level,
            crate::infra::contracts::PermissionLevel::REQUIRE_APPROVAL
        );
    }
}
