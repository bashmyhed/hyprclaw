use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RateLimitError {
    #[error("Rate limit exceeded for {0}")]
    Exceeded(String),
}

#[derive(Clone)]
pub struct RateLimitConfig {
    pub max_requests: usize,
    pub window: Duration,
}

impl RateLimitConfig {
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            max_requests,
            window,
        }
    }
}

struct TokenBucket {
    tokens: usize,
    last_refill: Instant,
    config: RateLimitConfig,
}

impl TokenBucket {
    fn new(config: RateLimitConfig) -> Self {
        Self {
            tokens: config.max_requests,
            last_refill: Instant::now(),
            config,
        }
    }

    fn try_consume(&mut self) -> bool {
        self.refill();

        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);

        if elapsed >= self.config.window {
            self.tokens = self.config.max_requests;
            self.last_refill = now;
        }
    }
}

pub struct RateLimiter {
    per_session: Mutex<HashMap<String, TokenBucket>>,
    per_tool: Mutex<HashMap<String, TokenBucket>>,
    global: Mutex<TokenBucket>,
    session_config: RateLimitConfig,
    tool_config: RateLimitConfig,
    #[allow(dead_code)]
    global_config: RateLimitConfig,
}

impl RateLimiter {
    pub fn new(
        session_config: RateLimitConfig,
        tool_config: RateLimitConfig,
        global_config: RateLimitConfig,
    ) -> Self {
        Self {
            per_session: Mutex::new(HashMap::new()),
            per_tool: Mutex::new(HashMap::new()),
            global: Mutex::new(TokenBucket::new(global_config.clone())),
            session_config,
            tool_config,
            global_config,
        }
    }

    pub fn check_session(&self, session_key: &str) -> Result<(), RateLimitError> {
        let mut sessions = self.per_session.lock();
        let bucket = sessions
            .entry(session_key.to_string())
            .or_insert_with(|| TokenBucket::new(self.session_config.clone()));

        if bucket.try_consume() {
            Ok(())
        } else {
            Err(RateLimitError::Exceeded(format!("session:{}", session_key)))
        }
    }

    pub fn check_tool(&self, tool_name: &str) -> Result<(), RateLimitError> {
        let mut tools = self.per_tool.lock();
        let bucket = tools
            .entry(tool_name.to_string())
            .or_insert_with(|| TokenBucket::new(self.tool_config.clone()));

        if bucket.try_consume() {
            Ok(())
        } else {
            Err(RateLimitError::Exceeded(format!("tool:{}", tool_name)))
        }
    }

    pub fn check_global(&self) -> Result<(), RateLimitError> {
        let mut global = self.global.lock();

        if global.try_consume() {
            Ok(())
        } else {
            Err(RateLimitError::Exceeded("global".to_string()))
        }
    }

    pub fn check_all(&self, session_key: &str, tool_name: &str) -> Result<(), RateLimitError> {
        self.check_global()?;
        self.check_session(session_key)?;
        self.check_tool(tool_name)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_session_rate_limit() {
        let limiter = RateLimiter::new(
            RateLimitConfig::new(3, Duration::from_secs(1)),
            RateLimitConfig::new(100, Duration::from_secs(1)),
            RateLimitConfig::new(1000, Duration::from_secs(1)),
        );

        assert!(limiter.check_session("session1").is_ok());
        assert!(limiter.check_session("session1").is_ok());
        assert!(limiter.check_session("session1").is_ok());
        assert!(limiter.check_session("session1").is_err());
    }

    #[test]
    fn test_tool_rate_limit() {
        let limiter = RateLimiter::new(
            RateLimitConfig::new(100, Duration::from_secs(1)),
            RateLimitConfig::new(2, Duration::from_secs(1)),
            RateLimitConfig::new(1000, Duration::from_secs(1)),
        );

        assert!(limiter.check_tool("read_file").is_ok());
        assert!(limiter.check_tool("read_file").is_ok());
        assert!(limiter.check_tool("read_file").is_err());
    }

    #[test]
    fn test_global_rate_limit() {
        let limiter = RateLimiter::new(
            RateLimitConfig::new(100, Duration::from_secs(1)),
            RateLimitConfig::new(100, Duration::from_secs(1)),
            RateLimitConfig::new(5, Duration::from_secs(1)),
        );

        for _ in 0..5 {
            assert!(limiter.check_global().is_ok());
        }
        assert!(limiter.check_global().is_err());
    }

    #[test]
    fn test_window_reset() {
        let limiter = RateLimiter::new(
            RateLimitConfig::new(2, Duration::from_millis(100)),
            RateLimitConfig::new(100, Duration::from_secs(1)),
            RateLimitConfig::new(1000, Duration::from_secs(1)),
        );

        assert!(limiter.check_session("session1").is_ok());
        assert!(limiter.check_session("session1").is_ok());
        assert!(limiter.check_session("session1").is_err());

        thread::sleep(Duration::from_millis(150));

        assert!(limiter.check_session("session1").is_ok());
    }

    #[test]
    fn test_per_session_isolation() {
        let limiter = RateLimiter::new(
            RateLimitConfig::new(2, Duration::from_secs(1)),
            RateLimitConfig::new(100, Duration::from_secs(1)),
            RateLimitConfig::new(1000, Duration::from_secs(1)),
        );

        assert!(limiter.check_session("session1").is_ok());
        assert!(limiter.check_session("session1").is_ok());
        assert!(limiter.check_session("session1").is_err());

        // Different session should have its own limit
        assert!(limiter.check_session("session2").is_ok());
        assert!(limiter.check_session("session2").is_ok());
        assert!(limiter.check_session("session2").is_err());
    }

    #[test]
    fn test_per_tool_isolation() {
        let limiter = RateLimiter::new(
            RateLimitConfig::new(100, Duration::from_secs(1)),
            RateLimitConfig::new(2, Duration::from_secs(1)),
            RateLimitConfig::new(1000, Duration::from_secs(1)),
        );

        assert!(limiter.check_tool("read_file").is_ok());
        assert!(limiter.check_tool("read_file").is_ok());
        assert!(limiter.check_tool("read_file").is_err());

        // Different tool should have its own limit
        assert!(limiter.check_tool("write_file").is_ok());
        assert!(limiter.check_tool("write_file").is_ok());
        assert!(limiter.check_tool("write_file").is_err());
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;

        let limiter = Arc::new(RateLimiter::new(
            RateLimitConfig::new(10, Duration::from_secs(1)),
            RateLimitConfig::new(100, Duration::from_secs(1)),
            RateLimitConfig::new(1000, Duration::from_secs(1)),
        ));

        let mut handles = vec![];

        for i in 0..5 {
            let limiter_clone = Arc::clone(&limiter);
            let handle = thread::spawn(move || {
                let session = format!("session{}", i);
                limiter_clone.check_session(&session).unwrap();
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_check_all() {
        let limiter = RateLimiter::new(
            RateLimitConfig::new(2, Duration::from_secs(1)),
            RateLimitConfig::new(2, Duration::from_secs(1)),
            RateLimitConfig::new(2, Duration::from_secs(1)),
        );

        assert!(limiter.check_all("session1", "tool1").is_ok());
        assert!(limiter.check_all("session1", "tool1").is_ok());

        // Global limit hit
        assert!(limiter.check_all("session1", "tool1").is_err());
    }
}
