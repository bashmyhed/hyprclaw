# Click Reliability Analysis & Implementation Plan

## Executive Summary

**Problem**: Clicking functionality in hyprclaw is returning empty tool calls and not working reliably.

**Root Cause Analysis**: After comprehensive codebase review, the issue is NOT with the click implementation itself (which is solid), but with how the LLM is invoking tools and how tool calls are being parsed/validated.

**Key Finding**: Picoclaw doesn't have native desktop automation tools - they likely use MCP (Model Context Protocol) servers for desktop control. Hyprclaw has superior native desktop automation but suffers from tool invocation reliability issues.

---

## Current State Analysis

### Hyprclaw Click Implementation (SOLID)

**Location**: `hypr-claw-tools/src/os_capabilities/desktop.rs`

```rust
pub async fn mouse_click(button: &str) -> OsResult<()> {
    let code = parse_mouse_button(button)?;
    if command_exists("ydotool").await {
        return run_checked("ydotool", &["click", code]).await;
    }
    if command_exists("wlrctl").await {
        return run_checked("wlrctl", &["pointer", "click", button]).await;
    }
    Err(OsError::OperationFailed(
        "No click backend found (install 'ydotool' or 'wlrctl')".to_string(),
    ))
}
```

**Strengths**:
1. Proper fallback chain: ydotool → wlrctl
2. Button validation (left/middle/right → 1/2/3)
3. Clear error messages
4. Async execution with proper error handling

**Tool Registration**: `hypr-claw-app/src/tool_setup.rs`
```rust
registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopMouseClickTool));
```

**Tool Schema**: `hypr-claw-tools/src/os_tools.rs`
```rust
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
```

### Picoclaw Approach (MCP-Based)

**Key Discovery**: Picoclaw has NO native mouse/click tools in `pkg/tools/`. Instead:

1. **MCP Integration**: `pkg/tools/mcp_tool.go` - wraps external MCP server tools
2. **Tool Registry**: Dynamic tool promotion with TTL (Time-To-Live)
3. **Hidden Tools**: Tools are discovered and promoted temporarily

**Architecture**:
```
User Request → Agent Loop → Tool Registry → MCP Manager → External MCP Server
                                                              ↓
                                                         Desktop Control
```

**Picoclaw Tool Execution** (`pkg/tools/toolloop.go`):
```go
// 7. Execute tool calls in parallel
results := make([]indexedResult, len(normalizedToolCalls))
var wg sync.WaitGroup

for i, tc := range normalizedToolCalls {
    wg.Add(1)
    go func(idx int, tc providers.ToolCall) {
        defer wg.Done()
        // Execute tool with proper error handling
        toolResult := config.Tools.Execute(ctx, tc.Name, tc.Arguments)
        results[idx].result = toolResult
    }(i, tc)
}
wg.Wait()
```

---

## Root Cause: Empty Tool Calls

### Problem Manifestation

"Clicking is not working reliably as of now returning empty tool calls"

### Investigation Path

**1. LLM Response Parsing** (`hypr-claw-runtime/src/llm_client.rs`)

The system parses tool calls from LLM responses in multiple ways:

```rust
// OpenAI-compatible structured tool calls
if let Some(tool_calls) = choice.message.tool_calls {
    if let Some(tool_call) = tool_calls.first() {
        LLMResponse::ToolCall {
            tool_name: tool_call.function.name.clone(),
            input: serde_json::from_str(&tool_call.function.arguments)
                .unwrap_or(serde_json::json!({})),
        }
    }
}

// Fallback: inline tool call parsing
if let Some((tool_name, input)) = parse_inline_tool_call(&content) {
    LLMResponse::ToolCall { tool_name, input }
}
```

**2. Inline Tool Call Parsing** (`parse_inline_tool_call`)

```rust
fn parse_inline_tool_call(content: &str) -> Option<(String, serde_json::Value)> {
    // Handles:
    // 1. <tool_call>desktop.mouse_click</tool_call>
    // 2. <tool_call>{"tool_name": "desktop.mouse_click", "input": {...}}</tool_call>
    // 3. desktop.mouse_click {"button": "left"}
    // 4. {"tool_name": "desktop.mouse_click", "input": {...}}
}
```

