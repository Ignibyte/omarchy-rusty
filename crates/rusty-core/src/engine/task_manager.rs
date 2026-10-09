//! Task state machine with SQLite persistence.

use crate::engine::db::Database;
use std::sync::Arc;

/// Task states in the lifecycle.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Running,
    Completed,
    Failed,
}

impl TaskState {
    fn from_str(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// A task record from the database.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Task {
    pub id: String,
    pub prompt: String,
    pub state: TaskState,
    pub result: String,
    pub error: String,
    pub session_id: String,
    pub claude_session_id: String,
    pub conversation_id: String,
    pub cost_usd: f64,
    pub num_turns: i64,
    pub duration_ms: i64,
    pub created_at: i64,
    pub started_at: i64,
    pub completed_at: i64,
}

/// Summary of a conversation for sidebar display.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConversationSummary {
    /// Unique conversation ID.
    pub id: String,
    /// Title derived from the first prompt, truncated.
    pub title: String,
    /// Number of completed tasks (each = 1 exchange).
    pub message_count: i64,
    /// Unix timestamp of last activity.
    pub last_activity: i64,
    /// Unix timestamp when conversation was created.
    pub created_at: i64,
    /// Whether TTS is muted for this conversation.
    pub tts_muted: bool,
}

/// Manages task lifecycle in SQLite.
pub struct TaskManager {
    db: Arc<Database>,
}

impl TaskManager {
    /// Create a new TaskManager backed by the given database.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Create a new task in PENDING state. Returns the task ID.
    pub fn create_task(
        &self,
        prompt: &str,
        conversation_id: &str,
        session_id: &str,
    ) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono_now();

        let conn = self.db.conn()?;
        conn.execute(
            "INSERT INTO tasks (id, prompt, state, conversation_id, session_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, prompt, "pending", conversation_id, session_id, now],
        )
        .map_err(|e| format!("Failed to create task: {e}"))?;

