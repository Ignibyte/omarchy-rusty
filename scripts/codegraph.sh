#!/usr/bin/env bash
# CodeGraph CLI, project-local. `scripts/codegraph.sh explore "<query>"` is the fallback
# for the codegraph_explore MCP tool before a session restart; also index, status,
# query, impact.
set -Eeuo pipefail
root=$(git rev-parse --show-toplevel 2>/dev/null) || { echo "run inside the repo" >&2; exit 1; }
bin="$root/.dev/pipeline-tools/codegraph/node_modules/.bin/codegraph"
[[ -x "$bin" ]] || { echo "CodeGraph is not prepared; run scripts/setup-pipeline-tools.sh" >&2; exit 1; }
export CODEGRAPH_TELEMETRY=0 DO_NOT_TRACK=1
cd "$root"
exec "$bin" "$@"
