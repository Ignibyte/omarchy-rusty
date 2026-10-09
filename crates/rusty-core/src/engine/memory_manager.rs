//! Persistent memory system for cross-conversation context.
//!
//! Stores categorized memories in SQLite and builds `<user_memory>` blocks
//! for injection into Claude's system prompt via `--append-system-prompt`.

use crate::engine::changes;
use crate::engine::db::Database;
use std::sync::Arc;

/// Maximum characters to include in the system prompt memory block.
const MAX_CONTEXT_CHARS: usize = 8000;

/// The importance words, lowest first (TICKET-052).
pub const IMPORTANCE: [&str; 3] = ["low", "normal", "high"];

/// The importance a memory gets when none is given.
pub const DEFAULT_IMPORTANCE: &str = "normal";

/// The list order: importance as a level (a stored word outside [`IMPORTANCE`] sorts
/// with `normal`), then newest first.
const LIST_ORDER: &str = "ORDER BY CASE importance WHEN 'high' THEN 2 WHEN 'low' THEN 0 ELSE 1 END DESC, created_at DESC";

/// The stored word for `word`: one of [`IMPORTANCE`], case and surrounding spaces
/// ignored. `medium`, the word `store_memory` documented before TICKET-052, is `normal`.
pub fn importance_word(word: &str) -> Result<&'static str, String> {
    let word = word.trim().to_ascii_lowercase();
    if word == "medium" {
        return Ok(DEFAULT_IMPORTANCE);
    }
    IMPORTANCE
        .iter()
        .copied()
        .find(|known| *known == word)
        .ok_or_else(|| format!("importance is low, normal or high, not {word:?}"))
}

/// A memory record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Memory {
    pub id: String,
    pub category: String,
    pub importance: String,
    pub content: String,
    #[serde(rename = "type")]
    pub memory_type: String,
    pub source: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Manages persistent memory storage and retrieval.
pub struct MemoryManager {
    db: Arc<Database>,
}

impl MemoryManager {
    /// Create a new MemoryManager backed by the given database.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Store a new memory. Returns the memory ID.
    pub fn store(
        &self,
        category: &str,
        importance: &str,
        content: &str,
        source: &str,
    ) -> Result<String, String> {
        let importance = importance_word(importance)?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono_now();

        let conn = self.db.conn()?;
        conn.execute(
            "INSERT INTO memories (id, category, importance, content, source, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, category, importance, content, source, now, now],
        )
        .map_err(|e| format!("Failed to store memory: {e}"))?;
        changes::record(&conn, "memory", &id, "created", "", "");

        Ok(id)
    }

    /// Change a memory's content, category or importance; `None` keeps the current
    /// value. Returns the memory as stored afterwards.
    pub fn update(
        &self,
        id: &str,
        content: Option<&str>,
        category: Option<&str>,
        importance: Option<&str>,
    ) -> Result<Memory, String> {
        let importance = importance.map(importance_word).transpose()?;
        let now = chrono_now();
        let changed = {
            let conn = self.db.conn()?;
            let changed = conn
                .execute(
                    "UPDATE memories SET content = COALESCE(?1, content), category = COALESCE(?2, category), \
                     importance = COALESCE(?3, importance), updated_at = ?4 WHERE id = ?5",
                    rusqlite::params![content, category, importance, now, id],
                )
                .map_err(|e| format!("Failed to update memory: {e}"))?;
            if changed > 0 {
                changes::record(&conn, "memory", id, "updated", "", "");
            }
            changed
        };
        if changed == 0 {
            return Err(format!("Memory not found: {id}"));
        }
        self.list(None)?
            .into_iter()
            .find(|m| m.id == id)
            .ok_or_else(|| format!("Memory not found: {id}"))
    }

