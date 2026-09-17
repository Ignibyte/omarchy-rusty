---
title: The Agent tab
pipeline_id: 099469a7-06e4-446c-b9f7-e046aaa1f990
status: Phase 5 — Complete PASS
ticket: TICKET-032
ticket_doc: docs/planning/tickets/closed/TICKET-032-agent-tab.md
aar: docs/planning/knowledge/aar/AAR-032-agent-tab.md
sealed: Chad, 2026-09-17, approving the plan — a new tab kind whose surface is the VS Code-style transcript, with the Claude glyph's click opening it ("Click opens the chat tab (Recommended)"); the plan's sessions list as a fourth left-sidebar pane rather than a column inside each tab, with the alternative recorded
created: 2026-09-17
---

# The Agent tab: spec

## Intent

TICKET-031 made a conversation outlive the app; this is the surface Chad asked for on top
of it. The note pane renders plain items: text without markdown, a tool call as its input
JSON, a permission as a line with two buttons. The tab is the full reading of a session —
a transcript of cards, an edit shown as a diff, a question answered with the keyboard, a
composer that says which mode and model are in force — and the pane keeps up by sharing
its components rather than growing a second implementation.

## Scope

- In: an `agent` tab kind (`AgentPage`) holding one `Assistant`; `AgentTranscript` and
  its cards (user, assistant text rendered as markdown when a block ends, thinking folded,
  tool calls by tool with an `Edit` as a line diff, permissions with Allow once, Allow
  always and Deny with a reason, `AskUserQuestion` as chips, `ExitPlanMode` as a plan to
  approve, a result footer, notices); `AgentComposer` (Enter sends, Shift+Enter breaks a
  line, Escape interrupts, Up recalls, the mode and model chips, Start and Reconnect);
  `AgentsPane`, a fourth left-sidebar pane listing the machine's sessions; the entry
  points (the Claude glyph's click, the `+` menu, the explorer's folder menu, a ribbon
  button, `Ctrl+Shift+A`); the palette entries, the shortcuts and the Settings hotkeys
  rows; the tab's persistence and its reattach on restart; the unread dot and the
  notification for a hidden tab; the pane rebuilt on the shared components; the scenes.
