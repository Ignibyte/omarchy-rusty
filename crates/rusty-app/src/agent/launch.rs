//! Starting and stopping a session's host through the user manager: one transient unit
//! per session, `systemd-run --user`, the host the same binary as the caller.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use super::client::Connection;
use super::protocol::ClientMsg;
use super::registry::{self, Entry};
use super::spawn::SpawnOptions;
use super::{paths, unit_name, SessionId};

/// The programs that drive systemd; `RUSTY_SYSTEMD_RUN` and `RUSTY_SYSTEMCTL` substitute
/// a stand-in that logs its arguments (tests, the screenshot scenes).
#[derive(Debug, Clone)]
pub struct Runner {
    pub systemd_run: PathBuf,
    pub systemctl: PathBuf,
}

impl Runner {
    /// The real programs, or what the environment names.
    pub fn from_env() -> Self {
        let pick = |var: &str, default: &str| {
            std::env::var_os(var)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(default))
        };
        Self {
            systemd_run: pick("RUSTY_SYSTEMD_RUN", "systemd-run"),
            systemctl: pick("RUSTY_SYSTEMCTL", "systemctl"),
        }
    }

    /// Whether `systemd-run` can be found at all; without it no host can start.
    pub fn available(&self) -> bool {
        if self.systemd_run.components().count() > 1 {
            return self.systemd_run.is_file();
        }
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths).any(|dir| dir.join(&self.systemd_run).is_file())
        })
    }
}

/// What a new session is made of.
#[derive(Debug, Clone)]
pub struct NewSession {
    pub cwd: String,
    pub title: String,
    pub page: Option<String>,
    pub options: SpawnOptions,
}

/// The environment the host inherits from the caller: `PATH` always (the app completes
/// it with `~/.local/bin`), and the Rusty overrides when they are set.
pub fn passthrough_env() -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Ok(path) = std::env::var("PATH") {
        env.push(("PATH".to_string(), path));
    }
    for var in [
        "RUSTY_CLAUDE_BIN",
        "RUSTY_AGENT_STATE_DIR",
        "RUSTY_AGENT_RUN_DIR",
        "RUSTY_MCP_URL",
        "RUSTY_NOTIFY_SEND",
    ] {
        if let Ok(value) = std::env::var(var) {
            if !value.is_empty() {
                env.push((var.to_string(), value));
            }
        }
    }
    env
}

/// The `systemd-run` arguments for a session's host: its own unit, stopped by a SIGTERM
/// to the host alone, restarted after a crash, unloaded when it fails, the caller's
/// environment passed through, the caller's own binary as the host.
pub fn systemd_run_args(
    id: &SessionId,
    title: &str,
    exe: &Path,
    env: &[(String, String)],
) -> Vec<String> {
    let mut args = vec![
        "--user".to_string(),
        "--quiet".to_string(),
        "--collect".to_string(),
        format!("--unit={}", unit_name(id)),
        format!("--description=Rusty agent: {}", title.replace('\n', " ")),
        "--service-type=exec".to_string(),
        "--property=KillMode=mixed".to_string(),
        "--property=TimeoutStopSec=20".to_string(),
        "--property=Restart=on-failure".to_string(),
        "--property=RestartSec=1".to_string(),
        "--property=SyslogIdentifier=rusty-agent".to_string(),
    ];
    for (name, value) in env {
        args.push(format!("--setenv={name}={value}"));
    }
    args.push("--".to_string());
    args.push(exe.to_string_lossy().into_owned());
    args.push("agent".to_string());
    args.push("host".to_string());
    args.push("--id".to_string());
    args.push(id.to_string());
    args
}

/// Run `systemd-run` for the session, then wait for its host to answer.
fn launch(runner: &Runner, id: &SessionId, title: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("the app's own path: {e}"))?;
    let args = systemd_run_args(id, title, &exe, &passthrough_env());
    let out = Command::new(&runner.systemd_run)
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", runner.systemd_run.display()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("systemd-run exited {}", out.status)
        } else {
            err
        });
    }
    wait_for_hello(id)
}

/// Connect to the session's socket and read the host's first line.
fn wait_for_hello(id: &SessionId) -> Result<(), String> {
    let socket = paths::socket_path(id);
    let mut connection = Connection::connect_with_retry(&socket, Duration::from_secs(5))
        .map_err(|_| "the host did not answer on its socket".to_string())?;
    match connection.read_line_timeout(Duration::from_secs(5)) {
        Ok(Some(line)) if line.contains("\"hello\"") => {
            connection.close();
            Ok(())
        }
        Ok(_) => Err("the host answered with something other than hello".into()),
        Err(e) => Err(format!("reading the host's hello: {e}")),
    }
}

