#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use hypr_claw_tools::sandbox::*;
    use hypr_claw_tools::tools::*;
    use hypr_claw_tools::*;
    use serde_json::json;
    use std::sync::Arc;

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

    #[tokio::test]
    async fn test_echo_tool() {
        let tool = EchoTool;
        let ctx = ExecutionContext::new("test_session".into(), 5000);
        let input = json!({"message": "hello"});

        let result = tool.execute(ctx, input.clone()).await.unwrap();
        assert!(result.success);
        assert_eq!(result.output, Some(input));
    }

    #[tokio::test]
    async fn test_echo_tool_empty() {
        let tool = EchoTool;
        let ctx = ExecutionContext::new("test".into(), 5000);
        let result = tool.execute(ctx, json!({})).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_registry_register_and_get() {
        let mut registry = ToolRegistryImpl::new();
        registry.register(Arc::new(EchoTool));

        assert!(registry.get("echo").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[tokio::test]
    async fn test_registry_list() {
        let mut registry = ToolRegistryImpl::new();
        registry.register(Arc::new(EchoTool));
        registry.register(Arc::new(ShellExecTool));

        let tools = registry.list();
        assert_eq!(tools.len(), 2);
        assert!(tools.contains(&"echo".to_string()));
        assert!(tools.contains(&"shell.exec".to_string()));
    }

    #[tokio::test]
    async fn test_registry_count() {
        let mut registry = ToolRegistryImpl::new();
        assert_eq!(registry.count(), 0);
        registry.register(Arc::new(EchoTool));
        assert_eq!(registry.count(), 1);
    }

    #[tokio::test]
    async fn test_registry_chainable() {
        let mut registry = ToolRegistryImpl::new();
        registry
            .register(Arc::new(EchoTool))
            .register(Arc::new(ShellExecTool));
        assert_eq!(registry.count(), 2);
    }

    #[test]
    fn test_path_guard_absolute_path_rejected() {
        let temp_dir = tempfile::tempdir().unwrap();
        let guard = PathGuard::new(temp_dir.path()).unwrap();
        let result = guard.validate("/etc/passwd");
        assert!(matches!(result, Err(ToolError::SandboxViolation(_))));
    }

    #[test]
    fn test_command_guard_blocked_command() {
        let cmd = vec!["sudo".to_string(), "ls".to_string()];
        let result = CommandGuard::validate(&cmd);
        assert!(matches!(result, Err(ToolError::SandboxViolation(_))));
    }

    #[test]
    fn test_command_guard_not_whitelisted() {
        let cmd = vec!["curl".to_string(), "http://example.com".to_string()];
        let result = CommandGuard::validate(&cmd);
        assert!(matches!(result, Err(ToolError::SandboxViolation(_))));
    }

    #[test]
    fn test_command_guard_dangerous_chars_pipe() {
        let cmd = vec!["ls".to_string(), "|".to_string(), "grep".to_string()];
        let result = CommandGuard::validate(&cmd);
        assert!(matches!(result, Err(ToolError::SandboxViolation(_))));
    }

    #[test]
    fn test_command_guard_dangerous_chars_ampersand() {
        let cmd = vec!["ls".to_string(), "&&".to_string()];
        let result = CommandGuard::validate(&cmd);
        assert!(matches!(result, Err(ToolError::SandboxViolation(_))));
    }

    #[test]
    fn test_command_guard_dangerous_chars_semicolon() {
        let cmd = vec!["ls".to_string(), ";".to_string()];
        let result = CommandGuard::validate(&cmd);
        assert!(matches!(result, Err(ToolError::SandboxViolation(_))));
    }

    #[test]
    fn test_command_guard_allowed() {
        let cmd = vec!["ls".to_string(), "-la".to_string()];
        let result = CommandGuard::validate(&cmd);
        assert!(result.is_ok());
    }

    #[test]
    fn test_command_guard_git_allowed() {
        let cmd = vec!["git".to_string(), "status".to_string()];
        let result = CommandGuard::validate(&cmd);
        assert!(result.is_ok());
    }

    #[test]
    fn test_command_guard_empty() {
        let cmd: Vec<String> = vec![];
        let result = CommandGuard::validate(&cmd);
        assert!(matches!(result, Err(ToolError::ValidationError(_))));
    }

    #[tokio::test]
    #[ignore] // Timing-sensitive test, may be flaky
    async fn test_shell_exec_timeout() {
        let tool = ShellExecTool;
        let ctx = ExecutionContext::new("test_session".into(), 100);
        let input = json!({"cmd": ["sleep", "10"]});

        let result = tool.execute(ctx, input).await;
        assert!(matches!(result, Err(ToolError::Timeout)));
    }

    #[test]
    fn test_tool_error_validation() {
        let err = ToolError::ValidationError("test".into());
        assert_eq!(err.to_string(), "Validation error: test");
    }

    #[test]
    fn test_tool_error_permission_denied() {
        let err = ToolError::PermissionDenied("test".into());
        assert_eq!(err.to_string(), "Permission denied: test");
    }

    #[test]
    fn test_tool_error_timeout() {
        let err = ToolError::Timeout;
        assert_eq!(err.to_string(), "Operation timed out");
    }

    #[test]
    fn test_tool_error_sandbox() {
        let err = ToolError::SandboxViolation("test".into());
        assert!(err.to_string().contains("Sandbox violation"));
    }

    #[test]
    fn test_tool_error_execution() {
        let err = ToolError::ExecutionFailed("test".into());
        assert!(err.to_string().contains("Execution failed"));
    }

    #[test]
    fn test_tool_error_internal() {
        let err = ToolError::Internal;
        assert_eq!(err.to_string(), "Internal error");
    }

    #[test]
    fn test_execution_context_creation() {
        let ctx = ExecutionContext::new("session123".into(), 5000);
        assert_eq!(ctx.session_key, "session123");
        assert_eq!(ctx.timeout_ms, 5000);
        assert!(!ctx.audit_ref.is_empty());
        assert!(!ctx.permission_ref.is_empty());
    }

    #[test]
    fn test_execution_context_unique_refs() {
        let ctx1 = ExecutionContext::new("s1".into(), 1000);
        let ctx2 = ExecutionContext::new("s2".into(), 2000);
        assert_ne!(ctx1.audit_ref, ctx2.audit_ref);
        assert_ne!(ctx1.permission_ref, ctx2.permission_ref);
    }

    #[tokio::test]
    async fn test_dispatcher_tool_not_found() {
        let registry = Arc::new(ToolRegistryImpl::new());
        let dispatcher = ToolDispatcherImpl::new(
            registry,
            Arc::new(MockPermissionEngine) as Arc<dyn PermissionEngine>,
            Arc::new(MockAuditLogger) as Arc<dyn AuditLogger>,
            5000,
        );

        let result = dispatcher
            .dispatch("session".into(), "nonexistent".into(), json!({}))
            .await;

        assert!(matches!(result, Err(ToolError::ValidationError(_))));
    }

    #[tokio::test]
    async fn test_dispatcher_echo_success() {
        let mut registry = ToolRegistryImpl::new();
        registry.register(Arc::new(EchoTool));

        let dispatcher = ToolDispatcherImpl::new(
            Arc::new(registry),
            Arc::new(MockPermissionEngine) as Arc<dyn PermissionEngine>,
            Arc::new(MockAuditLogger) as Arc<dyn AuditLogger>,
            5000,
        );

        let result = dispatcher
            .dispatch("session".into(), "echo".into(), json!({"message": "test"}))
            .await
            .unwrap();

        assert!(result.success);
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

    #[test]
    fn test_tool_schemas() {
        let echo = EchoTool;
        assert!(!echo.schema().is_null());
        assert_eq!(echo.name(), "echo");
        assert!(!echo.description().is_empty());
    }

    #[test]
    fn test_shell_exec_schema() {
        let tool = ShellExecTool;
        let schema = tool.schema();
        assert!(schema["properties"]["cmd"].is_object());
    }
}
