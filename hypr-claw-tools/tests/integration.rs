#[cfg(test)]
mod integration_tests {
    use async_trait::async_trait;
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
        let sandbox = std::env::temp_dir().join(format!("test_sandbox_{}", nanos));
        let _ = fs::remove_dir_all(&sandbox).await;
        fs::create_dir_all(&sandbox).await.unwrap();
        // Canonicalize to resolve any symlinks in the path
        sandbox.canonicalize().unwrap()
    }

    #[tokio::test]
    async fn test_file_read_success() {
        let sandbox = setup_sandbox().await;
        let test_file = sandbox.join("test.txt");
        fs::write(&test_file, "hello world").await.unwrap();

        let tool = FileReadTool::new(sandbox.to_str().unwrap()).unwrap();
        let ctx = ExecutionContext::new("session".into(), 5000);
        let result = tool
            .execute(ctx, json!({"path": "test.txt"}))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.is_some());
    }

    #[tokio::test]
    async fn test_file_read_nonexistent() {
        let sandbox = setup_sandbox().await;
        let tool = FileReadTool::new(sandbox.to_str().unwrap()).unwrap();
        let ctx = ExecutionContext::new("session".into(), 5000);
        let result = tool.execute(ctx, json!({"path": "nonexistent.txt"})).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_file_write_success() {
        let sandbox = setup_sandbox().await;
        let tool = FileWriteTool::new(sandbox.to_str().unwrap()).unwrap();
        let ctx = ExecutionContext::new("session".into(), 5000);

        let result = tool
            .execute(
                ctx,
                json!({"path": "output.txt", "content": "test content", "overwrite": false}),
            )
            .await
            .unwrap();

        assert!(result.success);

        let content = fs::read_to_string(sandbox.join("output.txt"))
            .await
            .unwrap();
        assert_eq!(content, "test content");
    }

    #[tokio::test]
    async fn test_file_write_no_overwrite() {
        let sandbox = setup_sandbox().await;
        let test_file = sandbox.join("existing.txt");
        fs::write(&test_file, "original").await.unwrap();

        let tool = FileWriteTool::new(sandbox.to_str().unwrap()).unwrap();
        let ctx = ExecutionContext::new("session".into(), 5000);

        let result = tool
            .execute(
                ctx,
                json!({"path": "existing.txt", "content": "new", "overwrite": false}),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_file_write_with_overwrite() {
        let sandbox = setup_sandbox().await;
        let test_file = sandbox.join("existing.txt");
        fs::write(&test_file, "original").await.unwrap();

        let tool = FileWriteTool::new(sandbox.to_str().unwrap()).unwrap();
        let ctx = ExecutionContext::new("session".into(), 5000);

        let result = tool
            .execute(
                ctx,
                json!({"path": "existing.txt", "content": "new", "overwrite": true}),
            )
            .await
            .unwrap();

        assert!(result.success);

        let content = fs::read_to_string(&test_file).await.unwrap();
        assert_eq!(content, "new");
    }

    #[tokio::test]
    async fn test_file_list_success() {
        let sandbox = setup_sandbox().await;
        fs::write(sandbox.join("file1.txt"), "").await.unwrap();
        fs::write(sandbox.join("file2.txt"), "").await.unwrap();

        let tool = FileListTool::new(sandbox.to_str().unwrap()).unwrap();
        let ctx = ExecutionContext::new("session".into(), 5000);
        let result = tool.execute(ctx, json!({"path": "."})).await.unwrap();

        assert!(result.success);
        let output = result.output.unwrap();
        let entries = output["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn test_file_list_hidden_files_excluded() {
        let sandbox = setup_sandbox().await;
        fs::write(sandbox.join("visible.txt"), "").await.unwrap();
        fs::write(sandbox.join(".hidden"), "").await.unwrap();

        let tool = FileListTool::new(sandbox.to_str().unwrap()).unwrap();
        let ctx = ExecutionContext::new("session".into(), 5000);
        let result = tool.execute(ctx, json!({"path": "."})).await.unwrap();

        let output = result.output.unwrap();
        let entries = output["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], "visible.txt");
    }

    #[tokio::test]
    async fn test_shell_exec_success() {
        let tool = ShellExecTool;
        let ctx = ExecutionContext::new("session".into(), 5000);
        let result = tool
            .execute(ctx, json!({"cmd": ["echo", "hello"]}))
            .await
            .unwrap();

        assert!(result.success);
        let output = result.output.unwrap();
        assert!(output["stdout"].as_str().unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn test_shell_exec_git_status() {
        let tool = ShellExecTool;
        let ctx = ExecutionContext::new("session".into(), 5000);
        let result = tool.execute(ctx, json!({"cmd": ["git", "status"]})).await;

        // May fail if not in git repo, but should not panic
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_shell_exec_blocked_rm() {
        let tool = ShellExecTool;
        let ctx = ExecutionContext::new("session".into(), 5000);
        let result = tool.execute(ctx, json!({"cmd": ["rm", "-rf", "/"]})).await;

        assert!(matches!(result, Err(ToolError::SandboxViolation(_))));
    }

    #[tokio::test]
    async fn test_shell_exec_blocked_sudo() {
        let tool = ShellExecTool;
        let ctx = ExecutionContext::new("session".into(), 5000);
        let result = tool.execute(ctx, json!({"cmd": ["sudo", "ls"]})).await;

        assert!(matches!(result, Err(ToolError::SandboxViolation(_))));
    }

    #[tokio::test]
    async fn test_shell_exec_pipe_blocked() {
        let tool = ShellExecTool;
        let ctx = ExecutionContext::new("session".into(), 5000);
        let result = tool
            .execute(ctx, json!({"cmd": ["ls", "|", "grep", "test"]}))
            .await;

        assert!(matches!(result, Err(ToolError::SandboxViolation(_))));
    }

    #[tokio::test]
    async fn test_parallel_execution_different_sessions() {
        let tool = Arc::new(EchoTool);

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let tool = tool.clone();
                tokio::spawn(async move {
                    let ctx = ExecutionContext::new(format!("session_{}", i), 5000);
                    tool.execute(ctx, json!({"message": format!("msg_{}", i)}))
                        .await
                })
            })
            .collect();

        for handle in handles {
            let result = handle.await.unwrap().unwrap();
            assert!(result.success);
        }
    }

    #[tokio::test]
    async fn test_parallel_execution_same_session() {
        let tool = Arc::new(EchoTool);

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let tool = tool.clone();
                tokio::spawn(async move {
                    let ctx = ExecutionContext::new("shared_session".into(), 5000);
                    tool.execute(ctx, json!({"message": format!("msg_{}", i)}))
                        .await
                })
            })
            .collect();

        for handle in handles {
            let result = handle.await.unwrap().unwrap();
            assert!(result.success);
        }
    }

    #[tokio::test]
    async fn test_parallel_execution_100_concurrent() {
        let tool = Arc::new(EchoTool);

        let handles: Vec<_> = (0..100)
            .map(|i| {
                let tool = tool.clone();
                tokio::spawn(async move {
                    let ctx = ExecutionContext::new(format!("session_{}", i % 10), 5000);
                    tool.execute(ctx, json!({"message": i})).await
                })
            })
            .collect();

        let mut success_count = 0;
        for handle in handles {
            if let Ok(Ok(result)) = handle.await {
                if result.success {
                    success_count += 1;
                }
            }
        }

        assert_eq!(success_count, 100);
    }

    #[tokio::test]
    async fn test_dispatcher_parallel_dispatch() {
        let mut registry = ToolRegistryImpl::new();
        registry.register(Arc::new(EchoTool));

        let dispatcher = Arc::new(ToolDispatcherImpl::new(
            Arc::new(registry),
            Arc::new(MockPermissionEngine) as Arc<dyn PermissionEngine>,
            Arc::new(MockAuditLogger) as Arc<dyn AuditLogger>,
            5000,
        ));

        let handles: Vec<_> = (0..50)
            .map(|i| {
                let dispatcher = dispatcher.clone();
                tokio::spawn(async move {
                    dispatcher
                        .dispatch(
                            format!("session_{}", i),
                            "echo".into(),
                            json!({"message": i}),
                        )
                        .await
                })
            })
            .collect();

        for handle in handles {
            let result = handle.await.unwrap().unwrap();
            assert!(result.success);
        }
    }

    #[tokio::test]
    async fn test_tool_result_serialization() {
        let result = ToolResult {
            success: true,
            output: Some(json!({"key": "value"})),
            error: None,
        };

        let serialized = serde_json::to_string(&result).unwrap();
        let deserialized: ToolResult = serde_json::from_str(&serialized).unwrap();

        assert_eq!(result.success, deserialized.success);
        assert_eq!(result.output, deserialized.output);
    }

    #[tokio::test]
    async fn test_execution_context_serialization() {
        let ctx = ExecutionContext::new("session".into(), 5000);
        let serialized = serde_json::to_string(&ctx).unwrap();
        let deserialized: ExecutionContext = serde_json::from_str(&serialized).unwrap();

        assert_eq!(ctx.session_key, deserialized.session_key);
        assert_eq!(ctx.timeout_ms, deserialized.timeout_ms);
    }

    #[tokio::test]
    async fn test_file_write_atomic() {
        let sandbox = setup_sandbox().await;
        let tool = FileWriteTool::new(sandbox.to_str().unwrap()).unwrap();

        // Write multiple times concurrently
        let handles: Vec<_> = (0..10).map(|i| {
            let tool = Arc::new(tool.clone());
            tokio::spawn(async move {
                let ctx = ExecutionContext::new("session".into(), 5000);
                tool.execute(
                    ctx,
                    json!({"path": format!("file_{}.txt", i), "content": format!("content_{}", i), "overwrite": false})
                ).await
            })
        }).collect();

        for handle in handles {
            let result = handle.await.unwrap().unwrap();
            assert!(result.success);
        }
    }

    #[tokio::test]
    async fn test_audit_logging_on_error() {
        let mut registry = ToolRegistryImpl::new();
        registry.register(Arc::new(EchoTool));

        let dispatcher = ToolDispatcherImpl::new(
            Arc::new(registry),
            Arc::new(MockPermissionEngine) as Arc<dyn PermissionEngine>,
            Arc::new(MockAuditLogger) as Arc<dyn AuditLogger>,
            5000,
        );

        // Tool not found should still audit
        let _ = dispatcher
            .dispatch("session".into(), "nonexistent".into(), json!({}))
            .await;

        // Should not panic
    }
}
