# GitHub Models Mode - Quick Start

## Usage

```bash
# Set your token
export GITHUB_TOKEN="github_pat_YOUR_TOKEN_HERE"

# Optional: Set model (defaults to openai/gpt-4o)
export GITHUB_MODEL="openai/gpt-4o-mini"

# Run GitHub mode
cargo run -- github
```

## Example Session

```
🚀 GitHub Models Mode

📡 Model: openai/gpt-4o
🔑 Token: github_pat_...

✅ Ready! Type your commands (Ctrl+C to exit)

> What is the capital of France?
🤖 The capital of France is Paris.

> Write a haiku about coding
🤖 Code flows like water,
    Bugs hide in silent shadows,
    Debug brings the light.

> ^C
```

## Available Models

- `openai/gpt-4o` - Most capable (default)
- `openai/gpt-4o-mini` - Fast and efficient
- `meta-llama/Llama-3.3-70B-Instruct` - Open source
- `anthropic/claude-3.5-sonnet` - Anthropic's best

## Features

✅ Simple REPL interface  
✅ No configuration needed  
✅ Direct GitHub Models API access  
✅ Free tier with rate limits  
✅ Separate from main agent mode  

## Rate Limits

Free tier:
- 15 requests/minute
- 150 requests/day
- 10,000 tokens/minute

## Implementation

**Minimal code** (~60 lines):
- Added `CliMode::GitHub` variant
- Added `run_github_mode()` function
- Simple REPL with direct API calls

**No dependencies on**:
- Config files
- Tool registry
- Agent loop
- Session storage

## Next Steps

To integrate with full agent:
1. Add GitHub provider to config
2. Use in bootstrap
3. Enable tool calling
4. Add to main agent loop

For now, this is a **standalone test mode** for GitHub Models API.
