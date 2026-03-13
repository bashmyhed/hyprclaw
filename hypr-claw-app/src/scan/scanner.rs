use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Notify;

use super::file_classifier::{classify_file_with_size, FileClass, SkipReason};
use super::policy::ScanPolicy;
use super::progress::ScanProgress;
use super::resource::ResourceMonitor;

/// Result of a scan operation
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub scanned_files: Vec<ScannedFileEntry>,
    pub stats: super::progress::ScanStats,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ScannedFileEntry {
    pub path: PathBuf,
    pub size: u64,
    pub file_class: FileClass,
}

/// Scan directory with policy and resource constraints
pub async fn scan_directory(
    root: &Path,
    policy: &ScanPolicy,
    resource_monitor: &ResourceMonitor,
    interrupt: Arc<Notify>,
) -> Result<ScanResult, Box<dyn std::error::Error>> {
    let progress = Arc::new(ScanProgress::new());
    let scanned_files = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    // Start progress printer
    let progress_clone = progress.clone();
    let progress_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            progress_clone.print_progress();
        }
    });

    // Scan with depth limit
    scan_recursive(
        root,
        policy,
        resource_monitor,
        &progress,
        &scanned_files,
        0,
        interrupt.clone(),
    )
    .await?;

    progress_handle.abort();
    progress.print_progress();
    println!(); // New line after progress

    let stats = progress.get_stats();
    let errors = progress.errors.lock().await.clone();
    let files = scanned_files.lock().await.clone();

    Ok(ScanResult {
        scanned_files: files,
        stats,
        errors,
    })
}

#[async_recursion::async_recursion]
async fn scan_recursive(
    dir: &Path,
    policy: &ScanPolicy,
    resource_monitor: &ResourceMonitor,
    progress: &Arc<ScanProgress>,
    scanned_files: &Arc<tokio::sync::Mutex<Vec<ScannedFileEntry>>>,
    depth: usize,
    interrupt: Arc<Notify>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Stop once scan budgets are reached.
    if progress.files_scanned_count() >= policy.max_files_total
        || progress.dirs_scanned_count() >= policy.max_dirs_total
    {
        return Ok(());
    }

    // Check interrupt
    if Arc::strong_count(&interrupt) > 1 {
        // Simple check - in production, use a proper cancellation token
        tokio::select! {
            _ = interrupt.notified() => return Ok(()),
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(1)) => {}
        }
    }

    // Check depth limit
    if depth >= policy.standard_depth {
        return Ok(());
    }

    // Check if should scan this directory
    if !policy.should_scan(dir) {
        progress.increment_skipped_excluded();
        return Ok(());
    }

    // Throttle if system is overloaded
    if resource_monitor.should_throttle() {
        tokio::time::sleep(tokio::time::Duration::from_millis(
            resource_monitor.io_throttle_ms,
        ))
        .await;
    }

    progress.increment_dirs();

    let entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(e) => {
            progress
                .add_error(format!("Failed to read {}: {}", dir.display(), e))
                .await;
            return Ok(());
        }
    };

    let mut entries = entries;
    let mut entries_seen = 0usize;
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entries_seen >= policy.max_entries_per_dir {
            break;
        }
        entries_seen += 1;

        if progress.files_scanned_count() >= policy.max_files_total
            || progress.dirs_scanned_count() >= policy.max_dirs_total
        {
            return Ok(());
        }

        // Check interrupt
        tokio::select! {
            _ = interrupt.notified() => return Ok(()),
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(1)) => {}
        }

        let path = entry.path();
        let file_type = match entry.file_type().await {
            Ok(t) => t,
            Err(_) => continue,
        };

        if file_type.is_dir() {
            scan_recursive(
                &path,
                policy,
                resource_monitor,
                progress,
                scanned_files,
                depth + 1,
                interrupt.clone(),
            )
            .await?;
        } else if file_type.is_file() {
            process_file(&path, policy, progress, scanned_files).await;
        }
    }

    Ok(())
}

