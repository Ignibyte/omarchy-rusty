#!/usr/bin/env bash
# Start CodeGraph's MCP server for this repo (wired in .mcp.json and .codex/config.toml).
set -Eeuo pipefail
root=$(git rev-parse --show-toplevel 2>/dev/null) || { echo "run inside the repo" >&2; exit 1; }
bin="$root/.dev/pipeline-tools/codegraph/node_modules/.bin/codegraph"
[[ -x "$bin" ]] || { echo "CodeGraph is not prepared; run scripts/setup-pipeline-tools.sh" >&2; exit 1; }
export CODEGRAPH_TELEMETRY=0 DO_NOT_TRACK=1
cd "$root"
exec "$bin" serve --mcp
