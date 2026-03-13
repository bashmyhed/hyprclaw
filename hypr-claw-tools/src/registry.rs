use crate::tools::Tool;
use std::collections::HashMap;
use std::sync::Arc;

pub struct ToolRegistryImpl {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistryImpl {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) -> &mut Self {
        self.tools.insert(tool.name().to_string(), tool);
        self
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn list(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    pub fn count(&self) -> usize {
        self.tools.len()
    }

    pub fn schemas(&self) -> Vec<serde_json::Value> {
        self.tools
            .values()
            .map(|tool| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": tool.name(),
                        "description": tool.description(),
                        "parameters": tool.schema()
                    }
                })
            })
            .collect()
    }
}

impl Default for ToolRegistryImpl {
    fn default() -> Self {
        Self::new()
    }
}
