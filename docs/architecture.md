# Architecture

*Set on 2026-09-02. A plan, not a spec; ROADMAP.md carries the sequence.*

## The three decisions

1. **The web UI goes.** React, Vite, the axum server, the password gate, TLS and the
   PTY-over-WebSocket transport are retired. The front end is Qt Quick (QML).
2. **The back end is a pure MCP server.** One `rusty-mcp` process owns tasks, notes,
   memories, the brain index, skills and settings, and serves both the QML app and the
   Claude and Codex sessions.
3. **A tabbed QML app with native terminals.** The same tabs the web UI had (Tasks, Chat,
   Brain, Notes, Memory, Skills, Secrets, Settings), where Chat is one or more real
   terminals running `claude` or `codex`, as in Alacritty.

## What Obsidian offers (checked 2026-09-02)

- **Official CLI, shipped February 2026.** `obsidian <command> key=value`, available on Linux
  as `~/.local/bin/obsidian` from Obsidian 1.12.4; this box runs 1.13.7. It talks to the
  running app over a local socket, and the first command launches the app if it is not
  running. Commands cover read, create, append, search (`format=json`), properties, tags,
  links and backlinks, tasks, daily notes, templates, bookmarks, plugins, and running any
  command-palette action by id. Vault selection is `vault=<name>`. Moving a file through
  it rewrites wikilinks. A separate "Obsidian Headless" exists for sync only.
- **No official MCP server.** The ecosystem is the community Local REST API plugin (HTTPS on
  127.0.0.1:27124 with an API key) plus several MCP wrappers around it, and two plugins that
  run an MCP server inside the app. All of them need the app open.
- **A vault is a folder of markdown.** Frontmatter, `[[wikilinks]]`, folders. Rusty's brain
  is already that: 237 pages under `~/.rusty/brain`, frontmatter with `title`, `type`,
  `created`, `updated`, wikilinks in 65 pages. Obsidian can open it today; it would add a
  `.obsidian/` folder for its own settings.

So "Obsidian as the back end" resolves to: **the vault files are the back end**, Obsidian is
the human editor, graph and (with Sync) the phone, and Rusty keeps its own index so the
agents can search and write when Obsidian is closed. Where the app is open, Rusty can lean
on the CLI for the things only the app knows well: opening a note in the editor, link
rewrites on rename, backlinks as Obsidian resolves them, command-palette actions.

## Proposed shape

```
 ┌──────────────────────── rusty (QML, cxx-qt) ─────────────────────────┐
 │ Tasks │ Terminals (claude, codex) │ Brain │ Notes │ Memory │ Skills │ … │
 └───────────────┬───────────────────────────────┬──────────────────────┘
                 │ MCP client (rmcp, Streamable HTTP, localhost)          │ qmltermwidget
                 ▼                                                        ▼ PTY
        rusty-mcp (rmcp server: stdio for agents, HTTP for the app)   tmux sessions
                 │ files + SQLite index + file watcher                    │ claude / codex
                 ▼                                                        ▼ --mcp-config → rusty-mcp
        ~/.rusty/brain (Obsidian vault) · notes · rusty.db · skills · .secret
                 ▲
        Obsidian app (editor, graph, mobile via Sync) · `obsidian` CLI when open
```

- **Back end.** Rebuild `rusty-mcp` on `rmcp` (the official Rust MCP SDK, 3.2 on
  crates.io) with two transports: stdio, so a Claude or Codex session spawns it as today,
  and Streamable HTTP on localhost, so the app and every terminal session share one live
  process and one file watcher. Resources for pages, notes and tasks; tools for the writes;
  `resources/updated` notifications replace the old WebSocket event bus. The existing
  managers in `rusty_lib` carry over; `web.rs`, `auth.rs`, `tls.rs` and the frontend go.
- **Front end.** QML on Qt 6 with `cxx-qt` exposing Rust models: task list, note tree, page
  search results, memory list, skills catalog, settings. The Brain tab is search plus a
  rendered read view (QML `TextArea` renders markdown; anything richer opens in Obsidian
  through the CLI). Theme colours come from the current theme's `colors.toml`, under
  `~/.local/state/omarchy/current/theme/` on Omarchy 4 and `~/.config/omarchy/current/theme/`
  before it.
