# Rusty Constitution

Binding rules for everyone who changes this repository, human or agent. `AGENTS.md`
(which `CLAUDE.md` imports) routes the work; this file holds the rules it routes by. Amend
it by adding to the log at the end, never by silent edits.

## §0 Quality gate

- `bin/gate.sh` is the gate. `--fast` runs rustfmt, clippy with warnings as errors, and
  the tests. `--diff` (the default; `--full` is the same run) adds the doc build with
  warnings as errors, shell syntax checks, a secrets scan and a whitespace check, and on
  green writes the receipt `.git/rusty-gate-receipt`, bound to the worktree's gated content
  and `HEAD`.
- Only a receipt that matches the worktree proves the gate ran on what is being committed.
  The pre-commit hook (installed by `scripts/setup-pipeline-tools.sh`) and Claude Code's
  commit hook refuse a commit of gated files without one.
- Cargo commands run one at a time. Never start two, never kill a running one: a killed or
  concurrent cargo corrupts the incremental cache and forces a full rebuild.
- Red is fixed at the source. No baselines, no suppressions, no deleted tests, no
  `#[allow]` without the reason on the line above, no weakened dial to get green.
- GitHub Actions runs `bin/gate.sh --diff` on every push to `main` and every pull request.
  It never replaces the local receipt.

## §3 Work phases

```
recall → plan → design → implement → inspect → validate → complete → delivery
```

- At most one active spec/notes pair under `docs/planning/pipeline/active/`. Resume or
  disposition it before opening another.
- Phases close on evidence recorded in the notes, never on a claim. A test that did not
  run did not pass.
- Requirements are EARS statements with a verification method each. At complete, every
  requirement is satisfied with named evidence, split into a follow-up ticket, or
  waived with a reason. None are dropped silently.
- Inspect is adversarial and never skipped; it does not pass with an empty ledger.
- A maintainer may run a small change under a waiver: the reason is written to
  `docs/planning/pipeline/WAIVER.md` for the duration and reported at handoff. Quality,
  tests, secrets and receipt rules still apply under a waiver. A contributor without the
  record needs no waiver and must not create one (§19).

## §10 Product boundaries

- Files are the truth for pages. The brain is a folder of markdown any tool can open; the
  page index in SQLite can be rebuilt from the folder at any time. What has no file (to-do
  lists, memories, settings, the change log) lives in SQLite alone.
- Agents and front ends reach the store only through `rusty-mcp`'s tools and resources.
  `rusty-cli` runs the same managers in-process; nothing writes the database around them.
- Rusty has no window of its own. A front end owns no state the server does not; it calls
  tools and renders what they return, and what it needs from Rusty becomes a tool.
- Nothing personal ships: no vault pages, no screenshots of real data, no hostnames or
  accounts. Examples use mock data.
- Nothing is sent off the machine without a setting that says so (embeddings are the
  standing example).

## §14 Code conventions

- Idiomatic Rust: `Result<T, E>`, no panics on user paths, no `unsafe` without a safety
  comment. Public items and modules carry doc comments.
- Prose everywhere follows the `no-ai-slop` standard: docs, comments, commit messages,
  tool descriptions, user-facing strings, brain pages.

## §15 Evidence and anti-circumvention

- Never `--no-verify`, never edit `core.hooksPath`, never write or edit a receipt by hand,
  never claim a command ran that did not.
- Hooks are guardrails; the gate and its receipt are the proof. Where a host runs no hooks
  (Codex), the same checks are run by hand and by the pre-commit hook.
- A pipeline completes only after the generated wiki (`openwiki/`) has been reconciled
  through the project-local OpenWiki lifecycle and `openwiki_finish` returned
  `complete`; the PostToolUse hook writes `.git/rusty-openwiki-receipt`, bound to the
  worktree like the gate receipt; a host without the hook hands the finish result to the
  same script (`AGENTS.md`, "Tools and hosts"). A commit that delivers a completed
  pipeline needs that receipt to match, unless a waiver is in force. The receipt is never
  written by hand.
