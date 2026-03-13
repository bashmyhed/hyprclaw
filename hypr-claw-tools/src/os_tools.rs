//! Structured OS capability tool wrappers.

use crate::error::ToolError;
use crate::execution_context::ExecutionContext;
use crate::os_capabilities::{desktop, filesystem, hyprland, process, system};
use crate::tools::base::{Tool, ToolResult};
use crate::traits::PermissionTier;
use async_trait::async_trait;
use serde_json::{json, Value};

fn required_str<'a>(input: &'a Value, field: &str) -> Result<&'a str, ToolError> {
    input[field]
        .as_str()
        .ok_or_else(|| ToolError::ValidationError(format!("Missing or invalid '{field}'")))
}

fn required_u32(input: &Value, field: &str) -> Result<u32, ToolError> {
    let n = input[field]
        .as_u64()
        .ok_or_else(|| ToolError::ValidationError(format!("Missing or invalid '{field}'")))?;
    u32::try_from(n).map_err(|_| ToolError::ValidationError(format!("'{field}' out of range")))
}

pub struct FsCreateDirTool;
pub struct FsDeleteTool;
pub struct FsMoveTool;
pub struct FsCopyTool;
pub struct FsReadTool;
pub struct FsWriteTool;
pub struct FsListTool;

#[async_trait]
impl Tool for FsCreateDirTool {
    fn name(&self) -> &'static str {
        "fs.create_dir"
    }
    fn description(&self) -> &'static str {
        "Create a directory at path"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Write
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "path": {"type": "string"} },
            "required": ["path"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let path = required_str(&input, "path")?;
        filesystem::create_dir(path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"created": path})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for FsDeleteTool {
    fn name(&self) -> &'static str {
        "fs.delete"
    }
    fn description(&self) -> &'static str {
        "Delete a file or directory"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Write
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "path": {"type": "string"} },
            "required": ["path"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let path = required_str(&input, "path")?;
        filesystem::delete(path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"deleted": path})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for FsMoveTool {
    fn name(&self) -> &'static str {
        "fs.move"
    }
    fn description(&self) -> &'static str {
        "Move/rename a file or directory"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Write
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "from": {"type": "string"},
                "to": {"type": "string"}
            },
            "required": ["from", "to"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let from = required_str(&input, "from")?;
        let to = required_str(&input, "to")?;
        filesystem::move_path(from, to)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"from": from, "to": to})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for FsCopyTool {
    fn name(&self) -> &'static str {
        "fs.copy"
    }
    fn description(&self) -> &'static str {
        "Copy a file"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Write
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "from": {"type": "string"},
                "to": {"type": "string"}
            },
            "required": ["from", "to"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let from = required_str(&input, "from")?;
        let to = required_str(&input, "to")?;
        filesystem::copy_file(from, to)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"from": from, "to": to})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for FsReadTool {
    fn name(&self) -> &'static str {
        "fs.read"
    }
    fn description(&self) -> &'static str {
        "Read file contents"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "path": {"type": "string"} },
            "required": ["path"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let path = required_str(&input, "path")?;
        let content = filesystem::read(path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"path": path, "content": content})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for FsWriteTool {
    fn name(&self) -> &'static str {
        "fs.write"
    }
    fn description(&self) -> &'static str {
        "Write file contents"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Write
    }
    fn schema(&self) -> Value {
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
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let path = required_str(&input, "path")?;
        let content = required_str(&input, "content")?;
        filesystem::write(path, content)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"written": path})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for FsListTool {
    fn name(&self) -> &'static str {
        "fs.list"
    }
    fn description(&self) -> &'static str {
        "List directory contents"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "path": {"type": "string"} },
            "required": ["path"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let path = required_str(&input, "path")?;
        let entries = filesystem::list(path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let entries: Vec<String> = entries
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        Ok(ToolResult {
            success: true,
            output: Some(json!({"path": path, "entries": entries})),
            error: None,
        })
    }
}

