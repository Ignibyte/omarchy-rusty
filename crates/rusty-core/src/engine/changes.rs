//! The change log: one row per write to the store, so a client in another process can
//! ask what changed after a cursor and re-read only that (TICKET-035).
//!
//! Every process that writes through `rusty-core` shares `rusty.db`, so a row recorded by
//! one is visible to all at commit. Rows carry names, never values: the kind, the id,
//! slug or key, the operation, an optional detail (a page's old slug on a move) and, for
//! pages, the content hash. A page edit seen by several processes' watchers is recorded
//! once: the first to index it changes the shared hash, and an insert whose key's last
//! row already has the same operation and hash is skipped.

use rusqlite::Connection;

use crate::engine::db::Database;

/// How many rows the log keeps; older ones are pruned.
pub const RETAIN: i64 = 20_000;

/// Pruning runs when a new row's sequence is a multiple of this.
const PRUNE_EVERY: i64 = 256;

/// The most rows one read returns.
pub const MAX_LIMIT: usize = 5_000;

/// One recorded change.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Change {
    /// Its place in the log; only grows.
    pub seq: i64,
    /// `page`, `task`, `task_group`, `memory`, `note`, `setting`, `secret`, `skill`,
    /// `script` or `bookmarks`.
    pub kind: String,
    /// The slug, id, path or key of what changed.
    pub key: String,
    /// `created`, `updated`, `deleted`, `moved`, or a kind's own word (`approved`).
    pub op: String,
    /// More about the change when there is more: a moved page's old slug, a task's group.
    pub detail: String,
    /// Unix seconds.
    pub at: i64,
}

/// What a read after a cursor returns.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChangeBatch {
    /// The cursor to pass next time: the last row returned, or the log's end.
    pub cursor: i64,
    /// The rows after the cursor, oldest first.
    pub changes: Vec<Change>,
    /// The cursor is older than the log keeps (or from another store): re-read everything.
    pub reset: bool,
    /// More rows wait after this batch.
    pub more: bool,
}

/// Record a change on a connection the caller holds. A failure is printed, never
/// returned: the write it describes has already happened.
pub fn record(conn: &Connection, kind: &str, key: &str, op: &str, detail: &str, hash: &str) {
    if let Err(e) = try_record(conn, kind, key, op, detail, hash) {
        eprintln!("rusty: change log: {e}");
    }
}

fn try_record(
    conn: &Connection,
    kind: &str,
    key: &str,
    op: &str,
    detail: &str,
    hash: &str,
) -> Result<(), String> {
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    // One statement, so two processes recording the same page edit cannot both pass the
    // check: an unhashed row is always written, a hashed one only when the key's last row
    // is not the same operation on the same content.
    let written = conn
        .execute(
            "INSERT INTO changes (kind, key, op, detail, hash, at) \
             SELECT ?1, ?2, ?3, ?4, ?5, ?6 \
             WHERE ?5 = '' OR NOT EXISTS ( \
                 SELECT 1 FROM changes WHERE seq = ( \
                     SELECT MAX(seq) FROM changes WHERE kind = ?1 AND key = ?2) \
                 AND op = ?3 AND hash = ?5)",
            rusqlite::params![kind, key, op, detail, hash, at],
        )
        .map_err(|e| format!("insert: {e}"))?;
    if written > 0 {
        let seq = conn.last_insert_rowid();
        if seq % PRUNE_EVERY == 0 {
            conn.execute(
                "DELETE FROM changes WHERE seq <= ?1",
                rusqlite::params![seq - RETAIN],
            )
            .map_err(|e| format!("prune: {e}"))?;
        }
    }
    Ok(())
}

