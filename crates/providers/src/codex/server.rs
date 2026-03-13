use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct OAuthServer {
    state: String,
}

impl OAuthServer {
    pub fn new(state: String) -> Self {
        Self { state }
    }

    pub async fn wait_for_callback(&self) -> Result<String, Box<dyn std::error::Error>> {
        let listener = match TcpListener::bind("127.0.0.1:1455").await {
            Ok(l) => l,
            Err(_) => {
                println!("\n[Codex] Port 1455 is busy. Please paste the redirect URL:");
                return self.manual_callback().await;
            }
        };

        println!("[OAuth] Listening on http://127.0.0.1:1455");

        let timeout = tokio::time::timeout(Duration::from_secs(300), async {
            loop {
                let (mut socket, _) = listener.accept().await?;
                let mut buffer = vec![0; 4096];
                let n = socket.read(&mut buffer).await?;
                let request = String::from_utf8_lossy(&buffer[..n]);

                if let Some(code) = self.parse_callback(&request) {
                    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                        <html><body><h1>Authentication successful!</h1>\
                        <p>You can close this window and return to the terminal.</p></body></html>";
                    socket.write_all(response.as_bytes()).await?;
                    return Ok(code);
                }
            }
        })
        .await;

        match timeout {
            Ok(result) => result,
            Err(_) => Err("OAuth timeout after 5 minutes".into()),
        }
    }

    fn parse_callback(&self, request: &str) -> Option<String> {
        let first_line = request.lines().next()?;
        let path = first_line.split_whitespace().nth(1)?;

        if !path.starts_with("/auth/callback") {
            return None;
        }

        let query = path.split('?').nth(1)?;
        let params: std::collections::HashMap<_, _> = query
            .split('&')
            .filter_map(|pair| {
                let mut parts = pair.split('=');
                Some((parts.next()?, parts.next()?))
            })
            .collect();

        let state = params.get("state")?;
        if *state != self.state {
            eprintln!("[OAuth] State mismatch! Possible CSRF attack.");
            return None;
        }

        params.get("code").map(|s| s.to_string())
    }

    async fn manual_callback(&self) -> Result<String, Box<dyn std::error::Error>> {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        let url = input.trim();
        if url.contains("code=") {
            let code = url
                .split("code=")
                .nth(1)
                .and_then(|s| s.split('&').next())
                .ok_or("Invalid URL format")?;
            Ok(code.to_string())
        } else {
            Err("No authorization code found in URL".into())
        }
    }
}