        Ok(id)
    }

    /// Transition a task to RUNNING state.
    pub fn set_running(&self, task_id: &str) -> Result<(), String> {
        let conn = self.db.conn()?;
        conn.execute(
            "UPDATE tasks SET state = 'running', started_at = ?1 WHERE id = ?2",
            rusqlite::params![chrono_now(), task_id],
        )
        .map_err(|e| format!("Failed to update task: {e}"))?;
        Ok(())
    }

    /// Mark a task as COMPLETED with result data.
    pub fn set_completed(
        &self,
        task_id: &str,
        result: &str,
        claude_session_id: &str,
        cost_usd: f64,
        num_turns: i64,
        duration_ms: i64,
    ) -> Result<(), String> {
        let conn = self.db.conn()?;
        conn.execute(
            "UPDATE tasks SET state = 'completed', result = ?1, claude_session_id = ?2, cost_usd = ?3, num_turns = ?4, duration_ms = ?5, completed_at = ?6 WHERE id = ?7",
            rusqlite::params![result, claude_session_id, cost_usd, num_turns, duration_ms, chrono_now(), task_id],
        )
        .map_err(|e| format!("Failed to complete task: {e}"))?;
        Ok(())
    }

    /// Mark a task as FAILED with an error message.
    pub fn set_failed(
        &self,
        task_id: &str,
        error: &str,
        claude_session_id: &str,
    ) -> Result<(), String> {
        let conn = self.db.conn()?;
        conn.execute(
            "UPDATE tasks SET state = 'failed', error = ?1, claude_session_id = ?2, completed_at = ?3 WHERE id = ?4",
            rusqlite::params![error, claude_session_id, chrono_now(), task_id],
        )
        .map_err(|e| format!("Failed to fail task: {e}"))?;
        Ok(())
    }

    /// Get a task by ID.
    pub fn get_task(&self, task_id: &str) -> Result<Option<Task>, String> {
        let conn = self.db.conn()?;
        let mut stmt = conn
            .prepare("SELECT id, prompt, state, result, error, session_id, claude_session_id, conversation_id, cost_usd, num_turns, duration_ms, created_at, started_at, completed_at FROM tasks WHERE id = ?1")
            .map_err(|e| format!("Query error: {e}"))?;

        let task = stmt
            .query_row(rusqlite::params![task_id], |row| {
                Ok(Task {
                    id: row.get(0)?,
                    prompt: row.get(1)?,
                    state: TaskState::from_str(&row.get::<_, String>(2)?),
                    result: row.get(3)?,
                    error: row.get(4)?,
                    session_id: row.get(5)?,
                    claude_session_id: row.get(6)?,
                    conversation_id: row.get(7)?,
                    cost_usd: row.get(8)?,
                    num_turns: row.get(9)?,
                    duration_ms: row.get(10)?,
                    created_at: row.get(11)?,
                    started_at: row.get(12)?,
                    completed_at: row.get(13)?,
                })
            })
            .optional()
            .map_err(|e| format!("Query error: {e}"))?;

        Ok(task)
    }

    /// Get the last claude_session_id for a conversation (for --resume).
    pub fn get_last_session_id(&self, conversation_id: &str) -> Result<Option<String>, String> {
        let conn = self.db.conn()?;
        let mut stmt = conn
            .prepare("SELECT claude_session_id FROM tasks WHERE conversation_id = ?1 AND claude_session_id != '' ORDER BY completed_at DESC LIMIT 1")
            .map_err(|e| format!("Query error: {e}"))?;

        let session_id = stmt
            .query_row(rusqlite::params![conversation_id], |row| {
                row.get::<_, String>(0)
            })
            .optional()
            .map_err(|e| format!("Query error: {e}"))?;

        Ok(session_id)
    }

    /// List all conversations with metadata for sidebar display.
    ///
    /// Returns conversations sorted by last activity (most recent first),
    /// with title derived from the first prompt in each conversation.
    pub fn list_conversations(&self) -> Result<Vec<ConversationSummary>, String> {
        let conn = self.db.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT c.id, c.created_at,
                        (SELECT t2.prompt FROM tasks t2 WHERE t2.conversation_id = c.id ORDER BY t2.created_at ASC LIMIT 1) as first_prompt,
                        COUNT(t.id) as message_count,
                        MAX(COALESCE(t.completed_at, t.created_at)) as last_activity,
                        c.tts_muted
                 FROM conversations c
                 LEFT JOIN tasks t ON t.conversation_id = c.id
                 GROUP BY c.id
                 HAVING message_count > 0
                 ORDER BY last_activity DESC, c.created_at DESC
                 LIMIT 50",
            )
            .map_err(|e| format!("Query error: {e}"))?;

        let conversations = stmt
            .query_map([], |row| {
                let first_prompt: String = row.get::<_, String>(2).unwrap_or_default();
                let title = if first_prompt.len() > 100 {
                    format!(
                        "{}...",
                        &first_prompt[..first_prompt.floor_char_boundary(97)]
                    )
                } else {
                    first_prompt
                };
                Ok(ConversationSummary {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    title,
                    message_count: row.get(3)?,
                    last_activity: row.get::<_, i64>(4).unwrap_or(0),
                    tts_muted: row.get::<_, i64>(5).unwrap_or(0) != 0,
                })
            })
            .map_err(|e| format!("Query error: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Row error: {e}"))?;

        Ok(conversations)
    }

    /// Get all tasks for a conversation, ordered by creation time.
    ///
    /// Used to rebuild the message history when switching conversations.
    pub fn get_conversation_tasks(&self, conversation_id: &str) -> Result<Vec<Task>, String> {
        let conn = self.db.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, prompt, state, result, error, session_id, claude_session_id, conversation_id, cost_usd, num_turns, duration_ms, created_at, started_at, completed_at
                 FROM tasks WHERE conversation_id = ?1 ORDER BY created_at ASC",
            )
            .map_err(|e| format!("Query error: {e}"))?;

        let tasks = stmt
            .query_map(rusqlite::params![conversation_id], |row| {
                Ok(Task {
                    id: row.get(0)?,
                    prompt: row.get(1)?,
                    state: TaskState::from_str(&row.get::<_, String>(2)?),
                    result: row.get(3)?,
                    error: row.get(4)?,
                    session_id: row.get(5)?,
                    claude_session_id: row.get(6)?,
                    conversation_id: row.get(7)?,
                    cost_usd: row.get(8)?,
                    num_turns: row.get(9)?,
                    duration_ms: row.get(10)?,
                    created_at: row.get(11)?,
                    started_at: row.get(12)?,
                    completed_at: row.get(13)?,
                })
            })
            .map_err(|e| format!("Query error: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Row error: {e}"))?;

        Ok(tasks)
    }

    /// Search conversations by keyword across prompts and results.
    ///
    /// Returns conversations where any task prompt or result contains the query string.
    /// Case-insensitive (SQLite LIKE is case-insensitive for ASCII).
    pub fn search_conversations(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ConversationSummary>, String> {
        let conn = self.db.conn()?;
        let pattern = format!("%{query}%");
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT c.id, c.created_at,
                        (SELECT t2.prompt FROM tasks t2 WHERE t2.conversation_id = c.id ORDER BY t2.created_at ASC LIMIT 1) as first_prompt,
                        (SELECT COUNT(*) FROM tasks t3 WHERE t3.conversation_id = c.id) as message_count,
                        (SELECT MAX(COALESCE(t4.completed_at, t4.created_at)) FROM tasks t4 WHERE t4.conversation_id = c.id) as last_activity,
                        c.tts_muted
                 FROM conversations c
                 INNER JOIN tasks t ON t.conversation_id = c.id
                 WHERE t.prompt LIKE ?1 OR t.result LIKE ?1
                 ORDER BY last_activity DESC
                 LIMIT ?2",
            )
            .map_err(|e| format!("Query error: {e}"))?;

        let conversations = stmt
            .query_map(rusqlite::params![pattern, limit as i64], |row| {
                let first_prompt: String = row.get::<_, String>(2).unwrap_or_default();
                let title = if first_prompt.len() > 100 {
                    format!(
                        "{}...",
                        &first_prompt[..first_prompt.floor_char_boundary(97)]
                    )
                } else {
                    first_prompt
                };
                Ok(ConversationSummary {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    title,
                    message_count: row.get(3)?,
                    last_activity: row.get::<_, i64>(4).unwrap_or(0),
                    tts_muted: row.get::<_, i64>(5).unwrap_or(0) != 0,
                })
            })
            .map_err(|e| format!("Query error: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Row error: {e}"))?;

        Ok(conversations)
    }

    /// Delete a conversation and all its associated tasks.
    pub fn delete_conversation(&self, conversation_id: &str) -> Result<(), String> {
        let conn = self.db.conn()?;
        conn.execute(
            "DELETE FROM tasks WHERE conversation_id = ?1",
            rusqlite::params![conversation_id],
        )
        .map_err(|e| format!("Failed to delete tasks: {e}"))?;

        conn.execute(
            "DELETE FROM conversations WHERE id = ?1",
            rusqlite::params![conversation_id],
        )
        .map_err(|e| format!("Failed to delete conversation: {e}"))?;

        Ok(())
    }

    /// Set TTS muted state for a conversation.
    pub fn set_tts_muted(&self, conversation_id: &str, muted: bool) -> Result<(), String> {
        let conn = self.db.conn()?;
        conn.execute(
            "UPDATE conversations SET tts_muted = ?1 WHERE id = ?2",
            rusqlite::params![muted as i64, conversation_id],
        )
        .map_err(|e| format!("Failed to set TTS muted: {e}"))?;
        Ok(())
    }

    /// Create a new conversation. Returns the conversation ID.
    pub fn create_conversation(&self) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono_now();

        let conn = self.db.conn()?;
        conn.execute(
            "INSERT INTO conversations (id, created_at) VALUES (?1, ?2)",
            rusqlite::params![id, now],
        )
        .map_err(|e| format!("Failed to create conversation: {e}"))?;

        Ok(id)
    }
}

