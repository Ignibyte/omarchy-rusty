//! `rusty <noun> <verb>`: the commands the app binary answers before Qt starts
//! (TICKET-029). The first noun is `session`, the way into a running Rusty: the back
//! end, then the app, each under its user unit (`omarchy/rusty-mcp.service` and
//! `omarchy/rusty-app.service`). Built-in nouns come before store scripts (TICKET-010);
//! an argument that starts with a dash belongs to Qt and is never read here.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// The back end's unit.
pub const MCP_UNIT: &str = "rusty-mcp.service";
/// The app's unit.
pub const APP_UNIT: &str = "rusty-app.service";
/// Where the back end listens for the app (see `omarchy/rusty-mcp.service`).
const MCP_ADDR: &str = "127.0.0.1:4174";
/// The same endpoint as the docs name it.
const MCP_URL: &str = "http://127.0.0.1:4174/mcp";

/// The session variables the app needs. uwsm imports them into the user manager at
/// login; a compositor started another way may not, so `start` copies them from its own
/// environment when the manager holds no display at all.
const DISPLAY_VARS: [&str; 5] = [
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "HYPRLAND_INSTANCE_SIGNATURE",
];

/// The MCP `initialize` the probe posts; the back end answers 200 when it is serving.
const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"rusty session","version":"0"}}}"#;

/// What `rusty help` prints.
pub const USAGE: &str = "\
usage: rusty [<command> [args...]]
  rusty                      the window
  rusty session start        the back end, then the app unit; safe to run again
  rusty session stop         stop the app unit; the back end keeps serving
  rusty session status       both units, the port, the app's processes
  rusty session run          what rusty-app.service runs: PATH completed, then the window
  rusty agent <verb> ...     Claude Code sessions that outlive the app: start, stop, list,
                             attach, rm (rusty agent help)
  rusty <script> [args...]   a store script, a *.sh beside a skill (rusty-cli scripts list)
  rusty help                 this text
An argument that starts with a dash goes to Qt.";

/// A `session` verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Start,
    Stop,
    Status,
    Run,
}

/// What the command line asks for, decided before Qt starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Open the window; every argument goes to Qt.
    Window,
    /// Print the usage and exit 0.
    Help,
    /// One of the session verbs.
    Session(Verb),
    /// `rusty session` alone, or with a verb that does not exist.
    SessionUsage(Option<String>),
    /// `rusty agent ...`, with everything after the noun.
    Agent(Vec<String>),
    /// A store script by name, with its arguments.
    Script(String, Vec<String>),
    /// A bare word that is neither a noun nor a script.
    Unknown(String),
}

/// Read the command line: nouns first, store scripts second, anything else an error.
pub fn parse(args: &[String], script_exists: impl Fn(&str) -> bool) -> Request {
    let Some(first) = args.first() else {
        return Request::Window;
    };
    match first.as_str() {
        "help" | "--help" | "-h" => Request::Help,
        flag if flag.starts_with('-') => Request::Window,
        "session" => match args.get(1).map(String::as_str) {
            Some("start") => Request::Session(Verb::Start),
            Some("stop") => Request::Session(Verb::Stop),
            Some("status") => Request::Session(Verb::Status),
            Some("run") => Request::Session(Verb::Run),
            other => Request::SessionUsage(other.map(str::to_string)),
        },
        "agent" => Request::Agent(args[1..].to_vec()),
        name if script_exists(name) => Request::Script(name.to_string(), args[1..].to_vec()),
        name => Request::Unknown(name.to_string()),
    }
}

