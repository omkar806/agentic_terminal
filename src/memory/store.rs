use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use tracing::info;

use crate::llm::Message;

/// SQLite-backed store that holds both raw messages and generated summaries.
/// Summaries are keyed by session_id and store a compressed version of
/// every 30-message window.
pub struct MemoryStore {
    session_id: String,
    pool: SqlitePool,
}

impl MemoryStore {
    pub async fn new(session_id: &str, db_path: &str) -> anyhow::Result<Self> {
        let url = format!("sqlite:{}?mode=rwc", db_path);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS memory_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT,
                tool_calls TEXT,
                tool_call_id TEXT,
                name TEXT,
                summarized INTEGER NOT NULL DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS memory_summaries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                summary TEXT NOT NULL,
                message_count INTEGER NOT NULL,
                from_message_id INTEGER NOT NULL,
                to_message_id INTEGER NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&pool)
        .await?;

        info!("MemoryStore initialized for session: {}", session_id);

        Ok(Self {
            session_id: session_id.to_string(),
            pool,
        })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Append a message to the memory store.
    pub async fn add_message(&self, message: &Message) -> anyhow::Result<i64> {
        let tool_calls_json = message
            .tool_calls
            .as_ref()
            .map(|tc| serde_json::to_string(tc).unwrap_or_default());

        let result = sqlx::query(
            "INSERT INTO memory_messages (session_id, role, content, tool_calls, tool_call_id, name)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&self.session_id)
        .bind(&message.role)
        .bind(&message.content)
        .bind(&tool_calls_json)
        .bind(&message.tool_call_id)
        .bind(&message.name)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Count how many un-summarized messages exist.
    pub async fn unsummarized_count(&self) -> anyhow::Result<i64> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM memory_messages WHERE session_id = ? AND summarized = 0",
        )
        .bind(&self.session_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    /// Get all un-summarized messages (the ones we'd want to compress).
    pub async fn get_unsummarized_messages(&self) -> anyhow::Result<Vec<(i64, Message)>> {
        let rows = sqlx::query_as::<_, (i64, String, Option<String>, Option<String>, Option<String>, Option<String>)>(
            "SELECT id, role, content, tool_calls, tool_call_id, name
             FROM memory_messages
             WHERE session_id = ? AND summarized = 0
             ORDER BY id ASC",
        )
        .bind(&self.session_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(id, role, content, tool_calls_json, tool_call_id, name)| {
                let tool_calls = tool_calls_json.and_then(|j| serde_json::from_str(&j).ok());
                (
                    id,
                    Message {
                        role,
                        content,
                        tool_calls,
                        tool_call_id,
                        name,
                    },
                )
            })
            .collect())
    }

    /// Mark a range of message IDs as summarized.
    pub async fn mark_as_summarized(&self, from_id: i64, to_id: i64) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE memory_messages SET summarized = 1
             WHERE session_id = ? AND id >= ? AND id <= ?",
        )
        .bind(&self.session_id)
        .bind(from_id)
        .bind(to_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Store a generated summary.
    pub async fn save_summary(
        &self,
        summary: &str,
        message_count: i64,
        from_id: i64,
        to_id: i64,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO memory_summaries (session_id, summary, message_count, from_message_id, to_message_id)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&self.session_id)
        .bind(summary)
        .bind(message_count)
        .bind(from_id)
        .bind(to_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get all summaries for this session, ordered chronologically.
    pub async fn get_summaries(&self) -> anyhow::Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT summary FROM memory_summaries
             WHERE session_id = ?
             ORDER BY id ASC",
        )
        .bind(&self.session_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(s,)| s).collect())
    }

    /// Get the recent (un-summarized) messages that are still "live" in context.
    pub async fn get_recent_messages(&self) -> anyhow::Result<Vec<Message>> {
        let rows = sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, Option<String>)>(
            "SELECT role, content, tool_calls, tool_call_id, name
             FROM memory_messages
             WHERE session_id = ? AND summarized = 0
             ORDER BY id ASC",
        )
        .bind(&self.session_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(role, content, tool_calls_json, tool_call_id, name)| {
                let tool_calls = tool_calls_json.and_then(|j| serde_json::from_str(&j).ok());
                Message {
                    role,
                    content,
                    tool_calls,
                    tool_call_id,
                    name,
                }
            })
            .collect())
    }

    /// Total message count for this session.
    pub async fn total_message_count(&self) -> anyhow::Result<i64> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM memory_messages WHERE session_id = ?",
        )
        .bind(&self.session_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }
}
