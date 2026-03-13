use super::constants::*;
use super::types::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::Rng;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn generate_pkce() -> PKCEPair {
    let verifier: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(96)
        .map(char::from)
        .collect();

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

    PKCEPair {
        challenge,
        verifier,
    }
}

pub fn generate_state() -> String {
    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

pub fn build_authorization_url(pkce: &PKCEPair, state: &str) -> String {
    format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&code_challenge={}&code_challenge_method=S256&codex_cli_simplified_flow=true&originator=codex_cli_rs",
        AUTHORIZE_URL,
        urlencoding::encode(CLIENT_ID),
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(SCOPE),
        state,
        urlencoding::encode(&pkce.challenge)
    )
}

pub async fn exchange_code_for_tokens(
    code: &str,
    verifier: &str,
) -> Result<OAuthTokens, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", CLIENT_ID),
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", REDIRECT_URI),
    ];

    let response = client.post(TOKEN_URL).form(&params).send().await?;

    if !response.status().is_success() {
        return Err(format!("Token exchange failed: {}", response.status()).into());
    }

    let token_response: TokenResponse = response.json().await?;
    let account_id = decode_jwt_account_id(&token_response.access_token)
        .ok_or("Failed to extract account ID from token")?;

    let expires_at =
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + token_response.expires_in;

    Ok(OAuthTokens {
        access_token: token_response.access_token,
        refresh_token: token_response.refresh_token,
        expires_at,
        account_id,
    })
}

pub async fn refresh_access_token(
    refresh_token: &str,
) -> Result<OAuthTokens, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", refresh_token),
    ];

    let response = client.post(TOKEN_URL).form(&params).send().await?;

    if !response.status().is_success() {
        return Err(format!("Token refresh failed: {}", response.status()).into());
    }

    let token_response: TokenResponse = response.json().await?;
    let account_id = decode_jwt_account_id(&token_response.access_token)
        .ok_or("Failed to extract account ID from token")?;

    let expires_at =
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + token_response.expires_in;

    Ok(OAuthTokens {
        access_token: token_response.access_token,
        refresh_token: token_response.refresh_token,
        expires_at,
        account_id,
    })
}

pub fn decode_jwt_account_id(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let claims: JWTClaims = serde_json::from_slice(&payload).ok()?;
    claims.auth?.chatgpt_account_id
}

pub fn is_token_expired(tokens: &OAuthTokens) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Refresh 5 minutes before expiration
    tokens.expires_at <= now + 300
}
