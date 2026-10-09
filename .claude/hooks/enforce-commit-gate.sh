#!/usr/bin/env bash
# PreToolUse on Bash: a `git commit` made while gated files differ from HEAD (staged, edited
# or new; the command may stage them itself) needs a receipt that matches the worktree
# (bin/gate.sh --diff); one that delivers a completed pipeline (the record at docs/planning
# holds a completed spec it has not committed yet) needs a matching OpenWiki completion
# receipt too, unless a waiver is in force. Docs-only commits pass, and so does a commit
# inside the record's own checkout. `--no-verify` and `-n` are refused. Without jq the
# command cannot be read, so the commit is refused rather than waved through.
set -Eeuo pipefail
HOOK_INPUT=$(cat)
if ! command -v jq >/dev/null 2>&1; then
  echo "COMMIT GATE: jq is needed to check a commit; install jq (pacman -S jq)." >&2
  exit 2
fi
# shellcheck source=.claude/hooks/lib-hook-helpers.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib-hook-helpers.sh"
# shellcheck source=bin/lib-gate.sh
source "$(dirname "${BASH_SOURCE[0]}")/../../bin/lib-gate.sh"

cmd=$(hook_field '.tool_input.command') || true
[[ -n "$cmd" ]] || exit 0
# `git commit`, with any `-C dir`, `-c key=value` or `--option` before the subcommand.
grep -qE '(^|[[:space:];&|(])git(([[:space:]]+-[Cc][[:space:]]+[^[:space:]]+)|([[:space:]]+--[^[:space:]]+))*[[:space:]]+commit([[:space:]]|$)' <<<"$cmd" || exit 0
grep -qE -- '--no-verify' <<<"$cmd" && hook_block "COMMIT GATE: --no-verify is not allowed (CONSTITUTION §15)."
# `-n` is --no-verify too, alone or in a cluster such as `-nam`; only the commit's own
# arguments are read, up to the next `;`, `&` or `|`.
after=${cmd#*commit}
after=${after%%[;&|]*}
grep -qE '(^|[[:space:]])-[a-zA-Z]*n[a-zA-Z]*([[:space:]]|$)' <<<"$after" && hook_block "COMMIT GATE: -n skips the hooks like --no-verify and is not allowed (CONSTITUTION §15)."

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
# What the commit may carry: the command can stage files itself (`git add … && git
# commit`, `-a`, a pathspec), so every gated change counts, staged or not.
gated=$(rusty_gated_changes)
carried=$(git -C "$root" status --porcelain 2>/dev/null | head -1)
completed=""
if [[ -n "$carried" ]]; then completed=$(rusty_undelivered_completed); fi
if [[ -n "$gated" ]] && ! msg=$(rusty_commit_matches_worktree); then
  hook_block "COMMIT GATE: $msg"
fi

if [[ -n "$gated" ]] && ! msg=$(rusty_verify_receipt); then
  hook_block "COMMIT GATE: this commit carries gated files ($(printf '%s' "$gated" | head -3 | tr '\n' ' ')…) and $msg"
fi
if [[ -n "$completed" && ! -f "$root/docs/planning/pipeline/WAIVER.md" ]] && ! msg=$(rusty_verify_openwiki_receipt); then
  hook_block "COMPLETION GATE: this commit delivers a completed pipeline ($(printf '%s' "$completed" | head -1)) and $msg (CONSTITUTION §15)"
fi
exit 0
