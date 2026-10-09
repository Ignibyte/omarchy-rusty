#!/usr/bin/env bash
# Shared helpers for the Claude Code hooks. Hooks read one JSON object on stdin
# (tool_name, tool_input, ...) and block by exiting 2 with the reason on stderr.

hook_root() { git -C "${CLAUDE_PROJECT_DIR:-$PWD}" rev-parse --show-toplevel 2>/dev/null; }

# The value of a field in the hook input, or empty.
hook_field() { jq -r "$1 // empty" <<<"$HOOK_INPUT" 2>/dev/null; }

# A repo-relative, canonical path for a tool's file argument.
hook_relpath() {
  local root=$1 path=$2 abs
  [[ -n "$path" ]] || return 1
  if [[ "$path" != /* ]]; then path="$root/$path"; fi
  abs=$(realpath -m "$path" 2>/dev/null) || return 1
  case "$abs" in
    "$root"/*) printf '%s\n' "${abs#"$root"/}" ;;
    *) return 1 ;;
  esac
}

# The active spec's status line, or empty when no pipeline is active.
hook_active_status() {
  local root=$1 spec
  spec=$(ls "$root"/docs/planning/pipeline/active/*.spec.md 2>/dev/null | head -1)
  [[ -n "$spec" ]] || return 0
  sed -n 's/^status:[[:space:]]*//p' "$spec" | head -1
}

hook_block() { echo "$1" >&2; exit 2; }
