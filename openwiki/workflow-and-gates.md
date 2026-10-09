---
type: "Reference"
title: "Workflow and gates: how a change moves through this repository"
openwiki_generated: true
sources:
  - id: openwiki-source-56983e7ea0f736093e014335
    resource: repo://.claude/hooks/enforce-commit-gate.sh
  - id: openwiki-source-2c698cba981b7b90d74aa109
    resource: repo://.claude/hooks/enforce-phase-gate.sh
  - id: openwiki-source-3426cd295e36cea6300feda3
    resource: repo://.claude/hooks/enforce-secrets.sh
  - id: openwiki-source-17abde28c4891dbc12b5b92d
    resource: repo://.claude/hooks/record-pipeline-tool-use.sh
  - id: openwiki-source-1fe463fcf07912e5cdbb5a91
    resource: repo://.claude/settings.json
  - id: openwiki-source-ea70eb6c045047448e446296
    resource: repo://.gitignore
  - id: openwiki-source-0bb8016edf4f4744d3a09cf4
    resource: repo://bin/gate.sh
  - id: openwiki-source-0118ed911c8f8689e6c1c0a1
    resource: repo://bin/lib-gate.sh
  - id: openwiki-source-a2371d6362e5db4bc834ad03
    resource: repo://CLAUDE.md
  - id: openwiki-source-307dfaff33e13bcb825730f9
    resource: repo://crates/rusty-cli/hooks/brain-ask-before-write.sh
  - id: openwiki-source-c176a432c8e7e89d8ba4f4a3
    resource: repo://crates/rusty-cli/hooks/brain-decide-before-stop.sh
  - id: openwiki-source-428803b45520d00a9ba153d7
    resource: repo://crates/rusty-cli/src/hooks.rs
  - id: openwiki-source-ce2dcb83054d44e4820d1fa8
    resource: repo://scripts/check-pipeline.sh
  - id: openwiki-source-58e168516499e3589535de84
    resource: repo://scripts/lib-openwiki.sh
generated: {by: "claude-code", at: "2026-10-09T18:25:07.825Z"}
verified:
  - by: openwiki/0.3.3
    at: 2026-10-09T18:25:07.825Z
---

# Workflow and gates: how a change moves through this repository

## Purpose

Every non-trivial change runs a spec-driven, phase-gated pipeline whose record is plain
files and whose proof is a pair of receipts bound to the worktree. `CONSTITUTION.md` holds
the binding rules, `AGENTS.md` routes the work (Codex reads it; `CLAUDE.md` imports it for
Claude Code), and the `rusty-workflow` skill walks the phases. The record is the
maintainers' private checkout at `docs/planning`; a contributor's clone has none and
follows the "Without `docs/planning`" section of `AGENTS.md`: the change with its tests,
`bin/gate.sh --diff`, a pull request.

## Ownership

- `CONSTITUTION.md`: the quality gate (§0), the phases (§3), product boundaries (§10),
  code conventions (§14), evidence and anti-circumvention (§15), recall first (§18),
  the local record (§19), the amendment log.
- `docs/planning/`: the work record, a private repository checked out at that path and
  ignored by this one: intakes, tickets (`INDEX.md` holds the next number), the active
  spec/notes pair (at most one), completed pairs, the knowledge register (`PR-`, `BF-`,
  `AD-` IDs), AARs, bulletins.
- `bin/gate.sh`, `bin/lib-gate.sh`: the gate and the receipts.
- `.claude/hooks/`, `.claude/settings.json`: the guardrails.
- `scripts/`: the pipeline tools (`setup-pipeline-tools.sh`, `check-pipeline-tools.sh`,
  `check-pipeline.sh`, `codegraph.sh`, `mcp-codegraph.sh`, `mcp-openwiki.sh`,
  `lib-openwiki.sh`).
- `.claude/skills/rusty-workflow/` (the phases), `.claude/skills/openwiki/` (the wiki
  lifecycle).

## The phases

```
recall → plan → design → implement → inspect → validate → complete → delivery
```

- Recall: bulletins, the knowledge register, the nearest completed notes, this wiki,
  the architecture docs, the brain through the `rusty` MCP server.
- Plan: a ticket, a spec with EARS requirements (each with a verification), locked
  decisions, a notes file, an open AAR. A spec that changes what Rusty is carries the
  user's seal on its `sealed:` line.
- Design: architecture, a file manifest, store consequences, the tool contract, a
  regression table, risks, CodeGraph evidence over the symbols touched.
- Implement: the manifest, `bin/gate.sh --fast` green.
- Inspect: an adversarial ledger through fixed lenses, findings resolved.
- Validate: the tests with their real output, `bin/gate.sh --diff` green, the receipt.
- Complete: the requirement audit, the `openwiki` lifecycle finished `complete`, docs,
  the AAR, register entries, a brain capture, the pair archived.
- Delivery: `bin/gate.sh --verify` green, the staged diff read, a commit that names its
  ticket and carries the co-author trailer, the push, the CI run; then the record's own
  commit, after the public one.

## The gate and the receipts

- `bin/gate.sh --fast`: `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --workspace`, one after another.
- `--diff` (the default; `--full` is the same run) adds the doc build with warnings as errors, shell
  syntax, a secrets scan over the gated files, a whitespace check, and on green write
  `.git/rusty-gate-receipt`: `version`, `fingerprint`, `mode`, `at`.
