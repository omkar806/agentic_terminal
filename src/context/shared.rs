use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared mutable state accessible by the orchestrator and all sub-agents.
/// Wrapped in Arc<RwLock<>> for safe concurrent access across tokio tasks.
#[derive(Debug, Clone)]
pub struct SharedContext {
    pub task_id: String,
    pub working_dir: PathBuf,
    pub env_vars: HashMap<String, String>,
    pub results: Arc<RwLock<HashMap<String, String>>>,
    pub logs: Arc<RwLock<Vec<LogEntry>>>,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub agent_name: String,
    pub action: String,
    pub detail: String,
}

impl SharedContext {
    pub fn new(task_id: &str, working_dir: PathBuf) -> Self {
        Self {
            task_id: task_id.to_string(),
            working_dir,
            env_vars: HashMap::new(),
            results: Arc::new(RwLock::new(HashMap::new())),
            logs: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn store_result(&self, key: &str, value: &str) {
        self.results
            .write()
            .await
            .insert(key.to_string(), value.to_string());
    }

    pub async fn get_result(&self, key: &str) -> Option<String> {
        self.results.read().await.get(key).cloned()
    }

    pub async fn log(&self, agent: &str, action: &str, detail: &str) {
        self.logs.write().await.push(LogEntry {
            timestamp: chrono::Utc::now(),
            agent_name: agent.to_string(),
            action: action.to_string(),
            detail: detail.to_string(),
        });
    }

    pub async fn get_logs(&self) -> Vec<LogEntry> {
        self.logs.read().await.clone()
    }
}
