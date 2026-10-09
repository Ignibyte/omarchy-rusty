#!/usr/bin/env bash
# PreToolUse on Bash: a `git commit` that includes gated files needs a receipt that matches
# the worktree (bin/gate.sh --diff); one that delivers a completed pipeline (the record at
# docs/planning holds a completed spec it has not committed yet) needs a matching OpenWiki
# completion receipt too, unless a waiver is in force. Docs-only commits pass, and so does
# a commit inside the record's own checkout. `--no-verify` is refused.
set -Eeuo pipefail
HOOK_INPUT=$(cat)
command -v jq >/dev/null 2>&1 || exit 0
# shellcheck source=.claude/hooks/lib-hook-helpers.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib-hook-helpers.sh"
# shellcheck source=bin/lib-gate.sh
source "$(dirname "${BASH_SOURCE[0]}")/../../bin/lib-gate.sh"

cmd=$(hook_field '.tool_input.command') || true
[[ -n "$cmd" ]] || exit 0
grep -qE '(^|[[:space:];&|])git([[:space:]]+-C[[:space:]]+[^[:space:]]+)?[[:space:]]+commit([[:space:]]|$)' <<<"$cmd" || exit 0
grep -qE -- '--no-verify' <<<"$cmd" && hook_block "COMMIT GATE: --no-verify is not allowed (CONSTITUTION §15)."

root=$(hook_root) || exit 0
# A commit inside the record (`git -C docs/planning commit`, or run from there) is the
# record's own and carries nothing gated.
record=$(rusty_record_dir) || record=""
cwd=$(hook_field '.cwd') || true
target=${cwd:-$root}
if [[ "$cmd" =~ git[[:space:]]+-C[[:space:]]+([^[:space:]]+)[[:space:]]+commit ]]; then
  c=${BASH_REMATCH[1]}
  if [[ "$c" == /* ]]; then target=$c; else target="$target/$c"; fi
fi
target=$(realpath -m "$target" 2>/dev/null) || target=""
if [[ -n "$record" && -n "$target" && ( "$target" == "$record" || "$target" == "$record"/* ) ]]; then
  exit 0
fi
# Which files would this commit carry: staged now, or (with -a) modified tracked files.
staged=$(git -C "$root" diff --cached --name-only 2>/dev/null; if grep -qE '(^|[[:space:]])-(a|am|-all)([[:space:]]|$)' <<<"$cmd"; then git -C "$root" diff --name-only 2>/dev/null; fi)
gated=""
carried=""
while IFS= read -r f; do
  [[ -n "$f" ]] || continue
  carried=1
  if rusty_is_gated "$f"; then gated+="$f"$'\n'; fi
done <<<"$staged"
completed=""
if [[ -n "$carried" ]]; then completed=$(rusty_undelivered_completed); fi

if [[ -n "$gated" ]] && ! msg=$(rusty_verify_receipt); then
  hook_block "COMMIT GATE: this commit carries gated files ($(printf '%s' "$gated" | head -3 | tr '\n' ' ')…) and $msg"
fi
if [[ -n "$completed" && ! -f "$root/docs/planning/pipeline/WAIVER.md" ]] && ! msg=$(rusty_verify_openwiki_receipt); then
  hook_block "COMPLETION GATE: this commit delivers a completed pipeline ($(printf '%s' "$completed" | head -1)) and $msg (CONSTITUTION §15)"
fi
exit 0
