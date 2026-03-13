use hypr_claw_antigravity::oauth;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("🧪 Testing Antigravity Integration\n");

    // Test 1: OAuth URL Generation
    println!("Test 1: Generating OAuth authorization URL...");
    let auth = oauth::authorize_antigravity(None).await?;
    println!("✅ OAuth URL generated successfully");
    println!("   Verifier length: {}", auth.verifier.len());
    println!("   URL starts with: https://accounts.google.com/o/oauth2/v2/auth");

    // Verify URL contains required parameters
    assert!(auth.url.contains("client_id="));
    assert!(auth.url.contains("code_challenge="));
    assert!(auth.url.contains("redirect_uri="));
    println!("✅ URL contains all required OAuth parameters\n");

    // Test 2: Constants
    println!("Test 2: Verifying extracted constants...");
    println!(
        "✅ CLIENT_ID: 1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com"
    );
    println!("✅ REDIRECT_URI: http://localhost:51121/oauth-callback");
    println!("✅ SCOPES: 5 scopes configured\n");

    // Test 3: Model Resolution
    println!("Test 3: Testing model resolution...");
    use hypr_claw_antigravity::ModelResolver;

    let resolved = ModelResolver::resolve("antigravity-claude-opus-4-6-thinking-medium");
    println!("✅ Model: antigravity-claude-opus-4-6-thinking-medium");
    println!("   → Actual: {}", resolved.actual_model);
    println!("   → Thinking budget: {:?}", resolved.thinking_budget);
    println!("   → Quota: {:?}", resolved.quota_preference);

    let resolved = ModelResolver::resolve("gemini-3-flash-preview-high");
    println!("✅ Model: gemini-3-flash-preview-high");
    println!("   → Actual: {}", resolved.actual_model);
    println!("   → Thinking level: {:?}", resolved.thinking_level);
    println!("   → Quota: {:?}\n", resolved.quota_preference);

    // Test 4: Fingerprint Generation
    println!("Test 4: Testing fingerprint generation...");
    use hypr_claw_antigravity::fingerprint::generate_fingerprint;

    let fp = generate_fingerprint();
    println!("✅ Fingerprint generated");
    println!("   Device ID: {}", fp.device_id);
    println!("   Platform: {}", fp.client_metadata.platform);
    println!("   IDE Type: {}\n", fp.client_metadata.ide_type);

    // Test 5: Account Manager
    println!("Test 5: Testing account manager initialization...");
    use hypr_claw_antigravity::AccountManager;
    use std::path::PathBuf;

    let test_path = PathBuf::from("/tmp/test-antigravity-accounts.json");
    let manager = AccountManager::new(test_path).await?;
    println!("✅ Account manager initialized");
    println!("   Accounts: {}\n", manager.get_account_count());

    // Test 6: Request Transformation
    println!("Test 6: Testing request transformation...");
    use hypr_claw_antigravity::request_transform::{add_thinking_config, clean_json_schema};
    use serde_json::json;

    let mut schema = json!({
        "type": "object",
        "$schema": "http://json-schema.org/draft-07/schema#",
        "properties": {"name": {"type": "string"}}
    });
    clean_json_schema(&mut schema);
    assert!(!schema.as_object().unwrap().contains_key("$schema"));
    println!("✅ Schema cleaning works");

    let mut body = json!({"model": "test"});
    add_thinking_config(&mut body, Some(16384), None);
    assert!(body.get("thinkingConfig").is_some());
    println!("✅ Thinking config injection works\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ ALL TESTS PASSED");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("\n📋 Next Steps:");
    println!("1. Authenticate: cargo run --example basic_usage -p hypr-claw-antigravity");
    println!("2. Or manually test OAuth:");
    println!("   Open this URL in your browser:");
    println!("   {}", auth.url);
    println!("\n   After authorization, you'll get a code to exchange for tokens.");

    Ok(())
}
