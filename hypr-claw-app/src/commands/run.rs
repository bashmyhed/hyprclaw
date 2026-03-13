use anyhow::{Result, Context};
use std::sync::Arc;
use std::time::Duration;
use crate::config::Config;
use serde_json::json;

pub async fn run(agent: &str, user: &str, message: &str) -> Result<()> {
    println!("🤖 Running agent: {}", agent);
    println!("👤 User: {}", user);
    println!("💬 Message: {}\n", message);

    // Load config
    let config = Config::load().context("Failed to load config. Run 'hypr-claw init' first.")?;

    // Load API key
    let master_key = load_master_key()?;
    let cred_store = hypr_claw::infra::credential_store::CredentialStore::new(
        "./.credentials",
        &master_key,
    )?;
    let _api_key = cred_store.get_secret("llm_api_key")?;

    // Verify integrity
    println!("🔍 Verifying integrity...");
    verify_integrity(&config)?;

    // Initialize infrastructure
    let session_store = Arc::new(hypr_claw::infra::session_store::SessionStore::new(
        &config.sessions_dir,
    )?);
    let lock_manager = Arc::new(hypr_claw::infra::lock_manager::LockManager::new(
        Duration::from_secs(300),
    ));
    let permission_engine = Arc::new(hypr_claw::infra::permission_engine::PermissionEngine::new());
    let audit_logger = Arc::new(hypr_claw::infra::audit_logger::AuditLogger::new(
        &config.audit_log,
    )?);

    // Create async adapters
    let async_session = Arc::new(hypr_claw_runtime::AsyncSessionStore::new(session_store));
    let async_locks = Arc::new(hypr_claw_runtime::AsyncLockManager::new(lock_manager));

    // Setup tools
    let mut registry = hypr_claw_tools::ToolRegistryImpl::new();
    registry.register(Arc::new(hypr_claw_tools::tools::EchoTool));
    
    // Create sandbox directory
    std::fs::create_dir_all("./sandbox")?;
    registry.register(Arc::new(hypr_claw_tools::tools::FileReadTool::new("./sandbox")?));
    registry.register(Arc::new(hypr_claw_tools::tools::FileWriteTool::new("./sandbox")?));
    registry.register(Arc::new(hypr_claw_tools::tools::FileListTool::new("./sandbox")?));
    registry.register(Arc::new(hypr_claw_tools::tools::ShellExecTool));

    let registry_arc = Arc::new(registry);

    let _dispatcher = Arc::new(hypr_claw_tools::ToolDispatcherImpl::new(
        registry_arc.clone(),
        permission_engine as Arc<dyn hypr_claw_tools::PermissionEngine>,
        audit_logger as Arc<dyn hypr_claw_tools::AuditLogger>,
        5000,
    ));

    // Create mock implementations for now
    let mock_dispatcher = Arc::new(MockToolDispatcher);
    let mock_registry = Arc::new(MockToolRegistry);

    // Create LLM client
    #[cfg(feature = "mock-llm")]
    let llm_client = {
        println!("  🎭 Using mock LLM");
        hypr_claw_runtime::LLMClient::new("http://mock", 1)
    };
    
    #[cfg(not(feature = "mock-llm"))]
    let llm_client = {
        let endpoint = config.llm_provider.endpoint();
        hypr_claw_runtime::LLMClient::new(endpoint, 3)
    };

    // Create compactor with mock summarizer
    let compactor = hypr_claw_runtime::Compactor::new(1000, MockSummarizer);

    // Create agent loop
    let agent_loop = hypr_claw_runtime::AgentLoop::new(
        async_session,
        async_locks,
        mock_dispatcher,
        mock_registry,
        llm_client,
        compactor,
        10,
    );

    // Create runtime controller
    let controller = hypr_claw_runtime::RuntimeController::new(
        agent_loop,
        config.agents_dir.to_string_lossy().to_string(),
    );

    // Execute
    println!("⚙️  Executing...\n");
    let response = controller.execute(user, agent, message).await?;

    println!("📤 Response:\n{}\n", response);
    println!("✅ Complete");

    Ok(())
}

fn load_master_key() -> Result<[u8; 32]> {
    let key_bytes = std::fs::read("./.credentials/master.key")?;
    let mut key = [0u8; 32];
    key.copy_from_slice(&key_bytes[..32]);
    Ok(key)
}

fn verify_integrity(config: &Config) -> Result<()> {
    // Check directories exist
    if !config.sessions_dir.exists() {
        anyhow::bail!("Sessions directory not found");
    }
    if !config.agents_dir.exists() {
        anyhow::bail!("Agents directory not found");
    }
    if !config.audit_log.exists() {
        anyhow::bail!("Audit log not found");
    }
    println!("  ✓ Integrity check passed");
    Ok(())
}

// Mock implementations
struct MockSummarizer;

impl hypr_claw_runtime::Summarizer for MockSummarizer {
    fn summarize(
        &self,
        _messages: &[hypr_claw_runtime::Message],
    ) -> Result<String, hypr_claw_runtime::RuntimeError> {
        Ok("Summary".to_string())
    }
}

struct MockToolDispatcher;
struct MockToolRegistry;

impl hypr_claw_runtime::ToolDispatcher for MockToolDispatcher {
    fn execute(
        &self,
        _tool_name: &str,
        _input: &serde_json::Value,
        _session_key: &str,
    ) -> Result<serde_json::Value, hypr_claw_runtime::RuntimeError> {
        Ok(serde_json::json!({"success": true}))
    }
}

impl hypr_claw_runtime::ToolRegistry for MockToolRegistry {
    fn get_active_tools(&self, _agent_id: &str) -> Vec<String> {
        vec!["echo".to_string()]
    }

        fn get_tool_schemas(&self, _agent_id: &str) -> Vec<serde_json::Value> {
            vec![
                json!({
                    "type": "function",
                    "function": {
                        "name": "echo",
                        "description": "Echo a message",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "message": {"type": "string"}
                            },
                            "required": ["message"]
                        }
                    }
                })
            ]
        }
}
