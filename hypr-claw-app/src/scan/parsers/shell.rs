use super::{ConfigParser, ConfigType, ParseError, ParsedConfig, ShellType};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

/// Shell config parser - handles bash, zsh, fish configs
pub struct ShellParser;

impl ConfigParser for ShellParser {
    fn name(&self) -> &'static str {
        "shell"
    }

    fn can_parse(&self, path: &Path) -> bool {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            matches!(
                name,
                ".bashrc"
                    | ".bash_profile"
                    | ".bash_logout"
                    | ".zshrc"
                    | ".zshenv"
                    | ".zprofile"
                    | ".profile"
            ) || name.ends_with("config.fish")
        } else {
            false
        }
    }

    fn parse(&self, path: &Path) -> Result<ParsedConfig, ParseError> {
        let start = Instant::now();

        let content = fs::read_to_string(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ParseError::FileNotFound(path.to_path_buf()),
            std::io::ErrorKind::PermissionDenied => {
                ParseError::PermissionDenied(path.to_path_buf())
            }
            _ => ParseError::IoError {
                path: path.to_path_buf(),
                error: e.to_string(),
            },
        })?;

        let shell_type = detect_shell_type(path);
        let data = match shell_type {
            ShellType::Fish => parse_fish_config(&content),
            _ => parse_bash_like_config(&content),
        };

        Ok(ParsedConfig {
            path: path.to_path_buf(),
            config_type: ConfigType::Shell { shell_type },
            data,
            parse_time_ms: start.elapsed().as_millis() as u64,
        })
    }
}

fn detect_shell_type(path: &Path) -> ShellType {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name.contains("zsh") {
            return ShellType::Zsh;
        }
        if name.contains("fish") {
            return ShellType::Fish;
        }
    }
    ShellType::Bash
}

fn parse_bash_like_config(content: &str) -> Value {
    let mut aliases = HashMap::new();
    let mut exports = HashMap::new();
    let mut functions = Vec::new();
    let mut path_additions = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip comments and empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Parse aliases: alias name='command'
        if trimmed.starts_with("alias ") {
            if let Some((name, cmd)) = parse_alias(trimmed) {
                aliases.insert(name, cmd);
            }
        }

        // Parse exports: export VAR=value
        if trimmed.starts_with("export ") {
            if let Some((var, value)) = parse_export(trimmed) {
                // Track PATH additions before moving value
                if var == "PATH" && value.contains("$PATH") {
                    if let Some(addition) = extract_path_addition(&value) {
                        path_additions.push(addition);
                    }
                }
                exports.insert(var, value);
            }
        }

        // Parse function definitions: function name() { or name() {
        if (trimmed.starts_with("function ") || trimmed.contains("()")) && trimmed.contains('{') {
            if let Some(func_name) = parse_function_name(trimmed) {
                functions.push(func_name);
            }
        }
    }

    json!({
        "aliases": aliases,
        "exports": exports,
        "functions": functions,
        "path_additions": path_additions,
    })
}

fn parse_fish_config(content: &str) -> Value {
    let mut aliases = HashMap::new();
    let mut exports = HashMap::new();
    let mut functions = Vec::new();
    let mut path_additions = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Fish aliases: alias name='command' or alias name 'command'
        if trimmed.starts_with("alias ") {
            if let Some((name, cmd)) = parse_alias(trimmed) {
                aliases.insert(name, cmd);
            }
        }

        // Fish exports: set -x VAR value
        if trimmed.starts_with("set -x ") || trimmed.starts_with("set --export ") {
            if let Some((var, value)) = parse_fish_export(trimmed) {
                // Check PATH additions before moving value
                if var == "PATH" {
                    if let Some(addition) = extract_path_addition(&value) {
                        path_additions.push(addition);
                    }
                }
                exports.insert(var, value);
            }
        }

        // Fish functions: function name
        if trimmed.starts_with("function ") {
            if let Some(func_name) = trimmed
                .strip_prefix("function ")
                .and_then(|s| s.split_whitespace().next())
            {
                functions.push(func_name.to_string());
            }
        }
    }

    json!({
        "aliases": aliases,
        "exports": exports,
        "functions": functions,
        "path_additions": path_additions,
    })
}

fn parse_alias(line: &str) -> Option<(String, String)> {
    // Parse: alias name='command' or alias name="command" or alias name command
    let line = line.strip_prefix("alias ")?.trim();

    // Try with = first (bash/zsh style)
    if let Some((name, cmd)) = line.split_once('=') {
        let name = name.trim().to_string();
        let cmd = cmd
            .trim()
            .trim_matches(|c| c == '\'' || c == '"')
            .to_string();
        return Some((name, cmd));
    }

    // Try fish style: alias name 'command' or alias name command
    if let Some((name, cmd)) = line.split_once(' ') {
        let name = name.trim().to_string();
        let cmd = cmd
            .trim()
            .trim_matches(|c| c == '\'' || c == '"')
            .to_string();
        return Some((name, cmd));
    }

    None
}

