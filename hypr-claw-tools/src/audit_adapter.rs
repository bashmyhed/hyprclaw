// Audit adapter - wraps Worker 3 AuditLogger
// When Worker 3 is available, replace with actual implementation

use std::sync::Arc;

pub struct AuditLogger;

impl AuditLogger {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

pub struct AuditAdapter {
    logger: Arc<AuditLogger>,
}

impl AuditAdapter {
    pub fn new(logger: Arc<AuditLogger>) -> Self {
        Self { logger }
    }

    pub fn logger(&self) -> &AuditLogger {
        &self.logger
    }
}
