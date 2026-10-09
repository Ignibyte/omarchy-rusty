#!/usr/bin/env bash
# brain-decide-before-stop.sh: record the decision before the session ends (Stop).
# Part of Rusty's loop, Ask, Decide, Follow up (TICKET-018). Scoped to sessions whose
# working directory carries a .mcp.json with a rusty server. When the transcript shows a
# file write (Edit, Write, MultiEdit, NotebookEdit) and no brain_decide or
# brain_no_decision call whose result was not an error, the stop is refused once with the
# missing record named; stop_hook_active marks the second attempt, which passes, so the
# honest way out is always open. Fails open without jq or a readable transcript. Exit 0
# allows; exit 2 refuses. Installed by `rusty-cli hooks install`.
set -u
INPUT=$(cat)
command -v jq >/dev/null 2>&1 || exit 0
active=$(printf '%s' "$INPUT" | jq -r '.stop_hook_active // false' 2>/dev/null) || exit 0
[ "$active" = "true" ] && exit 0
cwd=$(printf '%s' "$INPUT" | jq -r '.cwd // empty' 2>/dev/null) || exit 0
[ -n "$cwd" ] || cwd=$PWD
[ -f "$cwd/.mcp.json" ] || exit 0
grep -q '"rusty"' "$cwd/.mcp.json" 2>/dev/null || exit 0
transcript=$(printf '%s' "$INPUT" | jq -r '.transcript_path // empty' 2>/dev/null) || exit 0
[ -n "$transcript" ] && [ -r "$transcript" ] || exit 0

writes=$(jq -r 'select(.type == "assistant") | .message.content[]? | select(.type == "tool_use" and (.name == "Write" or .name == "Edit" or .name == "MultiEdit" or .name == "NotebookEdit")) | .id' "$transcript" 2>/dev/null) || exit 0
[ -n "$writes" ] || exit 0
records=$(jq -r 'select(.type == "assistant") | .message.content[]? | select(.type == "tool_use" and (.name == "mcp__rusty__brain_decide" or .name == "mcp__rusty__brain_no_decision")) | .id' "$transcript" 2>/dev/null) || exit 0
errors=$(jq -r 'select(.type == "user") | .message.content[]? | select(.type == "tool_result" and .is_error == true) | .tool_use_id' "$transcript" 2>/dev/null) || exit 0
for id in $records; do
  grep -qx -- "$id" <<<"$errors" || exit 0
done

{
  echo ""
  echo "BRAIN-DECIDE REFUSED ONCE: this session wrote files and recorded no decision."
  echo "Call brain_decide with the consultation id from brain_ask (the choice, the"
  echo "rationale, the alternatives, a follow-up date when the outcome is worth checking),"
  echo "or brain_no_decision with the reason nothing was decided. Then stop again."
} >&2
exit 2
