use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Directory category based on content analysis
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryCategory {
    SystemConfig,
    DotFiles,
    Projects,
    SourceCode,
    Documents,
    Media,
    Downloads,
    Credentials,
    PrivateData,
    Cache,
    Logs,
    Unknown,
}

/// Sensitivity level for directory access
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SensitivityLevel {
    Public,
    Personal,
    Sensitive,
}

/// Classify directory by name and content
pub fn classify_directory(path: &Path) -> super::discovery::DiscoveredDirectory {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    let (category, sensitivity) = match name.as_str() {
        // Credentials
        ".ssh" | ".gnupg" | ".aws" | ".password-store" => {
            (DirectoryCategory::Credentials, SensitivityLevel::Sensitive)
        }
        // System config
        ".config" | ".local" => (DirectoryCategory::SystemConfig, SensitivityLevel::Public),
        // Cache/temp
        ".cache" | ".tmp" | "cache" => (DirectoryCategory::Cache, SensitivityLevel::Public),
        // Logs
        ".log" | "logs" => (DirectoryCategory::Logs, SensitivityLevel::Public),
        // Development
        _ if is_project_directory(path) => (DirectoryCategory::Projects, SensitivityLevel::Public),
        // Content-based
        _ => classify_by_content(path),
    };

    let (size_estimate, file_count_estimate) = estimate_directory_size(path);

    super::discovery::DiscoveredDirectory {
        path: path.to_path_buf(),
        category,
        size_estimate,
        file_count_estimate,
        sensitivity,
    }
}

fn is_project_directory(path: &Path) -> bool {
    path.join(".git").exists()
        || path.join("Cargo.toml").exists()
        || path.join("package.json").exists()
        || path.join("pyproject.toml").exists()
        || path.join("go.mod").exists()
        || path.join("pom.xml").exists()
        || path.join("build.gradle").exists()
}

fn classify_by_content(path: &Path) -> (DirectoryCategory, SensitivityLevel) {
    let sample_size = 50;
    let mut file_types = HashMap::new();

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten().take(sample_size) {
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                *file_types.entry(ext.to_lowercase()).or_insert(0) += 1;
            }
        }
    }

    if file_types.is_empty() {
        return (DirectoryCategory::Unknown, SensitivityLevel::Public);
    }

    let total = file_types.values().sum::<usize>() as f32;

    for (ext, count) in file_types {
        let ratio = count as f32 / total;
        if ratio > 0.3 {
            return match ext.as_str() {
                "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "bmp" => {
                    (DirectoryCategory::Media, SensitivityLevel::Personal)
                }
                "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" => {
                    (DirectoryCategory::Media, SensitivityLevel::Personal)
                }
                "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" => {
                    (DirectoryCategory::Media, SensitivityLevel::Personal)
                }
                "pdf" | "doc" | "docx" | "txt" | "md" | "odt" | "rtf" => {
                    (DirectoryCategory::Documents, SensitivityLevel::Personal)
                }
                "rs" | "py" | "js" | "ts" | "go" | "c" | "cpp" | "java" | "rb" => {
                    (DirectoryCategory::SourceCode, SensitivityLevel::Public)
                }
                "log" => (DirectoryCategory::Logs, SensitivityLevel::Public),
                _ => (DirectoryCategory::Unknown, SensitivityLevel::Public),
            };
        }
    }

    (DirectoryCategory::Unknown, SensitivityLevel::Public)
}

fn estimate_directory_size(path: &Path) -> (u64, usize) {
    let mut total_size = 0u64;
    let mut file_count = 0usize;

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten().take(100) {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total_size += meta.len();
                    file_count += 1;
                }
            }
        }
    }

    if file_count == 100 {
        total_size = total_size.saturating_mul(10);
        file_count = file_count.saturating_mul(10);
    }

    (total_size, file_count)
}

pub fn format_category(cat: &DirectoryCategory) -> &'static str {
    match cat {
        DirectoryCategory::SystemConfig => "system config",
        DirectoryCategory::DotFiles => "dotfiles",
        DirectoryCategory::Projects => "projects",
        DirectoryCategory::SourceCode => "source code",
        DirectoryCategory::Documents => "documents",
        DirectoryCategory::Media => "media",
        DirectoryCategory::Downloads => "downloads",
        DirectoryCategory::Credentials => "credentials",
        DirectoryCategory::PrivateData => "private data",
        DirectoryCategory::Cache => "cache",
        DirectoryCategory::Logs => "logs",
        DirectoryCategory::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_is_project_directory() {
        let temp = std::env::temp_dir().join("test_project");
        fs::create_dir_all(&temp).unwrap();

        assert!(!is_project_directory(&temp));

        fs::write(temp.join("Cargo.toml"), "").unwrap();
        assert!(is_project_directory(&temp));

        fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_classify_directory_credentials() {
        let home = std::env::var("HOME").unwrap_or_default();
        let ssh_path = PathBuf::from(home).join(".ssh");

        if ssh_path.exists() {
            let classified = classify_directory(&ssh_path);
            assert_eq!(classified.category, DirectoryCategory::Credentials);
            assert_eq!(classified.sensitivity, SensitivityLevel::Sensitive);
        }
    }

    #[test]
    fn test_classify_directory_config() {
        let home = std::env::var("HOME").unwrap_or_default();
        let config_path = PathBuf::from(home).join(".config");

        if config_path.exists() {
            let classified = classify_directory(&config_path);
            assert_eq!(classified.category, DirectoryCategory::SystemConfig);
            assert_eq!(classified.sensitivity, SensitivityLevel::Public);
        }
    }

    #[test]
    fn test_format_category() {
        assert_eq!(format_category(&DirectoryCategory::Projects), "projects");
        assert_eq!(
            format_category(&DirectoryCategory::Credentials),
            "credentials"
        );
    }
}
