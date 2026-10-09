//! Rusty CLI — terminal access to Rusty's brain (the knowledge vault).
//!
//! A thin wrapper over `rusty_core`'s [`BrainManager`] so the terminal assistant
//! can search, read, capture to, and reindex the brain while keeping the FTS
//! index, link graph, and git auto-commit consistent — the same code path the
//! MCP server uses. Tasks, notes, and memories are reached directly (sqlite3 + files);
//! this CLI deliberately covers only the brain, where the SQLite side is derived
//! state that must stay in sync with the markdown vault.

mod hooks;

use rusty_core::brain::semantic;
use rusty_core::brain::{BrainManager, CaptureTarget};
use rusty_core::engine::conversation_archive::ConversationArchive;
use rusty_core::engine::db::Database;
use rusty_core::engine::secrets_manager::SecretsManager;
use rusty_core::engine::settings_manager::SettingsManager;
use rusty_core::skills::{self, SkillsManager};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::exit;
use std::sync::Arc;

const USAGE: &str = r#"rusty-cli: terminal access to Rusty's store

USAGE:

Brain pages
  rusty-cli brain search <query...> [--limit N] [--type TYPE]
  rusty-cli brain read <slug>
  rusty-cli brain new <type> <title...> [--content <text>]
  rusty-cli brain set <slug> <content...>      (replace the page body)
  rusty-cli brain append <slug> <summary...> [--detail <text>]   (add a timeline entry)
  rusty-cli brain capture <text...> [--to daily|inbox] [--date YYYY-MM-DD]
  rusty-cli brain daily [--date YYYY-MM-DD]    (open or create the daily page)
  rusty-cli brain context <prompt...>          (the pages that best match a prompt, as one block)
  rusty-cli brain types                        (page types, folders, counts)
  rusty-cli brain stats
  rusty-cli bookmarks [add|rm <path or search:query>]   (the vault's bookmarks)

Brain upkeep
  rusty-cli brain reindex
  rusty-cli brain embed [--all]                (vectors for stale pages, or every page)
  rusty-cli brain semantic                     (embedding provider and index state)
  rusty-cli brain migrate [--dry-run]          (timeline sections and vault-path links)
  rusty-cli brain import <vault> [--dry-run]   (an Obsidian vault: pages, attachments, bookmarks; a report under inbox/)

The brain loop
  rusty-cli brain ask <question>               (pages, decisions, follow-ups due, and a consultation id)
  rusty-cli brain decide <id> --title T --choice C --rationale R [--alt A]... [--follow-up-by DATE] [--supersedes SLUG]
  rusty-cli brain follow-up <slug> --status kept|revised|superseded --outcome O [--successor SLUG] [--follow-up-by DATE]
  rusty-cli brain no-decision <id> <reason>    (a consultation that led to no decision)
  rusty-cli brain due [--days N]               (follow-ups due today and overdue, or within N days)
  rusty-cli hooks install|uninstall|status     (the loop's Claude Code hooks, in ~/.claude/settings.json)

Sources
  rusty-cli source capture <url>               (keep a web page, PDF or text from a URL as a page under sources/)
  rusty-cli source search <query...> [--limit N]

Notes
  rusty-cli notes path                         (the notes folder the tools use)
  rusty-cli notes adopt [--dry-run]            (move a notes folder kept outside the vault into it, once)

Skills and scripts
  rusty-cli skills list [--all]
  rusty-cli skills view <name>
  rusty-cli skills new <name> [--desc <text>] [--body <text>] [--force]
  rusty-cli skills rm <name>
  rusty-cli skills path
  rusty-cli skills review                      (pending skills and their safety-scan findings)
  rusty-cli skills approve <name> [--force]
  rusty-cli skills reject <name>
  rusty-cli scripts list [--all]               (a *.sh beside a skill is the command `rusty <name>`)
  rusty-cli scripts view|path|edit|rm <name>   (name, or skill/name when two skills share one)
  rusty-cli scripts new <name> [--skill S] [--body TEXT] [--force]
  rusty-cli scripts run <name> [args...]       (an approved script, in place of this process)

Conversations
  rusty-cli ingest-conversation <path|session-id>   (keep a Claude Code transcript: searchable text and a brain page)
  rusty-cli ingest-conversation --all [--dir <path>] [--limit N]   (every transcript in this directory's Claude Code project)
  rusty-cli conversations search <query...> [--limit N]

The store
  rusty-cli changes [--since <cursor>] [--limit N]   (what changed, from any process)
  rusty-cli settings list                      (every setting; credential-looking values masked)
  rusty-cli settings get <key>
  rusty-cli settings set <key> <value>         (path settings take an absolute path and apply when a server starts)
  rusty-cli refresh                            (tell running clients the data changed, after a raw write)
  rusty-cli export <file.zip> [--include-secrets]   (the whole store in one zip; secrets only when asked)
  rusty-cli import <file.zip> [--replace] [--dry-run]   (unpack an export into ~/.rusty; --replace moves an existing store aside)"#;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match (
        args.get(1).map(String::as_str),
        args.get(2).map(String::as_str),
    ) {
        (Some("brain"), Some(sub)) => run_brain(sub, &args[3..]),
        (Some("notes"), Some(sub)) => run_notes(sub, &args[3..]),
        (Some("source"), Some(sub)) => run_source(sub, &args[3..]),
        (Some("hooks"), sub) => {
            if let Some(extra) = args.get(3) {
                usage_error(&format!(
                    "hooks {}: unexpected {extra:?}",
                    sub.unwrap_or("")
                ));
            }
            run_hooks(sub)
        }
        (Some("skills"), _) => {
            let sub = args.get(2).map(String::as_str).unwrap_or("list");
            run_skills(sub, args.get(3..).unwrap_or_default());
        }
        (Some("scripts"), _) => {
            let sub = args.get(2).map(String::as_str).unwrap_or("list");
            run_scripts(sub, args.get(3..).unwrap_or_default());
        }
        (Some("ingest-conversation"), _) => {
            run_ingest_conversation(args.get(2..).unwrap_or_default())
        }
        (Some("conversations"), _) => {
            let sub = args.get(2).map(String::as_str).unwrap_or("search");
            run_conversations(sub, args.get(3..).unwrap_or_default());
        }
        (Some("refresh"), _) => refresh_signal(),
        (Some("export"), _) => run_export(args.get(2..).unwrap_or_default()),
        (Some("import"), _) => run_import(args.get(2..).unwrap_or_default()),
        (Some("changes"), _) => run_changes(args.get(2..).unwrap_or_default()),
        (Some("bookmarks"), sub) => run_bookmarks(sub, args.get(3..).unwrap_or_default()),
        (Some("settings"), sub) => run_settings(sub, args.get(3..).unwrap_or_default()),
        (Some("--help") | Some("-h"), _) | (None, _) => {
            println!("{USAGE}");
        }
        _ => {
            eprintln!("{USAGE}");
            exit(2);
        }
    }
}

/// The brain vault path the MCP server uses: the `brain_vault_path` setting,
/// else `~/.rusty/brain`.
fn configured_brain_path(db: &Arc<Database>) -> PathBuf {
    let settings = SettingsManager::new(Arc::clone(db));
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let default_brain = home
        .join(".rusty")
        .join("brain")
        .to_string_lossy()
        .to_string();
    PathBuf::from(
        settings
            .get_or_default("brain_vault_path", &default_brain)
            .unwrap_or(default_brain),
    )
}

/// The notes folder the tools use: the `notes_path` setting, else `<vault>/notes`.
fn configured_notes_path(db: &Arc<Database>) -> PathBuf {
    let settings = SettingsManager::new(Arc::clone(db));
    let default_notes = configured_brain_path(db)
        .join("notes")
        .to_string_lossy()
        .to_string();
    PathBuf::from(
        settings
            .get_or_default("notes_path", &default_notes)
            .unwrap_or(default_notes),
    )
}

/// Where notes lived before they joined the vault: the `notes_path` setting when set,
/// else `~/.rusty/notes`.
fn legacy_notes_path(db: &Arc<Database>) -> PathBuf {
    let settings = SettingsManager::new(Arc::clone(db));
    match settings.get("notes_path") {
        Ok(Some(p)) if !p.trim().is_empty() => PathBuf::from(p),
        _ => dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".rusty")
            .join("notes"),
    }
}

