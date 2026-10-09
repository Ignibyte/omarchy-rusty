#!/usr/bin/env bash
# The quality gate. --fast: fmt, clippy, tests. --diff (default) and --full: those plus the
# doc build, shell syntax, a secrets scan and a whitespace check; green writes the receipt
# .git/rusty-gate-receipt bound to the worktree. --verify checks the receipt (and reports
# the OpenWiki completion receipt) and exits 0 or 1. Cargo steps run strictly one after
# another.
set -Eeuo pipefail

mode=diff
case "${1:-}" in
  "" | --diff) mode=diff ;;
  --fast) mode=fast ;;
  --full) mode=full ;;
  --verify)
    # shellcheck source=bin/lib-gate.sh
    source "$(dirname "${BASH_SOURCE[0]}")/lib-gate.sh"
    if wiki=$(rusty_verify_openwiki_receipt); then echo "OPENWIKI OK: $wiki"; else echo "OPENWIKI RECEIPT MISSING: $wiki"; fi
    if msg=$(rusty_verify_receipt); then echo "RECEIPT OK: $msg"; exit 0; else echo "RECEIPT MISSING: $msg" >&2; exit 1; fi
    ;;
  *) echo "Usage: $0 [--fast|--diff|--full|--verify]" >&2; exit 2 ;;
esac

# The repository this script belongs to, also when it runs from inside the record's checkout.
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && git rev-parse --show-toplevel 2>/dev/null) || { echo "run inside the repo" >&2; exit 2; }
cd "$root"
# shellcheck source=bin/lib-gate.sh
source "$root/bin/lib-gate.sh"

step() {
  local name=$1; shift
  echo "== $name"
  if "$@"; then
    echo "-- $name ok"
  else
    echo "GATE RED [$mode] at $name" >&2
    exit 1
  fi
}

check_shell_syntax() {
  local f rc=0
  while IFS= read -r f; do
    bash -n "$f" || rc=1
  done < <(git ls-files -co --exclude-standard -- 'bin/*.sh' 'scripts/*.sh' 'omarchy/*.sh' '.claude/hooks/*.sh' | grep -v '^$')
  return $rc
}

# Things that look like credentials in gated files. Fixtures assemble their samples at
# runtime rather than storing a matching literal.
check_secrets() {
  local hits
  hits=$(rusty_gated_files | while IFS= read -r f; do
    [[ -f "$f" ]] || continue
    grep -nE '(sk-ant-[A-Za-z0-9_-]{8,}|sk-proj-[A-Za-z0-9_-]{8,}|ghp_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16}|xox[bp]-[0-9A-Za-z-]{10,}|-----BEGIN [A-Z ]*PRIVATE KEY-----)' "$f" 2>/dev/null | sed "s|^|$f:|" || true
  done)
  if [[ -n "$hits" ]]; then
    echo "$hits" >&2
    return 1
  fi
  echo "   $(rusty_gated_files | wc -l) gated files scanned"
}

check_whitespace() {
  local hits
  hits=$(rusty_gated_files | grep -E '\.(rs|toml|sh|md|json|qml|yml|yaml)$' | while IFS= read -r f; do
    [[ -f "$f" ]] || continue
    grep -nE '[[:space:]]+$' "$f" 2>/dev/null | sed "s|^|$f:|" | head -3 || true
  done)
  if [[ -n "$hits" ]]; then
    echo "trailing whitespace:" >&2
    echo "$hits" >&2
    return 1
  fi
}

step fmt cargo fmt --all --check
step clippy cargo clippy --workspace --all-targets -- -D warnings
step test cargo test --workspace

if [[ "$mode" != "fast" ]]; then
  step doc env RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
  step shell-syntax check_shell_syntax
  step secrets check_secrets
  step whitespace check_whitespace
  receipt=$(rusty_receipt_path)
  printf 'version=1\nfingerprint=%s\nmode=%s\nat=%s\n' "$(rusty_fingerprint)" "$mode" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >"$receipt"
  echo "receipt written: $receipt"
fi

echo "GATE GREEN [$mode]"
