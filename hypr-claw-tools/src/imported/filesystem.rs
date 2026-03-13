use crate::error::ToolError;
use crate::execution_context::ExecutionContext;
use crate::sandbox::PathGuard;
use crate::tools::base::{Tool, ToolResult};
use crate::traits::PermissionTier;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tokio::fs;
use tokio::io::AsyncWriteExt;

const MAX_READ_BYTES: usize = 64 * 1024;
const MAX_LIST_ENTRIES: usize = 1000;

#[derive(Clone)]
struct ImportedFs {
    root: String,
}

impl ImportedFs {
    fn new(root: &str) -> Result<Self, ToolError> {
        let _ = PathGuard::new(root)?;
        Ok(Self {
            root: root.to_string(),
        })
    }

    fn guard(&self) -> Result<PathGuard, ToolError> {
        PathGuard::new(&self.root)
    }
}

#[derive(Deserialize)]
struct ReadInput {
    path: String,
    #[serde(default)]
    offset: usize,
    #[serde(default)]
    length: Option<usize>,
}

#[derive(Clone)]
pub struct Fs2ReadTool {
    fs: ImportedFs,
}

impl Fs2ReadTool {
    pub fn new(root: &str) -> Result<Self, ToolError> {
        Ok(Self {
            fs: ImportedFs::new(root)?,
        })
    }
}

#[async_trait]
impl Tool for Fs2ReadTool {
    fn name(&self) -> &'static str {
        "fs2.read"
    }

    fn description(&self) -> &'static str {
        "Read file contents with optional pagination."
    }

    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "offset": {"type": "integer", "minimum": 0},
                "length": {"type": "integer", "minimum": 1}
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        _ctx: ExecutionContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let input: ReadInput =
            serde_json::from_value(input).map_err(|e| ToolError::ValidationError(e.to_string()))?;
        let path = self.fs.guard()?.validate(&input.path)?;
        let content = fs::read(&path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let text = String::from_utf8(content)
            .map_err(|_| ToolError::ValidationError("Invalid UTF-8".into()))?;

        let length = input.length.unwrap_or(MAX_READ_BYTES).min(MAX_READ_BYTES);
        let chars = text.chars().collect::<Vec<_>>();
        if input.offset >= chars.len() {
            return Ok(ToolResult::success(json!({
                "path": input.path,
                "content": "",
                "offset": input.offset,
                "length": 0,
                "has_more": false
            })));
        }

        let end = (input.offset + length).min(chars.len());
        let slice = chars[input.offset..end].iter().collect::<String>();

        Ok(ToolResult::success(json!({
            "path": input.path,
            "content": slice,
            "offset": input.offset,
            "length": end - input.offset,
            "has_more": end < chars.len()
        })))
    }
}

#[derive(Deserialize)]
struct WriteInput {
    path: String,
    content: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Clone)]
pub struct Fs2WriteTool {
    fs: ImportedFs,
}

impl Fs2WriteTool {
    pub fn new(root: &str) -> Result<Self, ToolError> {
        Ok(Self {
            fs: ImportedFs::new(root)?,
        })
    }
}

#[async_trait]
impl Tool for Fs2WriteTool {
    fn name(&self) -> &'static str {
        "fs2.write"
    }

    fn description(&self) -> &'static str {
        "Write file contents atomically."
    }

    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Write
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"},
                "overwrite": {"type": "boolean"}
            },
            "required": ["path", "content"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        _ctx: ExecutionContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let input: WriteInput =
            serde_json::from_value(input).map_err(|e| ToolError::ValidationError(e.to_string()))?;
        let path = self.fs.guard()?.validate_new(&input.path)?;

        if path.exists() && !input.overwrite {
            return Err(ToolError::ValidationError(
                "File exists and overwrite=false".into(),
            ));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        }

        let temp_path = path.with_extension("tmp");
        let mut file = fs::File::create(&temp_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        file.write_all(input.content.as_bytes())
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        file.sync_all()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        fs::rename(&temp_path, &path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(ToolResult::success(json!({"path": input.path, "written": true})))
    }
}

#[derive(Deserialize)]
struct ListInput {
    path: String,
}

#[derive(Clone)]
pub struct Fs2ListTool {
    fs: ImportedFs,
}

impl Fs2ListTool {
    pub fn new(root: &str) -> Result<Self, ToolError> {
        Ok(Self {
            fs: ImportedFs::new(root)?,
        })
    }
}

#[async_trait]
impl Tool for Fs2ListTool {
    fn name(&self) -> &'static str {
        "fs2.list"
    }

    fn description(&self) -> &'static str {
        "List directory contents."
    }

    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        _ctx: ExecutionContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let input: ListInput =
            serde_json::from_value(input).map_err(|e| ToolError::ValidationError(e.to_string()))?;
        let path = self.fs.guard()?.validate(&input.path)?;
        let mut dir = fs::read_dir(&path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let mut entries = Vec::new();
        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
        {
            if entries.len() >= MAX_LIST_ENTRIES {
                break;
            }
            let metadata = entry
                .metadata()
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            entries.push(json!({
                "name": entry.file_name().to_string_lossy().to_string(),
                "is_dir": metadata.is_dir(),
                "size": metadata.len()
            }));
        }

        Ok(ToolResult::success(json!({"path": input.path, "entries": entries})))
    }
}

