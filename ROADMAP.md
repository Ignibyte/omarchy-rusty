# Roadmap

What Rusty is for, what 0.1.0 holds, and what comes next. The shape is in
`docs/architecture.md`.

## In 0.1.0

- The store: a markdown vault with frontmatter, wikilinks as vault paths, a `## Timeline`
  section per page, soft deletes into `archive/`, git history, and a SQLite index for
  full-text search, links, tags, aliases and properties.
- Semantic search over the vault with a local Ollama model (or OpenAI when you say so),
  merged with the full-text hits.
- The brain loop: `brain_ask`, `brain_decide`, `brain_follow_up` and `brain_due`, with
  two optional Claude Code hooks that make an agent ask before it writes and record what
  it decided.
- To-do lists, memories, notes, settings (also from a terminal, `rusty-cli settings`), and
  secrets behind a PIN.
- A skills store whose `*.sh` files are commands (`rusty <name>`), with optional staging
  and a safety scan before a staged skill is approved.
- Sources: a web page, PDF or text fetched from a URL and kept as a page, marked untrusted
  when an agent reads it.
- An Obsidian vault import; bookmarks in the vault; a feed of every change in the store
  (`changes_since`).
- A conversation archive of Claude Code transcripts.
- `rusty-mcp` (90 tools, stdio and Streamable HTTP), `rusty-cli`, `rusty`, a systemd user
  service and an installer.
- `rusty-cli export` and `import`: a whole store in one zip.

## Next

- `rusty-bin` and `rusty-git` on the AUR.
- Staging by default for skills an agent writes, so nothing an agent authors runs before
  you approve it.
- An encrypted export, so a backup can carry the secrets safely.
- An export tool over MCP for clients such as [Marley](https://github.com/Ignibyte/marley_ide),
  without the secrets.
- Tiered context for agents: an abstract, an overview and the details per page and
  folder, so an agent reads the least it needs.
- Turning a finished agent session into memories and timeline entries.
- Merging an export into an existing store.

## Principles

- **Local first.** One machine, one person. Nothing leaves the machine unless a setting
  says so.
- **Files are the truth for knowledge.** The vault is a folder of markdown any tool can
  open, and SQLite holds an index that can be rebuilt from it. Tasks, memories and
  settings live in the database, and an export carries them.
- **One back end.** `rusty-mcp` serves every MCP client: agents over stdio, long-running
  clients over local HTTP; `rusty-cli` uses the same managers in-process. No web UI, no
  REST layer, no second protocol.
- **Linux with systemd, at home on Omarchy.** The service and the installer need a systemd
  user session; nothing else assumes a desktop.
- **The same gate on every change.** `bin/gate.sh --diff`, locally and in CI: rustfmt,
  clippy with warnings as errors, the tests, the docs build, shell syntax, a secrets scan.

## Non-goals

No browser UI, no voice, no cloud service, no multi-user. Rusty runs on one machine for one
person and talks to the agents that person already uses.