**3. Validation** (`validate_response`)

```rust
fn validate_response(&self, response: &LLMResponse) -> Result<(), RuntimeError> {
    match response {
        LLMResponse::ToolCall { tool_name, .. } => {
            if tool_name.is_empty() {
                return Err(RuntimeError::LLMError(
                    "Tool call missing tool_name".to_string(),
                ));
            }
        }
        // ...
    }
}
```

### Likely Causes of Empty Tool Calls

**A. LLM Not Generating Proper Tool Calls**
- Model doesn't understand tool schema
- System prompt not reinforcing tool usage
- Tool names not clear enough

**B. Parsing Failures**
- LLM generates tool call in unexpected format
- `parse_inline_tool_call` doesn't match the format
- JSON parsing fails silently (`.unwrap_or(json!({})`)

**C. Tool Name Mismatch**
- LLM uses wrong tool name (e.g., "click" instead of "desktop.mouse_click")
- Tool not in registry for current agent
- Tool filtered out by capability check

**D. Empty Input Arguments**
- LLM generates tool call without required "button" parameter
- Validation catches this but error message unclear

---

## Picoclaw Patterns to Adopt

### 1. Parallel Tool Execution

**Picoclaw** (`pkg/tools/toolloop.go`):
```go
// Execute tool calls in parallel with WaitGroup
var wg sync.WaitGroup
for i, tc := range normalizedToolCalls {
    wg.Add(1)
    go func(idx int, tc providers.ToolCall) {
        defer wg.Done()
        results[idx].result = executeToolWithTimeout(ctx, tc)
    }(i, tc)
}
wg.Wait()
```

**Hyprclaw Current** (`hypr-claw-runtime/src/agent_loop.rs`):
```rust
// Sequential execution - one tool at a time
match response {
    LLMResponse::ToolCall { tool_name, input, .. } => {
        let tool_result = self.tool_dispatcher
            .execute(&tool_name, &input, session_key)
            .await?;
        // ...
    }
}
```

**Benefit**: Faster execution when LLM requests multiple tools.

### 2. Tool Call Normalization

**Picoclaw** (`pkg/providers/types.go`):
```go
func NormalizeToolCall(tc ToolCall) ToolCall {
    // Ensures consistent structure regardless of provider format
    if tc.Function != nil && tc.Name == "" {
        tc.Name = tc.Function.Name
    }
    if tc.Arguments == nil && tc.Function != nil {
        json.Unmarshal([]byte(tc.Function.Arguments), &tc.Arguments)
    }
    return tc
}
```

**Hyprclaw**: Missing this normalization step.

### 3. Detailed Tool Call Logging

**Picoclaw**:
```go
argsJSON, _ := json.Marshal(tc.Arguments)
argsPreview := utils.Truncate(string(argsJSON), 200)
logger.InfoCF("toolloop", fmt.Sprintf("Tool call: %s(%s)", tc.Name, argsPreview),
    map[string]any{
        "tool":      tc.Name,
        "iteration": iteration,
    })
```

**Hyprclaw**: Less detailed logging makes debugging harder.

### 4. Tool Result Structure

**Picoclaw** (`pkg/tools/result.go`):
```go
type ToolResult struct {
    Content string                 `json:"content"`
    Error   string                 `json:"error,omitempty"`
    IsError bool                   `json:"is_error"`
    Metadata map[string]interface{} `json:"metadata,omitempty"`
}
```

**Hyprclaw** (`hypr-claw-tools/src/tools/base.rs`):
```rust
pub struct ToolResult {
    pub success: bool,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub metadata: HashMap<String, String>,
}
```

**Similar but**: Picoclaw has explicit `IsError` flag separate from error message.

### 5. Tool Registry with TTL Promotion