- **Terminals.** `qmltermwidget` (Konsole's emulator, Qt 6, in Arch extra) per terminal tab.
  Each tab attaches to a tmux session (`tmux new -A -s rusty-<name> claude`), so sessions
  survive an app restart and are reachable from any terminal or over SSH, the way the
  `ssh_mac` and `ssh_ai` wrappers already work. That retires `ignibyte-bridge` from Rusty's
  hot path; the bridge stays a separate tool. Codex is the same tab with a different command.
- **Obsidian.** Register `~/.rusty/brain` as a vault, commit `.obsidian/` or gitignore it
  (decide), keep Rusty's brain writer producing Obsidian-clean markdown, and add a handful
  of MCP tools that shell out to the CLI when the app is open: `open_in_obsidian`,
  `rename_page` (link-safe), `backlinks`.

## What has to change in the vault rules

- Rusty treats a body `---` as the Timeline delimiter (brain rule 1 in
  the v2 vault notes). Obsidian users type `---` for a horizontal rule. v3 should
  store the timeline as a normal `## Timeline` section and drop the delimiter trick.
- Wikilinks: Rusty uses `[[folder/slug]]`; Obsidian resolves both bare names and paths, but
  the "shortest path when possible" default would write `[[slug]]`. Set the vault's link
  format to relative or absolute path so both writers agree.

## Phases

1. **Prototype the two unknowns in a day**, in Python: PySide6 window, `qmltermwidget`
   running `claude` inside tmux, one placeholder data tab, Omarchy theme colours. Install
   `qt6-wayland` first. This settles terminal quality and the tmux attach flow before any
   Rust is written.
2. **Back end first.** `rusty-mcp` on `rmcp` with both transports, resources and
   notifications, plus the CLI-backed Obsidian tools. The old server keeps running until
   the app replaces it. Gates stay: fmt, clippy, tests, docs.
3. **The app**, one tab at a time, starting with Terminals and Tasks. `.desktop` entry, icon,
   `app_id` for Hyprland rules, `omarchy launch or focus rusty`, a key.
4. **Retire the web stack** and the launchd-era scripts; update `docs/ops` and CLAUDE.md.

## Open decisions

- tmux as the session substrate (proposed) versus the bridge or plain PTYs.
- cxx-qt in Rust for the app versus PySide6 for good; the prototype will show whether
  Python is enough for the models, and Rust wins if the MCP client and the app share code.
- Where secrets live once the Secrets tab is native: keep `~/.rusty/.secret` as is.
- Whether the Mac gets the app at all (Qt builds fine there) or stays a Codex and browser box.

## Decided on 2026-09-02

- Obsidian is reached through its official CLI, never through its plugin API or a community REST
  server. `rusty_core::obsidian` wraps it; the app stays optional. (Retired on 2026-09-03; see
  the next bullet.)
- The vault is registered by writing Obsidian's config file (vault entry plus `cli: true`) while the
  app is closed. Opening an unregistered folder through an `obsidian://open?path=` URL left the app
  in its picker for five minutes on the box, so that route is out.
- `.obsidian/` is per-machine viewer state and is gitignored in the vault.
- Retired on 2026-09-03 (TICKET-006): the CLI bridge (`rusty_core::obsidian`, the six
  `obsidian_*` tools, `rusty-cli obsidian`, the installer's registration, the app's theme-snippet
  call) went once the workspace tiers covered links, unresolved targets, renames and opening
  pages. The vault stays an Obsidian vault by format.
- Skinned on 2026-09-03 (TICKET-008): the look is data in `crates/rusty-app/src/skin.rs`, a set
  of colour roles from a preset, the Omarchy theme or a file under `~/.config/rusty/themes/`;
  `Theme` turns the roles into every token the QML and the renderer bind to, `Desk` reads the
  machine for the top bar, and the application font is the skin's face. Since TICKET-012
  the theme also carries a base text size (`baseSize`, 12 to 18, default 14) and its
  `scale` over the drawn body size of 12; every QML label and the reading view's render
  style derive from it, the terminals keep the Alacritty font.
- Session-bound on 2026-09-03 (TICKET-009): the app runs as `rusty-app.service`, wanted by
  uwsm's `graphical-session.target`, restarted when it is killed and left alone when it is
  quit; the back end restarts after any exit but a stop; `rusty-session` was the one entry
  point the installer, the desktop entry and the key shared; the compositor's OOM score is a
  drop-in the installer points at, at 100, the floor a user unit can reach.
- Agent sessions on 2026-09-17 (TICKET-031): every conversation is its own transient user
  unit under the user manager, not a child of `rusty-app.service`; a stop or a crash
  restart of the app leaves it running, and the app reattaches to its socket and replays
  its log. The terminal tabs' tmux server is still the app unit's child, which TICKET-034
  takes.
- Commands as nouns on 2026-09-05 (TICKET-029): the app binary answers `rusty <noun> <verb>`
  before Qt starts (`crates/rusty-app/src/session.rs`); `session` (`start`, `stop`, `status`,
  `run`) replaces the `rusty-session` script and the app unit runs `rusty session run`;
  built-in nouns come before store scripts, an unknown bare word prints the usage and exits
  2, and dash-prefixed arguments stay Qt's. The same ticket moved the theme directory to
  Omarchy 4's `~/.local/state/omarchy/current/theme`, the Omarchy 3 path as the fallback.
- Vault rules, applied by `rusty-cli brain migrate`: the timeline is a `## Timeline` section (the
  bare `---` rule is read for compatibility, never written), and wikilinks are vault paths,
  `[[projects/orbit]]`, which is also what Obsidian now writes (`newLinkFormat: absolute`).
  Bare names that named a page's title or alias became `[[folder/slug|Name]]`, so prose reads as
  before; targets that matched nothing were left alone and listed.

## As built (2026-09-02)

- `crates/rusty-core`: the managers (tasks, notes, memories, brain vault and index, skills,
  secrets, settings), the semantic index (`brain::semantic`: chunks,
  providers, sqlite-vec, fusion), the vault migration, the file watcher and the event bus.
- `crates/rusty-mcp`: 85 tools (2026-09-03: six of them the PIN and the reveal behind the
  Secrets tab since TICKET-015, five the brain loop since TICKET-018, four the store's
  scripts since TICKET-010, two the Obsidian vault import since TICKET-026, three the sources since TICKET-027), five resources plus templates, `list_changed` notifications,
  a background indexer for embeddings; stdio for agents, Streamable HTTP for the app.
- `crates/rusty-app`: cxx-qt 0.10 on Qt 6. Rust types exposed to QML: `Theme` (Omarchy
  colours and the tokens read from the theme's `obsidian.css` and Alacritty palette, font,
  generated colour scheme, live re-theme), `Terminals` (tabs and the workspace state as JSON
  files, tmux), `Backend` (the MCP client, one session, reconnecting, `result` and
  `dataChanged` signals), `Assistant` (the client of an agent session: the process itself
  lives in a host of its own since TICKET-031, and this type attaches to it, replays the
  conversation and writes the pane's messages, answers and control requests; one instance
  per surface), `Agents` (the sessions on the machine, watched as the state directory
  changes), and two small C++ classes registered in the same QML module:
  `MarkdownHighlighter` (a `QSyntaxHighlighter` whose spans come from the Rust tokenizer in
  `src/markdown.rs`) and `Tools` (`grabWindow`, for offscreen screenshots). QML pages parse
  the tool JSON themselves and match replies to their own request ids.
- Agent sessions (2026-09-17, TICKET-031): a conversation with Claude Code is a session of
  its own, not a child of the window. `rusty agent start` mints an id, writes an entry
  under `~/.local/state/rusty/agents/` and runs `systemd-run --user --unit=rusty-agent-<id>
  … rusty agent host --id <id>`: a transient user unit in `app.slice`, so the session is
  in no cgroup the app can take down, has its own journal (`journalctl --user -u
  rusty-agent-<id>`), is restarted after a crash and stays stopped after a stop. The host
  (`src/agent/host.rs`) owns the one `claude -p` process over stream-json, appends every
  line it forwards or receives to `<id>/events.jsonl`, and serves clients over
  `$XDG_RUNTIME_DIR/rusty/agents/<id>.sock`: a client attaches with the sequence number it
  has, the log after it is replayed, then the stream is live. Any number of clients may
  attach, a client may leave while a turn runs, and the host raises a desktop notification
  when a permission is asked or a turn ends with nobody watching. The process is respawned
  with `--resume` on the next message after an exit, stopped when the session sits idle
  (the note pane's sessions after ten minutes) and ended in order — interrupt, stdin
  closed, SIGTERM, SIGKILL — when the session stops or the unit is told to. The deltas of
  a block are broadcast but never logged: the `assistant` line that follows carries the
  whole block, so a replay loses nothing. `rusty agent list|attach|stop|rm` is the same
  client in a terminal. The terminal tabs stay tmux terminals.
- The workspace (2026-09-02, TICKET-002): `qml/Main.qml` lays the window out as Obsidian
  does (ribbon, left sidebar with `Explorer`, `SearchPane` and `BookmarksPane`, tab strip and a stack of
  `TabHost`s, right sidebar `RightPane`, status bar) with `QuickSwitcher` and
  `CommandPalette` overlays. Every tab is one kind: `page` (`NoteTab`: view header, inline
  title, properties, the reading view as rich-text blocks split at top-level headings, the
  source editor with autosave), `terminal` (`AgentTerminal`), or a built-in view. Reading
  view HTML comes from `brain_render`; the renderer lives in `rusty-core::brain::render`
  and inlines the theme's colours because rich text has no stylesheet. Links carry the
  `rusty:` scheme (`page/`, `new/`, `task/`, `tag/`), which the note routes.
- Vault rules since the workspace: a file without frontmatter is a page (title from the
  file name, type from the top folder or `note`); pages may live in any folder; a rename or
  move rewrites `[[links]]` and `](links.md)` in every page except inside fenced code, and
  the index rows follow; deletes are soft (`archive/`). Link rows in `brain_links` hold the
  resolved slug (or the raw target when nothing matches) and the line the link sits on.
- Folders (2026-09-03, TICKET-016, part one): the explorer lists folders from the machine
  below the vault as roots kept per machine in the workspace state (`roots`), read through
  a small `Folders` type in the app (list, kind, text, base name, open outside), never
  through the back end; files open read-only in a `file` tab (markdown rendered by
  `brain_render` given the text, text with line numbers, images fitted), and a folder's
  menu opens an agent or a shell there. The vault's links, graph and search do not see the
  roots. File operations and git decorations are TICKET-019 and TICKET-020.
- The brain loop (2026-09-03, TICKET-018): a `decision` page type under `decisions/`,
  the consultation record `brain_consultations` in `rusty.db`, the tools `brain_ask`,
  `brain_decide`, `brain_follow_up`, `brain_no_decision` and `brain_due`, typed edges in
  `brain_graph` (`consulted`, `supersedes`, `follows_up`) drawn dashed in the app behind a
  filter, a Decisions view, the CLI mirror, the seed skill `ask-decide-follow-up`, and two
  Claude Code hooks shipped inside `rusty-cli` (`hooks install|uninstall|status`) that read
  the transcript: the first write waits for a `brain_ask`, a stop after writes waits once
  for a `brain_decide` or a `brain_no_decision`. The ADR: `docs/architecture/brain-loop.md`.
- Scripts as commands (2026-09-03, TICKET-010): a `*.sh` beside a skill in the store is
  `rusty <name>`; the app binary checks the store before Qt starts and execs `rusty-cli
  scripts run`, whose resolver (in `rusty-core::skills::scripts`) names clashes as
  `skill/name`, refuses a pending skill's script, and execs the file; `rusty-cli scripts
  list|view|new|rm|path|edit|run`, the tools `script_list`, `script_view`, `script_update`
  and `script_run` (approved only, capped at sixty seconds), and a Scripts section of the
  Skills tab whose Run opens a terminal tab (`run:<path>`) that keeps a shell after.
- `crates/rusty-cli`: the terminal counterpart, including `brain migrate`, `brain embed`,
  `brain semantic`, and `notes adopt` (2026-09-03, TICKET-014: the notes folder lives in
  the vault under `notes/`, and the one-shot moves an older `~/.rusty/notes` in).
- `omarchy/`: installer, the two user services (back end and app; the app unit runs
  `rusty session run`), the compositor drop-in, desktop entry and icon, key binding snippet, MCP config
  snippet.
