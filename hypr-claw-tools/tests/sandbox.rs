#[cfg(test)]
mod sandbox_tests {
    use hypr_claw_tools::error::ToolError;
    use hypr_claw_tools::sandbox::*;
    use std::path::PathBuf;
    use tokio::fs;

    async fn setup_sandbox() -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sandbox = std::env::temp_dir().join(format!("test_sandbox_security_{}", nanos));
        let _ = fs::remove_dir_all(&sandbox).await;
        fs::create_dir_all(&sandbox).await.unwrap();
        // Canonicalize to resolve any symlinks in the path
        sandbox.canonicalize().unwrap()
    }

    #[tokio::test]
    async fn test_path_traversal_dotdot() {
        let sandbox = setup_sandbox().await;
        let guard = PathGuard::new(&sandbox).unwrap();
        let result = guard.validate("../../../etc/passwd");
        assert!(matches!(result, Err(ToolError::SandboxViolation(_))));
    }

    #[tokio::test]
    async fn test_path_traversal_encoded() {
        let sandbox = setup_sandbox().await;
        let guard = PathGuard::new(&sandbox).unwrap();
        let result = guard.validate("..%2F..%2Fetc%2Fpasswd");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_path_absolute_unix() {
        let sandbox = setup_sandbox().await;
        let guard = PathGuard::new(&sandbox).unwrap();
        let result = guard.validate("/etc/passwd");
        assert!(matches!(result, Err(ToolError::SandboxViolation(_))));
    }

    #[tokio::test]
    async fn test_path_valid_relative() {
        let sandbox = setup_sandbox().await;
        fs::write(sandbox.join("valid.txt"), "content")
            .await
            .unwrap();
        let guard = PathGuard::new(&sandbox).unwrap();
        let result = guard.validate("valid.txt");
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_path_valid_subdirectory() {
        let sandbox = setup_sandbox().await;
        fs::create_dir_all(sandbox.join("subdir")).await.unwrap();
        fs::write(sandbox.join("subdir/file.txt"), "content")
            .await
            .unwrap();
        let guard = PathGuard::new(&sandbox).unwrap();
        let result = guard.validate("subdir/file.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn test_command_guard_semicolon() {
        let cmd = vec!["ls".to_string(), ";".to_string(), "rm".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_command_guard_ampersand() {
        let cmd = vec!["ls".to_string(), "&".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_command_guard_redirect_output() {
        let cmd = vec!["ls".to_string(), ">".to_string(), "file.txt".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_command_guard_redirect_input() {
        let cmd = vec!["cat".to_string(), "<".to_string(), "file.txt".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_command_guard_backtick() {
        let cmd = vec!["echo".to_string(), "`whoami`".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_command_guard_dollar_paren() {
        let cmd = vec!["echo".to_string(), "$(whoami)".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_command_guard_chmod() {
        let cmd = vec!["chmod".to_string(), "777".to_string(), "file".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_command_guard_ls_with_args() {
        let cmd = vec!["ls".to_string(), "-la".to_string(), "/tmp".to_string()];
        assert!(CommandGuard::validate(&cmd).is_ok());
    }

    #[test]
    fn test_command_guard_pwd() {
        let cmd = vec!["pwd".to_string()];
        assert!(CommandGuard::validate(&cmd).is_ok());
    }

    #[test]
    fn test_command_guard_cat_file() {
        let cmd = vec!["cat".to_string(), "file.txt".to_string()];
        assert!(CommandGuard::validate(&cmd).is_ok());
    }

    #[test]
    fn test_command_guard_grep_pattern() {
        let cmd = vec![
            "grep".to_string(),
            "pattern".to_string(),
            "file.txt".to_string(),
        ];
        assert!(CommandGuard::validate(&cmd).is_ok());
    }

    #[test]
    fn test_command_guard_git_diff() {
        let cmd = vec!["git".to_string(), "diff".to_string()];
        assert!(CommandGuard::validate(&cmd).is_ok());
    }

    #[test]
    fn test_command_guard_case_sensitive() {
        let cmd = vec!["LS".to_string()];
        assert!(matches!(
            CommandGuard::validate(&cmd),
            Err(ToolError::SandboxViolation(_))
        ));
    }

    #[test]
    fn test_command_guard_path_with_ls() {
        let cmd = vec!["/usr/bin/ls".to_string()];
        assert!(CommandGuard::validate(&cmd).is_ok());
    }

    #[tokio::test]
    async fn test_multiple_path_validations() {
        let sandbox = setup_sandbox().await;
        fs::write(sandbox.join("file1.txt"), "").await.unwrap();
        fs::write(sandbox.join("file2.txt"), "").await.unwrap();

        let guard = PathGuard::new(&sandbox).unwrap();

        assert!(guard.validate("file1.txt").is_ok());
        assert!(guard.validate("file2.txt").is_ok());
        assert!(guard.validate("nonexistent.txt").is_err());
    }

    #[test]
    fn test_command_validation_batch() {
        let valid_commands = vec![
            vec!["ls".to_string()],
            vec!["pwd".to_string()],
            vec!["echo".to_string(), "test".to_string()],
            vec!["git".to_string(), "status".to_string()],
            vec!["cat".to_string(), "file.txt".to_string()],
            vec!["grep".to_string(), "pattern".to_string()],
        ];

        for cmd in valid_commands {
            assert!(
                CommandGuard::validate(&cmd).is_ok(),
                "Failed for: {:?}",
                cmd
            );
        }
    }

    #[test]
    fn test_command_validation_batch_invalid() {
        let invalid_commands = vec![
            vec!["rm".to_string()],
            vec!["sudo".to_string(), "ls".to_string()],
            vec!["chmod".to_string(), "777".to_string()],
            vec!["ls".to_string(), "|".to_string()],
            vec!["echo".to_string(), ";".to_string()],
        ];

        for cmd in invalid_commands {
            assert!(
                CommandGuard::validate(&cmd).is_err(),
                "Should fail for: {:?}",
                cmd
            );
        }
    }

    #[tokio::test]
    async fn test_path_guard_new_file_validation() {
        let sandbox = setup_sandbox().await;
        let guard = PathGuard::new(&sandbox).unwrap();

        let result = guard.validate_new("new_file.txt");
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_path_guard_new_file_traversal() {
        let sandbox = setup_sandbox().await;
        let guard = PathGuard::new(&sandbox).unwrap();

        let result = guard.validate_new("../outside.txt");
        assert!(matches!(result, Err(ToolError::SandboxViolation(_))));
    }

    #[tokio::test]
    async fn test_path_guard_nested_directory() {
        let sandbox = setup_sandbox().await;
        fs::create_dir_all(sandbox.join("a/b/c")).await.unwrap();
        fs::write(sandbox.join("a/b/c/deep.txt"), "").await.unwrap();

        let guard = PathGuard::new(&sandbox).unwrap();
        let result = guard.validate("a/b/c/deep.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn test_error_types_coverage() {
        let errors = vec![
            ToolError::ValidationError("test".into()),
            ToolError::PermissionDenied("test".into()),
            ToolError::ExecutionFailed("test".into()),
            ToolError::Timeout,
            ToolError::SandboxViolation("test".into()),
            ToolError::Internal,
        ];

        for err in errors {
            assert!(!err.to_string().is_empty());
        }
    }
}
