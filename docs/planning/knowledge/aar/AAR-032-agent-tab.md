---
title: AAR-032-agent-tab
pipeline_id: 099469a7-06e4-446c-b9f7-e046aaa1f990
ticket: TICKET-032
status: closed
created: 2026-09-17
submitted: 2026-09-17
---

# AAR-032: The Agent tab

## Recall log

- The client contract is TICKET-031's, delivered an hour earlier in the same session: the
  signals, the invokables and the `Agents` registry are all in place, so this ticket adds
  no Rust beyond what QML needs.
- `AD-rusty-workspace-is-obsidian-001` put the sessions list in the sidebar rather than in
  each tab; the QML scope and signal-name rules (both of which bit in TICKET-031) shaped
  the naming before a line was written.
- TICKET-025's AAR carried the two facts this surface rests on: a `ListView` needs
  `contentHeightChanged` to follow streamed text, and a delta that copies the whole string
  is the cost the tab would make visible.

## 1. Outcomes

- `REQ-001` PASS — the `tab:agent` scene opens a tab for the folder with no process behind
  it; `agentCwd` reading.
- `REQ-002` PASS — the answer streams and the plan scene proves the render: a heading, a
  numbered list and a closing line came back through `brain_render`.
- `REQ-003` PASS — the Bash card (command, description, output collapsed to "↳ 4 lines")
  and the Edit card (the diff with its gutter characters).
- `REQ-004` PASS — the pending cards in three scenes, the chips in the question scene, and
  the ten rows the hotkeys table gained.
- `REQ-005` PASS — the mode and model chips read `permissionMode` and `model` off the
  client in every tab scene; the requests behind them are TICKET-031's tested wire.
- `REQ-006` PASS — `agentComp` wires `onUnread` and `onAttention` to the same
  `markUnread`/`attention` the terminals use, and `updateCurrent` clears the dot.
- `REQ-007` PASS — the composer's Start and Reconnect, the not-installed text, and
  `renderRow` returning while the back end is down.
- `REQ-008` PASS — the tab keeps its session in the tabs file and attaches on completion,
  the way the pane's replay was proved at TICKET-031.
- `REQ-009` PASS — the `right:agent` scene renders on the shared components.
- `REQ-010` PASS — `theme::tests::every_qml_file_is_in_the_module`.

`GATE GREEN [diff]`; `openwiki_finish` complete after a second run.

## 2. What went well

- The client contract held. TICKET-031 was designed with this ticket's needs in the
  signals — the permission meta, the duration, `userMessage`, `expired`, `diff` — and not
  one of them had to change while building the surface. The whole tab is QML plus a single
  Rust test.
- One `push` and one row shape stopped the model drifting as nine card kinds appeared;
  joining a call, its permission and its result by the tool-use id is what makes a decision
  readable, and it fell out of the shape rather than being bolted on.
- Sharing the transcript and the composer with the pane cost nothing at the time and
  immediately improved the pane: it gained markdown, diffs, "Allow always" and the mode and
  model chips without a line written for it.
- The scenes caught two defects reading would not have: `selectTab` (a method I invented)
  left the tab in the strip and the note on screen, and two tools logged a `TypeError` per
  card. Photographing new chrome, as TICKET-022's lesson says, keeps paying.

## 3. What went poorly

- I called `win.selectTab` without checking the window had one; every other opener writes
  `stack.currentIndex`. Reading the file I was editing would have been quicker than the
  scene that caught it.
- Three lookups for a pending row and a ternary whose arms were identical went in while I
  was moving fast; inspect took them out. The first draft of a surface accretes that.
- The Shift+N key set a property the card used for something else, so it did nothing
  visible. A key is not wired until the thing it should open has opened.
- The thinking line counted the words of a token estimate ("Thought · 4 words"), which is
  the kind of small lie a surface tells when a label is written before the data is known.

## 4. Surprises

- Claude Code's thinking does not travel: the deltas carry an empty string and only the
  token estimate is real. The line had to say what it knows rather than pretend to a
  transcript.
- `AskUserQuestion` and `ExitPlanMode` arrive as permission requests, so one card kind and
  one answer path covers them; the plan is the input, and approving it means allowing with
  it unchanged.
- The scene that answers a permission has to wait for the permission: the first version
  fired in the same tick as the message and photographed a card nobody had answered.

## 5. Lessons

- `PR-rusty-one-writer-one-row-shape-001`: a `ListModel` fixes its roles at the first
  append, so one `push` writes every row and a card reads what it needs; nine kinds shared
  one shape without drift.
- `PR-rusty-render-at-the-end-of-a-block-001`: rich text is laid out on every set, so a
  streamed answer stays plain until its block ends and is rendered by the card as it is
  built — a replay then renders what is read, not all of it.
- `AD-rusty-agent-tab-is-a-client-surface-001`: the decision itself.
- Standing: photograph new chrome (TICKET-022), and read the file you are editing before
  calling a method you remember rather than one you have seen.

## 6. Time spent

| Phase | Estimated | Actual |
|---|---|---|
| 1 Plan | 0.5 h | 0.5 h |
| 2 Design | 0.5 h | 0.5 h (the client contract was already the design) |
| 3 Implement | 2 h | 2 h (nine QML files, the window, the pane, the fake's four cases) |
| 3.5 Inspect | 0.5 h | 0.75 h (twelve entries, ten fixed) |
| 4 Validate | 0.5 h | 0.75 h (seven scenes, two gate reds) |
| 5 Complete | 0.5 h | 0.75 h (two wiki runs) |
