//! Example demonstrating Codex OAuth authentication and usage

use hypr_claw_memory::types::{ContextData, OAuthTokens};
use hypr_claw_providers::{CodexProvider, LLMProvider};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║           OpenAI Codex OAuth Example                             ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    // Initialize provider
    let provider = CodexProvider::new("gpt-5.1-codex".to_string());

    // Check if we have stored tokens
    let context_path = "./data/context/codex_test.json";
    let mut context = if Path::new(context_path).exists() {
        let content = std::fs::read_to_string(context_path)?;
        serde_json::from_str::<ContextData>(&content)?
    } else {
        ContextData::default()
    };

    // Authenticate or restore tokens
    if let Some(tokens) = &context.oauth_tokens {
        println!("[Codex] Restoring tokens from memory...");
        let codex_tokens = hypr_claw_providers::codex::types::OAuthTokens {
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone(),
            expires_at: tokens.expires_at,
            account_id: tokens.account_id.clone(),
        };
        provider.restore_tokens(codex_tokens).await;
        println!("[Codex] Account ID: {}", tokens.account_id);
    } else {
        println!("[Codex] No stored tokens found. Starting OAuth flow...\n");
        let tokens = provider.authenticate().await?;

        // Store tokens in context
        context.oauth_tokens = Some(OAuthTokens {
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone(),
            expires_at: tokens.expires_at,
            account_id: tokens.account_id.clone(),
        });

        // Save context
        std::fs::create_dir_all("./data/context")?;
        std::fs::write(context_path, serde_json::to_string_pretty(&context)?)?;
        println!("\n[Codex] Tokens saved to {}", context_path);
    }

    // Test the provider
    println!("\n🧪 Testing Codex provider...\n");

    let messages = vec![hypr_claw_providers::traits::Message {
        role: "user".to_string(),
        content: "Write a simple Rust function that calculates fibonacci numbers recursively."
            .to_string(),
    }];

    println!("[Codex] Sending request...");
    let response = provider.generate(&messages, None).await?;

    println!("\n╔══════════════════════════════════════════════════════════════════╗");
    println!("║                         Response                                  ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    if let Some(content) = response.content {
        println!("{}", content);
    } else {
        println!("No content in response");
    }

    println!("\n✅ Test completed successfully!");
    println!("💡 Tokens are saved and will be reused on next run");

    Ok(())
}
