use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{self, Write};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

pub mod bootstrap;
pub mod config;
pub mod scan;

use config::{Config, LLMProvider};

enum UiInputEvent {
    Line(String),
    RunQueued(SupervisedTask),
    Skip,
}

const MAX_RECOVERY_ATTEMPTS: u32 = 2;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse CLI arguments
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "config" && args.get(2).map(|s| s.as_str()) == Some("reset") {
        return handle_config_reset();
    }

    // Initialize directories
    if let Err(e) = initialize_directories() {
        eprintln!("❌ Failed to initialize directories: {}", e);
        return Err(e);
    }

    // Load or bootstrap configuration
    let mut config = if Config::exists() {
        match Config::load() {
            Ok(cfg) => {
                if let Err(e) = cfg.validate() {
                    eprintln!("❌ Invalid configuration: {}", e);
                    eprintln!("💡 Tip: Run 'hypr-claw config reset' to reconfigure");
                    return Err(e.into());
                }
                cfg
            }
            Err(e) => {
                eprintln!("❌ Failed to load config: {}", e);
                eprintln!("💡 Tip: Run 'hypr-claw config reset' to reconfigure");
                return Err(e.into());
            }
        }
    } else {
        match bootstrap::run_bootstrap() {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("❌ Bootstrap failed: {}", e);
                return Err(e.into());
            }
        }
    };

    if matches!(config.provider, LLMProvider::Nvidia)
        && (config.model.trim().is_empty() || config.model == "moonshotai/kimi-k2.5")
    {
        config.model = "z-ai/glm4.7".to_string();
        let _ = config.save();
        println!("ℹ️  NVIDIA default model set to {}", config.model);
    }

    // Provider info
    let provider_name = match &config.provider {
        LLMProvider::Nvidia => "NVIDIA Kimi",
        LLMProvider::Google => "Google Gemini",
        LLMProvider::Local { .. } => "Local",
        LLMProvider::Antigravity => "Antigravity (Claude + Gemini)",
        LLMProvider::GeminiCli => "Gemini CLI",
        LLMProvider::Codex => "OpenAI Codex (ChatGPT Plus/Pro)",
    };

    if !config.provider.supports_function_calling() {
        eprintln!(
            "❌ Provider '{}' does not support function/tool calling in agent mode.",
            provider_name
        );
        eprintln!("   Use NVIDIA, Google, or Local providers for autonomous execution.");
        return Err("Provider capability check failed".into());
    }

    let agent_name = detect_agent_name();
    let user_id = detect_user_id();
    let session_key = format!("{}:{}", user_id, agent_name);

    let context_manager = hypr_claw_memory::ContextManager::new("./data/context");
    context_manager.initialize().await?;
    let mut context = context_manager.load(&session_key).await?;
    if context.session_id.is_empty() {
        context.session_id = session_key.clone();
    }

    let active_soul_id = "power_agent".to_string();
    let active_soul = power_agent_profile();
    let mut agent_state = load_agent_os_state(&context);
    agent_state.soul_auto = false;
    agent_state.autonomy_mode = AutonomyMode::PromptFirst;
    agent_state.supervisor.auto_run = false;
    ensure_default_thread(&mut agent_state);
    let recovered_stale = reconcile_supervisor_after_restart(&mut agent_state);
    if recovered_stale > 0 {
        println!(
            "ℹ️  Recovered {} stale supervisor task(s) from previous session.",
            recovered_stale
        );
    }
    run_first_run_onboarding(&user_id, &mut context, &mut agent_state).await?;
    if profile_needs_capability_refresh(&agent_state.onboarding.system_profile) {
        agent_state.onboarding.system_profile = scan::run_integrated_scan(&user_id, false).await?;
        agent_state.onboarding.last_scan_at = Some(chrono::Utc::now().timestamp());
    }
    let (mut capability_registry, registry_loaded) = match load_capability_registry(&user_id) {
        Ok(registry) => (registry, true),
        Err(_) => (
            build_capability_registry(&agent_state.onboarding.system_profile),
            false,
        ),
    };
    if !registry_loaded
        || capability_registry_needs_refresh(
            &capability_registry,
            &agent_state.onboarding.system_profile,
        )
    {
        capability_registry = build_capability_registry(&agent_state.onboarding.system_profile);
        if let Err(e) = save_capability_registry(&user_id, &capability_registry) {
            eprintln!("⚠️  Failed to save capability registry: {}", e);
        }
    }
    agent_state.onboarding.trusted_full_auto = false;
    context.active_soul_id = active_soul_id.clone();
    persist_agent_os_state(&mut context, &agent_state);
    context_manager.save(&context).await?;

    println!("\n🔧 Initializing system...");

    // Initialize infrastructure
    let session_store = match hypr_claw::infra::session_store::SessionStore::new("./data/sessions")
    {
        Ok(store) => Arc::new(store),
        Err(e) => {
            eprintln!("❌ Failed to initialize session store: {}", e);
            return Err(Box::new(e));
        }
    };

    let lock_manager = Arc::new(hypr_claw::infra::lock_manager::LockManager::new(
        Duration::from_secs(300),
    ));
    let permission_engine = Arc::new(hypr_claw::infra::permission_engine::PermissionEngine::new());

    let audit_logger = match hypr_claw::infra::audit_logger::AuditLogger::new("./data/audit.log") {
        Ok(logger) => Arc::new(logger),
        Err(e) => {
            eprintln!("❌ Failed to initialize audit logger: {}", e);
            return Err(Box::new(e));
        }
    };

    // Wrap in async adapters
    let async_session = Arc::new(hypr_claw_runtime::AsyncSessionStore::new(session_store));
    let async_locks = Arc::new(hypr_claw_runtime::AsyncLockManager::new(lock_manager));

    // Create tool registry
    let mut registry = hypr_claw_tools::ToolRegistryImpl::new();
    registry.register(Arc::new(hypr_claw_tools::tools::EchoTool));

    // Register OS capability tools
    registry.register(Arc::new(hypr_claw_tools::os_tools::FsCreateDirTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::FsDeleteTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::FsMoveTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::FsCopyTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::FsReadTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::FsWriteTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::FsListTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::HyprWorkspaceSwitchTool));
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::HyprWorkspaceMoveWindowTool,
    ));
    registry.register(Arc::new(hypr_claw_tools::os_tools::HyprWindowFocusTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::HyprWindowCloseTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::HyprWindowMoveTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::HyprExecTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::ProcSpawnTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::ProcKillTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::ProcListTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopOpenUrlTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopLaunchAppTool));
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::DesktopLaunchAppAndWaitTextTool,
    ));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopSearchWebTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopOpenGmailTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopTypeTextTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopKeyPressTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopKeyComboTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopMouseClickTool));
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::DesktopCaptureScreenTool,
    ));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopActiveWindowTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopListWindowsTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopMouseMoveTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopClickAtTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopOcrScreenTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopFindTextTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopClickTextTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopWaitForTextTool));
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::DesktopCursorPositionTool,
    ));
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::DesktopMouseMoveAndVerifyTool,
    ));
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::DesktopClickAtAndVerifyTool,
    ));
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::DesktopReadScreenStateTool,
    ));
    registry.register(Arc::new(hypr_claw_tools::os_tools::WallpaperSetTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::SystemShutdownTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::SystemRebootTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::SystemBatteryTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::SystemMemoryTool));

    let registry_arc = Arc::new(registry);

    // Create tool dispatcher
    let dispatcher = Arc::new(hypr_claw_tools::ToolDispatcherImpl::new(
        registry_arc.clone(),
        permission_engine as Arc<dyn hypr_claw_tools::PermissionEngine>,
        audit_logger as Arc<dyn hypr_claw_tools::AuditLogger>,
        5000,
    ));

    let allowed_tools = derive_runtime_allowed_tools(&registry_arc, &capability_registry);
    if allowed_tools.is_empty() {
        return Err("No runtime tools available after capability filtering".into());
    }
    let active_allowed_tools = allowed_tools.clone();
    let allowed_tools_state = Arc::new(RwLock::new(allowed_tools.clone()));
    let action_feed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let task_event_feed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // Create runtime adapters
    let runtime_dispatcher = Arc::new(RuntimeDispatcherAdapter::new(
        dispatcher,
        action_feed.clone(),
    ));
    let runtime_registry = Arc::new(RuntimeRegistryAdapter::new(
        registry_arc.clone(),
        allowed_tools_state.clone(),
    ));

    // Initialize LLM client based on provider
    let llm_client = match &config.provider {
        LLMProvider::Nvidia => {
            let api_key = match bootstrap::get_nvidia_api_key() {
                Ok(key) => key,
                Err(e) => {
                    eprintln!("❌ Failed to retrieve NVIDIA API key: {}", e);
                    eprintln!("💡 Tip: Run 'hypr-claw config reset' to reconfigure");
                    return Err(e.into());
                }
            };
            hypr_claw_runtime::LLMClientType::Standard(
                hypr_claw_runtime::LLMClient::with_api_key_and_model(
                    config.provider.base_url(),
                    1,
                    api_key,
                    config.model.clone(),
                ),
            )
        }
        LLMProvider::Google => {
            let api_key = match bootstrap::get_google_api_key() {
                Ok(key) => key,
                Err(e) => {
                    eprintln!("❌ Failed to retrieve Google API key: {}", e);
                    eprintln!("💡 Tip: Run 'hypr-claw config reset' to reconfigure");
                    return Err(e.into());
                }
            };
            hypr_claw_runtime::LLMClientType::Standard(
                hypr_claw_runtime::LLMClient::with_api_key_and_model(
                    config.provider.base_url(),
                    1,
                    api_key,
                    config.model.clone(),
                ),
            )
        }
        LLMProvider::Local { .. } => hypr_claw_runtime::LLMClientType::Standard(
            hypr_claw_runtime::LLMClient::new(config.provider.base_url(), 1),
        ),
        LLMProvider::Codex | LLMProvider::Antigravity | LLMProvider::GeminiCli => {
            return Err("Provider does not support agent-mode tool calling".into());
        }
    };

    // Create compactor
    let compactor = hypr_claw_runtime::Compactor::new(4000, SimpleSummarizer);

    // Create agent loop
    let agent_loop = hypr_claw_runtime::AgentLoop::new(
        async_session.clone(),
        async_locks.clone(),
        runtime_dispatcher.clone(),
        runtime_registry.clone(),
        llm_client,
        compactor,
        active_soul.max_iterations,
    );

    // Create task manager
    let task_manager = Arc::new(hypr_claw_tasks::TaskManager::with_state_file(
        "./data/tasks/tasks.json",
    ));
    task_manager.restore().await?;
    context.active_tasks = to_context_tasks(task_manager.list_tasks().await);
    context_manager.save(&context).await?;

    // Run REPL loop
    let system_prompt = active_soul.system_prompt.clone();

    print_console_bootstrap(
        provider_name,
        &config.model,
        &agent_name,
        &display_name(&agent_state, &user_id),
        &user_id,
        &session_key,
        &agent_state.active_thread_id,
    );

    // Setup interrupt signal (Ctrl+C interrupts current request, does not exit process)
    let interrupt = Arc::new(tokio::sync::Notify::new());
    let interrupt_clone = interrupt.clone();
    tokio::spawn(async move {
        loop {
            tokio::signal::ctrl_c().await.ok();
            interrupt_clone.notify_waiters();
        }
    });
    let mut auto_queued_task: Option<SupervisedTask> = None;
    let mut queue_block_notice: Option<String> = None;
    let mut transcript_view_mode = true;
    let mut background_task_index: HashMap<String, TaskStateDigest> = HashMap::new();
    let mut supervisor_background_map: HashMap<String, String> = HashMap::new();
    for task in &agent_state.supervisor.tasks {
        if task.status == SupervisedTaskStatus::Running {
            if let Some(bg_id) = &task.background_task_id {
                supervisor_background_map.insert(task.id.clone(), bg_id.clone());
            }
        }
    }
    if recovered_stale > 0 {
        push_task_event(
            &task_event_feed,
            format!(
                "sup recovered {} stale running task(s) after restart",
                recovered_stale
            ),
        );
    }

    loop {
        let latest_task_list = task_manager.list_tasks().await;
        sync_background_task_events(
            &latest_task_list,
            &mut background_task_index,
            &task_event_feed,
        );

        let mut supervisor_state_changed = false;
        let mut finished_background_sup = Vec::<String>::new();
        for (sup_id, bg_id) in &supervisor_background_map {
            if let Some(bg_task) = latest_task_list.iter().find(|task| &task.id == bg_id) {
                match bg_task.status {
                    hypr_claw_tasks::TaskStatus::Completed => {
                        mark_supervised_task_completed(&mut agent_state, sup_id);
                        let summary = bg_task.result.clone().unwrap_or_else(|| "done".to_string());
                        push_task_event(
                            &task_event_feed,
                            format!(
                                "sup {} completed via background {} out={}",
                                sup_id,
                                bg_id,
                                truncate_for_table(&summary, 56)
                            ),
                        );
                        finished_background_sup.push(sup_id.clone());
                        supervisor_state_changed = true;
                    }
                    hypr_claw_tasks::TaskStatus::Failed => {
                        let err = bg_task.error.clone().unwrap_or_else(|| {
                            format!("background task {} {:?}", bg_id, bg_task.status)
                        });
                        mark_supervised_task_failed(&mut agent_state, sup_id, err.clone());
                        push_task_event(
                            &task_event_feed,
                            format!(
                                "sup {} failed via background {} {}",
                                sup_id,
                                bg_id,
                                truncate_for_table(&err, 56)
                            ),
                        );
                        finished_background_sup.push(sup_id.clone());
                        supervisor_state_changed = true;
                    }
                    hypr_claw_tasks::TaskStatus::Cancelled => {
                        let reason = bg_task
                            .error
                            .clone()
                            .unwrap_or_else(|| "Cancelled by user".to_string());
                        mark_supervised_task_cancelled(
                            &mut agent_state,
                            sup_id,
                            Some(reason.clone()),
                        );
                        push_task_event(
                            &task_event_feed,
                            format!(
                                "sup {} cancelled via background {} {}",
                                sup_id,
                                bg_id,
                                truncate_for_table(&reason, 56)
                            ),
                        );
                        finished_background_sup.push(sup_id.clone());
                        supervisor_state_changed = true;
                    }
                    _ => {}
                }
            }
        }
        for sup_id in finished_background_sup {
            supervisor_background_map.remove(&sup_id);
        }
        if supervisor_state_changed {
            persist_agent_os_state(&mut context, &agent_state);
            context_manager.save(&context).await?;
        }

        if auto_queued_task.is_none() && agent_state.supervisor.auto_run {
            loop {
                match start_next_queued_supervised_task(&mut agent_state) {
                    QueueStartResult::Started(task) => {
                        if can_run_supervisor_in_background(&task) {
                            let bg_task_id = format!("supbg-{}", task.id);
                            let bg_description = format!(
                                "supervisor {} {}",
                                task.id,
                                truncate_for_table(&task.prompt, 56)
                            );
                            let task_prompt = task.prompt.clone();
                            let task_id = task.id.clone();
                            let task_class = task.class.clone();
                            let timeout_bg =
                                watchdog_timeout_for_class(&task_class, &agent_state.autonomy_mode);
                            let max_iter_bg = active_soul
                                .max_iterations
                                .min(
                                    execution_budget_for_class(
                                        &task_class,
                                        &agent_state.autonomy_mode,
                                    )
                                    .max_iterations,
                                )
                                .max(1);
                            let provider_bg = config.provider.clone();
                            let model_bg = config.model.clone();
                            let async_session_bg = async_session.clone();
                            let async_locks_bg = async_locks.clone();
                            let runtime_dispatcher_bg = runtime_dispatcher.clone();
                            let registry_arc_bg = registry_arc.clone();
                            let allowed_tools_bg = active_allowed_tools.clone();
                            let task_session_key = format!("{}::sup::{}", session_key, task_id);
                            let agent_name_bg = agent_name.clone();
                            let system_prompt_bg = augment_system_prompt_for_turn(
                                &system_prompt,
                                &agent_state.onboarding.system_profile,
                                &capability_registry,
                                &active_allowed_tools,
                                &agent_state.autonomy_mode,
                            );

                            let spawn_result = task_manager
                                .spawn_task(
                                    bg_task_id.clone(),
                                    bg_description,
                                    move || async move {
                                        let llm_client =
                                            build_llm_client_for_provider(&provider_bg, &model_bg)
                                                .map_err(|e| {
                                                    format!("LLM client init failed: {}", e)
                                                })?;
                                        let allowed_state_bg =
                                            Arc::new(RwLock::new(allowed_tools_bg));
                                        let runtime_registry_bg =
                                            Arc::new(RuntimeRegistryAdapter::new(
                                                registry_arc_bg,
                                                allowed_state_bg,
                                            ));
                                        let compactor = hypr_claw_runtime::Compactor::new(
                                            4000,
                                            SimpleSummarizer,
                                        );
                                        let agent_loop_bg = hypr_claw_runtime::AgentLoop::new(
                                            async_session_bg,
                                            async_locks_bg,
                                            runtime_dispatcher_bg,
                                            runtime_registry_bg,
                                            llm_client,
                                            compactor,
                                            max_iter_bg,
                                        );
                                        match tokio::time::timeout(
                                            timeout_bg,
                                            agent_loop_bg.run(
                                                &task_session_key,
                                                &agent_name_bg,
                                                &system_prompt_bg,
                                                &task_prompt,
                                            ),
                                        )
                                        .await
                                        {
                                            Ok(Ok(response)) => Ok(response),
                                            Ok(Err(e)) => Err(e.to_string()),
                                            Err(_) => Err(format!(
                                                "Execution watchdog timeout after {}s",
                                                timeout_bg.as_secs()
                                            )),
                                        }
                                    },
                                )
                                .await;

                            match spawn_result {
                                Ok(background_id) => {
                                    queue_block_notice = None;
                                    set_supervised_task_background_id(
                                        &mut agent_state,
                                        &task_id,
                                        Some(background_id.clone()),
                                    );
                                    supervisor_background_map
                                        .insert(task_id.clone(), background_id);
                                    push_task_event(
                                        &task_event_feed,
                                        format!(
                                            "sup {} started in background class={} res={}",
                                            task_id,
                                            task.class.as_str(),
                                            truncate_for_table(&task.resources.join(","), 24)
                                        ),
                                    );
                                    persist_agent_os_state(&mut context, &agent_state);
                                    context_manager.save(&context).await?;
                                    println!(
                                        "▶ Auto-running queued task {} in background ({})",
                                        task_id,
                                        task.class.as_str()
                                    );
                                    continue;
                                }
                                Err(e) => {
                                    let err = e.to_string();
                                    set_supervised_task_background_id(
                                        &mut agent_state,
                                        &task_id,
                                        None,
                                    );
                                    mark_supervised_task_failed(
                                        &mut agent_state,
                                        &task_id,
                                        err.clone(),
                                    );
                                    push_task_event(
                                        &task_event_feed,
                                        format!(
                                            "sup {} background start failed {}",
                                            task_id,
                                            truncate_for_table(&err, 56)
                                        ),
                                    );
                                    persist_agent_os_state(&mut context, &agent_state);
                                    context_manager.save(&context).await?;
                                    continue;
                                }
                            }
                        }

                        queue_block_notice = None;
                        auto_queued_task = Some(task.clone());
                        push_task_event(
                            &task_event_feed,
                            format!(
                                "sup {} started (auto-run) class={} res={}",
                                task.id,
                                task.class.as_str(),
                                truncate_for_table(&task.resources.join(","), 24)
                            ),
                        );
                        persist_agent_os_state(&mut context, &agent_state);
                        context_manager.save(&context).await?;
                        println!(
                            "▶ Auto-running queued task {} ({})",
                            task.id,
                            task.class.as_str()
                        );
                        break;
                    }
                    QueueStartResult::Blocked(reason) => {
                        if queue_block_notice.as_deref() != Some(reason.as_str()) {
                            println!("⏸ Queue auto-run blocked: {}", reason);
                            push_task_event(
                                &task_event_feed,
                                format!("sup queue blocked: {}", reason),
                            );
                            queue_block_notice = Some(reason);
                        }
                        break;
                    }
                    QueueStartResult::Empty => {
                        queue_block_notice = None;
                        break;
                    }
                }
            }
        } else if !agent_state.supervisor.auto_run {
            queue_block_notice = None;
        }

        let task_list_snapshot = latest_task_list.clone();
        tokio::select! {
            _ = interrupt.notified() => {
                println!("\n^C received. No active request to interrupt. Use 'exit' to quit.");
                continue;
            }
            result = async {
                if let Some(task) = auto_queued_task.take() {
                    return UiInputEvent::RunQueued(task);
                }
                let running_tasks = task_list_snapshot
                    .iter()
                    .filter(|t| t.status == hypr_claw_tasks::TaskStatus::Running)
                    .count();
                let prompt = format!(
                    "hc[{}|{}|{}]> ",
                    truncate_for_table(&agent_state.active_thread_id, 12),
                    short_model_name(&config.model),
                    running_tasks
                );
                print!("{}", ui_accent(&prompt));
                io::stdout().flush().ok();

                let mut input = String::new();
                io::stdin().read_line(&mut input).ok();
                let line = sanitize_single_line(input.trim());
                if line.is_empty() {
                    UiInputEvent::Skip
                } else {
                    UiInputEvent::Line(line)
                }
            } => {
                let mut queued_execution: Option<SupervisedTask> = None;
                let (input, input_from_queue) = match result {
                    UiInputEvent::Line(s) => (sanitize_user_input_line(&s), false),
                    UiInputEvent::RunQueued(task) => {
                        queued_execution = Some(task.clone());
                        (task.prompt.clone(), true)
                    }
                    UiInputEvent::Skip => continue,
                };

                if input.as_bytes().first() == Some(&0x1B) {
                    println!("⚠ Ignored terminal escape input. Type a command or task request.");
                    continue;
                }

                if !input_from_queue {
                    if let Some(msg) = removed_feature_notice(&input) {
                        println!("{msg}");
                        continue;
                    }
                }

                if !input_from_queue {
                    match input.as_str() {
                    "exit" | "quit" | "/exit" => {
                        println!("👋 Goodbye!");
                        break;
                    }
                    "help" | "/help" => {
                        print_help();
                        continue;
                    }
                    "view" | "/view" => {
                        println!(
                            "View mode: {}",
                            if transcript_view_mode {
                                "transcript"
                            } else {
                                "compact"
                            }
                        );
                        println!("Use: view transcript | view compact");
                        continue;
                    }
                    "status" | "/status" => {
                        let task_list = task_manager.list_tasks().await;
                        print_status_panel(
                            &session_key,
                            &agent_name,
                            &display_name(&agent_state, &user_id),
                            &config.model,
                            &agent_state.active_thread_id,
                            &agent_state,
                            &context,
                            &task_list,
                        );
                        println!();
                        continue;
                    }
                    "tasks" | "/tasks" => {
                        let task_list = task_manager.list_tasks().await;
                        print_tasks_panel(&task_list);
                        println!();
                        context.active_tasks = to_context_tasks(task_list);
                        persist_agent_os_state(&mut context, &agent_state);
                        context_manager.save(&context).await?;
                        continue;
                    }
                    "profile" | "/profile" => {
                        let profile = &agent_state.onboarding.system_profile;
                        if profile.is_null() || profile == &json!({}) {
                            println!("\nNo profile stored yet.\n");
                        } else {
                            println!(
                                "\n{}\n",
                                serde_json::to_string_pretty(profile).unwrap_or_else(|_| "{}".to_string())
                            );
                        }
                        continue;
                    }
                    "capabilities history" | "/capabilities history" => {
                        print_capability_delta_history(&user_id, 20);
                        continue;
                    }
                    "capabilities" | "/capabilities" => {
                        print_capability_registry_summary(&user_id, &capability_registry);
                        continue;
                    }
                    "scan" | "/scan" => {
                        if prompt_yes_no("Run a new system scan now? [Y/n] ", true)? {
                            let deep_scan = prompt_yes_no(
                                "Deep scan (home directory with user consent)? [Y/n] ",
                                true,
                            )?;

                            let mut scanned_profile = scan::run_integrated_scan(&user_id, deep_scan).await?;

                            print_system_profile_summary(&scanned_profile);
                            let mut scanned_registry = build_capability_registry(&scanned_profile);
                            print_capability_registry_diff_summary(
                                &capability_registry,
                                &scanned_registry,
                            );

                            if prompt_yes_no("Edit scanned profile before save? [y/N] ", false)? {
                                edit_profile_interactively(&mut scanned_profile)?;
                                print_system_profile_summary(&scanned_profile);
                                scanned_registry = build_capability_registry(&scanned_profile);
                                print_capability_registry_diff_summary(
                                    &capability_registry,
                                    &scanned_registry,
                                );
                            }

                            if !prompt_yes_no(
                                "Apply scanned profile + capability changes? [Y/n] ",
                                true,
                            )? {
                                println!("⏭ Scan changes discarded.");
                                continue;
                            }

                            agent_state.onboarding.system_profile = scanned_profile;
                            agent_state.onboarding.deep_scan_completed = deep_scan;
                            agent_state.onboarding.profile_confirmed = true;
                            agent_state.onboarding.last_scan_at =
                                Some(chrono::Utc::now().timestamp());
                            let old_registry_for_history = capability_registry.clone();
                            capability_registry = scanned_registry;
                            if let Err(e) = save_capability_registry(&user_id, &capability_registry) {
                                eprintln!("⚠️  Failed to save capability registry: {}", e);
                            }
                            if let Err(e) = append_capability_delta_history(
                                &user_id,
                                &old_registry_for_history,
                                &capability_registry,
                            ) {
                                eprintln!("⚠️  Failed to append capability delta history: {}", e);
                            }
                            persist_agent_os_state(&mut context, &agent_state);
                            context_manager.save(&context).await?;
                            println!("✅ System profile and capability registry updated");
                        }
                        continue;
                    }
                    "clear" | "/clear" => {
                        print!("\x1B[2J\x1B[1;1H");
                        continue;
                    }
                    "interrupt" | "/interrupt" => {
                        interrupt.notify_waiters();
                        println!("⏹ Interrupt signal sent.");
                        continue;
                    }
                    "queue" | "/queue" => {
                        print_supervisor_queue(&agent_state);
                        continue;
                    }
                    "queue status" | "/queue status" => {
                        print_supervisor_queue_status(&agent_state);
                        continue;
                    }
                    _ => {}
                }
                }

                if !input_from_queue {
                    if let Some(mode) = input
                        .strip_prefix("view ")
                        .or_else(|| input.strip_prefix("/view "))
                        .map(str::trim)
                    {
                        match mode {
                            "transcript" | "live" | "on" => {
                                transcript_view_mode = true;
                                println!("✅ View mode set to transcript");
                            }
                            "compact" | "off" => {
                                transcript_view_mode = false;
                                println!("✅ View mode set to compact");
                            }
                            _ => {
                                println!("Use: view transcript | view compact");
                            }
                        }
                        continue;
                    }
                }

                if !input_from_queue {
                if let Some(prompt) = input
                    .strip_prefix("queue add ")
                    .or_else(|| input.strip_prefix("/queue add "))
                    .map(str::trim)
                {
                    if prompt.is_empty() {
                        println!("Usage: queue add <task prompt>");
                        continue;
                    }
                    let class = classify_supervised_task_class(prompt);
                    let task_id = enqueue_supervised_task(
                        &mut agent_state,
                        prompt.to_string(),
                        class.clone(),
                    );
                    persist_agent_os_state(&mut context, &agent_state);
                    context_manager.save(&context).await?;
                    push_task_event(
                        &task_event_feed,
                        format!(
                            "sup {} queued class={} {}",
                            task_id,
                            class.as_str(),
                            truncate_for_table(prompt, 44)
                        ),
                    );
                    println!(
                        "🗂 Queued {} ({}) as {}",
                        task_id,
                        truncate_for_table(prompt, 48),
                        class.as_str()
                    );
                    continue;
                }
                }

                if !input_from_queue && (input == "queue clear" || input == "/queue clear") {
                    let cleared = cancel_queued_supervised_tasks(&mut agent_state);
                    persist_agent_os_state(&mut context, &agent_state);
                    context_manager.save(&context).await?;
                    push_task_event(&task_event_feed, format!("sup queue cleared {} queued tasks", cleared));
                    println!("🧹 Cleared {} queued supervisor tasks", cleared);
                    continue;
                }

                if !input_from_queue {
                    if let Some(raw) = input
                        .strip_prefix("queue prune")
                        .or_else(|| input.strip_prefix("/queue prune"))
                        .map(str::trim)
                    {
                        let keep_terminal = if raw.is_empty() {
                            200usize
                        } else if let Ok(value) = raw.parse::<usize>() {
                            value.max(1)
                        } else {
                            println!("Usage: queue prune [keep_terminal_count]");
                            continue;
                        };
                        let removed = prune_supervisor_tasks(&mut agent_state, keep_terminal);
                        supervisor_background_map.retain(|sup_id, _| {
                            agent_state.supervisor.tasks.iter().any(|task| {
                                task.id == *sup_id && task.status == SupervisedTaskStatus::Running
                            })
                        });
                        push_task_event(
                            &task_event_feed,
                            format!(
                                "sup pruned removed={} keep_terminal={}",
                                removed, keep_terminal
                            ),
                        );
                        persist_agent_os_state(&mut context, &agent_state);
                        context_manager.save(&context).await?;
                        println!(
                            "🧹 Pruned {} terminal supervisor tasks (kept latest {}).",
                            removed, keep_terminal
                        );
                        continue;
                    }
                }

                if !input_from_queue {
                    if let Some(target) = input
                        .strip_prefix("queue inspect ")
                        .or_else(|| input.strip_prefix("/queue inspect "))
                        .map(str::trim)
                    {
                        if target.is_empty() {
                            println!("Usage: queue inspect <sup_id|bg_id>");
                            continue;
                        }
                        if let Some(task) = resolve_supervisor_task(target, &agent_state) {
                            print_supervisor_task_inspect(task);
                        } else {
                            println!("❌ Supervisor task not found for '{}'", target);
                        }
                        continue;
                    }
                }

                if !input_from_queue {
                    if let Some(raw) = input
                        .strip_prefix("queue events ")
                        .or_else(|| input.strip_prefix("/queue events "))
                        .map(str::trim)
                    {
                        if raw.is_empty() {
                            println!("Usage: queue events <sup_id|bg_id> [limit]");
                            continue;
                        }
                        let mut parts = raw.split_whitespace();
                        let target = parts.next().unwrap_or_default();
                        if target.is_empty() {
                            println!("Usage: queue events <sup_id|bg_id> [limit]");
                            continue;
                        }
                        let limit = if let Some(raw_limit) = parts.next() {
                            if let Ok(value) = raw_limit.parse::<usize>() {
                                value.max(1)
                            } else {
                                println!("Usage: queue events <sup_id|bg_id> [limit]");
                                continue;
                            }
                        } else {
                            24usize
                        };
                        if parts.next().is_some() {
                            println!("Usage: queue events <sup_id|bg_id> [limit]");
                            continue;
                        }

                        if let Some(task) = resolve_supervisor_task(target, &agent_state) {
                            let event_rows =
                                collect_supervisor_task_events(task, &task_event_feed, limit);
                            print_supervisor_task_events(task, &event_rows, limit);
                        } else {
                            println!("❌ Supervisor task not found for '{}'", target);
                        }
                        continue;
                    }
                }

                if !input_from_queue {
                    if let Some(target) = input
                        .strip_prefix("queue stop ")
                        .or_else(|| input.strip_prefix("/queue stop "))
                        .map(str::trim)
                    {
                        if target.is_empty() {
                            println!("Usage: queue stop <sup_id|bg_id|all>");
                            continue;
                        }
                        if target == "all" {
                            let candidate_ids = agent_state
                                .supervisor
                                .tasks
                                .iter()
                                .filter(|task| {
                                    task.status == SupervisedTaskStatus::Queued
                                        || task.status == SupervisedTaskStatus::Running
                                })
                                .map(|task| task.id.clone())
                                .collect::<Vec<_>>();
                            if candidate_ids.is_empty() {
                                println!("ℹ️  No queued/running supervisor tasks to stop.");
                                continue;
                            }

                            let mut cancelled = 0usize;
                            let mut errors = 0usize;
                            let mut interrupted_foreground = 0usize;
                            for sup_id in candidate_ids {
                                let snapshot = agent_state
                                    .supervisor
                                    .tasks
                                    .iter()
                                    .find(|task| task.id == sup_id)
                                    .cloned();
                                let Some(task) = snapshot else {
                                    continue;
                                };
                                match task.status {
                                    SupervisedTaskStatus::Queued => {
                                        mark_supervised_task_cancelled(
                                            &mut agent_state,
                                            &sup_id,
                                            Some("Cancelled by user (bulk)".to_string()),
                                        );
                                        supervisor_background_map.remove(&sup_id);
                                        cancelled += 1;
                                    }
                                    SupervisedTaskStatus::Running => {
                                        let bg_id = task
                                            .background_task_id
                                            .clone()
                                            .or_else(|| supervisor_background_map.get(&sup_id).cloned());
                                        if let Some(bg_id) = bg_id {
                                            match task_manager.cancel_task(&bg_id).await {
                                                Ok(_) => {
                                                    supervisor_background_map.remove(&sup_id);
                                                    mark_supervised_task_cancelled(
                                                        &mut agent_state,
                                                        &sup_id,
                                                        Some("Cancelled by user (bulk)".to_string()),
                                                    );
                                                    cancelled += 1;
                                                }
                                                Err(_) => {
                                                    errors += 1;
                                                }
                                            }
                                        } else {
                                            interrupt.notify_waiters();
                                            mark_supervised_task_cancelled(
                                                &mut agent_state,
                                                &sup_id,
                                                Some("Cancelled by user (bulk)".to_string()),
                                            );
                                            interrupted_foreground += 1;
                                            cancelled += 1;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            push_task_event(
                                &task_event_feed,
                                format!(
                                    "sup bulk stop all cancelled={} interrupted_fg={} errors={}",
                                    cancelled, interrupted_foreground, errors
                                ),
                            );
                            persist_agent_os_state(&mut context, &agent_state);
                            context_manager.save(&context).await?;
                            println!(
                                "🧹 Stopped all: cancelled={} interrupted_fg={} errors={}",
                                cancelled, interrupted_foreground, errors
                            );
                            continue;
                        }
                        let sup_id = if agent_state
                            .supervisor
                            .tasks
                            .iter()
                            .any(|task| task.id == target)
                        {
                            target.to_string()
                        } else if let Some((sup, _)) = supervisor_background_map
                            .iter()
                            .find(|(_, bg_id)| bg_id.as_str() == target)
                        {
                            sup.clone()
                        } else {
                            println!("❌ Supervisor task not found for '{}'", target);
                            continue;
                        };

                        let snapshot = agent_state
                            .supervisor
                            .tasks
                            .iter()
                            .find(|task| task.id == sup_id)
                            .cloned();
                        let Some(task) = snapshot else {
                            println!("❌ Supervisor task '{}' not found", sup_id);
                            continue;
                        };

                        match task.status {
                            SupervisedTaskStatus::Queued => {
                                mark_supervised_task_cancelled(
                                    &mut agent_state,
                                    &sup_id,
                                    Some("Cancelled by user".to_string()),
                                );
                                push_task_event(
                                    &task_event_feed,
                                    format!("sup {} cancelled from queue", sup_id),
                                );
                                println!("🧹 Cancelled queued supervisor task {}", sup_id);
                            }
                            SupervisedTaskStatus::Running => {
                                let bg_id = task
                                    .background_task_id
                                    .clone()
                                    .or_else(|| supervisor_background_map.get(&sup_id).cloned());
                                if let Some(bg_id) = bg_id {
                                    match task_manager.cancel_task(&bg_id).await {
                                        Ok(_) => {
                                            supervisor_background_map.remove(&sup_id);
                                            mark_supervised_task_cancelled(
                                                &mut agent_state,
                                                &sup_id,
                                                Some("Cancelled by user".to_string()),
                                            );
                                            push_task_event(
                                                &task_event_feed,
                                                format!(
                                                    "sup {} cancelled background {}",
                                                    sup_id, bg_id
                                                ),
                                            );
                                            println!(
                                                "🧹 Cancelled running supervisor task {} (bg {})",
                                                sup_id, bg_id
                                            );
                                        }
                                        Err(e) => {
                                            let err = e.to_string();
                                            push_task_event(
                                                &task_event_feed,
                                                format!(
                                                    "sup {} cancel failed {}",
                                                    sup_id,
                                                    truncate_for_table(&err, 56)
                                                ),
                                            );
                                            println!(
                                                "❌ Failed to cancel background task for {}: {}",
                                                sup_id, err
                                            );
                                        }
                                    }
                                } else {
                                    interrupt.notify_waiters();
                                    mark_supervised_task_cancelled(
                                        &mut agent_state,
                                        &sup_id,
                                        Some("Cancelled by user".to_string()),
                                    );
                                    push_task_event(
                                        &task_event_feed,
                                        format!("sup {} cancelled foreground run", sup_id),
                                    );
                                    println!(
                                        "⏹ Sent interrupt signal for running foreground task {}",
                                        sup_id
                                    );
                                }
                            }
                            SupervisedTaskStatus::Completed => {
                                println!("ℹ️  Task {} already completed.", sup_id);
                            }
                            SupervisedTaskStatus::Failed => {
                                println!("ℹ️  Task {} already failed.", sup_id);
                            }
                            SupervisedTaskStatus::Cancelled => {
                                println!("ℹ️  Task {} already cancelled.", sup_id);
                            }
                        }

                        persist_agent_os_state(&mut context, &agent_state);
                        context_manager.save(&context).await?;
                        continue;
                    }
                }

                if !input_from_queue {
                    if let Some(target) = input
                        .strip_prefix("queue retry ")
                        .or_else(|| input.strip_prefix("/queue retry "))
                        .map(str::trim)
                    {
                        if target.is_empty() {
                            println!("Usage: queue retry <sup_id|failed|cancelled|completed|all>");
                            continue;
                        }
                        if target == "failed" || target == "cancelled" {
                            let candidates = agent_state
                                .supervisor
                                .tasks
                                .iter()
                                .filter(|task| {
                                    task.status == SupervisedTaskStatus::Failed
                                        || task.status == SupervisedTaskStatus::Cancelled
                                })
                                .cloned()
                                .collect::<Vec<_>>();
                            if candidates.is_empty() {
                                println!("ℹ️  No failed/cancelled supervisor tasks to retry.");
                                continue;
                            }
                            let mut retried = 0usize;
                            for task in candidates {
                                let retry_id = enqueue_supervised_task(
                                    &mut agent_state,
                                    task.prompt.clone(),
                                    task.class.clone(),
                                );
                                push_task_event(
                                    &task_event_feed,
                                    format!("sup {} retried as {}", task.id, retry_id),
                                );
                                retried += 1;
                            }
                            persist_agent_os_state(&mut context, &agent_state);
                            context_manager.save(&context).await?;
                            println!("🔁 Queued {} retries from failed/cancelled tasks", retried);
                            continue;
                        }
                        if target == "completed" {
                            let candidates = agent_state
                                .supervisor
                                .tasks
                                .iter()
                                .filter(|task| task.status == SupervisedTaskStatus::Completed)
                                .cloned()
                                .collect::<Vec<_>>();
                            if candidates.is_empty() {
                                println!("ℹ️  No completed supervisor tasks to retry.");
                                continue;
                            }
                            let mut retried = 0usize;
                            for task in candidates {
                                let retry_id = enqueue_supervised_task(
                                    &mut agent_state,
                                    task.prompt.clone(),
                                    task.class.clone(),
                                );
                                push_task_event(
                                    &task_event_feed,
                                    format!("sup {} retried as {}", task.id, retry_id),
                                );
                                retried += 1;
                            }
                            persist_agent_os_state(&mut context, &agent_state);
                            context_manager.save(&context).await?;
                            println!("🔁 Queued {} retries from completed tasks", retried);
                            continue;
                        }
                        if target == "all" {
                            let candidates = agent_state
                                .supervisor
                                .tasks
                                .iter()
                                .filter(|task| {
                                    task.status == SupervisedTaskStatus::Completed
                                        || task.status == SupervisedTaskStatus::Failed
                                        || task.status == SupervisedTaskStatus::Cancelled
                                })
                                .cloned()
                                .collect::<Vec<_>>();
                            if candidates.is_empty() {
                                println!("ℹ️  No completed/failed/cancelled supervisor tasks to retry.");
                                continue;
                            }
                            let mut retried = 0usize;
                            for task in candidates {
                                let retry_id = enqueue_supervised_task(
                                    &mut agent_state,
                                    task.prompt.clone(),
                                    task.class.clone(),
                                );
                                push_task_event(
                                    &task_event_feed,
                                    format!("sup {} retried as {}", task.id, retry_id),
                                );
                                retried += 1;
                            }
                            persist_agent_os_state(&mut context, &agent_state);
                            context_manager.save(&context).await?;
                            println!(
                                "🔁 Queued {} retries from completed/failed/cancelled tasks",
                                retried
                            );
                            continue;
                        }
                        let snapshot = agent_state
                            .supervisor
                            .tasks
                            .iter()
                            .find(|task| task.id == target)
                            .cloned();
                        let Some(task) = snapshot else {
                            println!("❌ Supervisor task '{}' not found", target);
                            continue;
                        };
                        if matches!(
                            task.status,
                            SupervisedTaskStatus::Queued | SupervisedTaskStatus::Running
                        ) {
                            println!(
                                "⚠️  Task {} is currently {}. Stop/wait before retry.",
                                target,
                                format!("{:?}", task.status).to_lowercase()
                            );
                            continue;
                        }
                        let retry_id = enqueue_supervised_task(
                            &mut agent_state,
                            task.prompt.clone(),
                            task.class.clone(),
                        );
                        push_task_event(
                            &task_event_feed,
                            format!("sup {} retried as {}", target, retry_id),
                        );
                        persist_agent_os_state(&mut context, &agent_state);
                        context_manager.save(&context).await?;
                        println!("🔁 Queued retry {} from {}", retry_id, target);
                        continue;
                    }
                }

                if !input_from_queue && (input == "queue run" || input == "/queue run") {
                    match start_next_queued_supervised_task(&mut agent_state) {
                        QueueStartResult::Started(task) => {
                            if can_run_supervisor_in_background(&task) {
                                let bg_task_id = format!("supbg-{}", task.id);
                                let bg_description = format!(
                                    "supervisor {} {}",
                                    task.id,
                                    truncate_for_table(&task.prompt, 56)
                                );
                                let task_prompt = task.prompt.clone();
                                let task_id = task.id.clone();
                                let task_class = task.class.clone();
                                let timeout_bg =
                                    watchdog_timeout_for_class(&task_class, &agent_state.autonomy_mode);
                                let max_iter_bg = active_soul
                                    .max_iterations
                                    .min(
                                        execution_budget_for_class(
                                            &task_class,
                                            &agent_state.autonomy_mode,
                                        )
                                        .max_iterations,
                                    )
                                    .max(1);
                                let provider_bg = config.provider.clone();
                                let model_bg = config.model.clone();
                                let async_session_bg = async_session.clone();
                                let async_locks_bg = async_locks.clone();
                                let runtime_dispatcher_bg = runtime_dispatcher.clone();
                                let registry_arc_bg = registry_arc.clone();
                                let allowed_tools_bg = active_allowed_tools.clone();
                                let task_session_key = format!("{}::sup::{}", session_key, task_id);
                                let agent_name_bg = agent_name.clone();
                                let system_prompt_bg = augment_system_prompt_for_turn(
                                    &system_prompt,
                                    &agent_state.onboarding.system_profile,
                                    &capability_registry,
                                    &active_allowed_tools,
                                    &agent_state.autonomy_mode,
                                );

                                let spawn_result = task_manager
                                    .spawn_task(bg_task_id.clone(), bg_description, move || async move {
                                        let llm_client = build_llm_client_for_provider(
                                            &provider_bg,
                                            &model_bg,
                                        )
                                        .map_err(|e| format!("LLM client init failed: {}", e))?;
                                        let allowed_state_bg =
                                            Arc::new(RwLock::new(allowed_tools_bg));
                                        let runtime_registry_bg = Arc::new(
                                            RuntimeRegistryAdapter::new(
                                                registry_arc_bg,
                                                allowed_state_bg,
                                            ),
                                        );
                                        let compactor = hypr_claw_runtime::Compactor::new(
                                            4000,
                                            SimpleSummarizer,
                                        );
                                        let agent_loop_bg = hypr_claw_runtime::AgentLoop::new(
                                            async_session_bg,
                                            async_locks_bg,
                                            runtime_dispatcher_bg,
                                            runtime_registry_bg,
                                            llm_client,
                                            compactor,
                                            max_iter_bg,
                                        );
                                        match tokio::time::timeout(
                                            timeout_bg,
                                            agent_loop_bg.run(
                                                &task_session_key,
                                                &agent_name_bg,
                                                &system_prompt_bg,
                                                &task_prompt,
                                            ),
                                        )
                                        .await
                                        {
                                            Ok(Ok(response)) => Ok(response),
                                            Ok(Err(e)) => Err(e.to_string()),
                                            Err(_) => Err(format!(
                                                "Execution watchdog timeout after {}s",
                                                timeout_bg.as_secs()
                                            )),
                                        }
                                    })
                                    .await;

                                match spawn_result {
                                    Ok(background_id) => {
                                        set_supervised_task_background_id(
                                            &mut agent_state,
                                            &task_id,
                                            Some(background_id.clone()),
                                        );
                                        supervisor_background_map.insert(task_id.clone(), background_id);
                                        push_task_event(
                                            &task_event_feed,
                                            format!(
                                                "sup {} running(background) class={} res={}",
                                                task.id,
                                                task.class.as_str(),
                                                truncate_for_table(&task.resources.join(","), 22)
                                            ),
                                        );
                                        println!("▶ Started {} in background", task_id);
                                    }
                                    Err(e) => {
                                        let err = e.to_string();
                                        set_supervised_task_background_id(
                                            &mut agent_state,
                                            &task_id,
                                            None,
                                        );
                                        mark_supervised_task_failed(
                                            &mut agent_state,
                                            &task_id,
                                            err.clone(),
                                        );
                                        push_task_event(
                                            &task_event_feed,
                                            format!(
                                                "sup {} background start failed {}",
                                                task_id,
                                                truncate_for_table(&err, 56)
                                            ),
                                        );
                                        println!("❌ Failed to start {} in background: {}", task_id, err);
                                    }
                                }
                            } else {
                                queued_execution = Some(task);
                            }

                            if let Some(task) = queued_execution.as_ref() {
                                push_task_event(
                                    &task_event_feed,
                                    format!(
                                        "sup {} running class={} res={}",
                                        task.id,
                                        task.class.as_str(),
                                        truncate_for_table(&task.resources.join(","), 22)
                                    ),
                                );
                            }
                            persist_agent_os_state(&mut context, &agent_state);
                            context_manager.save(&context).await?;
                        }
                        QueueStartResult::Blocked(reason) => {
                            push_task_event(&task_event_feed, format!("sup queue blocked: {}", reason));
                            println!("⏸ Cannot start queued task: {}", reason);
                            continue;
                        }
                        QueueStartResult::Empty => {
                            push_task_event(&task_event_feed, "sup queue empty");
                            println!("ℹ️  No queued tasks.");
                            continue;
                        }
                    }
                }

                if input == "/models" || input == "models" {
                    let current_model = agent_loop
                        .current_model()
                        .unwrap_or_else(|| config.model.clone());
                    println!("\n🧠 Current model: {}", current_model);
                    println!("Fetching provider models...");
                    match agent_loop.list_models().await {
                        Ok(models) => {
                            let candidates = filter_agentic_models(&models);
                            if candidates.is_empty() {
                                println!("No models returned by provider.");
                                println!();
                                continue;
                            }
                            let limit = candidates.len().min(20);
                            println!("Top agent-friendly models:");
                            for (i, model) in candidates.iter().take(limit).enumerate() {
                                let marker = if model == &current_model { "*" } else { " " };
                                println!("  {} {:>2}. {}", marker, i + 1, model);
                            }
                            println!("\nUse '/models set <model_id>' to switch.");
                            let choice = prompt_line("Select model number (Enter to keep): ")?;
                            if choice.trim().is_empty() {
                                println!();
                                continue;
                            }
                            if let Ok(index) = choice.trim().parse::<usize>() {
                                if index >= 1 && index <= limit {
                                    let selected = &candidates[index - 1];
                                    if let Err(e) = apply_model_switch(
                                        selected,
                                        &agent_loop,
                                        &mut config,
                                        &context_manager,
                                        &mut context,
                                    ).await {
                                        eprintln!("❌ Failed to switch model: {}", e);
                                    }
                                } else {
                                    eprintln!("❌ Invalid model index");
                                }
                            } else {
                                eprintln!("❌ Invalid input");
                            }
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to fetch models: {}", e);
                            print_model_recommendations(&config.provider, &current_model);
                        }
                    }
                    println!();
                    continue;
                }

                if input == "/models list" || input == "models list" {
                    match agent_loop.list_models().await {
                        Ok(models) => {
                            let filtered = filter_agentic_models(&models);
                            println!("\n📦 Provider models ({}):", filtered.len());
                            for model in filtered.iter().take(60) {
                                println!("  {}", model);
                            }
                            if filtered.len() > 60 {
                                println!("  ... and {} more", filtered.len() - 60);
                            }
                            println!();
                        }
                        Err(e) => eprintln!("❌ Failed to list models: {}\n", e),
                    }
                    continue;
                }

                if let Some(model_id) = input
                    .strip_prefix("/models set ")
                    .or_else(|| input.strip_prefix("models set "))
                    .map(str::trim)
                {
                    if model_id.is_empty() {
                        eprintln!("❌ Usage: /models set <model_id>");
                        continue;
                    }
                    if let Err(e) = apply_model_switch(
                        model_id,
                        &agent_loop,
                        &mut config,
                        &context_manager,
                        &mut context,
                    ).await {
                        eprintln!("❌ Failed to switch model: {}", e);
                    }
                    continue;
                }

                if input.starts_with("approve ") {
                    println!("ℹ️  Approval checks are inline. System-critical tools prompt immediately.");
                    continue;
                }

                let (effective_input, task_class, supervisor_task_id) =
                    if let Some(queued_task) = queued_execution.take() {
                        println!(
                            "▶ Running queued task {} ({})",
                            queued_task.id,
                            queued_task.class.as_str()
                        );
                        (
                            queued_task.prompt.clone(),
                            queued_task.class.clone(),
                            Some(queued_task.id.clone()),
                        )
                    } else {
                        let class = classify_supervised_task_class(&input);
                        let running_background = task_manager
                            .list_tasks()
                            .await
                            .into_iter()
                            .filter(|task| task.status == hypr_claw_tasks::TaskStatus::Running)
                            .collect::<Vec<_>>();
                        if let Some(conflict_reason) =
                            running_background_conflict_reason(&input, &running_background)
                        {
                            println!(
                                "⚠️  Conflict detected with running background tasks: {}",
                                conflict_reason
                            );
                            println!("Choose: [q] queue new task, [r] run now, [c] cancel running background tasks and run");
                            let decision = prompt_line("Conflict action [q/r/c] (default q): ")?;
                            match decision.trim().to_lowercase().as_str() {
                                "" | "q" | "queue" => {
                                    let queued_id = enqueue_supervised_task(
                                        &mut agent_state,
                                        input.clone(),
                                        class.clone(),
                                    );
                                    persist_agent_os_state(&mut context, &agent_state);
                                    context_manager.save(&context).await?;
                                    push_task_event(
                                        &task_event_feed,
                                        format!("sup {} queued due to conflict", queued_id),
                                    );
                                    println!("🗂 Queued as {}", queued_id);
                                    continue;
                                }
                                "c" | "cancel" => {
                                    let running_ids = running_background
                                        .iter()
                                        .map(|t| t.id.clone())
                                        .collect::<Vec<_>>();
                                    let mut cancelled = 0usize;
                                    for task_id in running_ids {
                                        if task_manager.cancel_task(&task_id).await.is_ok() {
                                            cancelled += 1;
                                            push_task_event(
                                                &task_event_feed,
                                                format!("bg {} cancelled due to conflict policy", task_id),
                                            );
                                        }
                                    }
                                    context.active_tasks = to_context_tasks(task_manager.list_tasks().await);
                                    println!("🧹 Cancelled {} running background task(s)", cancelled);
                                }
                                "r" | "run" | "run_now" | "now" => {
                                    push_task_event(
                                        &task_event_feed,
                                        "sup conflict policy: run-now over background tasks",
                                    );
                                }
                                _ => {
                                    let queued_id = enqueue_supervised_task(
                                        &mut agent_state,
                                        input.clone(),
                                        class.clone(),
                                    );
                                    persist_agent_os_state(&mut context, &agent_state);
                                    context_manager.save(&context).await?;
                                    push_task_event(
                                        &task_event_feed,
                                        format!("sup {} queued due to invalid conflict choice", queued_id),
                                    );
                                    println!("🗂 Invalid choice. Queued as {}", queued_id);
                                    continue;
                                }
                            }
                        }

                        let task_id =
                            start_supervised_task(&mut agent_state, input.clone(), class.clone());
                        if let Some(task) = agent_state
                            .supervisor
                            .tasks
                            .iter()
                            .find(|task| task.id == task_id)
                        {
                            push_task_event(
                                &task_event_feed,
                                format!(
                                    "sup {} running class={} res={}",
                                    task.id,
                                    task.class.as_str(),
                                    truncate_for_table(&task.resources.join(","), 22)
                                ),
                            );
                        }
                        (
                            input.clone(),
                            class,
                            Some(task_id),
                        )
                    };

                context.recent_history.push(hypr_claw_memory::types::HistoryEntry {
                    timestamp: chrono::Utc::now().timestamp(),
                    role: "user".to_string(),
                    content: format!(
                        "[thread:{}] {}",
                        agent_state.active_thread_id, effective_input
                    ),
                    token_count: None,
                });
                touch_active_thread(&mut agent_state);
                context.current_plan = Some(plan_for_input(&effective_input));
                persist_agent_os_state(&mut context, &agent_state);
                context_manager.save(&context).await?;

                let task_session_key =
                    thread_session_key(&session_key, &agent_state.active_thread_id);
                let strict_workflow = strict_workflow_enabled();
                let focused_tools = focused_tools_for_input(&effective_input, &active_allowed_tools);
                let use_focused = !strict_workflow
                    && matches!(agent_state.autonomy_mode, AutonomyMode::Guarded)
                    && !focused_tools.is_empty()
                    && focused_tools.len() < active_allowed_tools.len();
                if use_focused {
                    runtime_registry.set_allowed_tools(focused_tools.clone());
                }

                let class_budget =
                    execution_budget_for_class(&task_class, &agent_state.autonomy_mode);
                let watchdog_timeout =
                    watchdog_timeout_for_class(&task_class, &agent_state.autonomy_mode);
                let effective_max_iterations = active_soul
                    .max_iterations
                    .min(class_budget.max_iterations)
                    .max(1);
                agent_loop.set_max_iterations(effective_max_iterations);
                let run_mode = agent_state.autonomy_mode.clone();
                let run_started_at = Instant::now();
                let mut fallback_attempts = 0u32;
                agent_state.reliability.run_id = agent_state.reliability.run_id.saturating_add(1);
                agent_state.reliability.last_stage = "running".to_string();
                agent_state.reliability.fallback_attempts = 0;
                agent_state.reliability.last_duration_ms = 0;
                agent_state.reliability.last_error.clear();
                agent_state.reliability.last_break_reason.clear();
                agent_state.reliability.updated_at = Some(chrono::Utc::now().timestamp());

                print_run_panel(
                    &effective_input,
                    task_class.as_str(),
                    agent_loop.max_iterations(),
                    watchdog_timeout.as_secs(),
                    &config.model,
                    active_allowed_tools.len(),
                    if use_focused {
                        Some(focused_tools.len())
                    } else {
                        None
                    },
                );

                let turn_system_prompt = augment_system_prompt_for_turn(
                    &system_prompt,
                    &agent_state.onboarding.system_profile,
                    &capability_registry,
                    &active_allowed_tools,
                    &agent_state.autonomy_mode,
                );
                let run_action_start = action_feed_len(&action_feed);
                let mut recovery_notes: Vec<String> = Vec::new();

                let mut run_result = run_with_interrupt_and_timeout(
                    &agent_loop,
                    &task_session_key,
                    &agent_name,
                    &turn_system_prompt,
                    &effective_input,
                    &interrupt,
                    watchdog_timeout,
                )
                .await;

                if use_focused {
                    runtime_registry.set_allowed_tools(active_allowed_tools.clone());
                }

                loop {
                    let err_msg = match &run_result {
                        Ok(_) => break,
                        Err(err) => err.to_string(),
                    };

                    let err_lower = err_msg.to_lowercase();
                    let is_rate_limit_error = err_lower.contains("rate limit")
                        || err_lower.contains("too many requests")
                        || err_lower.contains("resource_exhausted")
                        || err_lower.contains("429");
                    if is_rate_limit_error {
                        let wait_secs = extract_retry_after_seconds(&err_msg).unwrap_or(0);
                        if wait_secs > 0
                            && wait_secs <= 12
                            && fallback_attempts < MAX_RECOVERY_ATTEMPTS
                        {
                            fallback_attempts += 1;
                            agent_state.reliability.last_stage =
                                "recovery_provider_rate_limit".to_string();
                            agent_state.reliability.fallback_attempts = fallback_attempts;
                            let note = format!(
                                "[recovery {}/{}] provider_rate_limit -> retry in {}s",
                                fallback_attempts, MAX_RECOVERY_ATTEMPTS, wait_secs
                            );
                            recovery_notes.push(note.clone());
                            println!("{note}");
                            tokio::time::sleep(Duration::from_secs(wait_secs)).await;
                            run_result = run_with_interrupt_and_timeout(
                                &agent_loop,
                                &task_session_key,
                                &agent_name,
                                &turn_system_prompt,
                                &effective_input,
                                &interrupt,
                                watchdog_timeout,
                            )
                            .await;
                            continue;
                        }
                        agent_state.reliability.last_stage = "provider_rate_limited".to_string();
                        break;
                    }

                    if fallback_attempts >= MAX_RECOVERY_ATTEMPTS {
                        agent_state.reliability.last_stage =
                            "recovery_budget_exhausted".to_string();
                        agent_state.reliability.fallback_attempts = fallback_attempts;
                        break;
                    }

                    let is_tool_enforcement_error =
                        err_msg.contains("Tool invocation required but not performed");
                    let is_provider_argument_error =
                        err_msg.contains("INVALID_ARGUMENT") || err_msg.contains("400 Bad Request");
                    let is_watchdog_timeout_error =
                        err_msg.to_lowercase().contains("watchdog timeout");
                    let is_max_iterations_error = err_msg.contains("Max iterations")
                        || err_msg.contains("iteration limit")
                        || err_msg.contains("iterations (")
                        || err_msg.contains("reached after");
                    let is_tool_execution_error = err_msg.contains("Tool error:")
                        || err_msg.contains("dispatcher error")
                        || err_msg.contains("tool failed");

                    let mut recovery_prompt: Option<String> = None;
                    fallback_attempts += 1;
                    agent_state.reliability.fallback_attempts = fallback_attempts;
                    let prompt_first_mode =
                        matches!(agent_state.autonomy_mode, AutonomyMode::PromptFirst);
                    let note = format!(
                        "[recovery {}/{}] error={}",
                        fallback_attempts,
                        MAX_RECOVERY_ATTEMPTS,
                        truncate_for_table(&err_msg, 110)
                    );
                    recovery_notes.push(note.clone());
                    println!("{note}");

                    if is_tool_enforcement_error {
                        agent_state.reliability.last_stage = "recovery_tool_enforcement".to_string();
                        if strict_workflow || prompt_first_mode {
                            recovery_prompt = Some(format!(
                                "{}\n\nStrict workflow recovery:\n1) keep using available tools and choose the next best tool dynamically\n2) if one path fails, switch to another tool path immediately\n3) avoid repeating failed call shapes\n4) finish only when task is done or truly blocked.",
                                effective_input
                            ));
                        } else {
                            let fallback_playbook =
                                fallback_playbook_for_input(&effective_input, &active_allowed_tools);
                            recovery_prompt = Some(format!(
                                "{}\n\nExecute this now using available tools. Do not answer with explanation-only text.\nFallback strategy (deterministic):\n{}\nIf one tool fails, immediately move to the next fallback tool.",
                                effective_input, fallback_playbook
                            ));
                        }
                    } else if is_provider_argument_error {
                        agent_state.reliability.last_stage = "recovery_provider_argument".to_string();
                        if strict_workflow || prompt_first_mode {
                            recovery_prompt = Some(format!(
                                "{}\n\nProvider-compat recovery:\n1) emit exactly one tool call with strict JSON arguments\n2) avoid verbose narration\n3) if parsing fails, choose a simpler tool call and continue.",
                                effective_input
                            ));
                        } else {
                            let emergency_tools =
                                emergency_tool_subset(&effective_input, &active_allowed_tools);
                            if !emergency_tools.is_empty()
                                && emergency_tools.len() < active_allowed_tools.len()
                            {
                                runtime_registry.set_allowed_tools(emergency_tools);
                                run_result = run_with_interrupt_and_timeout(
                                    &agent_loop,
                                    &task_session_key,
                                    &agent_name,
                                    &turn_system_prompt,
                                    &effective_input,
                                    &interrupt,
                                    watchdog_timeout,
                                )
                                .await;
                                runtime_registry.set_allowed_tools(active_allowed_tools.clone());
                                continue;
                            }
                        }
                    } else if is_max_iterations_error
                        || is_tool_execution_error
                        || is_watchdog_timeout_error
                    {
                        agent_state.reliability.last_stage = "recovery_runtime".to_string();
                        if strict_workflow || prompt_first_mode {
                            recovery_prompt = Some(format!(
                                "{}\n\nStrict workflow runtime recovery:\n1) continue autonomously with a different tool strategy\n2) avoid prior failing call patterns\n3) verify result after each action\n4) return final status only when done or clearly blocked.",
                                effective_input
                            ));
                        } else {
                            let fallback_playbook =
                                fallback_playbook_for_input(&effective_input, &active_allowed_tools);
                            recovery_prompt = Some(format!(
                                "{}\n\nRecovery mode:\n1) keep working with available tools\n2) if a tool fails, pick the next fallback tool in this table:\n{}\n3) end with final status and exact remaining blocker only if no alternative worked.",
                                effective_input, fallback_playbook
                            ));
                        }
                    }

                    if let Some(prompt) = recovery_prompt {
                        run_result = run_with_interrupt_and_timeout(
                            &agent_loop,
                            &task_session_key,
                            &agent_name,
                            &turn_system_prompt,
                            &prompt,
                            &interrupt,
                            watchdog_timeout,
                        )
                        .await;
                        continue;
                    }

                    break;
                }

                agent_loop.set_max_iterations(active_soul.max_iterations);
                let run_elapsed_ms = run_started_at.elapsed().as_millis() as u64;

                match run_result {
                    Ok(response) => {
                        agent_state.reliability.last_stage = "completed".to_string();
                        agent_state.reliability.fallback_attempts = fallback_attempts;
                        agent_state.reliability.last_duration_ms = run_elapsed_ms;
                        agent_state.reliability.last_break_reason = "STOP_NONE".to_string();
                        agent_state.reliability.last_error.clear();
                        agent_state.reliability.updated_at = Some(chrono::Utc::now().timestamp());
                        record_autonomy_outcome(
                            &mut agent_state,
                            &run_mode,
                            true,
                            run_elapsed_ms,
                            fallback_attempts,
                            "STOP_NONE",
                        );
                        if let Some(task_id) = &supervisor_task_id {
                            if supervised_task_status(&agent_state, task_id)
                                != Some(SupervisedTaskStatus::Cancelled)
                            {
                                mark_supervised_task_completed(&mut agent_state, task_id);
                                push_task_event(
                                    &task_event_feed,
                                    format!("sup {} completed in {}ms", task_id, run_elapsed_ms),
                                );
                            } else {
                                push_task_event(
                                    &task_event_feed,
                                    format!("sup {} completion ignored (already cancelled)", task_id),
                                );
                            }
                        }
                        context.recent_history.push(hypr_claw_memory::types::HistoryEntry {
                            timestamp: chrono::Utc::now().timestamp(),
                            role: "assistant".to_string(),
                            content: format!("[thread:{}] {}", agent_state.active_thread_id, response),
                            token_count: None,
                        });
                        mark_plan_completed(&mut context, &response);
                        context.active_tasks = to_context_tasks(task_manager.list_tasks().await);
                        persist_agent_os_state(&mut context, &agent_state);
                        context_manager.save(&context).await?;
                        if transcript_view_mode {
                            let tool_rows = action_feed_since(&action_feed, run_action_start, 20);
                            let mut result_rows =
                                vec![format!("status: completed in {}ms", run_elapsed_ms)];
                            result_rows.push(format!("tool_events: {}", tool_rows.len()));
                            if !recovery_notes.is_empty() {
                                result_rows.push(format!(
                                    "recovery_attempts: {}",
                                    recovery_notes.len()
                                ));
                            }
                            print_transcript_panes(
                                &effective_input,
                                &config.model,
                                task_class.as_str(),
                                run_elapsed_ms,
                                &tool_rows,
                                &recovery_notes,
                                &result_rows,
                            );
                        }
                        println!();
                        println!("{}", ui_section("Assistant"));
                        println!("{}\n", strip_ansi_and_controls(&response));
                    }
                    Err(e) => {
                        let error_msg = e.to_string();
                        let stop_code = resolve_stop_code(&error_msg, fallback_attempts);
                        let hint = remediation_hint_for_stop_code(stop_code, &effective_input);
                        agent_state.reliability.last_stage = "failed".to_string();
                        agent_state.reliability.fallback_attempts = fallback_attempts;
                        agent_state.reliability.last_duration_ms = run_elapsed_ms;
                        agent_state.reliability.last_break_reason = stop_code.to_string();
                        agent_state.reliability.last_error = error_msg.clone();
                        agent_state.reliability.updated_at = Some(chrono::Utc::now().timestamp());
                        record_autonomy_outcome(
                            &mut agent_state,
                            &run_mode,
                            false,
                            run_elapsed_ms,
                            fallback_attempts,
                            stop_code,
                        );
                        if let Some(task_id) = &supervisor_task_id {
                            if supervised_task_status(&agent_state, task_id)
                                != Some(SupervisedTaskStatus::Cancelled)
                            {
                                mark_supervised_task_failed(
                                    &mut agent_state,
                                    task_id,
                                    error_msg.clone(),
                                );
                                push_task_event(
                                    &task_event_feed,
                                    format!(
                                        "sup {} failed code={} {}",
                                        task_id,
                                        stop_code,
                                        truncate_for_table(&error_msg, 60)
                                    ),
                                );
                            } else {
                                push_task_event(
                                    &task_event_feed,
                                    format!("sup {} failed ignored (already cancelled)", task_id),
                                );
                            }
                        }
                        mark_plan_failed(&mut context, &error_msg);
                        persist_agent_os_state(&mut context, &agent_state);
                        context_manager.save(&context).await?;
                        if transcript_view_mode {
                            let tool_rows = action_feed_since(&action_feed, run_action_start, 20);
                            let mut result_rows = vec![format!(
                                "status: failed {} in {}ms",
                                stop_code, run_elapsed_ms
                            )];
                            result_rows.extend(text_preview_lines(&error_msg, 2, 112));
                            if !hint.is_empty() {
                                result_rows.push(format!("next: {}", &hint));
                            }
                            print_transcript_panes(
                                &effective_input,
                                &config.model,
                                task_class.as_str(),
                                run_elapsed_ms,
                                &tool_rows,
                                &recovery_notes,
                                &result_rows,
                            );
                        }
                        if error_msg.contains("Interrupted by user") {
                            println!("⏹ Request interrupted by user.\n");
                        } else {
                            eprintln!("❌ Error [{}]: {}", stop_code, e);
                            if !hint.is_empty() {
                                eprintln!("💡 Next step: {}", hint);
                            }
                            eprintln!();
                        }
                    }
                }
            }
        }
    }

    context.active_tasks = to_context_tasks(task_manager.list_tasks().await);
    persist_agent_os_state(&mut context, &agent_state);
    context_manager.save(&context).await?;

    Ok(())
}

fn handle_config_reset() -> Result<(), Box<dyn std::error::Error>> {
    println!("Resetting configuration...");

    Config::delete()?;

    if let Err(e) = bootstrap::delete_nvidia_api_key() {
        // Ignore if key doesn't exist
        if !e.to_string().contains("not found") {
            eprintln!("⚠️  Warning: Failed to delete NVIDIA API key: {}", e);
        }
    }

    if let Err(e) = bootstrap::delete_google_api_key() {
        // Ignore if key doesn't exist
        if !e.to_string().contains("not found") {
            eprintln!("⚠️  Warning: Failed to delete Google API key: {}", e);
        }
    }

    println!("✅ Configuration reset. Run hypr-claw again to reconfigure.");
    Ok(())
}

fn initialize_directories() -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all("./data/sessions")?;
    std::fs::create_dir_all("./data/credentials")?;
    std::fs::create_dir_all("./data/agents")?;
    std::fs::create_dir_all("./data/context")?;
    std::fs::create_dir_all("./data/tasks")?;
    std::fs::create_dir_all("./data/capabilities")?;

    if !std::path::Path::new("./data/audit.log").exists() {
        std::fs::File::create("./data/audit.log")?;
    }

    std::fs::create_dir_all("./sandbox")?;

    // Create default agent config if it doesn't exist
    let default_agent_config = "./data/agents/default.yaml";
    let default_agent_soul = "./data/agents/default_soul.md";

    if !std::path::Path::new(default_agent_config).exists() {
        std::fs::write(
            default_agent_config,
            "id: default\nsoul: default_soul.md\ntools:\n  - echo\n  - fs.read\n  - fs.write\n  - fs.list\n  - fs.create_dir\n  - fs.move\n  - fs.copy\n  - fs.delete\n  - hypr.workspace.switch\n  - hypr.workspace.move_window\n  - hypr.window.focus\n  - hypr.window.close\n  - hypr.window.move\n  - hypr.exec\n  - proc.spawn\n  - proc.kill\n  - proc.list\n  - desktop.open_url\n  - desktop.launch_app\n  - desktop.launch_app_and_wait_text\n  - desktop.search_web\n  - desktop.open_gmail\n  - desktop.type_text\n  - desktop.key_press\n  - desktop.key_combo\n  - desktop.mouse_click\n  - desktop.capture_screen\n  - desktop.active_window\n  - desktop.list_windows\n  - desktop.cursor_position\n  - desktop.read_screen_state\n  - desktop.mouse_move\n  - desktop.mouse_move_and_verify\n  - desktop.click_at\n  - desktop.click_at_and_verify\n  - desktop.ocr_screen\n  - desktop.find_text\n  - desktop.click_text\n  - desktop.wait_for_text\n  - wallpaper.set\n  - system.memory\n  - system.battery\n"
        )?;
    }

    if !std::path::Path::new(default_agent_soul).exists() {
        std::fs::write(
            default_agent_soul,
            "You are a local Linux + Hyprland OS assistant. Follow strict workflow: observe -> plan -> execute tool -> verify -> continue until done. Choose tools dynamically from allowed set. Ask permission before destructive/high-impact actions. Use mouse+keyboard style actions for GUI tasks.",
        )?;
    }

    Ok(())
}

struct SoulProfile {
    system_prompt: String,
    max_iterations: usize,
}

fn power_agent_profile() -> SoulProfile {
    SoulProfile {
        system_prompt: "You are a powerful local Linux + Hyprland OS assistant.\nFollow strict workflow: observe -> plan -> execute tool -> verify -> continue until done.\nFor GUI tasks, observe with desktop.read_screen_state/active_window/list_windows/cursor_position first, then act with cursor + keyboard tools.\nChoose tools dynamically from allowed_tools_now.\nAsk for permission before destructive/high-impact actions.\nAvoid hardcoded assumptions and adapt from live system/context.".to_string(),
        max_iterations: 36,
    }
}

fn normalize_runtime_tool_name(name: &str) -> String {
    match name {
        "screen.capture" | "capture.screen" => "desktop.capture_screen".to_string(),
        "screen.ocr" | "ocr.screen" => "desktop.ocr_screen".to_string(),
        "screen.find_text" | "text.find_on_screen" => "desktop.find_text".to_string(),
        "screen.click_text" | "text.click_on_screen" => "desktop.click_text".to_string(),
        "gmail.open" => "desktop.open_gmail".to_string(),
        "browser.open_url" => "desktop.open_url".to_string(),
        "browser.search" => "desktop.search_web".to_string(),
        "app.open" | "app.launch" => "desktop.launch_app".to_string(),
        "process.spawn" => "proc.spawn".to_string(),
        "process.kill" => "proc.kill".to_string(),
        "process.list" => "proc.list".to_string(),
        other => other.to_string(),
    }
}

fn removed_feature_notice(input: &str) -> Option<&'static str> {
    let cmd = input.trim().to_ascii_lowercase();

    if cmd == "tui"
        || cmd == "/tui"
        || cmd.starts_with("tui ")
        || cmd.starts_with("/tui ")
        || cmd == "repl"
        || cmd == "/repl"
    {
        return Some("ℹ️ TUI/REPL mode switching has been removed. Use the single streamlined prompt workflow.");
    }
    if cmd == "dashboard" || cmd == "dash" || cmd == "/dashboard" {
        return Some("ℹ️ Dashboard mode has been removed to reduce bloat. Use `status` for a lightweight snapshot.");
    }
    if cmd.starts_with("soul") || cmd.starts_with("/soul ") {
        return Some(
            "ℹ️ Soul system has been removed. A single `power_agent` profile is always active.",
        );
    }
    if cmd.starts_with("autonomy") || cmd.starts_with("/autonomy ") {
        return Some(
            "ℹ️ Autonomy modes were removed. The agent always runs in strict power workflow mode.",
        );
    }
    if cmd == "trust" || cmd == "/trust" || cmd.starts_with("trust ") || cmd.starts_with("/trust ")
    {
        return Some("ℹ️ Trust/safe mode toggles were removed. Permission flow is handled directly per action.");
    }
    if cmd == "/task" || cmd.starts_with("/task ") {
        return Some("ℹ️ Task-thread commands were removed to simplify workflow. Use `queue add|run|status|clear`.");
    }
    if cmd.starts_with("queue prune")
        || cmd.starts_with("/queue prune")
        || cmd.starts_with("queue inspect")
        || cmd.starts_with("/queue inspect")
        || cmd.starts_with("queue events")
        || cmd.starts_with("/queue events")
        || cmd.starts_with("queue stop")
        || cmd.starts_with("/queue stop")
        || cmd.starts_with("queue retry")
        || cmd.starts_with("/queue retry")
        || cmd.starts_with("queue auto")
        || cmd.starts_with("/queue auto")
    {
        return Some("ℹ️ Advanced queue controls were removed. Supported queue commands: `queue`, `queue status`, `queue add <prompt>`, `queue run`, `queue clear`.");
    }

    None
}

fn derive_runtime_allowed_tools(
    registry: &Arc<hypr_claw_tools::ToolRegistryImpl>,
    capability_registry: &Value,
) -> HashSet<String> {
    let has_array_entries = |path: &str| {
        capability_registry
            .pointer(path)
            .and_then(|v| v.as_array())
            .map(|arr| !arr.is_empty())
            .unwrap_or(false)
    };
    let ocr_available = capability_registry
        .pointer("/capabilities/ocr_available")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let hyprland_available = capability_registry
        .pointer("/platform/hyprland_available")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let input_backends =
        read_string_array_from_value(capability_registry.pointer("/capabilities/input_backends"));
    let has_keyboard_backend = input_backends
        .iter()
        .any(|b| matches!(b.as_str(), "wtype" | "ydotool"));
    let has_pointer_backend = input_backends
        .iter()
        .any(|b| matches!(b.as_str(), "ydotool" | "wlrctl"));
    let has_screenshot_backend = has_array_entries("/capabilities/screenshot_backends");
    let has_wallpaper_backend = has_array_entries("/capabilities/wallpaper_backends");

    registry
        .list()
        .into_iter()
        .filter(|tool| match tool.as_str() {
            "hypr.workspace.switch"
            | "hypr.workspace.move_window"
            | "hypr.window.focus"
            | "hypr.window.close"
            | "hypr.window.move"
            | "hypr.exec" => hyprland_available,
            "wallpaper.set" => has_wallpaper_backend,
            "desktop.capture_screen" => has_screenshot_backend,
            "desktop.ocr_screen" | "desktop.find_text" => has_screenshot_backend && ocr_available,
            "desktop.wait_for_text" | "desktop.launch_app_and_wait_text" => {
                has_screenshot_backend && ocr_available
            }
            "desktop.cursor_position" => hyprland_available,
            "desktop.mouse_move_and_verify" | "desktop.click_at_and_verify" => {
                has_pointer_backend && hyprland_available
            }
            "desktop.read_screen_state" => hyprland_available || has_screenshot_backend,
            "desktop.type_text" | "desktop.key_press" | "desktop.key_combo" => {
                has_keyboard_backend
            }
            "desktop.mouse_click" | "desktop.mouse_move" | "desktop.click_at" => {
                has_pointer_backend
            }
            "desktop.click_text" => has_pointer_backend && has_screenshot_backend && ocr_available,
            _ => true,
        })
        .collect()
}

#[cfg(test)]
fn max_view_offset(total: usize, page: usize) -> usize {
    let page = page.max(1);
    total.saturating_sub(page)
}

#[cfg(test)]
fn tail_window_slice<T: Clone>(
    items: &[T],
    offset_from_latest: usize,
    page_size: usize,
) -> (Vec<T>, usize, usize, usize) {
    if items.is_empty() {
        return (Vec::new(), 0, 0, 0);
    }
    let page = page_size.max(1);
    let total = items.len();
    let clamped_offset = offset_from_latest.min(max_view_offset(total, page));
    let end = total.saturating_sub(clamped_offset);
    let start = end.saturating_sub(page);
    (items[start..end].to_vec(), start + 1, end, total)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskStateDigest {
    status: String,
    progress_percent: u16,
    error: String,
    result: String,
}

fn now_hms() -> String {
    chrono::Utc::now().format("%H:%M:%S").to_string()
}

fn push_task_event(feed: &Arc<Mutex<Vec<String>>>, message: impl Into<String>) {
    let line = format!("{} {}", now_hms(), message.into());
    if let Ok(mut rows) = feed.lock() {
        rows.push(line);
        if rows.len() > 320 {
            let drop_count = rows.len() - 320;
            rows.drain(0..drop_count);
        }
    }
}

fn digest_task_state(task: &hypr_claw_tasks::TaskInfo) -> TaskStateDigest {
    TaskStateDigest {
        status: format!("{:?}", task.status).to_lowercase(),
        progress_percent: (task.progress.clamp(0.0, 1.0) * 100.0).round() as u16,
        error: task.error.clone().unwrap_or_default(),
        result: task.result.clone().unwrap_or_default(),
    }
}

fn sync_background_task_events(
    task_list: &[hypr_claw_tasks::TaskInfo],
    index: &mut HashMap<String, TaskStateDigest>,
    feed: &Arc<Mutex<Vec<String>>>,
) {
    let mut seen = HashSet::new();

    for task in task_list {
        seen.insert(task.id.clone());
        let next = digest_task_state(task);
        match index.get(&task.id) {
            None => {
                push_task_event(
                    feed,
                    format!(
                        "bg {} created status={} {}%",
                        task.id, next.status, next.progress_percent
                    ),
                );
            }
            Some(prev) if prev != &next => {
                let mut detail = format!(
                    "bg {} {}->{} {}%->{}%",
                    task.id, prev.status, next.status, prev.progress_percent, next.progress_percent
                );
                if !next.error.is_empty() {
                    detail.push_str(&format!(" err={}", truncate_for_table(&next.error, 40)));
                } else if !next.result.is_empty() {
                    detail.push_str(&format!(" out={}", truncate_for_table(&next.result, 40)));
                }
                push_task_event(feed, detail);
            }
            _ => {}
        }
        index.insert(task.id.clone(), next);
    }

    let removed = index
        .keys()
        .filter(|id| !seen.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    for id in removed {
        index.remove(&id);
        push_task_event(feed, format!("bg {} removed from active cache", id));
    }
}

#[cfg(test)]
fn build_task_log_lines(
    task_list: &[hypr_claw_tasks::TaskInfo],
    agent_state: &AgentOsState,
    task_event_feed: &Arc<Mutex<Vec<String>>>,
) -> Vec<String> {
    let mut rows: Vec<(i64, String)> = Vec::new();

    if let Ok(events) = task_event_feed.lock() {
        for line in events.iter() {
            rows.push((chrono::Utc::now().timestamp(), format!("evt {}", line)));
        }
    }

    for task in task_list {
        let mut line = format!(
            "bg {} {} {:>3}% {}",
            task.id,
            format!("{:?}", task.status).to_lowercase(),
            (task.progress.clamp(0.0, 1.0) * 100.0).round() as u16,
            truncate_for_table(&task.description, 44),
        );
        if let Some(err) = &task.error {
            line.push_str(&format!(" | err: {}", truncate_for_table(err, 36)));
        } else if let Some(result) = &task.result {
            line.push_str(&format!(" | out: {}", truncate_for_table(result, 36)));
        }
        rows.push((task.updated_at, line));
    }

    for task in &agent_state.supervisor.tasks {
        let mut line = format!(
            "sup {} {} {} {}",
            task.id,
            format!("{:?}", task.status).to_lowercase(),
            task.class.as_str(),
            truncate_for_table(&task.prompt, 36),
        );
        if !task.resources.is_empty() {
            line.push_str(&format!(
                " | res: {}",
                truncate_for_table(&task.resources.join(","), 20)
            ));
        }
        if let Some(bg_id) = &task.background_task_id {
            line.push_str(&format!(" | bg: {}", truncate_for_table(bg_id, 18)));
        }
        if let Some(err) = &task.error {
            line.push_str(&format!(" | err: {}", truncate_for_table(err, 34)));
        }
        rows.push((task.updated_at, line));
    }

    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.into_iter().map(|(_, line)| line).collect()
}

const UI_RESET: &str = "\x1b[0m";
const UI_BOLD: &str = "\x1b[1m";
const UI_DIM: &str = "\x1b[2m";
const UI_ACCENT: &str = "\x1b[38;5;39m";
const UI_INFO: &str = "\x1b[38;5;81m";
const UI_SUCCESS: &str = "\x1b[38;5;42m";
const UI_WARN: &str = "\x1b[38;5;214m";
const UI_DANGER: &str = "\x1b[38;5;203m";

fn use_color() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

fn paint(text: &str, style: &str) -> String {
    if use_color() {
        format!("{style}{text}{UI_RESET}")
    } else {
        text.to_string()
    }
}

fn ui_title(text: &str) -> String {
    paint(text, UI_BOLD)
}

fn ui_accent(text: &str) -> String {
    paint(text, UI_ACCENT)
}

fn ui_dim(text: &str) -> String {
    paint(text, UI_DIM)
}

fn ui_info(text: &str) -> String {
    paint(text, UI_INFO)
}

fn ui_success(text: &str) -> String {
    paint(text, UI_SUCCESS)
}

fn ui_warn(text: &str) -> String {
    paint(text, UI_WARN)
}

fn ui_danger(text: &str) -> String {
    paint(text, UI_DANGER)
}

fn ui_divider() -> String {
    ui_accent(&"─".repeat(78))
}

fn ui_section(title: &str) -> String {
    let mut line = format!("─ {} ", title);
    let fill = 78usize.saturating_sub(line.chars().count());
    line.push_str(&"─".repeat(fill));
    ui_accent(&line)
}

fn status_badge(status: &hypr_claw_tasks::TaskStatus) -> String {
    match status {
        hypr_claw_tasks::TaskStatus::Running => ui_info("RUN"),
        hypr_claw_tasks::TaskStatus::Pending => ui_warn("PEND"),
        hypr_claw_tasks::TaskStatus::Completed => ui_success("DONE"),
        hypr_claw_tasks::TaskStatus::Failed => ui_danger("FAIL"),
        hypr_claw_tasks::TaskStatus::Cancelled => ui_warn("STOP"),
    }
}

fn progress_bar(progress: f32, width: usize) -> String {
    let p = progress.clamp(0.0, 1.0);
    let filled = (p * width as f32).round() as usize;
    let filled = filled.min(width);
    let mut bar = String::new();
    bar.push('[');
    bar.push_str(&"█".repeat(filled));
    bar.push_str(&"░".repeat(width.saturating_sub(filled)));
    bar.push(']');
    bar
}

fn short_model_name(model: &str) -> String {
    model.split('/').next_back().unwrap_or(model).to_string()
}

fn format_timestamp(ts: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

fn truncate_for_table(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

fn strip_ansi_and_controls(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == 0x1B {
            i += 1;
            if i >= bytes.len() {
                break;
            }
            match bytes[i] {
                b'[' => {
                    i += 1;
                    while i < bytes.len() {
                        let b = bytes[i];
                        i += 1;
                        if (0x40..=0x7E).contains(&b) {
                            break;
                        }
                    }
                    continue;
                }
                b']' => {
                    i += 1;
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1B && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                    continue;
                }
                _ => continue,
            }
        }

        let rest = &raw[i..];
        let Some(ch) = rest.chars().next() else {
            break;
        };
        i += ch.len_utf8();
        if ch.is_control() && ch != '\n' && ch != '\t' {
            continue;
        }
        out.push(ch);
    }

    out
}

fn sanitize_single_line(raw: &str) -> String {
    strip_ansi_and_controls(raw)
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn sanitize_user_input_line(raw: &str) -> String {
    let mut value = sanitize_single_line(raw.trim());
    for _ in 0..4 {
        if let Some(rest) = value.strip_prefix("tui> ") {
            value = sanitize_single_line(rest.trim());
            continue;
        }
        if value.starts_with("hypr[") {
            if let Some((_, rest)) = value.rsplit_once("]>") {
                let cleaned = rest.trim();
                if !cleaned.is_empty() {
                    value = sanitize_single_line(cleaned);
                    continue;
                }
            }
        }
        break;
    }
    value
}

fn print_console_bootstrap(
    provider_name: &str,
    model: &str,
    agent_name: &str,
    display_name: &str,
    user_id: &str,
    session_key: &str,
    active_thread_id: &str,
) {
    println!("{}", ui_section("Hypr-Claw"));
    println!(
        "{} {}  {} {}  {} {}",
        ui_dim("provider"),
        ui_info(&truncate_for_table(provider_name, 20)),
        ui_dim("model"),
        ui_info(&truncate_for_table(&short_model_name(model), 24)),
        ui_dim("session"),
        truncate_for_table(session_key, 26)
    );
    println!(
        "{} {}  {} {}  {} {}",
        ui_dim("user"),
        truncate_for_table(&format!("{} ({})", display_name, user_id), 24),
        ui_dim("agent"),
        truncate_for_table(agent_name, 16),
        ui_dim("thread"),
        truncate_for_table(active_thread_id, 14)
    );
    println!(
        "{} {}",
        ui_dim("commands"),
        truncate_for_table(
            "help  status  scan  capabilities  /models  tasks  queue add/run/status/clear  view",
            64
        )
    );
    println!("{}", ui_divider());
    println!();
}

fn print_run_panel(
    prompt: &str,
    class_label: &str,
    max_iterations: usize,
    timeout_secs: u64,
    model: &str,
    active_tools: usize,
    focused_tools: Option<usize>,
) {
    println!();
    println!("{}", ui_section("Run"));
    println!(
        "{} {}",
        ui_dim("goal"),
        truncate_for_table(&sanitize_single_line(prompt), 112)
    );
    println!(
        "{} power  {}  iter={}  timeout={}s  model={}",
        ui_dim("plan"),
        class_label,
        max_iterations,
        timeout_secs,
        short_model_name(model)
    );
    println!("{} observe -> plan -> execute -> verify", ui_dim("flow"));
    let focused = focused_tools
        .map(|count| format!(" (focused {})", count))
        .unwrap_or_default();
    println!("{} {} active{}", ui_dim("tools"), active_tools, focused);
}

fn print_help() {
    println!("\n{}", ui_title("Hypr-Claw Command Reference"));
    println!("  {}", ui_accent("Core"));
    println!("    help                  Show this help");
    println!("    status                Runtime state snapshot");
    println!("    scan                  Re-run standard/deep system learning scan");
    println!("    capabilities          Show runtime capability registry summary");
    println!("    clear                 Clear terminal");
    println!("    interrupt             Send interrupt signal to active run");
    println!("    exit | quit           Exit agent");
    println!("  {}", ui_accent("Models"));
    println!("    /models               Interactive model switch");
    println!("    /models list          List provider models");
    println!("    /models set <id>      Set model directly");
    println!("  {}", ui_accent("Tasks"));
    println!("    tasks                 Show background task table");
    println!("    queue                 Show supervisor queue");
    println!("    queue status          Compact queue summary + actionable IDs");
    println!("    queue add <prompt>    Add task prompt to queue");
    println!("    queue run             Run next queued task");
    println!("    queue clear           Cancel queued items");
    println!("  {}", ui_accent("System"));
    println!("    profile               Show learned system profile");
    println!("    scan                  Re-run system scan");
    println!("    view                  Show current CLI view mode");
    println!("    view transcript       Enable transcript panes");
    println!("    view compact          Disable transcript panes");
    println!();
}

fn action_feed_len(feed: &Arc<Mutex<Vec<String>>>) -> usize {
    feed.lock().ok().map(|f| f.len()).unwrap_or(0)
}

fn action_feed_since(feed: &Arc<Mutex<Vec<String>>>, start: usize, max_rows: usize) -> Vec<String> {
    let rows = feed.lock().ok().map(|f| f.clone()).unwrap_or_default();
    if start >= rows.len() {
        return Vec::new();
    }
    let mut slice = rows[start..].to_vec();
    if max_rows > 0 && slice.len() > max_rows {
        let keep_from = slice.len().saturating_sub(max_rows);
        slice = slice[keep_from..].to_vec();
    }
    slice
}

fn text_preview_lines(text: &str, max_lines: usize, max_width: usize) -> Vec<String> {
    if text.trim().is_empty() {
        return vec!["(empty)".to_string()];
    }
    let sanitized = strip_ansi_and_controls(text);
    let mut lines = Vec::new();
    for line in sanitized.lines().take(max_lines) {
        lines.push(truncate_for_table(&sanitize_single_line(line), max_width));
    }
    if sanitized.lines().count() > max_lines {
        lines.push(format!("... (truncated, {}+ lines)", max_lines));
    }
    if lines.is_empty() {
        lines.push(truncate_for_table(&sanitize_single_line(&sanitized), max_width));
    }
    lines
}

fn print_transcript_panes(
    prompt: &str,
    model: &str,
    class_label: &str,
    run_elapsed_ms: u64,
    tool_rows: &[String],
    recovery_rows: &[String],
    result_rows: &[String],
) {
    println!("{}", ui_section("Transcript"));
    println!(
        "{} {}  {} {}  {} {}ms",
        ui_dim("model"),
        ui_info(&short_model_name(model)),
        ui_dim("class"),
        ui_info(class_label),
        ui_dim("elapsed"),
        ui_info(&run_elapsed_ms.to_string())
    );
    let prompt_rows = text_preview_lines(prompt, 2, 104);
    if let Some(first) = prompt_rows.first() {
        println!("{} {}", ui_dim("prompt"), first);
    }
    for row in prompt_rows.iter().skip(1) {
        println!("{} {}", ui_dim("      "), row);
    }
    if tool_rows.is_empty() {
        println!("{} {}", ui_dim("tools "), ui_dim("(no tool calls)"));
    } else {
        let mut first = true;
        for row in tool_rows {
            if first {
                println!("{} {}", ui_dim("tools "), truncate_for_table(row, 104));
                first = false;
            } else {
                println!("{} {}", ui_dim("      "), truncate_for_table(row, 104));
            }
        }
    }
    if recovery_rows.is_empty() {
        println!("{} {}", ui_dim("retry "), ui_dim("(none)"));
    } else {
        let mut first = true;
        for row in recovery_rows {
            if first {
                println!("{} {}", ui_dim("retry "), truncate_for_table(row, 104));
                first = false;
            } else {
                println!("{} {}", ui_dim("      "), truncate_for_table(row, 104));
            }
        }
    }
    let mut first = true;
    for row in result_rows {
        if first {
            println!("{} {}", ui_dim("result"), truncate_for_table(row, 104));
            first = false;
        } else {
            println!("{} {}", ui_dim("      "), truncate_for_table(row, 104));
        }
    }
    println!("{}", ui_divider());
}

fn print_tasks_panel(task_list: &[hypr_claw_tasks::TaskInfo]) {
    println!("\n{}", ui_title("Background Task Monitor"));
    println!(
        "  {:<14} {:<8} {:>6}  {:<18} {}",
        "ID", "STATE", "PROG%", "PROGRESS", "DESCRIPTION"
    );
    println!("  {}", ui_dim(&"─".repeat(88)));

    if task_list.is_empty() {
        println!(
            "  {:<14} {:<8} {:>6}  {:<18} {}",
            "-",
            "-",
            "-",
            progress_bar(0.0, 16),
            ui_dim("no tasks")
        );
        return;
    }

    let mut sorted = task_list.to_vec();
    sorted.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    for task in sorted {
        let prog = task.progress.clamp(0.0, 1.0);
        let bar = progress_bar(prog, 16);
        println!(
            "  {:<14} {:<8} {:>5.0}%  {:<18} {}",
            truncate_for_table(&task.id, 14),
            status_badge(&task.status),
            task.progress * 100.0,
            bar,
            truncate_for_table(&task.description, 44),
        );
    }
}

fn reliability_summary(state: &AgentOsState) -> String {
    let rel = &state.reliability;
    let err = if rel.last_error.is_empty() {
        "none".to_string()
    } else {
        truncate_for_table(&rel.last_error, 56)
    };
    format!(
        "run={} stage={} fallbacks={} duration={}ms break={} err={}",
        rel.run_id,
        rel.last_stage,
        rel.fallback_attempts,
        rel.last_duration_ms,
        if rel.last_break_reason.is_empty() {
            "none"
        } else {
            rel.last_break_reason.as_str()
        },
        err
    )
}

fn record_autonomy_outcome(
    state: &mut AgentOsState,
    mode: &AutonomyMode,
    succeeded: bool,
    duration_ms: u64,
    fallback_attempts: u32,
    stop_code: &str,
) {
    let metrics = match mode {
        AutonomyMode::PromptFirst => &mut state.autonomy_calibration.prompt_first,
        AutonomyMode::Guarded => &mut state.autonomy_calibration.guarded,
    };
    metrics.runs = metrics.runs.saturating_add(1);
    if succeeded {
        metrics.successes = metrics.successes.saturating_add(1);
    } else {
        metrics.failures = metrics.failures.saturating_add(1);
    }
    metrics.total_duration_ms = metrics.total_duration_ms.saturating_add(duration_ms);
    metrics.total_fallback_attempts = metrics
        .total_fallback_attempts
        .saturating_add(fallback_attempts as u64);
    let entry = metrics.stop_codes.entry(stop_code.to_string()).or_insert(0);
    *entry = entry.saturating_add(1);
}

fn extract_retry_after_seconds(error_msg: &str) -> Option<u64> {
    let mut num = String::new();
    for ch in error_msg.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            num.push(ch);
            continue;
        }
        if matches!(ch, 's' | 'S') && !num.is_empty() {
            if let Ok(value) = num.parse::<f64>() {
                if value > 0.0 {
                    return Some(value.ceil() as u64);
                }
            }
        }
        num.clear();
    }
    None
}

fn stop_code_for_error(error_msg: &str) -> &'static str {
    let lower = error_msg.to_lowercase();
    if lower.contains("interrupted by user") {
        "STOP_USER_INTERRUPT"
    } else if lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("resource_exhausted")
        || lower.contains("429")
    {
        "STOP_PROVIDER_RATE_LIMIT"
    } else if lower.contains("watchdog timeout") {
        "STOP_WATCHDOG_TIMEOUT"
    } else if lower.contains("repetitive tool loop") {
        "STOP_TOOL_LOOP"
    } else if lower.contains("consecutive tool failures") {
        "STOP_TOOL_FAILURE_STREAK"
    } else if lower.contains("tool invocation required but not performed")
        || lower.contains("no tool call completed successfully")
    {
        "STOP_TOOL_ENFORCEMENT"
    } else if lower.contains("invalid_argument") || lower.contains("400 bad request") {
        "STOP_PROVIDER_ARGUMENT"
    } else if lower.contains("max iterations")
        || lower.contains("iteration limit")
        || lower.contains("reached after")
    {
        "STOP_MAX_ITERATIONS"
    } else {
        "STOP_RUNTIME_ERROR"
    }
}

fn resolve_stop_code(error_msg: &str, fallback_attempts: u32) -> &'static str {
    let base = stop_code_for_error(error_msg);
    if fallback_attempts >= MAX_RECOVERY_ATTEMPTS && base != "STOP_USER_INTERRUPT" {
        "STOP_RECOVERY_BUDGET_EXHAUSTED"
    } else {
        base
    }
}

fn remediation_hint_for_stop_code(stop_code: &str, input: &str) -> String {
    let lower = input.to_lowercase();
    match stop_code {
        "STOP_USER_INTERRUPT" => "Request was interrupted. Re-run the task or queue it with 'queue add <prompt>'.".to_string(),
        "STOP_WATCHDOG_TIMEOUT" => "Task exceeded time budget. Break it into smaller steps or run one concrete action per prompt.".to_string(),
        "STOP_TOOL_LOOP" => "Model got stuck on repeated tool calls. Try a more specific prompt with exact app/path/target window.".to_string(),
        "STOP_TOOL_FAILURE_STREAK" => "Multiple tools failed in a row. Verify required apps/backends are installed and available in PATH.".to_string(),
        "STOP_TOOL_ENFORCEMENT" => "No valid actionable tool path was produced. Use an imperative prompt like 'open X', 'create Y', or 'run Z'.".to_string(),
        "STOP_PROVIDER_ARGUMENT" => "Provider rejected the request format. Switch model with '/models' or retry after reducing prompt complexity.".to_string(),
        "STOP_PROVIDER_RATE_LIMIT" => "Provider quota/rate limit hit. Wait for retry window, switch to a lower-traffic model, or use a different provider key.".to_string(),
        "STOP_MAX_ITERATIONS" => {
            if lower.contains("gmail") || lower.contains("telegram") || lower.contains("mail") {
                "For inbox/chat tasks, request one concrete step first (open + capture), then ask for summary in next prompt.".to_string()
            } else if lower.contains("wallpaper") {
                "Specify one backend explicitly (e.g. 'use caelestia wallpaper -f <path>').".to_string()
            } else if lower.contains("vscode") || lower.contains("code") {
                "Set an explicit launch command for your editor in system profile/capabilities, then retry.".to_string()
            } else {
                "Task planning hit iteration budget. Narrow the scope and retry with fewer combined actions.".to_string()
            }
        }
        "STOP_RECOVERY_BUDGET_EXHAUSTED" => "Automatic recovery attempts were exhausted. Retry with a narrower prompt or switch model.".to_string(),
        _ => "Unexpected runtime failure. Run 'status' for telemetry, then retry with a narrower prompt.".to_string(),
    }
}

fn print_status_panel(
    session_key: &str,
    agent_name: &str,
    display_name: &str,
    model: &str,
    active_thread_id: &str,
    agent_state: &AgentOsState,
    context: &hypr_claw_memory::types::ContextData,
    task_list: &[hypr_claw_tasks::TaskInfo],
) {
    let running = task_list
        .iter()
        .filter(|t| t.status == hypr_claw_tasks::TaskStatus::Running)
        .count();
    let completed = task_list
        .iter()
        .filter(|t| t.status == hypr_claw_tasks::TaskStatus::Completed)
        .count();
    let failed = task_list
        .iter()
        .filter(|t| t.status == hypr_claw_tasks::TaskStatus::Failed)
        .count();
    let supervisor_queued = agent_state
        .supervisor
        .tasks
        .iter()
        .filter(|t| t.status == SupervisedTaskStatus::Queued)
        .count();
    let supervisor_running = agent_state
        .supervisor
        .tasks
        .iter()
        .filter(|t| t.status == SupervisedTaskStatus::Running)
        .count();

    println!("\n{}", ui_title("Runtime Status"));
    println!("  Session      : {}", ui_dim(session_key));
    println!(
        "  Agent/User   : {} / {}",
        ui_info(agent_name),
        ui_info(display_name)
    );
    println!(
        "  Mode         : {}",
        ui_info("power")
    );
    println!(
        "  Scan Depth   : {}",
        if agent_state.onboarding.deep_scan_completed {
            ui_success("deep")
        } else {
            ui_dim("standard")
        }
    );
    println!("  Model        : {}", ui_info(model));
    println!("  Thread       : {}", ui_dim(active_thread_id));
    println!(
        "  Threads      : {} active / {} total",
        agent_state
            .task_threads
            .iter()
            .filter(|t| !t.archived)
            .count(),
        agent_state.task_threads.len()
    );
    println!(
        "  Tasks        : {} running / {} completed / {} failed",
        ui_info(&running.to_string()),
        ui_success(&completed.to_string()),
        if failed > 0 {
            ui_danger(&failed.to_string())
        } else {
            ui_success(&failed.to_string())
        }
    );
    println!(
        "  Supervisor   : {} queued / {} running",
        supervisor_queued,
        supervisor_running
    );
    println!(
        "  Memory       : history={} facts={} approvals={} pending={}",
        context.recent_history.len(),
        context.facts.len(),
        context.approval_history.len(),
        context.pending_approvals.len()
    );
    println!(
        "  Tokens       : input={} output={} session={}",
        context.token_usage.total_input,
        context.token_usage.total_output,
        context.token_usage.by_session
    );
    println!("  Reliability  : {}", reliability_summary(agent_state));
    if let Some(plan) = &context.current_plan {
        println!(
            "  Plan         : {} [{} step {}/{}]",
            truncate_for_table(&plan.goal, 36),
            plan.status,
            plan.current_step.saturating_add(1),
            plan.steps.len()
        );
    } else {
        println!("  Plan         : none");
    }
}

fn model_priority(model_id: &str) -> usize {
    match model_id {
        "z-ai/glm4.7" => 0,
        "z-ai/glm5" => 1,
        "moonshotai/kimi-k2.5" => 2,
        "moonshotai/kimi-k2-instruct-0905" => 3,
        "qwen/qwen3-coder-480b-a35b-instruct" => 4,
        "meta/llama-4-maverick-17b-128e-instruct" => 5,
        _ => 100,
    }
}

fn filter_agentic_models(models: &[String]) -> Vec<String> {
    let blocked_tokens = [
        "embed",
        "embedding",
        "guard",
        "safety",
        "reward",
        "retriever",
        "parse",
        "clip",
        "deplot",
        "kosmos",
        "paligemma",
        "vila",
        "streampetr",
    ];

    let mut filtered: Vec<String> = models
        .iter()
        .filter(|id| {
            let lower = id.to_lowercase();
            !blocked_tokens.iter().any(|token| lower.contains(token))
        })
        .cloned()
        .collect();

    filtered.sort_by(|a, b| {
        model_priority(a)
            .cmp(&model_priority(b))
            .then_with(|| a.cmp(b))
    });
    filtered
}

fn print_model_recommendations(provider: &LLMProvider, current_model: &str) {
    match provider {
        LLMProvider::Nvidia => {
            println!("\nRecommended NVIDIA models for agentic tasks:");
            let recommendations = [
                "z-ai/glm4.7",
                "z-ai/glm5",
                "moonshotai/kimi-k2.5",
                "qwen/qwen3-coder-480b-a35b-instruct",
                "meta/llama-4-maverick-17b-128e-instruct",
            ];
            for model in recommendations {
                let marker = if model == current_model { "*" } else { " " };
                println!("  {} {}", marker, model);
            }
            println!("Use '/models set <model_id>' to switch.");
        }
        _ => {
            println!("Model recommendations are currently tuned for NVIDIA provider.");
        }
    }
}

async fn apply_model_switch<S, L, D, R, Sum>(
    model_id: &str,
    agent_loop: &hypr_claw_runtime::AgentLoop<S, L, D, R, Sum>,
    config: &mut Config,
    context_manager: &hypr_claw_memory::ContextManager,
    context: &mut hypr_claw_memory::types::ContextData,
) -> Result<(), String>
where
    S: hypr_claw_runtime::SessionStore,
    L: hypr_claw_runtime::LockManager,
    D: hypr_claw_runtime::ToolDispatcher,
    R: hypr_claw_runtime::ToolRegistry,
    Sum: hypr_claw_runtime::Summarizer,
{
    agent_loop.set_model(model_id).map_err(|e| e.to_string())?;
    config.model = model_id.to_string();
    config.save().map_err(|e| e.to_string())?;

    if !context.system_state.is_object() {
        context.system_state = json!({});
    }
    if let Some(obj) = context.system_state.as_object_mut() {
        obj.insert("active_model".to_string(), json!(model_id));
    }
    context_manager
        .save(context)
        .await
        .map_err(|e| e.to_string())?;

    println!("✅ Model switched to {}", model_id);
    if model_id == "z-ai/glm4.7" {
        println!("ℹ️  GLM-4.7 profile enabled (agentic terminal tuning).");
    }
    Ok(())
}

async fn run_with_interrupt_and_timeout<S, L, D, R, Sum>(
    agent_loop: &hypr_claw_runtime::AgentLoop<S, L, D, R, Sum>,
    session_key: &str,
    agent_name: &str,
    system_prompt: &str,
    prompt: &str,
    interrupt: &Arc<tokio::sync::Notify>,
    timeout: Duration,
) -> Result<String, hypr_claw_runtime::RuntimeError>
where
    S: hypr_claw_runtime::SessionStore,
    L: hypr_claw_runtime::LockManager,
    D: hypr_claw_runtime::ToolDispatcher,
    R: hypr_claw_runtime::ToolRegistry,
    Sum: hypr_claw_runtime::Summarizer,
{
    tokio::select! {
        res = agent_loop.run(session_key, agent_name, system_prompt, prompt) => res,
        _ = interrupt.notified() => Err(hypr_claw_runtime::RuntimeError::LLMError(
            "Interrupted by user".to_string()
        )),
        _ = tokio::time::sleep(timeout) => Err(hypr_claw_runtime::RuntimeError::LLMError(
            format!("Execution watchdog timeout after {}s", timeout.as_secs())
        )),
    }
}

fn build_llm_client_for_provider(
    provider: &LLMProvider,
    model: &str,
) -> Result<hypr_claw_runtime::LLMClientType, String> {
    match provider {
        LLMProvider::Nvidia => {
            let api_key = bootstrap::get_nvidia_api_key().map_err(|e| e.to_string())?;
            Ok(hypr_claw_runtime::LLMClientType::Standard(
                hypr_claw_runtime::LLMClient::with_api_key_and_model(
                    provider.base_url(),
                    1,
                    api_key,
                    model.to_string(),
                ),
            ))
        }
        LLMProvider::Google => {
            let api_key = bootstrap::get_google_api_key().map_err(|e| e.to_string())?;
            Ok(hypr_claw_runtime::LLMClientType::Standard(
                hypr_claw_runtime::LLMClient::with_api_key_and_model(
                    provider.base_url(),
                    1,
                    api_key,
                    model.to_string(),
                ),
            ))
        }
        LLMProvider::Local { .. } => Ok(hypr_claw_runtime::LLMClientType::Standard(
            hypr_claw_runtime::LLMClient::new(provider.base_url(), 1),
        )),
        LLMProvider::Codex | LLMProvider::Antigravity | LLMProvider::GeminiCli => {
            Err("Provider does not support agent-mode tool calling".to_string())
        }
    }
}

fn to_context_tasks(
    task_list: Vec<hypr_claw_tasks::TaskInfo>,
) -> Vec<hypr_claw_memory::types::TaskState> {
    task_list
        .into_iter()
        .map(|task| hypr_claw_memory::types::TaskState {
            id: task.id,
            description: task.description,
            status: format!("{:?}", task.status).to_lowercase(),
            progress: task.progress,
            created_at: task.created_at,
            updated_at: task.updated_at,
        })
        .collect()
}

fn plan_for_input(goal: &str) -> hypr_claw_memory::types::PlanState {
    hypr_claw_memory::types::PlanState {
        goal: goal.to_string(),
        steps: vec![
            hypr_claw_memory::types::PlanStepState {
                id: 0,
                description: "Analyze request".to_string(),
                status: "completed".to_string(),
                result: Some("Intent parsed".to_string()),
            },
            hypr_claw_memory::types::PlanStepState {
                id: 1,
                description: "Execute required tools".to_string(),
                status: "in_progress".to_string(),
                result: None,
            },
            hypr_claw_memory::types::PlanStepState {
                id: 2,
                description: "Report completion".to_string(),
                status: "pending".to_string(),
                result: None,
            },
        ],
        current_step: 1,
        status: "in_progress".to_string(),
        updated_at: chrono::Utc::now().timestamp(),
    }
}

fn mark_plan_completed(context: &mut hypr_claw_memory::types::ContextData, response: &str) {
    if let Some(plan) = &mut context.current_plan {
        if let Some(step) = plan.steps.get_mut(1) {
            step.status = "completed".to_string();
            step.result = Some("Tool execution completed".to_string());
        }
        if let Some(step) = plan.steps.get_mut(2) {
            step.status = "completed".to_string();
            step.result = Some(response.to_string());
        }
        plan.current_step = plan.steps.len();
        plan.status = "completed".to_string();
        plan.updated_at = chrono::Utc::now().timestamp();
    }
}

fn mark_plan_failed(context: &mut hypr_claw_memory::types::ContextData, error: &str) {
    if let Some(plan) = &mut context.current_plan {
        let step_index = plan.current_step.min(plan.steps.len().saturating_sub(1));
        if let Some(step) = plan.steps.get_mut(step_index) {
            step.status = "failed".to_string();
            step.result = Some(error.to_string());
        }
        plan.status = "failed".to_string();
        plan.updated_at = chrono::Utc::now().timestamp();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum SupervisedTaskClass {
    Question,
    Action,
    Investigation,
}

impl SupervisedTaskClass {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Question => "question",
            Self::Action => "action",
            Self::Investigation => "investigation",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum SupervisedTaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SupervisedTask {
    id: String,
    prompt: String,
    class: SupervisedTaskClass,
    #[serde(default)]
    resources: Vec<String>,
    #[serde(default)]
    background_task_id: Option<String>,
    status: SupervisedTaskStatus,
    created_at: i64,
    updated_at: i64,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SupervisorState {
    #[serde(default = "default_supervisor_auto_run")]
    auto_run: bool,
    #[serde(default = "default_supervisor_next_id")]
    next_id: u64,
    #[serde(default)]
    tasks: Vec<SupervisedTask>,
}

fn default_supervisor_auto_run() -> bool {
    false
}

fn default_supervisor_next_id() -> u64 {
    1
}

impl Default for SupervisorState {
    fn default() -> Self {
        Self {
            auto_run: default_supervisor_auto_run(),
            next_id: default_supervisor_next_id(),
            tasks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentOsState {
    #[serde(default)]
    onboarding: OnboardingState,
    #[serde(default)]
    soul_auto: bool,
    #[serde(default)]
    task_threads: Vec<TaskThread>,
    #[serde(default)]
    active_thread_id: String,
    #[serde(default)]
    supervisor: SupervisorState,
    #[serde(default)]
    reliability: ReliabilityState,
    #[serde(default = "default_autonomy_mode")]
    autonomy_mode: AutonomyMode,
    #[serde(default)]
    autonomy_calibration: AutonomyCalibrationState,
}

impl Default for AgentOsState {
    fn default() -> Self {
        Self {
            onboarding: OnboardingState::default(),
            soul_auto: false,
            task_threads: vec![TaskThread::new("task-1".to_string(), "Main".to_string())],
            active_thread_id: "task-1".to_string(),
            supervisor: SupervisorState::default(),
            reliability: ReliabilityState::default(),
            autonomy_mode: default_autonomy_mode(),
            autonomy_calibration: AutonomyCalibrationState::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReliabilityState {
    #[serde(default)]
    run_id: u64,
    #[serde(default)]
    last_stage: String,
    #[serde(default)]
    fallback_attempts: u32,
    #[serde(default)]
    last_duration_ms: u64,
    #[serde(default)]
    last_error: String,
    #[serde(default)]
    last_break_reason: String,
    #[serde(default)]
    updated_at: Option<i64>,
}

impl Default for ReliabilityState {
    fn default() -> Self {
        Self {
            run_id: 0,
            last_stage: "idle".to_string(),
            fallback_attempts: 0,
            last_duration_ms: 0,
            last_error: String::new(),
            last_break_reason: String::new(),
            updated_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AutonomyMode {
    PromptFirst,
    Guarded,
}

impl AutonomyMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::PromptFirst => "prompt_first",
            Self::Guarded => "guarded",
        }
    }
}

fn default_autonomy_mode() -> AutonomyMode {
    AutonomyMode::PromptFirst
}

fn strict_workflow_enabled() -> bool {
    std::env::var("HYPR_CLAW_STRICT_WORKFLOW")
        .ok()
        .map(|raw| {
            !matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "off"
            )
        })
        .unwrap_or(true)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AutonomyModeMetrics {
    #[serde(default)]
    runs: u64,
    #[serde(default)]
    successes: u64,
    #[serde(default)]
    failures: u64,
    #[serde(default)]
    total_duration_ms: u64,
    #[serde(default)]
    total_fallback_attempts: u64,
    #[serde(default)]
    stop_codes: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AutonomyCalibrationState {
    #[serde(default)]
    prompt_first: AutonomyModeMetrics,
    #[serde(default)]
    guarded: AutonomyModeMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OnboardingState {
    #[serde(default)]
    completed: bool,
    #[serde(default)]
    preferred_name: String,
    #[serde(default)]
    profile_confirmed: bool,
    #[serde(default = "empty_json")]
    system_profile: Value,
    #[serde(default)]
    last_scan_at: Option<i64>,
    #[serde(default)]
    deep_scan_completed: bool,
    #[serde(default)]
    trusted_full_auto: bool,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            completed: false,
            preferred_name: String::new(),
            profile_confirmed: false,
            system_profile: json!({}),
            last_scan_at: None,
            deep_scan_completed: false,
            trusted_full_auto: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskThread {
    id: String,
    title: String,
    archived: bool,
    created_at: i64,
    updated_at: i64,
}

impl TaskThread {
    fn new(id: String, title: String) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id,
            title,
            archived: false,
            created_at: now,
            updated_at: now,
        }
    }
}

fn empty_json() -> Value {
    json!({})
}

fn load_agent_os_state(context: &hypr_claw_memory::types::ContextData) -> AgentOsState {
    let maybe_state = context.system_state.get("agent_os_state").cloned();
    let mut state = maybe_state
        .as_ref()
        .and_then(|v| serde_json::from_value::<AgentOsState>(v.clone()).ok())
        .unwrap_or_default();

    // Migration: if legacy state did not include soul_auto, default to disabled.
    let soul_auto_present = maybe_state
        .as_ref()
        .and_then(|v| v.get("soul_auto"))
        .is_some();
    if !soul_auto_present {
        state.soul_auto = false;
    }

    state
}

fn persist_agent_os_state(
    context: &mut hypr_claw_memory::types::ContextData,
    state: &AgentOsState,
) {
    if !context.system_state.is_object() {
        context.system_state = json!({});
    }
    if let Some(obj) = context.system_state.as_object_mut() {
        let value = serde_json::to_value(state).unwrap_or_else(|_| json!({}));
        obj.insert("agent_os_state".to_string(), value);
    }
}

fn ensure_default_thread(state: &mut AgentOsState) {
    if state.task_threads.is_empty() {
        state
            .task_threads
            .push(TaskThread::new("task-1".to_string(), "Main".to_string()));
    }
    let active_exists = state
        .task_threads
        .iter()
        .any(|thread| thread.id == state.active_thread_id && !thread.archived);
    if !active_exists {
        if let Some(thread) = state.task_threads.iter().find(|thread| !thread.archived) {
            state.active_thread_id = thread.id.clone();
        } else {
            let fallback = TaskThread::new(next_thread_id(state), "Main".to_string());
            state.active_thread_id = fallback.id.clone();
            state.task_threads.push(fallback);
        }
    }
}

fn display_name(state: &AgentOsState, fallback_user_id: &str) -> String {
    if state.onboarding.preferred_name.trim().is_empty() {
        fallback_user_id.to_string()
    } else {
        state.onboarding.preferred_name.clone()
    }
}

fn detect_agent_name() -> String {
    if let Ok(from_env) = std::env::var("HYPR_CLAW_AGENT") {
        let candidate = from_env.trim();
        if !candidate.is_empty() {
            let path = format!("./data/agents/{}.yaml", candidate);
            if std::path::Path::new(&path).exists() {
                return candidate.to_string();
            }
        }
    }

    let default_path = "./data/agents/default.yaml";
    if std::path::Path::new(default_path).exists() {
        return "default".to_string();
    }

    if let Ok(entries) = std::fs::read_dir("./data/agents") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("yaml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    return stem.to_string();
                }
            }
        }
    }

    "default".to_string()
}

fn detect_user_id() -> String {
    std::env::var("HYPR_CLAW_USER")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("USER").ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "local_user".to_string())
}

fn prompt_line(prompt: &str) -> io::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn prompt_yes_no(prompt: &str, default_yes: bool) -> io::Result<bool> {
    let input = prompt_line(prompt)?;
    if input.is_empty() {
        return Ok(default_yes);
    }
    match input.to_lowercase().as_str() {
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        _ => Ok(default_yes),
    }
}

async fn run_first_run_onboarding(
    user_id: &str,
    context: &mut hypr_claw_memory::types::ContextData,
    state: &mut AgentOsState,
) -> Result<(), Box<dyn std::error::Error>> {
    if state.onboarding.completed {
        return Ok(());
    }

    println!("\n🧭 First Run Onboarding");
    if state.onboarding.preferred_name.trim().is_empty() {
        let preferred = prompt_line("What should I call you? ")?;
        state.onboarding.preferred_name = if preferred.trim().is_empty() {
            user_id.to_string()
        } else {
            preferred
        };
    }

    state.onboarding.trusted_full_auto = false;

    if prompt_yes_no("Allow first-time system study scan? [Y/n] ", true)? {
        let deep_scan = prompt_yes_no(
            "Run deep system learning scan (home directory with consent)? [Y/n] ",
            true,
        )?;

        state.onboarding.system_profile = scan::run_integrated_scan(user_id, deep_scan).await?;
        state.onboarding.deep_scan_completed = deep_scan;
        state.onboarding.last_scan_at = Some(chrono::Utc::now().timestamp());
        print_system_profile_summary(&state.onboarding.system_profile);

        loop {
            if prompt_yes_no("Is this system profile correct? [Y/n] ", true)? {
                state.onboarding.profile_confirmed = true;
                break;
            }
            if !prompt_yes_no("Edit profile now? [Y/n] ", true)? {
                break;
            }
            edit_profile_interactively(&mut state.onboarding.system_profile)?;
            print_system_profile_summary(&state.onboarding.system_profile);
        }
    } else {
        println!("ℹ️  System study skipped for now. Use `scan` later.");
    }

    state.onboarding.completed = true;
    if !context
        .facts
        .iter()
        .any(|f| f.starts_with("preferred_name:"))
    {
        context.facts.push(format!(
            "preferred_name:{}",
            state.onboarding.preferred_name
        ));
    }
    Ok(())
}

fn print_system_profile_summary(profile: &Value) {
    let distro = profile
        .pointer("/platform/distro_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let kernel = profile
        .pointer("/platform/kernel")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let hypr = profile
        .pointer("/desktop/hyprland_available")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let active_ws = profile
        .pointer("/desktop/active_workspace")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    println!("\n🖥️  Profile Summary");
    println!("  Distro: {}", distro);
    println!("  Kernel: {}", kernel);
    println!("  Hyprland available: {}", if hypr { "yes" } else { "no" });
    if hypr {
        println!("  Active workspace: {}", active_ws);
    }
    println!(
        "  Deep scan data: {}",
        if profile.pointer("/deep_scan").is_some() {
            "yes"
        } else {
            "no"
        }
    );
    println!();
}

fn edit_profile_interactively(profile: &mut Value) -> io::Result<()> {
    println!("Profile edit mode.");
    println!("Commands: show | set <path> <value> | done | accept | help");
    println!("Path example: desktop.active_workspace");
    loop {
        let line = prompt_line("profile> ")?;
        if line.is_empty() {
            continue;
        }
        if line == "done" || line == "accept" {
            break;
        }
        if line == "help" {
            println!("Commands: show | set <path> <value> | done | accept");
            continue;
        }
        if line == "show" {
            println!(
                "{}",
                serde_json::to_string_pretty(profile).unwrap_or_else(|_| "{}".to_string())
            );
            continue;
        }
        if let Some(rest) = line.strip_prefix("set ") {
            let mut parts = rest.splitn(2, ' ');
            let path = parts.next().unwrap_or_default();
            let raw_value = parts.next().unwrap_or_default().trim();
            if path.is_empty() || raw_value.is_empty() {
                println!("Usage: set <path> <value>");
                continue;
            }
            let parsed = serde_json::from_str::<Value>(raw_value)
                .unwrap_or_else(|_| Value::String(raw_value.to_string()));
            set_json_path(profile, path, parsed);
            println!("Updated {}", path);
            continue;
        }
        println!("Unknown command. Use show | set <path> <value> | done");
    }
    Ok(())
}

fn set_json_path(root: &mut Value, path: &str, value: Value) {
    let segments: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        *root = value;
        return;
    }

    let mut current = root;
    let mut pending = Some(value);
    for (index, segment) in segments.iter().enumerate() {
        let is_last = index == segments.len() - 1;
        if !current.is_object() {
            *current = json!({});
        }
        let map = current.as_object_mut().expect("object was just created");
        if is_last {
            map.insert(
                (*segment).to_string(),
                pending.take().unwrap_or(Value::Null),
            );
            return;
        }
        current = map
            .entry((*segment).to_string())
            .or_insert_with(|| json!({}));
    }
}

fn profile_needs_capability_refresh(profile: &Value) -> bool {
    profile.pointer("/capabilities").is_none() || profile.pointer("/paths/downloads").is_none()
}

fn sanitize_user_key_for_filename(user_id: &str) -> String {
    let mut out = String::new();
    for ch in user_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "local_user".to_string()
    } else {
        out
    }
}

fn capability_registry_file_path(user_id: &str) -> String {
    format!(
        "./data/capabilities/{}.json",
        sanitize_user_key_for_filename(user_id)
    )
}

fn read_string_array_from_value(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_string)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default()
}

fn build_capability_registry(profile: &Value) -> Value {
    let profile_scanned_at = profile
        .pointer("/scanned_at")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let has_deep_scan = profile.pointer("/deep_scan").is_some();

    let wallpaper_backends =
        read_string_array_from_value(profile.pointer("/capabilities/wallpaper_backends"));
    let screenshot_backends =
        read_string_array_from_value(profile.pointer("/capabilities/screenshot_backends"));
    let input_backends =
        read_string_array_from_value(profile.pointer("/capabilities/input_backends"));
    let launcher_commands = read_string_array_from_value(
        profile.pointer("/deep_scan/desktop_apps/launcher_commands_sample"),
    );
    let known_projects =
        read_string_array_from_value(profile.pointer("/deep_scan/home_inventory/project_roots"));

    let available_commands = profile
        .pointer("/commands")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(name, enabled)| {
                    enabled.as_bool().unwrap_or(false).then_some(name.clone())
                })
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    let vscode_command = ["code", "codium", "code-oss", "vscodium", "code-insiders"]
        .iter()
        .find(|name| available_commands.iter().any(|cmd| cmd == **name))
        .map(|s| (*s).to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let preferred_launchers =
        build_preferred_launchers(&launcher_commands, &available_commands, &vscode_command);

    json!({
        "schema_version": 1,
        "generated_at": chrono::Utc::now().timestamp(),
        "source_profile_scanned_at": profile_scanned_at,
        "source_has_deep_scan": has_deep_scan,
        "platform": {
            "distro_name": profile.pointer("/platform/distro_name").and_then(|v| v.as_str()).unwrap_or("unknown"),
            "kernel": profile.pointer("/platform/kernel").and_then(|v| v.as_str()).unwrap_or("unknown"),
            "arch": profile.pointer("/platform/arch").and_then(|v| v.as_str()).unwrap_or("unknown"),
            "hyprland_available": profile.pointer("/desktop/hyprland_available").and_then(|v| v.as_bool()).unwrap_or(false),
            "active_workspace": profile.pointer("/desktop/active_workspace").and_then(|v| v.as_u64()).unwrap_or(0),
            "workspace_count": profile.pointer("/desktop/workspace_count").and_then(|v| v.as_u64()).unwrap_or(0)
        },
        "paths": {
            "home": profile.pointer("/paths/home").and_then(|v| v.as_str()).unwrap_or(""),
            "downloads": profile.pointer("/paths/downloads").and_then(|v| v.as_str()).unwrap_or(""),
            "pictures": profile.pointer("/paths/pictures").and_then(|v| v.as_str()).unwrap_or(""),
            "documents": profile.pointer("/paths/documents").and_then(|v| v.as_str()).unwrap_or("")
        },
        "capabilities": {
            "wallpaper_backends": wallpaper_backends,
            "screenshot_backends": screenshot_backends,
            "input_backends": input_backends,
            "ocr_available": profile.pointer("/capabilities/ocr_available").and_then(|v| v.as_bool()).unwrap_or(false)
        },
        "editor": {
            "vscode_command": vscode_command
        },
        "desktop_apps": {
            "launcher_commands": launcher_commands,
            "preferred_launchers": preferred_launchers
        },
        "projects": {
            "known_roots": known_projects
        },
        "hyprland": {
            "main_config": profile.pointer("/deep_scan/hyprland/main_config").and_then(|v| v.as_str()).unwrap_or(""),
            "binds_sample": profile.pointer("/deep_scan/hyprland/binds_sample").cloned().unwrap_or_else(|| json!([])),
            "exec_once_sample": profile.pointer("/deep_scan/hyprland/exec_once_sample").cloned().unwrap_or_else(|| json!([])),
            "workspace_rules_sample": profile.pointer("/deep_scan/hyprland/workspace_rules_sample").cloned().unwrap_or_else(|| json!([]))
        },
        "packages": {
            "total_count": profile.pointer("/deep_scan/packages/total_count").and_then(|v| v.as_u64()).unwrap_or(0),
            "pacman_explicit_count": profile.pointer("/deep_scan/packages/pacman_explicit_count").and_then(|v| v.as_u64()).unwrap_or(0),
            "aur_count": profile.pointer("/deep_scan/packages/aur_count").and_then(|v| v.as_u64()).unwrap_or(0)
        },
        "commands": {
            "available": available_commands
        },
        "usage": {
            "top_commands": profile.pointer("/deep_scan/usage/shell_history_top").cloned().unwrap_or_else(|| json!([]))
        }
    })
}

fn build_preferred_launchers(
    launcher_commands: &[String],
    available_commands: &[String],
    vscode_command: &str,
) -> Value {
    let mut known = std::collections::BTreeSet::new();
    for command in launcher_commands {
        known.insert(command.clone());
    }
    for command in available_commands {
        known.insert(command.clone());
    }

    let pick = |candidates: &[&str]| -> String {
        candidates
            .iter()
            .find(|candidate| known.contains(**candidate))
            .map(|candidate| (*candidate).to_string())
            .unwrap_or_else(|| "unknown".to_string())
    };

    let vscode = if vscode_command == "unknown" {
        pick(&["code", "code-insiders", "code-oss", "codium", "vscodium"])
    } else {
        vscode_command.to_string()
    };

    json!({
        "vscode": vscode,
        "browser": pick(&["firefox", "google-chrome-stable", "google-chrome", "chromium", "brave-browser"]),
        "telegram": pick(&["telegram-desktop", "telegram", "org.telegram.desktop"]),
        "terminal": pick(&["kitty", "alacritty", "wezterm", "gnome-terminal", "xterm"])
    })
}

fn capability_registry_needs_refresh(registry: &Value, profile: &Value) -> bool {
    if registry.pointer("/schema_version").is_none() {
        return true;
    }
    if registry.pointer("/paths/downloads").is_none() {
        return true;
    }
    if registry
        .pointer("/capabilities/wallpaper_backends")
        .is_none()
    {
        return true;
    }
    if registry
        .pointer("/desktop_apps/preferred_launchers")
        .is_none()
    {
        return true;
    }

    let profile_scanned_at = profile
        .pointer("/scanned_at")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let registry_profile_scanned_at = registry
        .pointer("/source_profile_scanned_at")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    if profile_scanned_at > registry_profile_scanned_at {
        return true;
    }

    let profile_has_deep = profile.pointer("/deep_scan").is_some();
    let registry_has_deep = registry
        .pointer("/source_has_deep_scan")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    profile_has_deep && !registry_has_deep
}

fn load_capability_registry(user_id: &str) -> io::Result<Value> {
    let path = capability_registry_file_path(user_id);
    let raw = std::fs::read_to_string(path)?;
    serde_json::from_str::<Value>(&raw)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

fn save_capability_registry(user_id: &str, registry: &Value) -> io::Result<()> {
    std::fs::create_dir_all("./data/capabilities")?;
    let path = capability_registry_file_path(user_id);
    let payload = serde_json::to_string_pretty(registry)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    std::fs::write(path, payload)
}

/// Appends a timestamped capability delta record to the user's delta history (JSONL).
fn append_capability_delta_history(
    user_id: &str,
    old_registry: &Value,
    new_registry: &Value,
) -> io::Result<()> {
    std::fs::create_dir_all("./data/capabilities")?;
    let path = format!(
        "./data/capabilities/{}.deltas.jsonl",
        sanitize_user_key_for_filename(user_id)
    );
    let diff_lines = capability_registry_diff_lines(old_registry, new_registry);
    let summary = if diff_lines.is_empty() {
        "no changes".to_string()
    } else {
        format!("{} change(s)", diff_lines.len())
    };
    let at = chrono::Utc::now().timestamp();
    let record = json!({
        "at": at,
        "summary": summary,
        "diff_lines": diff_lines,
    });
    let line = serde_json::to_string(&record)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let mut content = line;
    content.push('\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(content.as_bytes())
}

/// Reads the last N lines from the user's capability deltas JSONL file (newest last in file).
fn read_capability_delta_history(user_id: &str, limit: usize) -> Vec<Value> {
    let path = format!(
        "./data/capabilities/{}.deltas.jsonl",
        sanitize_user_key_for_filename(user_id)
    );
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let lines: Vec<&str> = content.lines().filter(|s| !s.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(limit);
    lines[start..]
        .iter()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn print_capability_delta_history(user_id: &str, limit: usize) {
    let records = read_capability_delta_history(user_id, limit);
    println!("\n{}", ui_title("Capability delta history"));
    if records.is_empty() {
        println!("  No delta history yet. Run `scan` and apply changes to record deltas.");
        return;
    }
    for (i, rec) in records.iter().enumerate() {
        let at = rec.get("at").and_then(|v| v.as_i64()).unwrap_or(0);
        let summary = rec.get("summary").and_then(|v| v.as_str()).unwrap_or("?");
        println!("  {}  {}  {}", i + 1, format_timestamp(at), summary);
        if let Some(arr) = rec.get("diff_lines").and_then(|v| v.as_array()) {
            for line in arr.iter().take(5) {
                if let Some(s) = line.as_str() {
                    println!("      - {}", truncate_for_table(s, 72));
                }
            }
            if arr.len() > 5 {
                println!("      ... and {} more", arr.len() - 5);
            }
        }
    }
    println!();
}

fn print_capability_registry_summary(user_id: &str, registry: &Value) {
    let generated = registry
        .pointer("/generated_at")
        .and_then(|v| v.as_i64())
        .map(format_timestamp)
        .unwrap_or_else(|| "n/a".to_string());
    let distro = registry
        .pointer("/platform/distro_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let kernel = registry
        .pointer("/platform/kernel")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let workspace = registry
        .pointer("/platform/active_workspace")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let downloads = registry
        .pointer("/paths/downloads")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let wallpaper =
        read_string_array_from_value(registry.pointer("/capabilities/wallpaper_backends"));
    let screenshot =
        read_string_array_from_value(registry.pointer("/capabilities/screenshot_backends"));
    let input = read_string_array_from_value(registry.pointer("/capabilities/input_backends"));
    let launchers =
        read_string_array_from_value(registry.pointer("/desktop_apps/launcher_commands"));
    let preferred_launchers = registry
        .pointer("/desktop_apps/preferred_launchers")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|value| format!("{k}={value}")))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    let registry_path = capability_registry_file_path(user_id);

    println!("\n{}", ui_title("Capability Registry"));
    println!("  file         : {}", ui_dim(&registry_path));
    println!("  generated_at : {}", generated);
    println!(
        "  platform     : {} | kernel {} | workspace {}",
        distro, kernel, workspace
    );
    println!("  downloads    : {}", downloads);
    println!(
        "  backends     : wallpaper=[{}] screenshot=[{}] input=[{}]",
        wallpaper.join(", "),
        screenshot.join(", "),
        input.join(", ")
    );
    println!(
        "  launcher cmds: {}",
        truncate_for_table(&launchers.join(", "), 92)
    );
    println!(
        "  preferred    : {}",
        truncate_for_table(&preferred_launchers.join(", "), 92)
    );
    println!();
}

fn capability_registry_diff_lines(old_registry: &Value, new_registry: &Value) -> Vec<String> {
    let old_distro = old_registry
        .pointer("/platform/distro_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let new_distro = new_registry
        .pointer("/platform/distro_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let old_kernel = old_registry
        .pointer("/platform/kernel")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let new_kernel = new_registry
        .pointer("/platform/kernel")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let old_workspace = old_registry
        .pointer("/platform/active_workspace")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let new_workspace = new_registry
        .pointer("/platform/active_workspace")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let old_wallpaper =
        read_string_array_from_value(old_registry.pointer("/capabilities/wallpaper_backends"));
    let new_wallpaper =
        read_string_array_from_value(new_registry.pointer("/capabilities/wallpaper_backends"));
    let old_screenshot =
        read_string_array_from_value(old_registry.pointer("/capabilities/screenshot_backends"));
    let new_screenshot =
        read_string_array_from_value(new_registry.pointer("/capabilities/screenshot_backends"));
    let old_input =
        read_string_array_from_value(old_registry.pointer("/capabilities/input_backends"));
    let new_input =
        read_string_array_from_value(new_registry.pointer("/capabilities/input_backends"));

    let old_launchers =
        read_string_array_from_value(old_registry.pointer("/desktop_apps/launcher_commands"));
    let new_launchers =
        read_string_array_from_value(new_registry.pointer("/desktop_apps/launcher_commands"));
    let old_projects = read_string_array_from_value(old_registry.pointer("/projects/known_roots"));
    let new_projects = read_string_array_from_value(new_registry.pointer("/projects/known_roots"));
    let old_vscode = old_registry
        .pointer("/editor/vscode_command")
        .and_then(|v| v.as_str())
        .unwrap_or("none");
    let new_vscode = new_registry
        .pointer("/editor/vscode_command")
        .and_then(|v| v.as_str())
        .unwrap_or("none");

    let mut changes = Vec::<String>::new();
    if old_distro != new_distro {
        changes.push(format!(
            "platform distro: '{}' -> '{}'",
            old_distro, new_distro
        ));
    }
    if old_kernel != new_kernel {
        changes.push(format!(
            "platform kernel: '{}' -> '{}'",
            old_kernel, new_kernel
        ));
    }
    if old_workspace != new_workspace {
        changes.push(format!(
            "active workspace: {} -> {}",
            old_workspace, new_workspace
        ));
    }
    if old_wallpaper != new_wallpaper {
        changes.push(format!(
            "wallpaper backends: [{}] -> [{}]",
            old_wallpaper.join(", "),
            new_wallpaper.join(", ")
        ));
    }
    if old_screenshot != new_screenshot {
        changes.push(format!(
            "screenshot backends: [{}] -> [{}]",
            old_screenshot.join(", "),
            new_screenshot.join(", ")
        ));
    }
    if old_input != new_input {
        changes.push(format!(
            "input backends: [{}] -> [{}]",
            old_input.join(", "),
            new_input.join(", ")
        ));
    }
    if old_launchers != new_launchers {
        changes.push(format!(
            "launcher commands changed ({} -> {} entries)",
            old_launchers.len(),
            new_launchers.len()
        ));
    }
    if old_projects != new_projects {
        changes.push(format!(
            "known project roots changed ({} -> {} entries)",
            old_projects.len(),
            new_projects.len()
        ));
    }
    if old_vscode != new_vscode {
        changes.push(format!(
            "vscode command: '{}' -> '{}'",
            old_vscode, new_vscode
        ));
    }

    changes
}

fn print_capability_registry_diff_summary(old_registry: &Value, new_registry: &Value) {
    let changes = capability_registry_diff_lines(old_registry, new_registry);
    println!("\n{}", ui_title("Scan Diff Summary"));
    if changes.is_empty() {
        println!("  no major capability changes detected");
    } else {
        for change in changes {
            println!("  - {}", truncate_for_table(&change, 104));
        }
    }
    println!();
}

fn next_thread_id(state: &AgentOsState) -> String {
    let max_id = state
        .task_threads
        .iter()
        .filter_map(|thread| thread.id.strip_prefix("task-"))
        .filter_map(|suffix| suffix.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("task-{}", max_id + 1)
}

#[derive(Debug, Clone, Copy)]
struct ExecutionBudget {
    max_iterations: usize,
}

fn classify_supervised_task_class(input: &str) -> SupervisedTaskClass {
    let lower = input.to_lowercase();
    let investigation_tokens = [
        "diagnose",
        "why",
        "analyze",
        "investigate",
        "debug",
        "compare",
        "inspect",
        "check",
        "status",
        "trace",
        "benchmark",
    ];
    if investigation_tokens
        .iter()
        .any(|token| lower.contains(token))
    {
        return SupervisedTaskClass::Investigation;
    }

    let action_tokens = [
        "open",
        "create",
        "delete",
        "remove",
        "move",
        "copy",
        "write",
        "edit",
        "run",
        "build",
        "install",
        "switch",
        "focus",
        "close",
        "search",
        "reply",
        "send",
        "play",
        "pause",
        "lockscreen",
        "wallpaper",
        "volume",
    ];
    if action_tokens.iter().any(|token| lower.contains(token)) {
        return SupervisedTaskClass::Action;
    }

    SupervisedTaskClass::Question
}

fn infer_supervised_task_resources(input: &str) -> Vec<String> {
    let lower = input.to_lowercase();
    let mut resources = Vec::<String>::new();

    let add = |name: &str, resources: &mut Vec<String>| {
        if !resources.iter().any(|existing| existing == name) {
            resources.push(name.to_string());
        }
    };

    if [
        "open",
        "launch",
        "switch workspace",
        "workspace",
        "window",
        "click",
        "type",
        "press",
        "key",
        "mouse",
        "screen",
        "ocr",
        "telegram",
        "gmail",
        "mail",
        "browser",
    ]
    .iter()
    .any(|token| lower.contains(token))
    {
        add("desktop_input", &mut resources);
    }

    if [
        "file", "folder", "dir", "read", "write", "create", "delete", "copy", "move", "project",
    ]
    .iter()
    .any(|token| lower.contains(token))
    {
        add("filesystem", &mut resources);
    }

    if [
        "web", "search", "download", "upload", "api", "http", "gmail", "telegram",
    ]
    .iter()
    .any(|token| lower.contains(token))
    {
        add("network", &mut resources);
    }

    if [
        "run",
        "build",
        "compile",
        "install",
        "process",
        "spawn",
        "kill",
        "benchmark",
        "test",
    ]
    .iter()
    .any(|token| lower.contains(token))
    {
        add("compute", &mut resources);
    }

    if resources.is_empty() {
        add("general", &mut resources);
    }

    resources
}

fn can_run_supervisor_in_background(task: &SupervisedTask) -> bool {
    !task.resources.iter().any(|resource| {
        matches!(
            resource.as_str(),
            "desktop_input" | "filesystem" | "general"
        )
    })
}

fn resource_is_shared(resource: &str) -> bool {
    matches!(resource, "network" | "compute")
}

fn resource_conflicts(lhs: &[String], rhs: &[String]) -> Vec<String> {
    let left = lhs
        .iter()
        .map(|r| r.trim().to_lowercase())
        .filter(|r| !r.is_empty())
        .collect::<HashSet<_>>();
    let right = rhs
        .iter()
        .map(|r| r.trim().to_lowercase())
        .filter(|r| !r.is_empty())
        .collect::<HashSet<_>>();

    if left.is_empty() || right.is_empty() {
        return Vec::new();
    }
    if left.contains("general") || right.contains("general") {
        return vec!["general".to_string()];
    }

    let mut conflicts = BTreeSet::new();
    for resource in left.intersection(&right) {
        if !resource_is_shared(resource) {
            conflicts.insert(resource.to_string());
        }
    }
    conflicts.into_iter().collect::<Vec<_>>()
}

fn running_supervisor_conflicts_for_resources(
    required: &[String],
    running: &[(String, Vec<String>)],
) -> Vec<String> {
    let mut details = Vec::new();
    for (task_id, resources) in running {
        let overlap = resource_conflicts(required, resources);
        if !overlap.is_empty() {
            details.push(format!(
                "{} [{}]",
                task_id,
                truncate_for_table(&overlap.join("+"), 28)
            ));
        }
    }
    details
}

fn execution_budget_for_class(class: &SupervisedTaskClass, mode: &AutonomyMode) -> ExecutionBudget {
    match mode {
        AutonomyMode::PromptFirst => match class {
            SupervisedTaskClass::Question => ExecutionBudget { max_iterations: 14 },
            SupervisedTaskClass::Action => ExecutionBudget { max_iterations: 28 },
            SupervisedTaskClass::Investigation => ExecutionBudget { max_iterations: 40 },
        },
        AutonomyMode::Guarded => match class {
            SupervisedTaskClass::Question => ExecutionBudget { max_iterations: 8 },
            SupervisedTaskClass::Action => ExecutionBudget { max_iterations: 16 },
            SupervisedTaskClass::Investigation => ExecutionBudget { max_iterations: 24 },
        },
    }
}

fn watchdog_timeout_for_class(class: &SupervisedTaskClass, mode: &AutonomyMode) -> Duration {
    match mode {
        AutonomyMode::PromptFirst => match class {
            SupervisedTaskClass::Question => Duration::from_secs(60),
            SupervisedTaskClass::Action => Duration::from_secs(120),
            SupervisedTaskClass::Investigation => Duration::from_secs(210),
        },
        AutonomyMode::Guarded => match class {
            SupervisedTaskClass::Question => Duration::from_secs(45),
            SupervisedTaskClass::Action => Duration::from_secs(90),
            SupervisedTaskClass::Investigation => Duration::from_secs(150),
        },
    }
}

fn next_supervised_task_id(state: &mut AgentOsState) -> String {
    let id = format!("sup-{}", state.supervisor.next_id);
    state.supervisor.next_id += 1;
    id
}

fn start_supervised_task(
    state: &mut AgentOsState,
    prompt: String,
    class: SupervisedTaskClass,
) -> String {
    let now = chrono::Utc::now().timestamp();
    let id = next_supervised_task_id(state);
    let resources = infer_supervised_task_resources(&prompt);
    state.supervisor.tasks.push(SupervisedTask {
        id: id.clone(),
        prompt,
        class,
        resources,
        background_task_id: None,
        status: SupervisedTaskStatus::Running,
        created_at: now,
        updated_at: now,
        error: None,
    });
    id
}

fn enqueue_supervised_task(
    state: &mut AgentOsState,
    prompt: String,
    class: SupervisedTaskClass,
) -> String {
    let now = chrono::Utc::now().timestamp();
    let id = next_supervised_task_id(state);
    let resources = infer_supervised_task_resources(&prompt);
    state.supervisor.tasks.push(SupervisedTask {
        id: id.clone(),
        prompt,
        class,
        resources,
        background_task_id: None,
        status: SupervisedTaskStatus::Queued,
        created_at: now,
        updated_at: now,
        error: None,
    });
    id
}

enum QueueStartResult {
    Started(SupervisedTask),
    Blocked(String),
    Empty,
}

fn start_next_queued_supervised_task(state: &mut AgentOsState) -> QueueStartResult {
    let running = state
        .supervisor
        .tasks
        .iter()
        .filter(|task| task.status == SupervisedTaskStatus::Running)
        .map(|task| (task.id.clone(), task.resources.clone()))
        .collect::<Vec<_>>();

    let now = chrono::Utc::now().timestamp();
    let mut blocked_reason: Option<String> = None;
    for task in state.supervisor.tasks.iter_mut() {
        if task.status != SupervisedTaskStatus::Queued {
            continue;
        }
        let conflicts = running_supervisor_conflicts_for_resources(&task.resources, &running);
        if conflicts.is_empty() {
            task.status = SupervisedTaskStatus::Running;
            task.updated_at = now;
            task.background_task_id = None;
            return QueueStartResult::Started(task.clone());
        }
        if blocked_reason.is_none() {
            blocked_reason = Some(format!(
                "{} waiting on {} (conflict resources: {})",
                task.id,
                conflicts.join(", "),
                truncate_for_table(&task.resources.join(","), 28)
            ));
        }
    }
    if let Some(reason) = blocked_reason {
        return QueueStartResult::Blocked(reason);
    }
    QueueStartResult::Empty
}

fn mark_supervised_task_completed(state: &mut AgentOsState, task_id: &str) {
    let now = chrono::Utc::now().timestamp();
    if let Some(task) = state
        .supervisor
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
    {
        task.status = SupervisedTaskStatus::Completed;
        task.updated_at = now;
        task.error = None;
        task.background_task_id = None;
    }
}

fn mark_supervised_task_failed(state: &mut AgentOsState, task_id: &str, error: String) {
    let now = chrono::Utc::now().timestamp();
    if let Some(task) = state
        .supervisor
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
    {
        task.status = SupervisedTaskStatus::Failed;
        task.updated_at = now;
        task.error = Some(error);
        task.background_task_id = None;
    }
}

fn mark_supervised_task_cancelled(state: &mut AgentOsState, task_id: &str, reason: Option<String>) {
    let now = chrono::Utc::now().timestamp();
    if let Some(task) = state
        .supervisor
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
    {
        task.status = SupervisedTaskStatus::Cancelled;
        task.updated_at = now;
        task.error = reason;
        task.background_task_id = None;
    }
}

fn supervised_task_status(state: &AgentOsState, task_id: &str) -> Option<SupervisedTaskStatus> {
    state
        .supervisor
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .map(|task| task.status.clone())
}

fn resolve_supervisor_task<'a>(
    target: &str,
    agent_state: &'a AgentOsState,
) -> Option<&'a SupervisedTask> {
    agent_state
        .supervisor
        .tasks
        .iter()
        .find(|task| task.id == target || task.background_task_id.as_deref() == Some(target))
}

fn print_supervisor_task_inspect(task: &SupervisedTask) {
    println!("Supervisor task: {}", task.id);
    println!("  prompt: {}", truncate_for_table(&task.prompt, 72));
    println!("  class: {}", task.class.as_str());
    println!(
        "  status: {}",
        match &task.status {
            SupervisedTaskStatus::Queued => "queued",
            SupervisedTaskStatus::Running => "running",
            SupervisedTaskStatus::Completed => "completed",
            SupervisedTaskStatus::Failed => "failed",
            SupervisedTaskStatus::Cancelled => "cancelled",
        }
    );
    if !task.resources.is_empty() {
        println!("  resources: {}", task.resources.join(", "));
    }
    if let Some(bg) = &task.background_task_id {
        println!("  background_task_id: {}", bg);
    }
    println!("  created_at: {}", task.created_at);
    println!("  updated_at: {}", task.updated_at);
    if let Some(err) = &task.error {
        println!("  error: {}", err);
    }
}

fn collect_supervisor_task_events(
    task: &SupervisedTask,
    task_event_feed: &Arc<Mutex<Vec<String>>>,
    limit: usize,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Ok(events) = task_event_feed.lock() {
        for line in events.iter() {
            let mention = line.contains(&task.id)
                || task
                    .background_task_id
                    .as_ref()
                    .map_or(false, |bg| line.contains(bg.as_str()));
            if mention {
                out.push(line.clone());
            }
        }
    }
    let len = out.len();
    if len <= limit {
        out
    } else {
        out.into_iter().skip(len - limit).collect()
    }
}

fn print_supervisor_task_events(task: &SupervisedTask, event_rows: &[String], _limit: usize) {
    println!(
        "Events for supervisor task {} ({} entries):",
        task.id,
        event_rows.len()
    );
    for row in event_rows {
        println!("  {}", row);
    }
}

fn set_supervised_task_background_id(
    state: &mut AgentOsState,
    task_id: &str,
    background_task_id: Option<String>,
) {
    if let Some(task) = state
        .supervisor
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
    {
        task.background_task_id = background_task_id;
        task.updated_at = chrono::Utc::now().timestamp();
    }
}

fn cancel_queued_supervised_tasks(state: &mut AgentOsState) -> usize {
    let now = chrono::Utc::now().timestamp();
    let mut changed = 0usize;
    for task in state.supervisor.tasks.iter_mut() {
        if task.status == SupervisedTaskStatus::Queued {
            task.status = SupervisedTaskStatus::Cancelled;
            task.updated_at = now;
            task.background_task_id = None;
            changed += 1;
        }
    }
    changed
}

fn prune_supervisor_tasks(state: &mut AgentOsState, keep_terminal: usize) -> usize {
    let mut terminal = state
        .supervisor
        .tasks
        .iter()
        .filter(|task| {
            matches!(
                task.status,
                SupervisedTaskStatus::Completed
                    | SupervisedTaskStatus::Failed
                    | SupervisedTaskStatus::Cancelled
            )
        })
        .map(|task| (task.id.clone(), task.updated_at))
        .collect::<Vec<_>>();

    if terminal.len() <= keep_terminal {
        return 0;
    }

    terminal.sort_by(|a, b| b.1.cmp(&a.1));
    let keep_ids = terminal
        .into_iter()
        .take(keep_terminal)
        .map(|(id, _)| id)
        .collect::<HashSet<_>>();

    let before = state.supervisor.tasks.len();
    state.supervisor.tasks.retain(|task| {
        if matches!(
            task.status,
            SupervisedTaskStatus::Queued | SupervisedTaskStatus::Running
        ) {
            true
        } else {
            keep_ids.contains(&task.id)
        }
    });
    before.saturating_sub(state.supervisor.tasks.len())
}

fn reconcile_supervisor_after_restart(state: &mut AgentOsState) -> usize {
    let now = chrono::Utc::now().timestamp();
    let mut recovered = 0usize;
    for task in state.supervisor.tasks.iter_mut() {
        if task.status == SupervisedTaskStatus::Running {
            task.status = SupervisedTaskStatus::Failed;
            task.updated_at = now;
            task.background_task_id = None;
            task.error =
                Some("Recovered after restart: previous run did not finish cleanly".to_string());
            recovered += 1;
        }
    }
    recovered
}

fn print_supervisor_queue(state: &AgentOsState) {
    println!("\n🧰 Supervisor Queue");
    println!(
        "  actions: queue status | queue add <prompt> | queue run | queue clear"
    );
    if state.supervisor.tasks.is_empty() {
        println!("  empty");
        println!();
        return;
    }

    let mut tasks = state.supervisor.tasks.clone();
    tasks.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    for task in tasks.iter().take(20) {
        let bg = task.background_task_id.as_deref().unwrap_or("-");
        println!(
            "  {:<8} {:<13} {:<14} {:<14} {:<12} {}",
            truncate_for_table(&task.id, 8),
            format!("{:?}", task.status).to_lowercase(),
            task.class.as_str(),
            truncate_for_table(&task.resources.join(","), 14),
            truncate_for_table(bg, 12),
            truncate_for_table(&task.prompt, 52)
        );
    }
    if tasks.len() > 20 {
        println!("  ... and {} more", tasks.len() - 20);
    }
    println!();
}

fn print_supervisor_queue_status(state: &AgentOsState) {
    let queued = state
        .supervisor
        .tasks
        .iter()
        .filter(|task| task.status == SupervisedTaskStatus::Queued)
        .count();
    let running = state
        .supervisor
        .tasks
        .iter()
        .filter(|task| task.status == SupervisedTaskStatus::Running)
        .count();
    let completed = state
        .supervisor
        .tasks
        .iter()
        .filter(|task| task.status == SupervisedTaskStatus::Completed)
        .count();
    let failed = state
        .supervisor
        .tasks
        .iter()
        .filter(|task| task.status == SupervisedTaskStatus::Failed)
        .count();
    let cancelled = state
        .supervisor
        .tasks
        .iter()
        .filter(|task| task.status == SupervisedTaskStatus::Cancelled)
        .count();

    println!("\n📌 Supervisor Queue Status");
    println!(
        "  queued={} running={} done={} failed={} cancelled={}",
        queued,
        running,
        completed,
        failed,
        cancelled
    );

    let mut running_rows = state
        .supervisor
        .tasks
        .iter()
        .filter(|task| task.status == SupervisedTaskStatus::Running)
        .cloned()
        .collect::<Vec<_>>();
    running_rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    if !running_rows.is_empty() {
        println!("  running:");
        for task in running_rows.iter().take(6) {
            println!(
                "    {} bg={} res={} prompt={}",
                truncate_for_table(&task.id, 10),
                truncate_for_table(task.background_task_id.as_deref().unwrap_or("-"), 14),
                truncate_for_table(&task.resources.join(","), 16),
                truncate_for_table(&task.prompt, 46)
            );
        }
    }

    let mut retry_rows = state
        .supervisor
        .tasks
        .iter()
        .filter(|task| {
            matches!(
                task.status,
                SupervisedTaskStatus::Completed
                    | SupervisedTaskStatus::Failed
                    | SupervisedTaskStatus::Cancelled
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    retry_rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    if !retry_rows.is_empty() {
        println!("  retry candidates:");
        for task in retry_rows.iter().take(6) {
            println!(
                "    {} status={} class={} prompt={}",
                truncate_for_table(&task.id, 10),
                truncate_for_table(&format!("{:?}", task.status).to_lowercase(), 10),
                truncate_for_table(task.class.as_str(), 12),
                truncate_for_table(&task.prompt, 42)
            );
        }
    }
    println!();
}

fn running_background_conflict_reason(
    input: &str,
    running_tasks: &[hypr_claw_tasks::TaskInfo],
) -> Option<String> {
    let required = infer_supervised_task_resources(input);
    let mut collisions = Vec::new();

    for task in running_tasks {
        let running_resources = infer_supervised_task_resources(&task.description);
        let overlap = resource_conflicts(&required, &running_resources);
        if !overlap.is_empty() {
            collisions.push(format!("{} [{}]", task.id, overlap.join("+")));
        }
    }

    if collisions.is_empty() {
        None
    } else {
        Some(format!(
            "required [{}] conflicts with {}",
            required.join(","),
            collisions.join(", ")
        ))
    }
}

fn touch_active_thread(state: &mut AgentOsState) {
    for thread in &mut state.task_threads {
        if thread.id == state.active_thread_id {
            thread.updated_at = chrono::Utc::now().timestamp();
            return;
        }
    }
}

fn thread_session_key(base_session_key: &str, thread_id: &str) -> String {
    format!("{base_session_key}::thread::{thread_id}")
}

fn augment_system_prompt_for_turn(
    base_prompt: &str,
    profile: &Value,
    capability_registry: &Value,
    allowed_tools: &HashSet<String>,
    autonomy_mode: &AutonomyMode,
) -> String {
    let from_registry_str = |path: &str| -> Option<String> {
        capability_registry
            .pointer(path)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };

    let distro = from_registry_str("/platform/distro_name")
        .or_else(|| {
            profile
                .pointer("/platform/distro_name")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string());
    let kernel = from_registry_str("/platform/kernel")
        .or_else(|| {
            profile
                .pointer("/platform/kernel")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string());
    let workspace = capability_registry
        .pointer("/platform/active_workspace")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            profile
                .pointer("/desktop/active_workspace")
                .and_then(|v| v.as_u64())
        })
        .unwrap_or(0);

    let list_from_registry = |primary_path: &str, fallback_path: &str| -> String {
        let primary = read_string_array_from_value(capability_registry.pointer(primary_path));
        if !primary.is_empty() {
            return primary.join(", ");
        }
        let fallback = read_string_array_from_value(profile.pointer(fallback_path));
        if fallback.is_empty() {
            "none".to_string()
        } else {
            fallback.join(", ")
        }
    };

    let launcher_hints = list_from_registry(
        "/desktop_apps/launcher_commands",
        "/deep_scan/desktop_apps/launcher_commands_sample",
    );
    let preferred_launchers = capability_registry
        .pointer("/desktop_apps/preferred_launchers")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|value| format!("{k}={value}")))
                .collect::<Vec<String>>()
                .join(", ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "none".to_string());
    let project_hints = list_from_registry(
        "/projects/known_roots",
        "/deep_scan/home_inventory/project_roots",
    );
    let vscode_hint =
        from_registry_str("/editor/vscode_command").unwrap_or_else(|| "none".to_string());
    let registry_generated_at = capability_registry
        .pointer("/generated_at")
        .and_then(|v| v.as_i64())
        .map(format_timestamp)
        .unwrap_or_else(|| "n/a".to_string());
    let downloads_dir = from_registry_str("/paths/downloads")
        .or_else(|| {
            profile
                .pointer("/paths/downloads")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    let installed_packages = capability_registry
        .pointer("/packages/total_count")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            profile
                .pointer("/deep_scan/packages/total_count")
                .and_then(|v| v.as_u64())
        })
        .unwrap_or(0);

    let mut tools = allowed_tools.iter().cloned().collect::<Vec<String>>();
    tools.sort();
    let policy_block = if strict_workflow_enabled() {
        "Strict workflow:\n1) Observe first using desktop.read_screen_state/active_window/list_windows/cursor_position before GUI actions.\n2) Plan short and execute using tools, not explanation-only text.\n3) Prefer one decisive tool call at a time with valid JSON input.\n4) After each action, verify with tools (cursor/window/screen/file/process checks) and continue until done.\n5) Ask for user permission before high-impact or destructive actions.\n6) Stop only when truly blocked and report exact blocker + next best option."
    } else {
        match autonomy_mode {
            AutonomyMode::PromptFirst => {
                "Execution policy:\n1) Perform actions using tools, not explanation.\n2) You control fallback order dynamically; adapt tool strategy from observed failures.\n3) Prefer one decisive tool call at a time with valid JSON input.\n4) For opening local apps, prefer desktop.launch_app before proc.spawn/open_url and use launcher_commands/vscode_hint.\n5) End with concise result and what changed.\n6) Stop only when truly blocked after trying multiple tool paths."
            }
            AutonomyMode::Guarded => {
                "Execution policy:\n1) Perform actions using tools, not explanation.\n2) If a tool fails, choose an alternative backend/tool and continue.\n3) For opening local apps, try desktop.launch_app first, then proc.spawn as fallback; use launcher_commands/vscode_hint.\n4) End with concise result and what changed.\n5) Only stop as blocked after trying alternatives available in runtime context."
            }
        }
    };

    format!(
        "{}\n\nRuntime context:\n- autonomy_mode: {}\n- workflow_mode: {}\n- capability_registry_generated_at: {}\n- distro: {}\n- kernel: {}\n- active_workspace: {}\n- wallpaper_backends: {}\n- screenshot_backends: {}\n- input_backends: {}\n- vscode_hint: {}\n- launcher_commands: {}\n- preferred_launchers: {}\n- known_projects: {}\n- known_downloads_dir: {}\n- installed_packages: {}\n- allowed_tools_now: {}\n\n{}",
        base_prompt,
        autonomy_mode.as_str(),
        if strict_workflow_enabled() { "strict" } else { "legacy" },
        registry_generated_at,
        distro,
        kernel,
        workspace,
        list_from_registry("/capabilities/wallpaper_backends", "/capabilities/wallpaper_backends"),
        list_from_registry("/capabilities/screenshot_backends", "/capabilities/screenshot_backends"),
        list_from_registry("/capabilities/input_backends", "/capabilities/input_backends"),
        vscode_hint,
        launcher_hints,
        preferred_launchers,
        project_hints,
        downloads_dir,
        installed_packages,
        tools.join(", "),
        policy_block
    )
}

fn focused_tools_for_input(input: &str, allowed: &HashSet<String>) -> HashSet<String> {
    let lower = input.to_lowercase();
    let mut preferred = HashSet::new();

    let add = |set: &mut HashSet<String>, name: &str, allowed: &HashSet<String>| {
        if allowed.contains(name) {
            set.insert(name.to_string());
        }
    };

    if lower.contains("wallpaper") || lower.contains("workspace") || lower.contains("window") {
        add(&mut preferred, "wallpaper.set", allowed);
        add(&mut preferred, "hypr.workspace.switch", allowed);
        add(&mut preferred, "hypr.workspace.move_window", allowed);
        add(&mut preferred, "hypr.window.focus", allowed);
        add(&mut preferred, "hypr.window.close", allowed);
        add(&mut preferred, "hypr.window.move", allowed);
        add(&mut preferred, "hypr.exec", allowed);
    }

    if lower.contains("file")
        || lower.contains("folder")
        || lower.contains("dir")
        || lower.contains("read")
        || lower.contains("write")
        || lower.contains("create")
        || lower.contains("delete")
        || lower.contains("copy")
        || lower.contains("move")
    {
        add(&mut preferred, "fs.read", allowed);
        add(&mut preferred, "fs.write", allowed);
        add(&mut preferred, "fs.list", allowed);
        add(&mut preferred, "fs.create_dir", allowed);
        add(&mut preferred, "fs.delete", allowed);
        add(&mut preferred, "fs.copy", allowed);
        add(&mut preferred, "fs.move", allowed);
    }

    if lower.contains("open")
        || lower.contains("browser")
        || lower.contains("search")
        || lower.contains("web")
        || lower.contains("gmail")
        || lower.contains("mail")
    {
        add(&mut preferred, "desktop.launch_app", allowed);
        if lower.contains("gmail") || lower.contains("mail") {
            add(&mut preferred, "desktop.open_gmail", allowed);
            add(&mut preferred, "desktop.open_url", allowed);
            add(&mut preferred, "desktop.search_web", allowed);
        } else if lower.contains("browser") || lower.contains("search") || lower.contains("web") {
            add(&mut preferred, "desktop.search_web", allowed);
            add(&mut preferred, "desktop.open_url", allowed);
        } else {
            add(&mut preferred, "desktop.open_url", allowed);
            add(&mut preferred, "desktop.search_web", allowed);
        }
    }

    if lower.contains("type")
        || lower.contains("press")
        || lower.contains("shortcut")
        || lower.contains("hotkey")
        || lower.contains("click")
        || lower.contains("mouse")
        || lower.contains("screenshot")
        || lower.contains("screen")
    {
        add(&mut preferred, "desktop.read_screen_state", allowed);
        add(&mut preferred, "desktop.cursor_position", allowed);
        add(&mut preferred, "desktop.type_text", allowed);
        add(&mut preferred, "desktop.key_press", allowed);
        add(&mut preferred, "desktop.key_combo", allowed);
        add(&mut preferred, "desktop.mouse_click", allowed);
        add(&mut preferred, "desktop.capture_screen", allowed);
        add(&mut preferred, "desktop.active_window", allowed);
        add(&mut preferred, "desktop.list_windows", allowed);
        add(&mut preferred, "desktop.mouse_move", allowed);
        add(&mut preferred, "desktop.mouse_move_and_verify", allowed);
        add(&mut preferred, "desktop.click_at", allowed);
        add(&mut preferred, "desktop.click_at_and_verify", allowed);
        add(&mut preferred, "desktop.ocr_screen", allowed);
        add(&mut preferred, "desktop.find_text", allowed);
        add(&mut preferred, "desktop.click_text", allowed);
    }

    if lower.contains("run")
        || lower.contains("build")
        || lower.contains("start")
        || lower.contains("execute")
        || lower.contains("kill")
        || lower.contains("process")
    {
        add(&mut preferred, "proc.spawn", allowed);
        add(&mut preferred, "proc.kill", allowed);
        add(&mut preferred, "proc.list", allowed);
        add(&mut preferred, "hypr.exec", allowed);
    }

    if lower.contains("battery") || lower.contains("memory") || lower.contains("system") {
        add(&mut preferred, "system.battery", allowed);
        add(&mut preferred, "system.memory", allowed);
    }

    if preferred.is_empty() {
        return allowed.clone();
    }

    add(&mut preferred, "echo", allowed);
    preferred
}

fn emergency_tool_subset(input: &str, allowed: &HashSet<String>) -> HashSet<String> {
    let focused = focused_tools_for_input(input, allowed);
    if focused.is_empty() {
        return HashSet::new();
    }
    if focused.len() <= 10 {
        return focused;
    }
    let mut names: Vec<String> = focused.into_iter().collect();
    names.sort();
    names.into_iter().take(10).collect()
}

fn fallback_playbook_for_input(input: &str, allowed: &HashSet<String>) -> String {
    let lower = input.to_lowercase();
    let mut rows: Vec<(&str, Vec<&str>)> = Vec::new();

    let wallpaper_intent = lower.contains("wallpaper") || lower.contains("background");
    if wallpaper_intent {
        rows.push((
            "wallpaper",
            vec!["wallpaper.set", "hypr.exec", "desktop.open_url"],
        ));
    }

    let desktop_intent = lower.contains("open")
        || lower.contains("launch")
        || lower.contains("gmail")
        || lower.contains("telegram")
        || lower.contains("browser")
        || lower.contains("vscode");
    if desktop_intent {
        rows.push((
            "desktop-open",
            vec![
                "desktop.launch_app",
                "desktop.open_url",
                "proc.spawn",
                "hypr.exec",
            ],
        ));
    }

    let screen_intent = lower.contains("screen")
        || lower.contains("ocr")
        || lower.contains("read my")
        || lower.contains("summarise");
    if screen_intent {
        rows.push((
            "screen-automation",
            vec![
                "desktop.read_screen_state",
                "desktop.cursor_position",
                "desktop.capture_screen",
                "desktop.ocr_screen",
                "desktop.find_text",
                "desktop.mouse_move_and_verify",
                "desktop.click_at_and_verify",
                "desktop.click_text",
                "desktop.mouse_click",
            ],
        ));
    }

    let process_intent = lower.contains("run")
        || lower.contains("build")
        || lower.contains("install")
        || lower.contains("process")
        || lower.contains("command");
    if process_intent {
        rows.push((
            "process",
            vec!["proc.spawn", "hypr.exec", "proc.list", "proc.kill"],
        ));
    }

    let fs_intent = lower.contains("file")
        || lower.contains("folder")
        || lower.contains("dir")
        || lower.contains("write")
        || lower.contains("read")
        || lower.contains("create");
    if fs_intent {
        rows.push((
            "filesystem",
            vec![
                "fs.list",
                "fs.create_dir",
                "fs.write",
                "fs.read",
                "fs.copy",
                "fs.move",
                "fs.delete",
            ],
        ));
    }

    if rows.is_empty() {
        let fallback = emergency_tool_subset(input, allowed)
            .into_iter()
            .collect::<Vec<_>>();
        if fallback.is_empty() {
            return "- no fallback tools available".to_string();
        }
        return format!("- generic: {}", fallback.join(" -> "));
    }

    let mut lines = Vec::new();
    for (label, chain) in rows {
        let available = chain
            .into_iter()
            .filter(|tool| allowed.contains(*tool))
            .collect::<Vec<_>>();
        if !available.is_empty() {
            lines.push(format!("- {}: {}", label, available.join(" -> ")));
        }
    }
    if lines.is_empty() {
        "- no fallback tools available".to_string()
    } else {
        lines.join("\n")
    }
}

// Adapter for ToolDispatcher
struct RuntimeDispatcherAdapter {
    inner: Arc<hypr_claw_tools::ToolDispatcherImpl>,
    action_feed: Arc<Mutex<Vec<String>>>,
    action_counter: Arc<Mutex<HashMap<String, u64>>>,
}

impl RuntimeDispatcherAdapter {
    fn new(
        inner: Arc<hypr_claw_tools::ToolDispatcherImpl>,
        action_feed: Arc<Mutex<Vec<String>>>,
    ) -> Self {
        Self {
            inner,
            action_feed,
            action_counter: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Prefix with [task_id] when session_key is a supervisor task (e.g. "user:agent::sup::sup-1").
    pub(crate) fn task_tag(session_key: &str) -> Option<&str> {
        session_key.split("::sup::").nth(1)
    }

    fn push_action(&self, session_key: &str, line: String) {
        let tagged = if let Some(tag) = Self::task_tag(session_key) {
            format!("[{}] {}", tag, line)
        } else {
            line
        };
        if let Ok(mut feed) = self.action_feed.lock() {
            feed.push(tagged);
            if feed.len() > 256 {
                let drop_count = feed.len() - 256;
                feed.drain(0..drop_count);
            }
        }
    }

    fn next_action_index(&self, session_key: &str) -> u64 {
        if let Ok(mut counters) = self.action_counter.lock() {
            let entry = counters.entry(session_key.to_string()).or_insert(0);
            *entry = entry.saturating_add(1);
            return *entry;
        }
        0
    }

    fn print_action(
        &self,
        session_key: &str,
        index: u64,
        status: &str,
        tool_name: &str,
        detail: &str,
    ) {
        let tool = truncate_for_table(tool_name, 30);
        let detail_clean = truncate_for_table(&sanitize_single_line(detail), 108);
        let line = format!(
            "{:>3}. {:<7} {:<30} {}",
            index,
            status,
            tool,
            detail_clean
        );
        self.push_action(session_key, line.clone());
        let status_badge = match status {
            "ok" => ui_success("OK"),
            "tool" => ui_info("TOOL"),
            "alias" => ui_warn("MAP"),
            "fail" => ui_danger("FAIL"),
            "error" => ui_danger("ERR"),
            _ => ui_dim(status),
        };
        println!(
            "{} {} {} {}",
            ui_dim(&format!("{:>3}.", index)),
            status_badge,
            truncate_for_table(tool_name, 30),
            detail_clean
        );
    }
}

#[async_trait::async_trait]
impl hypr_claw_runtime::ToolDispatcher for RuntimeDispatcherAdapter {
    async fn execute(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        session_key: &str,
    ) -> Result<serde_json::Value, hypr_claw_runtime::RuntimeError> {
        let normalized_tool_name = normalize_runtime_tool_name(tool_name);
        let started = std::time::Instant::now();
        let action_index = self.next_action_index(session_key);
        self.print_action(
            session_key,
            action_index,
            "tool",
            &normalized_tool_name,
            &format!("input={}", truncate_for_table(&input.to_string(), 112)),
        );
        if normalized_tool_name != tool_name {
            self.print_action(
                session_key,
                action_index,
                "alias",
                &normalized_tool_name,
                &format!(
                    "remapped '{}' -> '{}'",
                    truncate_for_table(tool_name, 24),
                    truncate_for_table(&normalized_tool_name, 24)
                ),
            );
        }
        let result = self
            .inner
            .dispatch(
                session_key.to_string(),
                normalized_tool_name.clone(),
                input.clone(),
            )
            .await;

        match result {
            Ok(tool_result) => {
                if tool_result.success {
                    let output = tool_result.output.unwrap_or(serde_json::json!({}));
                    let elapsed = started.elapsed().as_millis();
                    self.print_action(
                        session_key,
                        action_index,
                        "ok",
                        &normalized_tool_name,
                        &format!(
                            "done in {}ms output={}",
                            elapsed,
                            truncate_for_table(&output.to_string(), 112)
                        ),
                    );
                    Ok(output)
                } else {
                    let base_detail = tool_result
                        .error
                        .clone()
                        .unwrap_or_else(|| "Unknown error".to_string());
                    let alternatives = fallback_tools_for_tool(&normalized_tool_name);
                    let detail = if alternatives.is_empty() {
                        base_detail
                    } else {
                        format!(
                            "{} | fallback_tools: {}",
                            base_detail,
                            alternatives.join(", ")
                        )
                    };
                    self.print_action(
                        session_key,
                        action_index,
                        "fail",
                        &normalized_tool_name,
                        &format!(
                            "failed in {}ms {}",
                            started.elapsed().as_millis(),
                            truncate_for_table(&detail, 112)
                        ),
                    );
                    Err(hypr_claw_runtime::RuntimeError::ToolError(detail))
                }
            }
            Err(e) => {
                let base_detail = e.to_string();
                let alternatives = fallback_tools_for_tool(&normalized_tool_name);
                let detail = if alternatives.is_empty() {
                    base_detail
                } else {
                    format!(
                        "{} | fallback_tools: {}",
                        base_detail,
                        alternatives.join(", ")
                    )
                };
                self.print_action(
                    session_key,
                    action_index,
                    "error",
                    &normalized_tool_name,
                    &format!(
                        "dispatcher in {}ms {}",
                        started.elapsed().as_millis(),
                        truncate_for_table(&detail, 112)
                    ),
                );
                Err(hypr_claw_runtime::RuntimeError::ToolError(detail))
            }
        }
    }
}

fn fallback_tools_for_tool(tool_name: &str) -> Vec<&'static str> {
    match tool_name {
        "wallpaper.set" => vec!["hypr.exec"],
        "desktop.open_gmail" => vec!["desktop.open_url", "desktop.launch_app"],
        "desktop.open_url" => vec!["desktop.search_web", "desktop.launch_app"],
        "desktop.launch_app" => vec!["proc.spawn", "hypr.exec"],
        "proc.spawn" => vec!["hypr.exec", "desktop.launch_app"],
        "desktop.capture_screen" => vec!["hypr.exec"],
        "desktop.ocr_screen" => vec!["desktop.capture_screen"],
        "desktop.click_text" => vec!["desktop.find_text", "desktop.mouse_click"],
        "desktop.find_text" => vec!["desktop.ocr_screen", "desktop.capture_screen"],
        "desktop.read_screen_state" => vec![
            "desktop.active_window",
            "desktop.list_windows",
            "desktop.cursor_position",
            "desktop.capture_screen",
            "desktop.ocr_screen",
        ],
        "desktop.mouse_move" => vec!["desktop.mouse_move_and_verify", "desktop.click_at"],
        "desktop.click_at" => vec!["desktop.click_at_and_verify", "desktop.mouse_click"],
        "desktop.click_at_and_verify" => vec!["desktop.mouse_move_and_verify", "desktop.click_at"],
        "hypr.workspace.switch" => vec!["hypr.exec"],
        "hypr.window.focus" => vec!["hypr.exec"],
        "hypr.window.move" => vec!["hypr.exec"],
        "hypr.window.close" => vec!["hypr.exec"],
        "hypr.exec" => vec!["proc.spawn"],
        "fs.list" => vec!["hypr.exec"],
        "fs.read" => vec!["hypr.exec"],
        "fs.write" => vec!["hypr.exec"],
        _ => Vec::new(),
    }
}

// Adapter for ToolRegistry
struct RuntimeRegistryAdapter {
    inner: Arc<hypr_claw_tools::ToolRegistryImpl>,
    allowed_tools: Arc<RwLock<HashSet<String>>>,
}

impl RuntimeRegistryAdapter {
    fn new(
        inner: Arc<hypr_claw_tools::ToolRegistryImpl>,
        allowed_tools: Arc<RwLock<HashSet<String>>>,
    ) -> Self {
        Self {
            inner,
            allowed_tools,
        }
    }

    fn set_allowed_tools(&self, allowed_tools: HashSet<String>) {
        if let Ok(mut guard) = self.allowed_tools.write() {
            *guard = allowed_tools;
        }
    }
}

impl hypr_claw_runtime::ToolRegistry for RuntimeRegistryAdapter {
    fn get_active_tools(&self, _agent_id: &str) -> Vec<String> {
        let allowed = self.allowed_tools.read().ok();
        let Some(allowed) = allowed else {
            return Vec::new();
        };
        self.inner
            .list()
            .into_iter()
            .filter(|name| allowed.contains(name))
            .collect()
    }

    fn get_tool_schemas(&self, _agent_id: &str) -> Vec<serde_json::Value> {
        let allowed = self.allowed_tools.read().ok();
        let Some(allowed) = allowed else {
            return Vec::new();
        };
        self.inner
            .schemas()
            .into_iter()
            .filter(|schema| {
                schema
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .map(|name| allowed.contains(name))
                    .unwrap_or(false)
            })
            .collect()
    }
}

// Simple summarizer implementation
struct SimpleSummarizer;

impl hypr_claw_runtime::Summarizer for SimpleSummarizer {
    fn summarize(
        &self,
        messages: &[hypr_claw_runtime::Message],
    ) -> Result<String, hypr_claw_runtime::RuntimeError> {
        Ok(format!("Summary of {} messages", messages.len()))
    }
}

#[cfg(test)]
mod reliability_policy_tests {
    use super::*;

    #[test]
    fn stop_code_mapping_basic_paths() {
        assert_eq!(
            stop_code_for_error("Interrupted by user"),
            "STOP_USER_INTERRUPT"
        );
        assert_eq!(
            stop_code_for_error("Execution watchdog timeout after 90s"),
            "STOP_WATCHDOG_TIMEOUT"
        );
        assert_eq!(
            stop_code_for_error("Detected repetitive tool loop for 'desktop.capture_screen'"),
            "STOP_TOOL_LOOP"
        );
        assert_eq!(
            stop_code_for_error("Too many consecutive tool failures. Halting this run."),
            "STOP_TOOL_FAILURE_STREAK"
        );
        assert_eq!(
            stop_code_for_error("Tool invocation required but not performed"),
            "STOP_TOOL_ENFORCEMENT"
        );
        assert_eq!(
            stop_code_for_error("Action requested, but no tool call completed successfully."),
            "STOP_TOOL_ENFORCEMENT"
        );
        assert_eq!(
            stop_code_for_error("HTTP error: 400 Bad Request. Details: INVALID_ARGUMENT"),
            "STOP_PROVIDER_ARGUMENT"
        );
        assert_eq!(
            stop_code_for_error(
                "LLM error: Rate limit exceeded. Details: {\"status\":429,\"title\":\"Too Many Requests\"}"
            ),
            "STOP_PROVIDER_RATE_LIMIT"
        );
        assert_eq!(
            stop_code_for_error("Max iterations (16) reached after 16 tool calls"),
            "STOP_MAX_ITERATIONS"
        );
        assert_eq!(
            stop_code_for_error("some unknown runtime"),
            "STOP_RUNTIME_ERROR"
        );
    }

    #[test]
    fn stop_code_resolution_respects_recovery_budget() {
        let err = "Max iterations (16) reached after 16 tool calls";
        assert_eq!(resolve_stop_code(err, 0), "STOP_MAX_ITERATIONS");
        assert_eq!(
            resolve_stop_code(err, MAX_RECOVERY_ATTEMPTS),
            "STOP_RECOVERY_BUDGET_EXHAUSTED"
        );
    }

    #[test]
    fn stop_code_resolution_keeps_user_interrupt_code() {
        assert_eq!(
            resolve_stop_code("Interrupted by user", MAX_RECOVERY_ATTEMPTS + 3),
            "STOP_USER_INTERRUPT"
        );
    }

    #[test]
    fn retry_after_parser_supports_fractional_seconds() {
        let err =
            "Rate limit exceeded. Please retry in 18.932776315s. quotaId=GenerateRequestsPerMinute";
        assert_eq!(extract_retry_after_seconds(err), Some(19));
    }

    #[test]
    fn remediation_hints_are_contextual() {
        let mail_hint =
            remediation_hint_for_stop_code("STOP_MAX_ITERATIONS", "open gmail and summarise");
        assert!(mail_hint.contains("open + capture"));

        let wallpaper_hint =
            remediation_hint_for_stop_code("STOP_MAX_ITERATIONS", "change wallpaper");
        assert!(wallpaper_hint.contains("caelestia"));

        let code_hint = remediation_hint_for_stop_code("STOP_MAX_ITERATIONS", "open vscode");
        assert!(code_hint.contains("launch command"));

        let generic_hint = remediation_hint_for_stop_code("STOP_RUNTIME_ERROR", "anything");
        assert!(generic_hint.contains("status"));
    }

    #[test]
    fn autonomy_defaults_to_prompt_first() {
        assert_eq!(default_autonomy_mode(), AutonomyMode::PromptFirst);
    }

    #[test]
    fn prompt_first_budget_is_higher_than_guarded() {
        let q_prompt =
            execution_budget_for_class(&SupervisedTaskClass::Question, &AutonomyMode::PromptFirst)
                .max_iterations;
        let q_guarded =
            execution_budget_for_class(&SupervisedTaskClass::Question, &AutonomyMode::Guarded)
                .max_iterations;
        assert!(q_prompt > q_guarded);
    }

    #[test]
    fn autonomy_calibration_records_by_mode() {
        let mut state = AgentOsState::default();
        record_autonomy_outcome(
            &mut state,
            &AutonomyMode::PromptFirst,
            true,
            1200,
            1,
            "STOP_NONE",
        );
        record_autonomy_outcome(
            &mut state,
            &AutonomyMode::Guarded,
            false,
            900,
            2,
            "STOP_MAX_ITERATIONS",
        );

        assert_eq!(state.autonomy_calibration.prompt_first.runs, 1);
        assert_eq!(state.autonomy_calibration.prompt_first.successes, 1);
        assert_eq!(
            *state
                .autonomy_calibration
                .prompt_first
                .stop_codes
                .get("STOP_NONE")
                .unwrap_or(&0),
            1
        );

        assert_eq!(state.autonomy_calibration.guarded.runs, 1);
        assert_eq!(state.autonomy_calibration.guarded.failures, 1);
        assert_eq!(
            *state
                .autonomy_calibration
                .guarded
                .stop_codes
                .get("STOP_MAX_ITERATIONS")
                .unwrap_or(&0),
            1
        );
    }

    #[test]
    fn capability_registry_path_is_sanitized() {
        let path = capability_registry_file_path("rick@local host");
        assert!(path.contains("rick_local_host"));
        assert!(path.ends_with(".json"));
    }

    #[test]
    fn capability_registry_refreshes_when_profile_is_newer() {
        let profile = json!({
            "scanned_at": 100,
            "paths": { "downloads": "/home/rick/Downloads" },
            "capabilities": { "wallpaper_backends": ["caelestia"] }
        });
        let registry = json!({
            "schema_version": 1,
            "source_profile_scanned_at": 90,
            "paths": { "downloads": "/home/rick/Downloads" },
            "capabilities": { "wallpaper_backends": ["caelestia"] },
            "source_has_deep_scan": false
        });
        assert!(capability_registry_needs_refresh(&registry, &profile));
    }

    #[test]
    fn capability_registry_diff_lines_report_key_changes() {
        let old_registry = json!({
            "platform": {
                "distro_name": "Arch Linux",
                "kernel": "6.18.1",
                "active_workspace": 2
            },
            "capabilities": {
                "wallpaper_backends": ["hyprpaper"],
                "screenshot_backends": ["grim"],
                "input_backends": ["wtype"]
            },
            "desktop_apps": {
                "launcher_commands": ["firefox"]
            },
            "projects": {
                "known_roots": ["/home/rick/hypr-claw"]
            },
            "editor": {
                "vscode_command": "code"
            }
        });
        let new_registry = json!({
            "platform": {
                "distro_name": "Arch Linux",
                "kernel": "6.18.7",
                "active_workspace": 4
            },
            "capabilities": {
                "wallpaper_backends": ["caelestia"],
                "screenshot_backends": ["grim", "grimblast"],
                "input_backends": ["ydotool"]
            },
            "desktop_apps": {
                "launcher_commands": ["firefox", "kitty"]
            },
            "projects": {
                "known_roots": ["/home/rick/hypr-claw", "/home/rick/other"]
            },
            "editor": {
                "vscode_command": "code-oss"
            }
        });

        let lines = capability_registry_diff_lines(&old_registry, &new_registry);
        assert!(lines.iter().any(|line| line.contains("platform kernel")));
        assert!(lines.iter().any(|line| line.contains("active workspace")));
        assert!(lines.iter().any(|line| line.contains("wallpaper backends")));
        assert!(lines.iter().any(|line| line.contains("vscode command")));
    }

    #[test]
    fn preferred_launchers_pick_known_commands() {
        let launcher_commands = vec![
            "firefox".to_string(),
            "telegram-desktop".to_string(),
            "kitty".to_string(),
        ];
        let available_commands = vec!["code-oss".to_string()];
        let preferred =
            build_preferred_launchers(&launcher_commands, &available_commands, "code-oss");

        assert_eq!(
            preferred.pointer("/vscode").and_then(|v| v.as_str()),
            Some("code-oss")
        );
        assert_eq!(
            preferred.pointer("/browser").and_then(|v| v.as_str()),
            Some("firefox")
        );
        assert_eq!(
            preferred.pointer("/telegram").and_then(|v| v.as_str()),
            Some("telegram-desktop")
        );
        assert_eq!(
            preferred.pointer("/terminal").and_then(|v| v.as_str()),
            Some("kitty")
        );
    }

    #[test]
    fn inferred_resources_include_desktop_and_network() {
        let resources = infer_supervised_task_resources("open telegram and summarize messages");
        assert!(resources.iter().any(|r| r == "desktop_input"));
        assert!(resources.iter().any(|r| r == "network"));
    }

    #[test]
    fn shared_resources_do_not_conflict() {
        let a = vec!["network".to_string()];
        let b = vec!["network".to_string(), "compute".to_string()];
        let conflicts = resource_conflicts(&a, &b);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn running_background_conflict_reason_reports_overlap() {
        let now = chrono::Utc::now().timestamp();
        let running = vec![hypr_claw_tasks::TaskInfo {
            id: "bg-99".to_string(),
            description: "open browser and click login".to_string(),
            status: hypr_claw_tasks::TaskStatus::Running,
            progress: 0.3,
            created_at: now - 10,
            updated_at: now - 2,
            result: None,
            error: None,
        }];

        let reason = running_background_conflict_reason("open gmail and reply", &running)
            .unwrap_or_default();
        assert!(reason.contains("bg-99"));
        assert!(reason.contains("desktop_input"));
    }

    #[test]
    fn queue_start_is_blocked_when_another_task_is_running() {
        let mut state = AgentOsState::default();
        start_supervised_task(
            &mut state,
            "open firefox".to_string(),
            SupervisedTaskClass::Action,
        );
        enqueue_supervised_task(
            &mut state,
            "open telegram".to_string(),
            SupervisedTaskClass::Action,
        );
        match start_next_queued_supervised_task(&mut state) {
            QueueStartResult::Blocked(_) => {}
            _ => panic!("expected queue start to be blocked by running task"),
        }
    }

    #[test]
    fn queue_start_allows_non_conflicting_resources() {
        let mut state = AgentOsState::default();
        start_supervised_task(
            &mut state,
            "open firefox".to_string(),
            SupervisedTaskClass::Action,
        );
        enqueue_supervised_task(
            &mut state,
            "search web api docs".to_string(),
            SupervisedTaskClass::Action,
        );
        match start_next_queued_supervised_task(&mut state) {
            QueueStartResult::Started(task) => {
                assert_eq!(task.status, SupervisedTaskStatus::Running);
                assert!(task.resources.iter().any(|r| r == "network"));
            }
            QueueStartResult::Blocked(reason) => {
                panic!(
                    "expected non-conflicting task to start, got blocked: {}",
                    reason
                )
            }
            QueueStartResult::Empty => panic!("expected queued task to start"),
        }
    }

    #[test]
    fn mark_cancelled_clears_background_link() {
        let mut state = AgentOsState::default();
        let task_id = start_supervised_task(
            &mut state,
            "search docs".to_string(),
            SupervisedTaskClass::Action,
        );
        set_supervised_task_background_id(&mut state, &task_id, Some("supbg-1".to_string()));
        mark_supervised_task_cancelled(&mut state, &task_id, Some("Cancelled by test".to_string()));
        let task = state
            .supervisor
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .expect("task must exist");
        assert_eq!(task.status, SupervisedTaskStatus::Cancelled);
        assert!(task.background_task_id.is_none());
    }

    #[test]
    fn prune_supervisor_tasks_keeps_running_and_latest_terminal() {
        let now = chrono::Utc::now().timestamp();
        let mut state = AgentOsState::default();
        state.supervisor.tasks = vec![
            SupervisedTask {
                id: "sup-run".to_string(),
                prompt: "running".to_string(),
                class: SupervisedTaskClass::Action,
                resources: vec!["network".to_string()],
                background_task_id: Some("supbg-run".to_string()),
                status: SupervisedTaskStatus::Running,
                created_at: now - 50,
                updated_at: now - 5,
                error: None,
            },
            SupervisedTask {
                id: "sup-old-1".to_string(),
                prompt: "old1".to_string(),
                class: SupervisedTaskClass::Action,
                resources: vec!["general".to_string()],
                background_task_id: None,
                status: SupervisedTaskStatus::Completed,
                created_at: now - 100,
                updated_at: now - 100,
                error: None,
            },
            SupervisedTask {
                id: "sup-old-2".to_string(),
                prompt: "old2".to_string(),
                class: SupervisedTaskClass::Action,
                resources: vec!["general".to_string()],
                background_task_id: None,
                status: SupervisedTaskStatus::Failed,
                created_at: now - 90,
                updated_at: now - 90,
                error: Some("x".to_string()),
            },
            SupervisedTask {
                id: "sup-new".to_string(),
                prompt: "new".to_string(),
                class: SupervisedTaskClass::Action,
                resources: vec!["general".to_string()],
                background_task_id: None,
                status: SupervisedTaskStatus::Cancelled,
                created_at: now - 10,
                updated_at: now - 1,
                error: Some("y".to_string()),
            },
        ];

        let removed = prune_supervisor_tasks(&mut state, 1);
        assert_eq!(removed, 2);
        assert!(state.supervisor.tasks.iter().any(|t| t.id == "sup-run"));
        assert!(state.supervisor.tasks.iter().any(|t| t.id == "sup-new"));
        assert!(!state.supervisor.tasks.iter().any(|t| t.id == "sup-old-1"));
        assert!(!state.supervisor.tasks.iter().any(|t| t.id == "sup-old-2"));
    }

    #[test]
    fn reconcile_marks_stale_running_tasks_as_failed() {
        let mut state = AgentOsState::default();
        start_supervised_task(
            &mut state,
            "open firefox".to_string(),
            SupervisedTaskClass::Action,
        );
        let recovered = reconcile_supervisor_after_restart(&mut state);
        assert_eq!(recovered, 1);
        assert!(state
            .supervisor
            .tasks
            .iter()
            .any(|t| t.status == SupervisedTaskStatus::Failed));
    }

    #[test]
    fn tail_window_slice_paginates_from_latest() {
        let rows: Vec<u32> = (1..=12).collect();
        let (latest, start, end, total) = tail_window_slice(&rows, 0, 5);
        assert_eq!(latest, vec![8, 9, 10, 11, 12]);
        assert_eq!((start, end, total), (8, 12, 12));

        let (older, start, end, total) = tail_window_slice(&rows, 5, 5);
        assert_eq!(older, vec![3, 4, 5, 6, 7]);
        assert_eq!((start, end, total), (3, 7, 12));
    }

    #[test]
    fn task_log_lines_include_background_and_supervisor_entries() {
        let now = chrono::Utc::now().timestamp();
        let bg_tasks = vec![hypr_claw_tasks::TaskInfo {
            id: "bg-1".to_string(),
            description: "compile project".to_string(),
            status: hypr_claw_tasks::TaskStatus::Completed,
            progress: 1.0,
            created_at: now - 5,
            updated_at: now - 2,
            result: Some("done".to_string()),
            error: None,
        }];

        let mut state = AgentOsState::default();
        state.supervisor.tasks.push(SupervisedTask {
            id: "sup-1".to_string(),
            prompt: "open firefox".to_string(),
            class: SupervisedTaskClass::Action,
            resources: vec!["desktop_input".to_string()],
            background_task_id: None,
            status: SupervisedTaskStatus::Failed,
            created_at: now - 4,
            updated_at: now - 1,
            error: Some("missing binary".to_string()),
        });

        let task_event_feed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let lines = build_task_log_lines(&bg_tasks, &state, &task_event_feed);
        assert!(lines.iter().any(|line| line.contains("bg bg-1")));
        assert!(lines.iter().any(|line| line.contains("sup sup-1")));
    }

    #[test]
    fn action_feed_task_tag_extracts_supervisor_id() {
        assert_eq!(
            RuntimeDispatcherAdapter::task_tag("user:agent::sup::sup-1"),
            Some("sup-1")
        );
        assert_eq!(
            RuntimeDispatcherAdapter::task_tag("alice:hypr-claw::sup::sup-42"),
            Some("sup-42")
        );
        assert_eq!(RuntimeDispatcherAdapter::task_tag("user:agent"), None);
        assert_eq!(RuntimeDispatcherAdapter::task_tag(""), None);
    }

    #[test]
    fn sanitize_single_line_strips_ansi_and_controls() {
        let raw = "hello\u{1b}[31m red\u{1b}[0m world\u{0008}";
        let cleaned = sanitize_single_line(raw);
        assert_eq!(cleaned, "hello red world");
        assert!(!cleaned.contains('\u{1b}'));
    }

    #[test]
    fn sanitize_user_input_removes_prompt_wrappers() {
        let raw = "hypr[task-1|glm4.7|run:0]> open mail\u{1b}[D";
        let cleaned = sanitize_user_input_line(raw);
        assert_eq!(cleaned, "open mail");
    }

    #[test]
    fn capability_delta_history_roundtrip() {
        let old_r = json!({"platform": {"distro_name": "old"}, "generated_at": 100});
        let new_r = json!({"platform": {"distro_name": "arch"}, "generated_at": 200});
        let diff = capability_registry_diff_lines(&old_r, &new_r);
        assert!(
            !diff.is_empty(),
            "diff should be non-empty when registries differ"
        );
        let record = json!({
            "at": chrono::Utc::now().timestamp(),
            "summary": format!("{} change(s)", diff.len()),
            "diff_lines": diff,
        });
        let line = serde_json::to_string(&record).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        let read_back = parsed.get("diff_lines").unwrap().as_array().unwrap();
        assert!(read_back.len() > 0);
    }

    #[test]
    fn start_next_returns_empty_when_no_queued_tasks() {
        let mut state = AgentOsState::default();
        let result = start_next_queued_supervised_task(&mut state);
        assert!(matches!(result, QueueStartResult::Empty));
        state.supervisor.tasks.push(SupervisedTask {
            id: "sup-1".to_string(),
            prompt: "done task".to_string(),
            class: SupervisedTaskClass::Action,
            resources: vec!["network".to_string()],
            background_task_id: None,
            status: SupervisedTaskStatus::Completed,
            created_at: 0,
            updated_at: 0,
            error: None,
        });
        let result2 = start_next_queued_supervised_task(&mut state);
        assert!(matches!(result2, QueueStartResult::Empty));
    }
}
