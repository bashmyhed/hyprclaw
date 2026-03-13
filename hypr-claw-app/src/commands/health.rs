use anyhow::{Result, Context};
use crate::config::Config;

pub async fn run() -> Result<()> {
    println!("🏥 Health Check\n");

    let config = Config::load().context("Config not found. Run 'hypr-claw init' first.")?;

    let mut healthy = true;

    // Check session directory
    print!("📁 Sessions directory... ");
    if config.sessions_dir.exists() && config.sessions_dir.is_dir() {
        println!("✓");
    } else {
        println!("✗");
        healthy = false;
    }

    // Check audit log
    print!("📝 Audit log... ");
    if config.audit_log.exists() {
        match std::fs::OpenOptions::new().append(true).open(&config.audit_log) {
            Ok(_) => println!("✓"),
            Err(_) => {
                println!("✗ (not writable)");
                healthy = false;
            }
        }
    } else {
        println!("✗ (not found)");
        healthy = false;
    }

    // Check integrity
    print!("🔍 Integrity check... ");
    match verify_integrity(&config) {
        Ok(_) => println!("✓"),
        Err(e) => {
            println!("✗ ({})", e);
            healthy = false;
        }
    }

    // Check LLM endpoint
    print!("🌐 LLM endpoint... ");
    #[cfg(feature = "mock-llm")]
    {
        println!("✓ (mock mode)");
    }
    
    #[cfg(not(feature = "mock-llm"))]
    {
        match check_llm_endpoint(&config).await {
            Ok(_) => println!("✓"),
            Err(e) => {
                println!("✗ ({})", e);
                healthy = false;
            }
        }
    }

    println!();
    if healthy {
        println!("✅ All checks passed");
        Ok(())
    } else {
        anyhow::bail!("Health check failed");
    }
}

fn verify_integrity(config: &Config) -> Result<()> {
    if !config.agents_dir.exists() {
        anyhow::bail!("Agents directory not found");
    }
    Ok(())
}

#[cfg(not(feature = "mock-llm"))]
async fn check_llm_endpoint(config: &Config) -> Result<()> {
    let endpoint = config.llm_provider.endpoint();
    let client = reqwest::Client::new();
    
    let response = client
        .head(&endpoint)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await?;

    if response.status().is_server_error() {
        anyhow::bail!("Server error");
    }

    Ok(())
}
