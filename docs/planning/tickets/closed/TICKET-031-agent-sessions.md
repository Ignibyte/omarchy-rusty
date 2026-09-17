---
title: TICKET-031-agent-sessions
status: done
ticket_number: 031
type: feature
created: 2026-09-17
intake:
pipeline_spec: docs/planning/pipeline/completed/agent-sessions.spec.md
---

# TICKET-031-agent-sessions

## Summary

Move the agent pane's Claude Code process out of the app and under a Rusty session host
that runs as a transient systemd user unit per session, keeps the process's pipes, logs
every event and serves them over a Unix socket. The app becomes a reconnectable view: a
turn keeps running when a page is switched or the app quits, and a reopened pane replays
the conversation. `rusty agent start|stop|list|attach|rm` is the same client from a
terminal. First of four tickets toward a VS Code-style Claude Code surface (TICKET-032,
TICKET-033) and the tmux server's own unit (TICKET-034).

## Why

Chad asked (2026-09-17) for Claude Code inside Rusty through an interface like VS Code's,
on the premise of a persistent process the front end attaches to. Today's pane (TICKET-025)
speaks the right wire but owns the process: one `Assistant` per window, restarted on every
page switch (a running turn dies), gone when the app quits, and never replayed. Found the
same day: the app runs as `rusty-app.service` and the tmux server the terminal tabs use
(pid 4160) sits in that unit's cgroup, so a unit stop or crash restart kills it. Whatever
hosts the agent must sit outside the app's cgroup, which a `systemd-run --user` transient
unit does.

## EARS requirements

| ID | Requirement | Verification |
|---|---|---|
| REQ-001 | WHEN a conversation is started from the pane or from `rusty agent start`, the system shall run Claude Code under a transient user unit `rusty-agent-<id>` outside the app's cgroup, owned by a host that keeps its stdin and stdout. | test of `systemd_run_args`; `/proc/<pid>/cgroup` of a probe session |
| REQ-002 | WHEN the app detaches, quits or restarts while a turn runs, the host and the process shall keep running and the turn shall finish. | host test (a client detaches, the answer still lands in the log); a probe session across an app quit |
| REQ-003 | WHEN a client attaches with a sequence number, the host shall replay the logged events after it, mark the catch-up, then stream live events. | host test; the pane scene reopened over the same state |
| REQ-004 | WHEN the process exits (an error, the idle stop, a kill), the host shall record the exit and respawn it with `--resume` on the next message; a resume the transcript cannot satisfy shall start a fresh session and say so. | host tests: respawn, stale resume |
| REQ-005 | WHEN the agent asks a permission or a turn ends while no client is attached, the host shall raise a desktop notification. | host test with a recording notifier |
| REQ-006 | WHEN a stop is requested or the unit gets SIGTERM, the host shall interrupt a running turn, close stdin, then signal the process, in that order, exit 0 and leave the entry stopped. | host stop tests; `systemctl --user kill -s TERM` on a probe session |
| REQ-007 | WHEN `rusty agent <verb>` runs, the binary shall answer before Qt starts as the `session` noun does; an unknown verb shall print the usage and exit 2. | parser and usage tests |
| REQ-008 | WHEN the pane opens a page that has a session, it shall attach without starting a process; the first message on a page without one shall create the session; switching pages shall detach without ending a turn. | reading of `RightPane.qml`; the scenes; smoke by Chad |
| REQ-009 | WHEN Claude Code emits thinking, permission metadata (tool use id, suggestions), durations, a compaction or a retry, the parser shall carry them, and an answer shall be able to allow always or deny with a reason. | wire tests on 2.1.274 fixtures |
| REQ-010 | WHEN `Assistant.diff(old, new)` is called, it shall answer line rows of kind same, add or del. | diff tests |

## Scope

- In: the `agent` noun and its verbs; the host (spawn, log, socket, replay, respawn,
  idle stop, stop sequence, notifications); the protocol; the registry entries and the
  `Agents` type; `Assistant` as a socket client with the widened signals; the pane on the
  host; the stand-in `systemd-run` for tests and scenes; the wire probe on claude 2.1.274;
  `Assistant.diff`.
- Out: the Agent tab and its cards (TICKET-032); slash commands, mentions, images, fork,
  a context meter (TICKET-033); the tmux server's unit (TICKET-034); log compaction; a
  Qt-free host crate (a fallback if the host's memory says so); Codex.

## Notes

- Pipeline spec: `docs/planning/pipeline/active/agent-sessions.spec.md`
- Related docs: `docs/planning/pipeline/completed/native-agent-pane.spec.md`,
  `session-resilience.spec.md`, `session-commands.spec.md`; `openwiki/workspace-app.md`
- Promoted from intake: none; Chad's plan approval of 2026-09-17
- Follow-ups opened: TICKET-032, TICKET-033, TICKET-034
