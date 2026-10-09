#!/usr/bin/env bash
# OpenWiki's MCP server, project-local and pinned (scripts/setup-pipeline-tools.sh). The
# `openwiki` skill drives its four lifecycle tools at Phase 5; the host agent authors the
# pages. Telemetry is off. `OPENWIKI_HOST` names the host in the run metadata (claude by
# default; the Codex config sets codex).
set -Eeuo pipefail
root=$(git rev-parse --show-toplevel 2>/dev/null) || { echo "run inside the repo" >&2; exit 1; }
cli="$root/.dev/pipeline-tools/openwiki/dist/cli/cli.js"
[[ -f "$cli" ]] || { echo "OpenWiki is not prepared; run scripts/setup-pipeline-tools.sh" >&2; exit 1; }
"$root/scripts/check-pipeline-tools.sh" openwiki >/dev/null || {
  echo "OpenWiki's build provenance does not match its pins; run scripts/setup-pipeline-tools.sh" >&2
  exit 1
}
export DO_NOT_TRACK=1 OPENWIKI_TELEMETRY_DISABLED=1
cd "$root"
exec node "$cli" mcp --host "${OPENWIKI_HOST:-claude}"
