# Architecture

How Rusty is put together. `ROADMAP.md` says what is in the release and what comes next;
`openwiki/` is the generated engineering wiki, with source anchors for each claim.

## The shape

```
 Claude Code, Codex ── stdio: a rusty-mcp child of their own ─────────────────┐
 Marley, other clients ── Streamable HTTP, 127.0.0.1:4174/mcp (rusty-mcp.service) ─┤
                                                                               ▼
                       rusty-mcp (rmcp; 90 tools, resources, notifications)
                                         │ rusty-core's managers
                                         ▼
     ~/.rusty: brain/ (markdown vault, git) · skills/ (git) · rusty.db (SQLite) · .secret · .pin
                                         ▲
                  rusty-cli (the same managers in-process) · rusty (session, store scripts)
```

## The crates

- `crates/rusty-core`: the managers and the store. The brain (`brain/`: the vault, the
  index, links and tags, the renderer and typed blocks, semantic search, decisions,
  sources, the Obsidian import, bookmarks), the engine (`engine/`: the database and its
  migrations, the change log, memories, to-do lists, settings, secrets and the PIN, the
  conversation archive), notes, skills and their scripts, the event bus and the file
  watcher, and `transfer` (export and import). `Core` builds every manager once;
  `StoreLocation` resolves where a store's parts live.
- `crates/rusty-mcp`: the server, on `rmcp`. Every tool is a thin wrapper over a manager;
  writes go through `mutate()`, which emits `DataChanged` so connected clients hear
  `resources/list_changed`. Stdio by default, Streamable HTTP with `--http`. Resources
  mirror the data under `rusty://` (`tasks`, `memories`, `skills`, `notes`, `brain` and
  their per-item templates).
- `crates/rusty-cli`: the same managers in-process for a terminal and for scripts: the
  brain, sources, notes, skills and scripts, the brain loop, conversation ingest, the
  change log, bookmarks, the Claude Code hooks, export and import.
- `crates/rusty-cmd`: the `rusty` command, with no dependencies: `rusty session start`
  and `status` for the service, and `rusty <name>` for a store script.

## The store

- **The vault is the truth for knowledge.** Pages are markdown files with YAML
  frontmatter; a file without frontmatter is a page too (its title from the file name,
  its type from the top folder, else `note`). Wikilinks are vault paths
  (`[[projects/orbit]]`). A page's history is its `## Timeline` section. Deletes are soft:
  the page or folder moves to `archive/`, which the index never reads. Every write commits
  the paths it touched to the vault's git repository; edits made outside the tools are
  committed on their own.
- **SQLite holds the index and the rest.** `rusty.db` (WAL, a five-second busy timeout,
  foreign keys on) keeps the vault's index (pages by slug, FTS5 text, links with their
  line, tags, aliases, frontmatter properties, timeline rows, chunks and vectors), all
  rebuildable from the files, and the data that has no file: memories, to-do lists,
  settings, decisions' consultations, the conversation archive, and the change log.
- **The change log.** Every writer appends a row (kind, key, operation, detail, time;
  never a value or a body), so `changes_since` tells a client what changed in any
  process. Page changes are recorded where the index is written, once per change.
- **The skills store** is its own git repository: active skills under
  `.claude/skills/<name>/SKILL.md`, skills waiting for approval under `staging/`. A `*.sh`
  beside a skill is a script that inherits the skill's approval; a staged skill's script
  never runs.
- **Secrets** are a plain `KEY='value'` file at mode 600. A PIN (an argon2id hash in
  `.pin`) guards reading and changing them over MCP with a short-lived token; settings
  whose key names a key, token, secret or password read back masked.

## Change propagation

The file watcher debounces changes to the vault, the skills store, a separate notes
folder and the sentinel `~/.rusty/.changed` (which `rusty-cli refresh` touches after a raw
database write), and emits `DataChanged`. The server's indexer then syncs the index from
the files, commits outside edits, and embeds stale pages when an embedding provider is
set; the notifier forwards the change to every connected client. A client that cannot
receive notifications polls `changes_since`.

## Search

`brain_search` runs FTS5 with operators (`tag:`, `path:`, `file:`, `type:`, quoted values,
`-` to exclude) and two text modes (match case, regular expression). With an embedding
provider (`auto` is a local Ollama when it answers; OpenAI only when set and keyed), pages
are chunked and embedded into `sqlite-vec`, and the two rankings merge by reciprocal rank
fusion. Changing the provider or model rebuilds the vectors.

## Agents and the brain loop

Agents reach Rusty through MCP. The brain loop (`docs/architecture/brain-loop.md`) asks
an agent to consult before it decides (`brain_ask`), to record the decision with what it
rested on (`brain_decide`), and to come back on a date (`brain_follow_up`); two optional
Claude Code hooks, installed by `rusty-cli hooks install`, enforce the first two. Captured
sources are marked untrusted in every answer that carries them.

## Moving a store

`rusty-cli export` writes a zip with a `VACUUM INTO` snapshot of the database, the vault
and the skills store with their git history, an external notes folder, and the secrets
only on request. `rusty-cli import` checks every entry first, builds the store beside the
home and renames it in, moves an existing home aside rather than deleting it, and
rewrites the path settings for the new layout.

## Running

`omarchy/install.sh` builds the three binaries into `~/.local/bin` and installs
`rusty-mcp.service`, wanted by `default.target`, restarted after any exit but a stop, at
OOM score 100. Agents start their own `rusty-mcp` over stdio; every process shares the
database (WAL) and the files.

## What is not here

No window and no web UI: front ends such as Marley draw over MCP. No agent runtime: Rusty
stores and answers; it does not run models or dispatch agents. No network service beyond
loopback.
