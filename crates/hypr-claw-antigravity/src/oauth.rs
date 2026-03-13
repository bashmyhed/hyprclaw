use anyhow::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

// OAuth constants extracted from src/constants.ts
pub const ANTIGRAVITY_CLIENT_ID: &str = "tba";
pub const ANTIGRAVITY_CLIENT_SECRET: &str = "tba";
pub const ANTIGRAVITY_REDIRECT_URI: &str = "http://localhost:51121/oauth-callback";
pub const ANTIGRAVITY_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cclog",
    "https://www.googleapis.com/auth/experimentsandconfigs",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthState {
    pub verifier: String,
    pub project_id: String,
}

#[derive(Debug, Clone)]
pub struct AuthorizationUrl {
    pub url: String,
    pub verifier: String,
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: u64,
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub email: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TokenExchangeSuccess {
    pub refresh: String,
    pub access: String,
    pub expires: u64,
    pub email: Option<String>,
    pub project_id: String,
}

/// Generate PKCE challenge and verifier
fn generate_pkce() -> (String, String) {
    let mut rng = rand::thread_rng();
    let verifier: String = (0..128)
        .map(|_| {
            let idx = rng.gen_range(0..62);
            match idx {
                0..=25 => (b'A' + idx) as char,
                26..=51 => (b'a' + (idx - 26)) as char,
                _ => (b'0' + (idx - 52)) as char,
            }
        })
        .collect();

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

    (verifier, challenge)
}

/// Encode state for OAuth flow
fn encode_state(state: &AuthState) -> Result<String> {
    let json = serde_json::to_string(state)?;
    Ok(URL_SAFE_NO_PAD.encode(json.as_bytes()))
}

/// Decode state from OAuth callback
fn decode_state(state: &str) -> Result<AuthState> {
    let bytes = URL_SAFE_NO_PAD.decode(state)?;
    let json = String::from_utf8(bytes)?;
    Ok(serde_json::from_str(&json)?)
}

/// Build authorization URL for Google OAuth
pub async fn authorize_antigravity(project_id: Option<String>) -> Result<AuthorizationUrl> {
    let (verifier, challenge) = generate_pkce();
    let project_id = project_id.unwrap_or_default();

    let state = AuthState {
        verifier: verifier.clone(),
        project_id: project_id.clone(),
    };

    let state_encoded = encode_state(&state)?;

    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")?;
    url.query_pairs_mut()
        .append_pair("client_id", ANTIGRAVITY_CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", ANTIGRAVITY_REDIRECT_URI)
        .append_pair("scope", &ANTIGRAVITY_SCOPES.join(" "))
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state_encoded)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");

    Ok(AuthorizationUrl {
        url: url.to_string(),
        verifier,
        project_id,
    })
}

/// Exchange authorization code for tokens
pub async fn exchange_antigravity(code: &str, state: &str) -> Result<TokenExchangeSuccess> {
    let auth_state = decode_state(state)?;
    let client = reqwest::Client::new();

    let start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;

    // Exchange code for tokens
    let mut params = HashMap::new();
    params.insert("client_id", ANTIGRAVITY_CLIENT_ID);
    params.insert("client_secret", ANTIGRAVITY_CLIENT_SECRET);
    params.insert("code", code);
    params.insert("grant_type", "authorization_code");
    params.insert("redirect_uri", ANTIGRAVITY_REDIRECT_URI);
    params.insert("code_verifier", &auth_state.verifier);

    let token_response = client
        .post("https://oauth2.googleapis.com/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("User-Agent", "google-api-nodejs-client/9.15.1")
        .form(&params)
        .send()
        .await?
        .error_for_status()?
        .json::<TokenResponse>()
        .await?;

    // Get user info
    let user_info = client
        .get("https://www.googleapis.com/oauth2/v1/userinfo?alt=json")
        .header(
            "Authorization",
            format!("Bearer {}", token_response.access_token),
        )
        .header("User-Agent", "google-api-nodejs-client/9.15.1")
        .send()
        .await?
        .json::<UserInfo>()
        .await
        .ok();

    // Fetch project ID if not provided
    let project_id = if auth_state.project_id.is_empty() {
        fetch_project_id(&token_response.access_token).await?
    } else {
        auth_state.project_id
    };

    // Store refresh token with project ID
    let stored_refresh = format!("{}|{}", token_response.refresh_token, project_id);

    let expires = start_time + (token_response.expires_in * 1000);

    Ok(TokenExchangeSuccess {
        refresh: stored_refresh,
        access: token_response.access_token,
        expires,
        email: user_info.and_then(|u| u.email),
        project_id,
    })
}

/// Fetch project ID from Antigravity API
async fn fetch_project_id(access_token: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let endpoints = [
        "https://cloudcode-pa.googleapis.com",
        "https://daily-cloudcode-pa.sandbox.googleapis.com",
        "https://autopush-cloudcode-pa.sandbox.googleapis.com",
    ];

    for endpoint in endpoints {
        let url = format!("{}/v1internal:loadCodeAssist", endpoint);
        let body = serde_json::json!({
            "metadata": {
                "ideType": "ANTIGRAVITY",
                "platform": if cfg!(windows) { "WINDOWS" } else { "MACOS" },
                "pluginType": "GEMINI"
            }
        });

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .header("User-Agent", "google-api-nodejs-client/9.15.1")
            .json(&body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;

        if let Ok(resp) = response {
            if resp.status().is_success() {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    if let Some(project) = data.get("cloudaicompanionProject") {
                        if let Some(id) = project.as_str() {
                            return Ok(id.to_string());
                        }
                        if let Some(id) = project.get("id").and_then(|v| v.as_str()) {
                            return Ok(id.to_string());
                        }
                    }
                }
            }
        }
    }

    Ok(String::new())
}

/// Refresh access token using refresh token
pub async fn refresh_access_token(refresh_token: &str) -> Result<(String, u64)> {
    let client = reqwest::Client::new();
    let start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;

    let mut params = HashMap::new();
    params.insert("client_id", ANTIGRAVITY_CLIENT_ID);
    params.insert("client_secret", ANTIGRAVITY_CLIENT_SECRET);
    params.insert("refresh_token", refresh_token);
    params.insert("grant_type", "refresh_token");

    let token_response = client
        .post("https://oauth2.googleapis.com/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("User-Agent", "google-api-nodejs-client/9.15.1")
        .form(&params)
        .send()
        .await?
        .error_for_status()?
        .json::<TokenResponse>()
        .await?;

    let expires = start_time + (token_response.expires_in * 1000);

    Ok((token_response.access_token, expires))
}
