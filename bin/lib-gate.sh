#!/usr/bin/env bash
# Shared pieces of the gate: which paths are gated, the worktree fingerprint, the receipt.
# Sourced by bin/gate.sh and the hooks; safe to source more than once.

# Paths whose content the receipt binds. docs/ and the roadmap are exempt on purpose: a
# pipeline writes notes while a gate run is in flight, and docs-only changes stay
# committable without a receipt.
rusty_gated_paths=(
  crates Cargo.toml Cargo.lock
  bin scripts omarchy packaging
  .claude .codex .mcp.json .github
  CONSTITUTION.md AGENTS.md CLAUDE.md
)

# The repository this library belongs to (it lives at <root>/bin/lib-gate.sh), whatever the
# current directory: inside docs/planning, git would answer for the record's own checkout.
rusty_root() { (cd "$(dirname "${BASH_SOURCE[0]}")/.." 2>/dev/null && git rev-parse --show-toplevel 2>/dev/null); }

# The work record (tickets, pipelines, AARs, the knowledge register) is a private checkout
# of its own at docs/planning, ignored by this repository. A contributor's clone has none,
# and the pipeline's checks step aside; the gate and its receipt still apply.
rusty_record_dir() {
  local root
  root=$(rusty_root) || return 1
  printf '%s/docs/planning\n' "$root"
}

rusty_has_record() {
  local dir
  dir=$(rusty_record_dir) || return 1
  [[ -d "$dir/pipeline" ]]
}

# Completed specs the record has not committed yet: a pipeline being delivered. The public
# commit goes first and the record's own commit follows, so these are what a public commit
# delivers. Prints one record-relative path per line.
rusty_undelivered_completed() {
  local dir
  dir=$(rusty_record_dir) || return 0
  [[ -e "$dir/.git" ]] || return 0
  git -C "$dir" status --porcelain --untracked-files=all -- 'pipeline/completed/*.spec.md' 2>/dev/null |
    cut -c4- | grep -v '^$' || true
}

rusty_receipt_path() {
  local git_dir
  git_dir=$(git -C "$(rusty_root)" rev-parse --absolute-git-dir 2>/dev/null) || return 1
  printf '%s/rusty-gate-receipt\n' "$git_dir"
}

# The OpenWiki completion receipt: written by the PostToolUse hook when a lifecycle run
# finishes `complete`, bound to the same fingerprint as the gate receipt.
rusty_openwiki_receipt_path() {
  local git_dir
  git_dir=$(git -C "$(rusty_root)" rev-parse --absolute-git-dir 2>/dev/null) || return 1
  printf '%s/rusty-openwiki-receipt\n' "$git_dir"
}

# Every gated file: tracked, plus untracked files that are not ignored.
rusty_gated_files() {
  local root
  root=$(rusty_root) || return 1
  (cd "$root" && git ls-files -co --exclude-standard -z -- "${rusty_gated_paths[@]}" 2>/dev/null | tr '\0' '\n' | grep -v '^$' | sort -u)
}

# sha256 over HEAD and every gated file's path and content. An empty file set yields a
# per-process value that can never match, so a broken checkout fails closed.
rusty_fingerprint() {
  local root files
  root=$(rusty_root) || return 1
  files=$(rusty_gated_files) || return 1
  if [[ -z "$files" ]]; then
    printf 'empty-%s-%s\n' "$$" "$(date +%s%N)"
    return 0
  fi
  {
    git -C "$root" rev-parse HEAD 2>/dev/null || echo "no-head"
    while IFS= read -r f; do
      [[ -f "$root/$f" ]] || continue
      printf '%s\0' "$f"
      sha256sum "$root/$f" | cut -c1-64
    done <<<"$files"
  } | sha256sum | cut -c1-64
}

# Is a path (relative to the root) gated?
rusty_is_gated() {
  local path=$1 p
  for p in "${rusty_gated_paths[@]}"; do
    if [[ "$path" == "$p" || "$path" == "$p"/* ]]; then
      return 0
    fi
  done
  return 1
}

# Does the receipt exist, parse, and match this worktree? Prints why not.
rusty_verify_receipt() {
  local receipt fp mode at
  receipt=$(rusty_receipt_path) || { echo "not inside a git worktree"; return 1; }
  [[ -f "$receipt" ]] || { echo "no receipt at $receipt; run bin/gate.sh --diff"; return 1; }
  fp=$(sed -n 's/^fingerprint=//p' "$receipt")
  mode=$(sed -n 's/^mode=//p' "$receipt")
  at=$(sed -n 's/^at=//p' "$receipt")
  [[ "$(sed -n 's/^version=//p' "$receipt")" == "1" && -n "$fp" && -n "$mode" ]] || { echo "receipt does not parse; run bin/gate.sh --diff"; return 1; }
  [[ "$mode" == "diff" || "$mode" == "full" ]] || { echo "receipt is from a --fast run; run bin/gate.sh --diff"; return 1; }
  if [[ "$fp" != "$(rusty_fingerprint)" ]]; then
    echo "worktree changed since the gate ran at $at; run bin/gate.sh --diff"
    return 1
  fi
  echo "receipt matches the worktree ($mode, $at)"
}

# Does the OpenWiki completion receipt exist and match this worktree? Prints why not.
rusty_verify_openwiki_receipt() {
  local receipt fp at pipeline
  receipt=$(rusty_openwiki_receipt_path) || { echo "not inside a git worktree"; return 1; }
  [[ -f "$receipt" ]] || { echo "no OpenWiki completion receipt at $receipt; run the openwiki skill (Phase 5)"; return 1; }
  fp=$(sed -n 's/^fingerprint=//p' "$receipt")
  at=$(sed -n 's/^at=//p' "$receipt")
  pipeline=$(sed -n 's/^pipeline=//p' "$receipt")
  [[ "$(sed -n 's/^version=//p' "$receipt")" == "1" && -n "$fp" ]] || { echo "OpenWiki receipt does not parse; run the openwiki skill again"; return 1; }
  if [[ "$fp" != "$(rusty_fingerprint)" ]]; then
    echo "worktree changed since the OpenWiki run finished at $at; run the openwiki skill again"
    return 1
  fi
  echo "OpenWiki receipt matches the worktree (pipeline $pipeline, $at)"
}
