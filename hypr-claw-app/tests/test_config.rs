use std::fs;

#[test]
fn test_config_nvidia_provider() {
    let config_yaml = r#"
provider: nvidia
model: moonshotai/kimi-k2.5
"#;

    let config: hypr_claw_app::config::Config = serde_yaml::from_str(config_yaml).unwrap();

    match config.provider {
        hypr_claw_app::config::LLMProvider::Nvidia => {
            assert_eq!(
                config.provider.base_url(),
                "https://integrate.api.nvidia.com/v1"
            );
            assert!(config.provider.requires_api_key());
        }
        _ => panic!("Expected Nvidia provider"),
    }

    assert_eq!(config.model, "moonshotai/kimi-k2.5");
}

#[test]
fn test_config_google_provider() {
    let config_yaml = r#"
provider: google
model: gemini-2.5-pro
"#;

    let config: hypr_claw_app::config::Config = serde_yaml::from_str(config_yaml).unwrap();

    match config.provider {
        hypr_claw_app::config::LLMProvider::Google => {
            assert_eq!(
                config.provider.base_url(),
                "https://generativelanguage.googleapis.com/v1beta/openai"
            );
            assert!(config.provider.requires_api_key());
        }
        _ => panic!("Expected Google provider"),
    }

    assert_eq!(config.model, "gemini-2.5-pro");
}

#[test]
fn test_config_local_provider() {
    let config_yaml = r#"
provider: !local
  base_url: http://localhost:8080
model: llama3
"#;

    let config: hypr_claw_app::config::Config = serde_yaml::from_str(config_yaml).unwrap();

    match &config.provider {
        hypr_claw_app::config::LLMProvider::Local { base_url } => {
            assert_eq!(base_url, "http://localhost:8080");
            assert_eq!(config.provider.base_url(), "http://localhost:8080");
            assert!(!config.provider.requires_api_key());
        }
        _ => panic!("Expected Local provider"),
    }

    assert_eq!(config.model, "llama3");
}

#[test]
fn test_config_save_and_load() {
    let test_config_path = "./test_data/test_config.yaml";
    let _ = fs::remove_dir_all("./test_data");
    fs::create_dir_all("./test_data").unwrap();

    let config = hypr_claw_app::config::Config {
        provider: hypr_claw_app::config::LLMProvider::Nvidia,
        model: "test-model".to_string(),
    };

    let yaml = serde_yaml::to_string(&config).unwrap();
    fs::write(test_config_path, yaml).unwrap();

    let loaded: hypr_claw_app::config::Config =
        serde_yaml::from_str(&fs::read_to_string(test_config_path).unwrap()).unwrap();

    assert!(matches!(
        loaded.provider,
        hypr_claw_app::config::LLMProvider::Nvidia
    ));
    assert_eq!(loaded.model, "test-model");

    fs::remove_dir_all("./test_data").unwrap();
}

#[test]
fn test_config_validation() {
    let valid_config = hypr_claw_app::config::Config {
        provider: hypr_claw_app::config::LLMProvider::Nvidia,
        model: "test-model".to_string(),
    };
    assert!(valid_config.validate().is_ok());

    let invalid_config = hypr_claw_app::config::Config {
        provider: hypr_claw_app::config::LLMProvider::Nvidia,
        model: "".to_string(),
    };
    assert!(invalid_config.validate().is_err());

    let invalid_local = hypr_claw_app::config::Config {
        provider: hypr_claw_app::config::LLMProvider::Local {
            base_url: "".to_string(),
        },
        model: "test".to_string(),
    };
    assert!(invalid_local.validate().is_err());
}
