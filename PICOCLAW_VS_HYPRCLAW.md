# Picoclaw vs Hyprclaw: Architecture Comparison

## Executive Summary

**Picoclaw**: Go-based agent using MCP (Model Context Protocol) for extensibility
**Hyprclaw**: Rust-based agent with native OS integration

**Key Finding**: Picoclaw doesn't have native desktop automation - they delegate to MCP servers. Hyprclaw has superior native implementation but needs better tool invocation reliability.

---

## Desktop Automation Approach

### Picoclaw: MCP-Based (External)

```
User Request
    ↓
Agent Loop (pkg/agent/loop.go)
    ↓
Tool Registry (pkg/tools/registry.go)
    ↓
MCP Manager (pkg/mcp/manager.go)
    ↓
External MCP Server (e.g., desktop-automation-mcp)
    ↓
OS Commands (ydotool, xdotool, etc.)
```

**Pros**:
- Modular: Desktop automation is a separate concern
- Extensible: Add new capabilities via MCP servers
- Language-agnostic: MCP servers can be in any language

**Cons**:
- Extra dependency: Requires MCP server installation
- Network overhead: Communication via stdio/HTTP
- Complexity: More moving parts to debug

**Evidence**:
```bash
# No mouse/click tools in picoclaw codebase
$ grep -r "mouse\|click" picoclaw/pkg/tools/*.go
# Only finds: i2c, spi (hardware), web, filesystem, shell, etc.
# NO desktop automation tools

# MCP integration is the key
$ cat picoclaw/pkg/tools/mcp_tool.go
type MCPTool struct {
    manager    MCPManager
    serverName string
    tool       *mcp.Tool
}
```

### Hyprclaw: Native (Built-in)

```
User Request
    ↓
Agent Loop (hypr-claw-runtime/src/agent_loop.rs)
    ↓
Tool Dispatcher (hypr-claw-tools/src/dispatcher.rs)
    ↓
OS Tools (hypr-claw-tools/src/os_tools.rs)
    ↓
Desktop Capabilities (hypr-claw-tools/src/os_capabilities/desktop.rs)
    ↓
OS Commands (ydotool, wlrctl, hyprctl)
```

**Pros**:
- Direct: No external dependencies
- Fast: No IPC overhead
- Integrated: Type-safe, compile-time checked
- Reliable: Fewer failure points

**Cons**:
- Monolithic: Desktop automation coupled to agent
- Less extensible: Adding new tools requires Rust code
- Platform-specific: Hyprland-focused

**Evidence**:
```rust
// hypr-claw-tools/src/os_capabilities/desktop.rs
pub async fn mouse_click(button: &str) -> OsResult<()> {
    let code = parse_mouse_button(button)?;
    if command_exists("ydotool").await {
        return run_checked("ydotool", &["click", code]).await;
    }
    if command_exists("wlrctl").await {
        return run_checked("wlrctl", &["pointer", "click", button]).await;
    }
    Err(OsError::OperationFailed(
        "No click backend found".to_string(),
    ))
}
```

---

## Tool Execution Pattern

### Picoclaw: Parallel Execution

**File**: `pkg/tools/toolloop.go`

```go
// Execute tool calls in parallel
results := make([]indexedResult, len(normalizedToolCalls))
var wg sync.WaitGroup

for i, tc := range normalizedToolCalls {
    results[i].tc = tc
    wg.Add(1)
    go func(idx int, tc providers.ToolCall) {
        defer wg.Done()
        
        logger.InfoCF("toolloop", fmt.Sprintf("Tool call: %s(%s)", tc.Name, argsPreview),
            map[string]any{
                "tool":      tc.Name,
                "iteration": iteration,
            })
        
        var toolResult *ToolResult
        if config.Tools != nil {
            toolResult = config.Tools.Execute(ctx, tc.Name, tc.Arguments, channel, chatID)
        } else {
            toolResult = &ToolResult{
                Content: "",
                Error:   "Tool registry not available",
                IsError: true,
            }
        }
        
        results[idx].result = toolResult
    }(i, tc)
}

wg.Wait()

// Append all results to messages
for _, res := range results {
    messages = append(messages, providers.Message{
        Role:       "tool",
        Content:    res.result.Content,
        ToolCallID: res.tc.ID,
    })
}
```

**Benefits**:
- Multiple tools execute simultaneously
- Faster overall execution time
- Better resource utilization

**Complexity**:
- Requires goroutines/async coordination
- Need to handle partial failures
- Order of results may vary

### Hyprclaw: Sequential Execution

