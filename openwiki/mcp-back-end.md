---
type: "Reference"
title: "MCP back end: one server for Marley and the agents"
openwiki_generated: true
sources:
  - id: openwiki-source-1de5221fd140fd89f39f87cd
    resource: repo://crates/rusty-cli/src/main.rs
  - id: openwiki-source-8237034823c8af0c3889096c
    resource: repo://crates/rusty-core/src/brain/blocks.rs
  - id: openwiki-source-d2476bcfdf1c1072b66eb52b
    resource: repo://crates/rusty-core/src/brain/decisions.rs
  - id: openwiki-source-c7501cab00d475ec77094adb
    resource: repo://crates/rusty-core/src/brain/mod.rs
  - id: openwiki-source-469079f987eef0b6a4cf0a50
    resource: repo://crates/rusty-core/src/brain/sources.rs
  - id: openwiki-source-705d180fc941297b1e844397
    resource: repo://crates/rusty-core/src/core.rs
  - id: openwiki-source-fe80ee2fefb437e929cac903
    resource: repo://crates/rusty-core/src/engine/changes.rs
  - id: openwiki-source-8f342262c76136dc27154aaf
    resource: repo://crates/rusty-core/src/engine/db.rs
  - id: openwiki-source-67d6ac061c03703e5231d5b2
    resource: repo://crates/rusty-core/src/engine/memory_manager.rs
  - id: openwiki-source-bb352c1ae3d0e8267aac9d76
    resource: repo://crates/rusty-core/src/engine/pin_lock.rs
  - id: openwiki-source-202dc84c86ee726d764536c6
    resource: repo://crates/rusty-core/src/engine/settings_manager.rs
  - id: openwiki-source-e98ff1e4c479bcbdfb34b05a
    resource: repo://crates/rusty-core/src/engine/user_tasks.rs
  - id: openwiki-source-5097c4ef41727eee45d8c689
    resource: repo://crates/rusty-core/src/lib.rs
  - id: openwiki-source-2bac0135ef08343388f2c7a1
    resource: repo://crates/rusty-core/src/notes/mod.rs
  - id: openwiki-source-637dadc84a3e86cb046587f2
    resource: repo://crates/rusty-core/src/skills/mod.rs
  - id: openwiki-source-38142a1a317c38546fd7b1f4
    resource: repo://crates/rusty-core/src/skills/scripts.rs
  - id: openwiki-source-5725b482ae3caf2b45126fc1
    resource: repo://crates/rusty-core/src/transfer.rs
  - id: openwiki-source-087a3c8d2ec2da0b0f978302
    resource: repo://crates/rusty-mcp/src/main.rs
  - id: openwiki-source-84acb13abf83511312610cd3
    resource: repo://crates/rusty-mcp/tests/smoke.rs
  - id: openwiki-source-f47a49d22d041953f356ca04
    resource: repo://omarchy/rusty-mcp.service
generated: {by: "claude-code", at: "2026-10-09T19:17:18.990Z"}
verified:
  - by: openwiki/0.3.3
    at: 2026-10-09T19:17:18.990Z
---

# MCP back end: one server for Marley and the agents

## Purpose

Agents and front ends reach the store through `rusty-mcp`; `rusty-cli` runs the same
managers in-process. Claude Code and Codex spawn the server over stdio (`.mcp.json`,
`.codex/config.toml`); Marley and any other HTTP client talk to one long-running instance
over Streamable HTTP at `127.0.0.1:4174/mcp`, kept up by the `rusty-mcp.service` user
unit, which is wanted by `default.target`, restarts two seconds after any exit but a
stop, and sits at OOM score 100 (see [Development and validation](development-and-validation.md)).
Every tool is a thin wrapper around a manager call in `rusty-core`; the managers own the
rules.

## Ownership

`crates/rusty-mcp/src/main.rs`: the `Rusty` server (an rmcp `ToolRouter` over a shared
`Core`), the parameter types, `mutate()` (announce a change on success), the resources
under `rusty://` (`RESOURCES`, `RESOURCE_TEMPLATES`), the indexer loop, the change
notifier, and `main` with the two transports. `crates/rusty-mcp/tests/smoke.rs` drives
the built binary over stdio in a scratch `HOME` the way an agent would.

