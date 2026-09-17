---
title: TICKET-033-composer-extras
status: open
ticket_number: 033
type: feature
created: 2026-09-17
intake:
pipeline_spec: TBC
---

# TICKET-033-composer-extras

## Summary

The Agent tab's second layer: slash commands from the session's command list and the
store's skills, `@` file mentions from the explorer and the vault, image paste as a base64
block, rename and fork (`--fork-session`), a context-usage meter, subagent nesting under
the Agent card, and "open in terminal" (a tmux tab on `claude --resume <id>` after the host
stops its process).

## Why

The things VS Code's panel does beyond the transcript, each a small addition once
TICKET-032's surface stands.

## EARS requirements

| ID | Requirement | Verification |
|---|---|---|
| REQ-001 | WHEN `/` starts the composer, a popup shall list the commands the session supports and insert the chosen one. | scene; probe of which built-ins print mode runs |
| REQ-002 | WHEN `@` is typed, a popup shall offer explorer paths and pages and insert the path. | scene |
| REQ-003 | WHEN an image is pasted, the send shall carry it as a base64 image block. | wire test |
| REQ-004 | WHEN a session is forked, a new session with the same history shall appear beside the original. | host test of `--fork-session` |
| REQ-005 | WHEN "open in terminal" is chosen, the host shall stop its process before the terminal resumes the transcript, and the registry shall refuse a second live writer. | host test |

## Scope

- In: the items above.
- Out: anything that needs a new wire from Claude Code.

## Notes

- Pipeline spec: TBC
- Related docs: TICKET-031, TICKET-032
