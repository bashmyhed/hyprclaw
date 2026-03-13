use crate::planning::Plan;
use crate::types::*;
use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("Provider error: {0}")]
    Provider(String),
    #[error("Memory error: {0}")]
    Memory(String),
    #[error("Tool execution error: {0}")]
    Tool(String),
    #[error("Max iterations reached")]
    MaxIterations,
}

#[async_trait]
pub trait LLMProvider: Send + Sync {
    async fn generate(
        &self,
        context: &AgentContext,
        messages: &[HistoryEntry],
    ) -> Result<AgentResponse, EngineError>;
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(
        &self,
        tool_call: &ToolCall,
        context: &AgentContext,
    ) -> Result<ToolResult, EngineError>;
}

pub struct AgentEngine {
    provider: Arc<dyn LLMProvider>,
    executor: Arc<dyn ToolExecutor>,
}

impl AgentEngine {
    pub fn new(provider: Arc<dyn LLMProvider>, executor: Arc<dyn ToolExecutor>) -> Self {
        Self { provider, executor }
    }

    pub async fn execute_task(
        &self,
        context: &mut AgentContext,
        task: &str,
    ) -> Result<String, EngineError> {
        tracing::info!("Starting task execution: {}", task);

        let mut plan = Plan::new(task.to_string());

        // Add task to history
        context
            .persistent_context
            .recent_history
            .push(HistoryEntry {
                timestamp: chrono::Utc::now().timestamp(),
                role: "user".to_string(),
                content: task.to_string(),
            });

        let max_iterations = context.soul_config.max_iterations;

        for iteration in 0..max_iterations {
            tracing::debug!("Iteration {}/{}", iteration + 1, max_iterations);

            // Generate response from LLM
            let response = self
                .provider
                .generate(context, &context.persistent_context.recent_history)
                .await?;

            // Handle tool calls
            if !response.tool_calls.is_empty() {
                for tool_call in &response.tool_calls {
                    tracing::info!("Executing tool: {}", tool_call.name);

                    plan.add_step(format!("Execute tool: {}", tool_call.name));

                    let result = self.executor.execute(tool_call, context).await?;

                    if result.success {
                        plan.complete_step(
                            serde_json::to_string(&result.output).unwrap_or_default(),
                        );
                    } else {
                        plan.fail_step(result.error.clone().unwrap_or_default());
                    }

                    // Add tool result to history
                    context
                        .persistent_context
                        .recent_history
                        .push(HistoryEntry {
                            timestamp: chrono::Utc::now().timestamp(),
                            role: "tool".to_string(),
                            content: serde_json::to_string(&result).unwrap_or_default(),
                        });
                }
            }

            // Add assistant response to history
            if let Some(content) = &response.content {
                context
                    .persistent_context
                    .recent_history
                    .push(HistoryEntry {
                        timestamp: chrono::Utc::now().timestamp(),
                        role: "assistant".to_string(),
                        content: content.clone(),
                    });
            }

            // Check if task is complete
            if response.completed {
                tracing::info!("Task completed. Progress: {:.1}%", plan.progress() * 100.0);
                return Ok(response.content.unwrap_or_default());
            }
        }

        Err(EngineError::MaxIterations)
    }
}
