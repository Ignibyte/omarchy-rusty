---
title: AAR-031-agent-sessions
pipeline_id: 17213969-b6d1-484f-b69f-dc04506b56e2
ticket: TICKET-031
status: closed
created: 2026-09-17
submitted: 2026-09-17
---

# AAR-031: Agent sessions that outlive the app

## 0. Recall log

- The register said the pane's agent is a headless Claude Code owned by the app
  (`AD-rusty-pane-agent-is-headless-claude-001`) and the tabs are terminals
  (`AD-rusty-agents-are-terminals-001`). This ticket qualifies both: the pane became a
  client, and the process moved into a unit of its own.
- `AD-rusty-app-as-session-service-001` and `PR-rusty-restart-always-001` gave the unit
  vocabulary; `PR-rusty-systemctl-stand-in-001` and
  `PR-rusty-probe-kills-from-outside-001` decided how it would be proved without taking
  Chad's window; bulletin 2 (no synthetic input on his desktop) held throughout.
- TICKET-025's notes carried the wire and the lesson that produced it: probe the case you
  will build on, and keep the probe's lines as the parser's fixtures.
- Claude Code's own docs (headless, streaming input, sessions, agent view) settled the
  flags before the probe and ruled out `claude --bg`: its attach protocol is undocumented,
  TUI-only and single-client.
- The brain's `projects/php-desktop-app` records the same shape from Rusty v2 — a daemon
  owning agent sessions with an event log so the UI is a reconnectable view — which is
  what this ticket rebuilt in Rust.

## 1. Outcomes

- `REQ-001` PASS — `launch::tests::systemd_run_args_are_exact`; on a real unit,
  `/proc/<MainPID>/cgroup` read `…/app.slice/rusty-agent-<id>.service`, beside the app's
  `…/app-graphical.slice/rusty-app.service`.
- `REQ-002` PASS — `host::tests::a_detached_client_does_not_stop_the_turn_…`; by hand, a
  message sent and the client closed at once, the answer in the log when it came back.
- `REQ-003` PASS — three host tests (`since: 0`, `since: N`, two clients); the pane
  scenes: a second run of the app replayed the whole conversation.
- `REQ-004` PASS — `an_exit_is_recorded_and_the_next_message_resumes`,
  `a_stale_resume_starts_fresh_with_a_note`, `an_idle_child_is_stopped_…`.
- `REQ-005` PASS — `a_permission_asked_while_nobody_watches_notifies` and the recording
  notifier asserting silence while a client is attached.
- `REQ-006` PASS — `stop_runs_the_sequence_…`; on a real unit, `systemctl --user stop`
  signalled the host alone and the process exited 0 because its stdin was closed.
- `REQ-007` PASS — `session::tests::agent_is_a_noun_before_the_scripts`,
  `agent::tests::verbs_that_cannot_run_answer_with_the_usage`.
- `REQ-008` PASS — reading of `RightPane.qml` and the two scenes.
- `REQ-009` PASS — `wire::tests::parse_line_reads_the_2_1_274_probe` and the message
  tests.
- `REQ-010` PASS — `diff::tests` (six).

`GATE GREEN [diff]` with the receipt; `openwiki_finish` returned `complete` and, for the
sixth ticket running, the PostToolUse hook did not write its receipt — bulletin 3's
recovery was needed again.

## 2. What went well

- The probe paid for itself twice over. Eleven scenarios against `claude` 2.1.274 turned
  four design guesses into facts before a line was written: `init` opens every *turn*, not
  the process; thinking text never travels, only its token estimate; `--session-id` is
  refused for a session that exists, so an existing conversation is only ever resumed; and
  a `set_permission_mode` to `manual` answers `{"mode":"default"}`. Each of those would
  have been a bug found late.
- The host's tests drive `serve` through its socket, exactly as the app does. That is why
  they caught the drain defect (finding 1), which no unit test of a function would have
  seen: the code was correct, the runtime simply went before the bytes did.
- Two things fell out of the design rather than being added: the pane's user bubbles come
  from the host's echo, so live and replayed conversations are rendered by one path; and
  the deltas being live-only kept the log small without losing anything on replay, because
  the `assistant` line carries the whole block.
- `current_exe` as the host meant no unit file to ship, nothing for the installer or the
  package to learn, and a development binary that hosts development sessions.
