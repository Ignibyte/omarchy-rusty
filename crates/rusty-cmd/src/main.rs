//! `rusty`: the command. `rusty <noun> <verb>` and `rusty <script>` are answered here;
//! `rusty` alone prints the usage, as `rusty help` does.

use std::os::unix::process::CommandExt;

use rusty_cmd::session;

/// Whether `<store>/.claude/skills/<skill>/<name>.sh` exists for `name` or `skill/name`.
/// The store is `RUSTY_SKILLS` or `~/.rusty/skills`; the CLI, which owns
/// the resolver, decides the rest (a pending script, a clash between skills).
fn store_script_exists(name: &str) -> bool {
    if name.is_empty() || name.starts_with('-') || name.contains("..") {
        return false;
    }
    let store = std::env::var_os("RUSTY_SKILLS")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|h| std::path::PathBuf::from(h).join(".rusty").join("skills"))
        });
    let Some(store) = store else {
        return false;
    };
    let active = store.join(".claude").join("skills");
    let name = name.strip_suffix(".sh").unwrap_or(name);
    if let Some((skill, base)) = name.split_once('/') {
        return active.join(skill).join(format!("{base}.sh")).is_file();
    }
    let Ok(entries) = std::fs::read_dir(&active) else {
        return false;
    };
    entries
        .flatten()
        .any(|e| e.path().join(format!("{name}.sh")).is_file())
}

/// Hand the process to `rusty-cli scripts run <name> args...`; returns only on failure.
fn exec_store_script(name: &str, args: &[String]) -> String {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("rusty-cli")))
        .filter(|p| p.is_file());
    let cli = beside.unwrap_or_else(|| std::path::PathBuf::from("rusty-cli"));
    let err = std::process::Command::new(&cli)
        .arg("scripts")
        .arg("run")
        .arg(name)
        .args(args)
        .exec();
    format!("{}: {err}", cli.display())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match session::parse(&args, store_script_exists) {
        session::Request::Help => {
            println!("{}", session::USAGE);
            0
        }
        session::Request::Session(session::Verb::Start) => session::start(),
        session::Request::Session(session::Verb::Status) => session::status(),
        session::Request::SessionUsage(verb) => {
            if let Some(verb) = verb {
                eprintln!("rusty session: unknown verb '{verb}'");
            }
            eprintln!("{}", session::USAGE);
            2
        }
        session::Request::Script(name, rest) => {
            eprintln!("rusty {name}: {}", exec_store_script(&name, &rest));
            126
        }
        session::Request::Unknown(word) => {
            eprintln!("rusty: unknown command '{word}'");
            eprintln!("{}", session::USAGE);
            2
        }
    };
    std::process::exit(code);
}
