#!/usr/bin/env bash
# OpenWiki, pinned and project-local: what it is pinned to, how it is built, and how a
# build is verified against its provenance. Sourced by scripts/setup-pipeline-tools.sh
# and scripts/check-pipeline-tools.sh. The pins are the ones OmarchyGS verified.
#
# Two local patches are applied to the checkout (ignored generated state, never
# upstreamed): the scheduled refresh workflow is never created, and the guidance
# OpenWiki writes into the agent guides names the project-local lifecycle instead of a
# hosted one. OpenWiki's own model-driven modes are not used; the host agent authors
# the pages through the MCP lifecycle, so nothing leaves the machine.

openwiki_version="0.3.3"
openwiki_commit="a525ed88fe1f189d08e0f0acf12f42caec2b600e"
openwiki_repo="https://github.com/langchain-ai/openwiki.git"
openwiki_pnpm_version="10.33.2"
openwiki_pnpm_sha512="a90faf6feeab71ad6c6e57f94e0fe1a12f5dcc22cd754db40ae9593eb6a3e0b6b12e3540218bb37ae083404b1f2ce6db2a4121e979829b4aff94b99f49da1cf8"
openwiki_package_manager="pnpm@$openwiki_pnpm_version+sha512.$openwiki_pnpm_sha512"
openwiki_upstream_guidance='The scheduled OpenWiki GitHub Actions workflow refreshes the repository wiki. Do not hand-edit generated OpenWiki pages unless explicitly asked; prefer updating source code/docs and letting OpenWiki regenerate.'
openwiki_local_guidance='Refresh the repository wiki only through the project-local OpenWiki lifecycle (the openwiki skill, at Phase 5 of every pipeline). There is no scheduled or hosted refresh. Do not hand-edit generated OpenWiki pages; update the source or the docs and let the lifecycle regenerate the affected pages.'
openwiki_expected_changes=$'src/ingestion/code-mode.ts\nsrc/integrations/core/session-manager.ts'

openwiki_root() { git rev-parse --show-toplevel 2>/dev/null; }
openwiki_prefix() { printf '%s/.dev/pipeline-tools\n' "$(openwiki_root)"; }
openwiki_source() { printf '%s/openwiki\n' "$(openwiki_prefix)"; }
openwiki_pnpm_prefix() { printf '%s/pnpm\n' "$(openwiki_prefix)"; }
openwiki_provenance() { printf '%s/openwiki-build.provenance\n' "$(openwiki_prefix)"; }
openwiki_cli() { printf '%s/dist/cli/cli.js\n' "$(openwiki_source)"; }

# sha256 over every file's path and content under a directory.
openwiki_tree_digest() {
  local dir=$1
  (cd "$dir" && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum) | sha256sum | cut -c1-64
}

openwiki_provenance_value() {
  sed -n "s/^$1=//p" "$(openwiki_provenance)" | head -1
}

# Is the build present, at the pins, patched, and as the provenance recorded it?
# Prints the first problem.
openwiki_verify() {
  local src pnpm prov mode session
  src=$(openwiki_source); pnpm=$(openwiki_pnpm_prefix); prov=$(openwiki_provenance)
  mode="$src/src/ingestion/code-mode.ts"
  session="$src/src/integrations/core/session-manager.ts"
  [[ -f "$src/package.json" && -f "$prov" && -f "$(openwiki_cli)" ]] || { echo "OpenWiki is not built"; return 1; }
  [[ "$(node -p 'require(process.argv[1]).version' "$src/package.json")" == "$openwiki_version" ]] || { echo "OpenWiki version is not $openwiki_version"; return 1; }
  [[ "$(git -C "$src" rev-parse HEAD)" == "$openwiki_commit" ]] || { echo "OpenWiki checkout is not at $openwiki_commit"; return 1; }
  [[ "$(node -p 'require(process.argv[1]).packageManager' "$src/package.json")" == "$openwiki_package_manager" ]] || { echo "OpenWiki's package manager pin changed"; return 1; }
  grep -Fq 'createWorkflow: false' "$session" || { echo "OpenWiki's no-workflow patch is missing"; return 1; }
  grep -Fq "$openwiki_local_guidance" "$mode" && grep -Fq "$openwiki_local_guidance" "$src/dist/ingestion/code-mode.js" \
    && ! grep -Fq "$openwiki_upstream_guidance" "$src/dist/ingestion/code-mode.js" || { echo "OpenWiki's local-guidance patch is missing"; return 1; }
  [[ "$(git -C "$src" diff --name-only | LC_ALL=C sort)" == "$openwiki_expected_changes" ]] || { echo "OpenWiki's checkout has changes beyond the two patches"; return 1; }
  [[ -f "$src/node_modules/.modules.yaml" && ! -e "$src/package-lock.json" ]] || { echo "OpenWiki's dependencies were not installed from the frozen pnpm lock"; return 1; }
  [[ "$(openwiki_provenance_value version)" == "1" \
    && "$(openwiki_provenance_value commit)" == "$openwiki_commit" \
    && "$(openwiki_provenance_value package_manager)" == "$openwiki_package_manager" \
    && "$(openwiki_provenance_value pnpm_tree_sha256)" == "$(openwiki_tree_digest "$pnpm/node_modules/pnpm")" \
    && "$(openwiki_provenance_value lock_sha256)" == "$(sha256sum "$src/pnpm-lock.yaml" | cut -c1-64)" \
    && "$(openwiki_provenance_value patch_sha256)" == "$(git -C "$src" diff --binary | sha256sum | cut -c1-64)" \
    && "$(openwiki_provenance_value dist_sha256)" == "$(openwiki_tree_digest "$src/dist")" ]] \
    || { echo "OpenWiki's build provenance is stale"; return 1; }
  echo "OpenWiki $openwiki_version at $openwiki_commit, patched, provenance matches"
}

