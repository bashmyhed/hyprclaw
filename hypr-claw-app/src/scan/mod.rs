// Scan module - dynamic home directory scanning system

pub mod classifier;
pub mod discovery;
pub mod file_classifier;
pub mod integration;
pub mod parsers;
pub mod policy;
pub mod progress;
pub mod resource;
pub mod scanner;

pub use classifier::{classify_directory, format_category, DirectoryCategory};
pub use discovery::{discover_home_structure, DiscoveredDirectory, UserDirectories};
pub use file_classifier::{classify_file, ConfigType, FileClass, SkipReason};
pub use integration::run_integrated_scan;
pub use parsers::{ConfigParser, ParseError, ParsedConfig, ParserRegistry};
pub use policy::{ScanPolicy, SensitivityLevel};
pub use progress::{ScanProgress, ScanStats};
pub use resource::ResourceMonitor;
pub use scanner::{scan_directory, ScanResult, ScannedFileEntry};
