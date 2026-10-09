#!/usr/bin/env bash
# Are the workflow's tools in place (the hooks present, executable and wired; the MCP
# servers wired), and, where the work record is checked out at docs/planning, is it
# well-formed? At most one active spec/notes pair, each with its ticket and AAR; every
# completed pair archived with a submitted AAR. Exit 1 with the first problem. A clone
# without the record (the record is the maintainers' private checkout) checks the tools.
set -Eeuo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && git rev-parse --show-toplevel 2>/dev/null) || { echo "run inside the repo" >&2; exit 1; }
cd "$root"
fail() { echo "Pipeline check failed: $1" >&2; exit 1; }

for f in CONSTITUTION.md AGENTS.md CLAUDE.md .claude/skills/rusty-workflow/SKILL.md \
  .claude/skills/rusty-workflow/references/phases.md bin/gate.sh bin/lib-gate.sh \
  .claude/skills/openwiki/SKILL.md scripts/mcp-openwiki.sh scripts/lib-openwiki.sh openwiki/INSTRUCTIONS.md; do
  [[ -f "$f" ]] || fail "missing $f"
done
for h in enforce-phase-gate.sh enforce-secrets.sh enforce-commit-gate.sh record-pipeline-tool-use.sh; do
  [[ -x ".claude/hooks/$h" ]] || fail "hook missing or not executable: $h"
  grep -q "$h" .claude/settings.json || fail "hook not wired in .claude/settings.json: $h"
done
for server in codegraph openwiki; do
  grep -q "\"$server\"" .mcp.json || fail "MCP server not wired in .mcp.json: $server"
  grep -q "\[mcp_servers.$server\]" .codex/config.toml || fail "MCP server not wired in .codex/config.toml: $server"
done
# One guide: AGENTS.md, which Codex reads; CLAUDE.md imports it for Claude Code and adds
# only what Claude Code alone does.
head -1 CLAUDE.md | grep -qx '@AGENTS.md' || fail "CLAUDE.md must start with the line @AGENTS.md; AGENTS.md is the one guide"

if [[ ! -d docs/planning/pipeline ]]; then
  echo "Tools check passed; no work record at docs/planning (a contributor's clone), so the pipeline steps aside"
  exit 0
fi
for f in docs/planning/README.md docs/planning/tickets/INDEX.md docs/planning/knowledge/INDEX.md \
  docs/planning/bulletins/INDEX.md docs/planning/pipeline/_templates/spec.md \
  docs/planning/pipeline/_templates/notes.md docs/planning/_templates/ticket.md \
  docs/planning/_templates/intake.md; do
  [[ -f "$f" ]] || fail "missing $f"
done

active=0
for spec in docs/planning/pipeline/active/*.spec.md; do [[ -f "$spec" ]] && active=$((active + 1)); done
[[ $active -le 1 ]] || fail "more than one active spec"
for spec in docs/planning/pipeline/active/*.spec.md; do
  [[ -f "$spec" ]] || continue
  notes="${spec%.spec.md}.notes.md"
  [[ -f "$notes" ]] || fail "missing notes pair for $spec"
  ticket=$(sed -n 's/^ticket_doc:[[:space:]]*//p' "$spec" | head -1)
  [[ -n "$ticket" && -f "$ticket" ]] || fail "active spec ticket is missing: ${ticket:-unset}"
  aar=$(sed -n 's/^aar:[[:space:]]*//p' "$spec" | head -1)
  [[ -n "$aar" && -f "$aar" ]] || fail "active spec AAR is missing: ${aar:-unset}"
done
for spec in docs/planning/pipeline/completed/*.spec.md; do
  [[ -f "$spec" ]] || continue
  notes="${spec%.spec.md}.notes.md"
  [[ -f "$notes" ]] || fail "missing completed notes pair for $spec"
  grep -q '^status:.*Phase 5 — Complete PASS' "$spec" || fail "completed spec status is not Phase 5 PASS: $spec"
  aar=$(sed -n 's/^aar:[[:space:]]*//p' "$spec" | head -1)
  [[ -n "$aar" && -f "$aar" ]] || fail "completed spec AAR is missing: ${aar:-unset}"
  grep -qi '^submitted:[[:space:]]*20' "$aar" || fail "completed spec AAR is not submitted: $aar"
done
if [[ -f docs/planning/pipeline/WAIVER.md ]]; then
  echo "note: a waiver is in force (docs/planning/pipeline/WAIVER.md)"
fi
echo "Pipeline structure check passed"