async fn process_file(
    path: &Path,
    policy: &ScanPolicy,
    progress: &Arc<ScanProgress>,
    scanned_files: &Arc<tokio::sync::Mutex<Vec<ScannedFileEntry>>>,
) {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(m) => m,
        Err(_) => return,
    };

    let size = metadata.len();

    // Classify file
    let file_class = match classify_file_with_size(path, size, policy.max_file_size) {
        Ok(c) => c,
        Err(_) => {
            progress.increment_skipped_excluded();
            return;
        }
    };

    // Handle skipped files
    match &file_class {
        FileClass::Binary { reason } => match reason {
            SkipReason::TooLarge(_) => {
                progress.increment_skipped_large();
                return;
            }
            SkipReason::BinaryExecutable | SkipReason::CompressedArchive => {
                progress.increment_skipped_binary();
                return;
            }
        },
        _ => {}
    }

    progress.increment_files();
    progress.add_bytes(size);

    scanned_files.lock().await.push(ScannedFileEntry {
        path: path.to_path_buf(),
        size,
        file_class,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn test_scan_directory() {
        let temp_dir = std::env::temp_dir().join("test_scan");
        fs::create_dir_all(&temp_dir).ok();

        // Create test files
        fs::write(temp_dir.join("test.rs"), "fn main() {}").unwrap();
        fs::write(temp_dir.join("config.toml"), "[test]").unwrap();
        fs::create_dir_all(temp_dir.join("subdir")).unwrap();
        fs::write(temp_dir.join("subdir/test.py"), "print('hello')").unwrap();

        let policy = ScanPolicy::default();
        let monitor = ResourceMonitor::auto_calibrate();
        let interrupt = Arc::new(Notify::new());

        let result = scan_directory(&temp_dir, &policy, &monitor, interrupt)
            .await
            .unwrap();

        assert!(result.scanned_files.len() >= 3);
        assert!(result.stats.files_scanned >= 3);
        assert!(result.stats.dirs_scanned >= 2);

        fs::remove_dir_all(temp_dir).ok();
    }

    #[tokio::test]
    async fn test_scan_respects_depth() {
        let temp_dir = std::env::temp_dir().join("test_depth");
        fs::create_dir_all(&temp_dir).ok();

        // Create nested structure
        fs::create_dir_all(temp_dir.join("a/b/c/d/e")).unwrap();
        fs::write(temp_dir.join("a/b/c/d/e/deep.txt"), "deep").unwrap();

        let mut policy = ScanPolicy::default();
        policy.standard_depth = 2;

        let monitor = ResourceMonitor::auto_calibrate();
        let interrupt = Arc::new(Notify::new());

        let result = scan_directory(&temp_dir, &policy, &monitor, interrupt)
            .await
            .unwrap();

        // Should not find the deep file
        assert!(!result
            .scanned_files
            .iter()
            .any(|f| f.path.ends_with("deep.txt")));

        fs::remove_dir_all(temp_dir).ok();
    }

    #[tokio::test]
    async fn test_scan_respects_exclusions() {
        let temp_dir = std::env::temp_dir().join("test_exclusions");
        fs::create_dir_all(&temp_dir).ok();

        fs::write(temp_dir.join("normal.txt"), "normal").unwrap();
        fs::create_dir_all(temp_dir.join(".git")).unwrap();
        fs::write(temp_dir.join(".git/config"), "git config").unwrap();

        let policy = ScanPolicy::default();
        let monitor = ResourceMonitor::auto_calibrate();
        let interrupt = Arc::new(Notify::new());

        let result = scan_directory(&temp_dir, &policy, &monitor, interrupt)
            .await
            .unwrap();

        // Should find normal.txt but not .git/config
        assert!(result
            .scanned_files
            .iter()
            .any(|f| f.path.ends_with("normal.txt")));
        assert!(!result
            .scanned_files
            .iter()
            .any(|f| f.path.to_string_lossy().contains(".git")));

        fs::remove_dir_all(temp_dir).ok();
    }
}
