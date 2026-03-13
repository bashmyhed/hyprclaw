use crate::traits::Interface;
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct TerminalInterface;

impl TerminalInterface {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TerminalInterface {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Interface for TerminalInterface {
    async fn receive_input(&self) -> Option<String> {
        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        match reader.read_line(&mut line).await {
            Ok(0) => None, // EOF
            Ok(_) => Some(line.trim().to_string()),
            Err(_) => None,
        }
    }

    async fn send_output(&self, message: &str) {
        let mut stdout = tokio::io::stdout();
        let _ = stdout.write_all(message.as_bytes()).await;
        let _ = stdout.write_all(b"\n").await;
        let _ = stdout.flush().await;
    }

    async fn request_approval(&self, action: &str) -> bool {
        self.send_output(&format!("⚠️  Approval required: {}", action))
            .await;
        self.send_output("Approve? (y/n): ").await;

        if let Some(response) = self.receive_input().await {
            response.to_lowercase().starts_with('y')
        } else {
            false
        }
    }

    async fn show_status(&self, status: &str) {
        self.send_output(&format!("ℹ️  {}", status)).await;
    }
}
