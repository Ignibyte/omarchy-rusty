# Changelog

What each release changed, newest first. Versions follow semantic versioning; before 1.0
a minor version may change a tool or a command.

## [0.1.0] - unreleased

The first public release.

- `rusty-mcp`: an MCP server with 90 tools over stdio and Streamable HTTP, resources under
  `rusty://`, `resources/list_changed` notifications and a change log any client can
  follow with `changes_since`.
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
- `rusty-cli` for the terminal, `rusty` for the service and store scripts, a systemd user
  service and an installer that builds from source or installs a release's binaries.
