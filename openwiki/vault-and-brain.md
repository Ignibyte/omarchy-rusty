---
type: "Reference"
title: "Vault and brain: files as the truth, SQLite as the index"
openwiki_generated: true
sources:
  - id: openwiki-source-b681a4cbd48f89edfeb514e4
    resource: repo://crates/rusty-core/src/brain/bookmarks.rs
  - id: openwiki-source-d2476bcfdf1c1072b66eb52b
    resource: repo://crates/rusty-core/src/brain/decisions.rs
  - id: openwiki-source-9c78ae52164f32a938f17cce
    resource: repo://crates/rusty-core/src/brain/frontmatter.rs
  - id: openwiki-source-ffc9a1027ebb83d922f541ac
    resource: repo://crates/rusty-core/src/brain/import.rs
  - id: openwiki-source-84bd94f4d6c1ff8ab953d365
    resource: repo://crates/rusty-core/src/brain/links.rs
  - id: openwiki-source-c7501cab00d475ec77094adb
    resource: repo://crates/rusty-core/src/brain/mod.rs
  - id: openwiki-source-061d1620011caae21b2a0d24
    resource: repo://crates/rusty-core/src/brain/semantic.rs
  - id: openwiki-source-469079f987eef0b6a4cf0a50
    resource: repo://crates/rusty-core/src/brain/sources.rs
  - id: openwiki-source-79e92c26a49d3b5ce7f4c00a
    resource: repo://crates/rusty-core/src/brain/vault.rs
  - id: openwiki-source-fe80ee2fefb437e929cac903
    resource: repo://crates/rusty-core/src/engine/changes.rs
generated: {by: "claude-code", at: "2026-10-09T17:29:16.068Z"}
verified:
  - by: openwiki/0.3.3
    at: 2026-10-09T17:39:34.627Z
---

# Vault and brain: files as the truth, SQLite as the index

## Purpose

The brain is a folder of markdown (`~/.rusty/brain`, a git repository) that Obsidian or
any editor can open. SQLite (`~/.rusty/rusty.db`) holds derived indexes: page metadata,
full text (FTS5), links, tags, aliases, timeline rows, versions, and vectors. Everything
in the database can be rebuilt from the folder.

## Ownership

`crates/rusty-core/src/brain/`:

- `vault.rs`: `VaultManager`, the filesystem side: type folders, the real nested tree,
  read and write by slug, rename and move, soft deletes into `archive/`, git auto-commits.
- `frontmatter.rs`: the page format: YAML frontmatter, the compiled truth, the
  `## Timeline` section; lenient parsing; ordered properties.
- `links.rs`: the one wikilink scanner (`scan`, `targets`, `rewrite_targets`) shared by
  the indexer, the renderer, the migration and the move rewrite, and the inline tag
  scanner (`tags`).
- `mod.rs`: `BrainManager`, the manager over both: create, read, update, whole-file
  write, rename, folders, timeline, capture, daily pages, sync, search, migration.
- `semantic.rs`: chunking, the `Embedder` trait and providers, the `sqlite-vec` index,
  rank fusion.
- `engine/db.rs`: the schema and `migrate()`; `sqlite-vec` is registered as an auto
  extension.

## The page rules

- A page is `<folder>/<name>.md`; its slug is the path without `.md`. Type folders
  (`people`, `companies`, `projects`, `concepts`, `meetings`, `ideas`, `daily`, `inbox`,
  `conversations`) imply a type; any other folder, and the root, imply `note`.
- Frontmatter carries `title`, `type`, `aliases`, `tags`, `created`, `updated` and any
  extra keys. A file without frontmatter is still a page: the title is the file name and
  the type comes from the top folder (`BrainFrontmatter::fill_defaults`). Unreadable YAML
  degrades the same way (`parse_lenient`); only strict callers see the error.
- Dates are the machine's local calendar day. `created`, `updated`, a decision's
  `decided`, follow-up headings, timeline entries and the default daily note all come
  from `frontmatter::today_iso`, which reads chrono's `Local` zone (`TZ`, else
  `/etc/localtime`); before TICKET-044 it counted UTC days, so pages written after 19:00
  on a UTC−5 box were dated tomorrow. A decision is overdue when its `follow_up_by` is
  before the local today, and `brain_due` counts its horizon from the same day.
