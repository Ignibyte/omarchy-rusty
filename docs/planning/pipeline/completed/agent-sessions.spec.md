---
title: Agent sessions that outlive the app
pipeline_id: 17213969-b6d1-484f-b69f-dc04506b56e2
status: Phase 5 — Complete PASS
ticket: TICKET-031
ticket_doc: docs/planning/tickets/closed/TICKET-031-agent-sessions.md
aar: docs/planning/knowledge/aar/AAR-031-agent-sessions.md
sealed: Chad, 2026-09-17, approving the plan — the host as a systemd transient unit per session ("systemd transient unit (Recommended)"), persistence first, the Claude glyph's click opening the chat tab in TICKET-032; with it the `agent` noun, `uuid` and `libc` in rusty-app and tokio's net, io-util, process, signal and macros features (all already in the workspace lock)
created: 2026-09-17
---

# Agent sessions that outlive the app: spec

## Intent

The agent pane's Claude Code process is a child of the app: one per window, restarted on
every page switch, gone when the app quits, never replayed. Chad wants a persistent
process the front end attaches to, with a friendlier surface on top (TICKET-032). This
ticket builds the persistence: a session host owns the process under a transient systemd
user unit, logs every event and serves them over a Unix socket; the app's `Assistant`
becomes a reconnectable client; the pane survives restarts and replays. The host sits
outside the app's cgroup on purpose — the terminal tabs' tmux server does not, and dies
with the app unit (TICKET-034).

## Scope

- In: the `agent` noun (`start`, `stop`, `list`, `attach`, `rm`, `host`); the host's
  loop (spawn, event log, socket, replay, respawn with `--resume`, idle stop, the stop
  sequence, notifications when nobody is attached); the NDJSON protocol; the registry
  entries and the `Agents` type; `Assistant` as a socket client, instantiable per pane,
  with the signals the tab will need (thinking, permission metadata, allow always, deny
  with a reason, durations, compaction and retry notices, `userMessage`, `answered`,
  `expired`); `Assistant.diff`; the pane on the host (attach when a page has a session,
  create on the first message, detach on a page switch); the stand-in `systemd-run` for
  tests and scenes; the wire probe on claude 2.1.274 with its lines as fixtures.
