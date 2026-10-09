# Changelog

What each release changed, newest first. Versions follow semantic versioning; before 1.0
a minor version may change a tool or a command.

## [0.1.1] - 2026-10-09

Fixes and hardening after 0.1.0.

- The installer is `install.sh` at the top of a checkout and of a release; the files it
  installs (the unit, the MCP config, the memory-pressure drop-in) are in `service/`.
- Search waits at most five seconds for a query's embedding, then answers from the text
  index, so a hung Ollama no longer holds a search for two minutes.
- Two processes indexing one changed file record it once: the check and the write share a
  transaction.
- `source_capture`, `brain_import` and `brain_rename` run off the request's task, so a long
  one no longer holds the server up.
- Bookmarks stay inside the vault, and `rusty-cli bookmarks rm` removes a folder bookmark
  whose folder was deleted outside Rusty.
- `import --replace` through a symbolic link replaces the store where it lives, and a
  server still holding a deleted database blocks an import.
- Transcripts without a session id no longer overwrite each other in the archive.
- `rusty session status` names and resolves the address it probes (`RUSTY_MCP_ADDR`).
- A client that stops reading no longer holds up change notifications to the others.
- The sweep of outside edits leaves files written in the last three seconds to the process
  that wrote them.
- `pdftotext` gets thirty seconds and 8 MiB of output, and the PDF's temporary copy is
  private.

## [0.1.0] - 2026-10-09

The first public release.

- `rusty-mcp`: an MCP server with 90 tools over stdio and Streamable HTTP, resources under
  `rusty://`, `resources/list_changed` notifications, and a feed of every change in the
  store, whichever process made it (`changes_since`).
- The brain: a markdown vault with frontmatter, wikilinks as vault paths, a `## Timeline`
  section per page, soft deletes, git history, full-text search with operators, typed
  blocks and HTML rendering, tags, properties, aliases, bookmarks, a graph, and an
  Obsidian vault import.
- Semantic search with a local Ollama model, or OpenAI only when set and keyed.
- The brain loop (`brain_ask`, `brain_decide`, `brain_follow_up`, `brain_due`) and two
  optional Claude Code hooks (`rusty-cli hooks install`).
- To-do lists, memories, notes, settings (credentials masked on read), and secrets behind
  a PIN.
- A skills store with staging, a safety scan, and `*.sh` scripts run as `rusty <name>`.
- Sources: web pages, PDFs and files captured as pages and marked untrusted.
- `rusty-cli export` and `import`: a whole store in one zip, secrets only on request.
- A conversation archive: `rusty-cli ingest-conversation` keeps Claude Code transcripts as
  searchable text and brain pages; `search_conversations` searches it.
- `rusty-cli` for the terminal (settings included), `rusty` for the service and store
  scripts, a systemd user service and an installer that builds from source or installs a
  release's binaries.
- References generated from or checked against the code: `docs/tools.md`, `docs/cli.md`,
  `docs/configuration.md`.
