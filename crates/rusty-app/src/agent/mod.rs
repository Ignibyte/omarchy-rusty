//! The `agent` noun (TICKET-031): Claude Code sessions that outlive the app. A session
//! is a transient user unit `rusty-agent-<id>` running [`host`], which owns the one
//! `claude -p` process over stream-json, logs every event and serves them over a Unix
//! socket; the app's `Assistant` and `rusty agent attach` are its clients, and a
//! conversation goes on when the window is closed. The verbs: `start`, `stop`, `list`,
//! `attach`, `rm`, and `host` (what the unit runs).

pub mod client;
pub mod host;
pub mod launch;
pub mod log;
pub mod protocol;
pub mod registry;
pub mod spawn;
pub mod wire;

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use client::{Connection, Incoming};
use launch::{NewSession, Runner};
use protocol::ClientMsg;
use spawn::SpawnOptions;

/// What `rusty agent` alone, or with a verb it lacks, prints.
pub const AGENT_USAGE: &str = "\
usage: rusty agent <verb> [args...]
  rusty agent start [--cwd <dir>] [--title <text>] [--page <slug>] [--mode <mode>]
                    [--model <name>] [--idle <seconds>] [--resume <claude session>]
                    [--strict-mcp] [--reads] [--prompt <text>]
                             a new session under its own unit; prints the id
  rusty agent start <id>     run a stopped session's host again
  rusty agent stop <id>      interrupt the turn, end the process and the host
  rusty agent list           every session: id, state, alive, title, cwd, updated
  rusty agent attach <id> [--since <n>]
                             the event stream in this terminal; a typed line is a
                             message, a typed JSON line goes to the process as is
  rusty agent rm <id>        stop it and delete its entry and log
  rusty agent host --id <id> what rusty-agent-<id>.service runs
Sessions live under ~/.local/state/rusty/agents (RUSTY_AGENT_STATE_DIR), their sockets
under $XDG_RUNTIME_DIR/rusty/agents (RUSTY_AGENT_RUN_DIR).";

/// A session's id: a uuid, valid as a path component and a unit name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// A fresh v4 uuid, which the process also takes as its Claude session id.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Accept letters, digits and dashes, up to 64 of them.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let valid = !text.is_empty()
            && text.len() <= 64
            && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if valid {
            Ok(Self(text.to_string()))
        } else {
            Err(format!("not a session id: '{text}'"))
        }
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The unit a session runs as.
pub fn unit_name(id: &SessionId) -> String {
    format!("rusty-agent-{id}")
}

/// Where sessions keep their files.
pub mod paths {
    use super::SessionId;
    use std::path::PathBuf;

    fn env_dir(var: &str) -> Option<PathBuf> {
        std::env::var_os(var)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    }

    /// `~/.local/state/rusty/agents`, or `RUSTY_AGENT_STATE_DIR`: an entry per session
    /// beside a directory with its log.
    pub fn state_dir() -> PathBuf {
        env_dir("RUSTY_AGENT_STATE_DIR").unwrap_or_else(|| {
            dirs::state_dir()
                .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
                .unwrap_or_else(|| PathBuf::from("."))
                .join("rusty/agents")
        })
    }

    /// `$XDG_RUNTIME_DIR/rusty/agents`, or `RUSTY_AGENT_RUN_DIR`: the sockets.
    pub fn run_dir() -> PathBuf {
        env_dir("RUSTY_AGENT_RUN_DIR").unwrap_or_else(|| {
            dirs::runtime_dir()
                .unwrap_or_else(|| std::env::temp_dir().join(format!("rusty-{}", unsafe_uid())))
                .join("rusty/agents")
        })
    }

    fn unsafe_uid() -> String {
        std::env::var("UID").unwrap_or_else(|_| "user".to_string())
    }

    /// The longest a Unix socket's path may be, minus room for the terminator.
    pub const SOCKET_PATH_MAX: usize = 107;

    /// The session's socket under a run directory.
    pub fn socket_path_in(run_dir: &std::path::Path, id: &SessionId) -> PathBuf {
        run_dir.join(format!("{id}.sock"))
    }

