---
type: "Reference"
title: "Development and validation"
openwiki_generated: true
sources:
  - id: openwiki-source-164e2da859b5277df81c7d94
    resource: repo://.github/workflows/ci.yml
  - id: openwiki-source-99a6e48b7904a84607bbfebf
    resource: repo://crates/rusty-app/src/agent/host.rs
  - id: openwiki-source-c98365caf5b5f92dde34e1ad
    resource: repo://crates/rusty-app/src/agent/launch.rs
  - id: openwiki-source-c8c0347aa7a687c601520d1a
    resource: repo://crates/rusty-app/src/main.rs
  - id: openwiki-source-188c50fac039d5c4d0e7eca9
    resource: repo://crates/rusty-app/src/session.rs
  - id: openwiki-source-6f3f2b250ced9228bf466afd
    resource: repo://omarchy/com.ignibyte.rusty.desktop
  - id: openwiki-source-b3f552fe80d52d1be46f06fa
    resource: repo://omarchy/hyprland-bindings.conf
  - id: openwiki-source-4d8ab597958b0e5c2507d7fd
    resource: repo://omarchy/install.sh
  - id: openwiki-source-6f895b21354ce2eb09c53bef
    resource: repo://omarchy/rusty-app.service
  - id: openwiki-source-40bfddd6b1c627968cf41f77
    resource: repo://omarchy/wayland-wm-oom.conf
  - id: openwiki-source-74bdf832aa1ee5e3f40cd980
    resource: repo://packaging/PKGBUILD
  - id: openwiki-source-d4dc2c7ea0d931bfc9466b41
    resource: repo://scripts/screenshot.sh
generated: {by: "claude-code", at: "2026-09-17T22:06:12.177Z"}
verified:
  - by: openwiki/0.3.3
    at: 2026-09-17T22:16:07.719Z
---

# Development and validation

## Purpose

The commands that build, test and prove a change, the narrowest one for each kind of
change, and the ways the machine's state is kept out of the record.

## Building

- A Cargo workspace of four crates. `cargo build` builds all; `cargo build -p rusty-app`
  builds the app (cxx-qt generates the bridges and runs moc on the two C++ classes in
  `crates/rusty-app/cpp/`; Qt 6 and `qmltermwidget` come from the system).
- One cargo command at a time, never killed: a killed or concurrent cargo corrupts the
  incremental cache.
- `omarchy/install.sh`: release builds of the three binaries into `~/.local/bin`, the
  desktop entry and icon (its `Exec` is `rusty session start`), the two user units
  (installed and enabled; the back end restarted and probed over HTTP; the app started
  through `rusty session start` when it was built and a graphical session is active), the
  MCP config snippets, and pointers to the compositor drop-in and the earlyoom line it does
  not apply. It deletes a stale `~/.local/bin/rusty-session` left by an earlier install.
  Idempotent.
- `packaging/PKGBUILD` (`rusty-git`) builds the same for the AUR, `!lto`, and installs
  both units (paths rewritten to `/usr/bin`; the app unit runs `/usr/bin/rusty session
  run`), the drop-in and the key snippet under `/usr/share/rusty/`. No wrapper script is
  packaged.

## Running as services