    /// List all memories, optionally filtered by category.
    pub fn list(&self, category: Option<&str>) -> Result<Vec<Memory>, String> {
        let conn = self.db.conn()?;

        let mut memories = Vec::new();

        if let Some(cat) = category {
            let mut stmt = conn
                .prepare(&format!("SELECT id, category, importance, content, type, source, created_at, updated_at FROM memories WHERE category = ?1 {LIST_ORDER}"))
                .map_err(|e| format!("Query error: {e}"))?;

            let rows = stmt
                .query_map(rusqlite::params![cat], row_to_memory)
                .map_err(|e| format!("Query error: {e}"))?;

            for m in rows.flatten() {
                memories.push(m);
            }
        } else {
            let mut stmt = conn
                .prepare(&format!("SELECT id, category, importance, content, type, source, created_at, updated_at FROM memories {LIST_ORDER}"))
                .map_err(|e| format!("Query error: {e}"))?;

            let rows = stmt
                .query_map([], row_to_memory)
                .map_err(|e| format!("Query error: {e}"))?;

            for m in rows.flatten() {
                memories.push(m);
            }
        }

        Ok(memories)
    }

    /// Delete a memory by ID.
    pub fn delete(&self, id: &str) -> Result<(), String> {
        let conn = self.db.conn()?;
        let changed = conn
            .execute("DELETE FROM memories WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| format!("Failed to delete memory: {e}"))?;
        if changed == 0 {
            return Err(format!("Memory not found: {id}"));
        }
        changes::record(&conn, "memory", id, "deleted", "", "");
        Ok(())
    }

    /// Build a `<user_memory>` block for system prompt injection.
    ///
    /// Groups memories by category. Core memories (importance=high) always included.
    /// Keyword-ranked relevance deferred to a future phase.
    pub fn build_system_prompt_context(&self) -> Result<String, String> {
        let memories = self.list(None)?;

        if memories.is_empty() {
            return Ok(String::new());
        }

        let mut parts: Vec<String> = Vec::new();
        parts.push("<user_memory>".to_string());

        // Group by category
        let categories = ["preference", "fact", "project", "context"];
        let mut total_chars = 0;

        // Core memories first (importance = high)
        let core: Vec<&Memory> = memories.iter().filter(|m| m.importance == "high").collect();
        if !core.is_empty() {
            parts.push("\n## Core".to_string());
            for m in &core {
                let line = format!("- {}", m.content);
                total_chars += line.len();
                if total_chars > MAX_CONTEXT_CHARS {
                    break;
                }
                parts.push(line);
            }
        }

        // Then by category (normal + low importance)
        for category in &categories {
            let cat_memories: Vec<&Memory> = memories
                .iter()
                .filter(|m| m.category == *category && m.importance != "high")
                .collect();

            if cat_memories.is_empty() {
                continue;
            }

            let label = match *category {
                "preference" => "Preferences",
                "fact" => "Facts",
                "project" => "Projects",
                "context" => "Context",
                other => other,
            };

            parts.push(format!("\n## {label}"));
            for m in &cat_memories {
                let line = format!("- {}", m.content);
                total_chars += line.len();
                if total_chars > MAX_CONTEXT_CHARS {
                    break;
                }
                parts.push(line);
            }

            if total_chars > MAX_CONTEXT_CHARS {
                break;
            }
        }

        parts.push("\n</user_memory>".to_string());

        Ok(parts.join("\n"))
    }
}

/// Extract `<memory>` tags from Claude's response text.
///
/// Returns the extracted memories and the cleaned text with tags removed.
///
/// Format: `<memory category="preference" importance="high">content</memory>`
pub fn extract_memory_tags(text: &str) -> (Vec<ExtractedMemory>, String) {
    let mut memories = Vec::new();
    let mut cleaned = text.to_string();

    // Simple regex-free parser for <memory> tags
    while let Some(start) = cleaned.find("<memory") {
        let Some(tag_end) = cleaned[start..].find('>') else {
            break;
        };
        let tag_end = start + tag_end;

        let Some(close) = cleaned[tag_end..].find("</memory>") else {
            break;
        };
        let close = tag_end + close;

        // Extract attributes from opening tag
        let tag_str = &cleaned[start..=tag_end];
        let category = extract_attr(tag_str, "category").unwrap_or_else(|| "context".to_string());
        // A model's tag has no caller to correct it: an unknown word becomes the default.
        let importance = extract_attr(tag_str, "importance")
            .and_then(|word| importance_word(&word).ok())
            .unwrap_or(DEFAULT_IMPORTANCE)
            .to_string();

        // Extract content between tags
        let content = cleaned[tag_end + 1..close].trim().to_string();

        if !content.is_empty() {
            memories.push(ExtractedMemory {
                category,
                importance,
                content,
            });
        }

        // Remove the entire tag from cleaned text
        cleaned = format!("{}{}", &cleaned[..start], &cleaned[close + 9..]);
    }

    (memories, cleaned.trim().to_string())
}

/// A memory extracted from inline `<memory>` tags.
#[derive(Debug, Clone)]
pub struct ExtractedMemory {
    pub category: String,
    pub importance: String,
    pub content: String,
}

/// Extract an attribute value from an XML-like tag string.
fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    let start = tag.find(&pattern)?;
    let value_start = start + pattern.len();
    let value_end = tag[value_start..].find('"')?;
    Some(tag[value_start..value_start + value_end].to_string())
}