- Out (named seams, not forgotten): the Agent tab and its cards (TICKET-032); slash
  commands, mentions, images, fork, the context meter, subagent nesting (TICKET-033); the
  tmux server's unit (TICKET-034); log compaction (`rusty agent compact`, when logs get
  big); a Qt-free host crate (the fallback if the host's own memory is unreasonable);
  Codex (no print mode); linger through logout (documented, not applied).

## Acceptance criteria (EARS)

| ID | Requirement | Verification |
|---|---|---|
| REQ-001 | WHEN a conversation is started from the pane or from `rusty agent start`, the system shall run Claude Code under a transient user unit `rusty-agent-<id>` outside the app's cgroup, owned by a host that keeps its stdin and stdout. | test of `systemd_run_args` (every property, `--`, `<exe> agent host --id`); a probe session's `/proc/<pid>/cgroup` in Phase 4 |
| REQ-002 | WHEN a client detaches, quits or restarts while a turn runs, the host and the process shall keep running and the turn shall finish. | host test: the answer lands in the log after the client closed; Phase 4: quit the built app's session, `attach` shows the turn finishing |
| REQ-003 | WHEN a client attaches with a sequence number, the host shall replay the logged events after it, mark the catch-up, then stream live events. | host tests (`since: 0`, `since: N`, two clients); the pane scene reopened over the same scratch state |
| REQ-004 | WHEN the process exits (an error, the idle stop, a kill), the host shall record the exit and respawn it with `--resume` on the next message; a resume the transcript cannot satisfy shall start a fresh session and say so. | host tests: exit-3 fake then respawn with `--resume`; stale-resume fake then a fresh id and a note; idle timeout |
| REQ-005 | WHEN the agent asks a permission or a turn ends while no client is attached, the host shall raise a desktop notification, and not when one is. | host test with a recording notifier |
| REQ-006 | WHEN a stop is requested or the unit gets SIGTERM, the host shall interrupt a running turn, close stdin, then signal the process, in that order, exit 0 and leave the entry stopped and the socket removed. | host stop tests (an EOF-exiting fake, a TERM-trapping fake under a millisecond grace); Phase 4: `systemctl --user kill -s TERM` |
| REQ-007 | WHEN `rusty agent <verb>` runs, the binary shall answer before Qt starts as the `session` noun does, and an unknown verb or a bad id shall print the usage and exit 2. | tests of `session::parse` and `agent::run`; `USAGE` names every verb |
| REQ-008 | WHEN the pane opens a page that has a session, it shall attach without starting a process; the first message on a page without one shall create the session; switching pages shall detach without ending a turn. | reading of `RightPane.qml`; the `right:agent,agent:ask:` scene and its reopening; smoke by Chad after the reinstall |
| REQ-009 | WHEN Claude Code emits thinking, permission metadata (tool use id, suggestions), durations, a compaction or a retry, the parser shall carry them, and an answer shall be able to allow always or deny with a reason. | wire tests on 2.1.274 fixtures; `control_response` shape tests |
| REQ-010 | WHEN `Assistant.diff(old, new)` is called, it shall answer line rows of kind same, add or del, deletions before additions in a changed block. | diff tests |

## Locked decisions

| # | Decision | Why | Alternatives set aside |
|---|---|---|---|
| 1 | Each session is a transient user unit: `systemd-run --user --quiet --collect --unit=rusty-agent-<id> --service-type=exec -p KillMode=mixed -p TimeoutStopSec=20 -p Restart=on-failure -p RestartSec=1 -p SyslogIdentifier=rusty-agent --setenv=PATH=… -- <current_exe()> agent host --id <id>`; default `app.slice`, no OOM score, no working directory | Its own cgroup (the app's tmux server proved the need); `mixed` lets the host run its stop sequence; `on-failure` restarts a crash and honours a stop; `--collect` leaves no failed ghost; `app.slice` survives a compositor restart; `current_exe()` keeps host and caller one build, so no unit file ships | tmux as the supervisor (the server is in the app's cgroup, no journal, no restart); Claude's `--bg` daemon (undocumented attach wire, one client, research preview); a child of the app; one host for all sessions (a crash takes every child) |
| 2 | The wire stays Claude Code's print mode over stream-json (`build_args` of TICKET-025 as the base, `--session-id <id>` for a new session so the Rusty id is the Claude id, `--resume` after an exit); the host logs and broadcasts what it forwards as `sent` lines instead of `--replay-user-messages`; `content_block_delta` events are broadcast live and never logged | The documented, probed wire; `sent` covers control responses and interrupts too, and needs no flag; the `assistant` message carries the whole text, so a replay loses nothing and the log stays a fraction of the turn | scraping the TUI in a PTY; the Agent SDK; logging deltas |
| 3 | Files: entries at `~/.local/state/rusty/agents/<id>.json` (start writes, the host rewrites atomically), the log at `<id>/events.jsonl` (the host alone appends), sockets at `$XDG_RUNTIME_DIR/rusty/agents/<id>.sock`; `RUSTY_AGENT_STATE_DIR` and `RUSTY_AGENT_RUN_DIR` override; nothing under `~/.rusty`, nothing in the store; `workspace.json`'s `agentSessions` keeps its shape with Rusty ids as values | Files are the truth; the app touches nothing under `~/.rusty`; a flat entry beside its log directory lets `Agents` watch the directory without the appends; an old Claude id in `agentSessions` fails `exists` and becomes `resume`, so no migration | a table in the store; state under `~/.config/rusty` (workspace state, not runtime state) |
| 4 | `Assistant` keeps its bridge and becomes a socket client, one instance per pane (RightPane declares its own), created on the first message, attached when the page has a session, detached on a page switch; the widened signals (`permissionAsked` with a meta JSON, `turnDone` with a duration, `thinkingDelta`/`thinkingFinal`, `userMessage`, `answered`, `expired`, `created`, `attached`, `replayDone`, `hostStarted`, `hostExited`) and `answer(…, extraJson)` are the contract TICKET-032 builds on | The pane keeps working through the same signals; opening ten pages must not start ten hosts; a JSON envelope keeps later fields from changing signatures again | one `Assistant` per window; a Rust-side item model (later, if the tab needs it) |
| 5 | An idle timeout per session (the pane 600 s, the CLI 0) stops the child gracefully and the next message respawns it with `--resume` | A node process per idle page is the cost of persistence; bounding it keeps the box's memory for Chad's work | never stopping; stopping the host too |
| 6 | The host is a verb of the app binary (`rusty agent host`), on a current-thread tokio runtime with `net`, `io-util`, `process`, `signal`; `uuid` mints ids; `libc::kill` sends the one SIGTERM | `AD-rusty-commands-are-nouns-and-verbs-001`; tokio already links; every crate is in the workspace lock | a `rusty-agent` crate (the fallback if the Qt-linked host's memory is unreasonable) |
| 7 | `AD-rusty-agents-are-terminals-001` is qualified again: the terminal tabs stay tmux terminals; the pane (and the tab to come) is a client of a host that owns a headless Claude Code | The pane's conversation now outlives the app, which the terminals were the only ones to do | reversing the decision |

## Linked artifacts

- Ticket: `docs/planning/tickets/open/TICKET-031-agent-sessions.md`
- Intake: none; the plan approved on 2026-09-17 (`~/.claude/plans/federated-foraging-liskov.md` on the dev box)
- Design references: `docs/planning/pipeline/completed/native-agent-pane.spec.md` (the
  wire, the pane), `session-resilience.spec.md` (the units), `session-commands.spec.md`
  (the noun); Claude Code's docs on print mode, streaming input, sessions and background
  sessions (read 2026-09-17)
- Architecture: `docs/architecture.md` (the app's objects, the session-bound units),
  `openwiki/workspace-app.md`, `openwiki/development-and-validation.md`; register
  `AD-rusty-pane-agent-is-headless-claude-001`, `AD-rusty-app-as-session-service-001`,
  `PR-rusty-systemctl-stand-in-001`, `PR-rusty-probe-kills-from-outside-001`,
  `PR-rusty-user-oom-floor-001`, `PR-rusty-restart-always-001`, `PR-rusty-workspace-state-in-json-001`

## Phase plan

| Phase | Deliverable | Exit gate |
|---|---|---|
| 1 Plan | Tickets 031–034, spec, notes, open AAR | scope settled; sealed by the plan approval |
| 2 Design | The probe on 2.1.274, the manifest, the protocol, the regression table, CodeGraph over `assistant`, `session`, `terminals` | design actionable |
| 3 Implement | `src/agent/*`, `src/diff.rs`, `assistant.rs`, `session.rs`, `main.rs`, `terminals.rs`, `build.rs`, `Cargo.toml`, `RightPane.qml`, `Main.qml`, `scripts/screenshot.sh`, `scripts/probe-claude-wire.sh` | `bin/gate.sh --fast` green |
| 3.5 Inspect | Finding ledger; CodeGraph over the new modules | confirmed findings resolved |
| 4 Validate | The tests, a probe session under a real unit, the scenes, `--diff` green | receipt matches worktree |
| 5 Complete | Audit, wiki, architecture, operator docs, AAR, register, brain, archive | pair archived |
