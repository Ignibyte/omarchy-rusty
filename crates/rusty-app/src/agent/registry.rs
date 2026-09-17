//! The sessions the machine knows: one JSON entry per session under the state
//! directory, written by `start` and rewritten by the host; the `Agents` QML type
//! (`crate::agents`) lists them and watches the directory. Liveness is never in the
//! entry: it is the socket.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::protocol::now;
use super::spawn::SpawnOptions;
use super::{paths, SessionId};

/// A session as the registry keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub title: String,
    /// The page this conversation is about, for the note pane; none for a general one.
    #[serde(default)]
    pub page: Option<String>,
    pub cwd: String,
    pub created: String,
    pub updated: String,
    pub options: SpawnOptions,
    /// The Claude session id, once a turn announced it; the Rusty id until a resume
    /// fails and a fresh conversation takes over.
    #[serde(default)]
    pub claude_session_id: Option<String>,
    /// The host's last word: `starting`, `idle`, `working`, `waiting`, `sleeping`,
    /// `stopped`.
    pub state: String,
}

impl Entry {
    /// A new entry, about to be started.
    pub fn new(
        id: &SessionId,
        title: &str,
        page: Option<String>,
        cwd: &str,
        options: SpawnOptions,
    ) -> Self {
        let stamp = now();
        Self {
            id: id.to_string(),
            title: title.to_string(),
            page,
            cwd: cwd.to_string(),
            created: stamp.clone(),
            updated: stamp,
            options,
            claude_session_id: None,
            state: "starting".to_string(),
        }
    }
}

/// `<state_dir>/<id>.json`.
pub fn entry_path_in(state_dir: &Path, id: &SessionId) -> PathBuf {
    state_dir.join(format!("{id}.json"))
}

/// Write the entry atomically: a temporary file beside it, then a rename.
pub fn write_entry_in(state_dir: &Path, entry: &Entry) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(state_dir)?;
    // An entry carries the session's system prompt and working directory.
    let _ = std::fs::set_permissions(state_dir, std::fs::Permissions::from_mode(0o700));
    let id = SessionId::parse(&entry.id).map_err(io::Error::other)?;
    let path = entry_path_in(state_dir, &id);
    let staging = state_dir.join(format!("{id}.json.tmp"));
    let text = serde_json::to_string_pretty(entry).map_err(io::Error::other)?;
    std::fs::write(&staging, text)?;
    let _ = std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o600));
    std::fs::rename(&staging, &path)
}

/// Read one entry.
pub fn read_entry_in(state_dir: &Path, id: &SessionId) -> io::Result<Entry> {
    let text = std::fs::read_to_string(entry_path_in(state_dir, id))?;
    serde_json::from_str(&text).map_err(io::Error::other)
}

/// Every entry, newest `updated` first; unreadable files are skipped.
pub fn list_entries_in(state_dir: &Path) -> Vec<Entry> {
    let mut entries: Vec<Entry> = match std::fs::read_dir(state_dir) {
        Ok(dir) => dir
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .filter_map(|text| serde_json::from_str(&text).ok())
            .collect(),
        Err(_) => Vec::new(),
    };
    entries.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.id.cmp(&b.id)));
    entries
}

