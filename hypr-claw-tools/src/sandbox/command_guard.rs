use crate::error::ToolError;

const WHITELIST: &[&str] = &["ls", "pwd", "cat", "grep", "echo", "git"];
const BLACKLIST: &[&str] = &["sudo", "rm", "chmod", "curl", "wget", "nc", "netcat"];
const DANGEROUS_CHARS: &[char] = &['|', '&', ';', '>', '<', '`', '$', '\n', '\r', '\0'];
const GIT_ALLOWED_SUBCOMMANDS: &[&str] = &["status", "diff", "log", "show"];
const SENSITIVE_PATHS: &[&str] = &["/etc/", "/proc/", "/sys/", "/dev/"];

pub struct CommandGuard;

impl CommandGuard {
    pub fn validate(cmd: &[String]) -> Result<(), ToolError> {
        if cmd.is_empty() {
            return Err(ToolError::ValidationError("Empty command".into()));
        }

        let program = &cmd[0];

        // Check blacklist first
        if BLACKLIST.iter().any(|&b| program.contains(b)) {
            return Err(ToolError::SandboxViolation(format!(
                "Blocked command: {}",
                program
            )));
        }

        // Extract base command
        let base_cmd = program.split('/').next_back().unwrap_or(program);

        // Whitelist check
        if !WHITELIST.contains(&base_cmd) {
            return Err(ToolError::SandboxViolation(format!(
                "Command not whitelisted: {}",
                base_cmd
            )));
        }

        // Validate all arguments
        for (idx, arg) in cmd.iter().enumerate() {
            Self::validate_argument(arg, idx == 0)?;
        }

        // Special validation for git
        if base_cmd == "git" && cmd.len() > 1 {
            Self::validate_git_command(&cmd[1..])?;
        }

        Ok(())
    }

    fn validate_argument(arg: &str, is_program: bool) -> Result<(), ToolError> {
        // Check for dangerous characters
        for &ch in DANGEROUS_CHARS {
            if arg.contains(ch) {
                return Err(ToolError::SandboxViolation(format!(
                    "Dangerous character in argument: {:?}",
                    ch
                )));
            }
        }

        // Check for control characters
        if arg.chars().any(|c| c.is_control() && c != '\t') {
            return Err(ToolError::SandboxViolation(
                "Control character in argument".into(),
            ));
        }

        // Skip further checks for program name
        if is_program {
            return Ok(());
        }

        // Check for path traversal
        if arg.contains("..") {
            return Err(ToolError::SandboxViolation(
                "Path traversal in argument".into(),
            ));
        }

        // Check for sensitive paths
        for &sensitive in SENSITIVE_PATHS {
            if arg.starts_with(sensitive) {
                return Err(ToolError::SandboxViolation(format!(
                    "Access to sensitive path: {}",
                    sensitive
                )));
            }
        }

        // Check for absolute paths outside sandbox
        if arg.starts_with('/') && !arg.starts_with("/tmp") {
            return Err(ToolError::SandboxViolation(
                "Absolute path not allowed".into(),
            ));
        }

        // Check for git config manipulation
        if arg.starts_with("--global") || arg.starts_with("--system") {
            return Err(ToolError::SandboxViolation(
                "Global/system config not allowed".into(),
            ));
        }

        // Check for -C flag (change directory)
        if arg == "-C" {
            return Err(ToolError::SandboxViolation(
                "Directory change not allowed".into(),
            ));
        }

        Ok(())
    }

    fn validate_git_command(args: &[String]) -> Result<(), ToolError> {
        if args.is_empty() {
            return Ok(());
        }

        let subcommand = &args[0];

        // Block config commands
        if subcommand == "config" {
            return Err(ToolError::SandboxViolation("git config not allowed".into()));
        }

        // Check if subcommand is in allowed list
        if !GIT_ALLOWED_SUBCOMMANDS
            .iter()
            .any(|&allowed| subcommand == allowed)
        {
            return Err(ToolError::SandboxViolation(format!(
                "git subcommand not allowed: {}",
                subcommand
            )));
        }

        Ok(())
    }
}
