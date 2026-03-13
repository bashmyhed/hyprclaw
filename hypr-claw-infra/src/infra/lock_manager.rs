use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LockError {
    #[error("Lock timeout for session: {0}")]
    Timeout(String),
}

struct LockState {
    locked: bool,
    condvar: Arc<Condvar>,
}

#[derive(Clone)]
pub struct LockManager {
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<LockState>>>>>,
    timeout: Duration,
}

pub struct LockMetrics {
    pub wait_time: Duration,
    pub acquired: bool,
}

impl LockManager {
    pub fn new(timeout: Duration) -> Self {
        Self {
            locks: Arc::new(Mutex::new(HashMap::new())),
            timeout,
        }
    }

    pub fn acquire(&self, session_key: &str) -> Result<SessionLock, LockError> {
        let start = Instant::now();

        let lock_state_arc = {
            let mut locks = self.locks.lock();
            locks
                .entry(session_key.to_string())
                .or_insert_with(|| {
                    Arc::new(Mutex::new(LockState {
                        locked: false,
                        condvar: Arc::new(Condvar::new()),
                    }))
                })
                .clone()
        };

        let mut state = lock_state_arc.lock();
        let deadline = start + self.timeout;

        while state.locked {
            let now = Instant::now();
            if now >= deadline {
                return Err(LockError::Timeout(session_key.to_string()));
            }

            let remaining = deadline - now;
            let condvar = Arc::clone(&state.condvar);
            let result = condvar.wait_for(&mut state, remaining);

            if result.timed_out() && state.locked {
                return Err(LockError::Timeout(session_key.to_string()));
            }
        }

        state.locked = true;
        let wait_time = start.elapsed();
        drop(state);

        Ok(SessionLock {
            lock_state: lock_state_arc,
            session_key: session_key.to_string(),
            wait_time,
        })
    }

    pub fn acquire_with_metrics(
        &self,
        session_key: &str,
    ) -> (Result<SessionLock, LockError>, LockMetrics) {
        let start = Instant::now();
        let result = self.acquire(session_key);
        let wait_time = start.elapsed();

        let metrics = LockMetrics {
            wait_time,
            acquired: result.is_ok(),
        };

        (result, metrics)
    }
}

pub struct SessionLock {
    lock_state: Arc<Mutex<LockState>>,
    session_key: String,
    wait_time: Duration,
}

impl SessionLock {
    pub fn wait_time(&self) -> Duration {
        self.wait_time
    }

    pub fn session_key(&self) -> &str {
        &self.session_key
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        let mut state = self.lock_state.lock();
        state.locked = false;
        state.condvar.notify_one();
    }
}
