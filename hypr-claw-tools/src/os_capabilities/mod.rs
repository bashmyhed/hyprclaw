//! Structured OS capability layer - replaces generic shell execution
//!
//! This module provides type-safe, permission-controlled OS operations:
//! - Filesystem operations
//! - Process management
//! - Hyprland control
//! - System operations

pub mod desktop;
pub mod filesystem;
pub mod hyprland;
pub mod process;
pub mod system;

/// OS capability error types
#[derive(Debug, thiserror::Error)]
pub enum OsError {
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Operation failed: {0}")]
    OperationFailed(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type OsResult<T> = Result<T, OsError>;
