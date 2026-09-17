---
title: Agent sessions that outlive the app: notes
pipeline_id: 17213969-b6d1-484f-b69f-dc04506b56e2
---

# Agent sessions that outlive the app: running notes

Chronological evidence and decisions. If a command did not run, these notes do not say it
passed.

## Phase 1: Plan

- Recall (2026-09-17, the planning session):
  - Bulletins: three notices, none critical. The second binds this ticket: no synthetic
    input on Chad's desktop; verify from logs, throwaway data and offscreen scenes.
  - Register: `AD-rusty-agents-are-terminals-001` (qualified once by
    `AD-rusty-pane-agent-is-headless-claude-001`, qualified again here),
    `AD-rusty-app-as-session-service-001`, `AD-rusty-commands-are-nouns-and-verbs-001`,
    `PR-rusty-restart-always-001`, `PR-rusty-user-oom-floor-001`,
    `PR-rusty-probe-kills-from-outside-001`, `PR-rusty-systemctl-stand-in-001`,
    `PR-rusty-workspace-state-in-json-001`, `PR-rusty-notes-as-you-go-001`, "the app
    touches nothing under `~/.rusty`" (TICKET-015).
  - Nearest notes: `native-agent-pane` (the wire on 2.1.260, `--permission-prompt-tool
    stdio` is the flag that prompts, a fake `claude` seeded by the screenshot script,
    probe the case you build on), `session-resilience` (the units, the OOM floor, kill
    probes from outside), `session-commands` (the noun parser, the stand-in `systemctl`).
  - Wiki: `openwiki/workspace-app.md` (the pane's paragraph, the terminals, the
    invariants), `development-and-validation.md` (running as services).
  - Brain: `projects/rusty-v3` (TICKET-025's timeline entry: "the tabs stay tmux
    terminals"; TICKET-008: "a model-backed chat pane is a later ticket");
    `projects/php-desktop-app` records Rusty v2's own shape — a daemon owning agent
    sessions with an event log so the UI is a reconnectable view.
  - Code: `assistant.rs` (`build_args` :366, `claude_binary` :406, `Process`/`spawn`
    :429-536, `parse_line` :215-321, the builders :324-356, no `Drop`), `RightPane.qml`
    :336-440, `Main.qml:56` (one `Assistant` per window), `session.rs` (`parse` :76-93,
    `USAGE` :38-47, `complete_path` :161), `terminals.rs` (`notify` :326, `state_path`
    :122), `theme.rs` `watch` :367-395.
  - The box: claude 2.1.274; `systemd-run`, `tmux`, `socat`, `notify-send` on PATH;
    `XDG_RUNTIME_DIR=/run/user/1000`; the tmux server (pid 4160, `tmux: server`, ppid
    1098) in `/user.slice/user-1000.slice/user@1000.service/app.slice/app-graphical.slice/rusty-app.service`
    beside the app (pid 3518) and its two tmux clients (4150, 4160): the app unit's
    cgroup holds the server → TICKET-034. `claude --help` on 2.1.274 lists
    `--permission-mode` choices `acceptEdits, auto, bypassPermissions, manual, dontAsk,
    plan` (`default` still parses); `--session-id`, `--replay-user-messages`, `--name`,
    `--fork-session` present; the strings `set_permission_mode`, `set_model`,
    `interrupt`, `can_use_tool`, `initialize` exist in the binary. The user manager's
    environment carries `PATH` (mise shims, `~/.local/bin`), `DBUS_SESSION_BUS_ADDRESS`,
    `WAYLAND_DISPLAY`. `rusty-app.service` reads `OOMScoreAdjust=200`.
  - Docs read: code.claude.com/docs headless, cli-reference, agent-sdk/user-input,
    agent-sdk/streaming-input, sessions, agent-view (2026-09-17).
- Decisions: the seven locked decisions in the spec; Chad's three answers of 2026-09-17
  (systemd transient unit; the click opens the chat tab; persistence first); tickets
  032–034 minted so the plan is in the record; `brain_ask` consultation
  `1218a070ff8445a08bc75579c5ff003c` (no prior decision touched the question).

## Phase 2: Design

- The wire probe (2026-09-17, `scripts/probe-claude-wire.sh`, eleven scenarios on claude
  2.1.274 with `--model haiku` in a scratch cwd; the lines under the session's scratchpad
  `probe/*.jsonl`, scrubbed copies as fixtures in `wire.rs`):
  1. `system/init` arrives at the start of every turn, after the user message, never at
     process start: a fresh process with an open stdin prints nothing for 60 s. Fields:
     `cwd`, `session_id`, `tools`, `mcp_servers` (`name`, `status`, `source`), `model`,
     `permissionMode`, `slash_commands`, `terminal_slash_commands`, `agents`, `skills`,
     `claude_code_version`, `capabilities` (`interrupt_receipt_v1`,
     `interrupt_cancel_queued_v1`, `msg_lifecycle_v1`).
  2. `system/status` `{"status":"requesting"}` opens each API request; after a
     `set_permission_mode` the line is `{"status":null,"permissionMode":"acceptEdits"}`.
     `system/thinking_tokens` carries `estimated_tokens` and a delta while the model thinks.
     `rate_limit_event` carries `rate_limit_info.unifiedWindows.{five_hour,seven_day}`
     (`utilization`, `resetsAt`) once a turn.
  3. `stream_event` lines are the API events verbatim: `message_start`,
     `content_block_start` (`thinking` with empty text and signature, `text`, `tool_use`),
     `content_block_delta` (`thinking_delta` with an empty `thinking` and an
     `estimated_tokens`, `signature_delta`, `text_delta`), `content_block_stop`,
     `message_delta` (usage, stop reason), `message_stop`. Thinking text is empty on the
     wire; only its token estimate and signature travel.
  4. `assistant` lines arrive once per content block, each with a one-element `content`
     (the thinking block, then the text or the tool use), the same `message.id`; `model`,
     `parent_tool_use_id`, `uuid`, `timestamp`, `request_id` beside.
  5. `user` lines: a tool result `{tool_use_id, type: tool_result, content, is_error}` (a
     denied tool's result carries the deny message with `is_error: true`); the interrupt
     inserts a text `[Request interrupted by user]`; a `set_model` after a turn echoes
     `<local-command-stdout>Set model to …</local-command-stdout>` as a string `content`
     with `isReplay: true`; `--replay-user-messages` echoes the user's own message with
     `isReplay: true` about a second after it was written.
  6. `control_request` `can_use_tool`: `request_id`, `request.{tool_name, display_name,
     input, description, permission_suggestions, tool_use_id}`; for a `Write` the
     suggestion was `{"type":"setMode","mode":"acceptEdits","destination":"session"}`.
  7. The CLI's `control_response` lines: an interrupt is acknowledged with
     `{"still_queued":[]}`, then the partial `assistant` text, the synthetic user line and a
     `result` `error_during_execution` (`is_error: true`, `terminal_reason:
     "aborted_streaming"`); the process stays alive. `set_permission_mode` answers
     `{"mode":"acceptEdits"}` and a status line; `manual` answers `{"mode":"default"}`;
     `bogus` answers `{"subtype":"error","error":"Cannot set permission mode: must be one
     of acceptEdits, auto, bypassPermissions, default, dontAsk, plan"}`; `set_model`
     answers a bare success and the next `init` names the model. `acceptEdits` ran a
     `Write` without a prompt; `default` prompted again.
  8. `result`: `subtype` `success` or `error_during_execution`, `is_error`, `result`,
     `errors` (a list, on a failed start), `duration_ms`, `duration_api_ms`, `num_turns`,
     `total_cost_usd`, `usage` (input, output, cache, thinking), `modelUsage` (per model,
     with `contextWindow`), `permission_denials`, `terminal_reason`,
     `queued_turn_count`, `result_index`. Two user messages written back to back ran as
     two turns, each with its own `init` and `result`.
  9. SIGTERM mid-turn: exit 143, no `result`. `--resume` afterwards prints nothing until a
     message; the cut turn's partial text is not in the transcript ("I didn't write any
     numbers"). `--resume` of a missing id: stderr `No conversation found with session
     ID: …`, a `result` `error_during_execution` with `errors` and `num_turns: 0`, exit 1,
     at once, before any input. `--session-id` of an existing id: stderr `Session ID … is
     already in use.`, exit 1 — an existing session is only ever `--resume`d.
     `--name` is accepted with `-p`. Closing stdin ends the process with the last result's
     status (0, or 1 after an interrupt).
  10. `--mcp-config` with an HTTP `rusty` beside a scratch `.mcp.json` naming a stdio
     `rusty`: one `rusty` in `init.mcp_servers`, `source: "dynamic"`, connected, 85 tools,
     no `mcp_server_errors`; without `--strict-mcp-config` the user-level servers load too.
  11. RSS: 263 MB after start (no message yet), 308 MB after one turn. `init` 0.5 s after
     spawn, the first token about a second later.
- Architecture and data flow: the app binary gains the `agent` noun (`src/agent/`). A
  session = a transient unit `rusty-agent-<id>.service` running `rusty agent host --id
  <id>`, which owns one `claude -p` child (stdio pipes), an append-only log, a Unix socket.
  Clients (`Assistant` in the app, `rusty agent attach`) attach with a sequence number,
  replay, then stream; anything a client sends is written to the child's stdin and echoed
  to every client as a `sent` record. The host, not the app, notifies when nobody is
  attached. The registry is the state directory (one JSON entry per session, flat, beside
  the session's log directory); `Agents` lists it and watches it. No tool, no resource, no
  store change: the back end is not involved (the child reaches it as an MCP client, as
  before). QML surfaces this ticket: `RightPane.qml` on the client; `Main.qml` holds the
  registry.
- Changes to the plan from the probe: the host does not wait for `init` to call the child
  ready (a spawned child is ready; `init` marks the start of each turn and refreshes
  `model` and `permission_mode`); the stale-resume rule keys on a `result` with `errors`
  and `num_turns: 0` (or any exit) before the first `sent` user message; an existing
  session is always `--resume`d, `--session-id` only on a session's first spawn;
  thinking reaches the client as a block start and a token estimate (`ThinkingTokens`),
  not text; `system/status` becomes `Status(requesting)` and `ModeChanged(mode)`;
  `rate_limit_event` becomes `RateLimit`; a `user` line with a string content or
  `isReplay` becomes a `Notice` (the local command's text) or nothing (an echo), never a
  tool result; `--replay-user-messages` stays unused (the host's `sent` echo covers it).
- File manifest (one purpose each):
  - `crates/rusty-app/src/agent/mod.rs`: the noun (`run`, `AGENT_USAGE`), `SessionId`,
    `paths`, `unit_name`, the test helpers (`fake`, `SPAWN`).
  - `src/agent/wire.rs`: Claude's lines in (`Event`, `parse_value`, `parse_line`,
    `is_live_only`) and out (`user_message`, `control_response`, `interrupt_request`,
    `set_permission_mode_request`, `set_model_request`, `mcp_config`, `READ_TOOLS`).
  - `src/agent/spawn.rs`: `SpawnOptions`, `SessionArg`, `build_args`, `claude_binary`,
    `spawn_child`.
  - `src/agent/protocol.rs`: `ClientMsg`, `HostMsg`, `HostEvent`, `State`, `PROTOCOL`.
  - `src/agent/log.rs`: `EventLog` (`open`, `append`, `read_since`, `last_seq`).
  - `src/agent/registry.rs`: `Entry`, `Options`, the entry file functions, `alive`, and
    the `Agents` bridge with its watcher.
  - `src/agent/launch.rs`: `Runner`, `systemd_run_args`, `start_new`, `start_existing`,
    `stop_unit`.
  - `src/agent/host.rs`: `HostConfig`, `Grace`, `serve`, the loop, the stop sequence, the
    idle timer, the notifier.
  - `src/agent/client.rs`: `Connection` (std `UnixStream`, a reader thread).
  - `src/diff.rs`: `diff_lines`, `to_json`.
  - `src/assistant.rs`: the bridge as a client; `translate` (host records → events).
  - `src/session.rs`: `Request::Agent`, the `agent` lines in `USAGE`.
  - `src/main.rs`: `mod agent; mod diff;`, the dispatch arm.
  - `src/terminals.rs`: `send_notification` shared with the host.
  - `build.rs`: `src/agent/registry.rs` in `.files`.
  - `Cargo.toml`: tokio `net io-util process signal macros time`; `uuid` v4; `libc`.
  - `qml/RightPane.qml`, `qml/Main.qml`: the pane on the client, the registry.
  - `scripts/screenshot.sh`: the stand-in `systemd-run`, the state and run directories.
  - `scripts/probe-claude-wire.sh`: the probe (kept for the next claude upgrade).
- Store consequences: none. No table, no migration, no vault write; the state lives under
  `~/.local/state/rusty/agents/` (XDG state), sockets under `$XDG_RUNTIME_DIR/rusty/agents/`.
- Tool contract: no tool added, renamed or removed. The child still reaches the store as
  an MCP client of the running `rusty-mcp` over HTTP; the dynamic `--mcp-config` entry
  wins over a project's stdio `rusty` by name (probe 10).
- Regression plan:
  | Requirement | Evidence |
  |---|---|
  | REQ-001 | `launch::tests::systemd_run_args_are_exact`; Phase 4 step 1 (cgroup, unit properties) |
  | REQ-002 | `host::tests::a_detached_client_does_not_stop_the_turn`; Phase 4 step 2 |
  | REQ-003 | `host::tests::attach_replays_then_streams`, `attach_since_skips_earlier_lines`, `two_clients_see_the_same_lines`; the pane scene reopened |
  | REQ-004 | `host::tests::an_exit_is_recorded_and_the_next_message_resumes`, `a_stale_resume_starts_fresh`, `an_idle_child_is_stopped_and_resumed` |
  | REQ-005 | `host::tests::nobody_attached_means_a_notification` |
  | REQ-006 | `host::tests::stop_runs_the_sequence`, `a_stubborn_child_is_killed_within_the_grace`; Phase 4 step 5 |
  | REQ-007 | `session::tests` (`agent` parses), `agent::tests` (verbs, a bad id, the usage) |
  | REQ-008 | reading of `RightPane.qml`; the `right:agent,agent:ask:` scene and `right:agent` again; Chad's smoke |
  | REQ-009 | `wire::tests` on the 2.1.274 lines; `control_response` with `updatedPermissions` and `message` |
  | REQ-010 | `diff::tests` |
- Risks: a Qt-linked host's own memory (measured in Phase 4; the fallback is a Qt-free
  crate); a node child per idle session (bounded by the idle timeout; the pane's is
  600 s); the log holding what the user typed in plaintext under `~/.local/state` (as
  Claude's own transcript does under `~/.claude`); a socket path over 107 bytes (refused
  with a message; tests use short paths); a permission pending across a respawn (expired
  by the host and the client; the model is not re-asked on `--resume`, probe 9); the
  transcript missing the partial text of a turn SIGTERM cut (the log keeps it; the client
  shows it; the model does not know it); no back end: the child reaches nothing and its
  tool calls fail as they do today, the pane keeps rendering; theme and keyboard:
  unchanged surfaces this ticket.
- Decisions made and alternatives set aside: the seven locked decisions; the probe's
  amendments above; `Restart=on-failure` over `always` (a stop must stay stopped);
  `KillMode=mixed` over `control-group` (the stop sequence needs the host alive after
  SIGTERM); the default `app.slice` over `app-graphical.slice` (agents need no display);
  the host's `sent` echo over `--replay-user-messages`; deltas live-only over a full log.
- CodeGraph evidence (`codegraph_explore` over `Assistant`, `session`, `terminals`,
  `theme::watch`, `main`): `build_args`, `parse_line`, `control_response` have two callers
  each, all inside `assistant.rs` (the bridge and its tests), so moving them under
  `agent::wire` and `agent::spawn` touches nothing else; `state_path` has three callers
  (`terminals.rs` twice, `theme.rs` once) and `tabs_path` three, none touched; `notify` is
  a bridge method with one QML caller (`Main.qml` `attention`), so extracting its body
  into a function changes no caller; `session::parse` is called from `main.rs` alone and
  covered by `session::tests`; `USAGE` is asserted by `the_usage_names_every_verb`. The
  blast radius of the refactor is `assistant.rs`, `RightPane.qml` and `Main.qml`; the new
  modules have no callers yet.

## Phase 3: Implement

- Built (2026-09-17):
  - `crates/rusty-app/src/agent/`: `mod.rs` (the noun, `SessionId`, `paths`, `unit_name`,
    the verbs and their argument parsing, the test helpers), `wire.rs` (Claude's lines in
    and out, with the 2.1.260 and 2.1.274 fixtures), `spawn.rs` (`SpawnOptions`,
    `SessionArg`, `build_args`, `claude_binary`), `protocol.rs` (`ClientMsg`, `HostMsg`,
    `HostEvent`, `State`), `log.rs` (`EventLog`), `registry.rs` (the entries), `launch.rs`
    (`Runner`, `systemd_run_args`, `start_new`, `start_existing`, `stop`), `host.rs` (the
    loop, the stop sequence, the idle timer, the notifier), `client.rs` (`Connection`).
  - `src/agents.rs`: the `Agents` QML type. It lives beside the other bridges rather than
    under `agent/` because cxx-qt takes every bridge of a QML module from one directory
    (its own error names Qt bug QTBUG-93443).
  - `src/diff.rs`: the line diff behind `Assistant.diff`, for TICKET-032's Edit cards.
  - `src/assistant.rs`: rewritten as the host's client — `create`, `attach`, `detach`,
    `send`, `answer`, `interrupt`, `change_mode`, `change_model`, `stop`, `remove`,
    `diff`, and the signals TICKET-032 needs; `translate` maps host messages to client
    events and is tested without Qt.
  - `src/session.rs`, `src/main.rs`: the `agent` noun before Qt; `src/terminals.rs`:
    `send_notification`, shared with the host; `build.rs`, `Cargo.toml`.
  - `qml/RightPane.qml`: the pane owns its own `Assistant`, attaches to the page's
    session, creates one on the first message, replays, and detaches on a page switch;
    `qml/Main.qml`: the `Agents` registry and its watcher.
  - `scripts/screenshot.sh`: a stand-in `systemd-run` (the scenes must never touch
    Chad's user manager) and the scratch state, run and notification paths.
  - `scripts/probe-claude-wire.sh`: the probe, kept for the next claude upgrade.
- Deviations from the manifest:
  - The `Agents` bridge is `src/agents.rs`, not `src/agent/registry.rs` (the one-directory
    rule above). The registry's own functions stay in `agent/registry.rs`.
  - `paths::entry_path` and `paths::log_path` became `entry_path_in`/`log_path_in` on the
    directories the host is configured with, so a test never writes to the real state
    directory.
  - `wire::parse_line` is `#[cfg(test)]`: the host parses values it has already read, and
    the tests read the probe's lines through it.
  - The pane's New stops and unbinds the session instead of deleting it; its log stays on
    the machine, which `rusty agent list` still shows.
  - A page whose saved id is from before this ticket (Claude Code's own id, which no entry
    knows) is passed as the `resume` of the session its first message creates, so the
    conversation carries over without a migration step.
- Fast gate: `bin/gate.sh --fast` → `GATE GREEN [fast]` (2026-09-17), after `cargo fmt
  --all` and two clippy fixes (a redundant closure on the notifier, a `loop`+`match` that
  is a `while let`).

## Phase 3.5: Inspect ledger

| # | Lens | Finding | Severity | Disposition |
|---|---|---|---|---|
| 1 | correctness | The last lines of a stopping host never reached its clients: `broadcast` queues to each client's writer task, and `serve_async` returned (dropping the runtime) before those tasks drained, so `stopping` and `child_exited` were lost. | high | Fixed: `Client` keeps its writer's `JoinHandle`, and `drain_clients` closes each channel and awaits its writer (two seconds each) after the stop sequence. Three host tests caught it and now prove it. |
| 2 | data safety | The event log and the registry entries were created with the default mode, so every account on the machine could read what was typed to an agent and what its tools answered. | high | Fixed: the state and log directories are `0700`, the entries and the log `0600`, written that way from the start (`OpenOptions::mode`, `set_permissions` before the rename). New test `the_state_stays_with_the_user`. |
| 3 | secrets, what leaves the machine | `build_args` added `--dangerously-skip-permissions` whenever the mode was `bypassPermissions`, a flag Rusty chose on the user's behalf. | high | Fixed: the mode is named and nothing else. Test `a_mode_is_passed_as_a_mode_and_no_more` asserts the flag is absent. |
| 4 | correctness | A clean idle stop reached the pane as `exited`, so a page's conversation grew a "Claude Code stopped" notice every ten minutes. | medium | Fixed: `exited` carries the reason (`exit`, `idle`, `stop`, `signal`) and the pane shows a line only for an exit nobody asked for. TICKET-032 gets the reason for free. |
| 5 | usability | `rusty agent attach <id> < /dev/null` (or through a pipe) quit at once: the end of stdin was read as "detach". | medium | Fixed: stdin's end detaches only when stdin is a terminal; otherwise the stream goes on. Verified by hand — the replay of a two-turn session printed in full. |
| 6 | usability | A run directory long enough to overrun a Unix socket path was refused by the host, in a journal the caller never reads; `rusty agent start` said only "the host did not answer". | medium | Fixed: `paths::check_socket_path` runs before a unit is asked for, in both `start_new` and `start_existing`, with the limit in the message. Test `a_socket_path_too_long_is_refused_up_front`. Found by running the probe from this session's own scratch path (149 bytes). |
| 7 | QML state | The pane declared `readonly property var assistant: assistant`, which binds a property to itself, and `agents` for the registry, which is already the window's list of agent CLIs on PATH. | medium | Fixed before it shipped: the pane uses the `Assistant` id directly, and the window's registry property is `registry`. This is `PR-rusty-qml-component-scope-001` biting a fourth time. |
| 8 | test integrity | A test that panicked left its host running, and `Drop` joined that thread for ever: the first run hung eleven minutes and was ended by hand. | medium | Fixed: `Drop` connects and sends `stop` before the join. The run now fails in seconds. |
| 9 | correctness | The `boot` test waited for the text delta before the turn's `init`, which the wait had already consumed, so the assertion could never hold. | low | Fixed: the test reads the lines in the order the process writes them. |
| 10 | fixture faithfulness | The screenshot script's fake `claude` never sent the final `assistant` text block, only the deltas, so a replayed scene showed an empty bubble where the answer belonged. | low | Fixed: the fake sends the block Claude Code sends (the probe's own shape). The replay scene now matches the live one. |
| 11 | complexity, reuse | The host re-serialises Claude's lines through `serde_json::Value`, so a logged line comes back with its keys sorted rather than as the process wrote them. | low | Accepted and documented in `protocol.rs`: nothing reads a line by position, Claude Code's own transcript keeps the original, and keeping the raw text would put a string where every client wants JSON. The round-trip tests compare parsed values. |
| 12 | correctness | The host's own interrupt used `u64::MAX` as its request number. | low | Fixed: the host numbers its control requests like any client. |
| 13 | keyboard, empty states | The pane's states were re-read: no `claude` (or no `systemd-run`) says so and hides the input; a page with no session shows the ask-about line and creates one on the first message; a detached host shows its notice; Enter sends and Shift+Enter breaks a line as before; every size still derives from `theme.scale` (the scan test passes). | — | No finding. |
| 14 | data safety | Nothing new touches the store, the vault or `~/.rusty`: the host's files are under XDG state and runtime, and the only thing sent anywhere is the localhost MCP URL the pane already used. | — | No finding. |
| 15 | false positive | CodeGraph reports "no covering tests found" for `serve`, `spawn_child`, `end_child` and `client_line`, and seven cross-crate callers for `entry`. | — | Rejected: the host is tested through its socket end to end (eleven tests drive `serve` as a client would), which is the level that matters; the `entry` callers are unrelated functions of the same name in `folders.rs` and the brain. |

- Post-implementation CodeGraph (`codegraph_explore` over `serve`, `spawn_child`,
  `end_child`, `client_line`, `Assistant`, `translate`, `attach`, `create`, `SessionId`,
  `session::parse`): the flow is `serve` → `serve_async` → `spawn_child`, and every caller
  of the new symbols is inside `agent/host.rs`; the modules have no dependents elsewhere,
  so the blast radius is the app's own `assistant.rs`, `RightPane.qml` and `Main.qml`, as
  the design said. `session::parse` gained one arm and keeps its single caller in
  `main.rs`.

## Phase 4: Validate

- Tests run (commands and output), 2026-09-17:
  - `cargo test -p rusty-app agent::` → `test result: ok. 38 passed; 0 failed; 0 ignored;
    0 measured; 54 filtered out; finished in 9.66s` (the wire's two probe fixtures, the
    protocol's round trips, the log, the registry and its permissions, the launcher's
    argument line and its socket-path guard, the client, the noun's parsing, and eleven
    host tests driving `serve` through its socket).
  - `cargo test -p rusty-app` → `test result: ok. 91 passed; 0 failed; 0 ignored;
    0 measured; 0 filtered out`.
  - Two runs failed on the way and are in the ledger: the first hung (finding 8) and was
    ended by hand; the second reported `8 passed; 3 failed` on findings 1 and 9.
- Gate run: `bin/gate.sh --diff` → `GATE GREEN [diff]`, `receipt written:
  .git/rusty-gate-receipt`; fmt, clippy with warnings as errors, the workspace's tests,
  the doc build, shell syntax, `167 gated files scanned` for secrets, whitespace. Two
  earlier runs were red at fmt and at clippy, fixed at the source.
- Smoke evidence (a scratch state directory, a fake `claude`, Chad's app and back end
  untouched throughout — `systemctl --user is-active rusty-app rusty-mcp` → `active`,
  `active` after every probe):
  - **The lifecycle through the CLI** with a stand-in `systemd-run`: `rusty agent start`
    printed the id, `list` showed it `idle`/alive, `attach` printed the hello and the
    replay, a typed line reached the process and its answer came back, `stop` left it
    `stopped`/not alive, `start <id>` brought it back, `rm` deleted the entry and the log.
  - **A conversation survives a detach**: a message sent and the client closed at once;
    reattaching with `--since 0` printed both turns, both answers and `caught_up`, in
    order, from the log alone.
  - **The unit is outside the app's cgroup** (REQ-001), on a real `systemd-run`:
    `/proc/<MainPID>/cgroup` read
    `…/user@1000.service/app.slice/rusty-agent-<id>.service`, beside (not inside)
    `…/app.slice/app-graphical.slice/rusty-app.service`. `systemctl --user show` read
    `Restart=on-failure`, `TimeoutStopUSec=20s`, `Slice=app.slice`, `KillMode=mixed`,
    `SyslogIdentifier=rusty-agent`.
  - **The stop sequence** (REQ-006), twice on a real unit:
    `systemctl --user kill -s TERM` (`PR-rusty-probe-kills-from-outside-001`; a group
    signal, harsher than a stop) → `stopping {reason: "signal"}`, `child_exited
    {signal: 15, reason: "stop"}`, entry `stopped`, unit inactive.
    `systemctl --user stop`, the path a logout takes → the journal shows the signal going
    to the host alone, the child then exits **0** because its stdin was closed, not
    because it was killed: `child_exited {code: 0, signal: null, reason: "stop"}`; the
    whole stop took 13 ms. That is what `KillMode=mixed` was chosen for.
  - **The pane, offscreen** (REQ-002, REQ-003, REQ-008): scene
    `right:agent,agent:ask:What is Orbit about?` shows the user's bubble, the tool call
    with its input, its result, the streamed answer and a permission prompt with Allow and
    Deny. Running the plain `right:agent` scene afterwards **in a second app process over
    the same state** brings the whole conversation back — question, tool call, result, the
    full answer, the still-pending permission, status `asking` — which is what TICKET-025
    could not do. Both photographed.
  - Journal: `journalctl --user -u rusty-agent-<id>` carried the unit's start and stop;
    the host's own lines and the process's stderr go there under `rusty-agent`.
  - Cleanup: every probe session removed, `systemctl --user list-units 'rusty-agent-*'` →
    `0 loaded units listed`, the run directory empty, `~/.local/state/rusty` never created.
- Skips or pre-existing failures: none. The live switch (the reinstall and Chad's own
  window) is his, as TICKET-029 left it; `cargo build` still warns that the gold linker is
  deprecated, which is the box's linker configuration and not this change.

## Phase 5: Complete

- Requirement audit: REQ-001 to REQ-010 all satisfied with named evidence (the list is in
  the AAR's outcomes, each against a test or a probe on a real unit). None split, none
  waived. Three things the ticket deliberately left out have tickets of their own:
  TICKET-032 (the Agent tab), TICKET-033 (composer extras) and TICKET-034 (the terminals'
  tmux server in its own unit, opened from this pipeline's finding of 2026-09-17).
- Docs: `docs/architecture.md` (the `Assistant` bullet rewritten, an agent-sessions
  bullet in the app's shape and one in the services list), `README.md` (an "Agent
  sessions" section and the pane's line), `ROADMAP.md` (031 ticked, 032 and 033 queued,
  034 under Later), `omarchy/README.md` (the operator's section: the unit line, why each
  property, the commands, the journal, logout and linger, and the tmux server's caveat),
  and the wiki below.
- OpenWiki: run `64acf721-8610-486a-9d77-58e538fa2b1a` (update). Claims: the agent-pane
  claim updated to the client shape and seven added on `workspace-app.md` (the transient
  unit, the host as the one owner and writer, the live-only deltas, the ordered stop, the
  idle stop and stale resume, the notification with nobody attached, the session's files
  and their permissions); three added on `development-and-validation.md` (the unit is not
  shipped, the socket-level suite, the systemd stand-in); one added on `quickstart.md`
  (the `agent` noun); five re-evidenced where lines moved. Prose updated on those three
  pages; the other four were read and left alone (no tool, resource, transport, vault
  rule, renderer or gate changed). `openwiki_finish` → `{"status":"complete"}`. The
  PostToolUse hook stayed silent, for the sixth ticket running, and the receipt at
  `.git/rusty-openwiki-receipt` was the previous pipeline's (`c7987c16…`, 2026-09-05) —
  which I first read as success because the file existed, the exact trap TICKET-010
  recorded ("a receipt from a previous run looks exactly like a receipt from this one
  until you read its pipeline id"). `bin/gate.sh --verify` named it: "worktree changed
  since the OpenWiki run finished at 2026-09-05T14:53:28Z". Recovery was bulletin 3's:
  the genuine finish result fed to `.claude/hooks/record-pipeline-tool-use.sh` on stdin
  with the spec put back under `active/` for the moment it reads the pipeline id (the
  planning record is not among the gated paths, so the move does not move the
  fingerprint), then the pair archived again and `--verify` re-read.
- AAR: `docs/planning/knowledge/aar/AAR-031-agent-sessions.md`, submitted. Register:
  `AD-rusty-agent-sessions-are-user-units-001` and five prevention rules
  (`PR-rusty-drain-before-the-runtime-goes-001`,
  `PR-rusty-a-test-that-panics-must-not-hold-a-process-001`,
  `PR-rusty-read-the-stream-in-the-order-it-is-written-001`,
  `PR-rusty-what-the-user-typed-is-theirs-001`,
  `PR-rusty-a-fixture-lies-by-omission-001`) in both the AAR and
  `docs/planning/knowledge/INDEX.md`.
- Brain capture: the decision page
  `decisions/rusty-agent-sessions-run-as-systemd-transient-user-units-owned-by-a-session-host`
  (written at Phase 1 from consultation `1218a070ff8445a08bc75579c5ff003c`, follow-up due
  2026-10-01) and a timeline entry on `projects/rusty-v3`.
- Archive: the ticket to `closed/`, the pair to `completed/`, the index updated.

## Defect and lesson ledger

| When | What | Lesson or rule ID |
|---|---|---|
| 2026-09-17 | The tmux server of the terminal tabs runs inside `rusty-app.service`'s cgroup; a unit stop or crash restart kills it | TICKET-034 |
| 2026-09-17 | The host's last lines were lost when its runtime went before the writer tasks drained | `PR-rusty-drain-before-the-runtime-goes-001` |
| 2026-09-17 | A panicking test harness joined a host it never stopped; the suite hung eleven minutes | `PR-rusty-a-test-that-panics-must-not-hold-a-process-001` |
| 2026-09-17 | A test waited for the text delta first and ate the `init` it was about to assert | `PR-rusty-read-the-stream-in-the-order-it-is-written-001` |
| 2026-09-17 | The event log and the entries were world-readable | `PR-rusty-what-the-user-typed-is-theirs-001` |
| 2026-09-17 | The screenshot fake never sent the final assistant text, so the replay looked broken | `PR-rusty-a-fixture-lies-by-omission-001` |
| 2026-09-17 | `build_args` added `--dangerously-skip-permissions` beside `bypassPermissions` | inspect finding 3; the pane's invariant holds |
| 2026-09-17 | A QML property bound to its own id, and `agents` clashing with the window's CLI list | `PR-rusty-qml-component-scope-001` |
