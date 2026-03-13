use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub mod git;
pub mod hyprland;
pub mod shell;

pub use git::GitParser;
pub use hyprland::HyprlandParser;
pub use shell::ShellParser;

/// Config parser trait - all parsers must implement this
pub trait ConfigParser: Send + Sync {
    /// Parser name for identification
    fn name(&self) -> &'static str;

    /// Check if this parser can handle the given file
    fn can_parse(&self, path: &Path) -> bool;

    /// Parse the config file and return structured data
    fn parse(&self, path: &Path) -> Result<ParsedConfig, ParseError>;
}

/// Successfully parsed config with metadata
#[derive(Debug, Clone)]
pub struct ParsedConfig {
    pub path: PathBuf,
    pub config_type: ConfigType,
    pub data: Value,
    pub parse_time_ms: u64,
}

/// Config type classification
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigType {
    Hyprland,
    I3,
    Sway,
    Shell { shell_type: ShellType },
    Git,
    Ssh,
    Tmux,
    Starship,
    Generic { format: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellType {
    Bash,
    Zsh,
    Fish,
}

/// Parse error types
#[derive(Debug, Clone)]
pub enum ParseError {
    FileNotFound(PathBuf),
    PermissionDenied(PathBuf),
    InvalidFormat { path: PathBuf, reason: String },
    IoError { path: PathBuf, error: String },
}

impl ParseError {
    pub fn user_message(&self) -> String {
        match self {
            Self::FileNotFound(p) => format!("Config not found: {}", p.display()),
            Self::PermissionDenied(p) => format!("Permission denied: {}", p.display()),
            Self::InvalidFormat { path, reason } => {
                format!("Invalid format in {}: {}", path.display(), reason)
            }
            Self::IoError { path, error } => format!("Error reading {}: {}", path.display(), error),
        }
    }
}

/// Parser registry - manages all config parsers
pub struct ParserRegistry {
    parsers: Vec<Box<dyn ConfigParser>>,
}

impl ParserRegistry {
    pub fn new() -> Self {
        Self {
            parsers: Vec::new(),
        }
    }

    pub fn register(&mut self, parser: Box<dyn ConfigParser>) {
        self.parsers.push(parser);
    }

    /// Parse a single file with the first matching parser
    pub fn parse(&self, path: &Path) -> Result<ParsedConfig, ParseError> {
        for parser in &self.parsers {
            if parser.can_parse(path) {
                return parser.parse(path);
            }
        }
        Err(ParseError::InvalidFormat {
            path: path.to_path_buf(),
            reason: "No parser available for this file type".to_string(),
        })
    }

    /// Parse multiple files, collecting results
    pub fn parse_all(&self, paths: &[PathBuf]) -> Vec<ParseResult> {
        paths
            .iter()
            .map(|path| ParseResult {
                path: path.clone(),
                result: self.parse(path),
            })
            .collect()
    }
}

impl Default for ParserRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of parsing a single file
#[derive(Debug, Clone)]
pub struct ParseResult {
    pub path: PathBuf,
    pub result: Result<ParsedConfig, ParseError>,
}

impl ParseResult {
    pub fn is_ok(&self) -> bool {
        self.result.is_ok()
    }

    pub fn is_err(&self) -> bool {
        self.result.is_err()
    }
}

/// Helper to partition results into success and failure
pub fn partition_results(
    results: Vec<ParseResult>,
) -> (Vec<ParsedConfig>, Vec<(PathBuf, ParseError)>) {
    let mut success = Vec::new();
    let mut failed = Vec::new();

    for result in results {
        match result.result {
            Ok(config) => success.push(config),
            Err(error) => failed.push((result.path, error)),
        }
    }

    (success, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestParser;

    impl ConfigParser for TestParser {
        fn name(&self) -> &'static str {
            "test"
        }

        fn can_parse(&self, path: &Path) -> bool {
            path.extension().and_then(|e| e.to_str()) == Some("test")
        }

        fn parse(&self, path: &Path) -> Result<ParsedConfig, ParseError> {
            Ok(ParsedConfig {
                path: path.to_path_buf(),
                config_type: ConfigType::Generic {
                    format: "test".to_string(),
                },
                data: serde_json::json!({"test": true}),
                parse_time_ms: 0,
            })
        }
    }

    #[test]
    fn test_registry_creation() {
        let registry = ParserRegistry::new();
        assert_eq!(registry.parsers.len(), 0);
    }

    #[test]
    fn test_registry_register() {
        let mut registry = ParserRegistry::new();
        registry.register(Box::new(TestParser));
        assert_eq!(registry.parsers.len(), 1);
    }

    #[test]
    fn test_registry_parse() {
        let mut registry = ParserRegistry::new();
        registry.register(Box::new(TestParser));

        let path = PathBuf::from("/test/file.test");
        let result = registry.parse(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_registry_parse_no_parser() {
        let registry = ParserRegistry::new();
        let path = PathBuf::from("/test/file.unknown");
        let result = registry.parse(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_partition_results() {
        let results = vec![
            ParseResult {
                path: PathBuf::from("/success.test"),
                result: Ok(ParsedConfig {
                    path: PathBuf::from("/success.test"),
                    config_type: ConfigType::Generic {
                        format: "test".to_string(),
                    },
                    data: serde_json::json!({}),
                    parse_time_ms: 0,
                }),
            },
            ParseResult {
                path: PathBuf::from("/fail.test"),
                result: Err(ParseError::FileNotFound(PathBuf::from("/fail.test"))),
            },
        ];

        let (success, failed) = partition_results(results);
        assert_eq!(success.len(), 1);
        assert_eq!(failed.len(), 1);
    }

    #[test]
    fn test_parse_error_user_message() {
        let error = ParseError::FileNotFound(PathBuf::from("/test.conf"));
        assert!(error.user_message().contains("not found"));

        let error = ParseError::PermissionDenied(PathBuf::from("/test.conf"));
        assert!(error.user_message().contains("Permission denied"));
    }
}