/// Start a new session: mint its id, write its entry, run its unit, wait for the host.
/// A launch that fails leaves no entry behind.
pub fn start_new(runner: &Runner, request: NewSession) -> Result<SessionId, String> {
    request.options.validate()?;
    let id = SessionId::new();
    // The host would refuse this in its own log, where a caller never looks.
    paths::check_socket_path(&paths::socket_path(&id))?;
    let state_dir = paths::state_dir();
    let entry = Entry::new(
        &id,
        &request.title,
        request.page.clone(),
        &request.cwd,
        request.options.clone(),
    );
    registry::write_entry_in(&state_dir, &entry).map_err(|e| format!("writing the entry: {e}"))?;
    if let Err(e) = launch(runner, &id, &request.title) {
        let _ = registry::remove_in(&state_dir, &id);
        return Err(e);
    }
    Ok(id)
}

/// Run a stopped session's host again; refused when one already answers.
pub fn start_existing(runner: &Runner, id: &SessionId) -> Result<(), String> {
    paths::check_socket_path(&paths::socket_path(id))?;
    let entry = registry::read_entry(id).map_err(|e| format!("no session {id}: {e}"))?;
    if registry::alive(id) {
        return Err(format!("session {id} is already running"));
    }
    launch(runner, id, &entry.title)
}

/// Stop a session's host: ask it over the socket and wait for the end; when the socket
/// does not answer, stop the unit.
pub fn stop(runner: &Runner, id: &SessionId) -> Result<(), String> {
    let socket = paths::socket_path(id);
    if let Ok(mut connection) = Connection::connect(&socket) {
        connection
            .send(&ClientMsg::Stop)
            .map_err(|e| format!("asking the host to stop: {e}"))?;
        // Read until the host closes the connection, which it does last of all.
        while let Ok(Some(_)) = connection.read_line_timeout(Duration::from_secs(25)) {}
        return Ok(());
    }
    let out = Command::new(&runner.systemctl)
        .args(["--user", "stop", &format!("{}.service", unit_name(id))])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", runner.systemctl.display()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemd_run_args_are_exact() {
        let id = SessionId::parse("8b3f0c2e-5d1a-4e7b-9c3d-2f6a1b8e4d70").unwrap();
        let env = vec![
            (
                "PATH".to_string(),
                "/home/x/.local/bin:/usr/bin".to_string(),
            ),
            ("RUSTY_CLAUDE_BIN".to_string(), "/opt/claude".to_string()),
        ];
        let args = systemd_run_args(&id, "Orbit\nnotes", Path::new("/usr/bin/rusty"), &env);
        assert_eq!(
            args,
            vec![
                "--user",
                "--quiet",
                "--collect",
                "--unit=rusty-agent-8b3f0c2e-5d1a-4e7b-9c3d-2f6a1b8e4d70",
                "--description=Rusty agent: Orbit notes",
                "--service-type=exec",
                "--property=KillMode=mixed",
                "--property=TimeoutStopSec=20",
                "--property=Restart=on-failure",
                "--property=RestartSec=1",
                "--property=SyslogIdentifier=rusty-agent",
                "--setenv=PATH=/home/x/.local/bin:/usr/bin",
                "--setenv=RUSTY_CLAUDE_BIN=/opt/claude",
                "--",
                "/usr/bin/rusty",
                "agent",
                "host",
                "--id",
                "8b3f0c2e-5d1a-4e7b-9c3d-2f6a1b8e4d70",
            ]
        );
    }

    #[test]
    fn the_runner_reads_its_overrides() {
        let runner = Runner {
            systemd_run: PathBuf::from("/nonexistent/systemd-run"),
            systemctl: PathBuf::from("systemctl"),
        };
        assert!(!runner.available());
        let real = Runner {
            systemd_run: PathBuf::from("sh"),
            systemctl: PathBuf::from("systemctl"),
        };
        assert!(real.available(), "sh is on every PATH");
    }

    /// A run directory so long that the kernel would cut the socket's path is refused
    /// before a unit is asked for, not in a log the caller never reads.
    #[test]
    fn a_socket_path_too_long_is_refused_up_front() {
        let long = std::path::PathBuf::from("/tmp").join("x".repeat(120));
        let err = paths::check_socket_path(&long.join("s.sock")).unwrap_err();
        assert!(err.contains("too long") && err.contains("107"), "{err}");
        assert!(paths::check_socket_path(std::path::Path::new(
            "/run/user/1000/rusty/agents/8b3f0c2e-5d1a-4e7b-9c3d-2f6a1b8e4d70.sock"
        ))
        .is_ok());
    }

    #[test]
    fn the_passthrough_carries_path_and_the_overrides_only() {
        let env = passthrough_env();
        assert!(env.iter().any(|(k, _)| k == "PATH"));
        assert!(env
            .iter()
            .all(|(k, _)| k == "PATH" || k.starts_with("RUSTY_")));
    }
}
