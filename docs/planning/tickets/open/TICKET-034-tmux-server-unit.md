---
title: TICKET-034-tmux-server-unit
status: open
ticket_number: 034
type: fix
created: 2026-09-17
intake:
pipeline_spec: TBC
---

# TICKET-034-tmux-server-unit

## Summary

Start the terminal tabs' tmux server through `systemd-run --user` (its own unit,
`exit-empty off`) before the first `new-session -A`, so a stop or a crash restart of
`rusty-app.service` no longer ends every terminal session.

## Why

Found on 2026-09-17 while planning TICKET-031: the tmux server the tabs attach to
(pid 4160, `tmux: server`, ppid 1098) lives in `rusty-app.service`'s cgroup
(`/user.slice/user-1000.slice/user@1000.service/app.slice/app-graphical.slice/rusty-app.service`,
`KillMode=control-group`). A unit stop, or the restart after a crash, kills the server;
tmux 3.7c scopes each pane's shell as `tmux-spawn-<uuid>.scope`, so the shells outlive it
by a SIGHUP only. The power glyph's promise, "terminals keep running in tmux", holds for a
quit through Qt (the unit stays down but the cgroup is cleaned) only when the server was
started outside the app, which it never is.

## EARS requirements

| ID | Requirement | Verification |
|---|---|---|
| REQ-001 | WHEN a terminal tab starts its first session, the tmux server shall run in its own user unit outside `rusty-app.service`'s cgroup. | `/proc/<server pid>/cgroup`; a stand-in `systemd-run` test |
| REQ-002 | WHEN the app unit stops or restarts, every tmux session shall survive and the tabs shall reattach. | smoke on Chad's word, on his terminals |

## Scope

- In: `terminals.rs` (the server's unit), `AgentTerminal.qml`, the docs' claim.
- Out: the agent host (TICKET-031 starts its own units).

## Notes

- Pipeline spec: TBC
- Related docs: `docs/planning/knowledge/aar/AAR-009-session-resilience.md`
