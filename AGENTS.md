# Rusty: agent guide

Read [CONSTITUTION.md](CONSTITUTION.md) before changing this repository. It is binding.

## Product

Rusty is a local-first AI personal assistant for Omarchy:

- `crates/rusty-core`: the managers (tasks, notes, memories, brain vault and index,
  semantic index, skills, secrets, settings, events, watcher).
- `crates/rusty-mcp`: the back end, an MCP server (90 tools, resources, notifications)
  over stdio for agents and Streamable HTTP for Marley and other HTTP clients.
- `crates/rusty-cli`: terminal access to the same store.
- `crates/rusty-cmd`: the `rusty` command: `rusty session` (the back end's unit) and
  store scripts.
- Rusty has no window of its own: Marley, a Zed fork, draws a knowledge workspace over
  `rusty-mcp`. New UI work belongs there; what a front end needs from Rusty is a tool.
- `docs/architecture.md`: the standing shape. `ROADMAP.md`: the product list.

## Work routing

The work record (`docs/planning/`: tickets, pipelines, AARs, the knowledge register,
bulletins, intakes) is the maintainers' private checkout of its own, ignored by this
repository. In a clone without it, skip the record's steps below: make the change, run
`bin/gate.sh --diff`, and open a pull request; CI runs the same gate.

- Non-trivial feature, fix, migration or workflow changes use the repository skill
  `rusty-workflow` (`.claude/skills/rusty-workflow/SKILL.md`). Read it and its
  `references/phases.md` before touching files.
- Product exploration that should not produce code goes into
  `docs/planning/intake/` from `docs/planning/_templates/intake.md`.
- Read-only questions and diagnosis are answered directly.
- A user may waive the ceremony for a small change: write the reason to
  `docs/planning/pipeline/WAIVER.md`, report it at handoff, delete it after. Quality,
  tests, secrets and receipt rules still apply.
- One active spec/notes pair at a time.
- Commit and push only when the user has authorized delivery. A standing authorization,
  where one exists, is a bulletin in the record.

The pipeline:

```
recall → plan → design → implement → inspect → validate → complete → delivery
```

## Quality commands

```bash
bin/gate.sh --fast     # fmt, clippy, test; no receipt
bin/gate.sh --diff     # the delivery gate; green writes .git/rusty-gate-receipt
bin/gate.sh --verify   # does the receipt match this worktree
omarchy/install.sh     # rebuild and reinstall the binaries and the service on Omarchy
```

Run cargo commands one at a time. Never kill a running cargo.

## Local knowledge

Before designing or implementing:

1. `docs/planning/bulletins/INDEX.md`, then `docs/planning/knowledge/INDEX.md` for `PR-`,
   `BF-` and `AD-` entries that touch the work.
2. The nearest notes under `docs/planning/pipeline/completed/`.
3. `openwiki/quickstart.md` and the wiki pages the work touches (the generated
   engineering documentation), then `docs/architecture.md` and `docs/architecture/*.md`.
4. The brain: `brain_search` and `brain_context` through the `rusty` MCP server, which
   holds this project's pages and lessons.
5. CodeGraph for the Rust symbols, callers and blast radius; QML and shell by reading.

At complete, reconcile the wiki through the `openwiki` skill (its `openwiki_finish` must
return `complete`), then record lessons in the AAR, the knowledge register, and the
brain.

## Tools

- `.mcp.json` wires the `rusty` server, CodeGraph and OpenWiki for Claude Code;
  `.codex/config.toml` does the same for Codex. `scripts/setup-pipeline-tools.sh`
  installs CodeGraph and OpenWiki pinned and project-local under `.dev/` (ignored);
  `scripts/codegraph.sh` is CodeGraph's CLI wrapper, `scripts/mcp-openwiki.sh` OpenWiki's
  server. OpenWiki is used only through its MCP lifecycle (the host agent writes the
  pages; nothing is sent to a provider).
- Hooks in `.claude/settings.json` refuse: edits to gated paths outside an
  implementing pipeline (or a waiver), writes that contain something that looks like a
  secret, `git commit` without a matching gate receipt, and delivery of a completed
  pipeline without a matching OpenWiki completion receipt; a PostToolUse hook writes
  that receipt when `openwiki_finish` returns `complete`.

<!-- OPENWIKI:START -->

## OpenWiki

This repository has a generated `openwiki/` evidence index. It is optional just-in-time context, not required startup reading.

- Treat source code and tests as authoritative. A brief's unknowns and review items are verification gaps, not automatic requirements.
- Prefer the narrowest quiet validation that proves the changed behavior. Preserve complete failure output.

Refresh the repository wiki only through the project-local OpenWiki lifecycle (the openwiki skill, at Phase 5 of every pipeline). There is no scheduled or hosted refresh. Do not hand-edit generated OpenWiki pages; update the source or the docs and let the lifecycle regenerate the affected pages.

<!-- OPENWIKI:END -->
