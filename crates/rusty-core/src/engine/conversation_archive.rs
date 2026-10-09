//! Conversation archive — ingest Claude Code session transcripts into a
//! full-text-searchable archive and the brain knowledge graph.
//!
//! Claude Code writes every session as a JSONL transcript at
//! `~/.claude/projects/<cwd-slug>/<session_id>.jsonl`. Rusty's own tables only
//! keep lossy GUI summaries (prompt + final answer), so terminal sessions — and
//! the full turn-by-turn dialogue — were never captured. A thought shared in a
//! terminal session (e.g. an article idea) could be lost forever.
//!
//! This module closes that gap. [`ConversationArchive::ingest`] parses a
//! transcript, stores the dialogue text in the `conversation_archive_fts` FTS5
//! index (durable recall that survives the `.jsonl` being pruned), records
//! metadata in `conversation_archive`, and creates a `conversation` brain page —
//! a graph node that summarizes the session, points back to the transcript, and
//! `[[links]]` to related entities already in the brain.

use crate::brain::{enrichment, BrainManager};
use crate::engine::db::Database;
use std::io::BufRead;
use std::path::Path;
use std::sync::Arc;

/// Max bytes of dialogue text retained per conversation (safety bound; the
/// turn-by-turn dialogue is far smaller than the raw `.jsonl` because tool I/O
/// bodies are dropped, but huge sessions still need a ceiling).
const MAX_DIALOGUE_BYTES: usize = 2_000_000;

/// Parsed contents of a Claude Code transcript.
#[derive(Debug, Default)]
pub struct ParsedTranscript {
    /// Session id (from the transcript lines, falling back to the file stem).
    pub session_id: String,
    /// Claude Code's own generated session title (`aiTitle`), or a fallback.
    pub title: String,
    /// Working directory the session ran in.
    pub project: String,
    /// Git branch at the time of the session.
    pub git_branch: String,
    /// ISO timestamp of the first message.
    pub started_at: String,
    /// ISO timestamp of the last message.
    pub ended_at: String,
    /// Total user + assistant messages.
    pub message_count: i64,
    /// User messages only.
    pub user_count: i64,
    /// Role-tagged dialogue text (indexed into FTS).
    pub dialogue: String,
    /// User messages only, concatenated (used for entity linking).
    pub user_text: String,
}

/// Outcome of ingesting one transcript.
#[derive(Debug)]
pub struct IngestOutcome {
    /// Session id ingested.
    pub session_id: String,
    /// Title used for the brain node.
    pub title: String,
    /// Slug of the created/updated `conversation` brain page.
    pub brain_slug: String,
    /// Total messages archived.
    pub message_count: i64,
    /// Existing brain pages this conversation was linked to.
    pub linked: Vec<String>,
    /// True if a new node was created, false if an existing one was updated.
    pub created: bool,
}

/// Archives conversation transcripts into SQLite FTS + the brain graph.
pub struct ConversationArchive {
    db: Arc<Database>,
    brain: Arc<BrainManager>,
}

impl ConversationArchive {
    /// Create a new archive over the shared database and brain.
    pub fn new(db: Arc<Database>, brain: Arc<BrainManager>) -> Self {
        Self { db, brain }
    }

