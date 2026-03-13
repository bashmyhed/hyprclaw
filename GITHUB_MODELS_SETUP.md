# GitHub Models Integration

## Setup

### 1. Get GitHub Token

Create a Personal Access Token with `models:read` permission:
1. Go to https://github.com/settings/tokens
2. Click "Generate new token (classic)"
3. Select `models:read` scope
4. Copy the token

### 2. Configure Environment

```bash
export GITHUB_TOKEN="github_pat_YOUR_TOKEN_HERE"
```

### 3. Update Config

Edit `data/config.yaml`:

```yaml
llm:
  provider: github  # Add this
  github_token: ${GITHUB_TOKEN}  # Or paste token directly
  github_model: "openai/gpt-4o"  # Or other available models
```

Available models:
- `openai/gpt-4o`
- `openai/gpt-4o-mini`
- `meta-llama/Llama-3.3-70B-Instruct`
- `anthropic/claude-3.5-sonnet`

### 4. Code Integration

The GitHub Models client is already integrated. To use it, modify the bootstrap code to detect the provider:

```rust
// In hypr-claw-app/src/bootstrap.rs or main.rs
use hypr_claw_runtime::github_models_client::GitHubModelsClient;
use hypr_claw_runtime::llm_client_type::LLMClientType;

// Check config for provider
let llm_client = if config.llm.provider == "github" {
    let token = std::env::var("GITHUB_TOKEN")
        .or_else(|_| config.llm.github_token.clone().ok_or("No GitHub token"))
        .expect("GitHub token required");
    
    let model = config.llm.github_model
        .unwrap_or_else(|| "openai/gpt-4o".to_string());
    
    LLMClientType::GitHub(GitHubModelsClient::new(token, model))
} else {
    // Existing standard/codex logic
    LLMClientType::Standard(...)
};
```

## Features

✅ Free tier with rate limits  
✅ Tool calling support  
✅ Multiple model options  
✅ Automatic normalization  
✅ Same interface as other providers  

## Rate Limits

Free tier limits:
- 15 requests per minute
- 150 requests per day
- 10,000 tokens per minute

For higher limits, enable billing in GitHub settings.

## Testing

```bash
export GITHUB_TOKEN="your-token"
RUST_LOG=debug cargo run
```

Try:
```
> click left mouse button
> open gmail and click on compose
```

## Troubleshooting

**401 Unauthorized**: Token missing `models:read` permission  
**429 Rate Limited**: Wait or enable billing  
**Model not found**: Check available models list  

## Implementation

New files:
- `hypr-claw-runtime/src/github_models_client.rs` - API client
- Updated `llm_client_type.rs` - Added GitHub variant

Minimal changes, fully integrated with existing architecture.
