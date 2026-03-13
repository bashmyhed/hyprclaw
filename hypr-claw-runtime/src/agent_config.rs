//! Agent configuration loader.

use crate::interfaces::RuntimeError;
use serde::Deserialize;
use std::path::Path;

/// Agent configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub soul: String,
    pub tools: Vec<String>,
}

/// Raw config structure from YAML.
#[derive(Debug, Deserialize)]
struct RawConfig {
    id: String,
    soul: String,
    #[serde(default)]
    tools: Vec<String>,
}

/// Load agent configuration from YAML file.
///
/// # Arguments
/// * `config_path` - Path to agent YAML config file
///
/// # Returns
/// AgentConfig with soul content loaded from markdown file
///
/// # Errors
/// Returns error if config file or soul file not found, or if config is invalid
pub fn load_agent_config(config_path: &str) -> Result<AgentConfig, RuntimeError> {
    let config_file = Path::new(config_path);

    if !config_file.exists() {
        return Err(RuntimeError::ConfigError(format!(
            "Config file not found: {}",
            config_path
        )));
    }

    let content = std::fs::read_to_string(config_file)?;

    if content.trim().is_empty() {
        return Err(RuntimeError::ConfigError(
            "Config file is empty".to_string(),
        ));
    }

    let raw_config: RawConfig = serde_yaml::from_str(&content)
        .map_err(|e| RuntimeError::ConfigError(format!("Invalid YAML: {}", e)))?;

    if raw_config.id.is_empty() {
        return Err(RuntimeError::ConfigError(
            "Config missing required field: id".to_string(),
        ));
    }

    if raw_config.soul.is_empty() {
        return Err(RuntimeError::ConfigError(
            "Config missing required field: soul".to_string(),
        ));
    }

    // Resolve soul file path relative to config file
    let soul_path = if Path::new(&raw_config.soul).is_absolute() {
        Path::new(&raw_config.soul).to_path_buf()
    } else {
        config_file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&raw_config.soul)
    };

    if !soul_path.exists() {
        return Err(RuntimeError::ConfigError(format!(
            "Soul file not found: {}",
            soul_path.display()
        )));
    }

    let soul_content = std::fs::read_to_string(&soul_path)?;

    Ok(AgentConfig {
        id: raw_config.id,
        soul: soul_content,
        tools: raw_config.tools,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_load_valid_config() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create soul file
        let soul_file = temp_path.join("agent.md");
        fs::write(&soul_file, "You are a helpful assistant.").unwrap();

        // Create config file
        let config_file = temp_path.join("agent.yaml");
        fs::write(
            &config_file,
            "id: test_agent\nsoul: agent.md\ntools:\n  - search\n  - calculator\n",
        )
        .unwrap();

        let config = load_agent_config(config_file.to_str().unwrap()).unwrap();
        assert_eq!(config.id, "test_agent");
        assert_eq!(config.soul, "You are a helpful assistant.");
        assert_eq!(config.tools, vec!["search", "calculator"]);
    }

    #[test]
    fn test_load_config_without_tools() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let soul_file = temp_path.join("agent.md");
        fs::write(&soul_file, "Assistant soul").unwrap();

        let config_file = temp_path.join("agent.yaml");
        fs::write(&config_file, "id: minimal_agent\nsoul: agent.md\n").unwrap();

        let config = load_agent_config(config_file.to_str().unwrap()).unwrap();
        assert_eq!(config.id, "minimal_agent");
        assert!(config.tools.is_empty());
    }

    #[test]
    fn test_config_file_not_found() {
        let result = load_agent_config("/nonexistent/config.yaml");
        assert!(result.is_err());
        match result {
            Err(RuntimeError::ConfigError(msg)) => {
                assert!(msg.contains("Config file not found"));
            }
            _ => panic!("Expected ConfigError"),
        }
    }

    #[test]
    fn test_soul_file_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let config_file = temp_path.join("agent.yaml");
        fs::write(&config_file, "id: test_agent\nsoul: missing.md\n").unwrap();

        let result = load_agent_config(config_file.to_str().unwrap());
        assert!(result.is_err());
        match result {
            Err(RuntimeError::ConfigError(msg)) => {
                assert!(msg.contains("Soul file not found"));
            }
            _ => panic!("Expected ConfigError"),
        }
    }

    #[test]
    fn test_empty_config_file() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let config_file = temp_path.join("agent.yaml");
        fs::write(&config_file, "").unwrap();

        let result = load_agent_config(config_file.to_str().unwrap());
        assert!(result.is_err());
        match result {
            Err(RuntimeError::ConfigError(msg)) => {
                assert!(msg.contains("empty"));
            }
            _ => panic!("Expected ConfigError"),
        }
    }

    #[test]
    fn test_missing_id_field() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let soul_file = temp_path.join("agent.md");
        fs::write(&soul_file, "Soul content").unwrap();

        let config_file = temp_path.join("agent.yaml");
        fs::write(&config_file, "soul: agent.md\n").unwrap();

        let result = load_agent_config(config_file.to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_soul_field() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let config_file = temp_path.join("agent.yaml");
        fs::write(&config_file, "id: test_agent\n").unwrap();

        let result = load_agent_config(config_file.to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_absolute_soul_path() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let soul_dir = temp_path.join("soul");
        fs::create_dir(&soul_dir).unwrap();
        let soul_file = soul_dir.join("agent.md");
        fs::write(&soul_file, "Absolute path soul").unwrap();

        let config_dir = temp_path.join("config");
        fs::create_dir(&config_dir).unwrap();
        let config_file = config_dir.join("agent.yaml");
        fs::write(
            &config_file,
            format!("id: abs_agent\nsoul: {}\n", soul_file.to_str().unwrap()),
        )
        .unwrap();

        let config = load_agent_config(config_file.to_str().unwrap()).unwrap();
        assert_eq!(config.soul, "Absolute path soul");
    }
}
