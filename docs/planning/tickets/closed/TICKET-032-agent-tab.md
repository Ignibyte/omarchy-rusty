---
title: TICKET-032-agent-tab
status: done
ticket_number: 032
type: feature
created: 2026-09-17
intake:
pipeline_spec: docs/planning/pipeline/completed/agent-tab.spec.md
---

# TICKET-032-agent-tab

## Summary

A VS Code-style Claude Code surface as a tab kind `agent` on the session host of
TICKET-031: a transcript of cards (streamed text rendered as markdown at the end, thinking
folded, tool calls by tool, an Edit as a line diff, permissions with Allow once / Allow
always / Deny with a reason, `AskUserQuestion` as chips, `ExitPlanMode` as a plan to
approve), a composer with mode and model chips, a sessions pane in the left sidebar, and
the Claude glyph's click opening the tab (the terminal moves into the menus).

## Why

Chad's request of 2026-09-17: run Claude Code through the app with an interface like
VS Code's panel. The pane of TICKET-025 renders plain items; the tab is the full surface,
shared with the pane through one `AgentTranscript` and one `AgentComposer`.

## EARS requirements

| ID | Requirement | Verification |
|---|---|---|
| REQ-001 | WHEN the Claude glyph is clicked, the app shall open an Agent tab in the current folder root without starting a process until the first message. | scene `agent:tab`; reading |
| REQ-002 | WHEN the assistant streams, the transcript shall show the text as it arrives and render it as markdown when the block ends. | scene; reading of the render pipeline |
| REQ-003 | WHEN a tool call arrives, the transcript shall show a card by tool, an Edit as a line diff, and land the result in the same card. | scenes `run the tests`, `fix the typo` |
| REQ-004 | WHEN a permission, a question or a plan is asked, the card shall answer with the keyboard (Y, A, N, Shift+N, chips, Enter). | scenes; hotkeys table |
| REQ-005 | WHEN the mode or model chip is changed, the session shall change it live. | wire test of the requests; smoke |
| REQ-006 | WHEN a hidden Agent tab needs input or finishes, the tab shall show a dot and the desktop a notification. | reading of the `termComp` wiring reused |
| REQ-007 | WHEN the host is dead or the back end is down, the tab shall say so and keep the log readable. | scene `agent:detached` |

## Scope

- In: `AgentPage`, `AgentTranscript`, `AgentComposer`, the cards, `AgentsPane`, the
  entry points, the keys and palette entries, the scenes, the fake `claude` cases.
- Out: slash commands, `@` mentions, image paste, fork, context meter, subagent nesting
  (TICKET-033).

## Notes

- Pipeline spec: TBC
- Related docs: the plan of 2026-09-17 (`~/.claude/plans/federated-foraging-liskov.md`
  on the dev box), TICKET-031
- Follow-ups opened: none yet
