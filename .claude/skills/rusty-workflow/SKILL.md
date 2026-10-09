---
name: rusty-workflow
description: Run or resume the evidence-based Rusty work pipeline for any non-trivial feature, fix, migration, packaging or workflow change in the Ignibyte/omarchy-rusty repo. Covers recall, the ticket and the spec/notes pair, EARS requirements, the design file manifest with CodeGraph evidence, the adversarial inspect ledger, the gate receipt, the AAR and the knowledge register. Use it before touching files; read references/phases.md completely.
---

# Rusty workflow

Read `AGENTS.md` and `CONSTITUTION.md`, then [references/phases.md](references/phases.md)
completely before changing files.

## Route the request

0. No `docs/planning/` in this clone: there is no pipeline to run. Follow `AGENTS.md`'s
   "Without `docs/planning`" section (the change with its tests, `bin/gate.sh --diff`, a
   pull request) and stop reading here. Never create `docs/planning/`.
1. Look in `docs/planning/pipeline/active/`. If a spec/notes pair is there, resume it from
   its recorded `status:`; never open a second pipeline.
2. If the request is an idea rather than approved work, write an intake
   (`docs/planning/_templates/intake.md`) and stop; do not write application code.
3. If the maintainer waives ceremony for a small change, write the reason to
   `docs/planning/pipeline/WAIVER.md`, do the change with the ordinary loop, run
   `bin/gate.sh --diff`, report the waiver, delete the file.
4. Otherwise run `scripts/check-pipeline.sh`, start Phase 1, and continue through the
   phases the requested outcome needs.

Before Phase 2, run `scripts/check-pipeline-tools.sh`; if CodeGraph or OpenWiki is not
prepared, run `scripts/setup-pipeline-tools.sh`. A newly wired MCP server needs a
session restart; until then `scripts/codegraph.sh explore …` is the permitted fallback
for CodeGraph. Phase 5 runs the `openwiki` skill (`.claude/skills/openwiki/SKILL.md`).

## Rules

- The `status:` line of the active spec is the truth about where the work stands. Do
  not infer a phase from chat history.
- Never claim a gate, test or tool ran unless it did, and paste what it printed.
- Never commit gated files without a matching receipt (`bin/gate.sh --verify`), never
  `--no-verify`, never touch `.git/rusty-gate-receipt` or `.git/rusty-openwiki-receipt` by
  hand. A completed pipeline is delivered only with a matching OpenWiki completion receipt.
- Tests and probes never touch the user's real data: the smoke test uses a scratch `HOME`;
  probes against the running service use throwaway rows they create and delete by id.
- Pause only for a missing decision that would change scope, or when the user asked for
  phase-by-phase review. Otherwise drive to the requested outcome.

At handoff report: the phase reached, changed surfaces, tests and gate results with
their real output, unresolved requirements, and delivery status (commit SHA, pushed or
not).
