//! The `rusty` binary run as a user would, with stand-ins: a logging `systemctl` first on
//! `PATH` and a back-end probe pointed at a closed port, so nothing on the machine's own
//! user manager or back end is touched (TICKET-038). The window these tests once looked
//! for retired with TICKET-053.

use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rusty_cmd_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    dir
}

/// A `systemctl` that appends its arguments to `log` and answers `inactive`.
fn stand_in_systemctl(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let log = dir.join("systemctl.log");
    let script = dir.join("bin/systemctl");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\ncase \"$*\" in *is-active*) echo inactive; exit 3;; esac\nexit 0\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    log
}

fn rusty(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rusty"))
        .args(args)
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", dir.join("bin").display()),
        )
        .env("HOME", dir)
        .env("RUSTY_MCP_ADDR", "127.0.0.1:1")
        .output()
        .unwrap()
}

#[test]
fn start_starts_the_back_end_alone() {
    let dir = scratch("start");
    let log = stand_in_systemctl(&dir);
    let out = rusty(&dir, &["session", "start"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("rusty-mcp.service"), "{stdout}");
    assert!(
        stdout.contains("not answering on http://127.0.0.1:4174/mcp"),
        "{stdout}"
    );
    let calls = std::fs::read_to_string(&log).unwrap();
    let starts: Vec<&str> = calls.lines().filter(|l| l.contains(" start ")).collect();
    assert_eq!(starts, ["--user start rusty-mcp.service"], "{calls}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn status_reports_the_back_end_alone() {
    let dir = scratch("status");
    let log = stand_in_systemctl(&dir);
    let out = rusty(&dir, &["session", "status"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("rusty-mcp.service  inactive"), "{stdout}");
    assert!(stdout.contains("back end"), "{stdout}");
    assert!(!stdout.contains("app"), "{stdout}");
    let calls = std::fs::read_to_string(&log).unwrap();
    assert_eq!(
        calls.trim(),
        "--user is-active rusty-mcp.service",
        "{calls}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn rusty_alone_prints_the_usage() {
    let dir = scratch("alone");
    let log = stand_in_systemctl(&dir);
    let out = rusty(&dir, &[]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("usage: rusty"), "{stdout}");
    assert!(!stdout.contains("window"), "{stdout}");
    assert!(!log.exists(), "rusty alone called systemctl");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn retired_verbs_and_dash_arguments_are_errors() {
    let dir = scratch("retired");
    let log = stand_in_systemctl(&dir);
    for args in [
        &["session", "stop"][..],
        &["session", "run"][..],
        &["--some-qt-flag"][..],
    ] {
        let out = rusty(&dir, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("usage: rusty"), "{args:?}: {stderr}");
    }
    assert!(!log.exists(), "a retired verb called systemctl");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unknown_word_prints_the_usage_and_exits_2() {
    let dir = scratch("unknown");
    let out = rusty(&dir, &["no-such-command"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: rusty"));
    let _ = std::fs::remove_dir_all(dir);
}