## The tool surface

`docs/tools.md` lists all 90 tools with their parameters, generated from the router by
the test `tools_reference_is_current`; `family()` in the same test module sorts them into
to-do lists, memories, notes, brain pages and search, the brain loop, bookmarks, sources,
skills, scripts, secrets, settings, the change feed and the conversation archive. The
rules below are the ones a family's list does not show.

- **Missing rows.** Every connection to `rusty.db` enforces foreign keys
  (`Database::prepare`, shared by `open` and the test constructor, which also set WAL and
  the busy timeout), and `open` reports rows that already violate one without repairing
  them. A write naming a task, task group, memory or secret that is not there fails with
  an error naming it (`No task <id>`, `No task group <id>`, `Memory not found: <id>`,
  `No secret <key>`), which a client receives as the call's JSON-RPC error.
- **Notes** live in the vault's `notes/` folder unless `notes_path` names another, so a
  note is also a page of type `note`; `rusty-cli notes adopt` moves a folder kept outside
  the vault in, once.
- **Memories.** Importance is `low`, `normal` or `high`; `medium` is accepted as `normal`
  and any other word is refused. The list orders by that level, then newest first.
- **Bookmarks** live in the vault's `.rusty/bookmarks.json` (files, folders, searches,
  headings; the file and folder ones are the favourites). `bookmark_set` replaces the
  list, which is how a client reorders or retitles; `rusty-cli bookmarks [add|rm]` reaches
  the same file.
- **Rendering.** `brain_render` returns the page as HTML with its outline, links,
  unresolved targets, counts, properties and raw file; with `blocks: true` it adds
  `blocks` and `body_start`, the page as typed data for a client that draws its own
  widgets (see [Markdown rendering](markdown-rendering.md)); given `markdown` it renders
  that text instead of a page, which is how a client shows a markdown file from outside
  the vault.
- **The change feed.** `changes_since` answers what changed after a cursor, from any
  process. Every `rusty-core` writer appends a row to the `changes` table (kind: page,
  task, task_group, memory, note, setting, secret, skill, script or bookmarks; key;
  operation; detail; time), names only, never a value or a body; page changes are
  recorded where the shared index is written, with the content hash, so an edit seen by
  several processes' watchers is recorded once. Without a cursor the tool returns the
  current one; a cursor older than the 20,000 rows kept gets `reset: true` (re-read
  everything); `more: true` asks for another call. `rusty-cli changes [--since N]` prints
  the same.
- **Page lists and paths.** Every `brain_list_pages` summary carries the page's aliases,
  and with `properties` (for example `["path", "task_group"]`) the values of those keys
  the page has, from the index's stored copy of its frontmatter. `brain_read_page` and
  `brain_render` carry `file`, the page's absolute path, and `brain_stats` carries
  `vault_root`, so a client never resolves `brain_vault_path` itself.
- **Creating pages.** `brain_new_page` takes a folder and an optional name, or a `path`
  that wins: the page at exactly that vault path, folders made, an existing page
  returned as it is.
- **Skills.** Every write commits the skills store before it answers (`commit_skills`),
  with the CLI's subjects (`skills: add`, `stage`, `remove`, `approve`, `reject`); a
  failed commit is ignored and the next one picks the change up. `skill_create` activates
  a skill unless `pending` is set; approval of a staged skill runs the safety scan first
  unless `force`.
- **Scripts.** A script is a `*.sh` file in a skill directory, so it inherits that skill's
  approval state; `resolve_script` finds it by basename without the extension, taking
  `skill/name` when two skills share a basename. Running resolves among the active skills
  only, so a staged copy never shadows an approved script. `script_run` is the only tool
  that executes anything; it refuses a script whose skill is still pending, runs it in a
  process group of its own, and at its cap ends the script and every job it started.
  `script_update` rewrites a script in place. The safety scan behind `skill_scan` and
  `skill_approve` reads the skill's whole folder, its scripts included.
