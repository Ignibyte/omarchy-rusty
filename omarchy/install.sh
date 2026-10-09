#!/usr/bin/env bash
# Install Rusty on this machine: the three binaries, the back end's user service and a
# reminder of the MCP config. From a release tarball it installs the binaries in `bin/`
# beside this folder; from a checkout it builds them with cargo. Every step is
# idempotent, so run it again after pulling or unpacking a newer release.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
bin="$HOME/.local/bin"
unit_dir="$HOME/.config/systemd/user"
mcp_url="http://127.0.0.1:4174/mcp"

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing: $1" >&2; exit 1; }; }
need systemctl
need curl
need git

if [[ -x "$repo/bin/rusty-mcp" && -x "$repo/bin/rusty-cli" && -x "$repo/bin/rusty" ]]; then
  echo "==> installing the release's rusty-mcp, rusty-cli and rusty into $bin"
  mkdir -p "$bin"
  for b in rusty-mcp rusty-cli rusty; do
    install -m755 "$repo/bin/$b" "$bin/$b"
  done
else
  need cargo
  echo "==> building rusty-mcp, rusty-cli and rusty into $bin"
  cargo install --path "$repo/crates/rusty-mcp" --root "$HOME/.local" --force --locked
  cargo install --path "$repo/crates/rusty-cli" --root "$HOME/.local" --force --locked
  # `rusty` answers `rusty session` and store scripts. --force replaces a binary an older
  # install left under that name.
  cargo install --path "$repo/crates/rusty-cmd" --root "$HOME/.local" --force --locked
fi
case ":$PATH:" in
  *":$bin:"*) ;;
  *) echo "    note: $bin is not on PATH yet" ;;
esac

echo "==> the back end's user service"
# Earlier installs put a rusty-session wrapper here; `rusty session` replaced it.
rm -f "$bin/rusty-session"
install -Dm644 "$here/rusty-mcp.service" "$unit_dir/rusty-mcp.service"
systemctl --user daemon-reload
systemctl --user enable rusty-mcp >/dev/null
systemctl --user restart rusty-mcp
answered=""
for _ in $(seq 1 30); do
  if curl -fs -o /dev/null -X POST "$mcp_url" \
      -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' \
      -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"install","version":"0"}}}' 2>/dev/null; then
    answered=yes
    break
  fi
  sleep 0.5
done
if [ -z "$answered" ]; then
  echo "    rusty-mcp did not answer on $mcp_url; see: journalctl --user -u rusty-mcp" >&2
  exit 1
fi
echo "    answering on $mcp_url"

echo "==> memory pressure"
echo "    two steps this script leaves to you (another program's unit; root):"
echo "      compositor last:  install -Dm644 $here/wayland-wm-oom.conf ~/.config/systemd/user/wayland-wm@hyprland.desktop.service.d/60-oom.conf && systemctl --user daemon-reload"
echo "      earlyoom, if used: add Hyprland and rusty-mcp to --avoid in /etc/default/earlyoom (see $here/README.md)"

echo "==> MCP config"
echo "    agents (stdio): add to .mcp.json  ->  \"rusty\": {\"type\": \"stdio\", \"command\": \"rusty-mcp\"}"
echo "    http clients:   $mcp_url   (both forms in $here/mcp-config.json)"
echo "done"
