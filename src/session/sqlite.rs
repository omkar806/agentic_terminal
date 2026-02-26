use async_trait::async_trait;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use tracing::info;

use super::trait_def::Session;
use crate::llm::Message;

/// SQLite-backed session for persistent conversation memory across CLI runs.
pub struct SqliteSession {
    session_id: String,
    pool: SqlitePool,
}

impl SqliteSession {
    pub async fn new(session_id: &str, db_path: &str) -> anyhow::Result<Self> {
        let url = format!("sqlite:{}?mode=rwc", db_path);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT,
                tool_calls TEXT,
                tool_call_id TEXT,
                name TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&pool)
        .await?;

        info!("SQLite session initialized: {}", session_id);

        Ok(Self {
            session_id: session_id.to_string(),
            pool,
        })
    }
}

#[async_trait]
impl Session for SqliteSession {
    async fn get_messages(&self, limit: Option<usize>) -> anyhow::Result<Vec<Message>> {
        let limit_clause = match limit {
            Some(n) => format!("LIMIT {}", n),
            None => String::new(),
        };

        let query = format!(
            "SELECT role, content, tool_calls, tool_call_id, name \
             FROM messages WHERE session_id = ? \
             ORDER BY id ASC {}",
            limit_clause
        );

        let rows = sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, Option<String>)>(
            &query,
        )
        .bind(&self.session_id)
        .fetch_all(&self.pool)
        .await?;

        let messages = rows
            .into_iter()
            .map(|(role, content, tool_calls_json, tool_call_id, name)| {
                let tool_calls = tool_calls_json
                    .and_then(|json| serde_json::from_str(&json).ok());
                Message {
                    role,
                    content,
                    tool_calls,
                    tool_call_id,
                    name,
                }
            })
            .collect();

        Ok(messages)
    }

    async fn add_message(&self, message: Message) -> anyhow::Result<()> {
        let tool_calls_json = message
            .tool_calls
            .as_ref()
            .map(|tc| serde_json::to_string(tc).unwrap_or_default());

        sqlx::query(
            "INSERT INTO messages (session_id, role, content, tool_calls, tool_call_id, name) \
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

        Ok(())
    }

    async fn add_messages(&self, messages: Vec<Message>) -> anyhow::Result<()> {
        for msg in messages {
            self.add_message(msg).await?;
        }
        Ok(())
    }

    async fn clear(&self) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM messages WHERE session_id = ?")
            .bind(&self.session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
