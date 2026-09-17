//! How the host starts Claude Code: the options a session was created with, the
//! command line they become, and where the `claude` binary is.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use super::wire::{mcp_config, PERMISSION_MODES, READ_TOOLS};
use super::SessionId;

/// What a session was created with; the host reads it from the registry entry at every
/// spawn, so a session keeps its shape across hosts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpawnOptions {
    /// The permission mode the process starts in.
    pub permission_mode: String,
    /// The model, or Claude Code's default.
    #[serde(default)]
    pub model: Option<String>,
    /// Only the servers named here (`--strict-mcp-config`): the note pane's shape.
    /// Otherwise the project's own servers load too, with Rusty's added.
    pub strict_mcp: bool,
    /// Tools pre-allowed by name (the `mcp__rusty__` prefix included).
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Appended to Claude Code's own system prompt; empty for none.
    #[serde(default)]
    pub system_prompt: String,
    /// Rusty's server, added through `--mcp-config`.
    pub mcp_url: String,
    /// A display name for the session (`--name`).
    #[serde(default)]
    pub name: Option<String>,
    /// Stop the process after this many seconds without a turn; 0 keeps it.
    #[serde(default)]
    pub idle_timeout_secs: u64,
    /// A Claude session to continue when the session is created (a page whose id is
    /// from before the host, for one); none for a fresh conversation.
    #[serde(default)]
    pub resume: Option<String>,
}

impl SpawnOptions {
    /// Refuse a permission mode Claude Code does not know, before it becomes a unit.
    pub fn validate(&self) -> Result<(), String> {
        if PERMISSION_MODES.contains(&self.permission_mode.as_str()) {
            Ok(())
        } else {
            Err(format!(
                "unknown permission mode '{}': one of {}",
                self.permission_mode,
                PERMISSION_MODES.join(", ")
            ))
        }
    }
}

/// Rusty's read-only tools, fully qualified, for `allowed_tools`.
pub fn read_tools() -> Vec<String> {
    READ_TOOLS
        .iter()
        .map(|t| format!("mcp__rusty__{t}"))
        .collect()
}

/// Which conversation the process opens: a new one under the session's own id, so the
/// Rusty id is the Claude session id, or an existing one resumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionArg {
    New(SessionId),
    Resume(String),
}

/// The arguments of the process: print mode over stream-json both ways with partial
/// messages, permissions asked on stdout, Rusty's server through `--mcp-config` (alone
/// under `strict_mcp`), the pre-allowed tools, the mode, the model, the name, the system
/// prompt, and the conversation to open.
pub fn build_args(opts: &SpawnOptions, session: &SessionArg) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--include-partial-messages",
        "--verbose",
        "--permission-prompt-tool",
        "stdio",
        "--permission-mode",
        opts.permission_mode.as_str(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if opts.strict_mcp {
        args.push("--strict-mcp-config".to_string());
    }
    args.push("--mcp-config".to_string());
    args.push(mcp_config(&opts.mcp_url));
    if !opts.allowed_tools.is_empty() {
        args.push("--allowedTools".to_string());
        args.push(opts.allowed_tools.join(","));
    }
    if let Some(model) = opts.model.as_deref().filter(|m| !m.trim().is_empty()) {
        args.push("--model".to_string());
        args.push(model.trim().to_string());
    }
    if let Some(name) = opts.name.as_deref().filter(|n| !n.trim().is_empty()) {
        args.push("--name".to_string());
        args.push(name.trim().to_string());
    }
    if !opts.system_prompt.is_empty() {
        args.push("--append-system-prompt".to_string());
        args.push(opts.system_prompt.clone());
    }
    match session {
        SessionArg::New(id) => {
            args.push("--session-id".to_string());
            args.push(id.to_string());
        }
        SessionArg::Resume(id) => {
            args.push("--resume".to_string());
            args.push(id.trim().to_string());
        }
    }
    args
}

