#!/usr/bin/env bash
# PostToolUse on the OpenWiki finish tool: a run that came back `complete` writes the
# completion receipt `.git/rusty-openwiki-receipt`, bound to the worktree's gated content
# the way the gate receipt is. The commit gate and the pre-commit shim require it when a
# completed pipeline is delivered (CONSTITUTION §15). Never written by hand.
set -Eeuo pipefail
HOOK_INPUT=$(cat)
command -v jq >/dev/null 2>&1 || exit 0
# shellcheck source=.claude/hooks/lib-hook-helpers.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib-hook-helpers.sh"
# shellcheck source=bin/lib-gate.sh
source "$(dirname "${BASH_SOURCE[0]}")/../../bin/lib-gate.sh"

tool=$(hook_field '.tool_name') || true
[[ "$tool" == "mcp__openwiki__openwiki_finish" ]] || exit 0

# Complete, as the structured content says or as the text content spells it.
jq -e '
  (.tool_response.structuredContent.status == "complete")
  or ([.tool_response.content[]?.text? // ""] | join(" ") | test("\"status\"[[:space:]]*:[[:space:]]*\"complete\""))
  or ((.tool_response | tostring) | test("\"status\"[[:space:]]*:[[:space:]]*\"complete\""))
' <<<"$HOOK_INPUT" >/dev/null 2>&1 || exit 0

root=$(hook_root) || exit 0
pipeline=$(ls "$root"/docs/planning/pipeline/active/*.spec.md 2>/dev/null | head -1)
pipeline_id="none"
if [[ -n "$pipeline" ]]; then
  pipeline_id=$(sed -n 's/^pipeline_id:[[:space:]]*//p' "$pipeline" | head -1)
fi
receipt=$(rusty_openwiki_receipt_path) || exit 0
{
  printf 'version=1\n'
  printf 'fingerprint=%s\n' "$(rusty_fingerprint)"
  printf 'pipeline=%s\n' "${pipeline_id:-none}"
  printf 'at=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} >"$receipt"
echo "OpenWiki completion receipt written: $receipt"
