#!/usr/bin/env bash
# PreToolUse on Edit, Write and MultiEdit: refuse content that looks like a credential. The
# patterns are the same as the gate's secrets step, applied before the bytes land.
set -Eeuo pipefail
HOOK_INPUT=$(cat)
command -v jq >/dev/null 2>&1 || exit 0
# shellcheck source=.claude/hooks/lib-hook-helpers.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib-hook-helpers.sh"

content=$(jq -r '[.tool_input.content, .tool_input.new_string, (.tool_input.edits[]?.new_string)] | map(select(. != null)) | join("\n")' <<<"$HOOK_INPUT" 2>/dev/null || true)
[[ -n "$content" ]] || exit 0
if grep -qE '(sk-ant-[A-Za-z0-9_-]{8,}|sk-proj-[A-Za-z0-9_-]{8,}|ghp_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16}|xox[bp]-[0-9A-Za-z-]{10,}|-----BEGIN [A-Z ]*PRIVATE KEY-----)' <<<"$content"; then
  hook_block "SECRETS: that write contains something that looks like a credential. Keep credentials out of the repository: Rusty keeps them in its secrets file (secret_set), and test fixtures assemble their samples at runtime."
fi
exit 0