/// Dispatch a `notes` subcommand.
/// `source capture <url>` and `source search <query>` (TICKET-027).
fn run_source(sub: &str, rest: &[String]) {
    match sub {
        "capture" => {
            let url = rest.first().cloned().unwrap_or_default();
            if url.is_empty() {
                fail("usage: rusty-cli source capture <url>");
            }
            match brain().capture_url(&url) {
                Ok(page) => {
                    let status = page
                        .frontmatter
                        .extra
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("captured");
                    println!("{status}: {} ({})", page.slug, page.title);
                    if let Some(error) =
                        page.frontmatter.extra.get("error").and_then(|v| v.as_str())
                    {
                        println!("  {error}");
                    }
                }
                Err(e) => fail(&e),
            }
        }
        "search" => {
            let (pos, flags) = parse(rest);
            let query = pos.join(" ");
            if query.is_empty() {
                fail("search: missing <query>");
            }
            let limit = flags.get("limit").and_then(|v| v.parse().ok());
            match brain().search(&format!("{query} type:source"), limit, None) {
                Ok(hits) => {
                    println!("{}", rusty_core::brain::sources::UNTRUSTED_NOTE);
                    for hit in hits {
                        println!("{}  {}", hit.slug, hit.title);
                        let snippet = rusty_core::brain::sources::normalise(&hit.snippet);
                        if !snippet.is_empty() {
                            println!("  {}", snippet.replace('\n', " "));
                        }
                    }
                }
                Err(e) => fail(&e),
            }
        }
        _ => fail(&format!("unknown source command: {sub}")),
    }
}

fn run_notes(sub: &str, rest: &[String]) {
    let db = Arc::new(Database::open().unwrap_or_else(|e| fail(&format!("open database: {e}"))));
    match sub {
        "path" => println!("{}", configured_notes_path(&db).display()),
        "adopt" => {
            let dry_run = rest.iter().any(|a| a == "--dry-run");
            let into = configured_brain_path(&db).join("notes");
            let from = legacy_notes_path(&db);
            if from.canonicalize().ok() == into.canonicalize().ok() && into.exists() {
                println!("notes already live in the vault at {}", into.display());
                return;
            }
            match rusty_core::notes::adopt(&from, &into, dry_run) {
                Ok(report) if report.nothing_to_do => {
                    println!("nothing to adopt under {}", from.display());
                }
                Ok(report) => {
                    let verb = if dry_run { "would move" } else { "moved" };
                    println!(
                        "{verb} {} file(s) from {} into {}",
                        report.moved.len(),
                        report.from,
                        report.into
                    );
                    for rel in &report.moved {
                        println!("  {rel}");
                    }
                    if !dry_run {
                        let settings = SettingsManager::new(Arc::clone(&db));
                        let value = into.to_string_lossy().to_string();
                        settings
                            .set("notes_path", &value)
                            .unwrap_or_else(|e| fail(&format!("set notes_path: {e}")));
                        println!("notes_path is now {value}; the old folder keeps a README");
                        refresh_signal();
                    }
                }
                Err(e) => fail(&e),
            }
        }
        other => {
            eprintln!("unknown notes subcommand: '{other}'\n\n{USAGE}");
            std::process::exit(2);
        }
    }
}

/// The embedding provider the settings and the secrets file point at, if any.
fn configured_embedder() -> Option<std::sync::Arc<dyn semantic::Embedder>> {
    let db = Arc::new(Database::open().unwrap_or_else(|e| fail(&format!("open database: {e}"))));
    let settings = SettingsManager::new(Arc::clone(&db));
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let secrets = SecretsManager::new(home.join(".rusty").join(".secret"));
    semantic::resolve_embedder(&settings, &secrets)
}

/// Open the database and a `BrainManager` rooted at the configured vault path.
///
/// Mirrors the MCP server's path resolution so both operate on one vault.
fn brain() -> BrainManager {
    let db = Arc::new(Database::open().unwrap_or_else(|e| fail(&format!("open database: {e}"))));
    let brain_path = configured_brain_path(&db);
    let brain = BrainManager::new(Arc::clone(&db), PathBuf::from(&brain_path));
    brain
        .ensure_vault()
        .unwrap_or_else(|e| fail(&format!("init brain vault: {e}")));
    brain
}

/// Open a `ConversationArchive` over the shared db + brain vault.
fn archive() -> (Arc<Database>, ConversationArchive) {
    let db = Arc::new(Database::open().unwrap_or_else(|e| fail(&format!("open database: {e}"))));
    let brain_path = configured_brain_path(&db);
    let brain = Arc::new(BrainManager::new(
        Arc::clone(&db),
        PathBuf::from(&brain_path),
    ));
    brain
        .ensure_vault()
        .unwrap_or_else(|e| fail(&format!("init brain vault: {e}")));
    let arch = ConversationArchive::new(Arc::clone(&db), brain);
    (db, arch)
}

/// Claude Code's transcript directory for the current working directory.
fn claude_project_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    // Claude Code slugifies the cwd by replacing every non-alphanumeric char with '-'.
    let slug: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    home.join(".claude").join("projects").join(slug)
}