# Clone, pin, patch, install with a verified pnpm from the frozen lock, build, record.
openwiki_build() {
  local src pnpm prov mode session tarball got bootstrap
  src=$(openwiki_source); pnpm=$(openwiki_pnpm_prefix); prov=$(openwiki_provenance)
  mode="$src/src/ingestion/code-mode.ts"
  session="$src/src/integrations/core/session-manager.ts"
  mkdir -p "$(openwiki_prefix)"
  export DO_NOT_TRACK=1 OPENWIKI_TELEMETRY_DISABLED=1

  if [[ ! -d "$src/.git" ]]; then
    [[ ! -e "$src" ]] || { echo "unexpected content at $src; remove it first" >&2; return 1; }
    if [[ -n "${OPENWIKI_MIRROR:-}" && -d "$OPENWIKI_MIRROR/.git" ]]; then
      git clone --quiet --no-checkout "$OPENWIKI_MIRROR" "$src"
    else
      git clone --quiet --no-checkout "$openwiki_repo" "$src"
    fi
  fi
  git -C "$src" checkout --quiet --detach "$openwiki_commit"
  [[ "$(git -C "$src" rev-parse HEAD)" == "$openwiki_commit" ]] || { echo "could not check out $openwiki_commit" >&2; return 1; }
  [[ "$(node -p 'require(process.argv[1]).packageManager' "$src/package.json")" == "$openwiki_package_manager" ]] \
    || { echo "OpenWiki's package-manager pin changed; refusing an unreviewed bootstrap" >&2; return 1; }

  # The two patches, applied once.
  grep -Fq "$openwiki_upstream_guidance" "$mode" || grep -Fq "$openwiki_local_guidance" "$mode" \
    || { echo "OpenWiki's agent guidance text changed upstream; the patch needs review" >&2; return 1; }
  grep -Fq 'createWorkflow: input.mode === "init"' "$session" || grep -Fq 'createWorkflow: false' "$session" \
    || { echo "OpenWiki's workflow setup changed upstream; the patch needs review" >&2; return 1; }
  sed -i "s|$openwiki_upstream_guidance|$openwiki_local_guidance|" "$mode"
  sed -i 's/createWorkflow: input.mode === "init"/createWorkflow: false/' "$session"

  # pnpm, by its SHA-512, into its own prefix.
  bootstrap=$(mktemp -d)
  npm pack --silent --pack-destination "$bootstrap" "pnpm@$openwiki_pnpm_version" >/dev/null
  tarball=$(find "$bootstrap" -maxdepth 1 -type f -name 'pnpm-*.tgz' -print -quit)
  [[ -n "$tarball" ]] || { echo "pnpm tarball not downloaded" >&2; return 1; }
  got=$(sha512sum "$tarball" | awk '{print $1}')
  [[ "$got" == "$openwiki_pnpm_sha512" ]] || { echo "pnpm tarball does not match its SHA-512 pin" >&2; return 1; }
  rm -rf -- "$pnpm"
  mkdir -p "$pnpm"
  npm install --prefix "$pnpm" --no-save --ignore-scripts --no-audit --no-fund --silent "$tarball"
  rm -rf -- "$bootstrap"
  [[ "$("$pnpm/node_modules/.bin/pnpm" --version)" == "$openwiki_pnpm_version" ]] || { echo "pnpm bootstrap has the wrong version" >&2; return 1; }

  rm -rf -- "$src/node_modules" "$src/package-lock.json"
  "$pnpm/node_modules/.bin/pnpm" --dir "$src" install --frozen-lockfile --ignore-scripts --prod=false --silent
  "$src/node_modules/.bin/tsc" -p "$src/tsconfig.json"
  "$src/node_modules/.bin/tsc" -p "$src/tsconfig.client.json"
  node "$src/scripts/copy-visualize-assets.cjs"
  chmod +x "$(openwiki_cli)"
  [[ "$(git -C "$src" diff --name-only | LC_ALL=C sort)" == "$openwiki_expected_changes" ]] \
    || { echo "the checkout carries changes beyond the two patches" >&2; return 1; }
  {
    printf 'version=1\n'
    printf 'commit=%s\n' "$openwiki_commit"
    printf 'package_manager=%s\n' "$openwiki_package_manager"
    printf 'pnpm_tree_sha256=%s\n' "$(openwiki_tree_digest "$pnpm/node_modules/pnpm")"
    printf 'lock_sha256=%s\n' "$(sha256sum "$src/pnpm-lock.yaml" | cut -c1-64)"
    printf 'patch_sha256=%s\n' "$(git -C "$src" diff --binary | sha256sum | cut -c1-64)"
    printf 'dist_sha256=%s\n' "$(openwiki_tree_digest "$src/dist")"
  } >"$prov"
  openwiki_verify >/dev/null
}