**Picoclaw** (`pkg/tools/registry.go`):
```go
// Core tools always visible
// Hidden tools promoted temporarily with TTL
func (r *ToolRegistry) PromoteTools(names []string, ttl int) {
    for _, name := range names {
        if entry, exists := r.tools[name]; exists {
            if !entry.IsCore {
                entry.TTL = ttl
            }
        }
    }
}

func (r *ToolRegistry) TickTTL() {
    for _, entry := range r.tools {
        if !entry.IsCore && entry.TTL > 0 {
            entry.TTL--
        }
    }
}
```

**Hyprclaw**: Static tool registration, no dynamic visibility.

**Benefit**: Reduces tool confusion by showing only relevant tools.

---

## Diagnostic Steps

### Step 1: Verify Backend Availability

```bash
# Check if ydotool or wlrctl is installed
which ydotool
which wlrctl

# Test manual click
ydotool click 1
wlrctl pointer click left
```

### Step 2: Enable Debug Logging

Add to `hypr-claw-runtime/src/agent_loop.rs`:

```rust
// Before tool execution
info!("🔧 Tool call received: name='{}', input={}", tool_name, 
      serde_json::to_string_pretty(&input).unwrap_or_default());

// After tool execution
info!("✅ Tool result: success={}, output={}", 
      tool_result.get("success").and_then(|v| v.as_bool()).unwrap_or(false),
      serde_json::to_string_pretty(&tool_result).unwrap_or_default());
```

### Step 3: Check LLM Response Format

Add to `hypr-claw-runtime/src/llm_client.rs`:

```rust
// After receiving response from LLM
debug!("📥 Raw LLM response: {}", 
       serde_json::to_string_pretty(&response_json).unwrap_or_default());
```

### Step 4: Validate Tool Schema Visibility

```rust
// In agent_loop.rs execute_loop
info!("🛠️  Available tools for agent '{}': {:?}", agent_id, tool_names);
```

---

## Implementation Plan

### Phase 1: Immediate Fixes (1-2 days)

**Goal**: Make clicking work reliably NOW.

#### 1.1 Enhanced Tool Call Logging

**File**: `hypr-claw-runtime/src/agent_loop.rs`

```rust
// Add before tool execution (line ~330)
info!("🔧 TOOL CALL DEBUG:");
info!("  - Tool name: '{}'", tool_name);
info!("  - Input: {}", serde_json::to_string_pretty(&input).unwrap_or_default());
info!("  - Session: {}", session_key);
info!("  - Iteration: {}/{}", iteration + 1, max_iterations);

// Add after tool execution
match &tool_result {
    Ok(result) => {
        info!("✅ TOOL SUCCESS:");
        info!("  - Success: {}", result.success);
        info!("  - Output: {:?}", result.output);
    }
    Err(e) => {
        error!("❌ TOOL FAILURE:");
        error!("  - Error: {}", e);
    }
}
```

#### 1.2 Tool Call Normalization

**File**: `hypr-claw-runtime/src/types.rs`

