// OAuth constants from OpenAI Codex CLI
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
pub const SCOPE: &str = "openid profile email offline_access";

// Codex API
#[allow(dead_code)]
pub const CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
pub const CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";

// Headers
pub const HEADER_OPENAI_BETA: &str = "OpenAI-Beta";
pub const HEADER_ORIGINATOR: &str = "originator";
pub const HEADER_ACCOUNT_ID: &str = "chatgpt-account-id";

pub const VALUE_OPENAI_BETA: &str = "responses=experimental";
pub const VALUE_ORIGINATOR: &str = "codex_cli_rs";

// JWT
#[allow(dead_code)]
pub const JWT_ACCOUNT_ID_CLAIM: &str = "https://api.openai.com/auth";