fn parse_export(line: &str) -> Option<(String, String)> {
    // Parse: export VAR=value or export VAR="value"
    let line = line.strip_prefix("export ")?.trim();

    if let Some((var, value)) = line.split_once('=') {
        let var = var.trim().to_string();
        let value = value
            .trim()
            .trim_matches(|c| c == '\'' || c == '"')
            .to_string();
        return Some((var, value));
    }

    None
}

fn parse_fish_export(line: &str) -> Option<(String, String)> {
    // Parse: set -x VAR value or set --export VAR value
    let line = line
        .strip_prefix("set -x ")
        .or_else(|| line.strip_prefix("set --export "))?
        .trim();

    let mut parts = line.splitn(2, ' ');
    let var = parts.next()?.trim().to_string();
    let value = parts
        .next()?
        .trim()
        .trim_matches(|c| c == '\'' || c == '"')
        .to_string();

    Some((var, value))
}

fn parse_function_name(line: &str) -> Option<String> {
    // Parse: function name() { or name() {
    if let Some(stripped) = line.strip_prefix("function ") {
        return stripped.split('(').next().map(|s| s.trim().to_string());
    }

    if let Some((name, _)) = line.split_once('(') {
        return Some(name.trim().to_string());
    }

    None
}

fn extract_path_addition(value: &str) -> Option<String> {
    // Extract new path from: $PATH:/new/path or /new/path:$PATH
    for part in value.split(':') {
        let part = part.trim();
        if !part.contains("$PATH") && !part.is_empty() {
            return Some(part.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_parse_shell_configs() {
        let parser = ShellParser;
        assert!(parser.can_parse(Path::new("/home/user/.bashrc")));
        assert!(parser.can_parse(Path::new("/home/user/.zshrc")));
        assert!(parser.can_parse(Path::new("/home/user/.config/fish/config.fish")));
        assert!(!parser.can_parse(Path::new("/home/user/.gitconfig")));
    }

    #[test]
    fn test_detect_shell_type() {
        assert_eq!(detect_shell_type(Path::new(".bashrc")), ShellType::Bash);
        assert_eq!(detect_shell_type(Path::new(".zshrc")), ShellType::Zsh);
        assert_eq!(detect_shell_type(Path::new("config.fish")), ShellType::Fish);
    }

    #[test]
    fn test_parse_alias() {
        assert_eq!(
            parse_alias("alias ll='ls -la'"),
            Some(("ll".to_string(), "ls -la".to_string()))
        );
        assert_eq!(
            parse_alias("alias gs=\"git status\""),
            Some(("gs".to_string(), "git status".to_string()))
        );
    }

    #[test]
    fn test_parse_export() {
        assert_eq!(
            parse_export("export EDITOR=nvim"),
            Some(("EDITOR".to_string(), "nvim".to_string()))
        );
        assert_eq!(
            parse_export("export PATH=\"$HOME/.local/bin:$PATH\""),
            Some(("PATH".to_string(), "$HOME/.local/bin:$PATH".to_string()))
        );
    }

    #[test]
    fn test_parse_fish_export() {
        assert_eq!(
            parse_fish_export("set -x EDITOR nvim"),
            Some(("EDITOR".to_string(), "nvim".to_string()))
        );
        assert_eq!(
            parse_fish_export("set --export PATH $HOME/.local/bin $PATH"),
            Some(("PATH".to_string(), "$HOME/.local/bin $PATH".to_string()))
        );
    }

    #[test]
    fn test_parse_function_name() {
        assert_eq!(
            parse_function_name("function update_system() {"),
            Some("update_system".to_string())
        );
        assert_eq!(
            parse_function_name("backup_home() {"),
            Some("backup_home".to_string())
        );
    }

    #[test]
    fn test_extract_path_addition() {
        assert_eq!(
            extract_path_addition("$HOME/.local/bin:$PATH"),
            Some("$HOME/.local/bin".to_string())
        );
        assert_eq!(
            extract_path_addition("$PATH:/usr/local/bin"),
            Some("/usr/local/bin".to_string())
        );
    }

    #[test]
    fn test_parse_bash_like_config() {
        let content = r#"
# Comment
alias ll='ls -la'
alias gs="git status"
export EDITOR=nvim
export PATH="$HOME/.local/bin:$PATH"
function update_system() {
    echo "Updating..."
}
"#;
        let result = parse_bash_like_config(content);

        assert_eq!(result["aliases"]["ll"], "ls -la");
        assert_eq!(result["aliases"]["gs"], "git status");
        assert_eq!(result["exports"]["EDITOR"], "nvim");
        assert!(result["functions"]
            .as_array()
            .unwrap()
            .contains(&json!("update_system")));
        assert!(result["path_additions"].as_array().unwrap().len() > 0);
    }

    #[test]
    fn test_parse_fish_config() {
        let content = r#"
# Fish config
alias ll 'ls -la'
set -x EDITOR nvim
set --export PATH $HOME/.local/bin $PATH
function update_system
    echo "Updating..."
end
"#;
        let result = parse_fish_config(content);

        assert_eq!(result["aliases"]["ll"], "ls -la");
        assert_eq!(result["exports"]["EDITOR"], "nvim");
        assert!(result["functions"]
            .as_array()
            .unwrap()
            .contains(&json!("update_system")));
    }
}
