use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

#[derive(Debug, Error)]
pub enum TaskError {
    #[error("Task not found: {0}")]
    NotFound(String),
    #[error("Task already exists: {0}")]
    AlreadyExists(String),
    #[error("Task execution failed: {0}")]
    ExecutionFailed(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub progress: f32,
    pub created_at: i64,
    pub updated_at: i64,
    pub result: Option<String>,
    pub error: Option<String>,
}

struct TaskHandle {
    info: TaskInfo,
    handle: Option<JoinHandle<Result<String, String>>>,
}

pub struct TaskManager {
    tasks: Arc<RwLock<HashMap<String, Arc<Mutex<TaskHandle>>>>>,
    state_file: Option<PathBuf>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            state_file: None,
        }
    }

    pub fn with_state_file<P: AsRef<Path>>(state_file: P) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            state_file: Some(state_file.as_ref().to_path_buf()),
        }
    }

    pub async fn restore(&self) -> Result<(), TaskError> {
        let Some(state_file) = &self.state_file else {
            return Ok(());
        };

        if !state_file.exists() {
            return Ok(());
        }

        let content = tokio::fs::read_to_string(state_file).await?;
        let mut task_infos: Vec<TaskInfo> = serde_json::from_str(&content)?;
        let now = chrono::Utc::now().timestamp();

        for task in &mut task_infos {
            if task.status == TaskStatus::Running || task.status == TaskStatus::Pending {
                task.status = TaskStatus::Failed;
                task.error = Some("Interrupted by restart".to_string());
                task.updated_at = now;
            }
        }

        let mut tasks = self.tasks.write().await;
        tasks.clear();
        for info in task_infos {
            tasks.insert(
                info.id.clone(),
                Arc::new(Mutex::new(TaskHandle { info, handle: None })),
            );
        }
        drop(tasks);

        self.persist_state().await?;
        Ok(())
    }

    pub async fn spawn_task<F, Fut>(
        &self,
        id: String,
        description: String,
        task_fn: F,
    ) -> Result<String, TaskError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
        let tasks = self.tasks.read().await;
        if tasks.contains_key(&id) {
            return Err(TaskError::AlreadyExists(id));
        }
        drop(tasks);

        let now = chrono::Utc::now().timestamp();
        let info = TaskInfo {
            id: id.clone(),
            description,
            status: TaskStatus::Running,
            progress: 0.0,
            created_at: now,
            updated_at: now,
            result: None,
            error: None,
        };

        let handle = tokio::spawn(task_fn());

        let task_handle = Arc::new(Mutex::new(TaskHandle {
            info,
            handle: Some(handle),
        }));

        let mut tasks = self.tasks.write().await;
        tasks.insert(id.clone(), task_handle);
        drop(tasks);
        self.persist_state().await?;

        tracing::info!("Spawned task: {}", id);
        Ok(id)
    }

    pub async fn get_status(&self, id: &str) -> Result<TaskInfo, TaskError> {
        let tasks = self.tasks.read().await;
        let task = tasks
            .get(id)
            .ok_or_else(|| TaskError::NotFound(id.to_string()))?;

        let mut task_guard = task.lock().await;
        let mut changed = false;

        // Check if task completed
        if let Some(handle) = &mut task_guard.handle {
            if handle.is_finished() {
                let result = handle.await;
                task_guard.handle = None;

                match result {
                    Ok(Ok(output)) => {
                        task_guard.info.status = TaskStatus::Completed;
                        task_guard.info.progress = 1.0;
                        task_guard.info.result = Some(output);
                    }
                    Ok(Err(error)) => {
                        task_guard.info.status = TaskStatus::Failed;
                        task_guard.info.error = Some(error);
                    }
                    Err(e) => {
                        task_guard.info.status = TaskStatus::Failed;
                        task_guard.info.error = Some(e.to_string());
                    }
                }
                task_guard.info.updated_at = chrono::Utc::now().timestamp();
                changed = true;
            }
        }

        let info = task_guard.info.clone();
        drop(task_guard);
        drop(tasks);

        if changed {
            self.persist_state().await?;
        }

        Ok(info)
    }

    pub async fn cancel_task(&self, id: &str) -> Result<(), TaskError> {
        let tasks = self.tasks.read().await;
        let task = tasks
            .get(id)
            .ok_or_else(|| TaskError::NotFound(id.to_string()))?;

        let mut task_guard = task.lock().await;

        if let Some(handle) = task_guard.handle.take() {
            handle.abort();
            task_guard.info.status = TaskStatus::Cancelled;
            task_guard.info.updated_at = chrono::Utc::now().timestamp();
            tracing::info!("Cancelled task: {}", id);
        }
        drop(task_guard);
        drop(tasks);
        self.persist_state().await?;

        Ok(())
    }

    pub async fn list_tasks(&self) -> Vec<TaskInfo> {
        let tasks = self.tasks.read().await;
        let mut result = Vec::new();

        for task in tasks.values() {
            let task_guard = task.lock().await;
            result.push(task_guard.info.clone());
        }

        result
    }

    pub async fn cleanup_completed(&self) {
        let mut tasks = self.tasks.write().await;
        tasks.retain(|_, task| {
            let task_guard = task.try_lock();
            if let Ok(guard) = task_guard {
                !matches!(
                    guard.info.status,
                    TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
                )
            } else {
                true
            }
        });
        drop(tasks);
        let _ = self.persist_state().await;
    }

    async fn persist_state(&self) -> Result<(), TaskError> {
        let Some(state_file) = &self.state_file else {
            return Ok(());
        };

        let tasks = self.tasks.read().await;
        let mut snapshot = Vec::with_capacity(tasks.len());
        for task in tasks.values() {
            let guard = task.lock().await;
            snapshot.push(guard.info.clone());
        }

        if let Some(parent) = state_file.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let tmp_file = state_file.with_extension("tmp");
        let content = serde_json::to_string_pretty(&snapshot)?;
        tokio::fs::write(&tmp_file, content).await?;
        tokio::fs::rename(tmp_file, state_file).await?;

        Ok(())
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_and_complete() {
        let manager = TaskManager::new();

        let task_id = manager
            .spawn_task("test_task".to_string(), "Test task".to_string(), || async {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                Ok("completed".to_string())
            })
            .await
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let status = manager.get_status(&task_id).await.unwrap();
        assert_eq!(status.status, TaskStatus::Completed);
        assert_eq!(status.result, Some("completed".to_string()));
    }

    #[tokio::test]
    async fn test_cancel_task() {
        let manager = TaskManager::new();

        let task_id = manager
            .spawn_task("long_task".to_string(), "Long task".to_string(), || async {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                Ok("done".to_string())
            })
            .await
            .unwrap();

        manager.cancel_task(&task_id).await.unwrap();

        let status = manager.get_status(&task_id).await.unwrap();
        assert_eq!(status.status, TaskStatus::Cancelled);
    }
}