/// The changes after `cursor`, at most `limit` of them (default 500, at most
/// [`MAX_LIMIT`]). With no cursor, the log's end and no rows, so a client can start
/// from now.
pub fn since(
    db: &Database,
    cursor: Option<i64>,
    limit: Option<usize>,
) -> Result<ChangeBatch, String> {
    let limit = limit.unwrap_or(500).clamp(1, MAX_LIMIT);
    let conn = db.conn()?;
    let (first, last): (Option<i64>, Option<i64>) = conn
        .query_row("SELECT MIN(seq), MAX(seq) FROM changes", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|e| format!("Query error: {e}"))?;
    let end = last.unwrap_or(0);
    let Some(cursor) = cursor else {
        return Ok(ChangeBatch {
            cursor: end,
            changes: Vec::new(),
            reset: false,
            more: false,
        });
    };
    // A cursor past the end is from another store or a rebuilt one; a cursor before the
    // oldest kept row has missed rows that were pruned.
    let behind = first.is_some_and(|first| cursor < first - 1);
    if cursor > end || behind {
        return Ok(ChangeBatch {
            cursor: end,
            changes: Vec::new(),
            reset: true,
            more: false,
        });
    }
    let mut stmt = conn
        .prepare(
            "SELECT seq, kind, key, op, detail, at FROM changes \
             WHERE seq > ?1 ORDER BY seq LIMIT ?2",
        )
        .map_err(|e| format!("Query error: {e}"))?;
    let mut changes: Vec<Change> = stmt
        .query_map(rusqlite::params![cursor, limit as i64 + 1], |row| {
            Ok(Change {
                seq: row.get(0)?,
                kind: row.get(1)?,
                key: row.get(2)?,
                op: row.get(3)?,
                detail: row.get(4)?,
                at: row.get(5)?,
            })
        })
        .map_err(|e| format!("Query error: {e}"))?
        .collect::<Result<_, _>>()
        .map_err(|e| format!("Row error: {e}"))?;
    let more = changes.len() > limit;
    changes.truncate(limit);
    let cursor = changes.last().map(|c| c.seq).unwrap_or(cursor);
    Ok(ChangeBatch {
        cursor,
        changes,
        reset: false,
        more,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_db() -> Database {
        let db = Database::from_conn(Connection::open_in_memory().unwrap());
        db.migrate().unwrap();
        db
    }

    #[test]
    fn since_reads_after_a_cursor() {
        let db = memory_db();
        let start = since(&db, None, None).unwrap();
        assert_eq!(
            (start.cursor, start.changes.len(), start.reset),
            (0, 0, false)
        );
        db.record_change("task", "1", "created", "group 1");
        db.record_change("task", "1", "updated", "");
        db.record_change("memory", "m", "deleted", "");
        let all = since(&db, Some(start.cursor), None).unwrap();
        let ops: Vec<&str> = all.changes.iter().map(|c| c.op.as_str()).collect();
        assert_eq!(ops, ["created", "updated", "deleted"]);
        assert_eq!(all.cursor, all.changes[2].seq);
        assert!(!all.more);
        let page = since(&db, Some(start.cursor), Some(2)).unwrap();
        assert_eq!(page.changes.len(), 2);
        assert!(page.more);
        let rest = since(&db, Some(page.cursor), Some(2)).unwrap();
        assert_eq!(rest.changes.len(), 1);
        assert_eq!(rest.changes[0].kind, "memory");
        let none = since(&db, Some(rest.cursor), None).unwrap();
        assert!(none.changes.is_empty() && !none.reset);
        assert_eq!(none.cursor, rest.cursor);
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rusty_changes_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn every_kind_of_write_is_recorded() {
        use crate::brain::BrainManager;
        use crate::engine::memory_manager::MemoryManager;
        use crate::engine::secrets_manager::SecretsManager;
        use crate::engine::settings_manager::SettingsManager;
        use crate::engine::user_tasks::UserTaskManager;
        use crate::notes::NotesManager;
        use crate::skills::SkillsManager;
        use std::sync::Arc;

        let dir = scratch("kinds");
        let db = Arc::new(memory_db());
        let brain = BrainManager::new(Arc::clone(&db), dir.join("brain"));
        brain.ensure_vault().unwrap();
        let tasks = UserTaskManager::new(Arc::clone(&db));
        let memories = MemoryManager::new(Arc::clone(&db));
        let settings = SettingsManager::new(Arc::clone(&db));
        let secrets = SecretsManager::new(dir.join(".secret")).with_changes(Arc::clone(&db));
        let skills = SkillsManager::new(dir.join("skills")).with_changes(Arc::clone(&db));
        skills.ensure_dirs().unwrap();
        let notes = NotesManager::with_root(dir.join("notes"))
            .unwrap()
            .with_changes(Arc::clone(&db));
        let start = since(&db, None, None).unwrap().cursor;

        brain.write_raw("ideas/x", "Secret body text.\n").unwrap();
        brain.update_page("ideas/x", "Changed body.").unwrap();
        brain
            .add_timeline("ideas/x", "2026-10-04", "test", "Noted", None)
            .unwrap();
        brain.rename("ideas/x", "ideas/y").unwrap();
        brain.delete_page("ideas/y").unwrap();
        let g = tasks.create_header("G").unwrap();
        let t = tasks.create_task(g, "T").unwrap();
        tasks.toggle_complete(t).unwrap();
        tasks.delete_task(t).unwrap();
        tasks.delete_header(g).unwrap();
        let m = memories
            .store("fact", "normal", "a memory", "test")
            .unwrap();
        memories.delete(&m).unwrap();
        settings.set("api_token", "sk-not-in-the-log").unwrap();
        secrets.set("API_KEY", "hunter2-value").unwrap();
        secrets.delete("API_KEY").unwrap();
        skills
            .create_skill("demo", "A demo.", "Body.", false)
            .unwrap();
        skills.delete_skill("demo").unwrap();
        let note = notes.create_note("", "today", false).unwrap();
        notes.save_note(&note, "Note body.").unwrap();
        notes.delete_note(&note).unwrap();

        let rows = since(&db, Some(start), Some(MAX_LIMIT)).unwrap().changes;
        let seen: Vec<(String, String, String)> = rows
            .iter()
            .map(|c| (c.kind.clone(), c.key.clone(), c.op.clone()))
            .collect();
        let has = |kind: &str, key: &str, op: &str| {
            seen.iter()
                .any(|(k, key2, o)| k == kind && key2 == key && o == op)
        };
        assert!(has("page", "ideas/x", "created"), "{seen:?}");
        assert!(has("page", "ideas/x", "updated"), "{seen:?}");
        assert!(has("page", "ideas/y", "moved"), "{seen:?}");
        assert!(has("page", "ideas/y", "deleted"), "{seen:?}");
        assert!(has("task_group", &g.to_string(), "created"));
        assert!(has("task", &t.to_string(), "created"));
        assert!(has("task", &t.to_string(), "updated"));
        assert!(has("task", &t.to_string(), "deleted"));
        assert!(has("task_group", &g.to_string(), "deleted"));
        assert!(has("memory", &m, "created") && has("memory", &m, "deleted"));
        assert!(has("setting", "api_token", "updated"));
        assert!(has("secret", "API_KEY", "updated") && has("secret", "API_KEY", "deleted"));
        assert!(has("skill", "demo", "created") && has("skill", "demo", "deleted"));
        assert!(has("note", &note, "created") && has("note", &note, "deleted"));
        let moved = rows.iter().find(|c| c.op == "moved").unwrap();
        assert_eq!(moved.detail, "ideas/x");
        // Names, never values or bodies (REQ-006).
        let all: String = db
            .conn()
            .unwrap()
            .query_row(
                "SELECT group_concat(kind || key || op || detail || hash, '|') FROM changes",
                [],
                |r| r.get(0),
            )
            .unwrap();
        for value in [
            "hunter2",
            "sk-not-in-the-log",
            "Secret body",
            "Note body",
            "a memory",
        ] {
            assert!(!all.contains(value), "{value} leaked into the log");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn two_processes_see_one_log() {
        use crate::brain::BrainManager;
        use crate::engine::user_tasks::UserTaskManager;
        use std::sync::Arc;

        let dir = scratch("two");
        let file = dir.join("rusty.db");
        // Two connections to one file, as the service and an agent's server have.
        let a = Arc::new(Database::open_path(&file).unwrap());
        let b = Arc::new(Database::open_path(&file).unwrap());
        let cursor = since(&b, None, None).unwrap().cursor;
        let g = UserTaskManager::new(Arc::clone(&a))
            .create_header("From A")
            .unwrap();
        let seen = since(&b, Some(cursor), None).unwrap();
        assert_eq!(seen.changes.len(), 1);
        assert_eq!(
            (seen.changes[0].kind.as_str(), seen.changes[0].key.clone()),
            ("task_group", g.to_string())
        );

        // One vault, both processes' indexers; an edit made on disk is recorded once.
        let vault = dir.join("brain");
        let brain_a = BrainManager::new(Arc::clone(&a), vault.clone());
        brain_a.ensure_vault().unwrap();
        let brain_b = BrainManager::new(Arc::clone(&b), vault.clone());
        std::fs::write(vault.join("ideas/edited.md"), "First.\n").unwrap();
        brain_a.sync_all().unwrap();
        brain_b.sync_all().unwrap();
        let mark = since(&b, None, None).unwrap().cursor;
        std::fs::write(vault.join("ideas/edited.md"), "Second, from an editor.\n").unwrap();
        brain_b.sync_all().unwrap();
        brain_a.sync_all().unwrap();
        let after = since(&a, Some(mark), None).unwrap().changes;
        let edits: Vec<&Change> = after.iter().filter(|c| c.key == "ideas/edited").collect();
        assert_eq!(edits.len(), 1, "{after:?}");
        assert_eq!(edits[0].op, "updated");
        drop((brain_a, brain_b, a, b));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_hashed_change_is_recorded_once_per_content() {
        let db = memory_db();
        let conn = db.conn().unwrap();
        record(&conn, "page", "ideas/x", "updated", "", "h1");
        record(&conn, "page", "ideas/x", "updated", "", "h1");
        record(&conn, "page", "ideas/x", "updated", "", "h2");
        record(&conn, "page", "ideas/x", "updated", "", "h1");
        drop(conn);
        let rows = since(&db, Some(0), None).unwrap().changes;
        assert_eq!(rows.len(), 3, "h1, h2, then h1 again: {rows:?}");
    }

    #[test]
    fn a_pruned_or_foreign_cursor_resets() {
        let db = memory_db();
        {
            let conn = db.conn().unwrap();
            for i in 0..(RETAIN + PRUNE_EVERY) {
                record(&conn, "task", &i.to_string(), "updated", "", "");
            }
        }
        let late = since(&db, Some(1), None).unwrap();
        assert!(late.reset, "rows after 1 were pruned");
        assert!(late.changes.is_empty());
        let end = since(&db, None, None).unwrap().cursor;
        assert!(
            since(&db, Some(end + 10), None).unwrap().reset,
            "a cursor past the end"
        );
        let kept: i64 = db
            .conn()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM changes", [], |r| r.get(0))
            .unwrap();
        assert!(kept <= RETAIN + PRUNE_EVERY, "{kept}");
    }
}
