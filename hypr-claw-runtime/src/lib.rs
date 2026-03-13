//! Hypr-Claw Runtime Core
//!
//! A production-grade agent runtime kernel implementing deterministic control flow.

pub mod agent_config;
pub mod agent_loop;
pub mod async_adapters;
pub mod codex_adapter;
pub mod compactor;
pub mod gateway;
pub mod interfaces;
pub mod llm_client;
pub mod llm_client_type;
pub mod metrics;
pub mod runtime_controller;
pub mod types;

pub use agent_config::{load_agent_config, AgentConfig};
pub use agent_loop::AgentLoop;
pub use async_adapters::{AsyncLockManager, AsyncSessionStore};
pub use codex_adapter::CodexAdapter;
pub use compactor::{Compactor, Summarizer};
pub use gateway::resolve_session;
pub use interfaces::{LockManager, RuntimeError, SessionStore, ToolDispatcher, ToolRegistry};
pub use llm_client::LLMClient;
pub use llm_client_type::LLMClientType;
pub use runtime_controller::RuntimeController;
pub use types::{LLMResponse, Message, Role, SCHEMA_VERSION};
