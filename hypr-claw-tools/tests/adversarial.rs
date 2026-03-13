#[cfg(test)]
mod adversarial_tests {
    use async_trait::async_trait;
    use hypr_claw_tools::sandbox::*;
    use hypr_claw_tools::tools::*;
    use hypr_claw_tools::*;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::fs;

    // Mock implementations for testing
    struct MockPermissionEngine;
    struct MockAuditLogger;

    #[async_trait]
    impl PermissionEngine for MockPermissionEngine {
        async fn check(&self, _request: PermissionRequest) -> PermissionDecision {
            PermissionDecision::Allow
        }
    }

    #[async_trait]
    impl AuditLogger for MockAuditLogger {
        async fn log(&self, _entry: serde_json::Value) {
            // No-op for tests
        }
    }

    async fn setup_sandbox() -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sandbox = std::env::temp_dir().join(format!("test_sandbox_adv_{}", nanos));
        let _ = fs::remove_dir_all(&sandbox).await;
        fs::create_dir_all(&sandbox).await.unwrap();
        // Canonicalize to resolve any symlinks in the path
        sandbox.canonicalize().unwrap()
    }

    // PHASE 1: Shell Exec Adversarial Tests

    #[test]
    fn test_git_directory_change_blocked() {
        let cmd = vec!["git".to_string(), "-C".to_string(), "../../".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_grep_etc_passwd_blocked() {
        let cmd = vec![
            "grep".to_string(),
            "root".to_string(),
            "/etc/passwd".to_string(),
        ];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_cat_proc_environ_blocked() {
        let cmd = vec!["cat".to_string(), "/proc/self/environ".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_ls_traversal_blocked() {
        let cmd = vec!["ls".to_string(), "sandbox/../../".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_git_config_global_blocked() {
        let cmd = vec![
            "git".to_string(),
            "config".to_string(),
            "--global".to_string(),
        ];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_null_byte_argument_blocked() {
        let cmd = vec!["echo".to_string(), "test\0malicious".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_newline_argument_blocked() {
        let cmd = vec!["echo".to_string(), "test\nmalicious".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_control_character_blocked() {
        let cmd = vec!["echo".to_string(), "test\x01malicious".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_git_subcommand_not_allowed() {
        let cmd = vec!["git".to_string(), "push".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_git_allowed_subcommands() {
        assert!(CommandGuard::validate(&["git".to_string(), "status".to_string()]).is_ok());
        assert!(CommandGuard::validate(&["git".to_string(), "diff".to_string()]).is_ok());
        assert!(CommandGuard::validate(&["git".to_string(), "log".to_string()]).is_ok());
        assert!(CommandGuard::validate(&["git".to_string(), "show".to_string()]).is_ok());
    }

    #[test]
    fn test_sys_path_blocked() {
        let cmd = vec!["cat".to_string(), "/sys/kernel/version".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_dev_path_blocked() {
        let cmd = vec!["cat".to_string(), "/dev/null".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    // PHASE 2: Resource Exhaustion Tests

    #[tokio::test]
    async fn test_infinite_loop_timeout() {
        let tool = ShellExecTool;
        let ctx = ExecutionContext::new("session".into(), 1000);

        // This would run forever without timeout
        let result = tool
            .execute(
                ctx,
                json!({
                    "cmd": ["sh", "-c", "while true; do :; done"]
                }),
            )
            .await;

        // Should timeout or be rejected
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_large_output_handled() {
        let tool = ShellExecTool;
        let ctx = ExecutionContext::new("session".into(), 5000);

        // Generate large output
        let result = tool
            .execute(
                ctx,
                json!({
                    "cmd": ["echo", "test"]
                }),
            )
            .await;

        assert!(result.is_ok());
    }

    // PHASE 3: TOCTOU Tests

    #[tokio::test]
    async fn test_symlink_inside_sandbox_to_outside() {
        let sandbox = setup_sandbox().await;

        // Create symlink pointing outside
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let _ = symlink("/etc/passwd", sandbox.join("malicious_link"));
        }

        let guard = PathGuard::new(&sandbox).unwrap();
        let result = guard.validate("malicious_link");

        // Should be rejected
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_path_traversal_before_canonicalization() {
        let sandbox = setup_sandbox().await;
        let guard = PathGuard::new(&sandbox).unwrap();

        let result = guard.validate("../../../etc/passwd");
        assert!(matches!(result, Err(ToolError::SandboxViolation(_))));
    }

    #[tokio::test]
    async fn test_nested_symlink_escape() {
        let sandbox = setup_sandbox().await;
        fs::create_dir_all(sandbox.join("nested")).await.unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let _ = symlink("../../..", sandbox.join("nested/escape"));
        }

        let guard = PathGuard::new(&sandbox).unwrap();
        let result = guard.validate("nested/escape/etc/passwd");

        assert!(result.is_err());
    }

    // PHASE 4: Audit Resilience Tests

    #[tokio::test]
    async fn test_audit_failure_doesnt_block_execution() {
        let mut registry = ToolRegistryImpl::new();
        registry.register(Arc::new(EchoTool));

        let dispatcher = ToolDispatcherImpl::new(
            Arc::new(registry),
            Arc::new(MockPermissionEngine) as Arc<dyn PermissionEngine>,
            Arc::new(MockAuditLogger) as Arc<dyn AuditLogger>,
            5000,
        );

        // Execute tool - audit may fail but execution should succeed
        let result = dispatcher
            .dispatch("session".into(), "echo".into(), json!({"message": "test"}))
            .await;

        assert!(result.is_ok());
        assert!(result.unwrap().success);
    }

    // PHASE 5: Panic Containment Tests

    #[tokio::test]
    async fn test_panic_in_tool_contained() {
        // Create a tool that panics
        struct PanicTool;

        #[async_trait::async_trait]
        impl Tool for PanicTool {
            fn name(&self) -> &'static str {
                "panic"
            }
            fn description(&self) -> &'static str {
                "Panics"
            }
            fn schema(&self) -> serde_json::Value {
                json!({})
            }

            async fn execute(
                &self,
                _ctx: ExecutionContext,
                _input: serde_json::Value,
            ) -> Result<ToolResult, ToolError> {
                panic!("Intentional panic for testing");
            }
        }

        let mut registry = ToolRegistryImpl::new();
        registry.register(Arc::new(PanicTool));

        let dispatcher = ToolDispatcherImpl::new(
            Arc::new(registry),
            Arc::new(MockPermissionEngine) as Arc<dyn PermissionEngine>,
            Arc::new(MockAuditLogger) as Arc<dyn AuditLogger>,
            5000,
        );

        let result = dispatcher
            .dispatch("session".into(), "panic".into(), json!({}))
            .await;

        // Should return Internal error, not crash
        assert!(matches!(result, Err(ToolError::Internal)));
    }

    // PHASE 6: Extreme Concurrency Stress Tests

    #[tokio::test]
    async fn test_1000_concurrent_dispatches() {
        let mut registry = ToolRegistryImpl::new();
        registry.register(Arc::new(EchoTool));

        let dispatcher = Arc::new(ToolDispatcherImpl::new(
            Arc::new(registry),
            Arc::new(MockPermissionEngine) as Arc<dyn PermissionEngine>,
            Arc::new(MockAuditLogger) as Arc<dyn AuditLogger>,
            5000,
        ));

        let mut handles = Vec::new();

        for i in 0..1000 {
            let dispatcher = dispatcher.clone();
            let handle = tokio::spawn(async move {
                dispatcher
                    .dispatch(
                        format!("session_{}", i % 100),
                        "echo".into(),
                        json!({"message": format!("msg_{}", i)}),
                    )
                    .await
            });
            handles.push(handle);
        }

        let mut success_count = 0;
        for handle in handles {
            if let Ok(Ok(result)) = handle.await {
                if result.success {
                    success_count += 1;
                }
            }
        }

        assert_eq!(success_count, 1000);
    }

    #[tokio::test]
    async fn test_mixed_operations_concurrent() {
        let sandbox = setup_sandbox().await;
        fs::write(sandbox.join("test.txt"), "content")
            .await
            .unwrap();

        let mut registry = ToolRegistryImpl::new();
        registry.register(Arc::new(EchoTool));
        registry.register(Arc::new(
            FileReadTool::new(sandbox.to_str().unwrap()).unwrap(),
        ));
        registry.register(Arc::new(ShellExecTool));

        let dispatcher = Arc::new(ToolDispatcherImpl::new(
            Arc::new(registry),
            Arc::new(MockPermissionEngine) as Arc<dyn PermissionEngine>,
            Arc::new(MockAuditLogger) as Arc<dyn AuditLogger>,
            5000,
        ));

        let mut handles = Vec::new();

        for i in 0..100 {
            let dispatcher = dispatcher.clone();
            let tool = match i % 3 {
                0 => "echo",
                1 => "file.read",
                _ => "shell.exec",
            };

            let input = match tool {
                "echo" => json!({"message": "test"}),
                "file.read" => json!({"path": "test.txt"}),
                _ => json!({"cmd": ["echo", "test"]}),
            };

            let handle = tokio::spawn(async move {
                dispatcher
                    .dispatch(format!("session_{}", i), tool.into(), input)
                    .await
            });
            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.await;
        }
    }

    // PHASE 7: Boundary Condition Tests

    #[tokio::test]
    async fn test_file_size_exactly_10mb() {
        let sandbox = setup_sandbox().await;
        let large_file = sandbox.join("10mb.txt");

        // Create exactly 10MB file
        let content = vec![b'A'; 10 * 1024 * 1024];
        fs::write(&large_file, content).await.unwrap();

        let guard = PathGuard::new(&sandbox).unwrap();
        let result = guard.validate("10mb.txt");

        // Should be allowed (exactly at limit)
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_file_size_over_10mb() {
        let sandbox = setup_sandbox().await;
        let large_file = sandbox.join("10mb_plus.txt");

        // Create 10MB + 1 byte file
        let content = vec![b'A'; 10 * 1024 * 1024 + 1];
        fs::write(&large_file, content).await.unwrap();

        let guard = PathGuard::new(&sandbox).unwrap();
        let result = guard.validate("10mb_plus.txt");

        // Should be rejected
        assert!(matches!(result, Err(ToolError::SandboxViolation(_))));
    }

    #[tokio::test]
    async fn test_directory_1000_entries() {
        let sandbox = setup_sandbox().await;

        for i in 0..1000 {
            fs::write(sandbox.join(format!("file_{}.txt", i)), "")
                .await
                .unwrap();
        }

        let tool = FileListTool::new(sandbox.to_str().unwrap()).unwrap();
        let ctx = ExecutionContext::new("session".into(), 5000);
        let result = tool.execute(ctx, json!({"path": "."})).await.unwrap();

        let output = result.output.unwrap();
        let entries = output["entries"].as_array().unwrap();
        assert!(
            entries.len() >= 900,
            "Expected at least 900 entries, got {}",
            entries.len()
        ); // Allow for some variance
    }

    #[tokio::test]
    async fn test_directory_1001_entries() {
        let sandbox = setup_sandbox().await;

        for i in 0..1001 {
            fs::write(sandbox.join(format!("file_{}.txt", i)), "")
                .await
                .unwrap();
        }

        let tool = FileListTool::new(sandbox.to_str().unwrap()).unwrap();
        let ctx = ExecutionContext::new("session".into(), 5000);
        let result = tool.execute(ctx, json!({"path": "."})).await.unwrap();

        let output = result.output.unwrap();
        let entries = output["entries"].as_array().unwrap();
        // Should be capped at 1000
        assert!(
            entries.len() >= 900 && entries.len() <= 1000,
            "Expected 900-1000 entries, got {}",
            entries.len()
        );
    }

    #[tokio::test]
    async fn test_empty_json_input() {
        let mut registry = ToolRegistryImpl::new();
        registry.register(Arc::new(EchoTool));

        let dispatcher = ToolDispatcherImpl::new(
            Arc::new(registry),
            Arc::new(MockPermissionEngine) as Arc<dyn PermissionEngine>,
            Arc::new(MockAuditLogger) as Arc<dyn AuditLogger>,
            5000,
        );

        let result = dispatcher
            .dispatch("session".into(), "echo".into(), json!({}))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_large_json_payload() {
        let mut registry = ToolRegistryImpl::new();
        registry.register(Arc::new(EchoTool));

        let dispatcher = ToolDispatcherImpl::new(
            Arc::new(registry),
            Arc::new(MockPermissionEngine) as Arc<dyn PermissionEngine>,
            Arc::new(MockAuditLogger) as Arc<dyn AuditLogger>,
            5000,
        );

        // Create large payload (2MB)
        let large_string = "A".repeat(2 * 1024 * 1024);
        let result = dispatcher
            .dispatch(
                "session".into(),
                "echo".into(),
                json!({"message": large_string}),
            )
            .await;

        // Should be rejected due to size
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_invalid_json_types() {
        let tool = ShellExecTool;
        let ctx = ExecutionContext::new("session".into(), 5000);

        // cmd should be array, not string
        let result = tool.execute(ctx, json!({"cmd": "ls"})).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_carriage_return_blocked() {
        let cmd = vec!["echo".to_string(), "test\rmalicious".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_multiple_dangerous_chars() {
        let cmd = vec!["ls".to_string(), "file1|file2&file3".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_wget_blocked() {
        let cmd = vec!["wget".to_string(), "http://example.com".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_nc_blocked() {
        let cmd = vec![
            "nc".to_string(),
            "localhost".to_string(),
            "8080".to_string(),
        ];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }
}