pub struct ProcSpawnTool;
pub struct ProcKillTool;
pub struct ProcListTool;
pub struct DesktopOpenUrlTool;
pub struct DesktopLaunchAppTool;
pub struct DesktopLaunchAppAndWaitTextTool;
pub struct DesktopSearchWebTool;
pub struct DesktopOpenGmailTool;
pub struct DesktopTypeTextTool;
pub struct DesktopKeyPressTool;
pub struct DesktopKeyComboTool;
pub struct DesktopMouseClickTool;
pub struct DesktopCaptureScreenTool;
pub struct DesktopActiveWindowTool;
pub struct DesktopListWindowsTool;
pub struct DesktopMouseMoveTool;
pub struct DesktopClickAtTool;
pub struct DesktopOcrScreenTool;
pub struct DesktopFindTextTool;
pub struct DesktopClickTextTool;
pub struct DesktopWaitForTextTool;
pub struct DesktopCursorPositionTool;
pub struct DesktopMouseMoveAndVerifyTool;
pub struct DesktopClickAtAndVerifyTool;
pub struct DesktopReadScreenStateTool;

#[async_trait]
impl Tool for ProcSpawnTool {
    fn name(&self) -> &'static str {
        "proc.spawn"
    }
    fn description(&self) -> &'static str {
        "Spawn a process"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "args": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let command = required_str(&input, "command")?;
        let args_vec: Vec<String> = input["args"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let arg_refs: Vec<&str> = args_vec.iter().map(String::as_str).collect();
        let pid = process::spawn(command, &arg_refs)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"pid": pid})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for ProcKillTool {
    fn name(&self) -> &'static str {
        "proc.kill"
    }
    fn description(&self) -> &'static str {
        "Kill a process by PID"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "pid": {"type": "number"} },
            "required": ["pid"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let pid = required_u32(&input, "pid")?;
        process::kill(pid)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"killed": pid})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for ProcListTool {
    fn name(&self) -> &'static str {
        "proc.list"
    }
    fn description(&self) -> &'static str {
        "List running processes"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {"type": "number"}
            },
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let limit = input["limit"].as_u64().unwrap_or(25) as usize;
        let mut procs = process::list()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        procs.truncate(limit);
        Ok(ToolResult {
            success: true,
            output: Some(json!({"processes": procs})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopOpenUrlTool {
    fn name(&self) -> &'static str {
        "desktop.open_url"
    }
    fn description(&self) -> &'static str {
        "Open a URL in the default desktop browser/application"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {"type": "string"}
            },
            "required": ["url"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let url = required_str(&input, "url")?;
        desktop::open_url(url)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"opened": url})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopLaunchAppTool {
    fn name(&self) -> &'static str {
        "desktop.launch_app"
    }
    fn description(&self) -> &'static str {
        "Launch a desktop application"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app": {"type": "string"},
                "args": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["app"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let app = required_str(&input, "app")?;
        let args: Vec<String> = input["args"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let pid = desktop::launch_app(app, &args)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"app": app, "pid": pid})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopLaunchAppAndWaitTextTool {
    fn name(&self) -> &'static str {
        "desktop.launch_app_and_wait_text"
    }
    fn description(&self) -> &'static str {
        "Launch an app, then wait until text appears on screen"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app": {"type": "string"},
                "args": {"type": "array", "items": {"type": "string"}},
                "query": {"type": "string"},
                "timeout_ms": {"type": "number"},
                "poll_interval_ms": {"type": "number"},
                "case_sensitive": {"type": "boolean"},
                "lang": {"type": "string"}
            },
            "required": ["app", "query"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let app = required_str(&input, "app")?;
        let query = required_str(&input, "query")?;
        let args: Vec<String> = input["args"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(10_000);
        let poll_interval_ms = input["poll_interval_ms"].as_u64().unwrap_or(500);
        let case_sensitive = input["case_sensitive"].as_bool().unwrap_or(false);
        let lang = input["lang"].as_str();

        let pid = desktop::launch_app(app, &args)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let found =
            desktop::wait_for_text(query, case_sensitive, timeout_ms, poll_interval_ms, lang)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(ToolResult {
            success: true,
            output: Some(json!({"app": app, "pid": pid, "query": query, "match": found})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopSearchWebTool {
    fn name(&self) -> &'static str {
        "desktop.search_web"
    }
    fn description(&self) -> &'static str {
        "Search the web in default browser"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "engine": {"type": "string"}
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let query = required_str(&input, "query")?;
        let engine = input["engine"].as_str();
        let url = desktop::search_web(query, engine)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"query": query, "url": url})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopOpenGmailTool {
    fn name(&self) -> &'static str {
        "desktop.open_gmail"
    }
    fn description(&self) -> &'static str {
        "Open Gmail in default browser"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }
    async fn execute(
        &self,
        _ctx: ExecutionContext,
        _input: Value,
    ) -> Result<ToolResult, ToolError> {
        desktop::open_gmail()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"opened": "https://mail.google.com"})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopTypeTextTool {
    fn name(&self) -> &'static str {
        "desktop.type_text"
    }
    fn description(&self) -> &'static str {
        "Type text into the currently focused window"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "text": {"type": "string"} },
            "required": ["text"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let text = required_str(&input, "text")?;
        desktop::type_text(text)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"typed": text.len()})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopKeyPressTool {
    fn name(&self) -> &'static str {
        "desktop.key_press"
    }
    fn description(&self) -> &'static str {
        "Press one key in the focused window (e.g. Return, Escape)"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "key": {"type": "string"} },
            "required": ["key"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let key = required_str(&input, "key")?;
        desktop::key_press(key)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"pressed": key})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopKeyComboTool {
    fn name(&self) -> &'static str {
        "desktop.key_combo"
    }
    fn description(&self) -> &'static str {
        "Press a key combo in focused window, e.g. [\"ctrl\", \"l\"]"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "keys": {"type": "array", "items": {"type": "string"}, "minItems": 2}
            },
            "required": ["keys"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let keys: Vec<String> = input["keys"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if keys.len() < 2 {
            return Err(ToolError::ValidationError(
                "keys must contain at least one modifier and one key".to_string(),
            ));
        }
        desktop::key_combo(&keys)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"combo": keys})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopMouseClickTool {
    fn name(&self) -> &'static str {
        "desktop.mouse_click"
    }
    fn description(&self) -> &'static str {
        "Click mouse button at current cursor position"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "button": {"type": "string", "enum": ["left", "middle", "right"]}
            },
            "required": ["button"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let button = required_str(&input, "button")?;
        desktop::mouse_click(button)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"clicked": button})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopCaptureScreenTool {
    fn name(&self) -> &'static str {
        "desktop.capture_screen"
    }
    fn description(&self) -> &'static str {
        "Capture screenshot to path and return saved file"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "path": {"type": "string"} },
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let path = input["path"].as_str();
        let saved = desktop::capture_screen(path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"path": saved})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopActiveWindowTool {
    fn name(&self) -> &'static str {
        "desktop.active_window"
    }
    fn description(&self) -> &'static str {
        "Get currently active Hyprland window metadata"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }
    async fn execute(
        &self,
        _ctx: ExecutionContext,
        _input: Value,
    ) -> Result<ToolResult, ToolError> {
        let window = desktop::active_window()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"window": window})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopListWindowsTool {
    fn name(&self) -> &'static str {
        "desktop.list_windows"
    }
    fn description(&self) -> &'static str {
        "List Hyprland client windows metadata"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "limit": {"type": "number"} },
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let limit = input["limit"].as_u64().unwrap_or(50) as usize;
        let windows = desktop::list_windows(limit)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"windows": windows})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopMouseMoveTool {
    fn name(&self) -> &'static str {
        "desktop.mouse_move"
    }
    fn description(&self) -> &'static str {
        "Move mouse cursor to absolute coordinate"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "x": {"type": "number"},
                "y": {"type": "number"}
            },
            "required": ["x", "y"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let x = required_u32(&input, "x")? as i32;
        let y = required_u32(&input, "y")? as i32;
        desktop::mouse_move_absolute(x, y)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"x": x, "y": y})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopClickAtTool {
    fn name(&self) -> &'static str {
        "desktop.click_at"
    }
    fn description(&self) -> &'static str {
        "Click at absolute coordinate"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "x": {"type": "number"},
                "y": {"type": "number"},
                "button": {"type": "string", "enum": ["left", "middle", "right"]}
            },
            "required": ["x", "y"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let x = required_u32(&input, "x")? as i32;
        let y = required_u32(&input, "y")? as i32;
        let button = input["button"].as_str().unwrap_or("left");
        desktop::click_at(x, y, button)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"x": x, "y": y, "button": button})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopOcrScreenTool {
    fn name(&self) -> &'static str {
        "desktop.ocr_screen"
    }
    fn description(&self) -> &'static str {
        "Run OCR on current screen and return recognized text and boxes"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "lang": {"type": "string"}
            },
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let path = input["path"].as_str();
        let lang = input["lang"].as_str();
        let (text, words) = desktop::ocr_screen(path, lang)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"text": text, "matches": words})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopFindTextTool {
    fn name(&self) -> &'static str {
        "desktop.find_text"
    }
    fn description(&self) -> &'static str {
        "Find text on screen using OCR"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "case_sensitive": {"type": "boolean"},
                "limit": {"type": "number"},
                "lang": {"type": "string"}
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let query = required_str(&input, "query")?;
        let case_sensitive = input["case_sensitive"].as_bool().unwrap_or(false);
        let limit = input["limit"].as_u64().unwrap_or(20) as usize;
        let lang = input["lang"].as_str();
        let matches = desktop::find_text(query, case_sensitive, limit, lang)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"query": query, "matches": matches})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopClickTextTool {
    fn name(&self) -> &'static str {
        "desktop.click_text"
    }
    fn description(&self) -> &'static str {
        "Find text on screen and click it"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "occurrence": {"type": "number"},
                "button": {"type": "string", "enum": ["left", "middle", "right"]},
                "case_sensitive": {"type": "boolean"},
                "lang": {"type": "string"}
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let query = required_str(&input, "query")?;
        let occurrence = input["occurrence"].as_u64().unwrap_or(0) as usize;
        let button = input["button"].as_str().unwrap_or("left");
        let case_sensitive = input["case_sensitive"].as_bool().unwrap_or(false);
        let lang = input["lang"].as_str();
        let target = desktop::click_text(query, occurrence, button, case_sensitive, lang)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"query": query, "target": target, "button": button})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopWaitForTextTool {
    fn name(&self) -> &'static str {
        "desktop.wait_for_text"
    }
    fn description(&self) -> &'static str {
        "Wait until target text appears on screen using OCR"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "timeout_ms": {"type": "number"},
                "poll_interval_ms": {"type": "number"},
                "case_sensitive": {"type": "boolean"},
                "lang": {"type": "string"}
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let query = required_str(&input, "query")?;
        let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(10_000);
        let poll_interval_ms = input["poll_interval_ms"].as_u64().unwrap_or(500);
        let case_sensitive = input["case_sensitive"].as_bool().unwrap_or(false);
        let lang = input["lang"].as_str();

        let found =
            desktop::wait_for_text(query, case_sensitive, timeout_ms, poll_interval_ms, lang)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"query": query, "match": found})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopCursorPositionTool {
    fn name(&self) -> &'static str {
        "desktop.cursor_position"
    }
    fn description(&self) -> &'static str {
        "Get current cursor position using compositor metadata"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }
    async fn execute(
        &self,
        _ctx: ExecutionContext,
        _input: Value,
    ) -> Result<ToolResult, ToolError> {
        let (x, y) = desktop::cursor_position()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"x": x, "y": y})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopMouseMoveAndVerifyTool {
    fn name(&self) -> &'static str {
        "desktop.mouse_move_and_verify"
    }
    fn description(&self) -> &'static str {
        "Move cursor to coordinate and verify final cursor position"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "x": {"type": "number"},
                "y": {"type": "number"},
                "tolerance": {"type": "number"},
                "timeout_ms": {"type": "number"}
            },
            "required": ["x", "y"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let x = required_u32(&input, "x")? as i32;
        let y = required_u32(&input, "y")? as i32;
        let tolerance = input["tolerance"].as_i64().unwrap_or(8) as i32;
        let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(900);
        let (vx, vy) = desktop::mouse_move_and_verify(x, y, tolerance, timeout_ms)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"target": {"x": x, "y": y}, "verified": {"x": vx, "y": vy}})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopClickAtAndVerifyTool {
    fn name(&self) -> &'static str {
        "desktop.click_at_and_verify"
    }
    fn description(&self) -> &'static str {
        "Move cursor, click at coordinate, and return pre/post cursor verification"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "x": {"type": "number"},
                "y": {"type": "number"},
                "button": {"type": "string", "enum": ["left", "middle", "right"]},
                "tolerance": {"type": "number"},
                "timeout_ms": {"type": "number"}
            },
            "required": ["x", "y"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let x = required_u32(&input, "x")? as i32;
        let y = required_u32(&input, "y")? as i32;
        let button = input["button"].as_str().unwrap_or("left");
        let tolerance = input["tolerance"].as_i64().unwrap_or(8) as i32;
        let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(900);
        let result = desktop::click_at_and_verify(x, y, button, tolerance, timeout_ms)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(result),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for DesktopReadScreenStateTool {
    fn name(&self) -> &'static str {
        "desktop.read_screen_state"
    }
    fn description(&self) -> &'static str {
        "Read screen state (windows/cursor/screenshot and optional OCR) in one call"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "include_ocr": {"type": "boolean"},
                "include_windows": {"type": "boolean"},
                "include_cursor": {"type": "boolean"},
                "include_screenshot": {"type": "boolean"},
                "lang": {"type": "string"},
                "window_limit": {"type": "number"},
                "max_ocr_matches": {"type": "number"}
            },
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let include_ocr = input["include_ocr"].as_bool().unwrap_or(true);
        let include_windows = input["include_windows"].as_bool().unwrap_or(true);
        let include_cursor = input["include_cursor"].as_bool().unwrap_or(true);
        let include_screenshot = input["include_screenshot"].as_bool().unwrap_or(true);
        let lang = input["lang"].as_str();
        let window_limit = input["window_limit"].as_u64().unwrap_or(30) as usize;
        let max_ocr_matches = input["max_ocr_matches"].as_u64().unwrap_or(240) as usize;

        let state = desktop::read_screen_state(
            include_ocr,
            include_windows,
            include_cursor,
            include_screenshot,
            lang,
            window_limit,
            max_ocr_matches,
        )
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(ToolResult {
            success: true,
            output: Some(state),
            error: None,
        })
    }
}