- Out (named seams, not forgotten): slash commands, `@` mentions, image paste, fork,
  the context meter, subagent nesting, "open in terminal" (TICKET-033); a model picker
  that lists what the account has (the four aliases and the session's own are enough);
  editing a message after it is sent; searching a transcript.

## Acceptance criteria (EARS)

| ID | Requirement | Verification |
|---|---|---|
| REQ-001 | WHEN the Claude glyph is clicked, or `Ctrl+Shift+A` is pressed, the app shall open an Agent tab for the current folder root without starting a process until the first message. | the `agent:tab` scene (a tab with no session); reading of `openAgent` |
| REQ-002 | WHEN the assistant answers, the transcript shall stream the text as it arrives and render it as markdown once the block is complete. | the `agent:tab:ask:` scenes; reading of the render round trip |
| REQ-003 | WHEN a tool is called, the transcript shall show a card chosen by the tool, render an `Edit` as a line diff, and put the tool's result in that same card. | scenes for a Bash call and an Edit |
| REQ-004 | WHEN a permission, a question or a plan is asked, the card shall be answerable from the keyboard alone (Y, A, N, Shift+N; the chips; Enter) and the answer shall reach the session. | scenes with a pending card; the hotkeys table; reading |
| REQ-005 | WHEN the mode or model chip is used, the session shall change it and the strip shall show what is in force. | reading of `changeMode`/`changeModel` against the wire tests of TICKET-031; a scene |
| REQ-006 | WHEN an Agent tab is not the current tab and its session needs input or finishes, the tab shall mark itself unread and the desktop shall be notified. | reading of the `termComp` wiring reused; `updateCurrent` clearing it |
| REQ-007 | WHEN the session's host is not running, or the back end is down, the tab shall say so and keep the conversation readable. | the `agent:detached` scene; reading |
| REQ-008 | WHEN the app restarts, an Agent tab shall reattach to its session and show the conversation. | the scenes run twice over one state, as TICKET-031's were |
| REQ-009 | WHEN the pane beside a note renders a conversation, it shall use the same transcript and composer as the tab. | reading; the `right:agent` scenes still pass |
| REQ-010 | WHEN a QML file is added, the build shall carry it. | a test that every `qml/*.qml` is listed in `build.rs` |

## Locked decisions

| # | Decision | Why | Alternatives set aside |
|---|---|---|---|
| 1 | The sessions list is a fourth left-sidebar pane (`AgentsPane`), not a column inside every Agent tab | Tabs already are the conversations, and the workspace puts navigation in the sidebar (`AD-rusty-workspace-is-obsidian-001`); one workspace-wide list costs no splitter or layout key per tab, and the tab strip already marks the sessions that are open | a 220 px column with its own splitter and `ui.agentLayout`, the original sketch; it stays possible and the plan records it |
| 2 | One `AgentTranscript` and one `AgentComposer`, used by the tab and by the pane (`compact: true`) | The pane's mechanics are the proven ones and the tab needs all of them; two implementations would drift within a ticket | a richer tab and a plain pane |
| 3 | A row is a fixed shape written by one `push`, and the delegate is a `Loader` over per-kind cards | A `ListModel` fixes its roles at the first append, so one writer keeps them from drifting; a card per kind keeps each one readable | a role per card kind; a single delegate with branches |
| 4 | Streamed text is plain and becomes rich only when the block ends, rendered on demand by the delegate through `brain_render` | Rendering every delta would re-lay out the whole message each frame; on demand means a replayed conversation renders only what is scrolled into view | rendering in Rust; rendering every delta |
| 5 | Deltas are batched into one model write per frame by a short timer | The append itself is cheap but each write re-lays out a `TextEdit`; TICKET-025's inspect recorded the copy per delta as accepted-not-fixed, and the tab makes it visible | one write per delta |
| 6 | Following the tail is the user's: the view follows until they scroll away, and a pill brings them back | A transcript that drags the view back while it is being read is worse than one that does not follow at all | always follow; never follow |
| 7 | The composer's Enter sends, Shift+Enter breaks a line, Escape interrupts a running turn, Shift+Tab cycles the mode | The pane's keys, plus the CLI's own mode key, so the two surfaces and the terminal agree | a Send button alone |

## Linked artifacts

- Ticket: `docs/planning/tickets/open/TICKET-032-agent-tab.md`
- Depends on: TICKET-031 (`pipeline/completed/agent-sessions.spec.md`), whose client
  signals, `Agents` registry and `Assistant.diff` this ticket renders
- Design references: the plan approved on 2026-09-17
  (`~/.claude/plans/federated-foraging-liskov.md` on the dev box);
  `docs/design/rusty-omarchy.html` for the chrome
- Architecture: `openwiki/workspace-app.md`; register
  `AD-rusty-workspace-is-obsidian-001`, `AD-rusty-one-splitter-owner-clamps-001`,
  `PR-rusty-qml-component-scope-001`, `PR-rusty-qml-signal-names-001`,
  `PR-rusty-signals-through-connections-001`, `BF-rusty-moving-frame-delta-001`

## Phase plan

| Phase | Deliverable | Exit gate |
|---|---|---|
| 1 Plan | Spec, notes, open AAR | scope settled; sealed by the plan approval |
| 2 Design | The card inventory, the row shape, the render path, the file manifest, the regression table, CodeGraph | design actionable |
| 3 Implement | The QML files, the Main.qml touch points, the pane on the shared components, the scenes | `bin/gate.sh --fast` green |
| 3.5 Inspect | Finding ledger; CodeGraph over the changed surfaces | confirmed findings resolved |
| 4 Validate | Tests, the scenes photographed in two skins, `--diff` green | receipt matches worktree |
| 5 Complete | Audit, wiki, docs, AAR, register, brain, archive | pair archived |
