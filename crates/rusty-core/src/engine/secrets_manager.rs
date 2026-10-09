//! The secrets file: a sourceable `.env`-style key/value file.
//!
//! Each secret is a `KEY=VALUE` line. Values are single-quoted on write so the
//! file stays `source`-able from a shell, and comment (`#`) / blank lines are
//! preserved across edits. Default location: `~/.rusty/.secret`.
//!
//! The file is written with `0600` permissions. The PIN in [`crate::engine::pin_lock`]
//! guards the tools that reveal or change a value, not the file.

use crate::engine::db::Database;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A single key/value secret.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Secret {
    /// Variable name (e.g. `ANTHROPIC_API_KEY`).
    pub key: String,
    /// Secret value (returned unquoted).
    pub value: String,
}

/// Reads and writes the secrets file, preserving non-key/value lines.
pub struct SecretsManager {
    path: PathBuf,
    /// Where a change is recorded for other processes, when attached (TICKET-035).
    changes: Option<Arc<Database>>,
}

impl SecretsManager {
    /// Create a manager backed by the file at `path` (created on first write).
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            changes: None,
        }
    }

    /// Record every set and delete in `db`'s change log, by key name only.
    pub fn with_changes(mut self, db: Arc<Database>) -> Self {
        self.changes = Some(db);
        self
    }

    fn record(&self, key: &str, op: &str) {
        if let Some(db) = &self.changes {
            db.record_change("secret", key, op, "");
        }
    }

    /// List all secrets in file order. Missing file → empty list.
    pub fn list(&self) -> Result<Vec<Secret>, String> {
        Ok(read_lines(&self.path)?
            .iter()
            .filter_map(|line| parse_line(line))
            .collect())
    }

    /// Read a single secret's value, or `None` if the key isn't set.
    ///
    /// Reads through to the file rather than caching, so a value changed by another process
    /// (or by editing `.secret` directly) takes effect on the next call.
    pub fn get(&self, key: &str) -> Option<String> {
        self.list()
            .ok()?
            .into_iter()
            .find(|secret| secret.key == key)
            .map(|secret| secret.value)
    }

    /// Insert or update a secret, preserving comments and ordering. New keys are
    /// appended.
    pub fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let key = key.trim();
        validate_key(key)?;
        validate_value(value)?;
        let mut lines = read_lines(&self.path)?;
        let formatted = format_line(key, value);
        let mut replaced = false;
        for line in lines.iter_mut() {
            if parse_line(line).is_some_and(|s| s.key == key) {
                *line = formatted.clone();
                replaced = true;
                break;
            }
        }
        if !replaced {
            lines.push(formatted);
        }
        write_lines(&self.path, &lines)?;
        self.record(key, "updated");
        Ok(())
    }

    /// Delete a secret by key. No-op if the key is absent.
    pub fn delete(&self, key: &str) -> Result<(), String> {
        let key = key.trim();
        let lines = read_lines(&self.path)?;
        let before = lines.len();
        let kept: Vec<String> = lines
            .into_iter()
            .filter(|line| parse_line(line).is_none_or(|s| s.key != key))
            .collect();
        if kept.len() == before {
            return Err(format!("No secret {key}"));
        }
        write_lines(&self.path, &kept)?;
        self.record(key, "deleted");
        Ok(())
    }
}

/// Parse a `KEY=VALUE` line into a [`Secret`]; `None` for comments and blanks.
fn parse_line(line: &str) -> Option<Secret> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let (key, raw) = trimmed.split_once('=')?;
    let key = key.trim().to_string();
    if key.is_empty() {
        return None;
    }
    Some(Secret {
        key,
        value: unquote(raw.trim()),
    })
}

