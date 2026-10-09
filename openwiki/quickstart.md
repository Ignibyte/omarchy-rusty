---
type: "Reference"
title: "Rusty engineering quickstart"
openwiki_generated: true
sources:
  - id: openwiki-source-651d1fb6c9e49916a916ab51
    resource: repo://Cargo.toml
  - id: openwiki-source-ee85702f1c240ca46ba70c7b
    resource: repo://crates/rusty-cmd/Cargo.toml
  - id: openwiki-source-b85e7e6c4128c2759dc6f275
    resource: repo://crates/rusty-cmd/src/main.rs
  - id: openwiki-source-052c1f89ee9ee9321b802fb2
    resource: repo://crates/rusty-cmd/src/session.rs
  - id: openwiki-source-a8160e123db68363371d6a65
    resource: repo://crates/rusty-cmd/tests/command.rs
generated: {by: "claude-code", at: "2026-10-09T18:25:07.825Z"}
verified:
  - by: openwiki/0.3.3
    at: 2026-10-09T18:25:07.825Z
---

# Rusty engineering quickstart

Rusty is a local-first memory and knowledge store for AI agents on Omarchy: an MCP server
over a markdown vault any tool can open, with to-do lists, notes, memories, skills and
secrets, a CLI, and the `rusty` command. Marley, a Zed fork, draws a knowledge workspace
over the server. It serves one person on one machine and sends nothing off it unless a
setting says so.

The user-facing references sit beside this wiki: `docs/tools.md` (every tool, generated
from the router), `docs/cli.md`, `docs/configuration.md` and `docs/architecture.md`.

## The parts

| Part | What it owns | Page |
|---|---|---|
| `crates/rusty-core` | the managers: to-do lists, notes, memories, the brain vault and its SQLite index, the renderer, semantic search, skills, secrets, settings, the change log, the file watcher, export and import | [Vault and brain](vault-and-brain.md), [Markdown rendering](markdown-rendering.md), [Export and import](export-and-import.md) |
| `crates/rusty-mcp` | the back end: 90 tools (among them `changes_since`, the change cursor, and four for bookmarks), five resources, change notifications, a background indexer; stdio for agents, Streamable HTTP for Marley and other HTTP clients | [MCP back end](mcp-back-end.md) |
| `crates/rusty-cmd` | the `rusty` command, with no dependencies: `rusty session start` and `status` for the back end's unit, and store scripts | [Development and validation](development-and-validation.md) |
| `crates/rusty-cli` | terminal access to the same store, through the managers in-process: the brain and its upkeep, sources, notes, skills and scripts, the brain loop and its hooks, conversation ingest, the change feed, bookmarks, export and import | `docs/cli.md` |
| `docs/architecture/brain-loop.md` | the design of ask, decide, follow up | [MCP back end](mcp-back-end.md) |
| `CONSTITUTION.md`, `docs/planning/`, `bin/`, `.claude/`, `scripts/` | the workflow: phases, record, gate and receipts, hooks, CodeGraph, OpenWiki | [Workflow and gates](workflow-and-gates.md) |

## Run it

- `rusty session start` starts the back end's user unit and prints its status;
  `rusty session status` reads the unit and probes the port. `rusty` alone, like
  `rusty help`, prints the usage. A flag, any other session verb, or a bare word that is
  neither a noun nor a store script prints the usage and exits 2.
- `rusty-mcp` serves agents over stdio; the user service `rusty-mcp.service` serves Marley
  and other HTTP clients at `http://127.0.0.1:4174/mcp` and comes back on its own after a
  kill.
- `rusty-cli --help` lists the terminal commands; `rusty-cli refresh` nudges the watcher
  after a raw write; `rusty-cli export` and `import` move a whole store in one zip (see
  [Export and import](export-and-import.md)).
- Data lives in `~/.rusty/`: `rusty.db`, `brain/` (the vault, a git repository, with
  notes in `brain/notes/`), `skills/`, `.secret`, `.pin`, `hooks/`
  (`docs/configuration.md` has the full layout).

## Build, test, gate

```bash
cargo build                       # one cargo command at a time, never killed
bin/gate.sh --fast                # fmt, clippy (-D warnings), tests
bin/gate.sh --diff                # the delivery gate; green writes .git/rusty-gate-receipt
bin/gate.sh --verify              # do the receipts match this worktree
omarchy/install.sh                # binaries into ~/.local/bin, the back end's unit
```

Details and the reasons behind them: [Development and validation](development-and-validation.md).

## Where a change starts

A contributor's change starts from the code, this wiki and `docs/architecture.md`, and
ends with `bin/gate.sh --diff` and a pull request (`AGENTS.md`, "Without
`docs/planning`"). The maintainers run the pipeline in `CONSTITUTION.md` §3 (recall,
plan, design, implement, inspect, validate, complete, delivery) with the
`rusty-workflow` skill; its recall reads their private record at `docs/planning/`, which
a clone does not have. The pages here name the owning module and the narrowest test for
each area so a change can find both quickly.

## Primary sources

- `README.md`, `AGENTS.md` (which `CLAUDE.md` imports), `CONSTITUTION.md`, `ROADMAP.md`
- `docs/architecture.md`, `docs/tools.md`, `docs/cli.md`, `docs/configuration.md`
- `bin/gate.sh`, `omarchy/install.sh`
- `crates/rusty-cmd/src/main.rs` and `session.rs` (the dispatch and the `rusty session` verbs)
