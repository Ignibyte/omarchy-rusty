# Architecture

How Rusty is put together. `ROADMAP.md` says what is in the release and what comes next;
`openwiki/` is the generated engineering wiki, with source anchors for each claim. The
references are [tools.md](tools.md), [cli.md](cli.md) and
[configuration.md](configuration.md).

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
  writes go through `mutate()`, which emits `DataChanged`. Stdio by default, Streamable
  HTTP with `--http`. Resources mirror the data under `rusty://`: `tasks`, `memories`,
  `skills`, `notes` and `brain`, with per-item templates for a to-do list, a page and a
  note.
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
- **SQLite holds the index and the rest.** `rusty.db` runs in WAL mode with a five-second
  busy timeout and foreign keys on. The tables are listed below; the page index can be
  rebuilt from the vault at any time, the rest cannot.
- **The change log.** Every writer appends a row (kind, key, operation, detail, time;
  never a value or a body), so `changes_since` tells a client what changed in any
  process. Page changes are recorded where the index is written, once per change. The
  newest 20,000 rows are kept.
- **The skills store** is its own git repository: active skills under
  `.claude/skills/<name>/SKILL.md`, skills waiting for approval under `staging/`. A `*.sh`
  beside a skill is a script that inherits the skill's approval; a staged skill's script
  never runs.
- **Secrets** are a plain `KEY='value'` file at mode 600. A PIN (an argon2id hash in
  `.pin`) guards reading and changing them over MCP with a short-lived token; settings
  whose key names a key, token, secret or password read back masked.

### The tables

| Tables | Holds | Rebuilt from the vault |
|---|---|---|
| `brain_pages`, `brain_fts`, `brain_links`, `brain_tags`, `brain_aliases`, `brain_timeline` | the page index: one row per page with its frontmatter, the full-text index, links with their line, tags, aliases, timeline entries | yes, `rusty-cli brain reindex` |
| `brain_chunks`, `brain_vec`, `brain_vec_meta` | page text in chunks and their vectors (`sqlite-vec`), with the model that made them | yes, by embedding again |
| `brain_versions` | a snapshot of a page before each update or whole-file write | no |
| `brain_consultations` | the brain loop's consultations and their outcome | no |
| `task_headers`, `user_tasks` | to-do lists and their tasks | no |
| `memories`, `settings`, `changes` | memories, settings, the change log | no |
| `conversation_archive`, `conversation_archive_fts` | ingested Claude Code transcripts and their text | from the transcripts, while they exist |
| `tasks`, `conversations`, `agents` | earlier versions' built-in agent runs; nothing writes them now, and `search_conversations` reads the first two | no |

Migrations live in `engine/db.rs` and only add.

## A write, step by step

A page update over MCP:

1. The tool handler parses the parameters and calls the brain manager inside `mutate()`.
2. The manager snapshots the page into `brain_versions`, then writes the file, keeping the
   frontmatter as written where it can.
3. It indexes the page: `brain_pages`, the full-text row, links, tags, aliases and
   timeline, in one place that also appends a `changes` row.
4. It commits the paths this write touched to the vault's git repository, on a background
   thread, so the call does not wait for git.
5. `mutate()` emits `DataChanged`; the notifier sends `notifications/resources/list_changed`
   to every client connected to this process.
6. The indexer embeds the page's new text a few seconds later, when a provider is set.

A write with no file (a to-do, a memory, a setting) is steps 1, 3 (the `changes` row
only) and 5.

## Several processes, one store

The service and every agent's stdio server are separate processes over the same files and
database. Each one has its own file watcher, indexer, embedding loop, ten-minute fallback
sync, database connection, PIN token and embedder cache. What keeps them consistent:

- SQLite's WAL and the busy timeout serialize writes across processes; within a process
  one connection sits behind a mutex.
- A page write indexes the page itself, and the other processes' watchers see the file
  change and sync it again, which finds nothing to do (the content hash matches).
- A vault commit names only the paths its write touched, and the sweep that commits
  outside edits leaves paths a write in flight has claimed; when two processes sweep, the
  second finds a clean tree.

Notifications do not cross processes. A client hears about writes made by the process it
is connected to, and about file changes that process's watcher sees. A change another
process makes only in the database (a to-do from an agent, say) reaches it through
`changes_since`, which is why a long-running client such as Marley polls it. After
writing the database by hand, `rusty-cli refresh` touches `~/.rusty/.changed`, which every
watcher sees.

## Change propagation

The file watcher debounces changes to the vault, the skills store, a separate notes folder
and the sentinel `~/.rusty/.changed`, and emits `DataChanged`. The server's indexer then
syncs the index from the files, commits outside edits, and embeds stale pages when an
embedding provider is set; the notifier forwards the change to the process's clients. The
indexer also runs every ten minutes when nothing has changed.

## Search

`brain_search` runs FTS5 with operators (`tag:`, `path:`, `file:`, `type:`, quoted values,
`-` to exclude) and two text modes (match case, regular expression). With an embedding
provider (`auto` is Ollama when it answers; OpenAI only when set and keyed), pages are
chunked and embedded into `sqlite-vec`, and the two rankings merge by reciprocal rank
fusion. Changing the provider or model rebuilds the vectors.

## Agents and the brain loop

Agents reach Rusty through MCP. The brain loop ([brain-loop.md](architecture/brain-loop.md))
asks an agent to consult before it decides (`brain_ask`), to record the decision with what
it rested on (`brain_decide`), and to come back on a date (`brain_follow_up`); two optional
Claude Code hooks, installed by `rusty-cli hooks install`, enforce the first two. Captured
sources are marked untrusted in every answer that carries them.

## Moving a store

`rusty-cli export` writes a zip with a `VACUUM INTO` snapshot of the database, the vault
and the skills store with their git history, an external notes folder, and the secrets
only on request. `rusty-cli import` checks every entry first, builds the store beside the
home and renames it in, moves an existing home aside rather than deleting it, and
rewrites the path settings for the new layout.

## Running

`omarchy/install.sh` builds the three binaries (or copies a release's) into
`~/.local/bin` and installs `rusty-mcp.service`, wanted by `default.target`, restarted
after any exit but a stop, at OOM score 100. Agents start their own `rusty-mcp` over
stdio.

## What is not here

Rusty has no window and no web UI; front ends such as Marley draw over MCP. It runs no
models and dispatches no agents: it stores and answers. It serves nothing beyond loopback.