- `bin/gate.sh --verify` caught the stale OpenWiki receipt. I had checked that the file
  existed and called it done; the verify read its fingerprint and refused. The guardrail
  did the job the eye did not.

## 3. What went poorly

- The first test run hung for eleven minutes and had to be ended by hand, because a
  harness that panicked joined a host thread nobody had told to stop
  (`PR-rusty-a-test-that-panics-must-not-hold-a-process-001`). The build was already done;
  what hung was the test binary, so cargo itself was left alone and reaped it.
- Two of the three failures in the next run were mine in the test, not the code: one read
  the stream out of order, one compared re-serialised JSON as a string.
- `build_args` grew a `--dangerously-skip-permissions` beside `bypassPermissions` while I
  was mapping the CLI's surface. Nothing shipped it, but inspect had to catch a flag Rusty
  would have chosen on the user's behalf — the one thing the pane's invariant forbids.
- The `Agents` bridge had to move out of `agent/` after the build failed: cxx-qt takes
  every bridge of a QML module from one directory (QTBUG-93443). Ten minutes, but it is
  the kind of constraint worth knowing before laying out a module.
- I recorded the OpenWiki hook as having fired, on the evidence that the receipt file
  existed. It was the previous pipeline's receipt, dated twelve days earlier. TICKET-010
  wrote that trap down and I walked into it anyway; the notes and this AAR were corrected
  once `--verify` said so, and the rule now reads as its own entry in the register.

## 4. Surprises

- `systemctl kill` and `systemctl stop` are different tests. `kill` signals every process
  in the unit whatever `KillMode` says, so it proved the host records a killed child;
  `stop` respects `KillMode=mixed`, and only then did the ordered shutdown show itself —
  the child exiting **0** because its stdin closed, in 13 ms.
- The socket-path limit (107 bytes) is not theoretical: this session's own scratch
  directory is 149 bytes, so the first end-to-end probe failed on it. The guard worked,
  but the message was in the host's log where the caller never looks.
- `serde_json::Value` sorts object keys, so a line logged through the protocol comes back
  alphabetised. Harmless — nothing reads a line by position, and Claude Code keeps the
  original — but it turns a string comparison in a test into a lie.
- An idle `claude` child is about 260 MB before its first message and 308 MB after a turn.
  That is what the idle timeout is for, and it is why the pane's sessions stop after ten
  minutes rather than sitting open per page.

## 5. Lessons

- `PR-rusty-drain-before-the-runtime-goes-001`: keep the writers' handles and await them
  before the owner returns, or the last thing said is lost.
- `PR-rusty-a-test-that-panics-must-not-hold-a-process-001`: a harness's `Drop` stops what
  it started before it joins.
- `PR-rusty-read-the-stream-in-the-order-it-is-written-001`: a wait for a later line eats
  the earlier one it was about to assert.
- `PR-rusty-what-the-user-typed-is-theirs-001`: `0700` and `0600` at creation for anything
  that records what was typed to an agent.
- `PR-rusty-a-fixture-lies-by-omission-001`: a fake sends what the probe recorded, not the
  minimum that makes one scene pass.
- `PR-rusty-read-the-receipt-not-the-filename-001`: a receipt is read, never listed — the
  previous pipeline's file sits at the same path and looks exactly like success.
- `AD-rusty-agent-sessions-are-user-units-001`: the decision itself, qualifying
  `AD-rusty-pane-agent-is-headless-claude-001` and, through it,
  `AD-rusty-agents-are-terminals-001` a second time.
- Standing, not new: the QML scope rule bit twice more in one file (a property bound to
  its own id, and `agents` clashing with the window's list of agent CLIs). It is
  `PR-rusty-qml-component-scope-001`, and it is worth reading before naming anything in
  QML.

## 6. Time spent

| Phase | Estimated | Actual |
|---|---|---|
| 1 Plan | 1 h | 1 h (four tickets, the spec pair, the AAR, the brain decision) |
| 2 Design | 1 h | 1.5 h (the probe was most of it) |
| 3 Implement | 3 h | 3 h (ten modules, the client, the pane, the scenes) |
| 3.5 Inspect | 1 h | 1 h (fifteen entries, eight fixed) |
| 4 Validate | 1 h | 1.5 h (one hung run, two real units, the scenes) |
| 5 Complete | 1 h | 1 h |
