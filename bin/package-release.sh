#!/usr/bin/env bash
# Build a release tarball: the three binaries in release mode, the installer, the back
# end's unit, the MCP config snippet, the compositor drop-in, README, LICENSE,
# THIRD_PARTY_NOTICES, CHANGELOG, SECURITY and the docs/ references, as dist/rusty-<version>-<arch>-linux.tar.gz with a .sha256 beside it. The
# release workflow runs this, and a maintainer can, to test exactly what a release ships.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
[[ -n "$version" ]] || { echo "no version in Cargo.toml" >&2; exit 1; }
name="rusty-${version}-$(uname -m)-linux"
target=${CARGO_TARGET_DIR:-$root/target}

cargo build --release --locked -p rusty-mcp -p rusty-cli -p rusty-cmd

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
dir="$stage/$name"
mkdir -p "$dir/bin" "$dir/omarchy"
install -m755 "$target/release/rusty-mcp" "$target/release/rusty-cli" "$target/release/rusty" "$dir/bin/"
install -m755 omarchy/install.sh "$dir/omarchy/"
install -m644 omarchy/rusty-mcp.service omarchy/mcp-config.json omarchy/wayland-wm-oom.conf \
  omarchy/README.md "$dir/omarchy/"
install -m644 README.md LICENSE THIRD_PARTY_NOTICES.md CHANGELOG.md SECURITY.md "$dir/"
# The references the README links to.
mkdir -p "$dir/docs/architecture"
install -m644 docs/*.md "$dir/docs/"
install -m644 docs/architecture/*.md "$dir/docs/architecture/"

mkdir -p dist
rm -f "dist/$name.tar.gz" "dist/$name.tar.gz.sha256"
tar -C "$stage" -czf "dist/$name.tar.gz" "$name"
(cd dist && sha256sum "$name.tar.gz" >"$name.tar.gz.sha256")
echo "dist/$name.tar.gz"
