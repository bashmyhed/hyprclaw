// Permission adapter - wraps Worker 3 PermissionEngine
// When Worker 3 is available, replace with actual implementation

use std::sync::Arc;

pub struct PermissionEngine;

impl PermissionEngine {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

pub struct PermissionAdapter {
    engine: Arc<PermissionEngine>,
}

impl PermissionAdapter {
    pub fn new(engine: Arc<PermissionEngine>) -> Self {
        Self { engine }
    }

    pub fn engine(&self) -> &PermissionEngine {
        &self.engine
    }
}