    /// Parse a Claude Code JSONL transcript file (streamed line-by-line).
    pub fn parse_transcript(path: &Path) -> Result<ParsedTranscript, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("open transcript: {e}"))?;
        let reader = std::io::BufReader::new(file);

        let mut t = ParsedTranscript::default();
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            t.session_id = stem.to_string();
        }

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            // The in-file sessionId is authoritative; the file stem (set above)
            // is only a fallback for transcripts that omit it.
            if let Some(sid) = v.get("sessionId").and_then(|x| x.as_str()) {
                if !sid.is_empty() {
                    t.session_id = sid.to_string();
                }
            }

            match v.get("type").and_then(|x| x.as_str()).unwrap_or("") {
                "ai-title" => {
                    if let Some(title) = v.get("aiTitle").and_then(|x| x.as_str()) {
                        if !title.trim().is_empty() {
                            t.title = title.trim().to_string();
                        }
                    }
                }
                role_type @ ("user" | "assistant") => {
                    if v.get("isMeta").and_then(|x| x.as_bool()).unwrap_or(false) {
                        continue; // skip injected meta turns
                    }
                    if let Some(ts) = v.get("timestamp").and_then(|x| x.as_str()) {
                        if t.started_at.is_empty() {
                            t.started_at = ts.to_string();
                        }
                        t.ended_at = ts.to_string();
                    }
                    if let Some(cwd) = v.get("cwd").and_then(|x| x.as_str()) {
                        t.project = cwd.to_string();
                    }
                    if let Some(b) = v.get("gitBranch").and_then(|x| x.as_str()) {
                        if !b.is_empty() {
                            t.git_branch = b.to_string();
                        }
                    }
                    let role = v
                        .get("message")
                        .and_then(|m| m.get("role"))
                        .and_then(|x| x.as_str())
                        .unwrap_or(role_type);
                    let text = extract_message_text(v.get("message"));
                    if text.trim().is_empty() {
                        continue;
                    }
                    if role == "user" {
                        t.user_count += 1;
                        push_capped(&mut t.user_text, &text);
                        push_capped(&mut t.user_text, "\n");
                    }
                    t.message_count += 1;
                    let tag = if role == "user" {
                        "## User"
                    } else {
                        "## Assistant"
                    };
                    push_capped(&mut t.dialogue, &format!("\n{tag}\n{text}\n"));
                }
                _ => {}
            }
        }

        if t.session_id.is_empty() {
            return Err("transcript has no session id".to_string());
        }
        if t.title.is_empty() {
            let first = first_substantive_line(&t.user_text);
            t.title = if first.is_empty() {
                "Untitled conversation".to_string()
            } else {
                truncate(first, 80)
            };
        }
        Ok(t)
    }

    /// Ingest a transcript file: archive it and create/update its brain node.
    pub fn ingest(&self, path: &Path) -> Result<IngestOutcome, String> {
        let parsed = Self::parse_transcript(path)?;
        self.ingest_parsed(&parsed, &path.to_string_lossy())
    }

    /// Ingest an already-parsed transcript.
    pub fn ingest_parsed(
        &self,
        t: &ParsedTranscript,
        transcript_path: &str,
    ) -> Result<IngestOutcome, String> {
        // Link to entities that ALREADY exist in the brain (keep the graph
        // clean — don't manufacture new entity pages from chat noise).
        let mut linked: Vec<String> = Vec::new();
        for ent in enrichment::extract_entities_passive(&t.user_text) {
            if let Some(slug) = self
                .brain
                .resolve_slug(&ent.name)
                .unwrap_or_default()
                .into_iter()
                .next()
            {
                if !linked.contains(&slug) {
                    linked.push(slug);
                }
            }
        }

        let summary = build_summary(t);
        let body = build_node_body(t, transcript_path, &summary, &linked);

        // Idempotent: reuse the existing node for this session if we have one.
        let (brain_slug, created) = match self.existing_brain_slug(&t.session_id) {
            Some(slug) if self.brain.read_page(&slug).ok().flatten().is_some() => {
                self.brain.update_page(&slug, &body)?;
                (slug, false)
            }
            _ => {
                let page = self.brain.create_page("conversation", &t.title, &body)?;
                (page.slug, true)
            }
        };

        self.upsert_archive(t, transcript_path, &summary, &brain_slug)?;
        self.upsert_fts(t, &summary)?;
        self.brain.flush_commits();

        Ok(IngestOutcome {
            session_id: t.session_id.clone(),
            title: t.title.clone(),
            brain_slug,
            message_count: t.message_count,
            linked,
            created,
        })
    }

    /// Full-text search the conversation archive (ranked, hyphen-safe).
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<ArchiveHit>, String> {
        let fts_query = sanitize_fts_query(query);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.db.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT a.session_id, a.title, IFNULL(a.project,''), IFNULL(a.started_at,''), \
                        IFNULL(a.brain_slug,''), \
                        snippet(conversation_archive_fts, 3, '[', ']', ' … ', 12) \
                 FROM conversation_archive_fts f \
                 JOIN conversation_archive a ON a.session_id = f.session_id \
                 WHERE conversation_archive_fts MATCH ?1 \
                 ORDER BY rank LIMIT ?2",
            )
            .map_err(|e| format!("search prepare: {e}"))?;
        let rows = stmt
            .query_map(rusqlite::params![fts_query, limit as i64], |row| {
                Ok(ArchiveHit {
                    session_id: row.get(0)?,
                    title: row.get(1)?,
                    project: row.get(2)?,
                    started_at: row.get(3)?,
                    brain_slug: row.get(4)?,
                    snippet: row.get(5)?,
                })
            })
            .map_err(|e| format!("search query: {e}"))?;
        Ok(rows.flatten().collect())
    }

    fn existing_brain_slug(&self, session_id: &str) -> Option<String> {
        let conn = self.db.conn().ok()?;
        conn.query_row(
            "SELECT brain_slug FROM conversation_archive WHERE session_id = ?1",
            rusqlite::params![session_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
    }

    fn upsert_archive(
        &self,
        t: &ParsedTranscript,
        transcript_path: &str,
        summary: &str,
        brain_slug: &str,
    ) -> Result<(), String> {
        let conn = self.db.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO conversation_archive \
             (session_id, title, summary, project, git_branch, started_at, ended_at, \
              message_count, user_count, transcript_path, brain_slug, ingested_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            rusqlite::params![
                t.session_id,
                t.title,
                summary,
                t.project,
                t.git_branch,
                t.started_at,
                t.ended_at,
                t.message_count,
                t.user_count,
                transcript_path,
                brain_slug,
                now_unix(),
            ],
        )
        .map_err(|e| format!("archive upsert: {e}"))?;
        Ok(())
    }

    fn upsert_fts(&self, t: &ParsedTranscript, summary: &str) -> Result<(), String> {
        let conn = self.db.conn()?;
        conn.execute(
            "DELETE FROM conversation_archive_fts WHERE session_id = ?1",
            rusqlite::params![t.session_id],
        )
        .map_err(|e| format!("fts clear: {e}"))?;
        conn.execute(
            "INSERT INTO conversation_archive_fts (session_id, title, summary, transcript) \
             VALUES (?1,?2,?3,?4)",
            rusqlite::params![t.session_id, t.title, summary, t.dialogue],
        )
        .map_err(|e| format!("fts insert: {e}"))?;
        Ok(())
    }
}

/// A conversation-archive search result.
#[derive(Debug, serde::Serialize)]
pub struct ArchiveHit {
    /// Session id.
    pub session_id: String,
    /// Conversation title.
    pub title: String,
    /// Project (cwd) the session ran in.
    pub project: String,
    /// ISO start timestamp.
    pub started_at: String,
    /// Slug of the conversation's brain node.
    pub brain_slug: String,
    /// Matching dialogue snippet.
    pub snippet: String,
}

/// Build a cheap, no-LLM summary: the title plus the first substantive user line.
fn build_summary(t: &ParsedTranscript) -> String {
    let first = first_substantive_line(&t.user_text);
    if first.is_empty() {
        t.title.clone()
    } else {
        format!("{} — {}", t.title, truncate(first, 200))
    }
}

/// First user line that is real prose — skips Claude Code's injected slash-command
/// wrappers (`<command-name>…`, `/clear`), tool markers, and blank lines.
fn first_substantive_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|l| {
            !l.is_empty()
                && !l.starts_with('<')
                && !l.starts_with('/')
                && !l.starts_with("[tool:")
                && !l.starts_with("Caveat:")
        })
        .unwrap_or("")
}

