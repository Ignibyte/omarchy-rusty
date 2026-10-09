# Omarchy integration

How Rusty runs on Omarchy. `install.sh` ties it together, and every step in it is
idempotent.

| File | Purpose |
|---|---|
| `install.sh` | the three binaries into `~/.local/bin` (`rusty-mcp`, `rusty-cli`, and `rusty`, the command: built with cargo from a checkout, copied from a release's `bin/`), the back end's user service, and the two memory-pressure steps below printed for you to apply |
| `rusty-mcp.service` | the back end over Streamable HTTP on localhost, wanted by `default.target`, restarted after any exit but a stop |
| `wayland-wm-oom.conf` | a drop-in for uwsm's compositor unit so Hyprland is the last of the session to go under memory pressure; pointed at, never applied |
| `mcp-config.json` | the `mcpServers` entries for a JSON MCP config such as Claude Code's `.mcp.json` (stdio) and for HTTP clients; Codex takes TOML (see the README) |

Rusty has no window of its own. [Marley](https://github.com/Ignibyte/marley_ide), a Zed
fork, draws a knowledge workspace over the back end's HTTP endpoint, and agents start
their own `rusty-mcp` over stdio.

## The back end

The back end comes back on its own. It is a user service wanted by `default.target`, so
it runs whether or not a desktop is up, and `Restart=always` brings it back after any exit
except `systemctl --user stop`: a session teardown and earlyoom both send SIGTERM, which
`on-failure` would have treated as clean.

`rusty session` is the way in from a terminal. `start` starts the unit, which does
nothing when it runs already, and prints the status; `status` reads the unit and posts an
`initialize` to the port. Every other bare word is a store script or an error; `rusty
help`, or `rusty` alone, lists the nouns.

```bash
rusty session start
rusty session status
journalctl --user -u rusty-mcp -f
```

## Memory pressure

A heavy build or test run can push a machine past its memory, and earlyoom then kills
whatever scores highest; in a uwsm session every process, the compositor included, sits at
an OOM score of 200, so Hyprland can go first. Two protections put the compositor last.
Neither is applied by `install.sh`: one is another program's unit, the other needs root.

1. The compositor drop-in, user level. 100 is the user manager's own score and the lowest
   a user unit can set; a request for less comes out at 100.

   ```bash
   install -Dm644 omarchy/wayland-wm-oom.conf \
     ~/.config/systemd/user/wayland-wm@hyprland.desktop.service.d/60-oom.conf
   systemctl --user daemon-reload      # takes effect at the next login
   ```

2. If you run earlyoom: its avoid list, as root. In `/etc/default/earlyoom`, add
   `Hyprland` and `rusty-mcp` to the `--avoid` pattern, then
   `sudo systemctl restart earlyoom`:

   ```
   EARLYOOM_ARGS="-r 3600 --avoid '(^|/)(systemd|systemd-logind|dbus-daemon|dbus-broker|Hyprland|rusty-mcp)$'"
   ```

   earlyoom then kills anything else first. The kernel's own OOM killer still scores by
   the adjustment, which is what the drop-in is for.