**File**: `hypr-claw-runtime/src/agent_loop.rs`

```rust
// Execute one tool at a time
match response {
    LLMResponse::ToolCall { tool_name, input, .. } => {
        saw_tool_call = true;
        
        info!("Executing tool: {} (iteration {})", tool_name, iteration + 1);
        
        // Execute tool
        let tool_result = match self.tool_dispatcher
            .execute(&tool_name, &input, session_key)
            .await
        {
            Ok(result) => result,
            Err(e) => {
                warn!("Tool execution failed: {}", e);
                json!({"error": e.to_string()})
            }
        };
        
        // Append tool result
        messages.push(Message::with_metadata(
            Role::Tool,
            tool_result,
            json!({"tool_name": tool_name}),
        ));
        
        // Continue loop for next iteration
    }
}
```

**Benefits**:
- Simpler to reason about
- Easier to debug
- Deterministic execution order

**Drawbacks**:
- Slower when multiple tools needed
- Underutilizes async capabilities
- More LLM round-trips

---

## Tool Registry Pattern

### Picoclaw: Dynamic Visibility with TTL

**File**: `pkg/tools/registry.go`

```go
type ToolEntry struct {
    Tool   Tool
    IsCore bool  // Core tools always visible
    TTL    int   // Hidden tools promoted temporarily
}

type ToolRegistry struct {
    tools   map[string]*ToolEntry
    mu      sync.RWMutex
    version atomic.Uint64
}

// Promote hidden tools temporarily
func (r *ToolRegistry) PromoteTools(names []string, ttl int) {
    r.mu.Lock()
    defer r.mu.Unlock()
    promoted := 0
    for _, name := range names {
        if entry, exists := r.tools[name]; exists {
            if !entry.IsCore {
                entry.TTL = ttl
                promoted++
            }
        }
    }
}

// Decrease TTL each iteration
func (r *ToolRegistry) TickTTL() {
    r.mu.Lock()
    defer r.mu.Unlock()
    for _, entry := range r.tools {
        if !entry.IsCore && entry.TTL > 0 {
            entry.TTL--
        }
    }
}

// Get only visible tools
func (r *ToolRegistry) Get(name string) (Tool, bool) {
    r.mu.RLock()
    defer r.mu.RUnlock()
    entry, ok := r.tools[name]
    if !ok {
        return nil, false
    }
    // Hidden tools with expired TTL are not callable
    if !entry.IsCore && entry.TTL <= 0 {
        return nil, false
    }
    return entry.Tool, true
}
```

**Benefits**:
- Reduces tool confusion: Only show relevant tools
- Context-aware: Tools appear when needed
- Automatic cleanup: TTL expires naturally

**Use Case**:
```go
// After successful file operation, promote related tools
if strings.Contains(toolName, "file") {
    registry.PromoteTools([]string{
        "read_file",
        "write_file", 
        "list_directory",
    }, 5) // Visible for next 5 iterations
}
```

### Hyprclaw: Static Registration

**File**: `hypr-claw-tools/src/registry.rs`

```rust
pub struct ToolRegistryImpl {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
    agent_tools: RwLock<HashMap<String, Vec<String>>>,
}

impl ToolRegistryImpl {
    pub fn register(&self, tool: Arc<dyn Tool>) {
        let mut tools = self.tools.write();
        tools.insert(tool.name().to_string(), tool);
    }
    
    pub fn assign_to_agent(&self, agent_id: &str, tool_names: Vec<String>) {
        let mut agent_tools = self.agent_tools.write();
        agent_tools.insert(agent_id.to_string(), tool_names);
    }
    
    pub fn get_tool_schemas(&self, agent_id: &str) -> Vec<Value> {
        let agent_tools = self.agent_tools.read();
        let tools = self.tools.read();
        
        if let Some(tool_names) = agent_tools.get(agent_id) {
            tool_names.iter()
                .filter_map(|name| tools.get(name))
                .map(|tool| tool.schema())
                .collect()
        } else {
            Vec::new()
        }
    }
}
```

**Benefits**:
- Simple: Tools are either available or not
- Predictable: Same tools every iteration
- Type-safe: Compile-time guarantees

**Drawbacks**:
- Tool overload: All tools visible all the time
- No context awareness: Can't adapt to workflow
- Higher wrong-tool-call rate

---

## Tool Call Normalization

### Picoclaw: Explicit Normalization

**File**: `pkg/providers/types.go`

