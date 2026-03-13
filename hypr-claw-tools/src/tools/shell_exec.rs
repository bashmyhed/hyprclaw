use crate::error::ToolError;
use crate::execution_context::ExecutionContext;
use crate::sandbox::CommandGuard;
use crate::tools::base::{Tool, ToolResult};
use crate::traits::PermissionTier;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

#[derive(Deserialize)]
struct ShellExecInput {
    cmd: Vec<String>,
}

pub struct ShellExecTool;

#[async_trait]
impl Tool for ShellExecTool {
    fn name(&self) -> &'static str {
        "shell.exec"
    }

    fn description(&self) -> &'static str {
        "Executes shell command"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "cmd": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            },
            "required": ["cmd"]
        })
    }

    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::SystemCritical
    }

    async fn execute(
        &self,
        ctx: ExecutionContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let input: ShellExecInput =
            serde_json::from_value(input).map_err(|e| ToolError::ValidationError(e.to_string()))?;

        if input.cmd.is_empty() {
            return Err(ToolError::ValidationError("Empty command".into()));
        }

        // Validate command and all arguments
        CommandGuard::validate(&input.cmd)?;

        let program = &input.cmd[0];
        let args = &input.cmd[1..];

        let exec_future = async {
            let mut cmd = Command::new(program);
            cmd.args(args)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);

            // Set process group for proper cleanup
            #[cfg(unix)]
            {
                unsafe {
                    cmd.pre_exec(|| {
                        // Create new process group
                        libc::setsid();
                        Ok(())
                    });
                }
            }

            cmd.output().await
        };

        let output = timeout(Duration::from_millis(ctx.timeout_ms), exec_future)
            .await
            .map_err(|_| ToolError::Timeout)?
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(ToolResult {
            success: output.status.success(),
            output: Some(json!({
                "stdout": stdout,
                "stderr": stderr,
                "exit_code": output.status.code()
            })),
            error: if output.status.success() {
                None
            } else {
                Some(stderr)
            },
        })
    }
}