/// Resolve a transcript argument: an existing path, or a session id looked up
/// under `~/.claude/projects/*/<sid>.jsonl`.
fn resolve_transcript(target: &str) -> Option<PathBuf> {
    let p = PathBuf::from(target);
    if p.is_file() {
        return Some(p);
    }
    let sid = target.trim_end_matches(".jsonl");
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let projects = home.join(".claude").join("projects");
    let entries = std::fs::read_dir(&projects).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path().join(format!("{sid}.jsonl"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// List `*.jsonl` transcript files in a directory, sorted.
fn list_jsonl(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("jsonl"))
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    files
}

/// `rusty-cli ingest-conversation <path|session-id> | --all [--dir <path>] [--limit N]`.
fn run_ingest_conversation(args: &[String]) {
    let (pos, flags) = parse_with_bools(args, &["all"]);
    let (_db, arch) = archive();

    if flags.contains_key("all") {
        let dir = flags
            .get("dir")
            .map(PathBuf::from)
            .unwrap_or_else(claude_project_dir);
        let limit: usize = flags
            .get("limit")
            .and_then(|s| s.parse().ok())
            .unwrap_or(usize::MAX);
        let files = list_jsonl(&dir);
        if files.is_empty() {
            fail(&format!("no transcripts found in {}", dir.display()));
        }
        let mut seen = 0usize;
        let mut ok = 0usize;
        for f in files {
            if seen >= limit {
                break;
            }
            seen += 1;
            match arch.ingest(&f) {
                Ok(o) => {
                    ok += 1;
                    println!(
                        "  [{}] {} ({} msgs) -> {}",
                        if o.created { "new" } else { "upd" },
                        o.title,
                        o.message_count,
                        o.brain_slug
                    );
                }
                Err(e) => eprintln!("  skip {}: {e}", f.display()),
            }
        }
        println!("ingested {ok}/{seen} transcripts from {}", dir.display());
        if ok == 0 {
            exit(1);
        }
        return;
    }

    let target = pos.first().unwrap_or_else(|| {
        fail("ingest-conversation: usage: rusty-cli ingest-conversation <path|session-id> | --all [--dir <path>] [--limit N]")
    });
    let path = resolve_transcript(target)
        .unwrap_or_else(|| fail(&format!("transcript not found for '{target}'")));
    match arch.ingest(&path) {
        Ok(o) => {
            println!(
                "ingested \"{}\" ({} messages){}",
                o.title,
                o.message_count,
                if o.created { "" } else { " [updated]" }
            );
            println!("  brain node: {}", o.brain_slug);
            if !o.linked.is_empty() {
                println!("  linked: {}", o.linked.join(", "));
            }
        }
        Err(e) => fail(&e),
    }
}

/// `rusty-cli conversations search <query...> [--limit N]`.
fn run_conversations(sub: &str, rest: &[String]) {
    match sub {
        "search" => {
            let (pos, flags) = parse(rest);
            let query = pos.join(" ");
            if query.is_empty() {
                fail("conversations search: missing <query>");
            }
            let limit = flags
                .get("limit")
                .and_then(|s| s.parse().ok())
                .unwrap_or(10);
            let (_db, arch) = archive();
            match arch.search(&query, limit) {
                Ok(hits) if hits.is_empty() => {
                    println!("(no conversations match \"{query}\")")
                }
                Ok(hits) => {
                    for h in hits {
                        let snip = h.snippet.replace('\n', " ");
                        println!(
                            "{}  [{}]\n  {} · {}\n  {}",
                            h.title,
                            h.session_id,
                            h.started_at,
                            h.brain_slug,
                            snip.trim()
                        );
                    }
                }
                Err(e) => fail(&e),
            }
        }
        other => {
            eprintln!("unknown conversations subcommand: '{other}'\n\n{USAGE}");
            exit(2);
        }
    }
}

/// Dispatch a `brain` subcommand.
fn run_brain(sub: &str, rest: &[String]) {
    match sub {
        "search" => {
            let (pos, flags) = parse(rest);
            let query = pos.join(" ");
            if query.is_empty() {
                fail("search: missing <query>");
            }
            let limit = flags.get("limit").and_then(|s| s.parse::<usize>().ok());
            let ptype = flags.get("type").map(String::as_str);
            match brain().search(&query, limit, ptype) {
                Ok(results) if results.is_empty() => println!("(no brain matches for \"{query}\")"),
                Ok(results) => {
                    for r in results {
                        let snip = r.snippet.replace("<b>", "").replace("</b>", "");
                        println!(
                            "{}\t[{}] {}\n    {}",
                            r.slug,
                            r.page_type,
                            r.title,
                            snip.trim()
                        );
                    }
                }
                Err(e) => fail(&e),
            }
        }
        "context" => {
            let prompt = rest.join(" ");
            if prompt.is_empty() {
                fail("context: missing <prompt>");
            }
            let ctx = brain().build_context_for_prompt(&prompt);
            if ctx.is_empty() {
                println!("(no relevant brain context)");
            } else {
                println!("{ctx}");
            }
        }
        "read" => {
            let (pos, _) = parse(rest);
            let slug = pos.first().unwrap_or_else(|| fail("read: missing <slug>"));
            match brain().read_page(slug) {
                Ok(Some(p)) => {
                    println!(
                        "# {} ({})\nslug: {}\nfile: {}\n\n{}",
                        p.title, p.page_type, p.slug, p.file, p.compiled_truth
                    );
                    if !p.timeline.trim().is_empty() {
                        println!("\n## Timeline\n{}", p.timeline);
                    }
                }
                Ok(None) => fail(&format!("no brain page: {slug}")),
                Err(e) => fail(&e),
            }
        }
        "new" => {
            let (pos, flags) = parse(rest);
            if pos.len() < 2 {
                fail("new: usage: brain new <type> <title...> [--content <text>]");
            }
            let content = flags.get("content").cloned().unwrap_or_default();
            let bm = brain();
            match bm.create_page(&pos[0], &pos[1..].join(" "), &content) {
                Ok(p) => println!("created {} ({})", p.slug, p.page_type),
                Err(e) => fail(&e),
            }
            bm.flush_commits();
        }
        "append" => {
            let (pos, flags) = parse(rest);
            if pos.len() < 2 {
                fail("append: usage: brain append <slug> <summary...> [--detail <text>]");
            }
            let detail = flags.get("detail").map(String::as_str);
            let date = flags.get("date").cloned().unwrap_or_else(today);
            let bm = brain();
            match bm.add_timeline(&pos[0], &date, "terminal", &pos[1..].join(" "), detail) {
                Ok(_) => println!("appended to {}", pos[0]),
                Err(e) => fail(&e),
            }
            bm.flush_commits();
        }
        "set" => {
            let (pos, _) = parse(rest);
            if pos.len() < 2 {
                fail("set: usage: brain set <slug> <content...>");
            }
            let bm = brain();
            match bm.update_page(&pos[0], &pos[1..].join(" ")) {
                Ok(p) => println!("updated {}", p.slug),
                Err(e) => fail(&e),
            }
            bm.flush_commits();
        }
        "capture" => {
            let (pos, flags) = parse(rest);
            let text = pos.join(" ");
            if text.is_empty() {
                fail("capture: missing <text>");
            }
            let target =
                CaptureTarget::parse(flags.get("to").map(String::as_str).unwrap_or("daily"))
                    .unwrap_or_else(|e| fail(&e));
            let brain = brain();
            match brain.capture(
                &text,
                target,
                flags.get("date").map(String::as_str),
                "terminal",
            ) {
                Ok(r) => println!("captured to {}", r.slug),
                Err(e) => fail(&e),
            }
            brain.flush_commits();
        }
        "daily" => {
            let (_, flags) = parse(rest);
            let brain = brain();
            match brain.daily_page(flags.get("date").map(String::as_str)) {
                Ok(page) => {
                    println!("{}\n", page.slug);
                    if !page.compiled_truth.is_empty() {
                        println!("{}\n", page.compiled_truth);
                    }
                    if !page.timeline.is_empty() {
                        println!("## Timeline\n\n{}", page.timeline);
                    }
                }
                Err(e) => fail(&e),
            }
            brain.flush_commits();
        }
        "ask" => {
            let question = rest.join(" ");
            match brain().ask(&question, None, None) {
                Ok(c) => {
                    println!("consultation {}", c.id);
                    for p in &c.pages {
                        println!(
                            "  {}  {}  {}",
                            p.slug,
                            p.title,
                            p.snippet.replace('\n', " ")
                        );
                    }
                    for d in &c.decisions {
                        println!("  decision {}  {}  {}", d.slug, d.status, d.title);
                    }
                    for d in &c.due {
                        println!("  due {}  {}  {}", d.follow_up_by, d.slug, d.title);
                    }
                }
                Err(e) => {
                    eprintln!("brain ask: {e}");
                    std::process::exit(1);
                }
            }
        }
        "decide" => {
            let mut input = rusty_core::brain::decisions::Decide::default();
            let mut it = rest.iter();
            input.consultation = it.next().cloned().unwrap_or_default();
            while let Some(flag) = it.next() {
                let value = it.next().cloned().unwrap_or_default();
                match flag.as_str() {
                    "--title" => input.title = value,
                    "--choice" => input.choice = value,
                    "--rationale" => input.rationale = value,
                    "--alt" => input.alternatives.push(value),
                    "--follow-up-by" => input.follow_up_by = Some(value),
                    "--supersedes" => input.supersedes = Some(value),
                    other => {
                        eprintln!("brain decide: unknown flag {other}");
                        std::process::exit(2);
                    }
                }
            }
            match brain().decide(&input) {
                Ok(page) => {
                    println!("{}", page.slug);
                    refresh_signal();
                }
                Err(e) => {
                    eprintln!("brain decide: {e}");
                    std::process::exit(1);
                }
            }
        }
        "follow-up" => {
            let mut input = rusty_core::brain::decisions::FollowUp::default();
            let mut it = rest.iter();
            input.slug = it.next().cloned().unwrap_or_default();
            while let Some(flag) = it.next() {
                let value = it.next().cloned().unwrap_or_default();
                match flag.as_str() {
                    "--status" => input.status = value,
                    "--outcome" => input.outcome = value,
                    "--successor" => input.successor = Some(value),
                    "--follow-up-by" => input.follow_up_by = Some(value),
                    other => {
                        eprintln!("brain follow-up: unknown flag {other}");
                        std::process::exit(2);
                    }
                }
            }
            match brain().follow_up(&input) {
                Ok(page) => {
                    println!("{}", page.slug);
                    refresh_signal();
                }
                Err(e) => {
                    eprintln!("brain follow-up: {e}");
                    std::process::exit(1);
                }
            }
        }
        "no-decision" => {
            let id = rest.first().cloned().unwrap_or_default();
            let reason = rest.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
            match brain().no_decision(&id, &reason) {
                Ok(()) => println!("recorded"),
                Err(e) => {
                    eprintln!("brain no-decision: {e}");
                    std::process::exit(1);
                }
            }
        }
        "due" => {
            let days = rest
                .iter()
                .position(|a| a == "--days")
                .and_then(|i| rest.get(i + 1))
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            match brain().due(days) {
                Ok(d) => {
                    for x in &d.due {
                        println!(
                            "{}  {}  {}{}{}",
                            x.follow_up_by,
                            x.slug,
                            x.title,
                            if x.overdue { "  (overdue)" } else { "" },
                            if x.followed_up.is_empty() {
                                String::new()
                            } else {
                                format!("  (last followed up {})", x.followed_up)
                            }
                        );
                    }
                    if d.due.is_empty() {
                        println!("nothing due");
                    }
                }
                Err(e) => {
                    eprintln!("brain due: {e}");
                    std::process::exit(1);
                }
            }
        }
        "types" => match brain().page_types() {
            Ok(types) => {
                for t in types {
                    println!("{:<13} {:<14} {}", t.page_type, t.dir, t.count);
                }
            }
            Err(e) => fail(&e),
        },
        "migrate" => {
            let (_, flags) = parse_with_bools(rest, &["dry-run"]);
            let dry = flags.contains_key("dry-run");
            match brain().migrate_vault(dry) {
                Ok(r) => {
                    println!(
                        "{}scanned {} pages: {} changed, {} timelines converted, {} links rewritten",
                        if dry { "dry run: " } else { "" },
                        r.pages_scanned,
                        r.pages_changed,
                        r.timelines_converted,
                        r.links_rewritten
                    );
                    for slug in &r.changed_slugs {
                        println!("  changed: {slug}");
                    }
                    for u in &r.unresolved_links {
                        println!("  unresolved: {u}");
                    }
                }
                Err(e) => fail(&e),
            }
        }
        "import" => {
            let (args, flags) = parse_with_bools(rest, &["dry-run"]);
            let dry = flags.contains_key("dry-run");
            let path = args.first().cloned().unwrap_or_default();
            if path.is_empty() {
                fail("usage: rusty-cli brain import <vault> [--dry-run]");
            }
            let print_plan = |p: &rusty_core::brain::import::ImportPlan| {
                println!(
                    "{}: {} pages in {} folders, {} attachments, {} tags, {} bookmarks; {} collisions skipped, {} unresolved links",
                    p.name,
                    p.pages.len(),
                    p.folders.len(),
                    p.attachments.len(),
                    p.tags.len(),
                    p.bookmarks.len(),
                    p.collisions.len(),
                    p.unresolved_links.len()
                );
                for c in &p.collisions {
                    println!("  collision (left as it is): {c}");
                }
                for u in &p.unresolved_links {
                    println!("  unresolved: {u}");
                }
                for b in &p.bookmarks_skipped {
                    println!("  bookmark not carried: {b}");
                }
                for b in &p.bookmarks {
                    println!("  bookmark: {} {} {}{}", b.kind, b.title, b.path, b.query);
                }
            };
            if dry {
                match brain().import_plan(std::path::Path::new(&path)) {
                    Ok(p) => {
                        print!("dry run: ");
                        print_plan(&p);
                    }
                    Err(e) => fail(&e),
                }
            } else {
                match brain().import_vault(std::path::Path::new(&path)) {
                    Ok(r) => {
                        print_plan(&r.plan);
                        println!(
                            "imported {} pages and {} attachments, {} links rewritten, {} bookmarks added; the report is {}",
                            r.imported_pages,
                            r.imported_attachments,
                            r.links_rewritten,
                            r.bookmarks_added,
                            r.report_slug
                        );
                    }
                    Err(e) => fail(&e),
                }
            }
        }
        "embed" => {
            let (_, flags) = parse_with_bools(rest, &["all"]);
            let Some(embedder) = configured_embedder() else {
                fail("no embedding provider: set embedding_provider to ollama, or to openai with openai_api_key in the secrets file")
            };
            match brain().index_stale(embedder.as_ref(), flags.contains_key("all")) {
                Ok(r) => {
                    println!(
                        "{}: embedded {} pages ({} chunks), removed {}, failed {}",
                        r.model,
                        r.pages_indexed,
                        r.chunks_written,
                        r.pages_removed,
                        r.pages_failed.len()
                    );
                    for f in &r.pages_failed {
                        println!("  failed: {f}");
                    }
                }
                Err(e) => fail(&e),
            }
        }
        "semantic" => {
            let provider = configured_embedder().map(|e| e.id());
            let index = brain().semantic();
            match index.stats() {
                Ok(s) => {
                    println!(
                        "provider: {}",
                        provider
                            .clone()
                            .unwrap_or_else(|| "none (full-text only)".to_string())
                    );
                    println!(
                        "indexed with: {}",
                        s.model.unwrap_or_else(|| "nothing yet".to_string())
                    );
                    println!("pages: {}  chunks: {}  dims: {}", s.pages, s.chunks, s.dims);
                    if let Some(model) = provider {
                        if let Ok((stale, _)) = index.stale_slugs(&model) {
                            println!("waiting: {}", stale.len());
                        }
                    }
                }
                Err(e) => fail(&e),
            }
        }
        "reindex" => match brain().sync_all() {
            Ok(n) => println!("reindexed {n} brain pages from the vault"),
            Err(e) => fail(&e),
        },
        "stats" => match brain().stats() {
            Ok(s) => {
                println!(
                    "vault: {}\npages: {}\nlinks: {}\ntags: {}\ntimeline entries: {}",
                    s.vault_root, s.page_count, s.link_count, s.tag_count, s.timeline_count
                );
                let mut by_type: Vec<_> = s.pages_by_type.into_iter().collect();
                by_type.sort();
                for (t, c) in by_type {
                    println!("  {t}: {c}");
                }
            }
            Err(e) => fail(&e),
        },
        other => {
            eprintln!("unknown brain subcommand: '{other}'\n\n{USAGE}");
            exit(2);
        }
    }
}

/// Open a `SkillsManager` rooted at the configured skills path.
fn skills() -> SkillsManager {
    let db = Arc::new(Database::open().unwrap_or_else(|e| fail(&format!("open database: {e}"))));
    let settings = SettingsManager::new(Arc::clone(&db));
    let mgr = SkillsManager::new(skills::resolve_root(&settings)).with_changes(db);
    mgr.ensure_dirs()
        .unwrap_or_else(|e| fail(&format!("init skills: {e}")));
    mgr
}

/// Dispatch a `skills` subcommand.
fn run_skills(sub: &str, rest: &[String]) {
    match sub {
        "list" => {
            let (_, flags) = parse_with_bools(rest, &["all"]);
            let all = flags.contains_key("all");
            let list = skills().list(all);
            if list.is_empty() {
                println!("(no skills)");
            } else {
                for s in list {
                    println!(
                        "{}\t[{}/{}] {}",
                        s.name,
                        s.status.as_str(),
                        s.origin.as_str(),
                        s.description
                    );
                }
            }
        }
        "view" => {
            let (pos, _) = parse(rest);
            let name = pos.first().unwrap_or_else(|| fail("view: missing <name>"));
            match skills().get(name) {
                Some(s) => println!(
                    "# {} ({}/{})\npath: {}\n\n{}",
                    s.display_name,
                    s.status.as_str(),
                    s.origin.as_str(),
                    s.path,
                    s.body
                ),
                None => fail(&format!("no skill: {name}")),
            }
        }
        "new" => {
            let (pos, flags) = parse_with_bools(rest, &["force"]);
            let name = pos.first().unwrap_or_else(|| {
                fail("new: usage: skills new <name> [--desc <text>] [--body <text>] [--force]")
            });
            let desc = flags.get("desc").cloned().unwrap_or_default();
            let body = flags
                .get("body")
                .cloned()
                .unwrap_or_else(|| "## Procedure\n\n## Pitfalls\n\n## Verification\n".to_string());
            let force = flags.contains_key("force");
            let mgr = skills();
            match mgr.create_skill(name, &desc, &body, force) {
                Ok(s) => {
                    mgr.git_commit_blocking(&format!("skills: add {}", s.name));
                    println!("created {}", s.name);
                }
                Err(e) => fail(&e),
            }
        }
        "rm" => {
            let (pos, _) = parse(rest);
            let name = pos.first().unwrap_or_else(|| fail("rm: missing <name>"));
            let mgr = skills();
            match mgr.delete_skill(name) {
                Ok(()) => {
                    mgr.git_commit_blocking(&format!("skills: remove {name}"));
                    println!("removed {name}");
                }
                Err(e) => fail(&e),
            }
        }
        "path" => println!("{}", skills().active_dir().display()),
        "review" => {
            let mgr = skills();
            let pending: Vec<_> = mgr
                .list(true)
                .into_iter()
                .filter(|s| s.status.as_str() == "pending")
                .collect();
            if pending.is_empty() {
                println!("(no pending skills)");
            } else {
                for s in pending {
                    let flag = match mgr.scan(&s.name) {
                        Ok(f) if f.is_empty() => "clean".to_string(),
                        Ok(f) => format!("FLAGGED: {}", f.join("; ")),
                        Err(e) => format!("scan error: {e}"),
                    };
                    println!("{}\t{}\t[{}]", s.name, s.description, flag);
                }
            }
        }
        "approve" => {
            let (pos, flags) = parse_with_bools(rest, &["force"]);
            let name = pos
                .first()
                .unwrap_or_else(|| fail("approve: missing <name>"));
            let force = flags.contains_key("force");
            let mgr = skills();
            match mgr.approve(name, force) {
                Ok(()) => {
                    mgr.git_commit_blocking(&format!("skills: approve {name}"));
                    println!("approved {name}");
                }
                Err(e) => fail(&e),
            }
        }
        "reject" => {
            let (pos, _) = parse(rest);
            let name = pos
                .first()
                .unwrap_or_else(|| fail("reject: missing <name>"));
            let mgr = skills();
            match mgr.reject(name) {
                Ok(()) => {
                    mgr.git_commit_blocking(&format!("skills: reject {name}"));
                    println!("rejected {name}");
                }
                Err(e) => fail(&e),
            }
        }
        other => {
            eprintln!("unknown skills subcommand: '{other}'\n\n{USAGE}");
            exit(2);
        }
    }
}

/// Split raw args into positionals and `--flag value` pairs (no boolean flags).
fn parse(args: &[String]) -> (Vec<String>, HashMap<String, String>) {
    parse_with_bools(args, &[])
}

/// `rusty-cli export <file.zip> [--include-secrets]`: the whole store in one
/// zip, the database as a consistent snapshot, the secrets only when asked.
fn run_export(rest: &[String]) {
    let (words, flags) = parse_with_bools(rest, &["include-secrets"]);
    if let Some(bad) = flags.keys().find(|k| k.as_str() != "include-secrets") {
        fail(&format!("export: unknown flag --{bad}"));
    }
    let [dest] = words.as_slice() else {
        fail("usage: rusty-cli export <file.zip> [--include-secrets]");
    };
    let home = rusty_core::transfer::StoreLocation::user_home();
    let db_path = home.join("rusty.db");
    if !db_path.is_file() {
        fail(&format!("no store at {}", home.display()));
    }
    let db = Arc::new(
        Database::open_path(&db_path).unwrap_or_else(|e| fail(&format!("open database: {e}"))),
    );
    let settings = SettingsManager::new(Arc::clone(&db));
    let loc = rusty_core::transfer::StoreLocation::resolve(&home, &settings);
    let opts = rusty_core::transfer::ExportOptions {
        include_secrets: flags.contains_key("include-secrets"),
    };
    match rusty_core::transfer::export(&loc, &db, std::path::Path::new(dest), opts) {
        Ok(r) => {
            let c = &r.manifest.counts;
            println!(
                "exported {} ({:.1} MB): {} pages, {} tasks, {} memories, {} files",
                r.path.display(),
                r.bytes as f64 / 1_000_000.0,
                c.pages,
                c.tasks,
                c.memories,
                c.files
            );
            if r.manifest.secrets {
                println!("the zip holds the secrets file and the PIN in the clear; keep it as carefully as this machine (it is mode 600)");
            } else {
                println!("secrets left out (export --include-secrets carries them)");
            }
            for s in &r.manifest.skipped {
                println!("skipped: {s}");
            }
        }
        Err(e) => fail(&format!("export: {e}")),
    }
}

/// `rusty-cli import <file.zip> [--replace] [--dry-run]` (TICKET-057): unpack an export
/// into `~/.rusty`. Refused while a `rusty-mcp` holds that store's database open, since it
/// would keep writing to the store being replaced.
fn run_import(rest: &[String]) {
    let (words, flags) = parse_with_bools(rest, &["replace", "dry-run"]);
    if let Some(bad) = flags
        .keys()
        .find(|k| !matches!(k.as_str(), "replace" | "dry-run"))
    {
        fail(&format!("import: unknown flag --{bad}"));
    }
    let [src] = words.as_slice() else {
        fail("usage: rusty-cli import <file.zip> [--replace] [--dry-run]");
    };
    let opts = rusty_core::transfer::ImportOptions {
        replace: flags.contains_key("replace"),
        dry_run: flags.contains_key("dry-run"),
    };
    let home = rusty_core::transfer::StoreLocation::user_home();
    let running = servers_holding(&home.join("rusty.db"));
    if !running.is_empty() && !opts.dry_run {
        fail(&format!(
            "rusty-mcp holds this store open (pid {}); stop it first (systemctl --user stop rusty-mcp, and quit the agents and editors that start their own), then import",
            running.join(", ")
        ));
    }
    match rusty_core::transfer::import(std::path::Path::new(src), &home, opts) {
        Ok(r) => {
            let m = &r.manifest;
            let c = &m.counts;
            println!(
                "{} {} from Rusty {} ({}): {} pages, {} tasks, {} memories, {} files, secrets {}",
                if opts.dry_run {
                    "would import"
                } else {
                    "imported"
                },
                src,
                m.rusty_version,
                m.created,
                c.pages,
                c.tasks,
                c.memories,
                r.files,
                if m.secrets {
                    "included"
                } else {
                    "not included"
                }
            );
            if opts.dry_run {
                if r.held_store && !opts.replace {
                    println!(
                        "{} holds a store already: the import would refuse without --replace",
                        home.display()
                    );
                } else if r.held_store {
                    println!(
                        "{} holds a store already: it would be moved aside, whole",
                        home.display()
                    );
                }
                if !running.is_empty() {
                    println!(
                        "rusty-mcp holds this store open (pid {}): stop it before the real import",
                        running.join(", ")
                    );
                }
                return;
            }
            println!("into {}", r.home.display());
            if let Some(aside) = &r.moved_aside {
                println!("the old store is at {}", aside.display());
            }
            for s in &r.settings {
                println!("setting {s}");
            }
            if r.hooks_carried {
                println!("the Claude Code hook scripts came across from the old store");
            }
            println!("next: systemctl --user start rusty-mcp (or rusty session start)");
        }
        Err(e) => fail(&format!("import: {e}")),
    }
}

/// The pids of the `rusty-mcp` processes that hold the database at `db` open, from
/// `/proc`: those would keep writing to the store an import replaces.
fn servers_holding(db: &std::path::Path) -> Vec<String> {
    let Ok(db) = db.canonicalize() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids: Vec<u32> = entries
        .flatten()
        .filter_map(|e| {
            let pid: u32 = e.file_name().to_str()?.parse().ok()?;
            let comm = std::fs::read_to_string(e.path().join("comm")).ok()?;
            if comm.trim() != "rusty-mcp" {
                return None;
            }
            let holds = std::fs::read_dir(e.path().join("fd"))
                .ok()?
                .flatten()
                .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|t| t == db));
            holds.then_some(pid)
        })
        .collect();
    pids.sort_unstable();
    pids.into_iter().map(|p| p.to_string()).collect()
}

