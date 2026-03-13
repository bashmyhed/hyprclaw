use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;

use crate::fingerprint::{generate_fingerprint, Fingerprint};
use crate::oauth;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub email: Option<String>,
    pub refresh_token: String,
    pub project_id: String,
    pub access_token: Option<String>,
    pub expires: Option<u64>,
    pub enabled: bool,
    pub added_at: u64,
    pub last_used: u64,
    pub fingerprint: Option<Fingerprint>,
    pub rate_limit_reset_times: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountStorage {
    pub accounts: Vec<Account>,
}

pub struct AccountManager {
    storage_path: PathBuf,
    accounts: Vec<Account>,
    current_index: usize,
}

impl AccountManager {
    pub async fn new(storage_path: PathBuf) -> Result<Self> {
        let accounts = if storage_path.exists() {
            let content = fs::read_to_string(&storage_path).await?;
            let storage: AccountStorage = serde_json::from_str(&content)?;
            storage.accounts
        } else {
            Vec::new()
        };

        Ok(Self {
            storage_path,
            accounts,
            current_index: 0,
        })
    }

    pub async fn add_account(
        &mut self,
        email: Option<String>,
        refresh: String,
        project_id: String,
    ) -> Result<()> {
        let parts: Vec<&str> = refresh.split('|').collect();
        let refresh_token = parts.first().unwrap_or(&"").to_string();
        let project_id = if parts.len() > 1 {
            parts[1].to_string()
        } else {
            project_id
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;

        let account = Account {
            email,
            refresh_token,
            project_id,
            access_token: None,
            expires: None,
            enabled: true,
            added_at: now,
            last_used: now,
            fingerprint: Some(generate_fingerprint()),
            rate_limit_reset_times: HashMap::new(),
        };

        self.accounts.push(account);
        self.save().await?;

        Ok(())
    }

    pub async fn get_current_account(&mut self) -> Result<&mut Account> {
        if self.accounts.is_empty() {
            anyhow::bail!("No accounts available");
        }

        // Find next available account (not rate limited)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;

        let start_index = self.current_index;
        loop {
            let account = &self.accounts[self.current_index];

            // Check if account is rate limited
            let is_rate_limited = account
                .rate_limit_reset_times
                .values()
                .any(|&reset_time| now < reset_time);

            if account.enabled && !is_rate_limited {
                break;
            }

            self.current_index = (self.current_index + 1) % self.accounts.len();
            if self.current_index == start_index {
                anyhow::bail!("All accounts are rate limited or disabled");
            }
        }

        Ok(&mut self.accounts[self.current_index])
    }

    pub async fn refresh_current_token(&mut self) -> Result<String> {
        let account = self.get_current_account().await?;

        // Check if token is still valid
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;

        if let (Some(token), Some(expires)) = (&account.access_token, account.expires) {
            if now + 60_000 < expires {
                return Ok(token.clone());
            }
        }

        // Refresh token
        let (access_token, expires) = oauth::refresh_access_token(&account.refresh_token).await?;

        let account = self.get_current_account().await?;
        account.access_token = Some(access_token.clone());
        account.expires = Some(expires);
        account.last_used = now;

        self.save().await?;

        Ok(access_token)
    }

    pub async fn mark_rate_limited(&mut self, quota_key: &str, backoff_ms: u64) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;

        let account = self.get_current_account().await?;
        account
            .rate_limit_reset_times
            .insert(quota_key.to_string(), now + backoff_ms);

        self.save().await?;

        // Rotate to next account
        self.current_index = (self.current_index + 1) % self.accounts.len();

        Ok(())
    }

    pub fn get_account_count(&self) -> usize {
        self.accounts.len()
    }

    async fn save(&self) -> Result<()> {
        let storage = AccountStorage {
            accounts: self.accounts.clone(),
        };

        let content = serde_json::to_string_pretty(&storage)?;

        // Create parent directory if it doesn't exist
        if let Some(parent) = self.storage_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(&self.storage_path, content).await?;

        Ok(())
    }
}
