use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HeaderStyle {
    Antigravity,
    #[serde(rename = "gemini-cli")]
    GeminiCli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingTier {
    Minimal,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub actual_model: String,
    pub thinking_budget: Option<u32>,
    pub thinking_level: Option<String>,
    pub tier: Option<ThinkingTier>,
    pub is_thinking_model: bool,
    pub is_image_model: bool,
    pub quota_preference: HeaderStyle,
    pub explicit_quota: bool,
}

pub struct ModelResolver;

impl ModelResolver {
    /// Resolve model name with tier suffix to actual API model
    pub fn resolve(requested_model: &str) -> ResolvedModel {
        let is_antigravity = requested_model.starts_with("antigravity-");
        let model_without_quota = requested_model.trim_start_matches("antigravity-");

        let tier = Self::extract_tier(model_without_quota);
        let base_name = if let Some(_) = tier {
            Self::strip_tier_suffix(model_without_quota)
        } else {
            model_without_quota.to_string()
        };

        let is_image_model = base_name.contains("image") || base_name.contains("imagen");
        let is_claude_model = base_name.contains("claude");

        // Default to Antigravity quota
        let quota_preference = HeaderStyle::Antigravity;
        let explicit_quota = is_antigravity || is_image_model;

        let is_gemini3 = base_name.starts_with("gemini-3");
        let is_gemini3_pro =
            base_name.starts_with("gemini-3-pro") || base_name.starts_with("gemini-3.1-pro");

        // Apply model aliases
        let actual_model = if is_antigravity && is_gemini3_pro && tier.is_none() && !is_image_model
        {
            format!("{}-low", base_name)
        } else {
            Self::apply_alias(&base_name)
        };

        // Image models don't support thinking
        if is_image_model {
            return ResolvedModel {
                actual_model,
                thinking_budget: None,
                thinking_level: None,
                tier: None,
                is_thinking_model: false,
                is_image_model: true,
                quota_preference,
                explicit_quota,
            };
        }

        let is_thinking = Self::is_thinking_capable(&actual_model);
        let is_claude_thinking = is_claude_model && actual_model.contains("thinking");

        // Handle thinking configuration
        match tier {
            None => {
                if is_gemini3 {
                    ResolvedModel {
                        actual_model,
                        thinking_budget: None,
                        thinking_level: Some("low".to_string()),
                        tier: None,
                        is_thinking_model: true,
                        is_image_model: false,
                        quota_preference,
                        explicit_quota,
                    }
                } else if is_claude_thinking {
                    ResolvedModel {
                        actual_model,
                        thinking_budget: Some(32768),
                        thinking_level: None,
                        tier: None,
                        is_thinking_model: true,
                        is_image_model: false,
                        quota_preference,
                        explicit_quota,
                    }
                } else {
                    ResolvedModel {
                        actual_model,
                        thinking_budget: None,
                        thinking_level: None,
                        tier: None,
                        is_thinking_model: is_thinking,
                        is_image_model: false,
                        quota_preference,
                        explicit_quota,
                    }
                }
            }
            Some(t) => {
                if is_gemini3 {
                    ResolvedModel {
                        actual_model,
                        thinking_budget: None,
                        thinking_level: Some(Self::tier_to_string(t)),
                        tier: Some(t),
                        is_thinking_model: true,
                        is_image_model: false,
                        quota_preference,
                        explicit_quota,
                    }
                } else {
                    let budget = Self::tier_to_budget(t, &actual_model);
                    ResolvedModel {
                        actual_model,
                        thinking_budget: Some(budget),
                        thinking_level: None,
                        tier: Some(t),
                        is_thinking_model: is_thinking,
                        is_image_model: false,
                        quota_preference,
                        explicit_quota,
                    }
                }
            }
        }
    }

    fn extract_tier(model: &str) -> Option<ThinkingTier> {
        if !Self::supports_thinking_tiers(model) {
            return None;
        }

        if model.ends_with("-minimal") {
            Some(ThinkingTier::Minimal)
        } else if model.ends_with("-low") {
            Some(ThinkingTier::Low)
        } else if model.ends_with("-medium") {
            Some(ThinkingTier::Medium)
        } else if model.ends_with("-high") {
            Some(ThinkingTier::High)
        } else {
            None
        }
    }

    fn strip_tier_suffix(model: &str) -> String {
        model
            .trim_end_matches("-minimal")
            .trim_end_matches("-low")
            .trim_end_matches("-medium")
            .trim_end_matches("-high")
            .to_string()
    }

    fn supports_thinking_tiers(model: &str) -> bool {
        let lower = model.to_lowercase();
        lower.contains("gemini-3")
            || lower.contains("gemini-2.5")
            || (lower.contains("claude") && lower.contains("thinking"))
    }

    fn is_thinking_capable(model: &str) -> bool {
        let lower = model.to_lowercase();
        lower.contains("thinking") || lower.contains("gemini-3") || lower.contains("gemini-2.5")
    }

    fn tier_to_budget(tier: ThinkingTier, model: &str) -> u32 {
        if model.contains("claude") {
            match tier {
                ThinkingTier::Minimal => 4096,
                ThinkingTier::Low => 8192,
                ThinkingTier::Medium => 16384,
                ThinkingTier::High => 32768,
            }
        } else if model.contains("gemini-2.5-pro") {
            match tier {
                ThinkingTier::Minimal => 4096,
                ThinkingTier::Low => 8192,
                ThinkingTier::Medium => 16384,
                ThinkingTier::High => 32768,
            }
        } else if model.contains("gemini-2.5-flash") {
            match tier {
                ThinkingTier::Minimal => 3072,
                ThinkingTier::Low => 6144,
                ThinkingTier::Medium => 12288,
                ThinkingTier::High => 24576,
            }
        } else {
            match tier {
                ThinkingTier::Minimal => 2048,
                ThinkingTier::Low => 4096,
                ThinkingTier::Medium => 8192,
                ThinkingTier::High => 16384,
            }
        }
    }

    fn tier_to_string(tier: ThinkingTier) -> String {
        match tier {
            ThinkingTier::Minimal => "minimal",
            ThinkingTier::Low => "low",
            ThinkingTier::Medium => "medium",
            ThinkingTier::High => "high",
        }
        .to_string()
    }

    fn apply_alias(model: &str) -> String {
        // Model aliases from model-resolver.ts
        match model {
            "gemini-3-pro-low" => "gemini-3-pro",
            "gemini-3-pro-high" => "gemini-3-pro",
            "gemini-3.1-pro-low" => "gemini-3.1-pro",
            "gemini-3.1-pro-high" => "gemini-3.1-pro",
            "gemini-3-flash-low" => "gemini-3-flash",
            "gemini-3-flash-medium" => "gemini-3-flash",
            "gemini-3-flash-high" => "gemini-3-flash",
            "gemini-claude-opus-4-6-thinking-low" => "claude-opus-4-6-thinking",
            "gemini-claude-opus-4-6-thinking-medium" => "claude-opus-4-6-thinking",
            "gemini-claude-opus-4-6-thinking-high" => "claude-opus-4-6-thinking",
            "gemini-claude-sonnet-4-6" => "claude-sonnet-4-6",
            _ => model,
        }
        .to_string()
    }

    pub fn get_model_family(model: &str) -> &'static str {
        let lower = model.to_lowercase();
        if lower.contains("claude") {
            "claude"
        } else if lower.contains("flash") {
            "gemini-flash"
        } else {
            "gemini-pro"
        }
    }
}