/// Split raw args into positionals and flags, `--flag value` or `--flag=value`. Flags
/// named in `bool_flags` are switches (recorded with an empty value) and do not take the
/// next word, so `--force <name>` keeps `<name>` as a positional. A flag the usage text
/// does not name, a value flag without its value, or a count that is not a number ends
/// the command with exit 2, so a mistyped flag never runs as if it were absent.
fn parse_with_bools(
    args: &[String],
    bool_flags: &[&str],
) -> (Vec<String>, HashMap<String, String>) {
    let mut pos = Vec::new();
    let mut flags = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let Some(raw) = args[i].strip_prefix("--") else {
            pos.push(args[i].clone());
            i += 1;
            continue;
        };
        let (name, inline) = match raw.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (raw, None),
        };
        if !known_flag(name) {
            usage_error(&format!("unknown flag --{name}"));
        }
        let value = if bool_flags.contains(&name) {
            if inline.is_some() {
                usage_error(&format!("--{name} takes no value"));
            }
            i += 1;
            String::new()
        } else if let Some(value) = inline {
            i += 1;
            value
        } else {
            let Some(value) = args.get(i + 1) else {
                usage_error(&format!("--{name} needs a value"));
            };
            i += 2;
            value.clone()
        };
        if NUMERIC_FLAGS.contains(&name) && value.parse::<i64>().is_err() {
            usage_error(&format!("--{name} takes a number, not {value:?}"));
        }
        flags.insert(name.to_string(), value);
    }
    (pos, flags)
}