    /// Refuse a socket path the kernel would cut; the run directory is `RUSTY_AGENT_RUN_DIR`
    /// when a long scratch path is in use.
    pub fn check_socket_path(path: &std::path::Path) -> Result<(), String> {
        let len = path.as_os_str().len();
        if len > SOCKET_PATH_MAX {
            Err(format!(
                "the socket path is too long for a Unix socket ({len} bytes, the limit is {SOCKET_PATH_MAX}): {}",
                path.display()
            ))
        } else {
            Ok(())
        }
    }

    /// The session's socket.
    pub fn socket_path(id: &SessionId) -> PathBuf {
        socket_path_in(&run_dir(), id)
    }

    /// The session's log under a state directory: a directory named after the session,
    /// beside its entry.
    pub fn log_path_in(state_dir: &std::path::Path, id: &SessionId) -> PathBuf {
        state_dir.join(id.as_str()).join("events.jsonl")
    }
}

/// `--flag value` pairs and bare words, the way the verbs take them.
#[derive(Debug, Default, PartialEq)]
struct Args {
    words: Vec<String>,
    flags: Vec<(String, String)>,
}

/// Split the verb's arguments: `--strict-mcp` and `--reads` stand alone, every other
/// `--flag` takes the next argument as its value.
fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut out = Args::default();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(name) = a.strip_prefix("--") {
            if name == "strict-mcp" || name == "reads" {
                out.flags.push((name.to_string(), "true".to_string()));
                i += 1;
                continue;
            }
            let Some(value) = args.get(i + 1) else {
                return Err(format!("--{name} needs a value"));
            };
            out.flags.push((name.to_string(), value.clone()));
            i += 2;
        } else {
            out.words.push(a.clone());
            i += 1;
        }
    }
    Ok(out)
}

impl Args {
    fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

fn usage_error(message: &str) -> i32 {
    eprintln!("rusty agent: {message}");
    eprintln!("{AGENT_USAGE}");
    2
}

/// `rusty agent ...`: run the verb, answer with the exit code.
pub fn run(args: &[String]) -> i32 {
    let Some(verb) = args.first().map(String::as_str) else {
        eprintln!("{AGENT_USAGE}");
        return 2;
    };
    let rest = &args[1..];
    let parsed = match parse_args(rest) {
        Ok(p) => p,
        Err(e) => return usage_error(&e),
    };
    match verb {
        "start" => start(&parsed),
        "stop" => with_id(&parsed, |id| launch::stop(&Runner::from_env(), &id)),
        "list" => list(),
        "attach" => match parsed.words.first() {
            Some(word) => match SessionId::parse(word) {
                Ok(id) => attach(
                    &id,
                    parsed
                        .flag("since")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0),
                ),
                Err(e) => usage_error(&e),
            },
            None => usage_error("attach needs a session id"),
        },
        "rm" => with_id(&parsed, |id| {
            if registry::alive(&id) {
                launch::stop(&Runner::from_env(), &id)?;
            }
            registry::remove_in(&paths::state_dir(), &id).map_err(|e| e.to_string())
        }),
        "host" => match parsed.flag("id").map(SessionId::parse) {
            Some(Ok(id)) => match host::serve(host::HostConfig::from_env(id)) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("rusty agent host: {e}");
                    1
                }
            },
            Some(Err(e)) => usage_error(&e),
            None => usage_error("host needs --id <id>"),
        },
        other => usage_error(&format!("unknown verb '{other}'")),
    }
}

fn with_id(parsed: &Args, act: impl FnOnce(SessionId) -> Result<(), String>) -> i32 {
    let Some(word) = parsed.words.first() else {
        return usage_error("a session id is needed");
    };
    match SessionId::parse(word) {
        Ok(id) => match act(id) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("rusty agent: {e}");
                1
            }
        },
        Err(e) => usage_error(&e),
    }
}

/// The MCP URL a session hands its process: `RUSTY_MCP_URL` or the app's default.
fn mcp_url() -> String {
    std::env::var("RUSTY_MCP_URL")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:4174/mcp".to_string())
}

