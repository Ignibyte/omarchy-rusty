//! Rusty's desktop app: the knowledge workspace for Omarchy, laid out as Obsidian is,
//! with native Claude Code and Codex terminals as tabs and as a pane beside the note,
//! and views for tasks, memories, skills, secrets and settings.
//!
//! The window is `qml/Main.qml`, bundled into the binary as the QML module
//! `dev.ignibyte.rusty`. Rust supplies `Theme` (Omarchy colours and tokens, terminal
//! font and colour scheme), `Terminals` (tabs and tmux), `Backend` (the MCP client) and
//! the source editor's tokenizer behind the C++ `MarkdownHighlighter`. Data comes from
//! `rusty-mcp` over local HTTP; the app holds no store of its own. `session.rs` answers
//! `rusty <noun> <verb>` before Qt starts; `agent/` is the `agent` noun, the session
//! host that keeps a Claude Code conversation alive when the window is closed.

mod agent;
mod agents;
mod assistant;
mod backend;
mod desk;
mod diff;
mod folders;
mod markdown;
mod omarchy;
mod session;
mod skin;
mod terminals;
mod theme;

use cxx_qt_lib::{QFont, QGuiApplication, QQmlApplicationEngine, QString, QUrl};

/// Whether `<store>/.claude/skills/<skill>/<name>.sh` exists for `name` or `skill/name`
/// (TICKET-010). The store is `RUSTY_SKILLS` or `~/.rusty/skills`; the CLI, which owns
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
    use std::os::unix::process::CommandExt;
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
    // `rusty <noun> <verb>` (TICKET-029) and `rusty <script>` (TICKET-010) run in place
    // of the window; an argument that starts with a dash is Qt's.
    let args: Vec<String> = std::env::args().skip(1).collect();
    match session::parse(&args, store_script_exists) {
        session::Request::Window => {}
        session::Request::Help => {
            println!("{}", session::USAGE);
            return;
        }
        session::Request::Session(session::Verb::Start) => std::process::exit(session::start()),
        session::Request::Session(session::Verb::Stop) => std::process::exit(session::stop()),
        session::Request::Session(session::Verb::Status) => std::process::exit(session::status()),
        session::Request::Session(session::Verb::Run) => session::complete_path(),
        session::Request::Agent(rest) => std::process::exit(agent::run(&rest)),
        session::Request::SessionUsage(verb) => {
            if let Some(verb) = verb {
                eprintln!("rusty session: unknown verb '{verb}'");
            }
            eprintln!("{}", session::USAGE);
            std::process::exit(2);
        }
        session::Request::Script(name, rest) => {
            eprintln!("rusty {name}: {}", exec_store_script(&name, &rest));
            std::process::exit(126);
        }
        session::Request::Unknown(word) => {
            eprintln!("rusty: unknown command '{word}'");
            eprintln!("{}", session::USAGE);
            std::process::exit(2);
        }
    }
    // Wayland first on Omarchy; Qt still falls back to X11 when there is no compositor.
    if std::env::var_os("QT_QPA_PLATFORM").is_none() {
        std::env::set_var("QT_QPA_PLATFORM", "wayland;xcb");
    }
    // The terminal widget lists colour schemes from the directory named by this
    // variable, so the scheme generated from the Omarchy theme is found there.
    if std::env::var_os("COLORSCHEMES_DIR").is_none() {
        let dir = omarchy::scheme_dir();
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("COLORSCHEMES_DIR", &dir);
    }
    let mut app = QGuiApplication::new();
    if let Some(mut app) = app.as_mut() {
        app.as_mut().set_application_name(&QString::from("rusty"));
        app.as_mut()
            .set_application_display_name(&QString::from("Rusty"));
        app.as_mut()
            .set_organization_name(&QString::from("Ignibyte"));
        app.as_mut()
            .set_organization_domain(&QString::from("ignibyte.com"));
        // The skin's face for every label, set once before any item exists.
        let (family, px) = theme::startup_font();
        let mut font = QFont::default();
        font.set_family(&QString::from(&family));
        font.set_pixel_size(px);
        app.as_mut().set_application_font(&font);
    }
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/dev/ignibyte/rusty/qml/Main.qml"));
    }
    let code = match app.as_mut() {
        Some(app) => app.exec(),
        None => 1,
    };
    std::process::exit(code);
}