/// The flags whose value is a count or a cursor.
const NUMERIC_FLAGS: [&str; 3] = ["limit", "days", "since"];

/// Whether the usage text names `--<name>`; the text is the list of every flag there is.
fn known_flag(name: &str) -> bool {
    let flag = format!("--{name}");
    USAGE.match_indices(&flag).any(|(at, _)| {
        !USAGE[at + flag.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '-')
    })
}

/// A mistake in how the command was called: the reason on stderr, exit 2.
fn usage_error(msg: &str) -> ! {
    eprintln!("error: {msg} (see rusty-cli --help)");
    exit(2);
}

/// `rusty-cli settings list|get <key>|set <key> <value>`: the settings, through the same
/// manager and rules as the `settings_list`, `setting_get` and `setting_set` tools, so a
/// change is recorded in the change log like any other.
fn run_settings(sub: Option<&str>, rest: &[String]) {
    use rusty_core::engine::settings_manager::refuse_the_mask;
    use rusty_core::transfer::{check_path_setting, StoreLocation};
    let db = Arc::new(Database::open().unwrap_or_else(|e| fail(&format!("open database: {e}"))));
    let settings = SettingsManager::new(db);
    match (sub, rest) {
        (Some("list") | None, []) => {
            for (key, value) in settings.list_masked().unwrap_or_else(|e| fail(&e)) {
                println!("{key} = {value}");
            }
        }
        (Some("get"), [key]) => match settings.get_masked(key).unwrap_or_else(|e| fail(&e)) {
            Some(value) => println!("{value}"),
            None => fail(&format!("{key} is not set")),
        },
        (Some("set"), [key, value]) => {
            let value = refuse_the_mask(key, value)
                .and_then(|()| check_path_setting(key, value, &StoreLocation::user_home()))
                .unwrap_or_else(|e| fail(&e));
            settings.set(key, &value).unwrap_or_else(|e| fail(&e));
            println!("{key} set");
        }
        _ => usage_error("usage: rusty-cli settings list | get <key> | set <key> <value>"),
    }
}

