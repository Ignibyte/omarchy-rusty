//! `rusty-cli hooks`: the two Claude Code hooks of the brain loop (TICKET-018), shipped
//! inside the binary, written to `~/.rusty/hooks/` and wired into
//! `~/.claude/settings.json` idempotently. Every other entry in the settings stays.

use std::path::{Path, PathBuf};

/// The PreToolUse hook: consult the brain before the first write.
pub const ASK_HOOK_NAME: &str = "brain-ask-before-write.sh";
/// The Stop hook: record the decision before the session ends.
pub const STOP_HOOK_NAME: &str = "brain-decide-before-stop.sh";
/// The scripts, as shipped.
pub const ASK_HOOK: &str = include_str!("../hooks/brain-ask-before-write.sh");
pub const STOP_HOOK: &str = include_str!("../hooks/brain-decide-before-stop.sh");
/// The tools the write hook watches.
pub const WRITE_MATCHER: &str = "Edit|Write|MultiEdit|NotebookEdit";

/// Where the scripts are written.
pub fn hooks_dir(home: &Path) -> PathBuf {
    home.join(".rusty").join("hooks")
}

/// Claude Code's user settings.
pub fn settings_path(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

/// What an install or uninstall did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub scripts_written: usize,
    pub entries_added: usize,
    pub entries_removed: usize,
    pub scripts_removed: usize,
}

/// Where things stand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub ask_script: bool,
    pub stop_script: bool,
    pub ask_wired: bool,
    pub stop_wired: bool,
}

fn command_for(dir: &Path, name: &str) -> String {
    format!("bash \"{}\"", dir.join(name).display())
}

fn read_settings(path: &Path) -> Result<serde_json::Value, String> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} does not parse: {e}", path.display()))?;
    if !value.is_object() {
        return Err(format!("{} is not a JSON object", path.display()));
    }
    Ok(value)
}

fn write_settings(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, format!("{text}\n")).map_err(|e| format!("write {}: {e}", path.display()))
}

/// The entries of one hook event, created when missing.
fn entries_mut<'a>(
    settings: &'a mut serde_json::Value,
    event: &str,
) -> Result<&'a mut Vec<serde_json::Value>, String> {
    let root = settings
        .as_object_mut()
        .ok_or_else(|| "settings are not an object".to_string())?;
    let hooks = root.entry("hooks").or_insert_with(|| serde_json::json!({}));
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| "\"hooks\" is not an object".to_string())?;
    let entries = hooks.entry(event).or_insert_with(|| serde_json::json!([]));
    entries
        .as_array_mut()
        .ok_or_else(|| format!("\"hooks\".\"{event}\" is not an array"))
}

/// Whether a hook command runs one of Rusty's scripts, from this home or another one:
/// Rusty writes them to `<home>/.rusty/hooks/<name>`.
fn is_ours(command: &str, name: &str) -> bool {
    command.contains(&format!(".rusty/hooks/{name}"))
}

/// Remove the hook commands that run `name` but are not `keep` from one event's entries.
/// An entry loses only those hook objects, and goes only when none is left in it, so a
/// user's own hook that shares an entry with Rusty's stays. Returns how many went.
fn remove_ours(entries: &mut Vec<serde_json::Value>, name: &str, keep: Option<&str>) -> usize {
    let mut removed = 0;
    entries.retain_mut(|entry| {
        let Some(hooks) = entry.get_mut("hooks").and_then(|h| h.as_array_mut()) else {
            return true;
        };
        let before = hooks.len();
        hooks.retain(|h| {
            let command = h["command"].as_str().unwrap_or("");
            !is_ours(command, name) || Some(command) == keep
        });
        let gone = before - hooks.len();
        removed += gone;
        gone == 0 || !hooks.is_empty()
    });
    removed
}

/// Whether one event's entries hold exactly `command`.
fn has_command(entries: &[serde_json::Value], command: &str) -> bool {
    entries.iter().any(|entry| {
        entry["hooks"]
            .as_array()
            .is_some_and(|hooks| hooks.iter().any(|h| h["command"].as_str() == Some(command)))
    })
}