fn start(parsed: &Args) -> i32 {
    let runner = Runner::from_env();
    if let Some(word) = parsed.words.first() {
        return match SessionId::parse(word) {
            Ok(id) => match launch::start_existing(&runner, &id) {
                Ok(()) => {
                    println!("started {id} ({}.service)", unit_name(&id));
                    0
                }
                Err(e) => {
                    eprintln!("rusty agent: {e}");
                    1
                }
            },
            Err(e) => usage_error(&e),
        };
    }
    let cwd = parsed
        .flag("cwd")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let cwd = match cwd.canonicalize() {
        Ok(c) if c.is_dir() => c,
        _ => return usage_error(&format!("not a directory: {}", cwd.display())),
    };
    let title = parsed.flag("title").map(str::to_string).unwrap_or_else(|| {
        cwd.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "agent".to_string())
    });
    let idle = match parsed.flag("idle").map(str::parse::<u64>) {
        Some(Ok(n)) => n,
        Some(Err(_)) => return usage_error("--idle takes a number of seconds"),
        None => 0,
    };
    let options = SpawnOptions {
        permission_mode: parsed.flag("mode").unwrap_or("default").to_string(),
        model: parsed.flag("model").map(str::to_string),
        strict_mcp: parsed.flag("strict-mcp").is_some(),
        allowed_tools: if parsed.flag("reads").is_some() {
            spawn::read_tools()
        } else {
            Vec::new()
        },
        system_prompt: parsed.flag("prompt").unwrap_or("").to_string(),
        mcp_url: mcp_url(),
        name: Some(title.clone()),
        idle_timeout_secs: idle,
        resume: parsed.flag("resume").map(str::to_string),
    };
    let request = NewSession {
        cwd: cwd.to_string_lossy().into_owned(),
        title,
        page: parsed.flag("page").map(str::to_string),
        options,
    };
    match launch::start_new(&runner, request) {
        Ok(id) => {
            println!("started {id} ({}.service)", unit_name(&id));
            0
        }
        Err(e) => {
            eprintln!("rusty agent: {e}");
            1
        }
    }
}

fn list() -> i32 {
    let entries = registry::list_entries();
    if entries.is_empty() {
        println!("no sessions");
        return 0;
    }
    println!(
        "{:<38} {:<9} {:<5} {:<24} {:<32} UPDATED",
        "ID", "STATE", "ALIVE", "TITLE", "CWD"
    );
    for e in entries {
        let alive = SessionId::parse(&e.id)
            .map(|id| registry::alive(&id))
            .unwrap_or(false);
        println!(
            "{:<38} {:<9} {:<5} {:<24} {:<32} {}",
            e.id,
            e.state,
            if alive { "yes" } else { "no" },
            brief(&e.title, 24),
            brief(&e.cwd, 32),
            e.updated
        );
    }
    0
}