/// Current Unix timestamp in seconds.
fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Extension trait for optional query results.
trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::db::Database;
    use rusqlite::Connection;

    fn test_db() -> Arc<Database> {
        let conn = Connection::open_in_memory().unwrap();
        let db = Database::from_conn(conn);
        db.migrate().unwrap();
        Arc::new(db)
    }

    #[test]
    fn create_and_get_task() {
        let db = test_db();
        let tm = TaskManager::new(db);

        let id = tm.create_task("hello", "conv1", "").unwrap();
        let task = tm.get_task(&id).unwrap().unwrap();

        assert_eq!(task.prompt, "hello");
        assert_eq!(task.state, TaskState::Pending);
        assert_eq!(task.conversation_id, "conv1");
    }

    #[test]
    fn task_lifecycle() {
        let db = test_db();
        let tm = TaskManager::new(db);

        let id = tm.create_task("test", "conv1", "").unwrap();
        tm.set_running(&id).unwrap();

        let task = tm.get_task(&id).unwrap().unwrap();
        assert_eq!(task.state, TaskState::Running);

        tm.set_completed(&id, "result text", "sess-123", 0.05, 3, 2000)
            .unwrap();

        let task = tm.get_task(&id).unwrap().unwrap();
        assert_eq!(task.state, TaskState::Completed);
        assert_eq!(task.result, "result text");
        assert_eq!(task.claude_session_id, "sess-123");
    }

    #[test]
    fn task_failure() {
        let db = test_db();
        let tm = TaskManager::new(db);

        let id = tm.create_task("fail", "conv1", "").unwrap();
        tm.set_running(&id).unwrap();
        tm.set_failed(&id, "something broke", "sess-456").unwrap();

        let task = tm.get_task(&id).unwrap().unwrap();
        assert_eq!(task.state, TaskState::Failed);
        assert_eq!(task.error, "something broke");
    }

    #[test]
    fn list_conversations_empty() {
        let db = test_db();
        let tm = TaskManager::new(db);
        let convos = tm.list_conversations().unwrap();
        assert!(convos.is_empty());
    }

    #[test]
    fn list_conversations_with_tasks() {
        let db = test_db();
        let tm = TaskManager::new(db);

        // Create two conversations with tasks
        let conv1 = tm.create_conversation().unwrap();
        let id1 = tm.create_task("First question", &conv1, "").unwrap();
        tm.set_completed(&id1, "Answer 1", "sess-1", 0.01, 1, 500)
            .unwrap();

        let conv2 = tm.create_conversation().unwrap();
        let id2 = tm
            .create_task("Second conversation prompt", &conv2, "")
            .unwrap();
        tm.set_completed(&id2, "Answer 2", "sess-2", 0.02, 1, 600)
            .unwrap();

        let convos = tm.list_conversations().unwrap();
        assert_eq!(convos.len(), 2);
        // Both conversations present with correct data
        let titles: Vec<&str> = convos.iter().map(|c| c.title.as_str()).collect();
        assert!(titles.contains(&"First question"));
        assert!(titles.contains(&"Second conversation prompt"));
        for c in &convos {
            assert_eq!(c.message_count, 1);
            assert!(c.last_activity > 0);
        }
    }

    #[test]
    fn get_conversation_tasks_returns_ordered() {
        let db = test_db();
        let tm = TaskManager::new(db);

        let conv = tm.create_conversation().unwrap();
        let id1 = tm.create_task("msg1", &conv, "").unwrap();
        tm.set_completed(&id1, "reply1", "s1", 0.01, 1, 100)
            .unwrap();
        let id2 = tm.create_task("msg2", &conv, "").unwrap();
        tm.set_completed(&id2, "reply2", "s2", 0.01, 1, 100)
            .unwrap();

        let tasks = tm.get_conversation_tasks(&conv).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].prompt, "msg1");
        assert_eq!(tasks[1].prompt, "msg2");
    }

    #[test]
    fn get_conversation_tasks_empty() {
        let db = test_db();
        let tm = TaskManager::new(db);
        let tasks = tm.get_conversation_tasks("nonexistent").unwrap();
        assert!(tasks.is_empty());
    }

    #[test]
    fn delete_conversation_removes_tasks() {
        let db = test_db();
        let tm = TaskManager::new(db);

        let conv = tm.create_conversation().unwrap();
        let id = tm.create_task("hello", &conv, "").unwrap();
        tm.set_completed(&id, "world", "s1", 0.01, 1, 100).unwrap();

        tm.delete_conversation(&conv).unwrap();

        let tasks = tm.get_conversation_tasks(&conv).unwrap();
        assert!(tasks.is_empty());
        // Conversation list should not include it
        let convos = tm.list_conversations().unwrap();
        assert!(convos.is_empty());
    }

    #[test]
    fn delete_conversation_nonexistent() {
        let db = test_db();
        let tm = TaskManager::new(db);
        // Should not error
        assert!(tm.delete_conversation("does-not-exist").is_ok());
    }

    #[test]
    fn search_conversations_finds_match() {
        let db = test_db();
        let tm = TaskManager::new(db);

        let conv = tm.create_conversation().unwrap();
        let id = tm
            .create_task("How do I fix the login bug?", &conv, "")
            .unwrap();
        tm.set_completed(&id, "Here's how to fix it...", "s1", 0.01, 1, 100)
            .unwrap();

        let results = tm.search_conversations("login", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].title.contains("login"));
    }

    #[test]
    fn search_conversations_searches_results_too() {
        let db = test_db();
        let tm = TaskManager::new(db);

        let conv = tm.create_conversation().unwrap();
        let id = tm.create_task("tell me something", &conv, "").unwrap();
        tm.set_completed(
            &id,
            "The answer involves quantum physics",
            "s1",
            0.01,
            1,
            100,
        )
        .unwrap();

        let results = tm.search_conversations("quantum", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_conversations_no_match() {
        let db = test_db();
        let tm = TaskManager::new(db);

        let conv = tm.create_conversation().unwrap();
        let id = tm.create_task("hello", &conv, "").unwrap();
        tm.set_completed(&id, "world", "s1", 0.01, 1, 100).unwrap();

        let results = tm.search_conversations("nonexistent-xyz", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn get_last_session_id() {
        let db = test_db();
        let tm = TaskManager::new(db);

        let id = tm.create_task("msg", "conv1", "").unwrap();
        tm.set_completed(&id, "ok", "sess-789", 0.01, 1, 1000)
            .unwrap();

        let session = tm.get_last_session_id("conv1").unwrap();
        assert_eq!(session, Some("sess-789".to_string()));
    }
}
