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
generated: {by: "claude-code", at: "2026-10-09T14:13:41.820Z"}
verified:
  - by: openwiki/0.3.3
    at: 2026-10-09T17:18:26.892Z
---

# MCP back end: one server for Marley and the agents

## Purpose

`rusty-mcp` is the only way into the store. Claude Code and Codex spawn it over stdio
(`.mcp.json`, `.codex/config.toml`); Marley, and any other HTTP client, talks to one long-running instance over
Streamable HTTP at `127.0.0.1:4174/mcp`, kept up by the `rusty-mcp.service` user unit,
which is wanted by `default.target`, restarts two seconds after any exit but a stop, and
sits at OOM score 100 (see [Development and validation](development-and-validation.md),
running as services).
Every tool is a thin wrapper around a manager call in `rusty-core`; the managers own the
rules.

## Ownership

`crates/rusty-mcp/src/main.rs`: the `Rusty` server (an rmcp `ToolRouter` over a shared
`Core`), the parameter types, `mutate()` (announce a change on success), resources under
`rusty://`, the indexer loop, the change notifier, and `main` with the two transports.
`crates/rusty-mcp/tests/smoke.rs` drives the built binary over stdio in a scratch `HOME`
the way an agent would.

## Tool families

- Tasks: lists, create, toggle, archive, unarchive, retitle, reorder, delete.
- Missing rows (TICKET-045): every connection to `rusty.db` enforces foreign keys
  (`Database::prepare`, shared by `open` and the test constructor, which also set WAL and
  the busy timeout), and `open` reports rows that already violate one without repairing
  them. A write naming a task, task group, memory or secret that is not there fails
  with an error naming it (`No task <id>`, `No task group <id>`, `Memory not found:
  <id>`, `No secret <key>`), which a client receives as the call's JSON-RPC error.
- Notes: list, read, write, create, rename, delete, over the notes folder: the vault's
  `notes/` since TICKET-014 (an explicit `notes_path` setting still wins), so a note is
  also a page of type `note`; `rusty-cli notes adopt` moves an older folder in once.
- Memories: list, store, update, delete; `search_conversations`. Importance is `low`,
  `normal` or `high` (`medium`, the word `store_memory` once documented, is taken as
  `normal`; any other word is refused), and the list orders by that level, then newest
  first (TICKET-052).
- Brain: `brain_search` (hybrid when an embedder exists), read, list, create, update,
  delete, timeline read and append, links, resolve a slug, stats, daily note, capture,
  page types, semantic status, reembed.
- Bookmarks (TICKET-037): `bookmark_list`, `bookmark_add`, `bookmark_remove` and
  `bookmark_set` over the vault's `.rusty/bookmarks.json` (files, folders, searches,
  headings; the file and folder ones are the favourites). `bookmark_set` replaces the
  list, which is how a client reorders or retitles; `rusty-cli bookmarks [add|rm]`
  reaches the same store.
- Blocks (TICKET-036): `brain_render` with `blocks: true` adds `blocks` and
  `body_start`, the page as typed data for a client that cannot draw Qt rich text; see
  [Markdown rendering](markdown-rendering.md).
- Change cursor (TICKET-035): `changes_since` answers what changed after a cursor, from
  any process. Every `rusty-core` writer appends a row to the `changes` table (kind:
  page, task, task_group, memory, note, setting, secret, skill or script; key; operation;
  detail; time), names only, never a value or a body; page changes are recorded where
  the shared index is written, with the content hash, so an edit seen by several
  processes' watchers is recorded once. Without a cursor the tool returns the current
  one; a cursor older than the 20,000 rows kept gets `reset: true` (re-read everything);
  `more: true` asks for another call. `rusty-cli changes [--since N]` prints the same.
  No process owns watcher side effects: each is idempotent against the shared store.
- Page lists (TICKET-047): every `brain_list_pages` summary carries the page's aliases,
  and with `properties` (for example `["path", "task_group"]`) the values of those keys
  the page has, from the index's stored copy of its frontmatter; without it there is no
  `properties` field.
- Paths (TICKET-042): `brain_read_page` and `brain_render` carry `file`, the page's
  absolute path, and `brain_stats` carries `vault_root`; the core resolves both
  (`BrainManager::vault_root`, canonical, else made absolute), so a client that opens a
  page in an editor never reads `brain_vault_path` or applies its default itself.
