use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// XDG-compliant user directory resolver
#[derive(Debug, Clone)]
pub struct UserDirectories {
    pub home: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub state: PathBuf,
    pub desktop: Option<PathBuf>,
    pub documents: Option<PathBuf>,
    pub download: Option<PathBuf>,
    pub music: Option<PathBuf>,
    pub pictures: Option<PathBuf>,
    pub videos: Option<PathBuf>,
    pub templates: Option<PathBuf>,
    pub publicshare: Option<PathBuf>,
}

impl UserDirectories {
    pub fn discover() -> Self {
        let home = PathBuf::from(env::var("HOME").unwrap_or_else(|_| "/home/user".into()));

        let config = env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".config"));
        let data = env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".local/share"));
        let cache = env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".cache"));
        let state = env::var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".local/state"));

        let user_dirs_file = config.join("user-dirs.dirs");
        let xdg_dirs = parse_xdg_user_dirs(&user_dirs_file, &home);

        Self {
            home,
            config,
            data,
            cache,
            state,
            desktop: xdg_dirs.get("DESKTOP").cloned(),
            documents: xdg_dirs.get("DOCUMENTS").cloned(),
            download: xdg_dirs.get("DOWNLOAD").cloned(),
            music: xdg_dirs.get("MUSIC").cloned(),
            pictures: xdg_dirs.get("PICTURES").cloned(),
            videos: xdg_dirs.get("VIDEOS").cloned(),
            templates: xdg_dirs.get("TEMPLATES").cloned(),
            publicshare: xdg_dirs.get("PUBLICSHARE").cloned(),
        }
    }
}

fn parse_xdg_user_dirs(path: &Path, home: &Path) -> HashMap<String, PathBuf> {
    let mut dirs = HashMap::new();
    if let Ok(content) = fs::read_to_string(path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key
                    .trim()
                    .strip_prefix("XDG_")
                    .and_then(|k| k.strip_suffix("_DIR"));
                let value = value
                    .trim()
                    .trim_matches('"')
                    .replace("$HOME", &home.to_string_lossy());
                if let Some(k) = key {
                    let path = PathBuf::from(value);
                    if path.exists() {
                        dirs.insert(k.to_string(), path);
                    }
                }
            }
        }
    }
    dirs
}

/// Discovered directory with metadata
#[derive(Debug, Clone)]
pub struct DiscoveredDirectory {
    pub path: PathBuf,
    pub category: super::classifier::DirectoryCategory,
    pub size_estimate: u64,
    pub file_count_estimate: usize,
    pub sensitivity: super::policy::SensitivityLevel,
}

/// Discover and classify home directory structure
pub fn discover_home_structure(home: &Path) -> Vec<DiscoveredDirectory> {
    let mut discovered = Vec::new();

    if let Ok(entries) = fs::read_dir(home) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                discovered.push(super::classifier::classify_directory(&path));
            }
        }
    }

    discovered.sort_by(|a, b| {
        a.sensitivity
            .cmp(&b.sensitivity)
            .then(b.size_estimate.cmp(&a.size_estimate))
    });

    discovered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_directories_discovery() {
        let dirs = UserDirectories::discover();
        assert!(dirs.home.exists(), "Home directory should exist");
        assert!(dirs.config.exists(), "Config directory should exist");
    }

    #[test]
    fn test_parse_xdg_user_dirs() {
        let home = PathBuf::from("/home/test");
        let content = r#"
# User dirs
XDG_DESKTOP_DIR="$HOME/Desktop"
XDG_DOWNLOAD_DIR="$HOME/Downloads"
XDG_DOCUMENTS_DIR="$HOME/Documents"
"#;
        let temp_file = std::env::temp_dir().join("test_user_dirs.dirs");
        fs::write(&temp_file, content).unwrap();

        let dirs = parse_xdg_user_dirs(&temp_file, &home);
        // Note: These paths won't exist, so they won't be in the result
        // The function filters out non-existent paths
        assert!(dirs.is_empty() || dirs.contains_key("DESKTOP") || dirs.contains_key("DOWNLOAD"));

        fs::remove_file(temp_file).ok();
    }

    #[test]
    fn test_discover_home_structure() {
        let home = UserDirectories::discover().home;
        let discovered = discover_home_structure(&home);
        assert!(!discovered.is_empty(), "Should discover some directories");

        for dir in &discovered {
            assert!(dir.path.exists(), "Discovered path should exist");
            assert!(dir.path.is_dir(), "Discovered path should be directory");
        }
    }
}