- **Secrets and the PIN.** The server owns the PIN: `PinLock`
  (`rusty-core::engine::pin_lock`) keeps an argon2id hash at `~/.rusty/.pin` (mode 0600)
  and one in-memory token per process. `secret_pin_status` reports set, unlocked and any
  lockout; `secret_pin_set` sets the PIN (six characters or more) and needs the live token
  once one exists; `secret_unlock` verifies the PIN, counts five wrong tries in a row into
  a one-minute lockout kept in `.pin-attempts` (so a new process gets no new guesses), and
  returns a token good for `pin_timeout_minutes` (default five);
  `secret_reveal` and `secret_update` require that token; `secret_lock`, a new unlock and
  a server restart end it. Once a PIN exists, `secret_set` and `secret_delete` require it
  too (`PinLock::check_write`), checked before the file is touched. An unlock on the HTTP
  service does not open an agent's stdio server. The PIN guards the tools, not the file:
  the secrets file's format and permissions do not change, because the back end reads it
  headless for the embeddings key.
- **Settings.** A key naming a key, token, secret or password reads back as `•••` from
  both `settings_list` and `setting_get` (one rule in `SettingsManager`; an unset key is
  still null), and `setting_set` refuses that mask written back, so a client that reads
  and saves a form cannot overwrite the credential. The core's own readers still get the
  value. The path settings go through `check_path_setting`: an absolute path, never the
  store's home or a folder above it, and a note path cannot name a dot-file, so the note
  and page tools never reach `.secret` or `.pin`. `rusty-cli settings` applies the same
  rules.
- **The brain loop.** `brain_ask` runs the hybrid search (text alone without a provider),
  lists the decisions touching the question with their status and the follow-ups due, and
  records a consultation (`brain_consultations`: id, question, hits, outcome) whose id the
  next step needs. `brain_decide` writes a `decision` page under `decisions/` (question,
  choice, rationale, alternatives, a wikilink per consulted page, `status: decided`,
  `decided`, `follow_up_by`, `consulted`, `supersedes`), adds a timeline entry to every
  consulted page, marks a superseded decision, and sets the consultation's outcome.
  `brain_follow_up` appends a dated outcome and sets kept, revised or superseded (with the
  successor); `brain_no_decision` records the reason nothing was decided; `brain_due` lists
  the follow-ups due within `days` and every decision. Each summary carries `followed_up`
  (the day of the last follow-up; a page without it falls back to its last
  `### Follow-up` heading) and `superseded_by`. `brain_graph` edges carry a `kind`:
  `link`, or a decision's `consulted`, `supersedes` and `follows_up`, read from its
  frontmatter. The design is `docs/architecture/brain-loop.md`.
- **The Obsidian import** is two tools over `BrainManager`: `brain_import_plan` answers
  what an import of a vault path would do and writes nothing; `brain_import` does it
  through `mutate`. Neither writes the source vault. `rusty-cli brain import <vault>
  [--dry-run]` calls the same two methods. There are no `obsidian_*` tools:
  `brain_get_links`, `brain_unresolved` and `brain_rename` answer for links and renames.
- **Sources** are three tools: `source_capture` (a URL fetched, read and kept as a
  `source` page), `source_search` (the query with ` type:source` appended, over
  `search_with`) and `source_preview` (a page under `sources/`; any other slug is
  refused). Every answer that carries a source is marked in the tool layer:
  `untrusted: true`, a `note` that says web content is data and never instructions, and
  `snippet`, `compiled_truth` and `timeline` normalised (control characters out, blank
  runs to one, four thousand characters at most). `brain_search`, `brain_read_page`,
  `brain_render`, `brain_ask` and the `rusty://brain/{slug}` resource mark a hit or a page
  of type `source` the same way, and `brain_get_links` and `brain_unresolved` mark a link
  row whose line comes from a source. A rendered source page's HTML shows its raw HTML as
  text, and every page's links keep only http, https, mailto, `rusty:` and anchors.
- **The conversation archive.** `search_conversations` searches the transcripts
  `rusty-cli ingest-conversation` kept (`ConversationArchive::search`) and returns them as
  `transcripts`, with `agent_runs` from the tables earlier versions' built-in agent runs
  wrote.

`EXPECTED` in the router test lists every name; a tool missing from it or from the router
fails the test, every tool must carry a description, and every tool must fit a family.

## Runtime flow