```rust
impl LLMResponse {
    /// Normalize tool call to ensure consistent structure
    pub fn normalize(self) -> Self {
        match self {
            LLMResponse::ToolCall { schema_version, tool_name, mut input } => {
                // Ensure input is an object
                if !input.is_object() {
                    input = serde_json::json!({});
                }
                
                // Trim tool name
                let tool_name = tool_name.trim().to_string();
                
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

**Usage in** `llm_client.rs`:

```rust
let llm_response = /* ... parse response ... */;
let normalized = llm_response.normalize();
self.validate_response(&normalized)?;
Ok(normalized)
```

#### 1.3 Better Error Messages

**File**: `hypr-claw-tools/src/os_tools.rs`

```rust
async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
    let button = match required_str(&input, "button") {
        Ok(b) => b,
        Err(e) => {
            return Ok(ToolResult {
                success: false,
                output: None,
                error: Some(format!(
                    "Missing required parameter 'button'. Expected one of: 'left', 'middle', 'right'. Error: {}",
                    e
                )),
                ..ToolResult::default()
            });
        }
    };
    
    match desktop::mouse_click(button).await {
        Ok(_) => Ok(ToolResult {
            success: true,
            output: Some(json!({"clicked": button, "message": "Mouse click executed successfully"})),
            error: None,
            ..ToolResult::default()
        }),
        Err(e) => Ok(ToolResult {
            success: false,
            output: None,
            error: Some(format!(
                "Failed to execute mouse click: {}. Ensure 'ydotool' or 'wlrctl' is installed and accessible.",
                e
            )),
            ..ToolResult::default()
        })
    }
}
```

#### 1.4 System Prompt Reinforcement

**File**: `hypr-claw-runtime/src/agent_loop.rs`

```rust
let reinforced_prompt = if action_requires_tool && !tool_names.is_empty() {
    format!(
        "{}\n\n## CRITICAL TOOL USAGE INSTRUCTIONS\n\
        You are a local autonomous Linux agent with direct OS control.\n\
        You MUST use tools to perform ANY file, process, desktop, or system operations.\n\
        Do NOT describe actions - CALL the appropriate tool immediately.\n\n\
        ## Available Tools:\n{}\n\n\
        ## Tool Call Format:\n\
        Use structured tool calls with exact tool names and required parameters.\n\
        Example for clicking: {{\"tool_name\": \"desktop.mouse_click\", \"input\": {{\"button\": \"left\"}}}}\n\n\
        ## Mouse Click Tool:\n\
        - Name: desktop.mouse_click\n\
        - Required parameter: button (must be 'left', 'middle', or 'right')\n\
        - Example: desktop.mouse_click with {{\"button\": \"left\"}}",
        system_prompt,
        tool_names.join("\n- ")
    )
} else {
    system_prompt.to_string()
};
```

### Phase 2: Picoclaw Pattern Adoption (3-5 days)

#### 2.1 Parallel Tool Execution

**File**: `hypr-claw-runtime/src/agent_loop.rs`

**Current**: Sequential tool execution (one LLMResponse::ToolCall per iteration)

**Target**: Support multiple tool calls in single LLM response

```rust
// New response type
pub enum LLMResponse {
    Final { schema_version: u32, content: String },
    ToolCalls { 
        schema_version: u32, 
        calls: Vec<ToolCallRequest> 
    },
}

pub struct ToolCallRequest {
    pub id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
}
```

**Execution**:

```rust
LLMResponse::ToolCalls { calls, .. } => {
    use futures::future::join_all;
    
    let futures: Vec<_> = calls.iter().map(|call| {
        let dispatcher = self.tool_dispatcher.clone();
        let session = session_key.to_string();
        let name = call.tool_name.clone();
        let input = call.input.clone();
        
        async move {
            (call.id.clone(), dispatcher.execute(&name, &input, &session).await)
        }
    }).collect();
    
    let results = join_all(futures).await;
    
    // Append all results to messages
    for (id, result) in results {
        messages.push(Message::with_metadata(
            Role::Tool,
            result?,
            json!({"tool_call_id": id}),
        ));
    }
}
```

#### 2.2 Tool Registry with Dynamic Visibility

**File**: `hypr-claw-tools/src/registry.rs`

```rust
pub struct ToolEntry {
    pub tool: Arc<dyn Tool>,
    pub is_core: bool,
    pub ttl: AtomicUsize,
}

impl ToolRegistryImpl {
    pub fn promote_tools(&self, names: &[&str], ttl: usize) {
        let mut tools = self.tools.write();
        for name in names {
            if let Some(entry) = tools.get_mut(*name) {
                if !entry.is_core {
                    entry.ttl.store(ttl, Ordering::SeqCst);
                }
            }
        }
    }
    