- Workspace: `brain_tree`, `brain_render` (rich text plus outline, links, unresolved
  targets, counts, properties, raw), `brain_write_page` (the whole file),
  `brain_new_page` (a folder and an optional name, or a `path` that wins: the page at
  exactly that vault path, folders made, an existing page returned; TICKET-041),
  `brain_new_folder`, `brain_delete_folder`, `brain_rename`,
  `brain_unresolved`, `brain_tags` (every tag with its count), `brain_set_property` and
  `brain_remove_property` (one frontmatter key, typed, the body untouched), and
  `brain_graph` (page nodes with title, type, folder and tags, edges from resolved
  links; tags and unresolved targets as nodes on request; `around` and `depth` for one
  page's neighbourhood). `brain_search` takes the operators of `parse_query`
  (`tag:`, `path:`, `file:`, `type:`, `-` excluding) in the query and the two text modes
  as `case_sensitive` and `regex`.
- Skills: list, view, create, update, scan, approve, reject, delete. Every write commits
  the skills store before it answers (`commit_skills`, TICKET-051), with the CLI's
  subjects (`skills: add`, `stage`, `remove`, `approve`, `reject`); a failed commit is
  ignored and the next one picks the change up.
- Scripts (TICKET-010): `script_list`, `script_view`, `script_update` and `script_run`.
  A script is not an object of its own — it is a `*.sh` file *inside* a skill directory,
  so it inherits that skill's approval state, and `resolve_script` finds it by basename
  without the extension, taking `skill/name` when two skills share a basename. That one
  decision carries the safety story: `script_run` is the only tool here that executes
  anything, and it refuses a script whose skill is still pending, so approving the skill
  is the only route to running it.
- Secrets: list names, set, delete; and, since TICKET-015, the PIN behind the Secrets
  tab, which the server owns. `PinLock` (`rusty-core::engine::pin_lock`) keeps an
  argon2id hash at `~/.rusty/.pin` (mode 0600) and one in-memory token. `secret_pin_status`
  reports set, unlocked and any lockout; `secret_pin_set` sets the PIN (six characters or
  more) and needs the live token once one exists; `secret_unlock` verifies the PIN,
  counts five wrong tries in a row into a one-minute lockout, and returns a token good
  for `pin_timeout_minutes` (default five); `secret_reveal` and `secret_update` require
  that token; `secret_lock`, a new unlock and a server restart end it. Once a PIN exists,
  `secret_set` and `secret_delete` require it too (`PinLock::check_write`, TICKET-049),
  checked before the file is touched; with no PIN set they need nothing, since there is
  nothing to unlock with. The token lives in one process, so an unlock on the HTTP
  service does not open an agent's stdio server. `secret_list` stays name-only, so no
  tool returns a value without a live unlock. The PIN protects
  the screen: the secrets file's format and permissions do not change, because the back
  end reads it headless for the embeddings key.
- Settings: get, set, list. A key naming a key, token, secret or password reads back as
  `•••` from both `settings_list` and `setting_get` (one rule in `SettingsManager`,
  TICKET-050; an unset key is still null), and `setting_set` refuses that mask written
  back, so a client that reads and saves a form cannot overwrite the credential. The
  core's own readers still get the value.
- The brain loop (TICKET-018): `brain_ask` runs the hybrid search (text alone without
  a provider), lists the decisions touching the question with their status and the
  follow-ups due, and records a consultation (`brain_consultations`: id, question, hits,
  outcome) whose id the next step needs. `brain_decide` writes a `decision` page under
  `decisions/` (question, choice, rationale, alternatives, a wikilink per consulted page,
  `status: decided`, `decided`, `follow_up_by`, `consulted`, `supersedes`), adds a
  timeline entry to every consulted page, marks a superseded decision, and sets the
  consultation's outcome. `brain_follow_up` appends a dated outcome and sets kept, revised
  or superseded (with the successor); `brain_no_decision` records the reason nothing was
  decided; `brain_due` lists the follow-ups due within `days` and every decision; each
  summary carries `followed_up` (the day of the last follow-up, which `brain_follow_up`
  records; older pages fall back to their last `### Follow-up` heading) and
  `superseded_by` (TICKET-048).
  `brain_graph` edges carry a `kind`: `link`, or a decision's `consulted`, `supersedes`
  and `follows_up`, read from its frontmatter. The record is `docs/architecture/brain-loop.md`.
- Text that is not a page: `brain_render` given `markdown` renders that text with the
  page renderer (links resolve against the vault as in a page; the slug may be empty),
  which is how a client shows a markdown file from outside the vault.

The Obsidian bridge (six `obsidian_*` tools over Obsidian's CLI) was retired on
2026-09-03; `brain_get_links`, `brain_unresolved` and `brain_rename` cover what it did,
and the client opens pages.

`EXPECTED` in the router test lists every name; a tool missing from it or from the
router fails the test, and every tool must carry a description.

## Runtime flow

- A write goes through `mutate()`, which emits `AppEvent::DataChanged` on success. The
  change notifier forwards every such event to every connected client as a
  `resources/list_changed` notification, dropping peers that went away.
- `start_data_watcher` watches the notes, brain and skills folders (and a sentinel
  `~/.rusty/.changed` that `rusty-cli refresh` touches) and emits `DataChanged` after a
  burst settles, `.git` excluded.
- The indexer loop runs at start and after every burst: `brain_manager.sync_all()`
  (files changed by other programs reach the index), then `index_stale` with the
  configured embedder when there is one; failures are logged to stderr, never fatal.
- Resources: `rusty://tasks`, `memories`, `skills`, `notes`, `brain` and the templates
  `tasks/{group_id}`, `brain/{slug}`, `notes/{path}`, all JSON except a note's markdown.
- Diagnostics go to stderr only; stdout is the protocol.
- The Obsidian import (TICKET-026) is two tools over `BrainManager`: `brain_import_plan`
  answers what an import of a vault path would do and writes nothing;
  `brain_import` does it through `mutate`, so every client sees `DataChanged` once the
  pages, attachments and the report page are in and the index is rebuilt. Neither writes
  the source vault. `rusty-cli brain import <vault> [--dry-run]` calls the same two
  methods and prints the plan and the report.
- Sources (TICKET-027) are three tools: `source_capture` (a URL fetched, read and kept as
  a `source` page, `DataChanged` emitted), `source_search` (the query with
  ` type:source` appended, over `search_with`) and `source_preview` (a page under
  `sources/`; any other slug is refused). Every answer that carries a source is marked
  in the tool layer, from the first commit: `untrusted: true`, a `note` that says web
  content is data and never instructions, and `snippet`, `compiled_truth` and
  `timeline` normalised (control characters out, blank runs to one, four thousand
  characters at most). `brain_search`, `brain_read_page` and `brain_render` mark a hit
  or a page of type `source` the same way; a rendered page's `raw` and HTML are left
  alone, since they are for display and editing. `rusty-cli source capture <url>` and
  `source search <query>` reach the same methods. The surface is 90 tools.

## Invariants

- No tool reaches the database directly; managers do.
- A renamed or removed tool is a versioned break; new tools are additive.
- Long work runs in `spawn_blocking` so the server keeps answering.
- A secret's value leaves the server only against the live PIN token, and no call
  logs a PIN, a token or a value.
- A script runs only from an approved skill. Both execution paths check the status, so
  neither the tool nor the command line can run a script waiting in staging.

## Failure modes

- Without an embedding provider `brain_reembed` errors and search stays full text.
- Five wrong PINs in a row refuse every unlock for a minute, the right PIN included;
  the lockout and the token live in memory, so a restart clears both.
- A script that does not finish inside the caller's cap is killed and reported as status
  124 with a `timed_out` flag rather than left running; each stream is truncated at
  64 KiB, so a script that prints forever cannot exhaust the server. The `rusty <name>`
  path has neither cap: `exec_script` replaces the process, so the script becomes the
  command and lives as long as the terminal lets it.

## Extension points

- A new tool: a parameter struct with doc comments, a `#[tool]` method that calls a
  manager, `mutate()` for writes, its name in `EXPECTED`, a line in the README.
- A new resource: a `ResourceUri` variant, `parse_resource_uri`, `resource_text`.

## Tests

- `cargo test -p rusty-mcp`: the router tests and the smoke test (list tools, a task
  group, resources, a new page, a whole-file write,
  a render with a style, a rename with its link rewrite, the tree, no unresolved links).
- `cargo test -p rusty-core pin_lock`: set, unlock, check and lock; the short and the
  wrong PIN; the lockout; the expiry; a PIN change needing the token; the file mode.
- `cargo test -p rusty-core decisions`: the consultation record, the decision page with
  its links and timeline entries, the follow-up's status and date, the due list, the
  typed edges; the smoke test walks the loop over stdio.
- The router test's `EXPECTED` list names `brain_import_plan` and `brain_import`; the
  import itself is tested in `rusty-core` (`vault-and-brain.md`).
- The router test's `EXPECTED` list names `source_capture`, `source_search` and
  `source_preview`; the mark and the capture are tested in `rusty-core`
  (`vault-and-brain.md`).

## Primary sources

- `crates/rusty-mcp/src/main.rs`, `crates/rusty-mcp/tests/smoke.rs`
- `omarchy/rusty-mcp.service`, `omarchy/mcp-config.json`