/// Write the scripts and wire them. A stale command (another home's path) is replaced;
/// a second run changes nothing, and the settings file is written only when it changes.
pub fn install(home: &Path) -> Result<Report, String> {
    let dir = hooks_dir(home);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = settings_path(home);
    // Read the settings first, so a file that does not parse stops the install before
    // anything is written.
    let original = read_settings(&path)?;
    let mut report = Report::default();
    for (name, body) in [(ASK_HOOK_NAME, ASK_HOOK), (STOP_HOOK_NAME, STOP_HOOK)] {
        let script = dir.join(name);
        std::fs::write(&script, body).map_err(|e| format!("write {}: {e}", script.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("chmod {}: {e}", script.display()))?;
        }
        report.scripts_written += 1;
    }
    let mut settings = original.clone();
    for (event, name, matcher) in [
        ("PreToolUse", ASK_HOOK_NAME, Some(WRITE_MATCHER)),
        ("Stop", STOP_HOOK_NAME, None),
    ] {
        let command = command_for(&dir, name);
        let entries = entries_mut(&mut settings, event)?;
        remove_ours(entries, name, Some(&command));
        if !has_command(entries, &command) {
            let hook = serde_json::json!({ "type": "command", "command": command, "timeout": 10 });
            entries.push(match matcher {
                Some(m) => serde_json::json!({ "matcher": m, "hooks": [hook] }),
                None => serde_json::json!({ "hooks": [hook] }),
            });
            report.entries_added += 1;
        }
    }
    if settings != original {
        write_settings(&path, &settings)?;
    }
    Ok(report)
}

/// Remove Rusty's hook commands and the scripts; every other hook and setting stays, and
/// the settings file is written only when something was removed from it.
pub fn uninstall(home: &Path) -> Result<Report, String> {
    let mut report = Report::default();
    let path = settings_path(home);
    if path.exists() {
        let mut settings = read_settings(&path)?;
        for (event, name) in [("PreToolUse", ASK_HOOK_NAME), ("Stop", STOP_HOOK_NAME)] {
            if let Some(entries) = settings
                .get_mut("hooks")
                .and_then(|h| h.get_mut(event))
                .and_then(|e| e.as_array_mut())
            {
                report.entries_removed += remove_ours(entries, name, None);
            }
        }
        if report.entries_removed > 0 {
            write_settings(&path, &settings)?;
        }
    }
    for name in [ASK_HOOK_NAME, STOP_HOOK_NAME] {
        let script = hooks_dir(home).join(name);
        if script.exists() {
            std::fs::remove_file(&script)
                .map_err(|e| format!("remove {}: {e}", script.display()))?;
            report.scripts_removed += 1;
        }
    }
    Ok(report)
}