    pub fn tick_ttl(&self) {
        let tools = self.tools.read();
        for entry in tools.values() {
            if !entry.is_core {
                entry.ttl.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }
    
    pub fn get_visible_tools(&self, agent_id: &str) -> Vec<Arc<dyn Tool>> {
        let tools = self.tools.read();
        tools.values()
            .filter(|entry| {
                entry.is_core || entry.ttl.load(Ordering::SeqCst) > 0
            })
            .map(|entry| entry.tool.clone())
            .collect()
    }
}
```

#### 2.3 Tool Discovery and Promotion

**File**: `hypr-claw-runtime/src/agent_loop.rs`

```rust
// After successful tool execution
if tool_name.contains("mouse") || tool_name.contains("click") {
    // Promote related tools
    self.tool_registry.promote_tools(&[
        "desktop.mouse_move",
        "desktop.cursor_position",
        "desktop.mouse_move_and_verify",
    ], 5); // Visible for next 5 iterations
}
```

### Phase 3: Advanced Reliability (5-7 days)

#### 3.1 Tool Call Retry Logic

```rust
async fn execute_tool_with_retry(
    &self,
    tool_name: &str,
    input: &Value,
    session_key: &str,
    max_retries: usize,
) -> Result<ToolResult, ToolError> {
    let mut last_error = None;
    
    for attempt in 0..max_retries {
        match self.tool_dispatcher.execute(tool_name, input, session_key).await {
            Ok(result) if result.success => return Ok(result),
            Ok(result) => {
                last_error = result.error;
                if attempt < max_retries - 1 {
                    tokio::time::sleep(Duration::from_millis(100 * (attempt as u64 + 1))).await;
                }
            }
            Err(e) => {
                last_error = Some(e.to_string());
                if attempt < max_retries - 1 {
                    tokio::time::sleep(Duration::from_millis(100 * (attempt as u64 + 1))).await;
                }
            }
        }
    }
    
    Err(ToolError::ExecutionFailed(format!(
        "Tool '{}' failed after {} attempts. Last error: {}",
        tool_name, max_retries, last_error.unwrap_or_default()
    )))
}
```

#### 3.2 Tool Call Validation Before Execution

```rust
fn validate_tool_call(
    &self,
    tool_name: &str,
    input: &Value,
    agent_id: &str,
) -> Result<(), RuntimeError> {
    // Check tool exists
    let tool = self.tool_registry.get_tool(agent_id, tool_name)
        .ok_or_else(|| RuntimeError::ToolError(format!(
            "Tool '{}' not found in registry for agent '{}'", 
            tool_name, agent_id
        )))?;
    
    // Validate input against schema
    let schema = tool.schema();
    if let Err(e) = validate_json_schema(input, &schema) {
        return Err(RuntimeError::ToolError(format!(
            "Tool '{}' input validation failed: {}", 
            tool_name, e
        )));
    }
    
    Ok(())
}
```

#### 3.3 Fallback Tool Suggestions

```rust
fn suggest_alternative_tools(
    &self,
    failed_tool: &str,
    agent_id: &str,
) -> Vec<String> {
    let all_tools = self.tool_registry.list_tools(agent_id);
    
    // Fuzzy match on tool name
    all_tools.iter()
        .filter(|name| {
            let similarity = strsim::jaro_winkler(failed_tool, name);
            similarity > 0.7
        })
        .cloned()
        .collect()
}
```

---

## Testing Strategy

### Unit Tests

**File**: `hypr-claw-tools/tests/unit.rs`

```rust
#[tokio::test]
async fn test_mouse_click_tool_with_valid_input() {
    let tool = DesktopMouseClickTool;
    let input = json!({"button": "left"});
    let ctx = ExecutionContext::default();
    
    let result = tool.execute(ctx, input).await;
    
    // Should succeed if backend available, or fail with clear message
    match result {
        Ok(r) => assert!(r.success || r.error.is_some()),
        Err(e) => assert!(e.to_string().contains("ydotool") || e.to_string().contains("wlrctl")),
    }
}

#[tokio::test]
async fn test_mouse_click_tool_missing_button() {
    let tool = DesktopMouseClickTool;
    let input = json!({});
    let ctx = ExecutionContext::default();
    
    let result = tool.execute(ctx, input).await;
    
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("button"));
}

#[tokio::test]
async fn test_mouse_click_tool_invalid_button() {
    let tool = DesktopMouseClickTool;
    let input = json!({"button": "invalid"});
    let ctx = ExecutionContext::default();
    
    let result = tool.execute(ctx, input).await;
    
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(!result.success);
}
```

### Integration Tests

**File**: `hypr-claw-runtime/tests/test_tool_execution.rs`

```rust
#[tokio::test]
async fn test_agent_loop_mouse_click_workflow() {
    // Setup mock environment
    let session_store = Arc::new(MockSessionStore::new());
    let lock_manager = Arc::new(MockLockManager::new());
    let tool_dispatcher = Arc::new(MockToolDispatcher::new());
    let tool_registry = Arc::new(MockToolRegistry::new());
    
    // Register mouse click tool
    tool_registry.register("default", Arc::new(DesktopMouseClickTool));
    
    let agent_loop = AgentLoop::new(
        session_store,
        lock_manager,
        tool_dispatcher,
        tool_registry,
        mock_llm_client(),
        mock_compactor(),
        10,
    );
    
    let response = agent_loop.run(
        "test_session",
        "default",
        "You are a desktop assistant",
        "Click the left mouse button",
    ).await;
    
    assert!(response.is_ok());
}
```

### Manual Testing Checklist

```bash
# 1. Verify backend availability
which ydotool
which wlrctl

# 2. Test manual click
ydotool click 1

# 3. Run hyprclaw with debug logging
RUST_LOG=debug cargo run

# 4. Test click command
> click left mouse button

# 5. Check logs for:
#    - Tool call received with correct name and input
#    - Tool execution started
#    - Tool execution completed
#    - Tool result returned to LLM

# 6. Test variations
> click the screen
> perform a left click
> mouse click at current position
```

---

## Success Metrics

### Immediate (Phase 1)
- [ ] 100% of click requests generate non-empty tool calls
- [ ] Clear error messages when tool call fails
- [ ] Debug logs show exact tool call parameters

### Short-term (Phase 2)
- [ ] 95%+ click success rate when backend available
- [ ] Tool execution time < 100ms
- [ ] Parallel tool execution working

### Long-term (Phase 3)
- [ ] Zero empty tool call errors
- [ ] Automatic tool suggestion on failure
- [ ] Dynamic tool visibility reduces wrong tool calls by 50%

---

## Risk Mitigation

### Risk 1: Backend Not Installed
**Mitigation**: Clear installation instructions in error message
```rust
"Failed to execute mouse click: No backend found. 
Install one of:
  - ydotool: sudo pacman -S ydotool (Arch) or sudo apt install ydotool (Ubuntu)
  - wlrctl: sudo pacman -S wlrctl (Arch)
Then ensure the service is running: sudo systemctl enable --now ydotoold"
```

### Risk 2: Permission Issues
**Mitigation**: Check permissions before execution
```rust
// In runtime_health.rs
fn probe_pointer() -> BackendStatus {
    if command_exists("ydotool") {
        // Test if we can actually use it
        match Command::new("ydotool").arg("--help").output() {
            Ok(_) => BackendStatus::ready("ydotool"),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                BackendStatus::degraded("ydotool", "Permission denied. Add user to input group.")
            }
            Err(e) => BackendStatus::failed("ydotool", &e.to_string()),
        }
    }
    // ...
}
```

### Risk 3: LLM Doesn't Use Tools
**Mitigation**: Stronger system prompt + require_tool_call flag
```rust
// When action requires tool
let require_tool_call = action_requires_tool && !tool_schemas.is_empty();
let response = self.llm_client
    .call_with_tool_requirement(&reinforced_prompt, messages, tool_schemas, require_tool_call)
    .await?;
```

---

## Conclusion

The click functionality in hyprclaw is **architecturally sound** but suffers from:

1. **Tool invocation reliability** - LLM not generating proper tool calls
2. **Error visibility** - Hard to debug when things go wrong
3. **Prompt clarity** - System prompt not reinforcing tool usage strongly enough

**Picoclaw's approach** (MCP-based) is more modular but requires external servers. Hyprclaw's native approach is superior IF we fix the invocation layer.

**Recommended Action**: Implement Phase 1 immediately (1-2 days) to get clicking working reliably, then evaluate Phase 2/3 based on results.

**Key Insight**: The problem is NOT the click implementation - it's the layer ABOVE it (LLM → tool call → execution pipeline). Fix that, and clicking will work perfectly.