/// Where `claude` is: `RUSTY_CLAUDE_BIN` when set, else the first on `PATH`, else what
/// a login shell knows (a user service's `PATH` may not carry the shims a shell adds).
pub fn claude_binary() -> Option<PathBuf> {
    if let Some(given) = std::env::var_os("RUSTY_CLAUDE_BIN").filter(|p| !p.is_empty()) {
        let given = PathBuf::from(given);
        return given.is_file().then_some(given);
    }
    if let Some(found) = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("claude"))
            .find(|p| p.is_file())
    }) {
        return Some(found);
    }
    let out = Command::new("bash")
        .args(["-lc", "command -v claude"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !path.is_empty()).then(|| PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane_options() -> SpawnOptions {
        SpawnOptions {
            permission_mode: "default".into(),
            model: None,
            strict_mcp: true,
            allowed_tools: read_tools(),
            system_prompt: "The page is x.".into(),
            mcp_url: "http://127.0.0.1:4174/mcp".into(),
            name: Some("Orbit".into()),
            idle_timeout_secs: 600,
            resume: None,
        }
    }

    #[test]
    fn build_args_carry_the_wire() {
        let id = SessionId::parse("8b3f0c2e-5d1a-4e7b-9c3d-2f6a1b8e4d70").unwrap();
        let args = build_args(&pane_options(), &SessionArg::New(id.clone()));
        let joined = args.join(" ");
        for flag in [
            "-p",
            "--input-format stream-json",
            "--output-format stream-json",
            "--include-partial-messages",
            "--verbose",
            "--permission-prompt-tool stdio",
            "--permission-mode default",
            "--strict-mcp-config",
            "--name Orbit",
            "--append-system-prompt The page is x.",
            "--session-id 8b3f0c2e-5d1a-4e7b-9c3d-2f6a1b8e4d70",
        ] {
            assert!(joined.contains(flag), "{flag} in {joined}");
        }
        assert!(!joined.contains("--resume"));
        assert!(!joined.contains("--model"));
        assert!(!joined.contains("--dangerously-skip-permissions"));
        let config = args
            .iter()
            .position(|a| a == "--mcp-config")
            .map(|i| args[i + 1].clone())
            .unwrap();
        let config: serde_json::Value = serde_json::from_str(&config).unwrap();
        assert_eq!(config["mcpServers"]["rusty"]["type"], "http");
        assert_eq!(
            config["mcpServers"]["rusty"]["url"],
            "http://127.0.0.1:4174/mcp"
        );
        let allowed = args
            .iter()
            .position(|a| a == "--allowedTools")
            .map(|i| args[i + 1].clone())
            .unwrap();
        assert!(allowed.contains("mcp__rusty__brain_read_page"));
        assert!(!allowed.contains("secret_reveal") && !allowed.contains("brain_delete"));
    }

    #[test]
    fn a_general_session_loads_the_project_servers_and_resumes() {
        let opts = SpawnOptions {
            permission_mode: "acceptEdits".into(),
            model: Some(" sonnet ".into()),
            strict_mcp: false,
            allowed_tools: Vec::new(),
            system_prompt: String::new(),
            mcp_url: "http://x".into(),
            name: None,
            idle_timeout_secs: 0,
            resume: None,
        };
        let args = build_args(&opts, &SessionArg::Resume(" abc-123 ".into()));
        let joined = args.join(" ");
        assert!(joined.ends_with("--resume abc-123"), "{joined}");
        assert!(joined.contains("--model sonnet"));
        assert!(joined.contains("--permission-mode acceptEdits"));
        assert!(!joined.contains("--strict-mcp-config"));
        assert!(!joined.contains("--allowedTools"));
        assert!(!joined.contains("--append-system-prompt"));
        assert!(!joined.contains("--name"));
        assert!(joined.contains("--mcp-config"));
    }

    /// A mode is named and nothing else: Rusty never adds a flag that skips the checks
    /// on the user's behalf.
    #[test]
    fn a_mode_is_passed_as_a_mode_and_no_more() {
        let mut opts = pane_options();
        opts.permission_mode = "bypassPermissions".into();
        let joined = build_args(&opts, &SessionArg::Resume("s".into())).join(" ");
        assert!(joined.contains("--permission-mode bypassPermissions"));
        assert!(!joined.contains("--dangerously-skip-permissions"));
    }

    #[test]
    fn options_refuse_an_unknown_mode() {
        let mut opts = pane_options();
        assert!(opts.validate().is_ok());
        opts.permission_mode = "bogus".into();
        let err = opts.validate().unwrap_err();
        assert!(
            err.contains("bogus") && err.contains("acceptEdits"),
            "{err}"
        );
    }

    #[test]
    fn options_round_trip_with_defaults() {
        let json = r#"{"permission_mode":"default","strict_mcp":true,"mcp_url":"http://x"}"#;
        let opts: SpawnOptions = serde_json::from_str(json).unwrap();
        assert_eq!(opts.idle_timeout_secs, 0);
        assert!(opts.allowed_tools.is_empty());
        assert!(opts.model.is_none() && opts.name.is_none() && opts.resume.is_none());
        let back: SpawnOptions =
            serde_json::from_str(&serde_json::to_string(&pane_options()).unwrap()).unwrap();
        assert_eq!(back, pane_options());
    }
}