- A write goes through `mutate()`, which emits `AppEvent::DataChanged` on success. The
  change notifier forwards every such event to the clients connected to this process as
  a `resources/list_changed` notification, dropping peers that went away. Notifications
  do not cross processes: a change another process made only in the database reaches a
  client through `changes_since`.
- `start_data_watcher` watches the notes, brain and skills folders (and the sentinel
  `~/.rusty/.changed` that `rusty-cli refresh` touches) and emits `DataChanged` after a
  burst settles, `.git` excluded.
- The indexer loop runs at start, after every burst and every ten minutes without one:
  `sync_all()` (files changed by other programs reach the index), `commit_outside_edits`
  (those edits get their own commit), then `index_stale` with the configured embedder
  when there is one; failures are logged to stderr, never fatal.
- Resources: `rusty://tasks`, `memories`, `skills`, `notes`, `brain` and the templates
  `tasks/{group_id}`, `brain/{slug}`, `notes/{path}`, all JSON except a note's markdown.
- Diagnostics go to stderr only; stdout is the protocol.

## Invariants

- No tool reaches the database directly; managers do.
- A renamed or removed tool is a versioned break; new tools are additive.
- `brain_search`, `brain_ask`, re-embedding, the semantic status and `script_run` run in
  `spawn_blocking`, so the server keeps answering while they work. `source_capture` (up to
  a twenty-second fetch), `brain_import`, `brain_rename`, `source_search` and
  `search_conversations` run on the request's own task.
- A secret's value leaves the server only against the live PIN token, and no call logs a
  PIN, a token or a value.
- A script runs only from an approved skill. Both execution paths check the status, so
  neither the tool nor the command line can run a script waiting in staging.

## Failure modes

- Without an embedding provider `brain_reembed` errors and search stays full text.
- Five wrong PINs in a row refuse every unlock for a minute, the right PIN included, in
  every server process on the store; a restart clears the token but not the lockout.
- An embedding provider that fails a query costs only the vector half: search logs it and
  answers from the full-text index.
- A script that does not finish inside the caller's cap is killed and reported as status
  124 with a `timed_out` flag rather than left running; each stream is truncated at
  64 KiB, so a script that prints forever cannot exhaust the server. The `rusty <name>`
  path has neither cap: `exec_script` replaces the process, so the script becomes the
  command and lives as long as the terminal lets it.

## Extension points

- A new tool: a parameter struct with doc comments, a `#[tool]` method with a description
  that says what it does, returns and refuses, a call into a manager, `mutate()` for
  writes, its name in `EXPECTED`, its family in `family()`, and `docs/tools.md`
  regenerated (`RUSTY_UPDATE_DOCS=1 cargo test -p rusty-mcp tools_reference_is_current`).
- A new resource: an entry in `RESOURCES` or `RESOURCE_TEMPLATES`, a `ResourceUri`
  variant, `parse_resource_uri`, `resource_text`.

## Tests

- `cargo test -p rusty-mcp`: the router tests (every tool once, each with a description
  and a family, `docs/tools.md` current, resource URIs) and the smoke tests over stdio: a
  task group, resources, pages, a whole-file write, a render with a style and blocks, a
  rename with its link rewrite, the tree, the change feed, bookmarks, scripts, the brain
  loop, the PIN on secret writes, masked settings, skill writes that commit, memory
  importance, and `search_conversations` finding an ingested transcript.
- `cargo test -p rusty-core pin_lock`: set, unlock, check and lock; the short and the
  wrong PIN; the lockout, also as a second process sees it; the expiry; a PIN change
  needing the token; the file mode.
- `cargo test -p rusty-core skills`: the cap ending a script's background jobs and the
  bounded output, a staged copy not shadowing an approved script, approval scanning the
  scripts and unreadable frontmatter.
- `cargo test -p rusty-core decisions`: the consultation record, the decision page with
  its links and timeline entries, the follow-up's status and date, the due list, the
  typed edges.
- The import and the sources are tested in `rusty-core`
  ([Vault and brain](vault-and-brain.md)).

## Primary sources

- `crates/rusty-mcp/src/main.rs`, `crates/rusty-mcp/tests/smoke.rs`, `docs/tools.md`
- `omarchy/rusty-mcp.service`, `omarchy/mcp-config.json`