/// Today's date as `YYYY-MM-DD` in local time.
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// `rusty-cli bookmarks`, `bookmarks add <path>`, `bookmarks rm <path>`: the vault's
/// bookmarks (TICKET-037). A `search:` prefix makes a search bookmark; a path that is a
/// folder makes a folder bookmark; anything else is a page.
fn run_bookmarks(sub: Option<&str>, rest: &[String]) {
    use rusty_core::brain::bookmarks::Bookmark;
    let bm = brain();
    let target = |rest: &[String]| -> Bookmark {
        let arg = rest
            .first()
            .cloned()
            .unwrap_or_else(|| fail("bookmarks: missing <path>"));
        match arg.strip_prefix("search:") {
            Some(query) => Bookmark {
                kind: "search".into(),
                query: query.to_string(),
                ..Default::default()
            },
            None => Bookmark {
                kind: if bm.vault_root().join(&arg).is_dir() {
                    "folder".into()
                } else {
                    "file".into()
                },
                path: arg.trim_end_matches(".md").to_string(),
                ..Default::default()
            },
        }
    };
    let result = match sub {
        None | Some("list") => bm.bookmarks(),
        Some("add") => bm.add_bookmark(target(rest)),
        Some("rm") | Some("remove") => bm.remove_bookmark(&target(rest)),
        Some(other) => fail(&format!("bookmarks: unknown subcommand '{other}'")),
    };
    bm.flush_commits();
    match result {
        Ok(list) => {
            for b in &list {
                let what = match b.kind.as_str() {
                    "search" => &b.query,
                    _ => &b.path,
                };
                let heading = if b.heading.is_empty() {
                    String::new()
                } else {
                    format!("#{}", b.heading)
                };
                println!("{:<8} {what}{heading}  ({})", b.kind, b.title);
            }
        }
        Err(e) => fail(&e),
    }
}

