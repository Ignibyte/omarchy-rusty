---
title: The Agent tab: notes
pipeline_id: 099469a7-06e4-446c-b9f7-e046aaa1f990
---

# The Agent tab: running notes

Chronological evidence and decisions. If a command did not run, these notes do not say it
passed.

## Phase 1: Plan

- Recall (2026-09-17, straight after TICKET-031 in the same session):
  - Bulletins: three notices, none critical; the second still binds (no synthetic input on
    Chad's desktop — the surface is proved by offscreen scenes and by reading).
  - The client contract this renders is TICKET-031's: `created`, `attached`, `replayDone`,
    `started`, `blockStarted`, `textDelta`, `textFinal`, `thinkingTokens`, `userMessage`,
    `toolInput`, `toolResult`, `permissionAsked` (with a meta JSON carrying the tool use
    id, the display name and the suggestions), `answered`, `expired`, `turnDone` (with a
    duration), `modeChanged`, `notice`, `exited` (with a reason), `hostStarted`,
    `hostExited`; the invokables `create`, `attach`, `detach`, `send`, `answer`,
    `interrupt`, `changeMode`, `changeModel`, `stop`, `remove`, `diff`; and the `Agents`
    registry with `sessions`, `refresh`, `exists`, `alive`, `watch`.
  - Register: `AD-rusty-workspace-is-obsidian-001` (navigation in the sidebar, documents
    in tabs) decided the sessions list; `PR-rusty-qml-component-scope-001` and
    `PR-rusty-qml-signal-names-001` (both bit again in TICKET-031) decide the naming;
    `PR-rusty-signals-through-connections-001` for anything third-party;
    `AD-rusty-one-splitter-owner-clamps-001` if a split appears;
    `BF-rusty-moving-frame-delta-001` for any drag.
  - TICKET-025's AAR: a `ListView` follows new rows on `countChanged` but not text growing
    inside the last one, which needs `contentHeightChanged` — and its F10 (a delta copying
    the whole string) is what decision 5 answers.
  - TICKET-022's AAR: photograph new chrome before calling it done; reading caught two of
    its three defects and the third only showed in a scene.
  - The QML to model on: `BookmarksPane.qml` for the sessions pane, `FileTab.qml` for the
    `brain_render` round trip and its `style()`, `NoteTab.qml` for the same style at the
    reading size, `TasksPage.qml` for a focus ring on a list row, `SkillsPage.qml` for a
    per-page JSON layout key, the `TabHost` `Loader` chain and the `Component`s beside it
    in `Main.qml` for a new tab kind.
- Decisions: the seven locked in the spec. The one that departs from the plan Chad
  approved is decision 1 (the sessions list as a sidebar pane rather than a column in
  every tab); the plan recorded it as the recommendation with the column as the named
  alternative, so it is taken here and stays reversible.

## Phase 2: Design

- Architecture and data flow: no Rust is added. The tab is QML over TICKET-031's client:
  `AgentPage` holds one `Assistant`, `AgentTranscript` turns its signals into rows, and
  `AgentComposer` writes back. A row is one fixed shape written by one `push`
  (`uid, kind, name, text, html, extra, input, meta, result, isError, answered, state,
  expanded`), because a `ListModel` fixes its roles at the first append; the delegate is a
  `Loader` over three cards (`AgentTextCard` for prose, `AgentToolCard` for a call,
  `AgentQuestionCard` for a question or a plan), with `AgentDiff` and `AgentDecision`
  inside the tool card. `extra` is the tool-use id, so a call, its permission and its
  result are one card; `meta` carries the request id, the display name, the suggestions
  and an `Edit`'s diff rows. Markdown comes from `brain_render`, asked for by the card
  when its block is complete and matched back by `uid`. `AgentsPane` reads the `Agents`
  registry. The pane beside a note is the same two components with `compact: true`.
- File manifest: nine QML files (`AgentPage`, `AgentTranscript`, `AgentComposer`,
  `AgentTextCard`, `AgentToolCard`, `AgentQuestionCard`, `AgentDiff`, `AgentDecision`,
  `AgentsPane`), all listed in `build.rs`; `Main.qml` (the tab kind, its component, the
  open/`agentCwd`/`currentAgent` helpers, the sidebar pane, the ribbon button, the glyph's
  click, three shortcuts, seven palette entries, the scenes); `RightPane.qml` (the pane on
  the shared components, about 110 lines lighter); `SettingsPage.qml` (the card and
  composer keys in the hotkeys table); `theme.rs` (the build-manifest test);
  `scripts/screenshot.sh` (four cases in the fake `claude`).
- Store consequences: none. The tab reads `brain_render` like every other rendered
  surface; nothing else touches the back end, and no state leaves `workspace.json` beyond
  one new key (`agentPrefs`).
- Tool contract: unchanged. `brain_render` is called with `slug: ""` and a `markdown`
  parameter, as the file tab does.
- Regression plan:
  | Requirement | Evidence |
  |---|---|
  | REQ-001 | the `tab:agent` scene; `openAgent` reading |
  | REQ-002 | the `plan it` scene (a plan rendered through `brain_render`); the streamed answer in the allow scene |
  | REQ-003 | the `run the tests` scene (a Bash card with its command and collapsed output) and `fix the typo` (an Edit as a diff) |
  | REQ-004 | the `which one` scene (chips) and the pending cards in the others; the hotkeys table |
  | REQ-005 | the mode and model chips in every tab scene; the wire tests of TICKET-031 behind them |
  | REQ-006 | reading of the `termComp` wiring reused in `agentComp`; `updateCurrent` |
  | REQ-007 | the composer's Start/Reconnect and the not-installed text |
  | REQ-008 | the scenes run over one scratch state, as TICKET-031's replay was proved |
  | REQ-009 | the `right:agent` scene on the shared components |
  | REQ-010 | `theme::tests::every_qml_file_is_in_the_module` |
- Risks: a long replay building many rows (cards are lazy, rendering is on demand, deltas
  are batched); a rich-text card re-laying out on every set (only at the end of a block,
  and only under 24k characters); focus fights between a pending card and the composer
  (one owner: the transcript focuses a card when a request arrives and hands focus back
  after the answer); the Amber phosphor skin (the diff's meaning is in its gutter
  character, not its colour).
- CodeGraph evidence: the change is QML with one Rust test; `codegraph_explore` over
  `Assistant`, `translate` and the theme tests at TICKET-031 already recorded the blast
  radius, and nothing in this ticket adds a Rust caller.

## Phase 3: Implement

- Built (2026-09-17): the nine QML files, the window's touch points, the pane rebuilt on
  the shared components, the hotkeys rows, the build-manifest test, and four cases in the
  screenshot fake (a shell call with a suggestion, an edit, two questions, a plan) plus
  the `tab:agent`, `tab:agent:ask:` and `agent:answer:` scenes.
- Deviations from the manifest: none in the file list. Two behaviours were added while
  building because the surface asked for them: the composer counts the messages queued
  while a turn runs (Claude Code queues them; the footer says how many), and the scene's
  answer waits for the question to arrive rather than firing blind.
- Fast gate: `bin/gate.sh --fast` → `GATE GREEN [fast]`, after one clippy fix (a doc
  comment left between two `#[test]` attributes).

## Phase 3.5: Inspect ledger

| # | Lens | Finding | Severity | Disposition |
|---|---|---|---|---|
| 1 | correctness | `AgentToolCard` read `.length` on fields a tool does not take (`Read` has no `command`, `Bash` no `file_path`), so every card of those tools logged a `TypeError` twice. | medium | Fixed: `base` and `lineCount` take `undefined` as empty. The scene logs are clean. |
| 2 | correctness | `win.selectTab` does not exist: `openAgent` threw and the new tab was never shown, which the first scene caught (the tab was in the strip, the note still on screen). | medium | Fixed: `stack.currentIndex`, as every other opener uses. |
| 3 | complexity | Three lookups for a pending row (`byRequest`, `requestRow`, `settle`) where one is needed, and a branch whose two arms were both `"done"`. | low | Fixed: the two dead functions are gone and the branch reads as what it does. |
| 4 | complexity | `AgentTranscript` emitted `needsInput` that nobody connected, and `AgentTextCard` kept a `words` count after the thinking line stopped using it. | low | Fixed: both removed. |
| 5 | keyboard | Shift+N on a card set `expanded`, which the card used for its output rather than for the reason field, so the key did nothing visible on a waiting call. | medium | Fixed: a waiting call has no output, so its expansion is what opens the reason field (`askingReason: card.pending && card.expanded`), and the hotkeys table says so. |
| 6 | keyboard | The card and composer keys are not window shortcuts, so the palette does not list them and the Settings table would not have shown them. | medium | Fixed: an `agentKeys` array beside `terminalKeys`, ten rows. |
| 7 | correctness | A scene's `agent:answer:allow` fired before the permission it meant to answer had arrived, so the answered-state shot showed a pending card. | low | Fixed: the scene retries every 150 ms for three seconds and stops when it has answered, which is what the shot then shows. |
| 8 | honesty of a surface | The thinking line said "Thought · 4 words" over a token estimate, counting the words of the estimate itself. | low | Fixed: it reads "Thought · 180 tokens", and the card no longer offers to expand text that does not travel. |
| 9 | reuse | `AgentPage` required a `terminals` property it never used (the window raises the notification through `attention`). | low | Fixed: removed from the page and from the window's binding. |
| 10 | theme | The chevrons were literal "▸"/"▾", which the interface font draws as a dot. | low | Fixed: the app's own `Icon` (`chevron-right`, `chevron-down`), as the explorer uses. |
| 11 | keyboard, empty states | Every state was re-read: no `claude` or no `systemd-run` (the composer says so and hides its field), a tab with no session (the empty line says what it will cost), a detached session (Start and Reconnect), a failed turn (the red footer), the back end down (`renderRow` returns and the text stays plain). | — | No finding. |
| 12 | data safety | Nothing new is written: the tab holds no store, the render call is the one the file tab makes, and the only new state is `agentPrefs` in the workspace JSON. | — | No finding. |

| 13 | portability (found by CI after delivery, 2026-09-17) | `AgentToolCard` had `readonly property string short`. `short` is a reserved word: Qt 6.11's `qmlcachegen` on this box compiled it, and the runner's Qt refused it — "Expected token `identifier'" — so the whole build failed there while every local gate was green. | high | Fixed: the property is `toolLabel`, and `theme::tests::qml_property_names_avoid_reserved_words` scans every QML property name against the reserved and future-reserved words, so the next card cannot repeat it on a Qt that happens to be lenient. The test was proved by putting the old name back: it failed, naming the file and line. |
- Post-implementation CodeGraph: the Rust surface is unchanged but for one test, so the
  blast radius recorded at TICKET-031 stands; the QML was read directly, which is what the
  constitution asks for QML.

## Phase 4: Validate

- Tests run: `cargo test -p rusty-app` → `test result: ok. 93 passed; 0 failed` (the 92 of
  TICKET-031 plus `every_qml_file_is_in_the_module`, which is REQ-010 and which would have
  caught a new card left out of the module).
- Gate run: `bin/gate.sh --diff` → `GATE GREEN [diff]`, receipt written. Two earlier runs
  were red: clippy on a doc comment between two `#[test]` attributes, and the whitespace
  check on one trailing space in `AgentsPane.qml`. Both fixed at the source.
- Smoke evidence (offscreen, against the scratch vault and the fake `claude`; Chad's
  desktop untouched):
  - `tab:agent,tab:agent:ask:run the tests` — the Agent tab opens for the folder, the
    question is a bubble, the thinking is one line ("Thought · 180 tokens"), the Bash card
    carries its command and its description, and the decision offers Allow, Allow always
    (the suggestion was there), Deny and a reason, with the focus ring on the card and
    "needs input · Ctrl+." in the strip.
  - `…,agent:answer:allow` — the same turn answered: the card stamped "Allowed", its
    output collapsed to "↳ 4 lines", the answer rendered, and the footer
    "8.1 s · 2 turns · $0.0412"; the composer is back to Send and the strip to "ready".
  - `tab:agent:ask:fix the typo` — an `Edit` as a line diff: the removed line on a red
    wash with `−`, the added one on a green wash with `+`, the context in between, the
    path above.
  - `tab:agent:ask:which one` — two questions as chips, one of them multi-select, each
    with an "Other…" row, and an Answer that waits until both are answered.
  - `tab:agent:ask:plan it` — `ExitPlanMode` rendered as markdown through `brain_render`
    (a heading, a numbered list, a closing line) with Approve, "Approve, accept edits" and
    "Keep planning".
  - `left:agents` — the sessions pane: the header, a filter, New, and one row with its
    state dot, its title, "needs input", and its folder and model.
  - `right:agent,agent:ask:…` — the pane beside the note on the same components: the MCP
    call as "rusty · brain_read_page" with its result, the answer rendered, the permission
    as a card, the footer, and the mode and model chips in a sidebar width.
  - Every scene's log was read for `TypeError`, `ReferenceError` and "is not a function";
    the two that appeared are findings 1 and 2 of the ledger, and the logs are clean now.
- After delivery: the first CI run failed where every local gate had passed (ledger entry
  13, a reserved word as a property name). Fixed and pushed as a follow-up commit with the
  scan that makes the rule executable.
- Skips or pre-existing failures: none. A pointer walk (dragging, clicking each chip) and
  the tab against a real `claude` are Chad's, as every UI ticket here has left them.

## Phase 5: Complete

- Requirement audit: REQ-001 to REQ-010 satisfied with named evidence (the AAR's outcomes
  list each against a scene, a test or a reading). None split, none waived. TICKET-033
  holds what the spec named as out: slash commands, `@` mentions, image paste, fork, the
  context meter, subagent nesting and "open in terminal".
- Docs: `docs/architecture.md` (an Agent tab bullet beside the sessions one), `README.md`
  (a section on the tab: what a card shows, the keys that answer one, the composer),
  `ROADMAP.md` (032 ticked), and the wiki below.
- OpenWiki: two runs. `04ea0409-f141-479f-b83f-cc0cba41c7b4` came back `complete` with a
  warning — `workspace-app.md` still carried evidence debt from claims whose lines this
  ticket moved, so its sidecar was left unchanged and none of the tab's claims landed.
  `59fcaed5-28b5-42ba-9678-fa859a0af2ae` re-cited those eight (the workspace state, the
  graph's settings and its opener, the hotkeys table, the text size, the pane, the scenes,
  the top bar) and added the tab's eight, and returned `{"status":"complete"}` with no
  warnings. The PostToolUse hook stayed silent both times, so bulletin 3's recovery wrote
  the receipt; the receipt was read, not listed
  (`PR-rusty-read-the-receipt-not-the-filename-001`, which this pipeline's predecessor
  earned an hour earlier).
- AAR: `docs/planning/knowledge/aar/AAR-032-agent-tab.md`, submitted. Register:
  `AD-rusty-agent-tab-is-a-client-surface-001`,
  `PR-rusty-one-writer-one-row-shape-001` and
  `PR-rusty-render-at-the-end-of-a-block-001`, in both the AAR and the index.
- Brain capture: a timeline entry on `projects/rusty-v3`.
- Archive: the ticket to `closed/`, the pair to `completed/`, the index updated.

## Defect and lesson ledger

| When | What | Lesson or rule ID |
|---|---|---|