- A maintainer's commit names its ticket (`TICKET-001`); a commit an agent wrote carries a
  `Co-Authored-By:` trailer naming that agent.

## §18 Recall and inspection first

- Before planning or implementing: read `docs/planning/bulletins/INDEX.md` (a critical
  bulletin blocks work), search `docs/planning/knowledge/INDEX.md`, read the nearest
  completed pipeline notes, `openwiki/quickstart.md` and the wiki pages the work
  touches, and the relevant `docs/architecture/` documents. Search the maintainer's brain
  (`brain_search`, and `brain_ask` before a decision) for the project's own memory.
- CodeGraph is used at design and after implementation to see structural flows and blast
  radius (`codegraph_explore`, or `scripts/codegraph.sh explore` when the MCP server is
  not up yet). It reads Rust; shell scripts are read directly.

## §19 Local work record

- The record lives at `docs/planning/`, the maintainers' private checkout of its own,
  which this repository ignores. A clone without it follows the "Without `docs/planning`"
  section of `AGENTS.md`: it runs the gate and CI; the pipeline's checks step aside
  (`scripts/check-pipeline.sh`, the phase gate); the gate receipt rule for gated files and
  the secrets check still hold. Only the record's own checkout (a `docs/planning/` that is
  a git repository) turns the pipeline's checks on; a folder made by hand does not.
- Tickets: `docs/planning/tickets/{open,closed}/TICKET-NNN-slug.md`, numbered from
  `tickets/INDEX.md`. Never renumber.
- Pipeline: `docs/planning/pipeline/{active,completed}/<slug>.spec.md` and
  `<slug>.notes.md`, from `pipeline/_templates/`.
- Knowledge: `docs/planning/knowledge/INDEX.md` holds every `PR-` (prevention rule),
  `BF-` (bug family) and `AD-` (architecture decision) ID; each AAR lives in
  `knowledge/aar/`. New IDs go in both the AAR and the register.
- Durable lessons also go into the brain as a project page or memory, so the next session
  (in any tool) recalls them.

## Amendment log

- 2026-09-02: adopted, from three earlier agent workflows: a recall-first pipeline with a
  local record, a gate receipt and CodeGraph at design and inspect; sealed specs, EARS
  requirements and an evidence policy; the AAR shape and one active pipeline. OpenWiki and
  OpenViking were reviewed and not adopted as dependencies: Rusty's brain fills their role
  for this project.
- 2026-09-03 (TICKET-007): OpenWiki adopted for documentation at the maintainer's request, pinned
  and project-local, driven only through its MCP lifecycle by the host agent, required at
  Phase 5 with a completion receipt the delivery checks (§15, §18). The brain keeps the
  memory role.
- 2026-10-09 (TICKET-055): the work record left the public tree for a private checkout at
  `docs/planning/` (§19). The pipeline's checks step aside in a clone without it, and a
  pipeline being delivered is a completed spec the record has not committed yet (§15).
- 2026-10-09 (TICKET-060): after the retired app, §10 and §14 lost their app and QML rules
  and §10 states which data has no file; `CLAUDE.md` imports `AGENTS.md`, the one guide;
  CI runs `bin/gate.sh --diff` (§0); a host without hooks runs the checks by hand and
  records the OpenWiki receipt through the hook script (§15); the waiver is the
  maintainers' alone (§3, §19); `brain_ask` replaces a `brain_context` tool that never
  existed (§18).
- 2026-10-09 (TICKET-063): the commit gate reads every form of `git commit`, refuses `-n`,
  needs `jq`, and checks that what is staged is what the gate saw (§0, §15); the OpenWiki
  receipt needs OpenWiki's own record of a complete run at `HEAD` (§15); the record is
  recognised by its own git checkout (§19).