/// Row mapper for SQLite query results.
fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: row.get(0)?,
        category: row.get(1)?,
        importance: row.get(2)?,
        content: row.get(3)?,
        memory_type: row.get(4)?,
        source: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
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
    fn store_and_list_memories() {
        let db = test_db();
        let mm = MemoryManager::new(db);

        mm.store("preference", "high", "Prefers dark mode", "manual")
            .unwrap();
        mm.store("fact", "normal", "Name is Robin", "inline")
            .unwrap();

        let all = mm.list(None).unwrap();
        assert_eq!(all.len(), 2);

        let prefs = mm.list(Some("preference")).unwrap();
        assert_eq!(prefs.len(), 1);
        assert_eq!(prefs[0].content, "Prefers dark mode");
    }

    #[test]
    fn importance_words_are_one_set() {
        for (word, stored) in [
            ("low", "low"),
            ("normal", "normal"),
            ("high", "high"),
            (" High ", "high"),
            ("medium", "normal"),
            ("MEDIUM", "normal"),
        ] {
            assert_eq!(importance_word(word).unwrap(), stored, "{word}");
        }
        for word in ["", "urgent", "critical", "mid"] {
            let err = importance_word(word).unwrap_err();
            assert!(err.contains("low, normal or high"), "{err}");
        }
    }

    #[test]
    fn store_and_update_refuse_other_words() {
        let mm = MemoryManager::new(test_db());
        assert!(mm.store("fact", "urgent", "Refused", "manual").is_err());
        assert!(mm.list(None).unwrap().is_empty(), "nothing written");
        let id = mm.store("fact", "medium", "Kept as normal", "mcp").unwrap();
        assert_eq!(mm.list(None).unwrap()[0].importance, "normal");
        assert!(mm.update(&id, None, None, Some("urgent")).is_err());
        assert_eq!(mm.list(None).unwrap()[0].importance, "normal");
        let updated = mm.update(&id, None, None, Some("HIGH")).unwrap();
        assert_eq!(updated.importance, "high");
    }

    #[test]
    fn list_orders_by_level_then_newest() {
        let db = test_db();
        let mm = MemoryManager::new(Arc::clone(&db));
        // Each with its own time, so "newest first" is decided by the data.
        for (n, (importance, content)) in [
            ("low", "old low"),
            ("high", "old high"),
            ("normal", "old normal"),
            ("low", "new low"),
            ("normal", "new normal"),
            ("high", "new high"),
        ]
        .into_iter()
        .enumerate()
        {
            let id = mm.store("fact", importance, content, "manual").unwrap();
            db.conn()
                .unwrap()
                .execute(
                    "UPDATE memories SET created_at = ?1 WHERE id = ?2",
                    rusqlite::params![1_000 + n as i64, id],
                )
                .unwrap();
        }
        let order: Vec<String> = mm
            .list(None)
            .unwrap()
            .into_iter()
            .map(|m| m.content)
            .collect();
        assert_eq!(
            order,
            [
                "new high",
                "old high",
                "new normal",
                "old normal",
                "new low",
                "old low"
            ]
        );
        let facts: Vec<String> = mm
            .list(Some("fact"))
            .unwrap()
            .into_iter()
            .map(|m| m.content)
            .collect();
        assert_eq!(facts, order, "the category filter keeps the same order");
    }

    #[test]
    fn an_odd_word_in_a_tag_becomes_normal() {
        let (memories, _) =
            extract_memory_tags(r#"<memory category="fact" importance="critical">X</memory>"#);
        assert_eq!(memories[0].importance, "normal");
        let (memories, _) =
            extract_memory_tags(r#"<memory category="fact" importance="Medium">Y</memory>"#);
        assert_eq!(memories[0].importance, "normal");
    }

    #[test]
    fn delete_memory() {
        let db = test_db();
        let mm = MemoryManager::new(db);

        let id = mm.store("fact", "normal", "test memory", "manual").unwrap();
        assert_eq!(mm.list(None).unwrap().len(), 1);

        mm.delete(&id).unwrap();
        assert_eq!(mm.list(None).unwrap().len(), 0);
    }

    #[test]
    fn delete_of_a_missing_memory_fails() {
        let mm = MemoryManager::new(test_db());
        let id = mm.store("fact", "normal", "once", "manual").unwrap();
        mm.delete(&id).unwrap();
        assert_eq!(
            mm.delete(&id).unwrap_err(),
            format!("Memory not found: {id}")
        );
    }

    #[test]
    fn build_system_prompt_context_empty() {
        let db = test_db();
        let mm = MemoryManager::new(db);

        let ctx = mm.build_system_prompt_context().unwrap();
        assert!(ctx.is_empty());
    }

    #[test]
    fn build_system_prompt_context_with_memories() {
        let db = test_db();
        let mm = MemoryManager::new(db);

        mm.store("preference", "high", "Dark mode", "manual")
            .unwrap();
        mm.store("fact", "normal", "Name is Robin", "manual")
            .unwrap();

        let ctx = mm.build_system_prompt_context().unwrap();
        assert!(ctx.contains("<user_memory>"));
        assert!(ctx.contains("</user_memory>"));
        assert!(ctx.contains("Dark mode"));
        assert!(ctx.contains("Name is Robin"));
        assert!(ctx.contains("## Core"));
    }

    #[test]
    fn extract_memory_tags_basic() {
        let text = r#"Hello! <memory category="preference" importance="high">User likes Rust</memory> Sure thing."#;
        let (memories, cleaned) = extract_memory_tags(text);

        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].category, "preference");
        assert_eq!(memories[0].importance, "high");
        assert_eq!(memories[0].content, "User likes Rust");
        assert_eq!(cleaned, "Hello!  Sure thing.");
    }

    #[test]
    fn extract_memory_tags_multiple() {
        let text = r#"A <memory category="fact" importance="normal">Name is Robin</memory> B <memory category="context" importance="low">Working on Rusty</memory> C"#;
        let (memories, cleaned) = extract_memory_tags(text);

        assert_eq!(memories.len(), 2);
        assert_eq!(memories[0].content, "Name is Robin");
        assert_eq!(memories[1].content, "Working on Rusty");
        assert_eq!(cleaned, "A  B  C");
    }

    #[test]
    fn extract_memory_tags_none() {
        let text = "No memory tags here.";
        let (memories, cleaned) = extract_memory_tags(text);

        assert_eq!(memories.len(), 0);
        assert_eq!(cleaned, "No memory tags here.");
    }

    #[test]
    fn extract_attr_works() {
        assert_eq!(
            extract_attr(r#"<memory category="fact">"#, "category"),
            Some("fact".to_string())
        );
        assert_eq!(
            extract_attr(r#"<memory importance="high">"#, "importance"),
            Some("high".to_string())
        );
        assert_eq!(extract_attr("<memory>", "category"), None);
    }

    #[test]
    fn update_changes_only_what_is_given() {
        let mm = MemoryManager::new(test_db());
        let id = mm
            .store("preference", "normal", "Prefers dark mode", "manual")
            .unwrap();
        let updated = mm
            .update(&id, Some("Prefers light mode"), None, Some("high"))
            .unwrap();
        assert_eq!(updated.content, "Prefers light mode");
        assert_eq!(updated.category, "preference");
        assert_eq!(updated.importance, "high");
        assert!(mm.update("nope", Some("x"), None, None).is_err());
    }
}
