//! SQLite database initialization and connection management.

use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

/// Thread-safe SQLite database wrapper.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Open (or create) the SQLite database at `~/.rusty/rusty.db`.
    /// Runs migrations to ensure schema is up to date.
    pub fn open() -> Result<Self, String> {
        Self::register_extensions();
        let db_path = Self::db_path();

        // Ensure directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create database directory: {e}"))?;
        }

        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open database at {}: {e}", db_path.display()))?;

        // WAL for concurrent reads, and a busy timeout so a write from a second
        // process (the service, an agent's stdio server, the CLI) waits instead of
        // failing with SQLITE_BUSY.
        Self::prepare(&conn)?;

        let db = Self {
            conn: Mutex::new(conn),
        };

        db.migrate()?;
        // Rows written before enforcement that point at rows now gone are reported, not
        // repaired; which to keep is a person's call.
        match db.foreign_key_violations() {
            Ok(0) => {}
            Ok(n) => eprintln!(
                "rusty: {n} rows in {} refer to rows that no longer exist (PRAGMA foreign_key_check); left in place",
                db_path.display()
            ),
            Err(e) => eprintln!("rusty: foreign key check: {e}"),
        }

        Ok(db)
    }

    /// Register `sqlite-vec` for every connection opened after this call, so the
    /// `vec0` virtual table exists for the semantic index. Idempotent; `open()` calls it,
    /// tests that open their own in-memory connection call it first.
    pub fn register_extensions() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            // SAFETY: `sqlite3_vec_init` is sqlite-vec's entry point. Its real C signature
            // is the one `sqlite3_auto_extension` expects; the crate declares it without
            // arguments, so the pointer is recast here exactly as the crate's own test
            // does. It runs once, before any connection is opened.
            unsafe {
                let init: unsafe extern "C" fn() = sqlite_vec::sqlite3_vec_init;
                let entry: unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3,
                    *mut *mut std::os::raw::c_char,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> std::os::raw::c_int = std::mem::transmute(init);
                rusqlite::ffi::sqlite3_auto_extension(Some(entry));
            }
        });
    }

    /// Run schema migrations.
    pub fn migrate(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                prompt TEXT NOT NULL,
                state TEXT NOT NULL DEFAULT 'pending',
                result TEXT DEFAULT '',
                error TEXT DEFAULT '',
                session_id TEXT DEFAULT '',
                claude_session_id TEXT DEFAULT '',
                conversation_id TEXT DEFAULT '',
                cost_usd REAL DEFAULT 0,
                num_turns INTEGER DEFAULT 0,
                duration_ms INTEGER DEFAULT 0,
                created_at INTEGER NOT NULL,
                started_at INTEGER DEFAULT 0,
                completed_at INTEGER DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                last_session_id TEXT DEFAULT '',
                tts_muted INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_tasks_conversation
                ON tasks(conversation_id);
            CREATE INDEX IF NOT EXISTS idx_tasks_state
                ON tasks(state);

            CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                category TEXT NOT NULL DEFAULT 'context',
                importance TEXT NOT NULL DEFAULT 'normal',
                content TEXT NOT NULL,
                type TEXT NOT NULL DEFAULT 'general',
                source TEXT DEFAULT 'manual',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_memories_category
                ON memories(category);
            CREATE INDEX IF NOT EXISTS idx_memories_importance
                ON memories(importance);

            CREATE TABLE IF NOT EXISTS task_headers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS user_tasks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                header_id INTEGER NOT NULL,
                title TEXT NOT NULL,
                completed INTEGER NOT NULL DEFAULT 0,
                archived INTEGER NOT NULL DEFAULT 0,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (header_id) REFERENCES task_headers(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_user_tasks_header
                ON user_tasks(header_id);

            CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL DEFAULT '',
                directory TEXT NOT NULL,
                prompt TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                result TEXT DEFAULT '',
                error TEXT DEFAULT '',
                cost_usd REAL DEFAULT 0,
                num_turns INTEGER DEFAULT 0,
                duration_ms INTEGER DEFAULT 0,
                session_id TEXT DEFAULT '',
                created_at INTEGER NOT NULL,
                started_at INTEGER DEFAULT 0,
                completed_at INTEGER DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_agents_status
                ON agents(status);
            CREATE INDEX IF NOT EXISTS idx_agents_created
                ON agents(created_at);

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            -- Semantic index: page chunks; their vectors live in the vec0 table brain_vec,
            -- created on first use because its width depends on the embedding model.
            CREATE TABLE IF NOT EXISTS brain_chunks (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                slug          TEXT NOT NULL,
                chunk_index   INTEGER NOT NULL,
                text          TEXT NOT NULL,
                content_hash  TEXT NOT NULL,
                model         TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_brain_chunks_slug ON brain_chunks(slug);
            CREATE TABLE IF NOT EXISTS brain_vec_meta (
                id     INTEGER PRIMARY KEY,
                model  TEXT NOT NULL,
                dims   INTEGER NOT NULL
            );

            -- Brain engine tables
            CREATE TABLE IF NOT EXISTS brain_pages (
                slug          TEXT PRIMARY KEY,
                page_type     TEXT NOT NULL,
                title         TEXT NOT NULL,
                frontmatter   TEXT,
                content_hash  TEXT,
                created_at    INTEGER NOT NULL,
                updated_at    INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_brain_pages_type
                ON brain_pages(page_type);

            CREATE VIRTUAL TABLE IF NOT EXISTS brain_fts USING fts5(
                slug UNINDEXED,
                title,
                content,
                page_type UNINDEXED,
                tokenize='porter unicode61'
            );

            CREATE TABLE IF NOT EXISTS brain_links (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                from_slug     TEXT NOT NULL,
                to_slug       TEXT NOT NULL,
                link_type     TEXT DEFAULT 'reference',
                context       TEXT,
                created_at    INTEGER NOT NULL,
                UNIQUE(from_slug, to_slug, link_type)
            );
            CREATE INDEX IF NOT EXISTS idx_brain_links_from
                ON brain_links(from_slug);
            CREATE INDEX IF NOT EXISTS idx_brain_links_to
                ON brain_links(to_slug);

            CREATE TABLE IF NOT EXISTS brain_tags (
                slug          TEXT NOT NULL,
                tag           TEXT NOT NULL,
                PRIMARY KEY (slug, tag)
            );
            CREATE INDEX IF NOT EXISTS idx_brain_tags_tag
                ON brain_tags(tag);

            CREATE TABLE IF NOT EXISTS brain_timeline (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                slug          TEXT NOT NULL,
                entry_date    TEXT NOT NULL,
                source        TEXT,
                summary       TEXT NOT NULL,
                detail        TEXT,
                created_at    INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_brain_timeline_slug
                ON brain_timeline(slug);
            CREATE INDEX IF NOT EXISTS idx_brain_timeline_date
                ON brain_timeline(entry_date);

            CREATE TABLE IF NOT EXISTS brain_versions (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                slug          TEXT NOT NULL,
                content       TEXT NOT NULL,
                frontmatter   TEXT,
                snapshot_at   INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_brain_versions_slug
                ON brain_versions(slug);

            CREATE TABLE IF NOT EXISTS brain_consultations (
                id TEXT PRIMARY KEY,
                question TEXT NOT NULL,
                hits TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                outcome TEXT
            );
            CREATE TABLE IF NOT EXISTS brain_aliases (
                slug          TEXT NOT NULL,
                alias         TEXT NOT NULL,
                PRIMARY KEY (slug, alias)
            );
            CREATE INDEX IF NOT EXISTS idx_brain_aliases_alias
                ON brain_aliases(alias);

            -- What changed, for clients in other processes (TICKET-035).
            CREATE TABLE IF NOT EXISTS changes (
                seq     INTEGER PRIMARY KEY AUTOINCREMENT,
                kind    TEXT NOT NULL,
                key     TEXT NOT NULL,
                op      TEXT NOT NULL,
                detail  TEXT NOT NULL DEFAULT '',
                hash    TEXT NOT NULL DEFAULT '',
                at      INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_changes_key ON changes(kind, key);

            CREATE TABLE IF NOT EXISTS conversation_archive (
                session_id      TEXT PRIMARY KEY,
                title           TEXT NOT NULL,
                summary         TEXT,
                project         TEXT,
                git_branch      TEXT,
                started_at      TEXT,
                ended_at        TEXT,
                message_count   INTEGER NOT NULL DEFAULT 0,
                user_count      INTEGER NOT NULL DEFAULT 0,
                transcript_path TEXT,
                brain_slug      TEXT,
                ingested_at     INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_conv_archive_started
                ON conversation_archive(started_at);

            CREATE VIRTUAL TABLE IF NOT EXISTS conversation_archive_fts USING fts5(
                session_id UNINDEXED,
                title,
                summary,
                transcript,
                tokenize='porter unicode61'
            );
            ",
        )
        .map_err(|e| format!("Migration failed: {e}"))?;

        // Backfill columns added to the CREATE statements above after some
        // databases were already created (CREATE TABLE IF NOT EXISTS never alters
        // an existing table). ALTER TABLE ADD COLUMN errors if the column already
        // exists, so the result is ignored — this is idempotent.
        let _ = conn.execute(
            "ALTER TABLE conversations ADD COLUMN tts_muted INTEGER NOT NULL DEFAULT 0",
            [],
        );

        // One importance vocabulary (TICKET-052): `medium`, which `store_memory` used to
        // document, reads as `normal`, and the three words are stored in lower case.
        // Idempotent; a row already right is not touched.
        conn.execute(
            "UPDATE memories SET importance = CASE lower(trim(importance)) WHEN 'medium' THEN 'normal' ELSE lower(trim(importance)) END \
             WHERE lower(trim(importance)) IN ('low', 'medium', 'normal', 'high') \
             AND importance <> CASE lower(trim(importance)) WHEN 'medium' THEN 'normal' ELSE lower(trim(importance)) END",
            [],
        )
        .map_err(|e| format!("Migration failed (memory importance): {e}"))?;

        Ok(())
    }

    /// Create a Database from an existing connection (for testing).
    pub fn from_conn(conn: Connection) -> Self {
        // As `open` does, so tests run under the same settings and enforcement.
        let _ = Self::prepare(&conn);
        Self {
            conn: Mutex::new(conn),
        }
    }

    /// The settings every connection runs with: WAL for concurrent reads, a busy timeout
    /// so a write from a second process (the service, the CLI, an agent's server) waits
    /// instead of failing with `SQLITE_BUSY`, and foreign keys enforced (TICKET-045).
    fn prepare(conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            // The timeout first: switching a new database to WAL takes a lock that another
            // process starting at the same moment may hold.
            "PRAGMA busy_timeout=5000; PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;",
        )
        .map_err(|e| format!("Failed to set connection pragmas: {e}"))
    }

    /// Record a change for other processes (TICKET-035). Takes the connection guard, so
    /// a caller that already holds it uses [`crate::engine::changes::record`] instead.
    pub fn record_change(&self, kind: &str, key: &str, op: &str, detail: &str) {
        match self.conn() {
            Ok(conn) => crate::engine::changes::record(&conn, kind, key, op, detail, ""),
            Err(e) => eprintln!("rusty: change log: {e}"),
        }
    }

    /// Open (and migrate) the store at `path`, with the same settings as [`Self::open`].
    /// For tests that need two connections to one file, as two processes would have.
    pub fn open_path(path: &std::path::Path) -> Result<Self, String> {
        Self::register_extensions();
        let conn = Connection::open(path)
            .map_err(|e| format!("Failed to open database at {}: {e}", path.display()))?;
        Self::prepare(&conn)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    /// How many rows violate a foreign key (`PRAGMA foreign_key_check`).
    pub fn foreign_key_violations(&self) -> Result<usize, String> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("PRAGMA foreign_key_check")
            .map_err(|e| format!("Query error: {e}"))?;
        let rows = stmt
            .query_map([], |_| Ok(()))
            .map_err(|e| format!("Query error: {e}"))?
            .count();
        Ok(rows)
    }

    /// Get a locked reference to the connection for executing queries.
    pub fn conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.conn.lock().map_err(|e| e.to_string())
    }

    /// Resolve the database file path.
    fn db_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".rusty")
            .join("rusty.db")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_and_migrate() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        let db = Database {
            conn: Mutex::new(conn),
        };
        assert!(db.migrate().is_ok());
    }

    #[test]
    fn importance_words_are_rewritten_at_open() {
        let db = Database::from_conn(Connection::open_in_memory().unwrap());
        db.migrate().unwrap();
        {
            let conn = db.conn().unwrap();
            for (id, importance) in [
                ("a", "medium"),
                ("b", "High"),
                ("c", " low "),
                ("d", "normal"),
                ("e", "urgent"),
            ] {
                conn.execute(
                    "INSERT INTO memories (id, category, importance, content, source, created_at, updated_at) \
                     VALUES (?1, 'fact', ?2, 'x', 'test', 0, 0)",
                    rusqlite::params![id, importance],
                )
                .unwrap();
            }
        }
        db.migrate().unwrap();
        db.migrate().unwrap();
        let conn = db.conn().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, importance FROM memories ORDER BY id")
            .unwrap();
        let rows: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let expected = [
            ("a", "normal"),
            ("b", "high"),
            ("c", "low"),
            ("d", "normal"),
            ("e", "urgent"),
        ];
        let expected: Vec<(String, String)> = expected
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        assert_eq!(
            rows, expected,
            "an unknown word is left for a person to read"
        );
    }

    #[test]
    fn foreign_keys_are_on() {
        let db = Database::from_conn(Connection::open_in_memory().unwrap());
        let on: i64 = db
            .conn()
            .unwrap()
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(on, 1);
    }

    #[test]
    fn violations_are_counted() {
        let db = Database::from_conn(Connection::open_in_memory().unwrap());
        db.migrate().unwrap();
        assert_eq!(db.foreign_key_violations().unwrap(), 0);
        {
            let conn = db.conn().unwrap();
            conn.execute_batch(
                "PRAGMA foreign_keys=OFF; \
                 INSERT INTO user_tasks (header_id, title, sort_order, created_at) VALUES (99, 'old orphan', 0, 0); \
                 PRAGMA foreign_keys=ON;",
            )
            .unwrap();
        }
        assert_eq!(db.foreign_key_violations().unwrap(), 1);
        let left: i64 = db
            .conn()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM user_tasks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 1, "reported, not repaired");
    }
}
