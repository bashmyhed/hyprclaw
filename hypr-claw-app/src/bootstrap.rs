use crate::config::{Config, LLMProvider};
use anyhow::{Context, Result};
use std::io::{self, Write};

const NVIDIA_API_KEY_NAME: &str = "llm/nvidia_api_key";
const GOOGLE_API_KEY_NAME: &str = "llm/google_api_key";

pub fn run_bootstrap() -> Result<Config> {
    println!("\nNo LLM provider configured.");
    println!("Select provider:");
    println!("1. NVIDIA Kimi");
    println!("2. Google Gemini");
    println!("3. Local model");
    println!("4. Antigravity (Claude + Gemini via Google OAuth)");
    println!("5. Gemini CLI (Gemini via Google OAuth)");
    println!("6. OpenAI Codex (ChatGPT Plus/Pro via OAuth)");
    print!("\nChoice [1-6]: ");
    io::stdout().flush()?;

    let mut choice = String::new();
    io::stdin().read_line(&mut choice)?;
    let choice = choice.trim();

    match choice {
        "1" => bootstrap_nvidia(),
        "2" => bootstrap_google(),
        "3" => bootstrap_local(),
        "4" => bootstrap_antigravity(),
        "5" => bootstrap_gemini_cli(),
        "6" => bootstrap_codex(),
        _ => {
            anyhow::bail!("Invalid choice. Please select 1-6.");
        }
    }
}

fn bootstrap_nvidia() -> Result<Config> {
    println!("\nEnter NVIDIA API key:");
    let api_key = rpassword::read_password().context("Failed to read API key")?;

    if api_key.trim().is_empty() {
        anyhow::bail!("API key cannot be empty");
    }

    // Store encrypted credential
    let master_key = get_or_create_master_key()?;
    let cred_store = hypr_claw::infra::credential_store::CredentialStore::new(
        "./data/credentials",
        &master_key,
    )?;

    cred_store.store_secret(NVIDIA_API_KEY_NAME, api_key.trim())?;

    let config = Config {
        provider: LLMProvider::Nvidia,
        model: "z-ai/glm4.7".to_string(),
    };

    config.save()?;
    println!("✅ NVIDIA provider configured");

    Ok(config)
}

fn bootstrap_google() -> Result<Config> {
    println!("\nEnter Google API key:");
    let api_key = rpassword::read_password().context("Failed to read API key")?;

    if api_key.trim().is_empty() {
        anyhow::bail!("API key cannot be empty");
    }

    // Store encrypted credential
    let master_key = get_or_create_master_key()?;
    let cred_store = hypr_claw::infra::credential_store::CredentialStore::new(
        "./data/credentials",
        &master_key,
    )?;

    cred_store.store_secret(GOOGLE_API_KEY_NAME, api_key.trim())?;

    let config = Config {
        provider: LLMProvider::Google,
        model: "gemini-2.5-flash".to_string(),
    };

    config.save()?;
    println!("✅ Google provider configured");

    Ok(config)
}

fn bootstrap_local() -> Result<Config> {
    print!("\nEnter local LLM base URL: ");
    io::stdout().flush()?;

    let mut base_url = String::new();
    io::stdin().read_line(&mut base_url)?;
    let base_url = base_url.trim().to_string();

    if base_url.is_empty() {
        anyhow::bail!("Base URL cannot be empty");
    }

    let config = Config {
        provider: LLMProvider::Local { base_url },
        model: "default".to_string(),
    };

    config.save()?;
    println!("✅ Local provider configured");

    Ok(config)
}

pub fn get_nvidia_api_key() -> Result<String> {
    let master_key = get_or_create_master_key()?;
    let cred_store = hypr_claw::infra::credential_store::CredentialStore::new(
        "./data/credentials",
        &master_key,
    )?;

    cred_store
        .get_secret(NVIDIA_API_KEY_NAME)
        .context("NVIDIA API key not found. Run bootstrap again.")
}

pub fn get_google_api_key() -> Result<String> {
    let master_key = get_or_create_master_key()?;
    let cred_store = hypr_claw::infra::credential_store::CredentialStore::new(
        "./data/credentials",
        &master_key,
    )?;

    cred_store
        .get_secret(GOOGLE_API_KEY_NAME)
        .context("Google API key not found. Run bootstrap again.")
}