/// `rusty-cli changes [--since N] [--limit N]`: the change log after a cursor, one line
/// per change, then the cursor to pass next (TICKET-035).
fn run_changes(rest: &[String]) {
    let (_, flags) = parse(rest);
    let number = |name: &str| flags.get(name).and_then(|v| v.parse::<i64>().ok());
    let db = Database::open().unwrap_or_else(|e| fail(&format!("open database: {e}")));
    match rusty_core::engine::changes::since(
        &db,
        number("since"),
        number("limit").map(|n| n.max(1) as usize),
    ) {
        Ok(batch) => {
            for c in &batch.changes {
                let detail = if c.detail.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", c.detail)
                };
                println!("{}  {}  {} {}{detail}", c.seq, c.op, c.kind, c.key);
            }
            if batch.reset {
                println!("reset: the log no longer reaches back that far; re-read everything");
            }
            println!(
                "cursor: {}{}",
                batch.cursor,
                if batch.more { " (more)" } else { "" }
            );
        }
        Err(e) => fail(&e),
    }
}

/// Touch the sentinel file the MCP server watches, so its clients hear about a terminal
/// data change.
fn refresh_signal() {
    let sentinel = dirs::home_dir()
        .unwrap_or_default()
        .join(".rusty")
        .join(".changed");
    match std::fs::write(&sentinel, chrono::Local::now().to_rfc3339()) {
        Ok(()) => println!("refresh signal sent"),
        Err(e) => fail(&format!("refresh: {e}")),
    }
}

/// Print an error to stderr and exit with status 1.
fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    exit(1);
}

