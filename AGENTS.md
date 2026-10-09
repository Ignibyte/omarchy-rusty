# Rusty: agent guide

The guide for anyone changing this repository, agent or person. Codex reads this file;
Claude Code reads it through `CLAUDE.md`. [CONSTITUTION.md](CONSTITUTION.md) holds the
binding rules.

## Product

Rusty is a local-first memory and knowledge store for AI agents on Omarchy:

- `crates/rusty-core`: the managers (to-do lists, notes, memories, the brain vault and its
  SQLite index, semantic search, skills and scripts, secrets and the PIN, settings, the
  change log, the file watcher, export and import).
- `crates/rusty-mcp`: the MCP server, 90 tools plus resources and change notifications,
  over stdio for agents and Streamable HTTP for front ends.
- `crates/rusty-cli`: the terminal client. It runs the same managers in-process.
- `crates/rusty-cmd`: the `rusty` command: `rusty session start|status` for the back end's
  systemd user service, and store scripts as `rusty <name>`.

Rusty has no window. [Marley](https://github.com/Ignibyte/marley_ide), a Zed fork, draws a
knowledge workspace over `rusty-mcp`; when a front end needs something from Rusty, the
answer is a new tool. `docs/architecture.md` shows how the crates and the store fit
together, `docs/tools.md`, `docs/cli.md` and `docs/configuration.md` are the references,
and `ROADMAP.md` lists what 0.1.0 holds and what comes next.

## Without `docs/planning` (contributors)

The maintainers keep a work record (tickets, specs, after-action reviews, a knowledge
register) in a private checkout at `docs/planning/`, which this repository ignores. In a
clone without it, follow this section and skip every step further down that names
`docs/planning`:

1. Read the code you will change, `openwiki/quickstart.md`, the wiki page for that
   subsystem, and `docs/architecture.md`.
2. Make the change with its tests. Tests use a scratch directory or `HOME`, never a real
   store.
3. Run `bin/gate.sh --diff` until it prints `GATE GREEN [diff]`.
4. Open a pull request. CI runs the same gate.

You need no waiver and no `docs/planning/`; the hooks only treat that folder as the record
when it is a git checkout of its own. Under Claude Code two hooks still apply, and they
need `jq`. A commit made while gated files (the list is `rusty_gated_paths` in
`bin/lib-gate.sh`) differ from `HEAD` needs a gate receipt for that exact tree and `HEAD`,
so run `bin/gate.sh --diff` again after every change, and stage gated files as the gate
saw them; `-n` and `--no-verify` are refused. A write that looks like a credential is
refused too. You do not need the MCP servers in `.mcp.json`, and you do not update
`openwiki/`; the maintainers reconcile it. If you installed Rusty's own brain-loop hooks
(`rusty-cli hooks install`), they also act here, because this repository's `.mcp.json`
names a `rusty` server: answer them with `brain_ask`, or work with them uninstalled.

## With the record (maintainers)

- Non-trivial features, fixes, migrations and workflow changes run through the
  `rusty-workflow` skill (`.claude/skills/rusty-workflow/SKILL.md`). Read it and its
  `references/phases.md` before touching files. The pipeline:

  ```
  recall → plan → design → implement → inspect → validate → complete → delivery
  ```

- Product exploration that should not produce code is an intake in
  `docs/planning/intake/`, from `docs/planning/_templates/intake.md`.
- Read-only questions and diagnosis are answered directly.
- A maintainer may waive the ceremony for a small change: the reason goes in
  `docs/planning/pipeline/WAIVER.md`, is reported at handoff, and the file is deleted
  after. Quality, tests, secrets and receipt rules still apply.
- At most one active spec/notes pair.
- Commit and push only when the maintainer has authorized delivery. A standing
  authorization, where one exists, is a bulletin in the record.

Before designing or implementing, recall:

1. `docs/planning/bulletins/INDEX.md`, then `docs/planning/knowledge/INDEX.md` for `PR-`,
   `BF-` and `AD-` entries that touch the work.
2. The nearest notes under `docs/planning/pipeline/completed/`.
3. `openwiki/quickstart.md` and the wiki pages the work touches, then
   `docs/architecture.md` and `docs/architecture/*.md`.
4. The maintainer's brain through the `rusty` MCP server: `brain_search`, and `brain_ask`
   before a decision.
5. CodeGraph for Rust symbols, callers and blast radius; shell scripts by reading.

At complete, update the docs first, then reconcile the wiki through the OpenWiki skill
(`.claude/skills/openwiki/SKILL.md`; `openwiki_finish` must return `complete`), then record
the lessons in the AAR, the knowledge register and the brain.

## Quality commands

```bash
bin/gate.sh --fast     # fmt, clippy, tests; no receipt
bin/gate.sh --diff     # also docs, shell syntax, secrets, whitespace; green writes .git/rusty-gate-receipt
bin/gate.sh --verify   # does the receipt match this worktree
./install.sh           # build (or copy a release's) binaries, install and restart the service
```

Run cargo commands one at a time. Never kill a running cargo.

## Tools and hosts

- `.mcp.json` (Claude Code) and `.codex/config.toml` (Codex) wire three MCP servers:
  `rusty` (needs `rusty-mcp` on `PATH`, which `install.sh` provides), CodeGraph and
  OpenWiki. `scripts/setup-pipeline-tools.sh` installs CodeGraph and OpenWiki pinned under
  `.dev/` (ignored), and a `.git/hooks/pre-commit` that checks the receipts for every
  committer. `scripts/codegraph.sh` is CodeGraph's CLI. OpenWiki runs only through its MCP
  lifecycle: the agent writes the pages and nothing goes to a provider.
- Claude Code runs the hooks in `.claude/settings.json` (`CLAUDE.md` lists them).
- Codex runs none of them. Run `bin/gate.sh --diff` before each commit and keep the
  pre-commit hook installed. After a real `openwiki_finish` that returned `complete`, a
  maintainer records the OpenWiki receipt by handing that result to the hook script:

  ```bash
  echo '{"tool_name":"mcp__openwiki__openwiki_finish","tool_response":{"content":[{"type":"text","text":"{\"status\":\"complete\"}"}]}}' \
    | bash .claude/hooks/record-pipeline-tool-use.sh
  ```

  The script writes the receipt only when `openwiki/.last-update.json` records a complete
  run at the current `HEAD`. The same command serves under Claude Code when its PostToolUse
  hook does not fire.

<!-- OPENWIKI:START -->

## OpenWiki

This repository has a generated `openwiki/` evidence index. It is optional just-in-time context, not required startup reading.

- Treat source code and tests as authoritative. A brief's unknowns and review items are verification gaps, not automatic requirements.
- Prefer the narrowest quiet validation that proves the changed behavior. Preserve complete failure output.

Refresh the repository wiki only through the project-local OpenWiki lifecycle (the openwiki skill, at Phase 5 of every pipeline). There is no scheduled or hosted refresh. Do not hand-edit generated OpenWiki pages; update the source or the docs and let the lifecycle regenerate the affected pages.

<!-- OPENWIKI:END -->