#[derive(Deserialize)]
struct EditInput {
    path: String,
    old_text: String,
    new_text: String,
}

#[derive(Clone)]
pub struct Fs2EditTool {
    fs: ImportedFs,
}

impl Fs2EditTool {
    pub fn new(root: &str) -> Result<Self, ToolError> {
        Ok(Self {
            fs: ImportedFs::new(root)?,
        })
    }
}

#[async_trait]
impl Tool for Fs2EditTool {
    fn name(&self) -> &'static str {
        "fs2.edit"
    }

    fn description(&self) -> &'static str {
        "Replace exactly one matching string in a file."
    }

    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Write
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "old_text": {"type": "string"},
                "new_text": {"type": "string"}
            },
            "required": ["path", "old_text", "new_text"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        _ctx: ExecutionContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let input: EditInput =
            serde_json::from_value(input).map_err(|e| ToolError::ValidationError(e.to_string()))?;
        let path = self.fs.guard()?.validate(&input.path)?;
        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        if !content.contains(&input.old_text) {
            return Err(ToolError::ValidationError(
                "old_text not found in file".into(),
            ));
        }
        if content.matches(&input.old_text).count() > 1 {
            return Err(ToolError::ValidationError(
                "old_text appears multiple times".into(),
            ));
        }

        let updated = content.replacen(&input.old_text, &input.new_text, 1);
        write_atomic(&path, updated.as_bytes()).await?;

        Ok(ToolResult::success(json!({"path": input.path, "edited": true})))
    }
}

#[derive(Deserialize)]
struct AppendInput {
    path: String,
    content: String,
}

#[derive(Clone)]
pub struct Fs2AppendTool {
    fs: ImportedFs,
}

impl Fs2AppendTool {
    pub fn new(root: &str) -> Result<Self, ToolError> {
        Ok(Self {
            fs: ImportedFs::new(root)?,
        })
    }
}

#[async_trait]
impl Tool for Fs2AppendTool {
    fn name(&self) -> &'static str {
        "fs2.append"
    }

    fn description(&self) -> &'static str {
        "Append content to the end of a file."
    }

    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Write
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path", "content"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        _ctx: ExecutionContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let input: AppendInput =
            serde_json::from_value(input).map_err(|e| ToolError::ValidationError(e.to_string()))?;
        let guard = self.fs.guard()?;
        let path = if guard.validate(&input.path).is_ok() {
            guard.validate(&input.path)?
        } else {
            guard.validate_new(&input.path)?
        };

        let existing = fs::read(&path).await.unwrap_or_default();
        let mut updated = existing;
        updated.extend_from_slice(input.content.as_bytes());
        write_atomic(&path, &updated).await?;

        Ok(ToolResult::success(json!({"path": input.path, "appended": true})))
    }
}

async fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<(), ToolError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    }
    let temp_path = path.with_extension("tmp");
    let mut file = fs::File::create(&temp_path)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    file.write_all(bytes)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    file.sync_all()
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    fs::rename(&temp_path, path)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn test_ctx() -> ExecutionContext {
        ExecutionContext::new("test-session".to_string(), 5_000)
    }

    #[tokio::test]
    async fn fs2_read_supports_pagination() {
        let sandbox = tempdir().unwrap();
        fs::write(sandbox.path().join("note.txt"), "abcdefghijklmnopqrstuvwxyz")
            .await
            .unwrap();
        let tool = Fs2ReadTool::new(sandbox.path().to_str().unwrap()).unwrap();

        let result = tool
            .execute(
                test_ctx(),
                json!({"path": "note.txt", "offset": 5, "length": 4}),
            )
            .await
            .unwrap();

        assert_eq!(result.output.unwrap()["content"], "fghi");
    }

    #[tokio::test]
    async fn fs2_edit_requires_unique_match() {
        let sandbox = tempdir().unwrap();
        fs::write(sandbox.path().join("note.txt"), "same same").await.unwrap();
        let tool = Fs2EditTool::new(sandbox.path().to_str().unwrap()).unwrap();

        let error = tool
            .execute(
                test_ctx(),
                json!({"path": "note.txt", "old_text": "same", "new_text": "diff"}),
            )
            .await
            .unwrap_err();

        assert!(matches!(error, ToolError::ValidationError(_)));
    }

    #[tokio::test]
    async fn fs2_append_extends_existing_file() {
        let sandbox = tempdir().unwrap();
        fs::write(sandbox.path().join("note.txt"), "hello").await.unwrap();
        let tool = Fs2AppendTool::new(sandbox.path().to_str().unwrap()).unwrap();

        tool.execute(
            test_ctx(),
            json!({"path": "note.txt", "content": " world"}),
        )
        .await
        .unwrap();

        let content = fs::read_to_string(sandbox.path().join("note.txt"))
            .await
            .unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn fs2_rejects_path_traversal() {
        let sandbox = tempdir().unwrap();
        let tool = Fs2ReadTool::new(sandbox.path().to_str().unwrap()).unwrap();

        let error = tool
            .execute(test_ctx(), json!({"path": "../secret.txt"}))
            .await
            .unwrap_err();

        assert!(matches!(error, ToolError::SandboxViolation(_)));
    }
}
