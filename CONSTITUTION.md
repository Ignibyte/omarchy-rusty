# Rusty Constitution

Binding rules for everyone who changes this repository, human or agent. `AGENTS.md`
(also `CLAUDE.md`) routes the work; this document is the law it routes by. Amend it by
adding to the log at the end, never by silent edits.

## §0 Quality gate

- `bin/gate.sh` is the gate. `--fast` runs rustfmt, clippy with warnings as errors, and
  the tests. `--diff` (the default) and `--full` add the doc build with warnings as errors,
  shell syntax checks, a secrets scan and a whitespace check, and on green write the
  receipt `.git/rusty-gate-receipt`, bound to the worktree's gated content and `HEAD`.
- Only a receipt that matches the worktree proves the gate ran on what is being committed.
  The pre-commit hook and the agent hooks refuse a commit of gated files without one.
- Cargo commands run one at a time. Never start two, never kill a running one: a killed or
  concurrent cargo corrupts the incremental cache and forces a full rebuild.
- Red is fixed at the source. No baselines, no suppressions, no deleted tests, no
  `#[allow]` without the reason on the line above, no weakened dial to get green.
- GitHub Actions runs the same gate set on every push as a second witness. It never
  replaces the local receipt.

## §3 Work phases

```
recall → plan → design → implement → inspect → validate → complete → delivery
```

- Exactly one active spec/notes pair under `docs/planning/pipeline/active/`. Resume or
  disposition it before opening another.
- Phases close on evidence recorded in the notes, never on a claim. A test that did not
  run did not pass. Writing a test file is not testing.
- Requirements are EARS statements with a verification method each. At complete, every
  requirement is satisfied with named evidence, split into a follow-up ticket, or
  waived with a reason. None are dropped silently.
- Inspect is adversarial and never skipped. An empty ledger is not a ledger.
- A small change may run with a waiver: the reason is written to
  `docs/planning/pipeline/WAIVER.md` for the duration and reported at handoff. Quality,
  tests, secrets and receipt rules still apply under a waiver.

## §10 Product boundaries

- Files are the truth. The brain is a folder of markdown any tool can open; SQLite holds
  derived indexes that can be rebuilt from the folder at any time.
- The back end is MCP only. The app, the CLI and any agent reach the store through
  `rusty-mcp`'s tools and resources, never through the database directly.
- The app is keyboard first and themed by Omarchy. Every surface answers "what can I do
  with the keyboard", and colours come from the current theme's tokens.
- Nothing personal ships: no vault pages, no screenshots of real data, no hostnames or
  accounts. Design references use mock data.
- Nothing is sent off the machine without a setting that says so (embeddings are the
  standing example).

## §14 Code conventions

- Idiomatic Rust: `Result<T, E>`, no panics on user paths, no `unsafe` without a safety
  comment. Public items and modules carry doc comments.
- QML pages own no state the server does not; they call tools and render JSON.
- Prose everywhere follows the `no-ai-slop` standard: docs, comments, commit messages,
  UI strings, brain pages.

## §15 Evidence and anti-circumvention

- Never `--no-verify`, never edit `core.hooksPath`, never write or edit a receipt by hand,
  never claim a command ran that did not.
- Hooks are guardrails; the gate and its receipt are the proof. A hook that is missing
  does not lower the bar.
- A pipeline completes only after the generated wiki (`openwiki/`) has been reconciled
  through the project-local OpenWiki lifecycle and `openwiki_finish` returned
  `complete`; the PostToolUse hook writes `.git/rusty-openwiki-receipt`, bound to the
  worktree like the gate receipt. A commit that delivers a completed pipeline needs
  that receipt to match, unless a waiver is in force. The receipt is never written by
  hand.
- A maintainer's commit names its ticket (`TICKET-001`); a commit an agent wrote carries a
  `Co-Authored-By:` trailer naming that agent.

## §18 Recall and inspection first

- Before planning or implementing: read `docs/planning/bulletins/INDEX.md` (a critical
  bulletin blocks work), search `docs/planning/knowledge/INDEX.md`, read the nearest
  completed pipeline notes, `openwiki/quickstart.md` and the wiki pages the work
  touches, and the relevant `docs/architecture/` documents. Search the brain
  (`brain_search`, `brain_context`) for the project's own memory.
- CodeGraph is used at design and after implementation to see structural flows and blast
  radius (`codegraph_explore`, or `scripts/codegraph.sh explore` when the MCP server is
  not up yet). It reads Rust; QML and shell are inspected directly.

## §19 Local work record

- The record lives at `docs/planning/`, the maintainers' private checkout of its own,
  which this repository ignores. A clone without it runs the gate and CI; the pipeline's
  checks step aside (`scripts/check-pipeline.sh`, the phase gate), and the gate receipt
  rule for gated files still holds.
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
