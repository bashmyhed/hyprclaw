use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const CONFIG_PATH: &str = "./data/config.yaml";

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub provider: LLMProvider,
    pub model: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LLMProvider {
    Nvidia,
    Google,
    #[serde(rename = "local")]
    Local {
        base_url: String,
    },
    Antigravity,
    #[serde(rename = "gemini-cli")]
    GeminiCli,
    Codex,
}

impl LLMProvider {
    pub fn base_url(&self) -> String {
        match self {
            LLMProvider::Nvidia => "https://integrate.api.nvidia.com/v1".to_string(),
            LLMProvider::Google => {
                "https://generativelanguage.googleapis.com/v1beta/openai".to_string()
            }
            LLMProvider::Local { base_url } => base_url.clone(),
            LLMProvider::Antigravity => {
                "https://daily-cloudcode-pa.sandbox.googleapis.com".to_string()
            }
            LLMProvider::GeminiCli => "https://cloudcode-pa.googleapis.com".to_string(),
            LLMProvider::Codex => "https://chatgpt.com/backend-api/codex".to_string(),
        }
    }

    pub fn requires_api_key(&self) -> bool {
        matches!(self, LLMProvider::Nvidia | LLMProvider::Google)
    }

    pub fn requires_oauth(&self) -> bool {
        matches!(
            self,
            LLMProvider::Antigravity | LLMProvider::GeminiCli | LLMProvider::Codex
        )
    }

    pub fn supports_function_calling(&self) -> bool {
        matches!(
            self,
            LLMProvider::Nvidia | LLMProvider::Google | LLMProvider::Local { .. }
        )
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let content = std::fs::read_to_string(CONFIG_PATH).context("Failed to read config.yaml")?;
        serde_yaml::from_str(&content).context("Failed to parse config.yaml")
    }

    pub fn save(&self) -> Result<()> {
        let content = serde_yaml::to_string(self)?;
        std::fs::write(CONFIG_PATH, content)?;
        Ok(())
    }

    pub fn exists() -> bool {
        Path::new(CONFIG_PATH).exists()
    }

    pub fn delete() -> Result<()> {
        if Self::exists() {
            std::fs::remove_file(CONFIG_PATH)?;
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if self.model.is_empty() {
            bail!("Model cannot be empty");
        }
        if let LLMProvider::Local { base_url } = &self.provider {
            if base_url.is_empty() {
                bail!("Base URL cannot be empty for local provider");
            }
        }
        Ok(())
    }
}
