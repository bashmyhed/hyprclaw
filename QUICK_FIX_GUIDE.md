# Quick Fix Implementation Guide

## Immediate Actions (Do This First)

### 1. Add Debug Logging

**File**: `hypr-claw-runtime/src/agent_loop.rs`

Find the tool execution section (around line 330) and add:

```rust
LLMResponse::ToolCall { tool_name, input, .. } => {
    // ADD THIS BLOCK
    info!("🔧 TOOL CALL DEBUG:");
    info!("  Tool: '{}'", tool_name);
    info!("  Input: {}", serde_json::to_string_pretty(&input).unwrap_or_else(|_| "{}".to_string()));
    info!("  Session: {}", session_key);
    info!("  Iteration: {}/{}", iteration + 1, max_iterations);
    // END ADD
    
    saw_tool_call = true;
    // ... rest of existing code
```

After tool execution (around line 360), add:

```rust
let tool_result = match self.tool_dispatcher.execute(&tool_name, &input, session_key).await {
    Ok(result) => {
        // ADD THIS
        info!("✅ TOOL SUCCESS: {:?}", result);
        // END ADD
        result
    },
    Err(e) => {
        // ADD THIS
        error!("❌ TOOL FAILURE: {}", e);
        // END ADD
        warn!("Tool execution failed: {}", e);
        // ... rest of existing code
```

### 2. Improve Error Messages

**File**: `hypr-claw-tools/src/os_tools.rs`

Replace the `DesktopMouseClickTool::execute` method (around line 746):

```rust
async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
    // Better error handling for missing button
    let button = match required_str(&input, "button") {
        Ok(b) => b,
        Err(_) => {
            return Ok(ToolResult {
                success: false,
                output: None,
                error: Some(
                    "Missing required parameter 'button'. Must be 'left', 'middle', or 'right'. \
                     Example: {\"button\": \"left\"}".to_string()
                ),
                ..ToolResult::default()
            });
        }
    };
    
    // Execute with better error messages
    match desktop::mouse_click(button).await {
        Ok(_) => Ok(ToolResult {
            success: true,
            output: Some(json!({
                "clicked": button,
                "message": "Mouse click executed successfully"
            })),
            error: None,
            ..ToolResult::default()
        }),
        Err(e) => Ok(ToolResult {
            success: false,
            output: None,
            error: Some(format!(
                "Failed to execute mouse click: {}. \n\
                 Ensure 'ydotool' or 'wlrctl' is installed:\n\
                 - Arch: sudo pacman -S ydotool\n\
                 - Ubuntu: sudo apt install ydotool\n\
                 - Enable service: sudo systemctl enable --now ydotoold",
                e
            )),
            ..ToolResult::default()
        })
    }
}
```

### 3. Strengthen System Prompt

**File**: `hypr-claw-runtime/src/agent_loop.rs`

Find the `reinforced_prompt` creation (around line 252) and replace with:

```rust
let reinforced_prompt = if action_requires_tool && !tool_names.is_empty() {
    format!(
        "{}\n\n\
        ═══════════════════════════════════════════════════════════════\n\
        🤖 CRITICAL: You are a LOCAL LINUX AGENT with DIRECT OS CONTROL\n\
        ═══════════════════════════════════════════════════════════════\n\n\
        YOU MUST USE TOOLS - Do NOT just describe actions!\n\n\
        Available Tools:\n{}\n\n\
        TOOL CALL FORMAT (IMPORTANT):\n\
        - Use exact tool names from the list above\n\
        - Provide ALL required parameters\n\
        - Use proper JSON structure\n\n\
        MOUSE CLICK EXAMPLE:\n\
        Tool: desktop.mouse_click\n\
        Required: button (must be 'left', 'middle', or 'right')\n\
        Correct: {{\"button\": \"left\"}}\n\
        Wrong: {{}} or {{\"click\": \"left\"}} or missing parameter\n\n\
        When user asks to click, IMMEDIATELY call desktop.mouse_click with proper button parameter.",
        system_prompt,
        tool_names.iter().map(|t| format!("  • {}", t)).collect::<Vec<_>>().join("\n")
    )
} else {
    system_prompt.to_string()
};
```

### 4. Add Tool Call Normalization

**File**: `hypr-claw-runtime/src/types.rs`

Add this method to the `LLMResponse` impl block (after line 90):

```rust
impl LLMResponse {
    // ... existing methods ...
    
    /// Normalize tool call to ensure consistent structure
    pub fn normalize(self) -> Self {
        match self {
            LLMResponse::ToolCall { schema_version, tool_name, mut input } => {
                // Ensure input is an object
                if !input.is_object() {
                    tracing::warn!("Tool call input is not an object, converting to empty object");
                    input = serde_json::json!({});
                }
                
                // Trim and validate tool name
                let tool_name = tool_name.trim().to_string();
                if tool_name.is_empty() {
                    tracing::error!("Tool call has empty tool name after normalization");
                }
                
                LLMResponse::ToolCall {
                    schema_version,
                    tool_name,
                    input,
                }
            }
            other => other,
        }
    }
}
```

