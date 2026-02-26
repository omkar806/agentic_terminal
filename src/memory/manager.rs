use tracing::info;

use crate::display::Printer;
use crate::llm::{LlmClient, Message};
use crate::memory::store::MemoryStore;
use crate::memory::summarizer;

const SUMMARY_THRESHOLD: i64 = 30;

/// MemoryManager wraps a MemoryStore and handles the automatic
/// summarization lifecycle:
///   1. Every message goes into the store
///   2. When un-summarized messages hit 30, trigger summarization
///   3. When building context for the LLM, inject summaries + recent messages
pub struct MemoryManager {
    store: MemoryStore,
    model: String,
}

impl MemoryManager {
    pub async fn new(session_id: &str, db_path: &str, model: &str) -> anyhow::Result<Self> {
        let store = MemoryStore::new(session_id, db_path).await?;
        Ok(Self {
            store,
            model: model.to_string(),
        })
    }

    /// Record a user message.
    pub async fn add_user_message(&self, content: &str) -> anyhow::Result<()> {
        self.store.add_message(&Message::user(content)).await?;
        Ok(())
    }

    /// Record an assistant message.
    pub async fn add_assistant_message(&self, content: &str) -> anyhow::Result<()> {
        self.store.add_message(&Message::assistant(content)).await?;
        Ok(())
    }

    /// Check if we've accumulated enough messages to warrant a summary,
    /// and if so, generate one. Call this after each conversation turn.
    pub async fn maybe_summarize(
        &self,
        llm: &LlmClient,
        printer: &Printer,
    ) -> anyhow::Result<bool> {
        let count = self.store.unsummarized_count().await?;

        if count < SUMMARY_THRESHOLD {
            return Ok(false);
        }

        info!(
            "Memory threshold reached ({} >= {}), generating summary...",
            count, SUMMARY_THRESHOLD
        );

        printer.memory_event("compressing memory", &format!("{} messages -> summary", count));

        let id_messages = self.store.get_unsummarized_messages().await?;
        let messages: Vec<Message> = id_messages.iter().map(|(_, m)| m.clone()).collect();
        let first_id = id_messages.first().map(|(id, _)| *id).unwrap_or(0);
        let last_id = id_messages.last().map(|(id, _)| *id).unwrap_or(0);

        let summary = summarizer::summarize_messages(&messages, llm, &self.model).await?;

        self.store
            .save_summary(&summary, count, first_id, last_id)
            .await?;
        self.store.mark_as_summarized(first_id, last_id).await?;

        printer.memory_event("memory compressed", &format!("{} chars summary stored", summary.len()));

        Ok(true)
    }

    /// Build the effective conversation history for the LLM:
    ///   [summaries as system context] + [recent un-summarized messages]
    ///
    /// This is what gets injected into the Runner instead of raw session history.
    pub async fn build_context_messages(&self) -> anyhow::Result<Vec<Message>> {
        let mut messages = Vec::new();

        let summaries = self.store.get_summaries().await?;
        if !summaries.is_empty() {
            let combined = summaries.join("\n\n---\n\n");
            messages.push(Message::system(&format!(
                "## Previous Conversation Context\n\
                 The following is a summary of earlier conversation turns:\n\n{}",
                combined
            )));
        }

        let recent = self.store.get_recent_messages().await?;
        messages.extend(recent);

        Ok(messages)
    }

    /// Get stats about the memory state.
    pub async fn stats(&self) -> anyhow::Result<MemoryStats> {
        let total = self.store.total_message_count().await?;
        let unsummarized = self.store.unsummarized_count().await?;
        let summaries = self.store.get_summaries().await?;

        Ok(MemoryStats {
            total_messages: total,
            unsummarized_messages: unsummarized,
            summary_count: summaries.len() as i64,
            next_summary_in: SUMMARY_THRESHOLD - unsummarized,
        })
    }
}

#[derive(Debug)]
pub struct MemoryStats {
    pub total_messages: i64,
    pub unsummarized_messages: i64,
    pub summary_count: i64,
    pub next_summary_in: i64,
}