- The timeline is the `## Timeline` section that runs to the end of the file; the body
  above it is the compiled truth. The bare `---` rule of older pages is read, never
  written; `rusty-cli brain migrate` rewrites it.
- An Obsidian vault comes in through `import_plan` and `import_vault` (`brain/mod.rs`,
  the pure parts in `brain/import.rs`; TICKET-026), reached by `brain_import_plan` and
  `brain_import`, and by `rusty-cli brain import <vault> [--dry-run]`. The source is
  walked read-only with dot-entries skipped (`.obsidian`, `.trash`, `.git`); a page
  keeps its path as its slug and an attachment its path; a slug or path already in the
  brain, or one that would land in `archive/`, is a collision, skipped and named, never
  overwritten or renamed; bare-name links are rewritten to vault paths by the
  migration's `LinkIndex` built over the brain's pages and the incoming ones, the
  frontmatter byte for byte, unresolved targets reported; the tags and the bookmarks in
  `.obsidian/bookmarks.json` (groups flattened; file, folder, search and heading kinds)
  travel in the plan; the import writes pages, then attachments, then a report page
  under `inbox/` (`import-<date>-<name>`, a suffix when the minute's name exists),
  rebuilds the index and commits once. Every path the run creates is recorded, and a
  failure removes them all, rebuilds the index and returns the error, so the brain is
  the import whole or as it was.
- A source (TICKET-027) is a page of type `source` under `sources/` — `url`, `site`,
  `captured` and `kind` in its frontmatter, the readable text as its body — indexed by
  `sync_page` like every page and embedded by the same loop when a provider is set.
  `capture_url` fetches the URL (http or https only, twenty seconds, five redirects,
  eight megabytes, a `rusty` user agent, nothing else sent) and `capture_fetched` reads
  it: HTML through a small state machine in `brain/sources.rs` (the `<title>` or the
  first `<h1>`; the text of `<main>` or `<article>` when the page has one, else the
  body; scripts, styles and the like dropped; headings and list items kept as markdown;
  entities decoded), PDF through `pdftotext` when the box has it, markdown and text as
  they are, a megabyte kept. A URL captured before is found by its `url` property and
  its page rewritten with `created` kept; a new one takes `sources/<site>-<title slug>`.
  A failure on a new URL writes a page that says why (`status: failed`, `error`); a
  failure on a captured URL keeps the text and records those two keys. `rusty-cli
  source capture <url>` and `source search <query>` are the terminal's doors; the tools
  are the back end's (`mcp-back-end.md`).
- Wikilinks are vault paths: `[[projects/orbit]]`, `[[projects/orbit|alias]]`,
  `[[projects/orbit#Heading]]`, `![[embed]]`. The scanner skips fenced and inline code.
- Deletes are soft: a page or folder moves to `archive/<name>_<timestamp>`. The root's
  `archive/` holds no pages (TICKET-040): the page walk, the attachment search and the
  tree skip it as they skip dot-folders (a folder named `archive` deeper down is
  ordinary), and `write_page`, `create_folder` and a move into it are refused with a
  message naming delete and restore, so a deleted folder's pages are never indexed or
  embedded again. A page that leaves (`delete_page`, `delete_folder`, or a file gone at
  `sync_all`) is forgotten with every link it made, typed edges included
  (`forget_page`), and `sync_all` drops any link row whose source is not indexed; a
  re-indexed page keeps its typed edges because only its `reference` links are rebuilt.
- Rusty's writers always write frontmatter; a whole-file write (`write_raw`) writes
  exactly the text it was given after snapshotting the previous version.
- Tags come from two places and land in one index: the frontmatter `tags` list and
  inline `#tags` in the body (a `#` at a boundary, then letters, digits, `_`, `-` and
  `/`, at least one letter, never inside code), compared without case and stored as
  first written. A nested tag `a/b` counts under `a` as well.
- A property edit (`set_property`, `remove_property`) rewrites only the frontmatter
  mapping, keys in their order, and writes the body back byte for byte; values are text,
  numbers, booleans, dates as `YYYY-MM-DD` text, or lists of strings. A page without
  frontmatter gains it; removing the last key drops it.

## Runtime flow

- Read: `read_page` parses leniently and fills defaults; `render_page` renders the body
  (see [Markdown rendering](markdown-rendering.md)) and returns the ordered properties
  and the raw file.
- Write: `create_page`, `update_page` (body only, frontmatter and timeline kept),
  `write_raw` (the whole file), `add_timeline`, `capture`. Each writes the file,
  re-indexes the page and fires a background git commit of the paths it touched.
- Commits (TICKET-043): `VaultManager`'s writes note the paths they touch, and
  `git_commit` stages and commits exactly those that `git status` shows changed
  (literal pathspecs, so a page named `o*` is that file), printing git's message on a
  failure. An edit made outside the tools (Obsidian, an editor, an agent's file write,
  the notes folder inside the vault) is committed on its own by
  `commit_outside_edits`, which the server's indexer runs after each sync; it leaves
  paths a write in flight has claimed, and a second process finds the tree clean. A
  failed commit leaves its files changed, and the next sweep commits them.
- Bookmarks (TICKET-037) live at `.rusty/bookmarks.json` in the vault: a JSON array of
  `{kind, title, path?, query?, heading?}` that git tracks and the page walk skips (a
  dot-folder). The core lists, adds, removes and replaces them; a rename carries the
  bookmarks on its old path along and a delete drops them, each after that write's own
  commit; the import adds the source vault's well-formed bookmarks.
- Every page change is recorded in the change log (TICKET-035) where the index is
  written: `created` or `updated` with the content hash from `index_page`, `deleted`
  from `forget_page`, `moved` with the old slug from `move_index_rows`, `updated` from a
  timeline entry. The first process to index a disk edit changes the shared hash, so the
  others find it unchanged and record nothing.
- The index keeps each page's properties as a JSON object in `brain_pages.frontmatter`
  (TICKET-047; until then the column held `{}`), written on every index and backfilled
  when an unchanged page is synced, so `brain_list_pages` answers properties without
  opening files.
- Writes to an existing page keep its frontmatter as written (TICKET-046).
  `update_page` and `add_timeline` swap the body under the original frontmatter bytes
  (`frontmatter::replace_body`), and `update_page` and a rename whose title follows the
  file name change only the `updated` or `title` line (`frontmatter::set_scalar_line`,
  which adds the line before the closing fence when it is missing). Key order, comments,
  quoting and unknown keys survive, and a page without frontmatter does not gain one. A
  key whose value spans lines, or an edit whose YAML would no longer parse (a key
  written `title : x` that the line match misses), goes through `set_property` instead,
  which keeps key order but re-serialises the mapping.
- Rename or move (`rename`): the file or folder moves; `links::rewrite_targets` rewrites
  every spelling of the old target in every page (exact slug, `.md`, leading `/`, and the
  bare file name when it was unique; markdown links too; fenced code untouched); the
  index rows follow by slug or by folder prefix (`move_index_rows`); one commit records
  it. A title that was the old file name follows the new one.
- Index: `sync_page` re-indexes when the content hash changed and refreshes the link
  rows either way; `sync_all` walks every folder, removes orphans, then resolves link
  rows whose targets arrived later. The back end runs `sync_all` after every burst of
  file changes, so edits made by Obsidian or an editor are indexed within seconds.
- Links: each row in `brain_links` holds the resolved slug (exact, case-insensitive;
  else a unique file name anywhere; else a unique title or alias) or the raw target, plus
  the line it sits on as context. `unresolved()` lists rows whose target is no page.
- Search: `parse_query` takes a query apart (`tag:`, `path:`, `file:` and `type:`
  terms, a value in quotes keeping its spaces, a leading `-` excluding; the words keep
  their quotes for FTS5 phrases). `search_with` narrows the pages by the operators
  (`tag:` through `brain_tags`, the rest over the indexed page rows), then matches the
  words through FTS5 (`porter unicode61`), or as typed when `case_sensitive`, or as a
  pattern over the indexed title and text when `regex` (ranked by match count, a
  snippet around the first match); operator terms alone list the admitted pages newest
  first. `search_hybrid_with` fuses FTS5 with vector hits through reciprocal rank
  fusion, applies the same operators to both halves, and hands the two text modes to
  `search_with`. `search` and `search_hybrid` are the same with the default options.
- Tags: `tags()` groups the index by tag with a page count, parents counting their
  nested tags too, for a client's tag pane and for agents.

## Semantic search and what leaves the machine

`resolve_embedder` reads `embedding_provider` (`auto`, `ollama`, `openai`, `off`),
`embedding_model` and `ollama_url` from settings and `openai_api_key` (or
`OPENAI_API_KEY`) from the secrets vault. `auto` picks Ollama only when it answers
locally; OpenAI is used only when the setting names it and a key exists, because it
sends page text off the machine. Vectors live in the `vec0` table `brain_vec`, created at
the model's width; changing the model rebuilds them. With no provider, search stays
full text and nothing else changes.

A capture is the one other thing that leaves the machine: a single GET of the URL the
user chose, with a `rusty` user agent and nothing else — no cookie, no key. What comes
back is data from the web, and every MCP answer that carries it says so (the mark in
`mcp-back-end.md`).

## Invariants

- Files are the truth; never write the database without the file.
- One `Mutex<Connection>`: a manager method never holds the guard across a call that
  takes it again (`sync_all` once deadlocked on that).
- Slugs never contain `..`; every path stays inside the vault root.
- Rewrites never touch fenced code, and never change a file where nothing resolved.

## Failure modes

- A page with broken YAML opens with empty frontmatter; a strict operation on it errors.
- A rename to an existing target is refused; a folder cannot move into itself.
- `sync_all` on a vault with pages that link by bare name resolves them in a second pass.
- An import that fails part way (a file where a folder must go, a disk full) leaves the
  brain as it was: the files the run created are removed and the index rebuilt; the
  folders it made may stay, empty. The source vault is never written in any path.
- A capture that fails (a fetch error, a page with no readable text, a PDF without
  `pdftotext` or without a text layer) is recorded on the source page — `status: failed`
  and `error` in the frontmatter, and for a new URL a body that says so — never an empty
  source; the next capture of the URL reads it again. The `sources/` folder appears in a
  vault on the next `ensure_dirs`, as every type folder does.

## Extension points

- New page types: `TYPE_DIRS` in `vault.rs` and the templates under `.templates/`.
- New providers: implement `Embedder` and add it to `resolve_embedder`.
- New index tables: additive `CREATE TABLE IF NOT EXISTS` in `engine/db.rs`.

## Tests

- `cargo test -p rusty-core brain::` covers parsing, the tree, folders, renames with
  every link spelling, the renderer, the scanner, the semantic index and migration.
- `crates/rusty-core/tests/integration.rs` exercises the managers together.
- `cargo test -p rusty-core import` covers the Obsidian import: the walk skipping
  dot-entries, the bookmarks parse, the report page, the plan on a fixture vault (a
  collision left alone, the tags, the unresolved link, bookmarks kept and not carried, the
  source byte for byte as before, the brain itself refused), the import (links rewritten,
  frontmatter kept, the attachment's bytes, search, tags and links after the run, the
  report page, the source unchanged), and the rollback.
- `cargo test -p rusty-core sources` and `capture_writes_updates_and_records_failures`
  cover the sources without the network: the HTML extractor on a fixture (chrome
  dropped, entities, headings, lists, the title and the `<h1>` fallback), the kind from
  the type then the bytes, the scheme refusal, the site and the slug, the normaliser, the
  mark on hits and on a page, and the capture from a fetched body — the keys, the index,
  the recapture keeping the slug and `created`, the two failure shapes.

## Primary sources

- `crates/rusty-core/src/brain/mod.rs`, `vault.rs`, `frontmatter.rs`, `links.rs`, `semantic.rs`
- `crates/rusty-core/src/engine/db.rs`, `crates/rusty-core/src/core.rs`