/// Delete the entry and the session's log directory.
pub fn remove_in(state_dir: &Path, id: &SessionId) -> io::Result<()> {
    let path = entry_path_in(state_dir, id);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    match std::fs::remove_dir_all(state_dir.join(id.as_str())) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Whether a host answers on the session's socket.
pub fn alive_at(socket: &Path) -> bool {
    match UnixStream::connect(socket) {
        Ok(stream) => {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let _ = stream.shutdown(std::net::Shutdown::Both);
            true
        }
        Err(_) => false,
    }
}

/// The entries of the default state directory.
pub fn list_entries() -> Vec<Entry> {
    list_entries_in(&paths::state_dir())
}

/// One entry of the default state directory.
pub fn read_entry(id: &SessionId) -> io::Result<Entry> {
    read_entry_in(&paths::state_dir(), id)
}

/// Whether the session's host answers.
pub fn alive(id: &SessionId) -> bool {
    alive_at(&paths::socket_path(id))
}

/// The rows QML lists.
pub fn rows_json(entries: &[Entry]) -> String {
    let rows: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            let alive = SessionId::parse(&e.id)
                .map(|id| alive(&id))
                .unwrap_or(false);
            serde_json::json!({
                "id": e.id,
                "title": e.title,
                "page": e.page,
                "cwd": e.cwd,
                "state": e.state,
                "alive": alive,
                "updated": e.updated,
                "model": e.options.model,
                "mode": e.options.permission_mode,
            })
        })
        .collect();
    serde_json::Value::Array(rows).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::spawn::read_tools;

    fn options() -> SpawnOptions {
        SpawnOptions {
            permission_mode: "default".into(),
            model: None,
            strict_mcp: true,
            allowed_tools: read_tools(),
            system_prompt: "x".into(),
            mcp_url: "http://127.0.0.1:4174/mcp".into(),
            name: None,
            idle_timeout_secs: 600,
            resume: None,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rusty_reg_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn entries_are_written_read_listed_and_removed() {
        let dir = scratch("crud");
        let a = SessionId::parse("aaaa-1").unwrap();
        let b = SessionId::parse("bbbb-2").unwrap();
        let mut first = Entry::new(
            &a,
            "First",
            Some("projects/orbit".into()),
            "/home/x",
            options(),
        );
        first.updated = "2026-09-17T10:00:00.000Z".into();
        let mut second = Entry::new(&b, "Second", None, "/home/y", options());
        second.updated = "2026-09-17T11:00:00.000Z".into();
        write_entry_in(&dir, &first).unwrap();
        write_entry_in(&dir, &second).unwrap();
        assert!(
            !dir.join("aaaa-1.json.tmp").exists(),
            "the staging file is renamed away"
        );
        assert_eq!(read_entry_in(&dir, &a).unwrap(), first);
        let listed = list_entries_in(&dir);
        assert_eq!(
            listed.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            vec!["bbbb-2", "aaaa-1"],
            "newest first"
        );
        std::fs::write(dir.join("broken.json"), "not json").unwrap();
        assert_eq!(list_entries_in(&dir).len(), 2, "a broken file is skipped");
        std::fs::create_dir_all(dir.join("aaaa-1")).unwrap();
        std::fs::write(dir.join("aaaa-1/events.jsonl"), "{}\n").unwrap();
        remove_in(&dir, &a).unwrap();
        assert!(!dir.join("aaaa-1.json").exists() && !dir.join("aaaa-1").exists());
        remove_in(&dir, &a).unwrap();
        let rows: serde_json::Value =
            serde_json::from_str(&rows_json(&list_entries_in(&dir))).unwrap();
        assert_eq!(rows[0]["id"], "bbbb-2");
        assert_eq!(rows[0]["alive"], false);
        assert_eq!(rows[0]["mode"], "default");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The log and the entries are the user's own: nobody else on the machine reads what
    /// was typed to an agent.
    #[test]
    fn the_state_stays_with_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("perms");
        let id = SessionId::parse("perm-1").unwrap();
        write_entry_in(&dir, &Entry::new(&id, "x", None, "/tmp", options())).unwrap();
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&entry_path_in(&dir, &id)), 0o600);
        let log =
            crate::agent::log::EventLog::open(&dir.join(id.as_str()).join("events.jsonl")).unwrap();
        drop(log);
        assert_eq!(mode(&dir.join(id.as_str())), 0o700);
        assert_eq!(mode(&dir.join(id.as_str()).join("events.jsonl")), 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nobody_is_alive_without_a_socket() {
        let dir = scratch("alive");
        assert!(!alive_at(&dir.join("none.sock")));
        assert!(list_entries_in(&dir).is_empty());
    }
}
