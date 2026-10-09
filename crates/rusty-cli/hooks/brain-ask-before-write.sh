#!/usr/bin/env bash
# brain-ask-before-write.sh: consult the brain before the first write (PreToolUse on
# Edit, Write, MultiEdit and NotebookEdit). Part of Rusty's loop, Ask, Decide, Follow up
# (TICKET-018). Scoped to sessions whose working directory carries a .mcp.json with a
# rusty server, and to files under that directory: a scratch script elsewhere is not a
# change to the repository. Monotonic: one brain_ask call whose result was not an error
# lets every later write through. Fails open without jq, without a transcript, or on a
# transcript that cannot be read: a hook never blocks on its own plumbing. Exit 0 allows;
# exit 2 blocks and names the tool on stderr. Installed by `rusty-cli hooks install`.
set -u
INPUT=$(cat)
command -v jq >/dev/null 2>&1 || exit 0
cwd=$(printf '%s' "$INPUT" | jq -r '.cwd // empty' 2>/dev/null) || exit 0
[ -n "$cwd" ] || cwd=$PWD
[ -f "$cwd/.mcp.json" ] || exit 0
grep -q '"rusty"' "$cwd/.mcp.json" 2>/dev/null || exit 0
file=$(printf '%s' "$INPUT" | jq -r '.tool_input.file_path // empty' 2>/dev/null) || exit 0
case "$file" in
  "") ;;
  /*) case "$file" in "$cwd"/*) ;; *) exit 0 ;; esac ;;
esac
transcript=$(printf '%s' "$INPUT" | jq -r '.transcript_path // empty' 2>/dev/null) || exit 0
[ -n "$transcript" ] && [ -r "$transcript" ] || exit 0

asks=$(jq -r 'select(.type == "assistant") | .message.content[]? | select(.type == "tool_use" and .name == "mcp__rusty__brain_ask") | .id' "$transcript" 2>/dev/null) || exit 0
errors=$(jq -r 'select(.type == "user") | .message.content[]? | select(.type == "tool_result" and .is_error == true) | .tool_use_id' "$transcript" 2>/dev/null) || exit 0
for id in $asks; do
  grep -qx -- "$id" <<<"$errors" || exit 0
done

{
  echo ""
  echo "BRAIN-ASK BLOCKED: consult the brain before the first write in this session."
  echo "Call brain_ask (the rusty MCP server) with the question you are about to decide;"
  echo "it returns the pages, the decisions and the follow-ups that touch it, and a"
  echo "consultation id for brain_decide. Then write."
  [ -n "$file" ] && echo "File: $file"
} >&2
exit 2