/// `rusty session start`: the back end, then the app unit unless it is running already or
/// a `rusty` outside the unit is; then the status. Returns the exit status.
pub fn start() -> i32 {
    let code = run_systemctl(&["start", MCP_UNIT]);
    if code != 0 {
        return code;
    }
    if !unit_active(APP_UNIT) {
        let others = unmanaged(&rusty_pids(), unit_main_pid(APP_UNIT));
        if others.is_empty() {
            let manager = manager_environment();
            if !manager.contains("WAYLAND_DISPLAY") && !manager.contains("DISPLAY") {
                let missing = missing_display_vars(&manager, |name| {
                    std::env::var_os(name).is_some_and(|v| !v.is_empty())
                });
                if !missing.is_empty() && import_environment(&missing) {
                    println!("imported into the user manager: {}", missing.join(" "));
                }
            }
            let code = run_systemctl(&["start", APP_UNIT]);
            if code != 0 {
                return code;
            }
        } else {
            println!(
                "rusty is running outside {APP_UNIT} (pid {}); quit it, then run: rusty session start",
                join(&others)
            );
        }
    }
    status()
}

/// `rusty session stop`: the app unit down, the back end kept.
pub fn stop() -> i32 {
    let code = run_systemctl(&["stop", APP_UNIT]);
    if code == 0 {
        println!("{APP_UNIT} stopped; {MCP_UNIT} keeps serving");
    }
    code
}

/// `rusty session status`: both units, the port, the app's processes.
pub fn status() -> i32 {
    for unit in [MCP_UNIT, APP_UNIT] {
        println!("{unit:<18} {}", unit_state(unit));
    }
    let word = if back_end_answers() {
        "answering"
    } else {
        "not answering"
    };
    println!("{:<18} {word} on {MCP_URL}", "back end");
    let pids = rusty_pids();
    let shown = if pids.is_empty() {
        "none".to_string()
    } else {
        join(&pids)
    };
    println!("{:<18} {shown}", "app process");
    0
}

/// `rusty session run`, the unit's command: `PATH` completed with `~/.local/bin` and
/// `~/.cargo/bin`, where the agent CLIs tend to live, so the terminals find them. The
/// window opens in this process afterwards.
pub fn complete_path() {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let current = std::env::var("PATH").unwrap_or_default();
    let cargo_bin = home.join(".cargo/bin").is_dir();
    std::env::set_var("PATH", completed_path(&current, &home, cargo_bin));
}

/// `path` with `home/.local/bin` in front and, when `cargo_bin` says the directory
/// exists, `home/.cargo/bin` behind; each once.
pub fn completed_path(path: &str, home: &Path, cargo_bin: bool) -> String {
    let mut parts: Vec<String> = path
        .split(':')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    let local = home.join(".local/bin").to_string_lossy().into_owned();
    if !parts.contains(&local) {
        parts.insert(0, local);
    }
    if cargo_bin {
        let cargo = home.join(".cargo/bin").to_string_lossy().into_owned();
        if !parts.contains(&cargo) {
            parts.push(cargo);
        }
    }
    parts.join(":")
}

/// The display variables set in this process that the user manager lacks.
pub fn missing_display_vars(
    manager: &BTreeSet<String>,
    set_here: impl Fn(&str) -> bool,
) -> Vec<String> {
    DISPLAY_VARS
        .iter()
        .filter(|name| set_here(name) && !manager.contains(**name))
        .map(|name| name.to_string())
        .collect()
}

/// The `rusty` processes that are not the unit's own: started from a terminal or a
/// launcher. Starting the unit beside one would open a second window.
pub fn unmanaged(pids: &[u32], unit_main: Option<u32>) -> Vec<u32> {
    pids.iter()
        .copied()
        .filter(|pid| Some(*pid) != unit_main)
        .collect()
}

/// Whether an HTTP response head says 200.
pub fn answers_ok(head: &str) -> bool {
    let Some(line) = head.lines().next() else {
        return false;
    };
    let mut words = line.split(' ');
    matches!(
        (words.next(), words.next()),
        (Some(version), Some("200")) if version.starts_with("HTTP/1.")
    )
}

/// Every process named `rusty` other than this one, from `/proc`, in pid order.
fn rusty_pids() -> Vec<u32> {
    let own = std::process::id();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids: Vec<u32> = entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            if pid == own {
                return None;
            }
            let comm = std::fs::read_to_string(entry.path().join("comm")).ok()?;
            (comm.trim() == "rusty").then_some(pid)
        })
        .collect();
    pids.sort_unstable();
    pids
}

