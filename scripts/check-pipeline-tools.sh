#!/usr/bin/env bash
# Are the pipeline tools prepared? Exit 1 with what is missing. `openwiki` as the only
# argument checks OpenWiki alone (the MCP wrapper asks this before it starts).
set -Eeuo pipefail
root=$(git rev-parse --show-toplevel 2>/dev/null) || { echo "run inside the repo" >&2; exit 1; }
# shellcheck source=scripts/lib-openwiki.sh
source "$root/scripts/lib-openwiki.sh"
missing=0
if [[ "${1:-}" == "openwiki" ]]; then
  if msg=$(openwiki_verify); then echo "$msg"; exit 0; else echo "missing: $msg (scripts/setup-pipeline-tools.sh)"; exit 1; fi
fi
[[ -x "$root/.dev/pipeline-tools/codegraph/node_modules/.bin/codegraph" ]] || { echo "missing: CodeGraph (scripts/setup-pipeline-tools.sh)"; missing=1; }
[[ -d "$root/.codegraph" ]] || { echo "missing: CodeGraph index (scripts/codegraph.sh index .)"; missing=1; }
if ! msg=$(openwiki_verify); then echo "missing: $msg (scripts/setup-pipeline-tools.sh)"; missing=1; fi
[[ -x "$(git -C "$root" rev-parse --git-dir)/hooks/pre-commit" ]] || { echo "missing: git pre-commit shim (scripts/setup-pipeline-tools.sh)"; missing=1; }
command -v jq >/dev/null 2>&1 || { echo "missing: jq (the hooks need it)"; missing=1; }
[[ $missing -eq 0 ]] && echo "pipeline tools ready"
exit $missing
