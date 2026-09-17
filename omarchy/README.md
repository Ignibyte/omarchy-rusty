# Omarchy integration

What makes Rusty an Omarchy app rather than a program that happens to run there.
`install.sh` ties it together, and every step in it is idempotent.

| File | Purpose |
|---|---|
| `install.sh` | dependencies through `omarchy pkg add`, release builds of the three binaries into `~/.local/bin`, the desktop entry and icon, the two user services, and the pointers below |
| `rusty-mcp.service` | the back end over Streamable HTTP on localhost, wanted by `default.target`, restarted after any exit but a stop |
| `rusty-app.service` | the app, wanted by `graphical-session.target`, restarted when it is killed, left alone when it is quit; its command is `rusty session run` |
| `wayland-wm-oom.conf` | a drop-in for uwsm's compositor unit so Hyprland is the last of the session to go under memory pressure; pointed at, never applied |
| `com.ignibyte.rusty.desktop`, `com.ignibyte.rusty.svg` | the launcher entry and icon; `Exec` is `rusty session start` |
| `hyprland-bindings.conf` | SUPER+ALT+R: focus the window, or `rusty session start`; append it to `~/.config/hypr/bindings.conf` |
| `mcp-config.json` | the `mcpServers` entries for Claude Code and Codex (stdio) and for HTTP clients |

Conventions Rusty follows: apps run in their own user units or through `uwsm-app`, the
theme is read from `~/.local/state/omarchy/current/theme/` (Omarchy 4; before it,
`~/.config/omarchy/current/theme/`), windows are found and focused with
`omarchy-launch-or-focus`, and nothing under `/usr/share/omarchy/` is ever edited.

## The session

Rusty comes back on its own. The back end is a user service wanted by `default.target`,
so it runs whether or not a desktop is up, and `Restart=always` brings it back after any
exit except `systemctl --user stop`: a session teardown and earlyoom both send SIGTERM,
which `on-failure` would have treated as clean. The app is a user service wanted by
`graphical-session.target`, the target uwsm raises at login and lowers at logout, so a
login starts it, a kill or a crash starts it again two seconds later, and a quit (exit 0)
leaves it stopped. Five starts inside ten seconds trip systemd's start limit, which ends
a crash loop.

`rusty session` is the one path in, a noun of the app binary (TICKET-029; a
`rusty-session` script did this before, and the installer removes a stale copy). `start`
starts the back end, copies the display variables into the user manager when a compositor
started outside uwsm left them out, refuses to open a second window when a `rusty`
started from a terminal is still running, and starts the app unit. `stop` stops the app
and keeps the back end. `status` reads both units, posts an `initialize` to the port, and
lists the app's processes. `run` is the unit's command: it completes PATH with
`~/.local/bin` and `~/.cargo/bin`, where the agent CLIs tend to live, and opens the window
in the same process. Every other bare word is a store script or an error; `rusty help`
lists the nouns.

```bash
rusty session start
rusty session status
journalctl --user -u rusty-app -f      # or: journalctl -t rusty
journalctl --user -u rusty-mcp -f
```

No Wayland client outlives its compositor. When Hyprland dies the app dies with it, and
what survives is the state: `tabs.json` and `workspace.json` under `~/.config/rusty/`, the
agent sessions (below), and the back end. The next login starts the app unit, which
reattaches all of it.

The tmux server behind the agent *tabs* is started by the app, so it lives in
`rusty-app.service`'s cgroup and a stop or a crash restart of that unit ends it with the
app. TICKET-034 moves it into a unit of its own; until then, a terminal tab's session
survives a quit (the cgroup is cleaned only when the unit stops) but not
`systemctl --user restart rusty-app`.

## Agent sessions

A conversation with Claude Code is not part of the app's lifetime. `rusty agent start`
writes an entry under `~/.local/state/rusty/agents/` and asks the user manager for a
transient unit:

```bash
systemd-run --user --quiet --collect --unit=rusty-agent-<id> --service-type=exec \
  --property=KillMode=mixed --property=TimeoutStopSec=20 \
  --property=Restart=on-failure --property=RestartSec=1 \
  --property=SyslogIdentifier=rusty-agent \
  -- rusty agent host --id <id>
```

The unit sits in `app.slice`, not `app-graphical.slice`: the session needs no display and
outlives a compositor restart. `KillMode=mixed` sends the stop signal to the host alone,
so it has its `TimeoutStopSec` to interrupt the turn, close the process's stdin and then
signal it, in that order. `Restart=on-failure` brings a crashed host back (it reopens the
log and resumes the conversation on the next message) while a deliberate stop stays
stopped. `--collect` unloads a unit that failed, so nothing lingers in the unit list; the
journal and the session's own entry keep the record. No `OOMScoreAdjust`: the session
inherits the session-wide 200, and a process the kernel or earlyoom takes is recorded and
resumed on the next message rather than protected above Chad's own apps.

```bash
rusty agent list
rusty agent attach <id>            # the event stream, NDJSON; a typed line is a message
rusty agent stop <id>
systemctl --user list-units 'rusty-agent-*'
journalctl --user -u rusty-agent-<id>      # or: journalctl --user -t rusty-agent
```

Sessions do not survive a logout unless the user lingers (`loginctl enable-linger`), which
is not set here: the user manager stops every unit, each host runs its stop sequence, and
the entries say `stopped`. The next login starts nothing by itself; opening the page, or
`rusty agent start <id>`, runs the host again and the conversation resumes from Claude
Code's own transcript.

## Memory pressure

On 2026-09-03 a four-worker mutation audit pushed the dev box past its memory and
earlyoom killed Hyprland, because every process in a uwsm session, the compositor
included, sits at an OOM score of 200. Two protections put the compositor last. Neither
is applied by `install.sh`: one is another program's unit, the other needs root.

1. The compositor drop-in, user level. 100 is the user manager's own score and the lowest
   a user unit can set; measured on 2026-09-03, a request for less comes out at 100.

   ```bash
   install -Dm644 omarchy/wayland-wm-oom.conf \
     ~/.config/systemd/user/wayland-wm@hyprland.desktop.service.d/60-oom.conf
   systemctl --user daemon-reload      # takes effect at the next login
   ```

2. earlyoom's avoid list, root. In `/etc/default/earlyoom`, add `Hyprland` and
   `rusty-mcp` to the `--avoid` pattern, then `sudo systemctl restart earlyoom`:

   ```
   EARLYOOM_ARGS="-r 3600 --avoid '(^|/)(systemd|systemd-logind|dbus-daemon|dbus-broker|Hyprland|rusty-mcp)$'"
   ```

   earlyoom then kills anything else first. The kernel's own OOM killer still scores by
   the adjustment, which is what the drop-in is for.