fn brief(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let cut: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// The stream in a terminal: every line the host sends, raw; every line typed goes in.
fn attach(id: &SessionId, since: u64) -> i32 {
    let socket = paths::socket_path(id);
    let mut connection = match Connection::connect(&socket) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "rusty agent: no host answers for {id} ({e}); `rusty agent start {id}` runs it"
            );
            return 1;
        }
    };
    match connection.read_line_timeout(Duration::from_secs(5)) {
        Ok(Some(hello)) => println!("{hello}"),
        _ => {
            eprintln!("rusty agent: the host said nothing");
            return 1;
        }
    }
    if let Err(e) = connection.send(&ClientMsg::Attach { since }) {
        eprintln!("rusty agent: {e}");
        return 1;
    }
    let (tx, rx) = std::sync::mpsc::channel::<Incoming>();
    if let Err(e) = connection.spawn_reader(move |incoming| {
        let _ = tx.send(incoming);
    }) {
        eprintln!("rusty agent: {e}");
        return 1;
    }
    // Typing ends the view only when someone is typing: with stdin redirected (a pipe,
    // `< /dev/null`), its end means there is nothing more to say, not that the stream
    // should stop.
    // SAFETY: `isatty` reads the mode of a descriptor and changes nothing.
    let interactive = unsafe { libc::isatty(libc::STDIN_FILENO) == 1 };
    let (typed_tx, typed_rx) = std::sync::mpsc::channel::<Option<String>>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut line = String::new();
        loop {
            line.clear();
            match std::io::BufRead::read_line(&mut stdin.lock(), &mut line) {
                Ok(0) | Err(_) => {
                    let _ = typed_tx.send(None);
                    break;
                }
                Ok(_) => {
                    let _ = typed_tx.send(Some(line.trim_end_matches(['\n', '\r']).to_string()));
                }
            }
        }
    });
    loop {
        if let Ok(typed) = typed_rx.try_recv() {
            match typed {
                None if interactive => {
                    connection.close();
                    return 0;
                }
                None => {}
                Some(text) if text.trim().is_empty() => {}
                Some(text) => {
                    let line = if text.trim_start().starts_with('{') {
                        serde_json::from_str::<serde_json::Value>(&text).ok()
                    } else {
                        serde_json::from_str(&wire::user_message(&text)).ok()
                    };
                    match line {
                        Some(line) => {
                            if let Err(e) = connection.send(&ClientMsg::Send { line }) {
                                eprintln!("rusty agent: {e}");
                                return 1;
                            }
                        }
                        None => eprintln!("rusty agent: not JSON, not sent"),
                    }
                }
            }
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Incoming::Line(line)) => println!("{line}"),
            Ok(Incoming::Closed(why)) => {
                eprintln!("rusty agent: {why}");
                return 0;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return 0,
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::path::{Path, PathBuf};

    /// The spawn tests run one at a time: a script written by one test thread while
    /// another forks to exec its own can answer `ETXTBSY` (the child holds the writer's
    /// descriptor until the exec), so they take this lock.
    pub static SPAWN: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A bash script standing in for `claude`, executable, landed by a rename.
    pub fn fake(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("claude-fake");
        let staging = dir.join("claude-fake.tmp");
        std::fs::write(&staging, format!("#!/usr/bin/env bash\n{body}")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::rename(&staging, &path).unwrap();
        path
    }

    /// A short scratch directory (socket paths are capped at 107 bytes), emptied.
    pub fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ra_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_paths_and_unit_names() {
        let id = SessionId::parse(" 8b3f0c2e-5d1a-4e7b-9c3d-2f6a1b8e4d70 ").unwrap();
        assert_eq!(id.as_str(), "8b3f0c2e-5d1a-4e7b-9c3d-2f6a1b8e4d70");
        assert_eq!(
            unit_name(&id),
            "rusty-agent-8b3f0c2e-5d1a-4e7b-9c3d-2f6a1b8e4d70"
        );
        for bad in ["", "../x", "a b", "x/y", &"a".repeat(65)] {
            assert!(SessionId::parse(bad).is_err(), "{bad:?}");
        }
        let fresh = SessionId::new();
        assert_eq!(fresh.as_str().len(), 36);
        assert!(SessionId::parse(fresh.as_str()).is_ok());
    }

    #[test]
    fn arguments_split_into_words_and_flags() {
        let args: Vec<String> = ["x-1", "--since", "5", "--reads", "--mode", "plan"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.words, vec!["x-1"]);
        assert_eq!(parsed.flag("since"), Some("5"));
        assert_eq!(parsed.flag("reads"), Some("true"));
        assert_eq!(parsed.flag("mode"), Some("plan"));
        assert_eq!(parsed.flag("model"), None);
        let dangling: Vec<String> = vec!["--cwd".to_string()];
        assert!(parse_args(&dangling).unwrap_err().contains("--cwd"));
    }

    #[test]
    fn verbs_that_cannot_run_answer_with_the_usage() {
        let owned = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(run(&owned(&[])), 2);
        assert_eq!(run(&owned(&["bogus"])), 2);
        assert_eq!(run(&owned(&["stop"])), 2);
        assert_eq!(run(&owned(&["stop", "not valid!"])), 2);
        assert_eq!(run(&owned(&["attach"])), 2);
        assert_eq!(run(&owned(&["host"])), 2);
        assert_eq!(run(&owned(&["host", "--id", "bad id"])), 2);
        assert_eq!(run(&owned(&["start", "--idle", "soon"])), 2);
        assert_eq!(run(&owned(&["start", "--cwd", "/nonexistent/dir"])), 2);
        for verb in ["start", "stop", "list", "attach", "rm", "host"] {
            assert!(
                AGENT_USAGE.contains(&format!("rusty agent {verb}")),
                "{verb}"
            );
        }
    }

    #[test]
    fn the_paths_follow_the_overrides() {
        let id = SessionId::parse("abc").unwrap();
        assert!(paths::socket_path(&id).ends_with("abc.sock"));
        assert!(paths::log_path_in(std::path::Path::new("/s"), &id).ends_with("abc/events.jsonl"));
        assert_eq!(
            paths::socket_path_in(std::path::Path::new("/r"), &id),
            PathBuf::from("/r/abc.sock")
        );
        assert!(
            paths::state_dir().ends_with("rusty/agents")
                || std::env::var_os("RUSTY_AGENT_STATE_DIR").is_some()
        );
    }
}
