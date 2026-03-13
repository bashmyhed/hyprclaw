use std::io;
use std::path::PathBuf;

pub use super::classifier::SensitivityLevel;

/// Scan policy with user-selected directories
#[derive(Debug, Clone)]
pub struct ScanPolicy {
    pub included_paths: Vec<PathBuf>,
    pub excluded_paths: Vec<PathBuf>,
    pub standard_depth: usize,
    pub max_file_size: u64,
    pub max_files_total: usize,
    pub max_dirs_total: usize,
    pub max_entries_per_dir: usize,
    pub exclude_patterns: Vec<String>,
    pub scan_sensitive: bool,
}

impl Default for ScanPolicy {
    fn default() -> Self {
        Self {
            included_paths: Vec::new(),
            excluded_paths: Vec::new(),
            standard_depth: 3,
            max_file_size: 100 * 1024 * 1024, // 100MB
            max_files_total: 15_000,
            max_dirs_total: 4_000,
            max_entries_per_dir: 256,
            exclude_patterns: default_exclude_patterns(),
            scan_sensitive: false,
        }
    }
}

impl ScanPolicy {
    pub fn build_interactively(
        discovered: &[super::discovery::DiscoveredDirectory],
    ) -> io::Result<Self> {
        println!("\n🏠 Discovered home directory structure:\n");

        let public: Vec<_> = discovered
            .iter()
            .filter(|d| d.sensitivity == SensitivityLevel::Public)
            .collect();
        let personal: Vec<_> = discovered
            .iter()
            .filter(|d| d.sensitivity == SensitivityLevel::Personal)
            .collect();
        let sensitive: Vec<_> = discovered
            .iter()
            .filter(|d| d.sensitivity == SensitivityLevel::Sensitive)
            .collect();

        println!("📂 System & Development (will be scanned):");
        for dir in &public {
            println!(
                "  ✓ {} ({}, ~{} files)",
                dir.path.display(),
                super::classifier::format_category(&dir.category),
                dir.file_count_estimate
            );
        }

        println!("\n📁 Personal Content (choose what to scan):");
        let mut included_personal = Vec::new();
        for dir in &personal {
            let size_mb = dir.size_estimate / 1024 / 1024;
            let prompt = format!(
                "  Scan {} ({}, ~{} MB)? [y/N] ",
                dir.path.display(),
                super::classifier::format_category(&dir.category),
                size_mb
            );
            if prompt_yes_no(&prompt, false)? {
                included_personal.push(dir.path.clone());
            }
        }

        let mut scan_sensitive = false;
        if !sensitive.is_empty() {
            println!("\n🔐 Sensitive Directories (credentials, keys):");
            for dir in &sensitive {
                println!(
                    "  ⚠️  {} ({})",
                    dir.path.display(),
                    super::classifier::format_category(&dir.category)
                );
            }
            scan_sensitive = prompt_yes_no(
                "\nScan sensitive directories? (stored encrypted) [y/N] ",
                false,
            )?;
        }

        let mut included_paths = Vec::new();
        included_paths.extend(public.iter().map(|d| d.path.clone()));
        included_paths.extend(included_personal);
        if scan_sensitive {
            included_paths.extend(sensitive.iter().map(|d| d.path.clone()));
        }

        let excluded_paths = discovered
            .iter()
            .filter(|d| {
                matches!(
                    d.category,
                    super::classifier::DirectoryCategory::Cache
                        | super::classifier::DirectoryCategory::Logs
                )
            })
            .map(|d| d.path.clone())
            .collect();

        Ok(ScanPolicy {
            included_paths,
            excluded_paths,
            standard_depth: 3,
            max_file_size: 100 * 1024 * 1024,
            max_files_total: 15_000,
            max_dirs_total: 4_000,
            max_entries_per_dir: 256,
            exclude_patterns: default_exclude_patterns(),
            scan_sensitive,
        })
    }

    pub fn should_scan(&self, path: &std::path::Path) -> bool {
        if self.excluded_paths.iter().any(|p| path.starts_with(p)) {
            return false;
        }

        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if self.exclude_patterns.iter().any(|p| name == p) {
                return false;
            }
        }

        true
    }
}

fn default_exclude_patterns() -> Vec<String> {
    vec![
        ".git".into(),
        "node_modules".into(),
        ".cache".into(),
        "target".into(),
        "build".into(),
        "dist".into(),
        ".venv".into(),
        "__pycache__".into(),
        ".npm".into(),
        ".cargo".into(),
        ".rustup".into(),
        ".local/share/Trash".into(),
    ]
}

fn prompt_yes_no(prompt: &str, default_yes: bool) -> io::Result<bool> {
    use std::io::Write;
    print!("{}", prompt);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();

    Ok(match input.as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        "" => default_yes,
        _ => default_yes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_default_policy() {
        let policy = ScanPolicy::default();
        assert_eq!(policy.standard_depth, 3);
        assert_eq!(policy.max_file_size, 100 * 1024 * 1024);
        assert_eq!(policy.max_files_total, 15_000);
        assert_eq!(policy.max_dirs_total, 4_000);
        assert_eq!(policy.max_entries_per_dir, 256);
        assert!(!policy.scan_sensitive);
        assert!(!policy.exclude_patterns.is_empty());
    }

    #[test]
    fn test_should_scan_excluded_patterns() {
        let policy = ScanPolicy::default();
        let git_path = PathBuf::from("/home/user/project/.git");
        assert!(!policy.should_scan(&git_path));

        let node_path = PathBuf::from("/home/user/project/node_modules");
        assert!(!policy.should_scan(&node_path));

        let normal_path = PathBuf::from("/home/user/project/src");
        assert!(policy.should_scan(&normal_path));
    }

    #[test]
    fn test_should_scan_excluded_paths() {
        let mut policy = ScanPolicy::default();
        policy
            .excluded_paths
            .push(PathBuf::from("/home/user/.cache"));

        let cache_path = PathBuf::from("/home/user/.cache/something");
        assert!(!policy.should_scan(&cache_path));

        let normal_path = PathBuf::from("/home/user/.config");
        assert!(policy.should_scan(&normal_path));
    }

    #[test]
    fn test_default_exclude_patterns() {
        let patterns = default_exclude_patterns();
        assert!(patterns.contains(&".git".to_string()));
        assert!(patterns.contains(&"node_modules".to_string()));
        assert!(patterns.contains(&"target".to_string()));
    }
}
