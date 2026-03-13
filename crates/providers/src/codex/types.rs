use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PKCEPair {
    pub challenge: String,
    pub verifier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
    pub account_id: String,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
pub struct JWTClaims {
    #[serde(rename = "https://api.openai.com/auth")]
    pub auth: Option<AuthClaim>,
}

#[derive(Debug, Deserialize)]
pub struct AuthClaim {
    pub chatgpt_account_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CodexRequest {
    pub model: String,
    pub store: bool,
    pub include: Vec<String>,
    pub input: Vec<CodexMessage>,
    pub reasoning: ReasoningConfig,
    pub text: TextConfig,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CodexMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct ReasoningConfig {
    pub effort: String,
    pub summary: String,
}

#[derive(Debug, Serialize)]
pub struct TextConfig {
    pub verbosity: String,
}

#[derive(Debug, Deserialize)]
pub struct CodexResponse {
    pub output: Option<CodexOutput>,
}

#[derive(Debug, Deserialize)]
pub struct CodexOutput {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<CodexToolCall>>,
}

#[derive(Debug, Deserialize)]
pub struct CodexToolCall {
    pub function: CodexFunction,
}

#[derive(Debug, Deserialize)]
pub struct CodexFunction {
    pub name: String,
    pub arguments: String,
}
