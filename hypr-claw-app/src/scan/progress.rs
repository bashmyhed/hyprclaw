use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

/// Progress tracking for scan operations
#[derive(Debug)]
pub struct ScanProgress {
    pub files_scanned: AtomicUsize,
    pub dirs_scanned: AtomicUsize,
    pub bytes_processed: AtomicU64,
    pub skipped_large: AtomicUsize,
    pub skipped_binary: AtomicUsize,
    pub skipped_excluded: AtomicUsize,
    pub errors: Arc<Mutex<Vec<String>>>,
    pub start_time: Instant,
}

impl ScanProgress {
    pub fn new() -> Self {
        Self {
            files_scanned: AtomicUsize::new(0),
            dirs_scanned: AtomicUsize::new(0),
            bytes_processed: AtomicU64::new(0),
            skipped_large: AtomicUsize::new(0),
            skipped_binary: AtomicUsize::new(0),
            skipped_excluded: AtomicUsize::new(0),
            errors: Arc::new(Mutex::new(Vec::new())),
            start_time: Instant::now(),
        }
    }

    pub fn increment_files(&self) {
        self.files_scanned.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_dirs(&self) {
        self.dirs_scanned.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_bytes(&self, bytes: u64) {
        self.bytes_processed.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn files_scanned_count(&self) -> usize {
        self.files_scanned.load(Ordering::Relaxed)
    }

    pub fn dirs_scanned_count(&self) -> usize {
        self.dirs_scanned.load(Ordering::Relaxed)
    }

    pub fn increment_skipped_large(&self) {
        self.skipped_large.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_skipped_binary(&self) {
        self.skipped_binary.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_skipped_excluded(&self) {
        self.skipped_excluded.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn add_error(&self, error: String) {
        self.errors.lock().await.push(error);
    }

    pub fn get_stats(&self) -> ScanStats {
        ScanStats {
            files_scanned: self.files_scanned.load(Ordering::Relaxed),
            dirs_scanned: self.dirs_scanned.load(Ordering::Relaxed),
            bytes_processed: self.bytes_processed.load(Ordering::Relaxed),
            skipped_large: self.skipped_large.load(Ordering::Relaxed),
            skipped_binary: self.skipped_binary.load(Ordering::Relaxed),
            skipped_excluded: self.skipped_excluded.load(Ordering::Relaxed),
            elapsed_secs: self.start_time.elapsed().as_secs(),
        }
    }

    pub fn print_progress(&self) {
        let stats = self.get_stats();
        let mb = stats.bytes_processed / 1024 / 1024;
        let total_skipped = stats.skipped_large + stats.skipped_binary + stats.skipped_excluded;

        print!(
            "\r🔎 Scanning: {} files, {} dirs, {} MB, {} skipped | {}s",
            stats.files_scanned, stats.dirs_scanned, mb, total_skipped, stats.elapsed_secs
        );
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }
}

impl Default for ScanProgress {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct ScanStats {
    pub files_scanned: usize,
    pub dirs_scanned: usize,
    pub bytes_processed: u64,
    pub skipped_large: usize,
    pub skipped_binary: usize,
    pub skipped_excluded: usize,
    pub elapsed_secs: u64,
}

impl ScanStats {
    pub fn throughput_mb_per_sec(&self) -> f64 {
        if self.elapsed_secs == 0 {
            return 0.0;
        }
        (self.bytes_processed as f64 / 1024.0 / 1024.0) / self.elapsed_secs as f64
    }

    pub fn files_per_sec(&self) -> f64 {
        if self.elapsed_secs == 0 {
            return 0.0;
        }
        self.files_scanned as f64 / self.elapsed_secs as f64
    }
}

/// Scanned file entry
#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub size: u64,
    pub file_type: FileType,
    pub category: FileCategory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileCategory {
    Config,
    Script,
    Source,
    Document,
    Media,
    Binary,
    Data,
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_progress_new() {
        let progress = ScanProgress::new();
        let stats = progress.get_stats();
        assert_eq!(stats.files_scanned, 0);
        assert_eq!(stats.dirs_scanned, 0);
        assert_eq!(stats.bytes_processed, 0);
    }

    #[test]
    fn test_scan_progress_increment() {
        let progress = ScanProgress::new();
        progress.increment_files();
        progress.increment_files();
        progress.increment_dirs();
        progress.add_bytes(1024);

        let stats = progress.get_stats();
        assert_eq!(stats.files_scanned, 2);
        assert_eq!(stats.dirs_scanned, 1);
        assert_eq!(stats.bytes_processed, 1024);
    }

    #[test]
    fn test_scan_stats_throughput() {
        let stats = ScanStats {
            files_scanned: 100,
            dirs_scanned: 10,
            bytes_processed: 10 * 1024 * 1024, // 10 MB
            skipped_large: 5,
            skipped_binary: 3,
            skipped_excluded: 2,
            elapsed_secs: 2,
        };

        assert_eq!(stats.throughput_mb_per_sec(), 5.0);
        assert_eq!(stats.files_per_sec(), 50.0);
    }

    #[tokio::test]
    async fn test_scan_progress_errors() {
        let progress = ScanProgress::new();
        progress.add_error("test error".to_string()).await;

        let errors = progress.errors.lock().await;
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0], "test error");
    }
}