- `omarchy/rusty-mcp.service` is wanted by `default.target`, so the back end serves with
  or without a desktop. `Restart=always` brings it back two seconds after any exit but
  `systemctl --user stop`; a session teardown and earlyoom both send SIGTERM, which
  `on-failure` would treat as clean. `OOMScoreAdjust=100` is the lowest a user unit can
  set (the user manager's own score; its services default to 200).
- `omarchy/rusty-app.service` is wanted by and ordered after uwsm's
  `graphical-session.target`, stops with it (`PartOf`), runs in `app-graphical.slice`
  after `rusty-mcp.service`, restarts after any exit except status 0 (a quit stays quit,
  a kill or a crash comes back), and logs under the identifier `rusty`. systemd's start
  limit, five starts in ten seconds, ends a crash loop.
- `rusty session` is the one entry point the installer, the desktop entry and the key
  share (TICKET-029): a noun of the app binary, decided in `crates/rusty-app/src/session.rs`
  before Qt starts, in place of the `rusty-session` script TICKET-009 installed. `start`
  starts the back end, imports the display variables into the user manager when a
  compositor started outside uwsm left them out, refuses a second window while a `rusty`
  outside the unit is running (its own process and the unit's main pid excepted), and
  starts the app unit; `stop` stops the app unit alone; `status` reads both units, posts
  an `initialize` to the port and lists the app's processes; `run` is the unit's command:
  PATH completed with `~/.local/bin` and `~/.cargo/bin` in-process, then the window in the
  same process. Because the unit now executes the binary itself, a missing binary is a
  failed exec (systemd status 203) that `Restart=always` retries until the start limit
  ends it, where the script exited 0 and left the unit stopped. A test reads every file
  under `omarchy/` and `packaging/` and refuses the old invocations and any app-unit
  `ExecStart` other than `rusty session run`.
- An agent session is a transient unit asked for at run time, not a unit file: nothing in
  the installer or the package declares `rusty-agent-<id>.service`, and because the host
  is started as `current_exe`, it is always the same build as the binary that started it
  (a development binary under `target/debug` while working, `~/.local/bin/rusty`
  installed, `/usr/bin/rusty` packaged). `systemctl --user list-units 'rusty-agent-*'`
  and `journalctl --user -t rusty-agent` are how the sessions are seen; `rusty agent
  list|stop|rm` is how they are managed. They sit in `app.slice`, so a compositor restart
  leaves them running, and they are not lingering, so a logout stops each one through its
  own stop sequence and the entries say `stopped` until something opens them again.
- `omarchy/wayland-wm-oom.conf` is a drop-in for the compositor unit (`OOMScoreAdjust=100`)
  that the installer points at and never applies, being another program's unit; the
  earlyoom avoid line, which needs root, is documented in `omarchy/README.md`. No Wayland
  client outlives its compositor: the next login starts the app unit, and the agent
  sessions, the tmux sessions and the state files under `~/.config/rusty/` reattach. The
  tmux server behind the terminal tabs is started by the app and so lives in the app
  unit's cgroup: it ends with a stop or a crash restart of that unit, which TICKET-034
  takes.

## Testing

- `cargo test -p rusty-core`: the managers, the vault, the renderer, the scanner, the
  semantic index; scratch directories under the system temp dir, an in-memory database.
- `cargo test -p rusty-mcp`: the router tests and `tests/smoke.rs`, which spawns the
  built binary over stdio in a scratch `HOME` and walks tasks, resources and the
  workspace tools.
- `cargo test -p rusty-app`: the highlighter's tokenizer, the tabs JSON, the theme
  tokens and colour math, the command dispatch and PATH completion (`session::tests`), the
  theme directory's two locations against a scratch home, and the shipped-files check
  above.
- UI probes never drive the app with synthetic input on the user's desktop and never
  touch real data; screenshots and log checks stand in.

## The gate and CI

- `bin/gate.sh --fast` while working; `bin/gate.sh --diff` before delivery, which
  writes the receipt the commit hooks check; `--verify` to confirm it.
- `.github/workflows/ci.yml` runs the same set on every push (Qt installed with apt,
  `QMAKE` pointed at Qt 6) as a second witness; it never replaces the local receipt.

## Screenshots and visual evidence

`scripts/screenshot.sh <dir> [scene ...]` builds a scratch vault of invented pages,
starts a scratch `rusty-mcp` on its own port, and runs the app offscreen
(`QT_QPA_PLATFORM=offscreen`) with `RUSTY_SHOT`, `RUSTY_SHOT_SCENE`, `RUSTY_TABS`,
`RUSTY_STATE` and `RUSTY_OMARCHY_THEME_DIR` so that no real data, no workspace switch
and no user setting is involved; `SHOT_THEME`, `SHOT_SIZE`, `SHOT_ENV` and `SHOT_KEEP`
adjust it. The app grabs itself through `Tools.grabWindow`, because an item grab cannot
start on the offscreen platform. The images in `docs/screenshots/` come from it.

## Logs

- The back end logs to stderr (`journalctl --user -u rusty-mcp` for the service).
- The app under its unit: `journalctl --user -u rusty-app`, or `journalctl -t rusty`,
  the identifier the unit sets.
- The app's Qt messages go to journald when it is not started from a terminal
  (`journalctl --user -t rusty`); `QT_FORCE_STDERR_LOGGING=1` keeps them on stderr;
  `RUSTY_DEBUG=1` adds a line per tab, title, shot and settings event.

## Data on this machine

`~/.rusty/`: `rusty.db`, `brain/` (a git repository the managers commit to),
`notes/`, `skills/`, `.secret`. `~/.config/rusty/`: `tabs.json`, `workspace.json`,
`color-schemes/`. `~/.local/state/rusty/agents/`: one JSON entry per agent session beside
a directory with its event log, for the user alone; `$XDG_RUNTIME_DIR/rusty/agents/`: the
sessions' sockets. Tests and the screenshot script never use these paths:
`RUSTY_AGENT_STATE_DIR` and `RUSTY_AGENT_RUN_DIR` point them at a scratch directory, as
`RUSTY_TABS` and `RUSTY_STATE` do for the workspace's own.

## Primary sources

- `bin/gate.sh`, `.github/workflows/ci.yml`, `omarchy/install.sh`, `packaging/PKGBUILD`
- `crates/rusty-app/src/session.rs`, `omarchy/rusty-app.service`, `omarchy/rusty-mcp.service`,
  `omarchy/wayland-wm-oom.conf`
- `crates/rusty-app/src/agent/launch.rs`, `crates/rusty-app/src/agent/host.rs`,
  `scripts/screenshot.sh`, `scripts/probe-claude-wire.sh`
- `crates/rusty-mcp/tests/smoke.rs`, `scripts/screenshot.sh`