**File**: `hypr-claw-runtime/src/llm_client.rs`

Find where `llm_response` is created (around line 640) and add normalization:

```rust
let llm_response = /* ... existing parsing code ... */;

// ADD THIS LINE
let llm_response = llm_response.normalize();

self.validate_response(&llm_response)?;
Ok(llm_response)
```

## Testing

### 1. Check Backend Installation

```bash
# Check if ydotool is installed
which ydotool

# If not installed:
# Arch Linux:
sudo pacman -S ydotool

# Ubuntu/Debian:
sudo apt install ydotool

# Enable the service:
sudo systemctl enable --now ydotoold

# Test manual click:
ydotool click 1
```

### 2. Run with Debug Logging

```bash
cd /home/bigfoot/hyprclaw
RUST_LOG=debug cargo run
```

### 3. Test Click Commands

Try these variations:
```
> click left mouse button
> perform a left click
> click the mouse
> left click
```

### 4. Check Logs

Look for these patterns in the output:

**Good**:
```
🔧 TOOL CALL DEBUG:
  Tool: 'desktop.mouse_click'
  Input: {
    "button": "left"
  }
✅ TOOL SUCCESS: ToolResult { success: true, ... }
```

**Bad (empty tool call)**:
```
🔧 TOOL CALL DEBUG:
  Tool: ''
  Input: {}
```

**Bad (wrong parameter)**:
```
🔧 TOOL CALL DEBUG:
  Tool: 'desktop.mouse_click'
  Input: {}
❌ TOOL FAILURE: Missing required parameter 'button'
```

## Troubleshooting

### Issue: "No click backend found"

**Solution**:
```bash
# Install ydotool
sudo pacman -S ydotool  # Arch
sudo apt install ydotool  # Ubuntu

# Start the service
sudo systemctl enable --now ydotoold

# Add your user to input group (may be needed)
sudo usermod -aG input $USER
# Log out and back in
```

### Issue: "Permission denied"

**Solution**:
```bash
# Check if ydotoold service is running
systemctl status ydotoold

# If not running:
sudo systemctl start ydotoold

# Check permissions
ls -l /dev/uinput
# Should show: crw-rw---- 1 root input

# Ensure you're in input group
groups | grep input
```

### Issue: Tool call has empty tool_name

**Cause**: LLM not generating proper tool calls

**Solution**:
1. Check if tools are registered: Look for "Available tools:" in logs
2. Verify system prompt includes tool instructions
3. Try different phrasing: "use desktop.mouse_click tool with left button"
4. Check LLM model supports function calling

### Issue: Tool call missing button parameter

**Cause**: LLM not including required parameters

**Solution**:
1. System prompt now includes explicit example
2. Try being more specific: "click left button" instead of just "click"
3. Check tool schema is being sent to LLM (look for "tool_count" in logs)

## Verification Checklist

After implementing fixes:

- [ ] Debug logs show tool calls with non-empty tool_name
- [ ] Debug logs show tool calls with proper input parameters
- [ ] Error messages are clear and actionable
- [ ] System prompt includes tool usage instructions
- [ ] Tool normalization prevents empty/malformed calls
- [ ] Backend (ydotool/wlrctl) is installed and accessible
- [ ] Manual test: `ydotool click 1` works
- [ ] Agent test: "click left mouse button" executes successfully

## Next Steps

If clicking still doesn't work after these fixes:

1. **Capture full debug log** and check:
   - Is the tool registered? (search for "desktop.mouse_click" in tool list)
   - Is the LLM generating tool calls? (search for "🔧 TOOL CALL DEBUG")
   - What's the exact error? (search for "❌ TOOL FAILURE")

2. **Test with explicit tool name**:
   ```
   > use the desktop.mouse_click tool with button parameter set to left
   ```

3. **Check LLM provider**:
   - Some models don't support function calling well
   - Try a different model if available
   - Check if provider requires specific tool format

4. **Review full analysis**: See `CLICK_RELIABILITY_ANALYSIS.md` for deeper investigation

## Summary

These changes add:
1. **Visibility**: Debug logs show exactly what's happening
2. **Robustness**: Normalization prevents malformed tool calls
3. **Clarity**: Better error messages guide troubleshooting
4. **Guidance**: Stronger system prompt teaches LLM proper tool usage

The click implementation itself is solid - these fixes address the invocation layer.