/// Render the `conversation` brain page body (summary + pointer + links).
fn build_node_body(
    t: &ParsedTranscript,
    transcript_path: &str,
    summary: &str,
    linked: &[String],
) -> String {
    let project = if t.project.is_empty() {
        "—"
    } else {
        &t.project
    };
    let mut body = format!(
        "{summary}\n\n\
         - **Session:** `{}`\n\
         - **Project:** `{project}`\n\
         - **Messages:** {} ({} from you)\n\
         - **When:** {} → {}\n\
         - **Transcript:** `{transcript_path}`\n",
        t.session_id,
        t.message_count,
        t.user_count,
        short_date(&t.started_at),
        short_date(&t.ended_at),
    );
    if !linked.is_empty() {
        let refs: Vec<String> = linked.iter().map(|s| format!("[[{s}]]")).collect();
        body.push_str("\n**Related:** ");
        body.push_str(&refs.join(" "));
        body.push('\n');
    }
    body
}

/// Extract the readable text from a message's `content` (string or block array).
///
/// Text blocks are kept verbatim; tool calls are reduced to a `[tool: NAME]`
/// marker so the flow is preserved without indexing huge tool-result bodies.
fn extract_message_text(msg: Option<&serde_json::Value>) -> String {
    let Some(msg) = msg else {
        return String::new();
    };
    match msg.get("content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => {
            let mut out = String::new();
            for block in arr {
                match block.get("type").and_then(|x| x.as_str()) {
                    Some("text") => {
                        if let Some(s) = block.get("text").and_then(|x| x.as_str()) {
                            out.push_str(s);
                            out.push('\n');
                        }
                    }
                    Some("tool_use") => {
                        if let Some(name) = block.get("name").and_then(|x| x.as_str()) {
                            out.push_str("[tool: ");
                            out.push_str(name);
                            out.push_str("]\n");
                        }
                    }
                    _ => {} // skip tool_result and other block types
                }
            }
            out
        }
        _ => String::new(),
    }
}