/// Strip matching surrounding single/double quotes (undoing single-quote
/// escaping for values we wrote ourselves).
fn unquote(s: &str) -> String {
    let bytes = s.as_bytes();
    if s.len() >= 2 {
        let (first, last) = (bytes[0], bytes[s.len() - 1]);
        if first == b'\'' && last == b'\'' {
            return s[1..s.len() - 1].replace("'\\''", "'");
        }
        if first == b'"' && last == b'"' {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

/// Format `KEY='value'`, single-quoting so the file stays `source`-able for any
/// value (single quotes suppress all shell expansion).
fn format_line(key: &str, value: &str) -> String {
    let escaped = value.replace('\'', "'\\''");
    format!("{key}='{escaped}'")
}

/// Reject a value with a line break: the file holds one secret per line, so a break would
/// end the value early and turn its next line into a key of its own.
fn validate_value(value: &str) -> Result<(), String> {
    if value.contains(['\n', '\r']) {
        return Err(
            "a secret value cannot hold a line break; store a multi-line key base64-encoded"
                .to_string(),
        );
    }
    Ok(())
}

/// Reject keys that aren't valid shell variable names.
fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("Secret key cannot be empty".to_string());
    }
    if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!(
            "Invalid key '{key}': use only letters, digits, and underscore"
        ));
    }
    Ok(())
}

/// Read the file into lines. Missing file → empty vector.
fn read_lines(path: &Path) -> Result<Vec<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(content.lines().map(str::to_string).collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("Failed to read secrets file: {e}")),
    }
}

/// Write lines back to the file (creating parent dirs). The new text goes to a temporary
/// file created at mode `0600` beside it and is renamed over it, so the secrets are never
/// readable by others, even for a moment, and a crash leaves the old file whole.
fn write_lines(path: &Path, lines: &[String]) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {e}"))?;
    let mut body = lines.join("\n");
    body.push('\n');
    // One name per write, so two writes at once (threads, or processes) never share it.
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = parent.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options
        .open(&tmp)
        .and_then(|mut f| {
            f.write_all(body.as_bytes())?;
            f.sync_all()
        })
        .and_then(|()| std::fs::rename(&tmp, path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Failed to write secrets file: {e}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mgr() -> (SecretsManager, std::path::PathBuf) {
        // Unique temp path per test via the test thread name.
        let name = std::thread::current()
            .name()
            .unwrap_or("t")
            .replace("::", "_");
        let path = std::env::temp_dir().join(format!("rusty-secrets-{name}.secret"));
        let _ = std::fs::remove_file(&path);
        (SecretsManager::new(path.clone()), path)
    }

    #[test]
    fn a_line_break_in_a_value_is_refused_and_the_file_stays_private() {
        let (m, path) = mgr();
        assert!(m
            .set("NOTE", "line one\nAWS_SECRET_ACCESS_KEY=planted")
            .is_err());
        assert!(m.set("NOTE", "carriage\rreturn").is_err());
        m.set("API_TOKEN", "plain").unwrap();
        assert_eq!(m.list().unwrap().len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn set_list_delete_roundtrip() {
        let (m, _p) = mgr();
        assert!(m.list().unwrap().is_empty());
        m.set("API_KEY", "abc123").unwrap();
        m.set("TOKEN", "x=y z").unwrap();
        let list = m.list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].key, "API_KEY");
        assert_eq!(list[0].value, "abc123");
        assert_eq!(list[1].value, "x=y z");
        m.set("API_KEY", "updated").unwrap();
        assert_eq!(m.list().unwrap()[0].value, "updated");
        m.delete("API_KEY").unwrap();
        let list = m.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].key, "TOKEN");
        assert_eq!(m.delete("API_KEY").unwrap_err(), "No secret API_KEY");
        assert_eq!(
            m.list().unwrap().len(),
            1,
            "a failed delete changes nothing"
        );
    }

    #[test]
    fn preserves_comments_and_quotes_values() {
        let (m, p) = mgr();
        std::fs::write(&p, "# header comment\nRAW=plain\n").unwrap();
        m.set("NEW", "v'alue").unwrap();
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains("# header comment"));
        assert!(body.contains("RAW=plain"));
        // Round-trips a value containing a single quote.
        assert_eq!(
            m.list()
                .unwrap()
                .iter()
                .find(|s| s.key == "NEW")
                .unwrap()
                .value,
            "v'alue"
        );
    }

    #[test]
    fn rejects_bad_keys() {
        let (m, _p) = mgr();
        assert!(m.set("bad key", "x").is_err());
        assert!(m.set("", "x").is_err());
        assert!(m.set("OK_1", "x").is_ok());
    }
}
