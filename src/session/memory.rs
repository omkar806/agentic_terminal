use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::trait_def::Session;
use crate::llm::Message;

/// In-memory session for ephemeral sub-agent conversations.
/// No persistence — ideal for spawned agents that don't need long-term memory.
#[derive(Debug, Clone)]
pub struct InMemorySession {
    messages: Arc<RwLock<Vec<Message>>>,
}

impl InMemorySession {
    pub fn new() -> Self {
        Self {
            messages: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl Default for InMemorySession {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Session for InMemorySession {
    async fn get_messages(&self, limit: Option<usize>) -> anyhow::Result<Vec<Message>> {
        let msgs = self.messages.read().await;
        match limit {
            Some(n) => Ok(msgs.iter().rev().take(n).rev().cloned().collect()),
            None => Ok(msgs.clone()),
        }
    }

    async fn add_message(&self, message: Message) -> anyhow::Result<()> {
        self.messages.write().await.push(message);
        Ok(())
    }

    async fn add_messages(&self, messages: Vec<Message>) -> anyhow::Result<()> {
        self.messages.write().await.extend(messages);
        Ok(())
    }

    async fn clear(&self) -> anyhow::Result<()> {
        self.messages.write().await.clear();
        Ok(())
    }
}
