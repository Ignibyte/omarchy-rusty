#!/usr/bin/env bash
# Install the pipeline tools project-local and pinned, under .dev/ (ignored): CodeGraph
# (semantic code graph; MCP server + CLI), OpenWiki (the generated engineering wiki's
# lifecycle, as an MCP server the host agent drives) and the git pre-commit shim that
# verifies the receipts. Idempotent. Needs node 22 or newer and npm on PATH.
set -Eeuo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && git rev-parse --show-toplevel 2>/dev/null) || { echo "run inside the repo" >&2; exit 1; }
cd "$root"
# shellcheck source=scripts/lib-openwiki.sh
source "$root/scripts/lib-openwiki.sh"

codegraph_version="1.5.0"
prefix="$root/.dev/pipeline-tools/codegraph"

command -v npm >/dev/null 2>&1 || { echo "npm is required (node 22 or newer)" >&2; exit 1; }
command -v node >/dev/null 2>&1 || { echo "node is required (22 or newer)" >&2; exit 1; }

echo "==> CodeGraph $codegraph_version (project-local, telemetry off)"
mkdir -p "$prefix"
export CODEGRAPH_TELEMETRY=0 DO_NOT_TRACK=1
if [[ -x "$prefix/node_modules/.bin/codegraph" ]] && [[ "$(cat "$prefix/.version" 2>/dev/null)" == "$codegraph_version" ]]; then
  echo "    present"
else
  (cd "$prefix" && npm install --no-audit --no-fund --silent "@colbymchenry/codegraph@$codegraph_version")
  echo "$codegraph_version" >"$prefix/.version"
  echo "    installed"
fi

echo "==> index this repo (init on the first run, incremental after)"
if [[ -d "$root/.codegraph" ]]; then
  "$root/scripts/codegraph.sh" index --quiet . || echo "    index failed; run scripts/codegraph.sh index . later" >&2
else
  "$root/scripts/codegraph.sh" init . </dev/null || echo "    init failed; run scripts/codegraph.sh init . later" >&2
fi

echo "==> OpenWiki $openwiki_version at $openwiki_commit (project-local, patched, telemetry off)"
if openwiki_verify >/dev/null 2>&1; then
  echo "    present"
else
  openwiki_build
  echo "    built"
fi

echo "==> git pre-commit shim (verifies the receipts)"
hook="$(git rev-parse --git-dir)/hooks/pre-commit"
cat >"$hook" <<'EOF'
#!/usr/bin/env bash
# Installed by scripts/setup-pipeline-tools.sh. Gated files need a matching gate receipt;
# a commit made while the work record (docs/planning, a checkout of its own) holds a
# completed spec it has not committed yet delivers that pipeline, and needs a matching
# OpenWiki completion receipt too, unless a waiver is in force.
root=$(git rev-parse --show-toplevel)
source "$root/bin/lib-gate.sh"
gated=0
completed=0
while IFS= read -r f; do
  rusty_is_gated "$f" && gated=1
done < <(git diff --cached --no-renames --name-only)
[[ -n "$(rusty_undelivered_completed)" ]] && completed=1
if [[ $gated -eq 1 ]] && ! msg=$(rusty_verify_receipt); then
  echo "pre-commit: gated files staged and $msg" >&2
  exit 1
fi
if [[ $gated -eq 1 ]] && ! msg=$(rusty_commit_matches_worktree); then
  echo "pre-commit: $msg" >&2
  exit 1
fi
if [[ $completed -eq 1 && ! -f "$root/docs/planning/pipeline/WAIVER.md" ]] && ! msg=$(rusty_verify_openwiki_receipt); then
  echo "pre-commit: this commit delivers a completed pipeline (the record holds an uncommitted completed spec) and $msg" >&2
  exit 1
fi
exit 0
EOF
chmod +x "$hook"
echo "    $hook"

echo "done. MCP servers: .mcp.json (Claude Code), .codex/config.toml (Codex). Restart the session once so the codegraph and openwiki servers are picked up."