pub struct HyprWorkspaceSwitchTool;
pub struct HyprWorkspaceMoveWindowTool;
pub struct HyprWindowFocusTool;
pub struct HyprWindowCloseTool;
pub struct HyprWindowMoveTool;
pub struct HyprExecTool;

#[async_trait]
impl Tool for HyprWorkspaceSwitchTool {
    fn name(&self) -> &'static str {
        "hypr.workspace.switch"
    }
    fn description(&self) -> &'static str {
        "Switch active workspace in Hyprland"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "workspace_id": {"type": "number"} },
            "required": ["workspace_id"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let workspace_id = required_u32(&input, "workspace_id")?;
        hyprland::workspace_switch(workspace_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"workspace": workspace_id})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for HyprWorkspaceMoveWindowTool {
    fn name(&self) -> &'static str {
        "hypr.workspace.move_window"
    }
    fn description(&self) -> &'static str {
        "Move window to another workspace"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "window_id": {"type": "string"},
                "workspace_id": {"type": "number"}
            },
            "required": ["window_id", "workspace_id"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let window_id = required_str(&input, "window_id")?;
        let workspace_id = required_u32(&input, "workspace_id")?;
        hyprland::workspace_move_window(window_id, workspace_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"window_id": window_id, "workspace_id": workspace_id})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for HyprWindowFocusTool {
    fn name(&self) -> &'static str {
        "hypr.window.focus"
    }
    fn description(&self) -> &'static str {
        "Focus a window"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "window_id": {"type": "string"} },
            "required": ["window_id"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let window_id = required_str(&input, "window_id")?;
        hyprland::window_focus(window_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"focused": window_id})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for HyprWindowCloseTool {
    fn name(&self) -> &'static str {
        "hypr.window.close"
    }
    fn description(&self) -> &'static str {
        "Close a window"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "window_id": {"type": "string"} },
            "required": ["window_id"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let window_id = required_str(&input, "window_id")?;
        hyprland::window_close(window_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"closed": window_id})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for HyprWindowMoveTool {
    fn name(&self) -> &'static str {
        "hypr.window.move"
    }
    fn description(&self) -> &'static str {
        "Move a window to workspace"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "window_id": {"type": "string"},
                "workspace_id": {"type": "number"}
            },
            "required": ["window_id", "workspace_id"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let window_id = required_str(&input, "window_id")?;
        let workspace_id = required_u32(&input, "workspace_id")?;
        hyprland::window_move(window_id, workspace_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"window_id": window_id, "workspace_id": workspace_id})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for HyprExecTool {
    fn name(&self) -> &'static str {
        "hypr.exec"
    }
    fn description(&self) -> &'static str {
        "Execute a command via Hyprland dispatcher"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "command": {"type": "string"} },
            "required": ["command"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let command = required_str(&input, "command")?;
        hyprland::exec(command)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"executed": command})),
            error: None,
        })
    }
}