/// `rusty-cli scripts ...`: the store's scripts as commands (TICKET-010).
fn run_scripts(sub: &str, rest: &[String]) {
    let mgr = skills();
    match sub {
        "list" => {
            let (_, flags) = parse_with_bools(rest, &["all"]);
            let list = mgr.scripts(flags.contains_key("all"));
            if list.is_empty() {
                println!("(no scripts)");
            } else {
                for s in list {
                    println!(
                        "{}\t[{}] {}/{}.sh{}",
                        s.name,
                        s.status,
                        s.skill,
                        s.name,
                        if s.executable {
                            ""
                        } else {
                            "  (not executable)"
                        }
                    );
                }
            }
        }
        "view" => {
            let (pos, _) = parse(rest);
            let name = pos.first().unwrap_or_else(|| fail("view: missing <name>"));
            match mgr.script_text(name) {
                Ok((s, text)) => print!("# {} ({}, {})\n{}", s.name, s.skill, s.status, text),
                Err(e) => fail(&e),
            }
        }
        "path" => {
            let (pos, _) = parse(rest);
            let name = pos.first().unwrap_or_else(|| fail("path: missing <name>"));
            match mgr.resolve_script(name) {
                Ok(s) => println!("{}", s.path),
                Err(e) => fail(&e),
            }
        }
        "edit" => {
            let (pos, _) = parse(rest);
            let name = pos.first().unwrap_or_else(|| fail("edit: missing <name>"));
            let script = mgr.resolve_script(name).unwrap_or_else(|e| fail(&e));
            let editor = std::env::var("VISUAL")
                .or_else(|_| std::env::var("EDITOR"))
                .unwrap_or_else(|_| "vi".to_string());
            let status = std::process::Command::new(&editor)
                .arg(&script.path)
                .status()
                .unwrap_or_else(|e| fail(&format!("start {editor}: {e}")));
            if status.success() {
                mgr.git_commit_blocking(&format!("scripts: edit {}", script.name));
            }
        }
        "new" => {
            let (pos, flags) = parse_with_bools(rest, &["force"]);
            let name = pos.first().unwrap_or_else(|| {
                fail("new: usage: scripts new <name> [--skill <skill>] [--body <text>] [--force]")
            });
            match mgr.create_script(
                name,
                flags.get("skill").map(String::as_str),
                flags.get("body").map(String::as_str),
                flags.contains_key("force"),
            ) {
                Ok(s) => {
                    mgr.git_commit_blocking(&format!("scripts: add {}/{}", s.skill, s.name));
                    println!("created {} ({})", s.path, s.status);
                }
                Err(e) => fail(&e),
            }
        }
        "rm" => {
            let (pos, _) = parse(rest);
            let name = pos.first().unwrap_or_else(|| fail("rm: missing <name>"));
            match mgr.delete_script(name) {
                Ok(s) => {
                    mgr.git_commit_blocking(&format!("scripts: remove {}/{}", s.skill, s.name));
                    println!("removed {}", s.path);
                }
                Err(e) => fail(&e),
            }
        }
        "run" => {
            let name = rest.first().unwrap_or_else(|| fail("run: missing <name>"));
            let script_args: Vec<String> = rest.iter().skip(1).cloned().collect();
            if let Err(e) = mgr.exec_script(name, &script_args) {
                fail(&e);
            }
        }
        other => fail(&format!("unknown scripts command: {other}")),
    }
}

/// `rusty-cli hooks install|uninstall|status`: the brain loop's Claude Code hooks.
fn run_hooks(sub: Option<&str>) {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/"));
    match sub {
        Some("install") => match hooks::install(&home) {
            Ok(r) => println!(
                "hooks: {} script(s) in {}, {} entr{} added to {}",
                r.scripts_written,
                hooks::hooks_dir(&home).display(),
                r.entries_added,
                if r.entries_added == 1 { "y" } else { "ies" },
                hooks::settings_path(&home).display()
            ),
            Err(e) => {
                eprintln!("hooks install: {e}");
                std::process::exit(1);
            }
        },
        Some("uninstall") => match hooks::uninstall(&home) {
            Ok(r) => println!(
                "hooks: {} entr{} removed, {} script(s) removed",
                r.entries_removed,
                if r.entries_removed == 1 { "y" } else { "ies" },
                r.scripts_removed
            ),
            Err(e) => {
                eprintln!("hooks uninstall: {e}");
                std::process::exit(1);
            }
        },
        Some("status") | None => {
            let s = hooks::status(&home);
            let word = |b: bool| if b { "yes" } else { "no" };
            println!(
                "{}: script {}, wired {}",
                hooks::ASK_HOOK_NAME,
                word(s.ask_script),
                word(s.ask_wired)
            );
            println!(
                "{}: script {}, wired {}",
                hooks::STOP_HOOK_NAME,
                word(s.stop_script),
                word(s.stop_wired)
            );
        }
        Some(other) => {
            eprintln!("unknown hooks command: {other}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{known_flag, parse, parse_with_bools, USAGE};

    /// `docs/cli.md` shows every command line of the usage text exactly as `--help` prints
    /// it, so the reference cannot fall behind the CLI.
    #[test]
    fn the_cli_reference_lists_every_usage_line() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/cli.md");
        let doc = std::fs::read_to_string(&path).unwrap();
        for line in USAGE
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("rusty-cli "))
        {
            let command = line.split("  (").next().unwrap().trim_end();
            assert!(
                doc.contains(command),
                "docs/cli.md does not show `{command}`"
            );
        }
    }

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_pairs_value_flags() {
        let (pos, flags) = parse(&argv(&["a", "--type", "v", "b"]));
        assert_eq!(pos, vec!["a", "b"]);
        assert_eq!(flags.get("type").map(String::as_str), Some("v"));
        let (pos, flags) = parse(&argv(&["alpha", "--limit=1", "beta"]));
        assert_eq!(pos, vec!["alpha", "beta"]);
        assert_eq!(flags.get("limit").map(String::as_str), Some("1"));
    }

    /// The flags a command accepts are the ones the usage text names: a mistyped one is
    /// unknown (the parser then exits 2), a prefix of a real one is not a match.
    #[test]
    fn only_the_flags_the_usage_names_are_known() {
        for flag in [
            "limit",
            "content",
            "dry-run",
            "include-secrets",
            "follow-up-by",
            "since",
        ] {
            assert!(known_flag(flag), "{flag}");
        }
        for flag in ["contents", "lim", "dry", "include", "k"] {
            assert!(!known_flag(flag), "{flag}");
        }
    }

    #[test]
    fn bool_flag_does_not_eat_next_token() {
        let (pos, flags) = parse_with_bools(&argv(&["--force", "my-skill"]), &["force"]);
        assert_eq!(pos, vec!["my-skill"]);
        assert_eq!(flags.get("force").map(String::as_str), Some(""));
    }

    #[test]
    fn bool_flag_mixed_with_value_flag() {
        let (pos, flags) =
            parse_with_bools(&argv(&["--force", "--desc", "hello", "name"]), &["force"]);
        assert_eq!(pos, vec!["name"]);
        assert!(flags.contains_key("force"));
        assert_eq!(flags.get("desc").map(String::as_str), Some("hello"));
    }
}
