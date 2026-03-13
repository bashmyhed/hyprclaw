use hypr_claw_app::config::{Config, LLMProvider};

fn main() {
    let nvidia_config = Config {
        provider: LLMProvider::Nvidia,
        model: "test".to_string(),
    };

    let local_config = Config {
        provider: LLMProvider::Local {
            base_url: "http://localhost:8080".to_string(),
        },
        model: "test".to_string(),
    };

    println!("Nvidia YAML:");
    println!("{}", serde_yaml::to_string(&nvidia_config).unwrap());

    println!("Local YAML:");
    println!("{}", serde_yaml::to_string(&local_config).unwrap());
}