```go
type ToolCall struct {
    ID        string                 `json:"id"`
    Type      string                 `json:"type"`
    Name      string                 `json:"name,omitempty"`
    Arguments map[string]interface{} `json:"arguments,omitempty"`
    Function  *FunctionCall          `json:"function,omitempty"`
}

type FunctionCall struct {
    Name      string `json:"name"`
    Arguments string `json:"arguments"` // JSON string
}

// NormalizeToolCall ensures consistent structure
func NormalizeToolCall(tc ToolCall) ToolCall {
    // If Name is empty but Function.Name exists, use it
    if tc.Name == "" && tc.Function != nil {
        tc.Name = tc.Function.Name
    }
    
    // If Arguments is nil but Function.Arguments exists, parse it
    if tc.Arguments == nil && tc.Function != nil && tc.Function.Arguments != "" {
        var args map[string]interface{}
        if err := json.Unmarshal([]byte(tc.Function.Arguments), &args); err == nil {
            tc.Arguments = args
        }
    }
    
    // Ensure Type is set
    if tc.Type == "" {
        tc.Type = "function"
    }
    
    return tc
}
```

**Usage**:
```go
normalizedToolCalls := make([]providers.ToolCall, 0, len(response.ToolCalls))
for _, tc := range response.ToolCalls {
    normalizedToolCalls = append(normalizedToolCalls, providers.NormalizeToolCall(tc))
}
```

### Hyprclaw: Implicit Validation

**File**: `hypr-claw-runtime/src/llm_client.rs`

```rust
// Parse tool call from OpenAI format
if let Some(tool_calls) = choice.message.tool_calls {
    if let Some(tool_call) = tool_calls.first() {
        LLMResponse::ToolCall {
            schema_version: crate::types::SCHEMA_VERSION,
            tool_name: tool_call.function.name.clone(),
            input: serde_json::from_str(&tool_call.function.arguments)
                .unwrap_or(serde_json::json!({})),  // Silent fallback
        }
    }
}

// Validation happens later
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
    Ok(())
}
```

**Issue**: Silent fallback with `.unwrap_or(json!({}))` can hide parsing errors.

**Fix Needed**: Explicit normalization like picoclaw.

---

## Error Handling

### Picoclaw: Structured Tool Results

**File**: `pkg/tools/result.go`

```go
type ToolResult struct {
    Content  string                 `json:"content"`
    Error    string                 `json:"error,omitempty"`
    IsError  bool                   `json:"is_error"`
    Metadata map[string]interface{} `json:"metadata,omitempty"`
}

// Helper constructors
func Success(content string) *ToolResult {
    return &ToolResult{
        Content: content,
        IsError: false,
    }
}

func Error(err string) *ToolResult {
    return &ToolResult{
        Content: "",
        Error:   err,
        IsError: true,
    }
}
```

**Usage**:
```go
func (t *ShellTool) Execute(ctx context.Context, args map[string]interface{}) *ToolResult {
    cmd := args["command"].(string)
    
    output, err := exec.CommandContext(ctx, "sh", "-c", cmd).CombinedOutput()
    if err != nil {
        return tools.Error(fmt.Sprintf("Command failed: %v\nOutput: %s", err, output))
    }
    
    return tools.Success(string(output))
}
```

### Hyprclaw: Result + Error Enum

**File**: `hypr-claw-tools/src/tools/base.rs`

```rust
pub struct ToolResult {
    pub success: bool,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub metadata: HashMap<String, String>,
}

impl Default for ToolResult {
    fn default() -> Self {
        Self {
            success: false,
            output: None,
            error: None,
            metadata: HashMap::new(),
        }
    }
}
```

**Usage**:
```rust
async fn execute(&self, _ctx: ExecutionContext, input: Value) -> Result<ToolResult, ToolError> {
    let button = required_str(&input, "button")?;
    
    desktop::mouse_click(button).await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    
    Ok(ToolResult {
        success: true,
        output: Some(json!({"clicked": button})),
        error: None,
        ..ToolResult::default()
    })
}
```

**Difference**: Picoclaw uses `IsError` flag, Hyprclaw uses `success` bool. Functionally equivalent.

---

## Logging and Observability

### Picoclaw: Structured Logging

```go
logger.InfoCF("toolloop", "Tool call",
    map[string]any{
        "tool":      tc.Name,
        "iteration": iteration,
        "args":      argsPreview,
    })

logger.ErrorCF("toolloop", "LLM call failed",
    map[string]any{
        "iteration": iteration,
        "error":     err.Error(),
    })
```

**Benefits**:
- Structured: Easy to parse and analyze
- Context-rich: Includes relevant metadata
- Consistent: Same format everywhere

