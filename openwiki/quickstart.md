---
type: "Reference"
title: "Rusty engineering quickstart"
openwiki_generated: true
sources:
  - id: openwiki-source-6db403fd15f39cd68c2bfe93
    resource: repo://crates/rusty-app/src/agent/mod.rs
  - id: openwiki-source-68599611588cfbbf1f2b222b
    resource: repo://crates/rusty-app/src/backend.rs
  - id: openwiki-source-c8c0347aa7a687c601520d1a
    resource: repo://crates/rusty-app/src/main.rs
  - id: openwiki-source-188c50fac039d5c4d0e7eca9
    resource: repo://crates/rusty-app/src/session.rs
generated: {by: "claude-code", at: "2026-09-17T22:06:12.177Z"}
verified:
  - by: openwiki/0.3.3
    at: 2026-09-17T22:06:12.177Z
---

# Rusty engineering quickstart

Rusty is a local-first assistant for Omarchy: a knowledge workspace laid out as Obsidian
lays out a vault, over a markdown folder any tool can open, with a pure MCP back end and
native Claude Code and Codex terminals. One machine, one person, nothing sent off the
box without a setting that says so.

## The parts

| Part | What it owns | Page |
|---|---|---|
| `crates/rusty-core` | the managers: tasks, notes, memories, the brain vault and its SQLite index, the renderer, semantic search, skills, secrets, settings, the file watcher | [Vault and brain](vault-and-brain.md), [Markdown rendering](markdown-rendering.md) |
| `crates/rusty-mcp` | the back end: 85 tools, five resources, change notifications, a background indexer; stdio for agents, Streamable HTTP for the app | [MCP back end](mcp-back-end.md) |
| `crates/rusty-app` | the desktop app (binary `rusty`): the workspace in QML on cxx-qt, terminals on tmux, theme from Omarchy, the agent sessions that outlive it, and the `rusty session …` and `rusty agent …` commands answered before Qt starts | [Workspace app](workspace-app.md) |
| `crates/rusty-cli` | terminal access to the same store (brain, tasks, notes, refresh, conversation ingest) | [MCP back end](mcp-back-end.md) |
| `CONSTITUTION.md`, `docs/planning/`, `bin/`, `.claude/`, `scripts/` | the workflow: phases, record, gate and receipts, hooks, CodeGraph, OpenWiki | [Workflow and gates](workflow-and-gates.md) |

## Run it

- `rusty session start` starts the back end and the app under their user units (`stop`,
  `status`; `rusty help` lists the nouns); SUPER+ALT+R under Omarchy runs it. `rusty` alone
  opens the app tied to the terminal it was typed in; a bare word that is neither a noun
  nor a store script prints the usage and exits 2.
- `rusty-mcp` serves agents over stdio; the user service `rusty-mcp.service` serves the
  app at `http://127.0.0.1:4174/mcp` and comes back on its own after a kill.
- `rusty-cli --help` lists the terminal commands; `rusty-cli refresh` nudges the watcher
  after a raw write.
- `rusty agent list` shows the Claude Code conversations running on this machine; each is
  its own user unit (`rusty agent attach <id>` watches one, `stop` and `rm` end it).
- Data lives in `~/.rusty/`: `rusty.db`, `brain/` (the vault, a git repository),
  `notes/`, `skills/`, `.secret`.

## Build, test, gate

```bash
cargo build                       # one cargo command at a time, never killed
bin/gate.sh --fast                # fmt, clippy (-D warnings), tests
bin/gate.sh --diff                # the delivery gate; green writes .git/rusty-gate-receipt
bin/gate.sh --verify              # do the receipts match this worktree
omarchy/install.sh                # release build into ~/.local/bin, both units, the desktop entry
scripts/screenshot.sh <dir>       # the docs' screenshots, offscreen, scratch vault
```

Details and the reasons behind them: [Development and validation](development-and-validation.md).

## Where a change starts

Every non-trivial change runs the pipeline in `CONSTITUTION.md` §3 (recall, plan,
design, implement, inspect, validate, complete, delivery) with the `rusty-workflow`
skill. Recall reads `docs/planning/knowledge/INDEX.md`, the nearest completed notes,
this wiki, and the brain. The pages here name the owning module and the narrowest test
for each area so a change can find both quickly.

## Primary sources

- `README.md`, `AGENTS.md` (also `CLAUDE.md`), `CONSTITUTION.md`, `ROADMAP.md`
- `docs/architecture.md`
- `bin/gate.sh`, `omarchy/install.sh`, `scripts/screenshot.sh`
- `crates/rusty-app/src/session.rs` (the `rusty session` verbs and the dispatch)
- `crates/rusty-app/src/agent/mod.rs` (the `rusty agent` verbs and where a session's files live)