/// Post an MCP `initialize` to the back end and read the status line.
fn back_end_answers() -> bool {
    let Ok(addr) = MCP_ADDR.parse::<SocketAddr>() else {
        return false;
    };
    let timeout = Duration::from_secs(2);
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: {MCP_ADDR}\r\nContent-Type: application/json\r\n\
         Accept: application/json, text/event-stream\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{INITIALIZE}",
        INITIALIZE.len()
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(2).any(|w| w == b"\r\n") && head.len() < 4096 {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    answers_ok(&String::from_utf8_lossy(&head))
}

/// `systemctl --user <args>` with the terminal as its output; the exit status.
fn run_systemctl(args: &[&str]) -> i32 {
    match Command::new("systemctl").arg("--user").args(args).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(err) => {
            eprintln!("systemctl --user {}: {err}", args.join(" "));
            1
        }
    }
}

/// `systemctl --user <args>`, its standard output whatever the status.
fn systemctl_output(args: &[&str]) -> Option<String> {
    let out = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn unit_active(unit: &str) -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", unit])
        .status()
        .is_ok_and(|status| status.success())
}

fn unit_state(unit: &str) -> String {
    systemctl_output(&["is-active", unit])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn unit_main_pid(unit: &str) -> Option<u32> {
    systemctl_output(&["show", "-p", "MainPID", "--value", unit])?
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|pid| *pid != 0)
}

/// The names the user manager's environment holds.
fn manager_environment() -> BTreeSet<String> {
    systemctl_output(&["show-environment"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name.to_string()))
        .collect()
}

fn import_environment(names: &[String]) -> bool {
    let mut args = vec!["import-environment"];
    args.extend(names.iter().map(String::as_str));
    run_systemctl(&args) == 0
}