### Hyprclaw: Traditional Logging

```rust
info!("Executing tool: {} (iteration {})", tool_name, iteration + 1);
warn!("Tool execution failed: {}", e);
error!("LLM call failed: {}", e);
```

**Benefits**:
- Simple: Easy to read
- Familiar: Standard Rust logging

**Drawbacks**:
- Less structured: Harder to parse programmatically
- Less context: Missing metadata

**Improvement Needed**: Add structured logging with context.

---

## Session Persistence

### Picoclaw: JSONL (Append-Only)

**File**: `pkg/memory/jsonl.go`

```go
// Append message to JSONL file
func (s *JSONLStore) AddFullMessage(ctx context.Context, sessionKey string, msg providers.Message) error {
    s.mu.Lock()
    defer s.mu.Unlock()
    
    // Open file in append mode
    f, err := os.OpenFile(s.filePath(sessionKey), os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
    if err != nil {
        return err
    }
    defer f.Close()
    
    // Write message as single JSON line
    encoder := json.NewEncoder(f)
    if err := encoder.Encode(msg); err != nil {
        return err
    }
    
    return nil
}

// Load all messages from JSONL
func (s *JSONLStore) LoadHistory(ctx context.Context, sessionKey string) ([]providers.Message, error) {
    f, err := os.Open(s.filePath(sessionKey))
    if err != nil {
        if os.IsNotExist(err) {
            return []providers.Message{}, nil
        }
        return nil, err
    }
    defer f.Close()
    
    var messages []providers.Message
    scanner := bufio.NewScanner(f)
    for scanner.Scan() {
        var msg providers.Message
        if err := json.Unmarshal(scanner.Bytes(), &msg); err != nil {
            continue // Skip malformed lines
        }
        messages = append(messages, msg)
    }
    
    return messages, nil
}
```

**Benefits**:
- Crash-safe: Each line is atomic
- Append-only: No data loss on partial write
- Replayable: Can reconstruct session from log

### Hyprclaw: JSON (Overwrite)

**File**: `hypr-claw-infra/src/infra/session_store.rs`

```rust
async fn save(&self, session_key: &str, messages: &[Message]) -> Result<(), RuntimeError> {
    let path = self.session_path(session_key);
    
    // Serialize entire message array
    let json = serde_json::to_string_pretty(messages)
        .map_err(|e| RuntimeError::SerializationError(e.to_string()))?;
    
    // Write entire file (overwrite)
    tokio::fs::write(&path, json).await
        .map_err(|e| RuntimeError::IOError(e.to_string()))?;
    
    Ok(())
}

async fn load(&self, session_key: &str) -> Result<Vec<Message>, RuntimeError> {
    let path = self.session_path(session_key);
    
    if !path.exists() {
        return Ok(Vec::new());
    }
    
    // Read entire file
    let json = tokio::fs::read_to_string(&path).await
        .map_err(|e| RuntimeError::IOError(e.to_string()))?;
    
    // Deserialize entire array
    let messages: Vec<Message> = serde_json::from_str(&json)
        .map_err(|e| RuntimeError::SerializationError(e.to_string()))?;
    
    Ok(messages)
}
```

**Benefits**:
- Simple: Easy to implement
- Human-readable: Pretty-printed JSON

**Drawbacks**:
- Not crash-safe: Partial write corrupts file
- Inefficient: Rewrites entire history each time
- No replay: Can't reconstruct from partial data

**Improvement Needed**: Adopt JSONL pattern from picoclaw.

---

## Recommendation

### What to Keep from Hyprclaw
1. ✅ Native desktop automation (superior to MCP approach)
2. ✅ Type-safe Rust implementation
3. ✅ Direct OS integration
4. ✅ Hyprland-specific optimizations

### What to Adopt from Picoclaw
1. 🔄 Tool call normalization
2. 🔄 Parallel tool execution
3. 🔄 Dynamic tool visibility with TTL
4. 🔄 JSONL session persistence
5. 🔄 Structured logging
6. 🔄 Better error context

### Priority Order
1. **Immediate**: Tool call normalization + better logging (fixes click issue)
2. **Short-term**: Parallel execution + structured logging
3. **Long-term**: Dynamic tool visibility + JSONL persistence

### Why Hyprclaw's Approach is Better (When Fixed)
- **Performance**: No IPC overhead
- **Reliability**: Fewer failure points
- **Integration**: Type-safe, compile-time checked
- **Simplicity**: One binary, no external servers

The click issue is NOT an architecture problem - it's an invocation layer problem that can be fixed with better normalization and validation.
