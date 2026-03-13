use crate::error::ToolError;
use crate::execution_context::ExecutionContext;
use crate::registry::{HiddenToolDoc, ToolRegistryImpl};
use crate::tools::{Tool, ToolResult};
use crate::traits::PermissionTier;
use async_trait::async_trait;
use regex::RegexBuilder;
use serde::Deserialize;
use serde_json::json;
const DEFAULT_LIMIT: usize = 5;
const DEFAULT_PROMOTION_TTL: usize = 3;

#[derive(Clone)]
pub struct HiddenToolSearchRegexTool {
    registry: ToolRegistryImpl,
}

#[derive(Clone)]
pub struct HiddenToolSearchBm25Tool {
    registry: ToolRegistryImpl,
}

#[derive(Deserialize)]
struct RegexSearchInput {
    pattern: String,
    #[serde(default)]
    case_sensitive: bool,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    promote_ttl: Option<usize>,
}

#[derive(Deserialize)]
struct Bm25SearchInput {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    promote_ttl: Option<usize>,
}

impl HiddenToolSearchRegexTool {
    pub fn new(registry: ToolRegistryImpl) -> Self {
        Self { registry }
    }
}

impl HiddenToolSearchBm25Tool {
    pub fn new(registry: ToolRegistryImpl) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for HiddenToolSearchRegexTool {
    fn name(&self) -> &'static str {
        "tools.search_regex"
    }

    fn description(&self) -> &'static str {
        "Search hidden tools with a regex and temporarily promote matches"
    }

    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "case_sensitive": {"type": "boolean"},
                "limit": {"type": "integer", "minimum": 1},
                "promote_ttl": {"type": "integer", "minimum": 1}
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        _ctx: ExecutionContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let input: RegexSearchInput =
            serde_json::from_value(input).map_err(|e| ToolError::ValidationError(e.to_string()))?;
        let regex = RegexBuilder::new(&input.pattern)
            .case_insensitive(!input.case_sensitive)
            .build()
            .map_err(|e| ToolError::ValidationError(format!("Invalid regex pattern: {e}")))?;

        let limit = input.limit.unwrap_or(DEFAULT_LIMIT).min(20);
        let ttl = input.promote_ttl.unwrap_or(DEFAULT_PROMOTION_TTL).min(10);
        let snapshot = self.registry.snapshot_hidden_tools();
        let matches = snapshot
            .docs
            .into_iter()
            .filter(|doc| regex.is_match(&format!("{} {}", doc.name, doc.description)))
            .take(limit)
            .collect::<Vec<_>>();

        promote_matches(&self.registry, &matches, ttl);
        Ok(search_result(
            "regex",
            ttl,
            matches,
            Some(format!("Promoted hidden tools for {ttl} turn(s)")),
        ))
    }
}

#[async_trait]
impl Tool for HiddenToolSearchBm25Tool {
    fn name(&self) -> &'static str {
        "tools.search_bm25"
    }

    fn description(&self) -> &'static str {
        "Search hidden tools by keyword relevance and temporarily promote matches"
    }

    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1},
                "promote_ttl": {"type": "integer", "minimum": 1}
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        _ctx: ExecutionContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let input: Bm25SearchInput =
            serde_json::from_value(input).map_err(|e| ToolError::ValidationError(e.to_string()))?;
        let limit = input.limit.unwrap_or(DEFAULT_LIMIT).min(20);
        let ttl = input.promote_ttl.unwrap_or(DEFAULT_PROMOTION_TTL).min(10);
        let tokens = input
            .query
            .split_whitespace()
            .map(|token| token.to_lowercase())
            .collect::<Vec<_>>();
        if tokens.is_empty() {
            return Err(ToolError::ValidationError(
                "query must contain at least one token".to_string(),
            ));
        }

        let mut scored = self
            .registry
            .snapshot_hidden_tools()
            .docs
            .into_iter()
            .map(|doc| {
                let haystack = format!("{} {}", doc.name, doc.description).to_lowercase();
                let score = tokens
                    .iter()
                    .filter(|token| haystack.contains(token.as_str()))
                    .count();
                (score, doc)
            })
            .filter(|(score, _)| *score > 0)
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        let matches = scored
            .into_iter()
            .take(limit)
            .map(|(_, doc)| doc)
            .collect::<Vec<_>>();

        promote_matches(&self.registry, &matches, ttl);
        Ok(search_result(
            "bm25",
            ttl,
            matches,
            Some(format!("Promoted hidden tools for {ttl} turn(s)")),
        ))
    }
}

fn promote_matches(registry: &ToolRegistryImpl, matches: &[HiddenToolDoc], ttl: usize) {
    let names = matches.iter().map(|doc| doc.name.clone()).collect::<Vec<_>>();
    if !names.is_empty() {
        registry.promote_tools(&names, ttl);
    }
}

fn search_result(
    strategy: &str,
    ttl: usize,
    matches: Vec<HiddenToolDoc>,
    user_message: Option<String>,
) -> ToolResult {
    ToolResult {
        success: true,
        output: Some(json!({
            "strategy": strategy,
            "promotion_ttl": ttl,
            "matches": matches.iter().map(|doc| json!({
                "name": doc.name,
                "description": doc.description
            })).collect::<Vec<_>>()
        })),
        for_user: user_message,
        ..ToolResult::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::EchoTool;
    use serde_json::json;
    use std::sync::Arc;

    fn test_ctx() -> ExecutionContext {
        ExecutionContext::new("test-session".to_string(), 5_000)
    }

    #[tokio::test]
    async fn regex_search_promotes_matching_hidden_tools() {
        let mut registry = ToolRegistryImpl::new();
        registry.register_hidden(Arc::new(EchoTool));
        let search = HiddenToolSearchRegexTool::new(registry.clone());

        let result = search
            .execute(test_ctx(), json!({"pattern": "echo"}))
            .await
            .unwrap();

        assert!(result.is_effective_success());
        assert!(registry.get("echo").is_some());
    }

    #[tokio::test]
    async fn bm25_search_returns_matching_hidden_tools() {
        let mut registry = ToolRegistryImpl::new();
        registry.register_hidden(Arc::new(EchoTool));
        let search = HiddenToolSearchBm25Tool::new(registry.clone());

        let result = search
            .execute(test_ctx(), json!({"query": "input echo"}))
            .await
            .unwrap();

        let matches = result.output.unwrap()["matches"].as_array().unwrap().clone();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0]["name"], "echo");
    }
}