fn join(pids: &[u32]) -> String {
    pids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn no_scripts(_: &str) -> bool {
        false
    }

    #[test]
    fn nouns_come_first_then_scripts_then_errors() {
        assert_eq!(parse(&args(&[]), no_scripts), Request::Window);
        assert_eq!(
            parse(&args(&["-platform", "offscreen"]), no_scripts),
            Request::Window
        );
        assert_eq!(parse(&args(&["--help"]), no_scripts), Request::Help);
        assert_eq!(parse(&args(&["-h"]), no_scripts), Request::Help);
        assert_eq!(parse(&args(&["help"]), no_scripts), Request::Help);
        assert_eq!(
            parse(&args(&["session", "start"]), no_scripts),
            Request::Session(Verb::Start)
        );
        assert_eq!(
            parse(&args(&["session", "stop"]), no_scripts),
            Request::Session(Verb::Stop)
        );
        assert_eq!(
            parse(&args(&["session", "status"]), no_scripts),
            Request::Session(Verb::Status)
        );
        assert_eq!(
            parse(&args(&["session", "run"]), no_scripts),
            Request::Session(Verb::Run)
        );
        assert_eq!(
            parse(&args(&["session"]), no_scripts),
            Request::SessionUsage(None)
        );
        assert_eq!(
            parse(&args(&["session", "dance"]), no_scripts),
            Request::SessionUsage(Some("dance".into()))
        );
        // A store script named like a noun is shadowed; any other name runs with its
        // arguments, in the plain and the `skill/name` form.
        let scripts =
            |name: &str| matches!(name, "usb-reset" | "session" | "dev-box-usb/usb-reset");
        assert_eq!(
            parse(&args(&["session", "start"]), scripts),
            Request::Session(Verb::Start)
        );
        assert_eq!(
            parse(&args(&["usb-reset", "check"]), scripts),
            Request::Script("usb-reset".into(), args(&["check"]))
        );
        assert_eq!(
            parse(&args(&["dev-box-usb/usb-reset"]), scripts),
            Request::Script("dev-box-usb/usb-reset".into(), Vec::new())
        );
        assert_eq!(
            parse(&args(&["sesion", "start"]), scripts),
            Request::Unknown("sesion".into())
        );
    }

    #[test]
    fn the_path_gains_local_bin_in_front_and_cargo_bin_behind_once() {
        let home = Path::new("/home/x");
        assert_eq!(
            completed_path("/usr/bin:/bin", home, true),
            "/home/x/.local/bin:/usr/bin:/bin:/home/x/.cargo/bin"
        );
        assert_eq!(
            completed_path("/usr/bin", home, false),
            "/home/x/.local/bin:/usr/bin"
        );
        assert_eq!(
            completed_path("/home/x/.local/bin:/usr/bin:/home/x/.cargo/bin", home, true),
            "/home/x/.local/bin:/usr/bin:/home/x/.cargo/bin"
        );
        assert_eq!(completed_path("", home, false), "/home/x/.local/bin");
    }

    #[test]
    fn display_vars_are_imported_only_when_set_here_and_absent_there() {
        let manager: BTreeSet<String> = ["XDG_SESSION_TYPE".to_string()].into_iter().collect();
        let here = |name: &str| {
            matches!(
                name,
                "WAYLAND_DISPLAY" | "XDG_SESSION_TYPE" | "HYPRLAND_INSTANCE_SIGNATURE"
            )
        };
        assert_eq!(
            missing_display_vars(&manager, here),
            vec!["WAYLAND_DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE"]
        );
        assert!(missing_display_vars(&manager, |_| false).is_empty());
    }

    #[test]
    fn the_units_own_process_is_not_an_unmanaged_window() {
        assert_eq!(unmanaged(&[100, 200], Some(200)), vec![100]);
        assert_eq!(unmanaged(&[100, 200], None), vec![100, 200]);
        assert!(unmanaged(&[], Some(1)).is_empty());
    }

    #[test]
    fn a_200_head_means_answering() {
        assert!(answers_ok(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n"
        ));
        assert!(!answers_ok("HTTP/1.1 404 Not Found\r\n\r\n"));
        assert!(!answers_ok(""));
        assert!(!answers_ok("garbage"));
    }

    #[test]
    fn the_usage_names_every_verb() {
        for verb in ["start", "stop", "status", "run"] {
            assert!(USAGE.contains(&format!("rusty session {verb}")), "{verb}");
        }
        assert!(USAGE.contains("rusty agent <verb>"));
    }

    /// The `agent` noun (TICKET-031) is answered before Qt starts and before the store
    /// scripts, whatever follows it.
    #[test]
    fn agent_is_a_noun_before_the_scripts() {
        let owned = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            parse(&owned(&["agent", "list"]), |_| true),
            Request::Agent(vec!["list".to_string()])
        );
        assert_eq!(parse(&owned(&["agent"]), |_| false), Request::Agent(vec![]));
        assert_eq!(
            parse(&owned(&["agent", "start", "--cwd", "/x"]), |_| false),
            Request::Agent(owned(&["start", "--cwd", "/x"]))
        );
    }

    /// The wrapper TICKET-009 installed is gone: nothing shipped from `omarchy/` or
    /// `packaging/` invokes it or installs its script (the installer may still name it,
    /// once, to delete a stale copy), and the app unit runs the binary itself.
    #[test]
    fn the_shipped_files_launch_through_the_binary() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let old_ways = [
            "rusty-session.sh",
            "rusty-session up",
            "rusty-session down",
            "rusty-session status",
            "rusty-session run",
        ];
        for dir in ["omarchy", "packaging"] {
            for entry in std::fs::read_dir(root.join(dir)).unwrap().flatten() {
                let path = entry.path();
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                for old in old_ways {
                    assert!(!text.contains(old), "{} says `{old}`", path.display());
                }
            }
        }
        let unit = std::fs::read_to_string(root.join("omarchy/rusty-app.service")).unwrap();
        assert!(
            unit.lines()
                .any(|l| l == "ExecStart=%h/.local/bin/rusty session run"),
            "the app unit runs `rusty session run`"
        );
    }
}