- The fingerprint is a sha256 over `HEAD` and every gated file's path and content. Gated
  paths: `crates`, `Cargo.toml`, `Cargo.lock`, `bin`, `scripts`,
  `omarchy`, `packaging`, `.claude`, `.codex`, `.mcp.json`, `.github`,
  `CONSTITUTION.md`, `AGENTS.md`, `CLAUDE.md`. `docs/` and the roadmap are exempt, so a
  pipeline can write notes while a gate runs and docs-only changes commit without a
  receipt.
- `.git/rusty-openwiki-receipt` has the same shape plus the pipeline id. The
  PostToolUse hook writes it when `openwiki_finish` returns `complete`. Neither receipt
  is ever written by hand.
- `--verify` reports the OpenWiki receipt when the record is checked out, and exits on the
  gate receipt. CI runs `--diff` on every push to `main` and every pull request.
- The library and the scripts find the repository from their own file's location, not
  the current directory: inside the nested record, git answers for the record.

## The guardrails

- `enforce-phase-gate.sh` (PreToolUse on Edit, Write and MultiEdit): a gated path changes only while
  the active spec is in an implementing phase, or under `docs/planning/pipeline/WAIVER.md`.
  With no record checked out it allows: there is no pipeline to enforce.
- `enforce-secrets.sh` (the same matcher): a write that contains credential-looking bytes
  is refused, in a contributor's clone too.
- `enforce-commit-gate.sh` (PreToolUse on Bash): a `git commit` that carries gated files
  needs a matching gate receipt; a public commit made while the record holds a completed
  spec it has not committed yet delivers that pipeline and needs a matching OpenWiki
  receipt too, unless a waiver is in force; a commit aimed at the record is exempt;
  `--no-verify` is refused.
- `record-pipeline-tool-use.sh` (PostToolUse on `openwiki_finish`): the completion
  receipt.
- The git pre-commit shim installed by `scripts/setup-pipeline-tools.sh` applies the
  same two receipt rules to any tool that commits. Codex runs none of the Claude Code
  hooks: a Codex session runs the gate by hand, relies on the shim, and records the
  OpenWiki receipt by handing the `complete` finish result to
  `record-pipeline-tool-use.sh` (`AGENTS.md`, "Tools and hosts"), as a Claude Code
  session does when its PostToolUse hook does not fire.

## The tools

- CodeGraph 1.5.0, project-local under `.dev/pipeline-tools/codegraph`, an MCP server
  (`codegraph`) and a CLI wrapper; `explore` and `impact` at design and inspect.
- OpenWiki 0.3.3, pinned to one commit, built from its frozen lockfile with a pnpm
  verified by SHA-512, patched twice (no scheduled refresh workflow, local-only guidance
  in the agent guides), recorded in a provenance file the check script verifies. It is
  used only through its MCP lifecycle (`openwiki_begin`, `openwiki_inspect_claims`,
  `openwiki_resolve_claims`, `openwiki_finish`); the host agent writes the pages, so
  nothing is sent to a provider. `scripts/check-pipeline.sh` verifies the wiring and that
  `CLAUDE.md` starts with `@AGENTS.md`, so the two hosts read one guide.

## The brain loop's hooks

Two Claude Code hooks ship inside `rusty-cli` (`crates/rusty-cli/hooks/`, embedded in the
binary). `rusty-cli hooks install` writes them to `~/.rusty/hooks/` and adds a PreToolUse
entry (Edit, Write, MultiEdit, NotebookEdit) and a Stop entry to `~/.claude/settings.json`,
idempotently and keeping every other entry; `hooks status` and `hooks uninstall` exist. In
a working directory whose `.mcp.json` names a rusty server, the write hook blocks a write
under that directory until the transcript holds a `mcp__rusty__brain_ask` tool use whose
result was not an error (a file elsewhere, a scratch script, passes), and the stop hook
refuses a stop once when files were written and no non-error `brain_decide` or
`brain_no_decision` follows; `stop_hook_active` passes the retry. Both fail open without
jq or a readable transcript. The corpus test in `crates/rusty-cli/src/hooks.rs` runs the
shipped scripts under bash. The record is `docs/architecture/brain-loop.md`.

## Invariants

- At most one active spec/notes pair; the `status:` line of the spec is the truth about
  where the work stands.
- Phases close on evidence in the notes, never on a claim; a test that did not run did
  not pass.
- Cargo commands run one at a time and are never killed.
- Tests and probes never touch the user's real data.

## Failure modes

- A host without the hooks (Codex) runs the same checks by hand and through the
  pre-commit shim.
- Creating `docs/planning/` in a contributor's clone turns the phase gate on with no
  pipeline to satisfy it, and every gated edit is then refused; a contributor never
  creates it.
- A commit moves `HEAD`, so both receipts read stale right after it; the next gated
  commit needs its own gate run.

## Extension points

- A new gated path: `rusty_gated_paths` in `bin/lib-gate.sh`.
- A new phase rule: `CONSTITUTION.md` (with an amendment log entry) and
  `references/phases.md`.

## Tests

- The guardrail hooks have no test in the repository; they are tried with synthetic hook
  input when they change, and `scripts/check-pipeline.sh` checks that they are present,
  executable and wired.
- `scripts/check-pipeline.sh` and `scripts/check-pipeline-tools.sh` are the structural
  checks; the first checks the tools always and the record when it is checked out.

## Primary sources

- `CONSTITUTION.md`, `AGENTS.md`, `.claude/skills/rusty-workflow/references/phases.md`,
  `.claude/skills/openwiki/SKILL.md`
- `bin/gate.sh`, `bin/lib-gate.sh`, `.claude/hooks/*.sh`, `.claude/settings.json`
- `scripts/setup-pipeline-tools.sh`, `scripts/lib-openwiki.sh`, `scripts/check-pipeline.sh`
