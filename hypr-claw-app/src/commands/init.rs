use anyhow::{Result, Context, bail};
use dialoguer::{Input, Select};
use std::path::PathBuf;
use crate::config::{Config, LlmProvider};

pub async fn run() -> Result<()> {
    println!("🚀 Hypr-Claw Initialization\n");

    // Select LLM provider
    let providers = vec!["nvidia_kiro", "openai", "custom"];
    let selection = Select::new()
        .with_prompt("Select LLM provider")
        .items(&providers)
        .default(0)
        .interact()?;

    let llm_provider = match selection {
        0 => {
            let model: String = Input::new()
                .with_prompt("Model name")
                .default("meta/llama-3.1-8b-instruct".to_string())
                .interact_text()?;
            LlmProvider::NvidiaKiro { model }
        }
        1 => {
            let model: String = Input::new()
                .with_prompt("Model name")
                .default("gpt-4".to_string())
                .interact_text()?;
            LlmProvider::OpenAI { model }
        }
        2 => {
            let endpoint: String = Input::new()
                .with_prompt("API endpoint")
                .interact_text()?;
            let model: String = Input::new()
                .with_prompt("Model name")
                .interact_text()?;
            LlmProvider::Custom { endpoint, model }
        }
        _ => unreachable!(),
    };

    // Get API key
    let api_key: String = Input::new()
        .with_prompt("API key")
        .interact_text()?;

    // Get default agent name
    let agent_name: String = Input::new()
        .with_prompt("Default agent name")
        .default("assistant".to_string())
        .interact_text()?;

    // Create directories
    println!("\n📁 Creating directories...");
    std::fs::create_dir_all("./sessions")?;
    std::fs::create_dir_all("./agents")?;
    std::fs::create_dir_all("./.credentials")?;

    // Create audit log
    std::fs::write("./audit.log", "")?;

    // Create config
    let config = Config {
        llm_provider: llm_provider.clone(),
        default_agent: agent_name.clone(),
        sessions_dir: PathBuf::from("./sessions"),
        agents_dir: PathBuf::from("./agents"),
        audit_log: PathBuf::from("./audit.log"),
    };
    config.save()?;

    // Store API key in encrypted credential store
    println!("🔐 Storing API key...");
    let key = generate_master_key();
    let cred_store = hypr_claw::infra::credential_store::CredentialStore::new(
        "./.credentials",
        &key,
    )?;
    cred_store.store_secret("llm_api_key", &api_key)?;

    // Save master key to file (in production, this should be handled differently)
    std::fs::write("./.credentials/master.key", key)?;

    // Create default agent
    println!("🤖 Creating agent: {}", agent_name);
    let agent_config = format!(
        "id: {}\nsoul: {}.soul\ntools:\n  - echo\n  - file.read\n  - file.write\n  - file.list\n  - shell.exec\n",
        agent_name, agent_name
    );
    std::fs::write(format!("./agents/{}.yaml", agent_name), agent_config)?;

    let soul = "You are a helpful AI assistant with access to tools. Use them when appropriate to help the user.";
    std::fs::write(format!("./agents/{}.soul", agent_name), soul)?;

    // Validate LLM endpoint
    println!("🔍 Validating LLM endpoint...");
    if let Err(e) = validate_llm_endpoint(&llm_provider, &api_key).await {
        bail!("LLM endpoint validation failed: {}", e);
    }

    println!("\n✅ Initialization complete!");
    println!("\nNext steps:");
    println!("  cargo run -- run --agent {} --user user1 --message \"hello\"", agent_name);

    Ok(())
}

fn generate_master_key() -> [u8; 32] {
    use rand::RngCore;
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    key
}

async fn validate_llm_endpoint(provider: &LlmProvider, api_key: &str) -> Result<()> {
    #[cfg(feature = "mock-llm")]
    {
        println!("  Mock LLM mode - skipping validation");
        return Ok(());
    }

    let client = reqwest::Client::new();
    let endpoint = provider.endpoint();
    
    let response = client
        .post(&endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": provider.model(),
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 1
        }))
        .send()
        .await
        .context("Failed to connect to LLM endpoint")?;

    if !response.status().is_success() && response.status().as_u16() != 401 {
        bail!("LLM endpoint returned error: {}", response.status());
    }

    println!("  ✓ Endpoint reachable");
    Ok(())
}