/// Append `s` to `buf` unless `buf` already hit the dialogue size ceiling.
fn push_capped(buf: &mut String, s: &str) {
    if buf.len() < MAX_DIALOGUE_BYTES {
        buf.push_str(s);
    }
}

/// Truncate to `max` characters (char-boundary safe), adding an ellipsis.
fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}…")
}

/// First 10 chars of an ISO timestamp (YYYY-MM-DD), or `—` when empty.
fn short_date(iso: &str) -> &str {
    if iso.len() >= 10 {
        &iso[..10]
    } else if iso.is_empty() {
        "—"
    } else {
        iso
    }
}

/// Build a safe FTS5 query: each whitespace term becomes a quoted phrase with
/// interior punctuation neutralized, AND-ed together. This avoids FTS5 syntax
/// errors on `-`/`:`/etc. (the bug where `brain search "push-live"` matched
/// nothing) and keeps matching intuitive.
fn sanitize_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| {
            term.chars()
                .map(|c| if c.is_alphanumeric() { c } else { ' ' })
                .collect::<String>()
                .trim()
                .to_string()
        })
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Current unix time in seconds.
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::db::Database;
    use rusqlite::Connection;
    use std::io::Write;
    use std::path::PathBuf;

    fn write_transcript(name: &str, lines: &[&str]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "rusty_conv_test_{}_{name}.jsonl",
            std::process::id()
        ));
        let mut f = std::fs::File::create(&path).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        path
    }

    fn test_archive(name: &str) -> (PathBuf, ConversationArchive) {
        let dir =
            std::env::temp_dir().join(format!("rusty_conv_brain_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        let db = Arc::new(Database::from_conn(conn));
        db.migrate().unwrap();
        let brain = Arc::new(BrainManager::new(Arc::clone(&db), dir.clone()));
        brain.ensure_vault().unwrap();
        (dir, ConversationArchive::new(db, brain))
    }

    #[test]
    fn parses_title_messages_and_metadata() {
        let path = write_transcript(
            "parse",
            &[
                r#"{"type":"ai-title","aiTitle":"Queue up article for writing","sessionId":"sid-1"}"#,
                r#"{"type":"user","sessionId":"sid-1","cwd":"/proj","gitBranch":"main","timestamp":"2026-06-27T10:00:00.000Z","message":{"role":"user","content":"Let's draft the Ignibyte launch article."}}"#,
                r#"{"type":"assistant","sessionId":"sid-1","timestamp":"2026-06-27T10:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Sure, here is an outline."},{"type":"tool_use","name":"Bash"}]}}"#,
            ],
        );
        let t = ConversationArchive::parse_transcript(&path).unwrap();
        assert_eq!(t.session_id, "sid-1");
        assert_eq!(t.title, "Queue up article for writing");
        assert_eq!(t.project, "/proj");
        assert_eq!(t.user_count, 1);
        assert_eq!(t.message_count, 2);
        assert_eq!(t.started_at, "2026-06-27T10:00:00.000Z");
        assert!(t.dialogue.contains("Ignibyte launch article"));
        assert!(t.dialogue.contains("[tool: Bash]"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ingest_creates_node_and_is_searchable() {
        let (dir, archive) = test_archive("ingest");
        let path = write_transcript(
            "ingest",
            &[
                r#"{"type":"ai-title","aiTitle":"Plan the quokka migration","sessionId":"sid-2"}"#,
                r#"{"type":"user","sessionId":"sid-2","cwd":"/proj","timestamp":"2026-06-27T10:00:00.000Z","message":{"role":"user","content":"We should migrate the Quokkanaut service to Rust."}}"#,
            ],
        );

        let out = archive.ingest(&path).unwrap();
        assert!(out.created);
        assert_eq!(out.session_id, "sid-2");
        assert_eq!(out.brain_slug, "conversations/plan-the-quokka-migration");

        // Searchable via the archive FTS (hyphen-safe sanitizer too).
        let hits = archive.search("Quokkanaut", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, "sid-2");

        // The brain node exists and points back to the transcript.
        let node = archive.brain.read_page(&out.brain_slug).unwrap().unwrap();
        assert_eq!(node.page_type, "conversation");
        assert!(node.compiled_truth.contains("sid-2"));

        // Re-ingest updates the same node (idempotent), doesn't duplicate.
        let out2 = archive.ingest(&path).unwrap();
        assert!(!out2.created);
        assert_eq!(out2.brain_slug, out.brain_slug);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
