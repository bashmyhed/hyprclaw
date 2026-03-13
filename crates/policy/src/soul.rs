use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SoulError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("Soul not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Soul {
    pub id: String,
    pub config: SoulConfig,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoulConfig {
    pub allowed_tools: Vec<String>,
    pub autonomy_mode: AutonomyMode,
    pub max_iterations: usize,
    pub risk_tolerance: RiskTolerance,
    pub verbosity: VerbosityLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutonomyMode {
    Auto,
    Confirm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskTolerance {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerbosityLevel {
    Minimal,
    Normal,
    Verbose,
}

impl Soul {
    pub async fn load<P: AsRef<Path>>(path: P) -> Result<Self, SoulError> {
        let content = tokio::fs::read_to_string(&path).await?;
        let soul: Soul = serde_yaml::from_str(&content)?;
        Ok(soul)
    }

    pub async fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), SoulError> {
        let content = serde_yaml::to_string(self)?;
        tokio::fs::write(path, content).await?;
        Ok(())
    }
}

impl Default for SoulConfig {
    fn default() -> Self {
        Self {
            allowed_tools: vec![
                "echo".to_string(),
                "file_read".to_string(),
                "file_write".to_string(),
            ],
            autonomy_mode: AutonomyMode::Confirm,
            max_iterations: 10,
            risk_tolerance: RiskTolerance::Medium,
            verbosity: VerbosityLevel::Normal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_soul_serialization() {
        let soul = Soul {
            id: "test".to_string(),
            config: SoulConfig::default(),
            system_prompt: "You are a helpful assistant.".to_string(),
        };

        let yaml = serde_yaml::to_string(&soul).unwrap();
        let deserialized: Soul = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(soul.id, deserialized.id);
        assert_eq!(soul.system_prompt, deserialized.system_prompt);
    }
}
