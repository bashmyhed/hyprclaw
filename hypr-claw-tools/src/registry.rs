use crate::tools::Tool;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

#[derive(Clone)]
struct ToolEntry {
    tool: Arc<dyn Tool>,
    is_core: bool,
    ttl: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenToolDoc {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenToolSnapshot {
    pub docs: Vec<HiddenToolDoc>,
    pub version: u64,
}

#[derive(Clone)]
pub struct ToolRegistryImpl {
    tools: Arc<RwLock<HashMap<String, ToolEntry>>>,
    version: Arc<AtomicU64>,
}

impl ToolRegistryImpl {
    pub fn new() -> Self {
        Self {
            tools: Arc::new(RwLock::new(HashMap::new())),
            version: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) -> &mut Self {
        self.register_core(tool)
    }

    pub fn register_core(&mut self, tool: Arc<dyn Tool>) -> &mut Self {
        self.insert(tool, true, 0);
        self
    }

    pub fn register_hidden(&mut self, tool: Arc<dyn Tool>) -> &mut Self {
        self.insert(tool, false, 0);
        self
    }

    fn insert(&mut self, tool: Arc<dyn Tool>, is_core: bool, ttl: usize) {
        self.tools.write().expect("registry poisoned").insert(
            tool.name().to_string(),
            ToolEntry {
                tool,
                is_core,
                ttl,
            },
        );
        self.version.fetch_add(1, Ordering::SeqCst);
    }

    pub fn promote_tools(&self, names: &[String], ttl: usize) {
        let mut promoted = false;
        let mut tools = self.tools.write().expect("registry poisoned");
        for name in names {
            if let Some(entry) = tools.get_mut(name) {
                if !entry.is_core {
                    entry.ttl = ttl;
                    promoted = true;
                }
            }
        }
        if promoted {
            self.version.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub fn tick_ttl(&self) {
        let mut changed = false;
        let mut tools = self.tools.write().expect("registry poisoned");
        for entry in tools.values_mut() {
            if !entry.is_core && entry.ttl > 0 {
                entry.ttl -= 1;
                changed = true;
            }
        }
        if changed {
            self.version.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }

    pub fn snapshot_hidden_tools(&self) -> HiddenToolSnapshot {
        HiddenToolSnapshot {
            docs: self
                .sorted_entries()
                .into_iter()
                .filter(|(_, entry)| !entry.is_core)
                .map(|(name, entry)| HiddenToolDoc {
                    name,
                    description: entry.tool.description().to_string(),
                })
                .collect(),
            version: self.version(),
        }
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools
            .read()
            .expect("registry poisoned")
            .get(name)
            .and_then(|entry| {
            if entry.is_core || entry.ttl > 0 {
                Some(entry.tool.clone())
            } else {
                None
            }
        })
    }

    pub fn list(&self) -> Vec<String> {
        self.sorted_entries()
            .into_iter()
            .filter(|(_, entry)| entry.is_core || entry.ttl > 0)
            .map(|(name, _)| name)
            .collect()
    }

    pub fn count(&self) -> usize {
        self.list().len()
    }

    pub fn schemas(&self) -> Vec<serde_json::Value> {
        self.sorted_entries()
            .into_iter()
            .filter(|(_, entry)| entry.is_core || entry.ttl > 0)
            .map(|(_, entry)| {
                json!({
                    "type": "function",
                    "function": {
                        "name": entry.tool.name(),
                        "description": entry.tool.description(),
                        "parameters": entry.tool.schema()
                    }
                })
            })
            .collect()
    }

    fn sorted_entries(&self) -> Vec<(String, ToolEntry)> {
        let mut entries = self
            .tools
            .read()
            .expect("registry poisoned")
            .iter()
            .map(|(name, entry)| (name.clone(), entry.clone()))
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }
}

impl Default for ToolRegistryImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::EchoTool;

    #[test]
    fn hidden_tools_are_not_visible_until_promoted() {
        let mut registry = ToolRegistryImpl::new();
        registry.register_hidden(Arc::new(EchoTool));

        assert!(registry.get("echo").is_none());
        assert!(registry.list().is_empty());
    }

    #[test]
    fn hidden_tools_become_visible_when_promoted_and_expire_after_ticks() {
        let mut registry = ToolRegistryImpl::new();
        registry.register_hidden(Arc::new(EchoTool));
        registry.promote_tools(&["echo".to_string()], 2);

        assert!(registry.get("echo").is_some());
        assert_eq!(registry.list(), vec!["echo".to_string()]);

        registry.tick_ttl();
        assert!(registry.get("echo").is_some());

        registry.tick_ttl();
        assert!(registry.get("echo").is_none());
        assert!(registry.list().is_empty());
    }

    #[test]
    fn snapshot_hidden_tools_is_deterministic() {
        let mut registry = ToolRegistryImpl::new();
        registry.register_hidden(Arc::new(EchoTool));
        let snapshot = registry.snapshot_hidden_tools();

        assert_eq!(
            snapshot.docs,
            vec![HiddenToolDoc {
                name: "echo".to_string(),
                description: "Echoes input back".to_string()
            }]
        );
    }
}
