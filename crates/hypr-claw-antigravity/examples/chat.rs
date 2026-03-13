use anyhow::Result;
use hypr_claw_antigravity::api_client::{ChatRequest, Message};
use hypr_claw_antigravity::{oauth, AntigravityClient};
use std::io::{self, Write};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║         Antigravity Chat - Test Claude & Gemini Models          ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    let storage_path = PathBuf::from("./data/antigravity-accounts.json");

    // Check if accounts exist
    if !storage_path.exists() {
        println!("❌ No accounts found. Please authenticate first:\n");
        println!("1. Generate OAuth URL:");
        let auth = oauth::authorize_antigravity(None).await?;
        println!("\n   Open this URL in your browser:");
        println!("   {}\n", auth.url);

        println!("2. After authorization, enter the code from the redirect URL:");
        print!("   Code: ");
        io::stdout().flush()?;
        let mut code = String::new();
        io::stdin().read_line(&mut code)?;
        let code = code.trim();

        print!("   State: ");
        io::stdout().flush()?;
        let mut state = String::new();
        io::stdin().read_line(&mut state)?;
        let state = state.trim();

        println!("\n3. Exchanging code for tokens...");
        let result = oauth::exchange_antigravity(code, state).await?;

        println!("✅ Authentication successful!");
        println!("   Email: {:?}", result.email);
        println!("   Project ID: {}\n", result.project_id);

        // Initialize client and add account
        let mut client = AntigravityClient::new(storage_path.clone()).await?;
        client
            .add_account(result.email, result.refresh, result.project_id)
            .await?;
        println!("✅ Account saved\n");
    }

    // Initialize client
    let mut client = AntigravityClient::new(storage_path).await?;
    println!("✅ Loaded {} account(s)\n", client.account_count());

    // Model selection
    println!("Select model:");
    println!("1. Claude Opus 4.6 Thinking (medium) - Antigravity quota");
    println!("2. Claude Opus 4.6 Thinking (high) - Antigravity quota");
    println!("3. Gemini 3 Flash (high) - Antigravity quota");
    println!("4. Gemini 3 Flash Preview (high) - Gemini CLI quota");
    println!("5. Gemini 3 Pro (low) - Antigravity quota");
    print!("\nChoice [1-5]: ");
    io::stdout().flush()?;

    let mut choice = String::new();
    io::stdin().read_line(&mut choice)?;

    let model = match choice.trim() {
        "1" => "antigravity-claude-opus-4-6-thinking-medium",
        "2" => "antigravity-claude-opus-4-6-thinking-high",
        "3" => "antigravity-gemini-3-flash-high",
        "4" => "gemini-3-flash-preview-high",
        "5" => "antigravity-gemini-3-pro-low",
        _ => {
            println!("Invalid choice, using Claude Opus 4.6 (medium)");
            "antigravity-claude-opus-4-6-thinking-medium"
        }
    };

    println!("\n✅ Using model: {}\n", model);

    // Chat loop
    loop {
        print!("You: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        if input == "exit" || input == "quit" {
            println!("Goodbye!");
            break;
        }

        let request = ChatRequest {
            model: model.to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: input.to_string(),
            }],
            tools: None,
            max_tokens: Some(2048),
            temperature: Some(0.7),
        };

        print!("\nAssistant: ");
        io::stdout().flush()?;

        match client.chat(request).await {
            Ok(response) => {
                if let Some(choice) = response.choices.first() {
                    println!("{}\n", choice.message.content);

                    if let Some(usage) = response.usage {
                        println!(
                            "📊 Tokens: {} prompt + {} completion = {} total\n",
                            usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
                        );
                    }
                } else {
                    println!("❌ No response from model\n");
                }
            }
            Err(e) => {
                println!("❌ Error: {}\n", e);
                if e.to_string().contains("Rate limited") {
                    println!("💡 Rotated to next account, try again\n");
                }
            }
        }
    }

    Ok(())
}