pub fn delete_nvidia_api_key() -> Result<()> {
    let master_key = get_or_create_master_key()?;
    let cred_store = hypr_claw::infra::credential_store::CredentialStore::new(
        "./data/credentials",
        &master_key,
    )?;

    cred_store.delete_secret(NVIDIA_API_KEY_NAME)?;
    Ok(())
}

pub fn delete_google_api_key() -> Result<()> {
    let master_key = get_or_create_master_key()?;
    let cred_store = hypr_claw::infra::credential_store::CredentialStore::new(
        "./data/credentials",
        &master_key,
    )?;

    cred_store.delete_secret(GOOGLE_API_KEY_NAME)?;
    Ok(())
}

fn get_or_create_master_key() -> Result<[u8; 32]> {
    let key_path = "./data/.master_key";

    if std::path::Path::new(key_path).exists() {
        let key_bytes = std::fs::read(key_path)?;
        if key_bytes.len() != 32 {
            anyhow::bail!("Invalid master key length");
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&key_bytes);
        Ok(key)
    } else {
        use rand::RngCore;
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        std::fs::write(key_path, key)?;
        Ok(key)
    }
}

fn bootstrap_antigravity() -> Result<Config> {
    println!("\n🔐 Antigravity OAuth Setup");
    println!("This will open a browser for Google authentication.");
    println!("You'll get access to Claude Opus 4.6 and Gemini 3 models.");
    println!("\nPress Enter to continue...");
    let mut _input = String::new();
    io::stdin().read_line(&mut _input)?;

    // Check if accounts already exist
    let accounts_path = "./data/antigravity-accounts.json";
    if std::path::Path::new(accounts_path).exists() {
        println!("✅ Antigravity accounts found");
        let config = Config {
            provider: LLMProvider::Antigravity,
            model: "antigravity-claude-opus-4-6-thinking-medium".to_string(),
        };
        config.save()?;
        return Ok(config);
    }

    println!("\n⚠️  No accounts configured yet.");
    println!("Run this command to authenticate:");
    println!("  cargo run --example basic_usage -p hypr-claw-antigravity");
    println!("\nOr manually add accounts to: {}", accounts_path);

    anyhow::bail!("Antigravity authentication required. Please run the OAuth flow first.");
}

fn bootstrap_gemini_cli() -> Result<Config> {
    println!("\n🔐 Gemini CLI OAuth Setup");
    println!("This will use the same Google OAuth as Antigravity.");
    println!("You'll get access to Gemini 3 models via CLI quota.");
    println!("\nPress Enter to continue...");
    let mut _input = String::new();
    io::stdin().read_line(&mut _input)?;

    // Check if accounts already exist
    let accounts_path = "./data/antigravity-accounts.json";
    if std::path::Path::new(accounts_path).exists() {
        println!("✅ Accounts found (shared with Antigravity)");
        let config = Config {
            provider: LLMProvider::GeminiCli,
            model: "gemini-3-flash-preview-high".to_string(),
        };
        config.save()?;
        return Ok(config);
    }

    println!("\n⚠️  No accounts configured yet.");
    println!("Run this command to authenticate:");
    println!("  cargo run --example basic_usage -p hypr-claw-antigravity");
    println!("\nOr manually add accounts to: {}", accounts_path);

    anyhow::bail!("OAuth authentication required. Please run the OAuth flow first.");
}

fn bootstrap_codex() -> Result<Config> {
    println!("\n🔐 OpenAI Codex OAuth Setup");
    println!("This will authenticate with your ChatGPT Plus/Pro account.");
    println!("You'll get access to GPT-5.x and Codex models.");
    println!("\nPress Enter to continue...");
    let mut _input = String::new();
    io::stdin().read_line(&mut _input)?;

    print!("\nEnter model [gpt-5.1-codex]: ");
    io::stdout().flush()?;
    let mut model = String::new();
    io::stdin().read_line(&mut model)?;
    let model = model.trim();
    let model = if model.is_empty() {
        "gpt-5.1-codex".to_string()
    } else {
        model.to_string()
    };

    let config = Config {
        provider: LLMProvider::Codex,
        model,
    };

    config.save()?;
    println!("✅ Codex provider configured");
    println!("💡 OAuth flow will run on first use");

    Ok(config)
}