/// Whether the scripts exist and the settings run them from this home.
pub fn status(home: &Path) -> Status {
    let dir = hooks_dir(home);
    let settings = read_settings(&settings_path(home)).unwrap_or(serde_json::json!({}));
    let wired = |event: &str, name: &str| {
        settings["hooks"][event]
            .as_array()
            .is_some_and(|entries| has_command(entries, &command_for(&dir, name)))
    };
    Status {
        ask_script: dir.join(ASK_HOOK_NAME).is_file(),
        stop_script: dir.join(STOP_HOOK_NAME).is_file(),
        ask_wired: wired("PreToolUse", ASK_HOOK_NAME),
        stop_wired: wired("Stop", STOP_HOOK_NAME),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rusty_hooks_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn has_jq() -> bool {
        Command::new("jq")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    #[test]
    fn install_is_idempotent_and_keeps_every_other_entry() {
        let home = home("install");
        let settings = settings_path(&home);
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(
            &settings,
            r#"{"permissions":{"allow":["Bash(ls:*)"]},"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"bash /elsewhere/other.sh"}]}]}}"#,
        )
        .unwrap();
        let first = install(&home).unwrap();
        assert_eq!(first.scripts_written, 2);
        assert_eq!(first.entries_added, 2);
        let second = install(&home).unwrap();
        assert_eq!(second.entries_added, 0, "a second run adds nothing");
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(value["permissions"]["allow"][0], "Bash(ls:*)");
        assert_eq!(value["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
        assert_eq!(
            value["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "bash /elsewhere/other.sh"
        );
        assert_eq!(value["hooks"]["Stop"].as_array().unwrap().len(), 1);
        let s = status(&home);
        assert!(
            s.ask_script && s.stop_script && s.ask_wired && s.stop_wired,
            "{s:?}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(hooks_dir(&home).join(ASK_HOOK_NAME))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "executable");
        }
        let removed = uninstall(&home).unwrap();
        assert_eq!((removed.entries_removed, removed.scripts_removed), (2, 2));
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            value["hooks"]["PreToolUse"].as_array().unwrap().len(),
            1,
            "the other entry stays"
        );
        assert_eq!(value["permissions"]["allow"][0], "Bash(ls:*)");
        let s = status(&home);
        assert!(!s.ask_script && !s.stop_script && !s.ask_wired && !s.stop_wired);
        let _ = std::fs::remove_dir_all(home);
    }

    /// A user's own hook in the same entry as Rusty's survives an uninstall, and a file
    /// with nothing of Rusty's in it is not rewritten.
    #[test]
    fn uninstall_removes_only_rustys_commands() {
        let home = home("uninstall_shared");
        let settings = settings_path(&home);
        install(&home).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        value["hooks"]["PreToolUse"][0]["hooks"]
            .as_array_mut()
            .unwrap()
            .push(
                serde_json::json!({ "type": "command", "command": "bash /my/own/format-check.sh" }),
            );
        value["hooks"]["Stop"][0]["hooks"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({ "type": "command", "command": "notify-send done" }));
        std::fs::write(&settings, serde_json::to_string(&value).unwrap()).unwrap();
        let removed = uninstall(&home).unwrap();
        assert_eq!(removed.entries_removed, 2);
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            value["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "bash /my/own/format-check.sh"
        );
        assert_eq!(
            value["hooks"]["Stop"][0]["hooks"][0]["command"],
            "notify-send done"
        );

        let untouched = r#"{"theme":"dark","permissions":{"allow":[]}}"#;
        std::fs::write(&settings, untouched).unwrap();
        assert_eq!(uninstall(&home).unwrap().entries_removed, 0);
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), untouched);
        let _ = std::fs::remove_dir_all(home);
    }

    /// The settings file keeps its own key order through an install.
    #[test]
    fn install_keeps_the_settings_key_order() {
        let home = home("install_order");
        let settings = settings_path(&home);
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(
            &settings,
            r#"{"theme":"dark","permissions":{"allow":[]},"env":{"Z":"1","A":"2"}}"#,
        )
        .unwrap();
        install(&home).unwrap();
        let text = std::fs::read_to_string(&settings).unwrap();
        let at = |key: &str| text.find(key).unwrap();
        assert!(
            at("\"theme\"") < at("\"permissions\"") && at("\"permissions\"") < at("\"env\""),
            "{text}"
        );
        assert!(at("\"Z\"") < at("\"A\""), "{text}");
        let _ = std::fs::remove_dir_all(home);
    }

    /// A command left by another home's install is replaced, not counted as wired, and a
    /// second install leaves the file's bytes alone.
    #[test]
    fn install_replaces_a_stale_command_and_then_writes_nothing() {
        let home = home("install_stale");
        let settings = settings_path(&home);
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(
            &settings,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"bash \"/srv/olduser/.rusty/hooks/brain-decide-before-stop.sh\""}]}]}}"#,
        )
        .unwrap();
        assert!(!status(&home).stop_wired, "a stale path is not wired");
        let first = install(&home).unwrap();
        assert_eq!(first.entries_added, 2);
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        let stop = value["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1, "the stale entry went: {stop:?}");
        assert!(!stop[0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("olduser"));
        let s = status(&home);
        assert!(s.ask_wired && s.stop_wired, "{s:?}");
        let bytes = std::fs::read(&settings).unwrap();
        assert_eq!(install(&home).unwrap().entries_added, 0);
        assert_eq!(std::fs::read(&settings).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(home);
    }

    fn tool_use(name: &str, id: &str) -> String {
        serde_json::json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": name, "id": id, "input": {}}]}}).to_string()
    }

    fn tool_result(id: &str, is_error: bool) -> String {
        serde_json::json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": id, "is_error": is_error, "content": "x"}]}}).to_string()
    }

    fn text_line() -> String {
        serde_json::json!({"type": "assistant", "message": {"content": "plain text, not blocks"}})
            .to_string()
    }

    /// Run a hook with `input` on stdin; the exit code.
    fn run(script: &str, dir: &Path, input: &serde_json::Value) -> i32 {
        let path = dir.join("hook.sh");
        std::fs::write(&path, script).unwrap();
        let mut child = Command::new("bash")
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        out.status.code().unwrap_or(-1)
    }

    fn wired_cwd(dir: &Path) -> PathBuf {
        let cwd = dir.join("repo");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(
            cwd.join(".mcp.json"),
            r#"{"mcpServers":{"rusty":{"command":"rusty-mcp"}}}"#,
        )
        .unwrap();
        cwd
    }

    fn transcript(dir: &Path, name: &str, lines: &[String]) -> PathBuf {
        let path = dir.join(format!("{name}.jsonl"));
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        path
    }

    #[test]
    fn the_write_hook_blocks_until_a_brain_ask_succeeds() {
        if !has_jq() {
            eprintln!("jq is not installed; the hook corpus did not run");
            return;
        }
        let dir = home("write_hook");
        let cwd = wired_cwd(&dir);
        let input = |t: &Path| serde_json::json!({"cwd": cwd, "transcript_path": t, "tool_name": "Write", "tool_input": {"file_path": cwd.join("a.rs")}});
        let empty = transcript(&dir, "empty", &[text_line()]);
        assert_eq!(run(ASK_HOOK, &dir, &input(&empty)), 2, "no ask blocks");
        let errored = transcript(
            &dir,
            "errored",
            &[
                tool_use("mcp__rusty__brain_ask", "t1"),
                tool_result("t1", true),
            ],
        );
        assert_eq!(
            run(ASK_HOOK, &dir, &input(&errored)),
            2,
            "an errored ask blocks"
        );
        let asked = transcript(
            &dir,
            "asked",
            &[
                text_line(),
                tool_use("mcp__rusty__brain_ask", "t1"),
                tool_result("t1", false),
                tool_use("Write", "t2"),
            ],
        );
        assert_eq!(run(ASK_HOOK, &dir, &input(&asked)), 0, "an ask allows");
        let other = transcript(
            &dir,
            "other",
            &[
                tool_use("mcp__rusty__brain_search", "t1"),
                tool_result("t1", false),
            ],
        );
        assert_eq!(
            run(ASK_HOOK, &dir, &input(&other)),
            2,
            "only brain_ask counts"
        );
        let unwired = dir.join("plain");
        std::fs::create_dir_all(&unwired).unwrap();
        let out_of_scope = serde_json::json!({"cwd": unwired, "transcript_path": empty, "tool_input": {"file_path": "x"}});
        assert_eq!(
            run(ASK_HOOK, &dir, &out_of_scope),
            0,
            "no .mcp.json with rusty: allow"
        );
        let missing = serde_json::json!({"cwd": cwd, "transcript_path": dir.join("absent.jsonl"), "tool_input": {"file_path": "x"}});
        assert_eq!(
            run(ASK_HOOK, &dir, &missing),
            0,
            "an unreadable transcript fails open"
        );
        let elsewhere = serde_json::json!({"cwd": cwd, "transcript_path": empty, "tool_input": {"file_path": "/tmp/scratch/probe.py"}});
        assert_eq!(
            run(ASK_HOOK, &dir, &elsewhere),
            0,
            "a file outside the repository is not gated"
        );
        let relative = serde_json::json!({"cwd": cwd, "transcript_path": empty, "tool_input": {"file_path": "src/lib.rs"}});
        assert_eq!(
            run(ASK_HOOK, &dir, &relative),
            2,
            "a relative path is inside the repository"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_stop_hook_refuses_once_after_writes_without_a_record() {
        if !has_jq() {
            eprintln!("jq is not installed; the hook corpus did not run");
            return;
        }
        let dir = home("stop_hook");
        let cwd = wired_cwd(&dir);
        let input = |t: &Path, active: bool| serde_json::json!({"cwd": cwd, "transcript_path": t, "stop_hook_active": active});
        let wrote = transcript(
            &dir,
            "wrote",
            &[
                tool_use("mcp__rusty__brain_ask", "t1"),
                tool_result("t1", false),
                tool_use("Edit", "t2"),
                tool_result("t2", false),
            ],
        );
        assert_eq!(
            run(STOP_HOOK, &dir, &input(&wrote, false)),
            2,
            "writes without a record refuse"
        );
        assert_eq!(
            run(STOP_HOOK, &dir, &input(&wrote, true)),
            0,
            "the second attempt passes"
        );
        let decided = transcript(
            &dir,
            "decided",
            &[
                tool_use("Write", "t2"),
                tool_result("t2", false),
                tool_use("mcp__rusty__brain_decide", "t3"),
                tool_result("t3", false),
            ],
        );
        assert_eq!(
            run(STOP_HOOK, &dir, &input(&decided, false)),
            0,
            "a decision allows"
        );
        let honest = transcript(
            &dir,
            "honest",
            &[
                tool_use("Write", "t2"),
                tool_use("mcp__rusty__brain_no_decision", "t3"),
                tool_result("t3", false),
            ],
        );
        assert_eq!(
            run(STOP_HOOK, &dir, &input(&honest, false)),
            0,
            "no decision, said so, allows"
        );
        let failed = transcript(
            &dir,
            "failed",
            &[
                tool_use("Write", "t2"),
                tool_use("mcp__rusty__brain_decide", "t3"),
                tool_result("t3", true),
            ],
        );
        assert_eq!(
            run(STOP_HOOK, &dir, &input(&failed, false)),
            2,
            "an errored record does not count"
        );
        let read_only = transcript(
            &dir,
            "read_only",
            &[
                tool_use("Read", "t1"),
                tool_use("mcp__rusty__brain_search", "t2"),
            ],
        );
        assert_eq!(
            run(STOP_HOOK, &dir, &input(&read_only, false)),
            0,
            "no writes, nothing to record"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
