//! `rusty <noun> <verb>`: the commands the `rusty` binary answers. The first noun is
//! `session`, the back end under its systemd user unit (`omarchy/rusty-mcp.service`).
//! Built-in nouns come before store scripts.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::Command;
use std::time::Duration;

/// The back end's unit.
pub const MCP_UNIT: &str = "rusty-mcp.service";
/// Where the back end serves HTTP (see `omarchy/rusty-mcp.service`).
const MCP_ADDR: &str = "127.0.0.1:4174";
/// The same endpoint as the docs name it.
const MCP_URL: &str = "http://127.0.0.1:4174/mcp";

/// The MCP `initialize` the probe posts; the back end answers 200 when it is serving.
const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"rusty session","version":"0"}}}"#;

/// What `rusty help`, and `rusty` alone, print.
pub const USAGE: &str = "\
usage: rusty <command> [args...]
  rusty session start        start the back end (rusty-mcp.service); safe to run again
  rusty session status       the back end's unit, and whether it answers on its port
  rusty <script> [args...]   a store script, a *.sh beside a skill (rusty-cli scripts list)
  rusty help                 this text";

/// A `session` verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Start,
    Status,
}

/// What the command line asks for, decided before anything runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Print the usage and exit 0.
    Help,
    /// One of the session verbs.
    Session(Verb),
    /// `rusty session` alone, or with a verb that does not exist.
    SessionUsage(Option<String>),
    /// A store script by name, with its arguments.
    Script(String, Vec<String>),
    /// A word or a flag that is neither a noun nor a script.
    Unknown(String),
}

/// Read the command line: nouns first, store scripts second, anything else an error.
pub fn parse(args: &[String], script_exists: impl Fn(&str) -> bool) -> Request {
    let Some(first) = args.first() else {
        return Request::Help;
    };
    match first.as_str() {
        "help" | "--help" | "-h" => Request::Help,
        flag if flag.starts_with('-') => Request::Unknown(flag.to_string()),
        "session" => match args.get(1).map(String::as_str) {
            Some("start") => Request::Session(Verb::Start),
            Some("status") => Request::Session(Verb::Status),
            other => Request::SessionUsage(other.map(str::to_string)),
        },
        name if script_exists(name) => Request::Script(name.to_string(), args[1..].to_vec()),
        name => Request::Unknown(name.to_string()),
    }
}

/// `rusty session start`: the back end's unit started (nothing happens when it runs
/// already), then the status. Returns the exit status.
pub fn start() -> i32 {
    let code = run_systemctl(&["start", MCP_UNIT]);
    if code != 0 {
        return code;
    }
    status()
}

/// `rusty session status`: the back end's unit, and whether it answers on its port.
pub fn status() -> i32 {
    println!("{MCP_UNIT:<18} {}", unit_state(MCP_UNIT));
    let word = if back_end_answers() {
        "answering"
    } else {
        "not answering"
    };
    println!("{:<18} {word} on {MCP_URL}", "back end");
    0
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

/// Post an MCP `initialize` to the back end and read the status line.
fn back_end_answers() -> bool {
    // `RUSTY_MCP_ADDR` points a test at a port of its own, never the live back end.
    let host = std::env::var("RUSTY_MCP_ADDR")
        .ok()
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| MCP_ADDR.to_string());
    let Ok(addr) = host.parse::<SocketAddr>() else {
        return false;
    };
    let timeout = Duration::from_secs(2);
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\n\
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

fn unit_state(unit: &str) -> String {
    systemctl_output(&["is-active", unit])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn no_scripts(_: &str) -> bool {
        false
    }

    #[test]
    fn nouns_come_first_then_scripts_then_errors() {
        assert_eq!(parse(&args(&[]), no_scripts), Request::Help);
        assert_eq!(parse(&args(&["--help"]), no_scripts), Request::Help);
        assert_eq!(parse(&args(&["-h"]), no_scripts), Request::Help);
        assert_eq!(parse(&args(&["help"]), no_scripts), Request::Help);
        assert_eq!(
            parse(&args(&["session", "start"]), no_scripts),
            Request::Session(Verb::Start)
        );
        assert_eq!(
            parse(&args(&["session", "status"]), no_scripts),
            Request::Session(Verb::Status)
        );
        assert_eq!(
            parse(&args(&["session"]), no_scripts),
            Request::SessionUsage(None)
        );
        // The verbs that drove the retired app unit are unknown now (TICKET-053).
        for gone in ["stop", "run"] {
            assert_eq!(
                parse(&args(&["session", gone]), no_scripts),
                Request::SessionUsage(Some(gone.into()))
            );
        }
        // A dash argument went to Qt; with no window it is an error, even when a
        // script resolver would say yes.
        assert_eq!(
            parse(&args(&["-platform", "offscreen"]), |_| true),
            Request::Unknown("-platform".into())
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
    fn a_200_head_means_answering() {
        assert!(answers_ok(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n"
        ));
        assert!(!answers_ok("HTTP/1.1 404 Not Found\r\n\r\n"));
        assert!(!answers_ok(""));
        assert!(!answers_ok("garbage"));
    }

    #[test]
    fn the_usage_names_every_verb_and_no_window() {
        for verb in ["start", "status"] {
            assert!(USAGE.contains(&format!("rusty session {verb}")), "{verb}");
        }
        for gone in [
            "rusty session stop",
            "rusty session run",
            "window",
            "rusty-app",
            "Qt",
        ] {
            assert!(!USAGE.contains(gone), "the usage still says {gone}");
        }
    }

    /// The `agent` noun left with the session host (TICKET-056): `agent` is a store
    /// script when one has that name, else an unknown word.
    #[test]
    fn agent_is_no_longer_a_noun() {
        let owned = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            parse(&owned(&["agent", "list"]), |_| false),
            Request::Unknown("agent".into())
        );
        assert_eq!(
            parse(&owned(&["agent", "list"]), |n| n == "agent"),
            Request::Script("agent".into(), owned(&["list"]))
        );
        assert!(!USAGE.contains("rusty agent"));
    }

    /// Nothing shipped from `omarchy/` or `packaging/` invokes the wrapper TICKET-009
    /// installed (the installer may still name it, once, to delete a stale copy), and
    /// nothing but a README names the app that retired with TICKET-053.
    #[test]
    fn the_shipped_files_name_neither_the_wrapper_nor_the_app() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let old_ways = [
            "rusty-session.sh",
            "rusty-session up",
            "rusty-session down",
            "rusty-session status",
            "rusty-session run",
        ];
        let the_app = [
            "rusty-app",
            "rusty session run",
            "rusty session stop",
            "qmltermwidget",
            "com.ignibyte.rusty",
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
                if path.extension().is_some_and(|e| e == "md") {
                    continue;
                }
                for old in the_app {
                    assert!(!text.contains(old), "{} says `{old}`", path.display());
                }
            }
        }
    }
}
