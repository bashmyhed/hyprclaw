//! Filesystem operations - structured, safe file management

use super::{OsError, OsResult};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Create a directory
pub async fn create_dir<P: AsRef<Path>>(path: P) -> OsResult<()> {
    let path = path.as_ref();
    fs::create_dir_all(path).await?;
    Ok(())
}

/// Delete a file or directory
pub async fn delete<P: AsRef<Path>>(path: P) -> OsResult<()> {
    let path = path.as_ref();

    if !path.exists() {
        return Err(OsError::NotFound(path.display().to_string()));
    }

    if path.is_dir() {
        fs::remove_dir_all(path).await?;
    } else {
        fs::remove_file(path).await?;
    }

    Ok(())
}

/// Move/rename a file or directory
pub async fn move_path<P: AsRef<Path>>(from: P, to: P) -> OsResult<()> {
    let from = from.as_ref();
    let to = to.as_ref();

    if !from.exists() {
        return Err(OsError::NotFound(from.display().to_string()));
    }

    fs::rename(from, to).await?;
    Ok(())
}

/// Copy a file
pub async fn copy_file<P: AsRef<Path>>(from: P, to: P) -> OsResult<()> {
    let from = from.as_ref();
    let to = to.as_ref();

    if !from.exists() {
        return Err(OsError::NotFound(from.display().to_string()));
    }

    if !from.is_file() {
        return Err(OsError::InvalidArgument(
            "Source must be a file".to_string(),
        ));
    }

    fs::copy(from, to).await?;
    Ok(())
}

/// Read file contents
pub async fn read<P: AsRef<Path>>(path: P) -> OsResult<String> {
    let path = path.as_ref();

    if !path.exists() {
        return Err(OsError::NotFound(path.display().to_string()));
    }

    let content = fs::read_to_string(path).await?;
    Ok(content)
}

/// Write file contents
pub async fn write<P: AsRef<Path>>(path: P, content: &str) -> OsResult<()> {
    let path = path.as_ref();
    fs::write(path, content).await?;
    Ok(())
}

/// List directory contents
pub async fn list<P: AsRef<Path>>(path: P) -> OsResult<Vec<PathBuf>> {
    let path = path.as_ref();

    if !path.exists() {
        return Err(OsError::NotFound(path.display().to_string()));
    }

    if !path.is_dir() {
        return Err(OsError::InvalidArgument(
            "Path must be a directory".to_string(),
        ));
    }

    let mut entries = Vec::new();
    let mut dir = fs::read_dir(path).await?;

    while let Some(entry) = dir.next_entry().await? {
        entries.push(entry.path());
    }

    Ok(entries)
}
