use super::{ConfigParser, ConfigType, ParseError, ParsedConfig};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

/// Git config parser - handles .gitconfig and git/config files
pub struct GitParser;

impl ConfigParser for GitParser {
    fn name(&self) -> &'static str {
        "git"
    }

    fn can_parse(&self, path: &Path) -> bool {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            name == ".gitconfig"
                || (name == "config"
                    && path
                        .parent()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        == Some("git"))
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

        let data = parse_git_config(&content);

        Ok(ParsedConfig {
            path: path.to_path_buf(),
            config_type: ConfigType::Git,
            data,
            parse_time_ms: start.elapsed().as_millis() as u64,
        })
    }
}

fn parse_git_config(content: &str) -> Value {
    let mut user = HashMap::new();
    let mut aliases = HashMap::new();
    let mut remotes = Vec::new();
    let mut core = HashMap::new();
    let mut current_section: Option<String> = None;
    let mut current_remote: Option<HashMap<String, String>> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        // Parse section headers: [section] or [section "subsection"]
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // Save previous remote if any
            if let Some(remote) = current_remote.take() {
                remotes.push(remote);
            }

            let section = trimmed.trim_matches(|c| c == '[' || c == ']');
            current_section = Some(section.to_string());

            // Start tracking remote
            if section.starts_with("remote ") {
                current_remote = Some(HashMap::new());
                if let Some(name) = section
                    .strip_prefix("remote \"")
                    .and_then(|s| s.strip_suffix('"'))
                {
                    if let Some(ref mut remote) = current_remote {
                        remote.insert("name".to_string(), name.to_string());
                    }
                }
            }
            continue;
        }

        // Parse key = value pairs
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            let value = value.trim();

            match current_section.as_deref() {
                Some("user") => {
                    user.insert(key.to_string(), value.to_string());
                }
                Some("core") => {
                    core.insert(key.to_string(), value.to_string());
                }
                Some(section) if section.starts_with("alias") => {
                    aliases.insert(key.to_string(), value.to_string());
                }
                Some(section) if section.starts_with("remote ") => {
                    if let Some(ref mut remote) = current_remote {
                        remote.insert(key.to_string(), value.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    // Save last remote if any
    if let Some(remote) = current_remote {
        remotes.push(remote);
    }

    json!({
        "user": user,
        "aliases": aliases,
        "remotes": remotes,
        "core": core,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_parse_git_config() {
        let parser = GitParser;
        assert!(parser.can_parse(Path::new("/home/user/.gitconfig")));
        assert!(parser.can_parse(Path::new("/home/user/.config/git/config")));
        assert!(!parser.can_parse(Path::new("/home/user/.bashrc")));
    }

    #[test]
    fn test_parse_git_config() {
        let content = r#"
[user]
    name = John Doe
    email = john@example.com

[core]
    editor = nvim
    autocrlf = input

[alias]
    st = status
    co = checkout
    br = branch

[remote "origin"]
    url = git@github.com:user/repo.git
    fetch = +refs/heads/*:refs/remotes/origin/*
"#;
        let result = parse_git_config(content);

        assert_eq!(result["user"]["name"], "John Doe");
        assert_eq!(result["user"]["email"], "john@example.com");
        assert_eq!(result["core"]["editor"], "nvim");
        assert_eq!(result["aliases"]["st"], "status");
        assert_eq!(result["remotes"][0]["name"], "origin");
        assert!(result["remotes"][0]["url"]
            .as_str()
            .unwrap()
            .contains("github.com"));
    }
}
