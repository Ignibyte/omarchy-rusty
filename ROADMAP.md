# Rusty v3 roadmap

The tracker for the rewrite: a QML desktop app for [Omarchy](https://omarchy.org) with a
pure MCP back end, an Obsidian-compatible markdown brain, to-do lists, notes, memories,
editable skills, and native Claude Code and Codex terminals. Tick items as they land; when
something unplanned turns up, add it to the milestone it belongs to rather than a side list.
The shape is in `docs/architecture.md`.

## Where we are

| Milestone | State |
|---|---|
| M0 Foundation | done 2026-09-02 |
| M1 The back end, complete | done 2026-09-02 |
| M2 App shell, Terminals, Tasks | done 2026-09-02 |
| M3 Knowledge tabs: Brain, Notes, Memory | done 2026-09-02 |
| M4 Semantic search | done 2026-09-02 |
| M5 Skills, Secrets, Settings | done 2026-09-02 |
| M6 Omarchy packaging and cutover | next (waits on Chad) |
| M7 Daily use | done 2026-09-02 |
| M8 Knowledge workspace | in progress (shell landed 2026-09-02) |
| M9 Workflow | in progress |

## M0. Foundation (done 2026-09-02)

- [x] Public repo `Ignibyte/rusty`, MIT, Cargo workspace, clean history
- [x] `rusty-core` lifted from v2: tasks, notes, memories, brain vault and index, skills,
      secrets, settings, events, file watcher; web, auth, TLS and PTY code left behind
- [x] Gates green: rustfmt, clippy with warnings as errors, 190 tests, docs
- [x] `rusty-mcp` on rmcp 3: 19 tools over stdio, verified against a real store
- [x] `rusty-mcp --http`: Streamable HTTP at `127.0.0.1:4174/mcp`, verified with curl
- [x] Prototype: PySide6 + qmltermwidget, side rail, Claude and Codex tabs on tmux
      sessions, Omarchy theme colours and font, Ctrl+PgUp/PgDn tab cycling
- [x] Decision: agent terminals are always embedded; the Alacritty-window mode was tried
      and removed. The Settings page stays as a page.

## M1. The back end, complete (done 2026-09-02)

Done when every v2 GUI action has an MCP equivalent, the dev box's own `.mcp.json`
points at v3, and the v2 server is not needed.

Tools and resources

- [x] Tasks: rename and delete a list, update a task title, unarchive, delete
- [x] Tasks: reorder (`reorder_tasks`)
- [x] Notes: create, rename, delete
- [x] Notes: daily note open-or-create (`brain_daily_note`, the daily page in the vault)
- [x] Memories: delete
- [x] Memories: update (`update_memory`)
- [x] Brain: read timeline, links and backlinks, set page body, delete page, resolve a
      partial slug
- [x] Brain: capture into the daily page or the inbox, page types listed (`brain_capture`,
      `brain_page_types`)
- [x] Skills: create (active or staged), approve, reject, delete, safety scan on demand
- [x] Skills: update description and body in place, other keys kept (`skill_update`)
- [x] Secrets: list names, set, delete; values are never returned
- [x] Settings: get, set
- [x] Settings: list, credential-looking values masked (`settings_list`)
- [x] Resources: `rusty://tasks`, `memories`, `skills`, `notes`, `brain` and the templates
      `tasks/{group_id}`, `brain/{slug}`, `notes/{path}`; `resources/list_changed` fires on
      every tool mutation and every watched-file change (verified over HTTP)
- [x] Router tests: every tool advertised once, every tool described
- [x] An rmcp-client smoke test that runs in CI (`crates/rusty-mcp/tests/smoke.rs`,
      `.github/workflows/ci.yml`)

Transport and services

- [x] A `systemd --user` unit for `rusty-mcp --http` (`omarchy/rusty-mcp.service`; running on
      the dev box)
- [x] The installer enables it (`omarchy/install.sh`: binaries, unit, Obsidian registration)
- [x] MCP config snippets for Claude Code and Codex (`omarchy/mcp-config.json`)
- [x] `rusty-cli` ported onto `rusty-core`, installed over the v2 binary

Obsidian and the vault

- [x] Obsidian CLI bridge: `open_in_obsidian`, link-safe `rename_page`, `backlinks`;
      skip cleanly when the app is not running
- [x] Vault rule: the timeline is a `## Timeline` section; the bare `---` rule is only read,
      never written. `rusty-cli brain migrate` converted the dev box's 22 legacy pages on
      2026-09-02
- [x] Vault rule: one wikilink path style that Obsidian and Rusty both write: `[[folder/slug]]`;
      the migration rewrote 161 bare-name links, leaving 14 it could not resolve as written
      (Obsidian's side is set on 2026-09-02: vault-path wikilinks, `newLinkFormat: absolute`,
      `useMarkdownLinks: false`; Rusty's writers already use `[[folder/slug]]`)
- [x] Register `~/.rusty/brain` as an Obsidian vault; decide the `.obsidian/` policy
      (2026-09-02: `rusty-cli obsidian register` writes the vault and the CLI toggle into
      Obsidian's config; `.obsidian/` is gitignored in the vault, so it stays per machine)

Cutover for the back end

- [x] Parity check against v2's tool list: all 29 present
- [x] Switch the dev box's `.mcp.json` to v3 and retire the v2 `rusty-mcp`

## M2. App shell, Terminals, Tasks (done 2026-09-02)

Done when the app replaces a terminal for daily Claude and Codex use.

- [x] `rusty-app` crate: cxx-qt 0.10 on Qt 6.11, one window, side rail, tab stack; the
      prototype's QML ported, `Theme` served from Rust (binary `rusty`, app_id
      `com.ignibyte.rusty`)
- [x] MCP client over Streamable HTTP: the `Backend` type keeps one session on a tokio
      runtime, reconnects every 3s, forwards tool calls and answers as JSON through a
      `result` signal, and turns `resources/list_changed` into `dataChanged` (the tabs parse
      the JSON in QML; typed Rust models can come when a tab needs one)
- [x] Terminals: qmltermwidget per tab, one tmux session per tab, new tab and rename, close
      (keeping or ending the session), pick the agent (Claude, Codex, shell), attach an
      existing session; tabs persist in `~/.config/rusty/tabs.json`; Ctrl+Shift+T/W, F2
- [x] Terminal colours from the Omarchy theme: the scheme is generated into
      `~/.config/rusty/color-schemes` and the widget is pointed there with `COLORSCHEMES_DIR`,
      no root-owned file; font from the Alacritty config
- [x] Live re-theme: the app watches `~/.config/omarchy/current` and reloads when
      `omarchy theme set` repoints the theme link (no hook script to install)
- [x] Tasks: lists (add, rename, delete with confirm), quick add, toggle, archive and
      restore, inline rename (F2), reorder (Ctrl+Up/Down), delete; keyboard first (Enter,
      arrows, Space, Delete, Escape); live refresh on `dataChanged`; verified against the
      running service on the box
- [x] Settings page: theme, font, scheme, tabs file and back end shown; every setting
      `rusty-mcp` stores is listed and editable in place (Enter saves), new keys can be added
- [x] Desktop entry, icon, stable `app_id` (`com.ignibyte.rusty`), `omarchy-launch-or-focus
      rusty`, and a key: `omarchy/hyprland-bindings.conf` binds SUPER+ALT+R (the installer
      points at it rather than editing your bindings)
- [x] Delete the prototype (2026-09-02, superseded by `crates/rusty-app`)

## M3. Knowledge tabs (done 2026-09-02)

- [x] Brain: type tree with counts, full-text search, rendered markdown with wikilinks that
      navigate, edit and save, timeline with append, backlinks and outbound links, open in
      Obsidian, capture to the daily page or the inbox
- [x] Notes: daily pages in the vault (so Obsidian sees them); Today opens or creates, older
      days listed newest first, same page view as the Brain tab
- [x] Memory: list with category and importance, add with Enter, edit and delete in a side
      panel, filter by category
- [x] Obsidian themed by `omarchy theme set`: the theme's `obsidian.css` becomes the vault's
      `omarchy` CSS snippet (`rusty-cli obsidian configure`, run again by the app on every
      theme change); a theme without the file removes the snippet

## M4. Semantic search (done 2026-09-02)

- [x] `sqlite-vec` inside `rusty.db`: an embedding per page chunk (`brain_chunks` + the `vec0`
      table `brain_vec`, created at the model's width); notes and memories can follow the same
      path when a tab needs them
- [x] Embedding provider trait; Ollama when it is running (default), OpenAI only when the
      setting says so and the vault has the key; no provider means no vectors and everything
      else still works (verified with unit tests and a mock Ollama; the dev box has no provider
      yet)
- [x] Hybrid retrieval, full text and vectors merged by reciprocal rank fusion, behind
      `brain_search`
- [x] Incremental indexing: the server embeds stale pages after each burst of changes and
      every ten minutes; `brain_reembed` and `rusty-cli brain embed [--all]` on demand
- [x] Settings entries: `embedding_provider`, `embedding_model`, `ollama_url`; the key lives in
      the secrets vault as `openai_api_key`

## M5. Skills, Secrets, Settings (done 2026-09-02)

- [x] Skills: list with status and origin, description and body edited in place (Ctrl+S saves),
      new skill (active or staged), safety scan with findings shown, approve (or approve
      anyway), reject, delete
- [x] Secrets: names listed, set or replace (typed into a password field), delete with confirm;
      no value is ever rendered or returned
- [x] Settings: every key the back end reads, with what it means and its default, editable in
      place; other stored keys below; the machine's theme, font, scheme, tabs file and back end

## M6. Omarchy packaging and cutover

- [x] `omarchy/install.sh`: dependencies through `omarchy pkg add`, the binaries, the desktop
      entry and icon, the keybinding snippet (pointed at, not applied), the MCP config snippet,
      the user service, the Obsidian registration; no theme hook is needed, the app watches
      the theme link itself
- [x] A package: `packaging/PKGBUILD` (`rusty-git`) builds the three binaries, the desktop
      entry and icon, the user unit and the snippets, and passes `check()` under makepkg on the
      dev box (2026-09-02). Submitting it to the AUR waits for the first release
- [x] Retire the v2 web UI and server: the code left the private repo on 2026-09-02 (git history
      keeps it) and that repo is the ops side only now; the directories stay as they are
      (`/srv/stacks/rusty` ops, `/srv/stacks/rusty-v3` this repo). The Mac has no Rusty left
- [ ] Docs, screenshots, first release (docs are current; screenshots need a scratch vault so
      no real data ships; the release tag is Chad's to cut)

## M7. Daily use (done 2026-09-02)

What using the app every day asks for, added as it comes up.

- [x] Terminal tabs start in a working directory (typed or picked when the tab is made, saved
      with the tab; verified: a tab made for `/srv/stacks/rusty-v3` opened its shell there)
- [x] A tab that gets output while another tab is showing is marked in the rail (the
      widget's `imagePainted` fires on new output, shown or not); the mark clears when the tab
      is shown. Verified from the debug log with a scratch tab
- [x] The terminal's title shows under the tab name: tmux is started with `set-titles on`
      and `#T`, so "✳ Claude Code" and Codex's "[ ! ] Action Required" come through; the
      host-name default is hidden
- [x] The window remembers its size and the last tab between runs
      (`~/.config/Ignibyte/rusty.conf`; `RUSTY_TAB` still overrides)
- [x] Brain tab: recently updated pages when idle (loads with the tab); Up, Down and Enter
      move through and open search results
- [x] Desktop notification when an agent tab asks for attention while it is not showing or
      the window is not focused. The bell does not survive tmux, so the trigger is the title
      ("[ ! ] Action Required", "needs your input"), once per title, through `notify-send`
- [x] Reorder tabs from the keyboard (Ctrl+Shift+PgUp/PgDn) and the tab menu
- [x] A launch bar across the top with one button per agent CLI found on PATH (Claude Code,
      Codex, Gemini, Aider, OpenCode) plus a shell; a click opens a new tab. First run opens
      one tab for the first agent found instead of a fixed Claude and Codex pair (Chad, 2026-09-02)
- [x] Drag to reorder tasks (a handle on each row) and agent tabs (drag in the rail); built
      2026-09-02, keyboard paths stay; pointer drags await a hands-on check

- [x] Session resilience (TICKET-009, 2026-09-03): the back end restarts after any exit but
      a stop, the app runs as a user service bound to the graphical session, `rusty-session
      up|down|status` was the entry point the installer, the desktop entry and the key shared
      (now `rusty session`, TICKET-029), and the compositor drop-in and the earlyoom line
      ship as pointers

- [x] Top bar, command and agents (TICKET-011, 2026-09-03): the command button and one glyph
      per agent CLI move into the top bar (click for a tab, right-click for the agent pane), the
      Hyprland workspace strip goes (waybar has it), the ribbon keeps the vault and the views

- [x] Text size (TICKET-012, 2026-09-03): one base size on the theme (12 to 18, default 14,
      two more than the drawn body size), every label and the reading view scaled from it, a
      stepper in Settings, Ctrl with plus, minus and zero, a test that refuses a literal size

## M8. Knowledge workspace (Obsidian inside Rusty)

Chad, 2026-09-02: skip the Obsidian API and remake Obsidian inside Rusty; the Replit mock
(`docs/design/rusty-omarchy.html`) is the layout. The full feature inventory is
`docs/planning/intake/INTAKE-knowledge-workspace.md`; work runs as tickets through the
workflow in `CONSTITUTION.md`.

- [x] TICKET-002 Workspace shell (2026-09-02): Obsidian's layout (ribbon, explorer over the
      real folder tree with new, rename, move and delete, document tabs, breadcrumb header,
      inline title, properties, right sidebar with backlinks in context, outgoing links,
      outline and an agent pane, status bar counts), the Rust renderer for Obsidian's flavour
      (`brain::render`), the source editor with a Rust-tokenized highlighter, link-safe rename
      and folder operations in the core with eight new tools, quick switcher, command palette,
      Obsidian's keys, theme tokens from the Omarchy theme's `obsidian.css`; the Brain and
      Notes tabs are gone. Screenshots come from `scripts/screenshot.sh` against a scratch vault
- [x] TICKET-003 Tags (inline and frontmatter, one index, a Tags pane with counts, `tag:` in
      search) and a properties editor typed as Obsidian types them (2026-09-03)
- [x] TICKET-004 Global and local graph views on a canvas: forces, filters, colour groups,
      display settings, hover and click, a depth for the local graph (2026-09-03)
- [x] TICKET-005 Search operators (`path:`, `file:`, `tag:`, `type:`, exclusion, match case,
      regex, in the pane and in `brain_search`), a Bookmarks tab (files, folders, searches,
      headings) and a Hotkeys table in Settings (2026-09-03)
- [x] TICKET-006 Retire the Obsidian bridge: the six tools, the core module, the CLI
      commands, the installer's registration and the app's theme-snippet call are gone; the
      vault stays an Obsidian vault by format (2026-09-03, on Chad's approval of tickets 2
      to 6 rather than on daily use)
- [x] TICKET-008 The mock's look as a skin: colour roles, the Amber phosphor preset and
      three more, Follow Omarchy, theme files, the skin picker; the top bar, rail labels,
      pane heads, tabs, note furniture, the assistant header, a CRT overlay (2026-09-03)
- [x] TICKET-013 Favorites over the bookmarks (2026-09-03): a star on the note header, Ctrl+D, a
      Favorites section above the explorer's tree, favorites first in the quick switcher; the
      storage unchanged
- [x] TICKET-014 Notes into the vault (2026-09-03): the notes folder defaults to `<vault>/notes`,
      `rusty-cli notes adopt` moves an older folder in once (refusing clashes, deleting nothing,
      leaving a README), the watcher covers it through the vault
- [x] TICKET-015 Secrets behind a PIN (2026-09-03): the back end keeps an argon2id PIN and a
      five-minute unlock token, six tools (status, set, unlock, lock, reveal, update), the tab
      reveals one value at a time, edits in place and copies; `secret_list` stays name-only
- [x] TICKET-016 Folders, part one (2026-09-03): folders from the machine as roots below the vault,
      remembered per machine; markdown, text and images viewed in a tab; an agent or a shell opened
      in a folder from its menu. Parts two (file operations) and three (git decorations) are
      TICKET-019 and TICKET-020
- [x] TICKET-018 The brain loop (2026-09-03): Ask, Decide, Follow up as a decision page type,
      five tools, typed graph edges, a Decisions view, the CLI mirror, the seed skill and two
      Claude Code hooks shipped in rusty-cli (`hooks install`)
- [x] TICKET-010 Scripts as commands (2026-09-03): a `*.sh` beside a skill is `rusty <name>`,
      dispatched by the app binary through `rusty-cli scripts run`; the CLI's `scripts` family,
      four tools, a Scripts section of the Skills tab with Run in a terminal tab
- [x] TICKET-021 Terminal clipboard (2026-09-05): Ctrl+Shift+C and Ctrl+Shift+V, middle
      click pasting the primary selection, a right-click menu whose Copy follows the widget's
      own selection state; the chords attach to the terminal item so plain Ctrl+C stays the
      interrupt, and the middle button takes an exclusive grab so the widget's native paste
      does not fire a second time
- [x] TICKET-022 Workspace chrome fixes (2026-09-05): the sidebar splitters measure in the
      scene, so a drag follows the pointer instead of the handle that moves under it; tabs
      reorder by drag (the Tasks page's handler, turned sideways); the rename dialog lays its
      field out; the tab strip's `+` is a menu — the page switcher, every top-bar agent by
      name, the custom terminal
- [x] TICKET-019 Folders, part two (2026-09-05): new file, new folder, rename, move by drag or
      dialog and delete under a folder root act on the disk through the app's `Folders` type,
      delete a move to the XDG trash with its record; a text or markdown file's tab has an
      edit mode with the note editor's autosave and Ctrl+S, saved atomically
- [x] TICKET-020 Folders, part three (2026-09-05): a root in a git repository marks modified,
      added and untracked files and the folders above them and carries its branch; one read
      of `git status --porcelain=v2` per root, cached with the listing, never a write
- [x] TICKET-024 First-class tagging (2026-09-05): Tags as a property type writing the
      `tags:` list, completions from the tag index under the tags field, the Tags pane
      tagging the open page (a `+`, a menu, `T`), a "Tags: Tag this page" command
- [x] TICKET-025 Native agent pane (2026-09-05): the pane beside a note is a headless
      Claude Code over stream-json — one process per page, sessions resumed by id, tool
      calls and results as items, permissions as Allow/Deny, Rusty as its MCP server —
      rendered by the app; the terminal tabs stay tmux terminals
- [x] TICKET-026 Obsidian vault migration (2026-09-05): a plan, then an import at the vault's
      own paths — collisions skipped and named, bare links rewritten, bookmarks carried into
      the app, a report page under `inbox/`, a rollback that leaves the brain as it was; two
      tools (82), `rusty-cli brain import`, a review dialog
- [x] TICKET-027 Personal search engine (2026-09-05): a `source` page type captured by URL
      (a fetch, an in-house HTML extractor, `pdftotext` for PDF), indexed like any page; three
      tools, `rusty-cli source capture|search`, a capture dialog and a glyph in the tree; every
      answer an agent gets about a source marked untrusted and normalised; 85 tools
- [x] TICKET-028 Live preview editing (2026-09-05): a third view at the section grain — a
      click on a rendered block opens that section alone as a highlighted source editor, the
      rest stays rendered, the same autosave and Ctrl+S; the editing mode (live by default)
      kept per user; `page_sections` in Rust, joined back byte for byte
- [x] TICKET-029 Session commands (2026-09-05): `rusty <noun> <verb>` as the command
      convention of the app binary, `rusty session start|stop|status|run` replacing the
      `rusty-session` script (nouns before store scripts, an unknown word prints the usage),
      and the theme read from Omarchy 4's `~/.local/state/omarchy/current/theme` with the
      Omarchy 3 path as the fallback
- [x] TICKET-023 Skills page layout (2026-09-05): a draggable split between the list and
      the detail, the Skills and Scripts sections collapsible from their headers (click, or
      Enter and Space when focused) with the open one taking the height, both remembered in
      the workspace state; the splitter lifted into `Splitter.qml` so the sidebars and the page
      share one
- [x] TICKET-031 Agent sessions that outlive the app (2026-09-17): a conversation is its own
      transient user unit (`rusty-agent-<id>`) running a host that owns the `claude -p`
      process, logs every event and serves it over a socket; `Assistant` is a client that
      attaches and replays, one per surface, and `rusty agent start|stop|list|attach|rm` is
      the same client in a terminal; the note pane's conversation survives the window
- [ ] TICKET-032 The Agent tab: the VS Code-style surface on those sessions — a transcript
      of cards (markdown, thinking, tool calls, an Edit as a diff), permissions and
      questions answered from the keyboard, a composer with mode and model, a sessions pane
- [ ] TICKET-033 Composer extras: slash commands, `@` mentions, image paste, fork, the
      context meter, subagent nesting, open in terminal
- [ ] Brain: tiered context (abstract, overview, details per page and folder) and
      session-to-memory extraction, the two OpenViking ideas worth keeping

## M9. Workflow

- [x] TICKET-001 Spec-driven, phase-gated workflow with a local record, gate receipt,
      hooks and CodeGraph (2026-09-02; `CONSTITUTION.md`, `docs/planning/`)
- [x] TICKET-007 OpenWiki, pinned and project-local, driven through its MCP lifecycle
      by the host agent; required at Phase 5 with a completion receipt the delivery
      checks; the first wiki generated (2026-09-03)
- [ ] Enroll in `rw` (rustal-workflow) when rustal-brain runs on the box
- [ ] Stop-hook claim checking (OmarchyGS has it; the OpenWiki tool receipt is in)

## Later

- TICKET-034: the terminal tabs' tmux server in its own user unit, so a stop or a crash
  restart of the app unit no longer ends every terminal session
- Background agents tab: dispatch, watch, results
- Phone access through Obsidian Sync of the vault
- A macOS build if anyone wants one; nothing in the core is Linux-only

## Principles

- **Omarchy is a dependency, not a target.** Rusty assumes Hyprland, uwsm, the `omarchy`
  CLI, its theme files and its launcher conventions, and uses them instead of reinventing
  them.
- **Files are the truth.** The brain is a folder of markdown Obsidian can open. SQLite holds
  the index, never the only copy of anything.
- **One back end process.** `rusty-mcp` serves the app over local HTTP and the agents over
  stdio. No web UI, no REST layer, no second protocol.
- **Native terminals.** Claude Code and Codex run in a real terminal emulator inside the app,
  attached to tmux sessions, so nothing is lost when the app closes.
- **Same gates as before.** rustfmt, clippy with warnings as errors, tests, docs, on every
  change.

## Non-goals

No browser UI, no voice, no cloud service, no multi-user. Rusty runs on one machine for one
person and talks to the agents that person already uses.
