use async_trait::async_trait;
use crate::llm::Message;

/// Session trait — the Rust equivalent of the Python SDK's Session protocol.
/// Manages persistent conversation history across agent runs.
#[async_trait]
pub trait Session: Send + Sync {
    async fn get_messages(&self, limit: Option<usize>) -> anyhow::Result<Vec<Message>>;
    async fn add_message(&self, message: Message) -> anyhow::Result<()>;
    async fn add_messages(&self, messages: Vec<Message>) -> anyhow::Result<()>;
    async fn clear(&self) -> anyhow::Result<()>;
}
