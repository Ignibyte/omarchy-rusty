#!/usr/bin/env bash
# PreToolUse on Edit, Write and MultiEdit: a gated path (rusty_gated_paths in
# bin/lib-gate.sh) may only change inside a pipeline that has reached Phase 3 (Implement)
# or later, or under a waiver. Every other path is always writable; the record's own files
# are how a pipeline advances. In a clone without the work record (docs/planning is the
# maintainers' private checkout) there is no pipeline to enforce, and the gate's receipt
# at commit is what applies.
set -Eeuo pipefail
HOOK_INPUT=$(cat)
command -v jq >/dev/null 2>&1 || exit 0
# shellcheck source=.claude/hooks/lib-hook-helpers.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib-hook-helpers.sh"
# shellcheck source=bin/lib-gate.sh
source "$(dirname "${BASH_SOURCE[0]}")/../../bin/lib-gate.sh"

root=$(hook_root) || exit 0
path=$(hook_field '.tool_input.file_path') || true
rel=$(hook_relpath "$root" "$path") || exit 0
rusty_is_gated "$rel" || exit 0
rusty_has_record || exit 0

[[ -f "$root/docs/planning/pipeline/WAIVER.md" ]] && exit 0
status=$(hook_active_status "$root")
case "$status" in
  *"Phase 3"* | *"Phase 3.5"* | *"Phase 4"* | *"Phase 5"*) exit 0 ;;
esac
if [[ -z "$status" ]]; then
  hook_block "PHASE GATE: $rel is a gated path and no pipeline is active. Open one with the rusty-workflow skill (plan, design, then implement), or write the reason for a small change to docs/planning/pipeline/WAIVER.md."
fi
hook_block "PHASE GATE: $rel is a gated path and the active spec is at '$status'. Finish plan and design first (status must reach Phase 3 — Implement), or write a waiver to docs/planning/pipeline/WAIVER.md."
