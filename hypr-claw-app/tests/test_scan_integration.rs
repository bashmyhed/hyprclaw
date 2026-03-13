#[cfg(test)]
mod scan_integration_tests {
    use hypr_claw_app::scan::*;
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[test]
    fn test_full_discovery_workflow() {
        // Discover user directories
        let user_dirs = UserDirectories::discover();
        println!("\n📂 User Directories:");
        println!("  Home: {}", user_dirs.home.display());
        println!("  Config: {}", user_dirs.config.display());
        println!("  Data: {}", user_dirs.data.display());

        if let Some(docs) = &user_dirs.documents {
            println!("  Documents: {}", docs.display());
        }
        if let Some(dl) = &user_dirs.download {
            println!("  Downloads: {}", dl.display());
        }

        // Discover home structure
        let discovered = discover_home_structure(&user_dirs.home);
        println!("\n🔍 Discovered {} directories", discovered.len());

        // Group by sensitivity
        let public = discovered
            .iter()
            .filter(|d| d.sensitivity == SensitivityLevel::Public)
            .count();
        let personal = discovered
            .iter()
            .filter(|d| d.sensitivity == SensitivityLevel::Personal)
            .count();
        let sensitive = discovered
            .iter()
            .filter(|d| d.sensitivity == SensitivityLevel::Sensitive)
            .count();

        println!("  Public: {}", public);
        println!("  Personal: {}", personal);
        println!("  Sensitive: {}", sensitive);

        // Show some examples
        println!("\n📋 Sample discoveries:");
        for dir in discovered.iter().take(5) {
            println!(
                "  {} - {} ({:?})",
                dir.path.display(),
                format_category(&dir.category),
                dir.sensitivity
            );
        }

        // Test resource monitor
        let monitor = ResourceMonitor::auto_calibrate();
        println!("\n⚙️  Resource Monitor:");
        println!("  CPU limit: {:.1}%", monitor.cpu_limit_percent);
        println!("  Memory limit: {} MB", monitor.memory_limit_mb);
        println!("  Worker threads: {}", monitor.adjust_worker_count());

        // Test policy creation
        let policy = ScanPolicy::default();
        println!("\n📜 Default Policy:");
        println!("  Standard depth: {}", policy.standard_depth);
        println!("  Max file size: {} MB", policy.max_file_size / 1024 / 1024);
        println!("  Exclude patterns: {}", policy.exclude_patterns.len());

        assert!(!discovered.is_empty());
        assert!(monitor.cpu_limit_percent > 0.0);
        assert!(policy.standard_depth > 0);
    }

    #[test]
    fn test_policy_filtering() {
        let policy = ScanPolicy::default();

        // Should exclude .git
        let git_path = std::path::PathBuf::from("/home/user/project/.git");
        assert!(!policy.should_scan(&git_path));

        // Should exclude node_modules
        let node_path = std::path::PathBuf::from("/home/user/project/node_modules");
        assert!(!policy.should_scan(&node_path));

        // Should allow normal directories
        let src_path = std::path::PathBuf::from("/home/user/project/src");
        assert!(policy.should_scan(&src_path));
    }

    #[tokio::test]
    async fn test_full_scan_workflow() {
        use std::fs;

        // Create test directory structure
        let temp_dir = std::env::temp_dir().join("test_full_scan");
        fs::create_dir_all(&temp_dir).ok();

        // Create various file types
        fs::write(temp_dir.join("main.rs"), "fn main() {}").unwrap();
        fs::write(temp_dir.join("config.toml"), "[package]\nname = \"test\"").unwrap();
        fs::write(temp_dir.join("script.sh"), "#!/bin/bash\necho hello").unwrap();
        fs::write(temp_dir.join("README.md"), "# Test Project").unwrap();

        // Create subdirectory
        fs::create_dir_all(temp_dir.join("src")).unwrap();
        fs::write(temp_dir.join("src/lib.rs"), "pub fn test() {}").unwrap();

        // Create excluded directory
        fs::create_dir_all(temp_dir.join(".git")).unwrap();
        fs::write(temp_dir.join(".git/config"), "git config").unwrap();

        println!("\n🔎 Starting full scan test...");

        let policy = ScanPolicy::default();
        let monitor = ResourceMonitor::auto_calibrate();
        let interrupt = Arc::new(Notify::new());

        let result = scan_directory(&temp_dir, &policy, &monitor, interrupt)
            .await
            .unwrap();

        println!("\n📊 Scan Results:");
        println!("  Files scanned: {}", result.stats.files_scanned);
        println!("  Directories: {}", result.stats.dirs_scanned);
        println!("  Bytes processed: {}", result.stats.bytes_processed);
        println!(
            "  Throughput: {:.2} MB/s",
            result.stats.throughput_mb_per_sec()
        );
        println!("  Files/sec: {:.2}", result.stats.files_per_sec());

        // Verify results
        assert!(
            result.stats.files_scanned >= 5,
            "Should scan at least 5 files"
        );
        assert!(
            result.stats.dirs_scanned >= 2,
            "Should scan at least 2 dirs"
        );

        // Check file classifications
        let rust_files = result
            .scanned_files
            .iter()
            .filter(|f| matches!(f.file_class, FileClass::Source { .. }))
            .count();
        let config_files = result
            .scanned_files
            .iter()
            .filter(|f| matches!(f.file_class, FileClass::Config { .. }))
            .count();

        println!("\n📁 File Classifications:");
        println!("  Rust source files: {}", rust_files);
        println!("  Config files: {}", config_files);

        assert!(rust_files >= 2, "Should find Rust files");
        assert!(config_files >= 1, "Should find config files");

        // Verify .git was excluded
        assert!(
            !result
                .scanned_files
                .iter()
                .any(|f| f.path.to_string_lossy().contains(".git")),
            ".git should be excluded"
        );

        fs::remove_dir_all(temp_dir).ok();
    }
}
