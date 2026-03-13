use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error("Command not whitelisted: {0}")]
    NotWhitelisted(String),
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Timeout")]
    Timeout,
}

pub struct CommandExecutor {
    whitelist: Vec<String>,
}

impl CommandExecutor {
    pub fn new(whitelist: Vec<String>) -> Self {
        Self { whitelist }
    }

    pub fn default_whitelist() -> Vec<String> {
        vec![
            "ls".to_string(),
            "cat".to_string(),
            "echo".to_string(),
            "pwd".to_string(),
            "date".to_string(),
            "whoami".to_string(),
        ]
    }

    pub async fn execute(&self, command: &str, args: &[String]) -> Result<String, ExecutorError> {
        if !self.is_whitelisted(command) {
            return Err(ExecutorError::NotWhitelisted(command.to_string()));
        }

        tracing::info!("Executing command: {} {:?}", command, args);

        let output = tokio::process::Command::new(command)
            .args(args)
            .output()
            .await
            .map_err(|e| ExecutorError::ExecutionFailed(e.to_string()))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(ExecutorError::ExecutionFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ))
        }
    }

    fn is_whitelisted(&self, command: &str) -> bool {
        self.whitelist.iter().any(|w| w == command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_whitelisted_command() {
        let executor = CommandExecutor::new(CommandExecutor::default_whitelist());
        let result = executor.execute("echo", &["hello".to_string()]).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_non_whitelisted_command() {
        let executor = CommandExecutor::new(CommandExecutor::default_whitelist());
        let result = executor.execute("rm", &["-rf".to_string()]).await;
        assert!(matches!(result, Err(ExecutorError::NotWhitelisted(_))));
    }
}