pub struct WallpaperSetTool;
pub struct SystemShutdownTool;
pub struct SystemRebootTool;
pub struct SystemBatteryTool;
pub struct SystemMemoryTool;

#[async_trait]
impl Tool for WallpaperSetTool {
    fn name(&self) -> &'static str {
        "wallpaper.set"
    }
    fn description(&self) -> &'static str {
        "Set desktop wallpaper"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Write
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "image_path": {"type": "string"} },
            "required": ["image_path"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
        let image_path = required_str(&input, "image_path")?;
        system::wallpaper_set(image_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"wallpaper": image_path})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for SystemShutdownTool {
    fn name(&self) -> &'static str {
        "system.shutdown"
    }
    fn description(&self) -> &'static str {
        "Shutdown the system"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::SystemCritical
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }
    async fn execute(
        &self,
        _ctx: ExecutionContext,
        _input: Value,
    ) -> Result<ToolResult, ToolError> {
        system::shutdown()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"shutdown": true})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for SystemRebootTool {
    fn name(&self) -> &'static str {
        "system.reboot"
    }
    fn description(&self) -> &'static str {
        "Reboot the system"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::SystemCritical
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }
    async fn execute(
        &self,
        _ctx: ExecutionContext,
        _input: Value,
    ) -> Result<ToolResult, ToolError> {
        system::reboot()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"reboot": true})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for SystemBatteryTool {
    fn name(&self) -> &'static str {
        "system.battery"
    }
    fn description(&self) -> &'static str {
        "Get battery percentage"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }
    async fn execute(
        &self,
        _ctx: ExecutionContext,
        _input: Value,
    ) -> Result<ToolResult, ToolError> {
        let percent = system::battery_level()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"battery_percent": percent})),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for SystemMemoryTool {
    fn name(&self) -> &'static str {
        "system.memory"
    }
    fn description(&self) -> &'static str {
        "Get memory usage information"
    }
    fn permission_tier(&self) -> PermissionTier {
        PermissionTier::Read
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }
    async fn execute(
        &self,
        _ctx: ExecutionContext,
        _input: Value,
    ) -> Result<ToolResult, ToolError> {
        let info = system::memory_info()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult {
            success: true,
            output: Some(json!({"memory": info})),
            error: None,
        })
    }
}
