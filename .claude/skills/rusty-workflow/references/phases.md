# Rusty workflow phases

## Recall and preflight

Before opening or resuming work:

- Read `docs/planning/bulletins/INDEX.md`; a critical bulletin blocks work.
- Search `docs/planning/knowledge/INDEX.md`, the nearest completed pipeline notes,
  `openwiki/quickstart.md` and the wiki pages the work touches, and the relevant
  `docs/architecture/` documents.
- Search the maintainer's brain (`brain_search` on the `rusty` MCP server, and `brain_ask`
  before a decision) for the project's pages and lessons.
- Inspect the affected code, callers and tests. CodeGraph for Rust; shell scripts by hand.
- Run `scripts/check-pipeline.sh` and `scripts/check-pipeline-tools.sh`.

Record what recall turned up in the notes and the AAR's recall log.

## Phase 1: Plan

No application code. Keep the request to one shippable slice.

1. Take the next number from `docs/planning/tickets/INDEX.md` and increment it.
2. Create the ticket from `docs/planning/_templates/ticket.md` under `tickets/open/` and
   add its row to the open queue.
3. Create a UUID and the spec/notes pair from `docs/planning/pipeline/_templates/` under
   `pipeline/active/`.
4. Write scope in and out, locked decisions, and EARS requirements with an explicit
   verification method each.
5. Open the AAR under `docs/planning/knowledge/aar/` and seed its recall log.

Set the spec status to `Phase 1 — Plan PASS; ready for Phase 2 — Design`. A spec that
changes what Rusty is (a new kind of data, a new tool family, a new dependency) is sealed
by the maintainer before Phase 3: their words and the date go on the spec's `sealed:` line.

## Phase 2: Design

No application code. Re-read the spec, the ticket, the architecture doc and the actual
producers and consumers of what changes. Add to the notes:

1. Architecture and data flow (which crate owns what; the tools, resources and
   notifications that change).
2. The exact file manifest, one purpose per file.
3. Store consequences: schema changes in `engine/db.rs` are additive and migrated in
   `migrate()`; the vault's file format stays readable by any markdown tool.
4. Tool contract and compatibility (a renamed or removed tool is a versioned break).
5. A regression table mapping every requirement to its evidence.
6. Risks: data safety, concurrency on the single SQLite connection and between copies of
   the server (the service and agents' stdio servers share one store), what a client sees
   when a call fails.
7. Decisions made and the alternatives set aside.

Run `codegraph_explore` over the relevant symbols after the plan is stable (or
`scripts/codegraph.sh explore …`) and record the topology and blast radius in the notes.

Set the status to `Phase 2 — Design PASS; ready for Phase 3 — Implement`.

## Phase 3: Implement

Build the manifest, inside the confirmed scope.

- Managers in `rusty-core` own the logic; tools in `rusty-mcp` are thin and emit
  `DataChanged` through `mutate()` on every write; the CLI calls the same managers.
- A tool's description is documentation agents read: say what it does, what it returns
  and what it refuses. `docs/tools.md` is generated from them (its test says how).
- Run `cargo fmt --all` and focused checks as you go; `bin/gate.sh --fast` before
  claiming the phase.
- Record deviations from the manifest and why.

Set the status to `Phase 3 — Implement PASS; ready for Phase 3.5 — Inspect`.

## Phase 3.5: Inspect

Review the whole diff adversarially through the lenses that apply:

- correctness and EARS coverage;
- data safety (never touches real data; migrations additive; no data loss on error);
- the single-connection SQLite lock (no guard held across a call that takes it again);
- secrets, paths, and anything that leaves the machine;
- the tool contract: names, parameters, results and errors a client already relies on;
- unnecessary complexity and missed reuse;
- prose (docs, strings, commit message) against `no-ai-slop`.

Verify every finding, reject false positives with a reason, fix confirmed defects, and
write the finding and disposition ledger in the notes. Then run a fresh
`codegraph_explore` over the changed symbols and their one-hop dependents and reconcile
its blast radius with the ledger.

Set the status to `Phase 3.5 — Inspect PASS; ready for Phase 4 — Validate` only when
confirmed findings are resolved.

## Phase 4: Validate

1. Implement every remaining test from the regression table.
2. Run the tests and record the commands and their real output.
3. Run `bin/gate.sh --diff` after the last gated edit. Fix red at the source and rerun
   until it prints `GATE GREEN [diff]` and writes the receipt.
4. For a change a client will see, probe a scratch server (the smoke test's pattern) or
   the running service with throwaway rows, and record how.

Set the status to `Phase 4 — Validate PASS; ready for Phase 5 — Complete`.

## Phase 5: Complete

1. Audit every requirement: satisfied with named evidence, split to a follow-up ticket,
   or waived with a reason.
2. Update `docs/architecture.md`, the references in `docs/`, `README.md`, `CHANGELOG.md`
   (under the unreleased version), `ROADMAP.md` (move what landed under the release) and
   any operator docs the change touched.
3. Run the `openwiki` skill: `init` when `openwiki/quickstart.md` is absent, `update`
   otherwise. Reconcile the pages the change touched through claims with source
   evidence, then call `openwiki_finish` until it returns `complete`; the hook writes
   `.git/rusty-openwiki-receipt` (a host without it feeds the script, `AGENTS.md`).
   Delivery of the completed pair needs that receipt.
4. Write the AAR (outcomes, what went well, what went poorly, surprises, lessons, time).
   Put every new `PR-`, `BF-` and `AD-` ID in the AAR and in
   `docs/planning/knowledge/INDEX.md`.
5. Capture the durable lesson into the brain (`brain_add_timeline` on the project page,
   or `store_memory` for a rule), so any session recalls it.
6. If completion edits or the wiki run touched a gated path (`AGENTS.md` and
   `CLAUDE.md` carry OpenWiki's managed section), rerun `bin/gate.sh --diff`.
7. Move the ticket from `open/` to `closed/`, mark it done, update the index. Move the
   spec/notes pair from `active/` to `completed/`.

Set the status to `Phase 5 — Complete PASS`.

## Delivery

1. `bin/gate.sh --verify` must be green on the final worktree, and report the OpenWiki
   receipt as matching when a completed pair is being delivered.
2. Stage the intended files; read the staged diff for secrets, generated state and
   unrelated changes.
3. Commit with a clear subject, a body that says why, the ticket id, and the trailer.
4. Push `main` when delivery is authorized (a standing authorization is a bulletin in the
   record) and confirm the CI run.

Report the gate result, the commit SHA, and the push and CI status.
