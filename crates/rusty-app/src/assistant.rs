//! The `Assistant` QML type: a client of a session host (TICKET-031). The headless
//! Claude Code of TICKET-025 is no longer the app's child: a host under its own user
//! unit owns it (`agent::host`), and this type attaches to the host's socket, replays
//! the conversation from the log, streams what follows, and writes the pane's messages,
//! permission answers and control requests through the host. One instance per pane or
//! tab; a page switch detaches and the turn goes on. Nothing here talks to the Claude
//! API: it is Claude Code's own harness without its terminal.

use core::pin::Pin;
use std::collections::BTreeSet;
use std::time::Duration;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde_json::Value;

use crate::agent::client::{Connection, Incoming};
use crate::agent::launch::{self, NewSession, Runner};
use crate::agent::protocol::{ClientMsg, HostEvent, HostMsg, State};
use crate::agent::spawn::{claude_binary, read_tools, SpawnOptions};
use crate::agent::wire::{self, Event};
use crate::agent::{paths, registry, SessionId};

#[cxx_qt::bridge]
mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        /// Qt's string type.
        type QString = cxx_qt_lib::QString;
    }

    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, available)]
        #[qproperty(bool, running)]
        #[qproperty(bool, busy)]
        #[qproperty(QString, status)]
        #[qproperty(QString, session_id)]
        #[qproperty(QString, claude_session_id)]
        #[qproperty(QString, child_state)]
        #[qproperty(QString, permission_mode)]
        #[qproperty(QString, model)]
        #[qproperty(i32, pending_count)]
        #[qproperty(QString, attached_state)]
        type Assistant = super::AssistantRust;

        /// Start a new session from a JSON object `{cwd, title, page, permissionMode,
        /// model, strictMcp, allowedTools: "reads"|"none", systemPrompt, mcpUrl,
        /// idleTimeout, resume}`; `created` answers with the id or the error.
        #[qinvokable]
        fn create(self: Pin<&mut Assistant>, options_json: &QString);
        /// Attach to a session's host, running it again first when it is down;
        /// `attached` answers, the log is replayed, `replayDone` marks the catch-up.
        #[qinvokable]
        fn attach(self: Pin<&mut Assistant>, id: &QString);
        /// Leave the host; the conversation goes on without a view.
        #[qinvokable]
        fn detach(self: Pin<&mut Assistant>);
        /// Send the user's text as one message; false when nothing is attached.
        #[qinvokable]
        fn send(self: Pin<&mut Assistant>, text: &QString) -> bool;
        /// Answer a permission request: allow with the input as given (`extra_json` may
        /// add `updatedPermissions`), or deny (`extra_json` may carry a `message`).
        #[qinvokable]
        fn answer(
            self: Pin<&mut Assistant>,
            request_id: &QString,
            allow: bool,
            input_json: &QString,
            extra_json: &QString,
        ) -> bool;
        /// Ask the running turn to stop; the process stays.
        #[qinvokable]
        fn interrupt(self: Pin<&mut Assistant>) -> bool;
        /// Change the permission mode for the turns to come.
        #[qinvokable]
        fn change_mode(self: Pin<&mut Assistant>, mode: &QString) -> bool;
        /// Change the model for the turns to come.
        #[qinvokable]
        fn change_model(self: Pin<&mut Assistant>, model: &QString) -> bool;
        /// End the process and its host; the session stays and can be started again.
        #[qinvokable]
        fn stop(self: Pin<&mut Assistant>);
        /// Stop the host and delete the session's entry and log.
        #[qinvokable]
        fn remove(self: Pin<&mut Assistant>);
        /// The line diff between two texts, as JSON rows of kind `same`, `add` or `del`.
        #[qinvokable]
        fn diff(self: &Assistant, before: &QString, after: &QString) -> QString;

        /// `create` finished: the new session's id, or an empty id and the error.
        #[qsignal]
        fn created(self: Pin<&mut Assistant>, id: QString, error: QString);
        /// The host answered and the replay begins.
        #[qsignal]
        fn attached(self: Pin<&mut Assistant>, id: QString);
        /// The replay is done; what follows is live.
        #[qsignal]
        fn replay_done(self: Pin<&mut Assistant>);
        /// A host came up (`resumed` when it continued an existing log).
        #[qsignal]
        fn host_started(self: Pin<&mut Assistant>, resumed: bool);
        /// The connection to the host ended.
        #[qsignal]
        fn host_exited(self: Pin<&mut Assistant>, message: QString);
        /// A turn began; the process announced its session id.
        #[qsignal]
        fn started(self: Pin<&mut Assistant>, session_id: QString);
        /// A content block began: `text`, `tool_use` (with its name and id) or `thinking`.
        #[qsignal]
        fn block_started(self: Pin<&mut Assistant>, kind: QString, name: QString, id: QString);
        /// A piece of the assistant's text.
        #[qsignal]
        fn text_delta(self: Pin<&mut Assistant>, text: QString);
        /// The whole text of an assistant message, once it is complete.
        #[qsignal]
        fn text_final(self: Pin<&mut Assistant>, text: QString);
        /// The model is thinking; the CLI's estimate of the tokens so far.
        #[qsignal]
        fn thinking_tokens(self: Pin<&mut Assistant>, estimated: i32);
        /// A user message went in, live or replayed.
        #[qsignal]
        fn user_message(self: Pin<&mut Assistant>, text: QString);
        /// A permission request was answered, live or replayed.
        #[qsignal]
        fn answered(self: Pin<&mut Assistant>, request_id: QString, allowed: bool);
        /// A permission request can no longer be answered (the process went).
        #[qsignal]
        fn expired(self: Pin<&mut Assistant>, request_id: QString);
        /// A tool call's input, once the message that carries it is complete.
        #[qsignal]
        fn tool_input(self: Pin<&mut Assistant>, id: QString, name: QString, input_json: QString);
        /// What a tool answered.
        #[qsignal]
        fn tool_result(self: Pin<&mut Assistant>, id: QString, text: QString, is_error: bool);
        /// The agent wants to use a tool that needs a decision; `meta_json` carries the
        /// tool use id, the display name and the permission suggestions.
        #[qsignal]
        fn permission_asked(
            self: Pin<&mut Assistant>,
            request_id: QString,
            tool: QString,
            input_json: QString,
            description: QString,
            meta_json: QString,
        );
        /// A turn ended, well or not.
        #[qsignal]
        fn turn_done(
            self: Pin<&mut Assistant>,
            ok: bool,
            cost_usd: f64,
            num_turns: i32,
            text: QString,
            duration_ms: i32,
        );
        /// The permission mode in force changed.
        #[qsignal]
        fn mode_changed(self: Pin<&mut Assistant>, mode: QString);
        /// Something worth a line in the conversation.
        #[qsignal]
        fn notice(self: Pin<&mut Assistant>, text: QString);
        /// The process ended: `reason` is `exit`, `idle`, `stop` or `signal`, and
        /// `message` says it in words with the tail of its stderr. An `idle` or `stop`
        /// exit is the ordinary end of a session that is not being talked to.
        #[qsignal]
        fn exited(self: Pin<&mut Assistant>, code: i32, reason: QString, message: QString);
    }

    impl cxx_qt::Threading for Assistant {}
}

/// What one line from the host means to the client.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientEvent {
    Hello(State),
    CaughtUp,
    Wire(Event),
    UserMessage(String),
    Answered { request_id: String, allowed: bool },
    Host(HostEvent),
    Status(State),
    Error(String),
}

/// The text of a user message line: its text blocks joined.
fn user_text(line: &Value) -> String {
    match &line["message"]["content"] {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"].as_str() == Some("text"))
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// One host message as client events. A replay skips what only mattered live: the
/// deltas, the thinking estimates, the request status.
pub fn translate(msg: &HostMsg, replaying: bool) -> Vec<ClientEvent> {
    match msg {
        HostMsg::Hello { state, .. } => vec![ClientEvent::Hello(state.clone())],
        HostMsg::CaughtUp { .. } => vec![ClientEvent::CaughtUp],
        HostMsg::Claude { line, .. } => wire::parse_value(line)
            .into_iter()
            .filter(|event| {
                !replaying
                    || !matches!(
                        event,
                        Event::TextDelta(_) | Event::ThinkingTokens(_) | Event::Status(_)
                    )
            })
            .map(ClientEvent::Wire)
            .collect(),
        HostMsg::Sent { line, .. } => match line["type"].as_str().unwrap_or("") {
            "user" => vec![ClientEvent::UserMessage(user_text(line))],
            "control_response" => {
                let response = &line["response"];
                vec![ClientEvent::Answered {
                    request_id: response["request_id"].as_str().unwrap_or("").to_string(),
                    allowed: response["response"]["behavior"].as_str() == Some("allow"),
                }]
            }
            _ => Vec::new(),
        },
        HostMsg::Host { event, .. } => vec![ClientEvent::Host(event.clone())],
        HostMsg::Status { state } => vec![ClientEvent::Status(state.clone())],
        HostMsg::Error { message } => vec![ClientEvent::Error(message.clone())],
    }
}

/// The request a QML `create` describes, from its JSON.
fn parse_create(options_json: &str) -> Result<NewSession, String> {
    let v: Value = serde_json::from_str(options_json).map_err(|e| format!("create: {e}"))?;
    let str_of = |key: &str| v[key].as_str().unwrap_or("").trim().to_string();
    let cwd = str_of("cwd");
    if cwd.is_empty() {
        return Err("create: a cwd is needed".into());
    }
    let allowed_tools = match v["allowedTools"].as_str().unwrap_or("none") {
        "reads" => read_tools(),
        _ => Vec::new(),
    };
    let title = {
        let t = str_of("title");
        if t.is_empty() {
            std::path::Path::new(&cwd)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "agent".to_string())
        } else {
            t
        }
    };
    let optional = |key: &str| Some(str_of(key)).filter(|s| !s.is_empty());
    let options = SpawnOptions {
        permission_mode: optional("permissionMode").unwrap_or_else(|| "default".to_string()),
        model: optional("model"),
        strict_mcp: v["strictMcp"].as_bool().unwrap_or(false),
        allowed_tools,
        system_prompt: str_of("systemPrompt"),
        mcp_url: optional("mcpUrl").unwrap_or_else(|| "http://127.0.0.1:4174/mcp".to_string()),
        name: Some(title.clone()),
        idle_timeout_secs: v["idleTimeout"].as_u64().unwrap_or(0),
        resume: optional("resume"),
    };
    options.validate()?;
    Ok(NewSession {
        cwd,
        title,
        page: optional("page"),
        options,
    })
}

/// The Rust side of [`qobject::Assistant`].
pub struct AssistantRust {
    available: bool,
    running: bool,
    busy: bool,
    status: QString,
    session_id: QString,
    claude_session_id: QString,
    child_state: QString,
    permission_mode: QString,
    model: QString,
    pending_count: i32,
    attached_state: QString,
    connection: Option<Connection>,
    /// Lines from a connection that was replaced are dropped by this.
    generation: u64,
    replaying: bool,
    /// Requests asked and not answered, as far as the log says.
    pending: BTreeSet<String>,
    /// What the host said was pending when the connection opened.
    hello_pending: Vec<String>,
    requests: u64,
}

impl Default for AssistantRust {
    fn default() -> Self {
        let available = claude_binary().is_some() && Runner::from_env().available();
        Self {
            available,
            running: false,
            busy: false,
            status: QString::from(if available {
                "not started"
            } else {
                "Claude Code is not installed"
            }),
            session_id: QString::default(),
            claude_session_id: QString::default(),
            child_state: QString::from("none"),
            permission_mode: QString::from("default"),
            model: QString::default(),
            pending_count: 0,
            attached_state: QString::from("detached"),
            connection: None,
            generation: 0,
            replaying: false,
            pending: BTreeSet::new(),
            hello_pending: Vec::new(),
            requests: 0,
        }
    }
}

impl qobject::Assistant {
    /// See the bridge.
    pub fn create(mut self: Pin<&mut Self>, options_json: &QString) {
        let request = match parse_create(&options_json.to_string()) {
            Ok(r) => r,
            Err(e) => {
                self.as_mut().created(QString::default(), QString::from(&e));
                return;
            }
        };
        self.as_mut().set_status(QString::from("starting"));
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let result = launch::start_new(&Runner::from_env(), request);
            let _ = qt.queue(move |mut assistant| match result {
                Ok(id) => assistant
                    .as_mut()
                    .created(QString::from(id.as_str()), QString::default()),
                Err(e) => {
                    assistant.as_mut().set_status(QString::from(&e));
                    assistant
                        .as_mut()
                        .created(QString::default(), QString::from(&e));
                }
            });
        });
    }

    /// See the bridge.
    pub fn attach(mut self: Pin<&mut Self>, id: &QString) {
        let id = match SessionId::parse(&id.to_string()) {
            Ok(id) => id,
            Err(e) => {
                self.as_mut().notice(QString::from(&e));
                return;
            }
        };
        self.as_mut().detach();
        let generation = self.rust().generation;
        self.as_mut().set_session_id(QString::from(id.as_str()));
        self.as_mut().set_attached_state(QString::from("attaching"));
        self.as_mut().set_status(QString::from("attaching"));
        self.as_mut().rust_mut().pending.clear();
        self.as_mut().set_pending_count(0);
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let socket = paths::socket_path(&id);
            let connected = match Connection::connect(&socket) {
                Ok(c) => Ok(c),
                Err(_) => launch::start_existing(&Runner::from_env(), &id).and_then(|()| {
                    Connection::connect_with_retry(&socket, Duration::from_secs(5))
                        .map_err(|e| format!("connecting to the host: {e}"))
                }),
            };
            let mut connection = match connected {
                Ok(c) => c,
                Err(e) => {
                    let _ = qt.queue(move |mut assistant| {
                        if assistant.rust().generation == generation {
                            assistant
                                .as_mut()
                                .set_attached_state(QString::from("detached"));
                            assistant.as_mut().set_status(QString::from("detached"));
                            assistant.as_mut().host_exited(QString::from(&e));
                        }
                    });
                    return;
                }
            };
            let hello = match connection.read_line_timeout(Duration::from_secs(5)) {
                Ok(Some(line)) => line,
                _ => {
                    let _ = qt.queue(move |mut assistant| {
                        if assistant.rust().generation == generation {
                            assistant
                                .as_mut()
                                .set_attached_state(QString::from("detached"));
                            assistant
                                .as_mut()
                                .host_exited(QString::from("the host said nothing"));
                        }
                    });
                    return;
                }
            };
            if connection.send(&ClientMsg::Attach { since: 0 }).is_err() {
                let _ = qt.queue(move |mut assistant| {
                    if assistant.rust().generation == generation {
                        assistant
                            .as_mut()
                            .set_attached_state(QString::from("detached"));
                        assistant
                            .as_mut()
                            .host_exited(QString::from("the host took no attach"));
                    }
                });
                return;
            }
            let reader_qt = qt.clone();
            let spawned = connection.spawn_reader(move |incoming| {
                let _ = reader_qt.queue(move |mut assistant| {
                    if assistant.rust().generation == generation {
                        assistant.as_mut().handle(incoming);
                    }
                });
            });
            let id_text = id.as_str().to_string();
            let _ = qt.queue(move |mut assistant| {
                if assistant.rust().generation != generation {
                    connection.close();
                    return;
                }
                match spawned {
                    Ok(()) => {
                        assistant.as_mut().rust_mut().connection = Some(connection);
                        assistant.as_mut().rust_mut().replaying = true;
                        assistant.as_mut().set_running(true);
                        assistant
                            .as_mut()
                            .set_attached_state(QString::from("attached"));
                        assistant.as_mut().set_status(QString::from("replaying"));
                        assistant.as_mut().attached(QString::from(&id_text));
                        assistant.as_mut().handle(Incoming::Line(hello));
                    }
                    Err(e) => {
                        assistant
                            .as_mut()
                            .set_attached_state(QString::from("detached"));
                        assistant
                            .as_mut()
                            .host_exited(QString::from(&e.to_string()));
                    }
                }
            });
        });
    }

    /// See the bridge.
    pub fn detach(mut self: Pin<&mut Self>) {
        let generation = self.rust().generation + 1;
        self.as_mut().rust_mut().generation = generation;
        if let Some(connection) = self.as_mut().rust_mut().connection.take() {
            connection.close();
        }
        self.as_mut().rust_mut().replaying = false;
        self.as_mut().set_running(false);
        self.as_mut().set_busy(false);
        self.as_mut().set_attached_state(QString::from("detached"));
        self.as_mut().set_status(QString::from("detached"));
    }

    fn write(mut self: Pin<&mut Self>, line: &str) -> bool {
        let Ok(line) = serde_json::from_str::<Value>(line) else {
            return false;
        };
        let mut rust = self.as_mut().rust_mut();
        let Some(connection) = rust.connection.as_mut() else {
            return false;
        };
        connection.send(&ClientMsg::Send { line }).is_ok()
    }

    /// See the bridge.
    pub fn send(mut self: Pin<&mut Self>, text: &QString) -> bool {
        let line = wire::user_message(&text.to_string());
        if !self.as_mut().write(&line) {
            return false;
        }
        self.as_mut().set_busy(true);
        self.as_mut().set_status(QString::from("working"));
        true
    }

    /// See the bridge.
    pub fn answer(
        mut self: Pin<&mut Self>,
        request_id: &QString,
        allow: bool,
        input_json: &QString,
        extra_json: &QString,
    ) -> bool {
        let line = wire::control_response(
            &request_id.to_string(),
            allow,
            &input_json.to_string(),
            &extra_json.to_string(),
        );
        let ok = self.as_mut().write(&line);
        if ok {
            let id = request_id.to_string();
            self.as_mut().rust_mut().pending.remove(&id);
            let count = self.rust().pending.len() as i32;
            self.as_mut().set_pending_count(count);
            self.as_mut().set_status(QString::from("working"));
        }
        ok
    }

    fn next_request(mut self: Pin<&mut Self>) -> u64 {
        let n = self.rust().requests + 1;
        self.as_mut().rust_mut().requests = n;
        n
    }

    /// See the bridge.
    pub fn interrupt(mut self: Pin<&mut Self>) -> bool {
        let n = self.as_mut().next_request();
        self.write(&wire::interrupt_request(n))
    }

    /// See the bridge.
    pub fn change_mode(mut self: Pin<&mut Self>, mode: &QString) -> bool {
        let n = self.as_mut().next_request();
        self.write(&wire::set_permission_mode_request(n, &mode.to_string()))
    }

    /// See the bridge.
    pub fn change_model(mut self: Pin<&mut Self>, model: &QString) -> bool {
        let n = self.as_mut().next_request();
        self.write(&wire::set_model_request(n, &model.to_string()))
    }

    /// See the bridge.
    pub fn stop(mut self: Pin<&mut Self>) {
        let sent = {
            let mut rust = self.as_mut().rust_mut();
            rust.connection
                .as_mut()
                .is_some_and(|c| c.send(&ClientMsg::Stop).is_ok())
        };
        if sent {
            self.as_mut().set_status(QString::from("stopping"));
        }
    }

    /// See the bridge.
    pub fn remove(mut self: Pin<&mut Self>) {
        let id = self.rust().session_id.to_string();
        self.as_mut().detach();
        let Ok(id) = SessionId::parse(&id) else {
            return;
        };
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let runner = Runner::from_env();
            if registry::alive(&id) {
                let _ = launch::stop(&runner, &id);
            }
            let result = registry::remove_in(&paths::state_dir(), &id);
            let _ = qt.queue(move |mut assistant| {
                if let Err(e) = result {
                    assistant.as_mut().notice(QString::from(&format!(
                        "The session could not be removed: {e}"
                    )));
                }
            });
        });
    }

    /// See the bridge.
    pub fn diff(&self, before: &QString, after: &QString) -> QString {
        QString::from(&crate::diff::to_json(&crate::diff::diff_lines(
            &before.to_string(),
            &after.to_string(),
        )))
    }

    /// A line from the host or the end of the connection, on the Qt thread.
    fn handle(mut self: Pin<&mut Self>, incoming: Incoming) {
        match incoming {
            Incoming::Line(line) => {
                let Ok(msg) = serde_json::from_str::<HostMsg>(&line) else {
                    return;
                };
                let replaying = self.rust().replaying;
                for event in translate(&msg, replaying) {
                    self.as_mut().emit(event);
                }
            }
            Incoming::Closed(why) => {
                self.as_mut().rust_mut().connection = None;
                self.as_mut().rust_mut().replaying = false;
                self.as_mut().set_running(false);
                self.as_mut().set_busy(false);
                self.as_mut().set_attached_state(QString::from("detached"));
                self.as_mut().set_status(QString::from("detached"));
                self.as_mut().host_exited(QString::from(&why));
            }
        }
    }

    fn apply_state(mut self: Pin<&mut Self>, state: &State) {
        self.as_mut().set_child_state(QString::from(&state.child));
        self.as_mut().set_claude_session_id(QString::from(
            state.claude_session_id.as_deref().unwrap_or(""),
        ));
        self.as_mut()
            .set_permission_mode(QString::from(&state.permission_mode));
        self.as_mut()
            .set_model(QString::from(state.model.as_deref().unwrap_or("")));
        self.as_mut().set_busy(state.child == "working");
    }

    fn expire_all(mut self: Pin<&mut Self>) {
        let pending: Vec<String> = self.rust().pending.iter().cloned().collect();
        self.as_mut().rust_mut().pending.clear();
        self.as_mut().set_pending_count(0);
        for id in pending {
            self.as_mut().expired(QString::from(&id));
        }
    }

    fn emit(mut self: Pin<&mut Self>, event: ClientEvent) {
        match event {
            ClientEvent::Hello(state) => {
                self.as_mut().rust_mut().hello_pending = state.pending.clone();
                self.as_mut().apply_state(&state);
            }
            ClientEvent::CaughtUp => {
                let live: Vec<String> = self.rust().hello_pending.clone();
                let dead: Vec<String> = self
                    .rust()
                    .pending
                    .iter()
                    .filter(|id| !live.contains(id))
                    .cloned()
                    .collect();
                for id in dead {
                    self.as_mut().rust_mut().pending.remove(&id);
                    self.as_mut().expired(QString::from(&id));
                }
                let count = self.rust().pending.len() as i32;
                self.as_mut().set_pending_count(count);
                self.as_mut().rust_mut().replaying = false;
                let status = match self.rust().child_state.to_string().as_str() {
                    "working" => "working",
                    "none" => "sleeping",
                    "stopping" => "stopping",
                    _ if count > 0 => "asking",
                    _ => "ready",
                };
                self.as_mut().set_status(QString::from(status));
                self.as_mut().replay_done();
            }
            ClientEvent::Status(state) => self.as_mut().apply_state(&state),
            ClientEvent::UserMessage(text) => {
                if !self.rust().replaying {
                    self.as_mut().set_busy(true);
                    self.as_mut().set_status(QString::from("working"));
                }
                self.as_mut().user_message(QString::from(&text));
            }
            ClientEvent::Answered {
                request_id,
                allowed,
            } => {
                self.as_mut().rust_mut().pending.remove(&request_id);
                let count = self.rust().pending.len() as i32;
                self.as_mut().set_pending_count(count);
                self.as_mut().answered(QString::from(&request_id), allowed);
            }
            ClientEvent::Host(event) => match event {
                HostEvent::Started { resumed } => {
                    self.as_mut().expire_all();
                    self.as_mut().host_started(resumed);
                }
                HostEvent::ChildStarted { .. } => {
                    self.as_mut().set_child_state(QString::from("ready"));
                    if !self.rust().replaying {
                        self.as_mut().set_status(QString::from("ready"));
                    }
                }
                HostEvent::ChildExited {
                    code,
                    signal,
                    reason,
                    stderr,
                } => {
                    self.as_mut().expire_all();
                    self.as_mut().set_child_state(QString::from("none"));
                    self.as_mut().set_busy(false);
                    if !self.rust().replaying {
                        self.as_mut().set_status(QString::from("sleeping"));
                    }
                    let mut message = match (reason.as_str(), signal) {
                        ("idle", _) => "stopped after sitting idle".to_string(),
                        ("stop", _) => "stopped".to_string(),
                        (_, Some(signal)) => format!("ended by signal {signal}"),
                        _ => "exited".to_string(),
                    };
                    if !stderr.is_empty() {
                        message.push_str(": ");
                        message.push_str(&stderr);
                    }
                    self.as_mut().exited(
                        code.unwrap_or(-1),
                        QString::from(&reason),
                        QString::from(&message),
                    );
                }
                HostEvent::Stopping { .. } => {
                    self.as_mut().set_status(QString::from("stopping"));
                }
                HostEvent::Note { text } => self.as_mut().notice(QString::from(&text)),
            },
            ClientEvent::Error(message) => {
                self.as_mut()
                    .notice(QString::from(&format!("The host answered: {message}")));
            }
            ClientEvent::Wire(event) => self.as_mut().emit_wire(event),
        }
    }

    fn emit_wire(mut self: Pin<&mut Self>, event: Event) {
        let replaying = self.rust().replaying;
        match event {
            Event::Init {
                session_id,
                model,
                permission_mode,
            } => {
                self.as_mut()
                    .set_claude_session_id(QString::from(&session_id));
                if !model.is_empty() {
                    self.as_mut().set_model(QString::from(&model));
                }
                if !permission_mode.is_empty() {
                    self.as_mut()
                        .set_permission_mode(QString::from(&permission_mode));
                }
                if !replaying {
                    self.as_mut().set_busy(true);
                    self.as_mut().set_status(QString::from("working"));
                }
                self.as_mut().started(QString::from(&session_id));
            }
            Event::Status(status) => {
                if !replaying {
                    self.as_mut().set_status(QString::from(&status));
                }
            }
            Event::ModeChanged(mode) => {
                self.as_mut().set_permission_mode(QString::from(&mode));
                self.as_mut().mode_changed(QString::from(&mode));
            }
            Event::ThinkingTokens(n) => {
                if !replaying {
                    self.as_mut().set_status(QString::from("thinking"));
                }
                self.as_mut()
                    .thinking_tokens(i32::try_from(n).unwrap_or(i32::MAX));
            }
            Event::BlockStart { kind, name, id } => {
                if kind == "thinking" && !replaying {
                    self.as_mut().set_status(QString::from("thinking"));
                }
                self.as_mut().block_started(
                    QString::from(&kind),
                    QString::from(&name),
                    QString::from(&id),
                );
            }
            Event::TextDelta(text) => self.as_mut().text_delta(QString::from(&text)),
            Event::TextFinal(text) => self.as_mut().text_final(QString::from(&text)),
            Event::ToolInput { id, name, input } => {
                if !replaying {
                    self.as_mut()
                        .set_status(QString::from(&format!("running {name}")));
                }
                self.as_mut().tool_input(
                    QString::from(&id),
                    QString::from(&name),
                    QString::from(&input),
                );
            }
            Event::ToolResult { id, text, is_error } => {
                self.as_mut()
                    .tool_result(QString::from(&id), QString::from(&text), is_error);
            }
            Event::Permission {
                request_id,
                tool,
                input,
                description,
                meta,
            } => {
                self.as_mut().rust_mut().pending.insert(request_id.clone());
                let count = self.rust().pending.len() as i32;
                self.as_mut().set_pending_count(count);
                if !replaying {
                    self.as_mut()
                        .set_status(QString::from(&format!("asking to use {tool}")));
                }
                self.as_mut().permission_asked(
                    QString::from(&request_id),
                    QString::from(&tool),
                    QString::from(&input),
                    QString::from(&description),
                    QString::from(&meta),
                );
            }
            Event::ControlAck { ok, error, .. } => {
                if !ok && !error.is_empty() {
                    self.as_mut().notice(QString::from(&error));
                }
            }
            Event::TurnDone {
                ok,
                cost_usd,
                num_turns,
                text,
                duration_ms,
            } => {
                if !replaying {
                    self.as_mut().set_busy(false);
                    self.as_mut().set_status(QString::from(if ok {
                        "ready"
                    } else {
                        "the turn failed"
                    }));
                }
                self.as_mut().turn_done(
                    ok,
                    cost_usd,
                    i32::try_from(num_turns).unwrap_or(i32::MAX),
                    QString::from(&text),
                    i32::try_from(duration_ms).unwrap_or(i32::MAX),
                );
            }
            Event::RateLimit { .. } => {}
            Event::Notice(text) => self.as_mut().notice(QString::from(&text)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::protocol::now;

    fn parsed(line: &str) -> HostMsg {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn a_replay_skips_what_only_mattered_live() {
        let delta = parsed(
            r#"{"type":"claude","t":"t","line":{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"a"}}}}"#,
        );
        assert_eq!(
            translate(&delta, false),
            vec![ClientEvent::Wire(Event::TextDelta("a".into()))]
        );
        assert!(translate(&delta, true).is_empty());
        let status = parsed(
            r#"{"type":"claude","seq":3,"t":"t","line":{"type":"system","subtype":"status","status":"requesting"}}"#,
        );
        assert!(translate(&status, true).is_empty());
        assert_eq!(
            translate(&status, false),
            vec![ClientEvent::Wire(Event::Status("requesting".into()))]
        );
        let text = parsed(
            r#"{"type":"claude","seq":4,"t":"t","line":{"type":"assistant","message":{"content":[{"type":"text","text":"hello"}]}}}"#,
        );
        assert_eq!(
            translate(&text, true),
            vec![ClientEvent::Wire(Event::TextFinal("hello".into()))]
        );
    }

    #[test]
    fn sent_lines_become_user_messages_and_answers() {
        let user = parsed(
            r#"{"type":"sent","seq":8,"t":"t","line":{"type":"user","message":{"role":"user","content":[{"type":"text","text":"What is this page about?"}]}}}"#,
        );
        assert_eq!(
            translate(&user, true),
            vec![ClientEvent::UserMessage("What is this page about?".into())]
        );
        let allow = parsed(
            r#"{"type":"sent","seq":30,"t":"t","line":{"type":"control_response","response":{"subtype":"success","request_id":"8366049a","response":{"behavior":"allow","updatedInput":{}}}}}"#,
        );
        assert_eq!(
            translate(&allow, false),
            vec![ClientEvent::Answered {
                request_id: "8366049a".into(),
                allowed: true
            }]
        );
        let deny = parsed(
            r#"{"type":"sent","seq":31,"t":"t","line":{"type":"control_response","response":{"subtype":"success","request_id":"r","response":{"behavior":"deny","message":"no"}}}}"#,
        );
        assert!(matches!(
            translate(&deny, false)[0],
            ClientEvent::Answered { allowed: false, .. }
        ));
        let interrupt = parsed(
            r#"{"type":"sent","seq":32,"t":"t","line":{"type":"control_request","request_id":"rusty-interrupt-1","request":{"subtype":"interrupt"}}}"#,
        );
        assert!(translate(&interrupt, false).is_empty());
    }

    #[test]
    fn host_lines_carry_their_events() {
        let hello = parsed(
            r#"{"type":"hello","protocol":1,"id":"x","seq":2,"state":{"child":"ready","pending":["p1"],"claude_session_id":null,"permission_mode":"default","model":null,"clients":1,"pid":1}}"#,
        );
        assert!(
            matches!(translate(&hello, false)[0], ClientEvent::Hello(ref s) if s.pending == vec!["p1".to_string()])
        );
        let caught = parsed(r#"{"type":"caught_up","seq":2}"#);
        assert_eq!(translate(&caught, true), vec![ClientEvent::CaughtUp]);
        let exited = parsed(
            r#"{"type":"host","seq":9,"t":"t","event":"child_exited","code":3,"signal":null,"reason":"exit","stderr":"oops"}"#,
        );
        assert_eq!(
            translate(&exited, false),
            vec![ClientEvent::Host(HostEvent::ChildExited {
                code: Some(3),
                signal: None,
                reason: "exit".into(),
                stderr: "oops".into()
            })]
        );
        let error = parsed(r#"{"type":"error","message":"no process to answer"}"#);
        assert_eq!(
            translate(&error, false),
            vec![ClientEvent::Error("no process to answer".into())]
        );
        let _ = now();
    }

    #[test]
    fn create_options_come_from_the_pane_and_the_tab() {
        let pane = parse_create(r#"{"cwd":"/home/x","title":"Orbit","page":"projects/orbit","permissionMode":"default","strictMcp":true,"allowedTools":"reads","systemPrompt":"The page.","mcpUrl":"http://127.0.0.1:4174/mcp","idleTimeout":600,"resume":""}"#).unwrap();
        assert_eq!(pane.title, "Orbit");
        assert_eq!(pane.page.as_deref(), Some("projects/orbit"));
        assert!(pane.options.strict_mcp);
        assert!(pane
            .options
            .allowed_tools
            .contains(&"mcp__rusty__brain_read_page".to_string()));
        assert_eq!(pane.options.idle_timeout_secs, 600);
        assert!(pane.options.resume.is_none());
        assert_eq!(pane.options.name.as_deref(), Some("Orbit"));
        let tab = parse_create(r#"{"cwd":"/srv/stacks/rusty-v3","permissionMode":"acceptEdits","model":"sonnet","resume":"abc"}"#).unwrap();
        assert_eq!(tab.title, "rusty-v3");
        assert!(!tab.options.strict_mcp);
        assert!(tab.options.allowed_tools.is_empty());
        assert_eq!(tab.options.model.as_deref(), Some("sonnet"));
        assert_eq!(tab.options.resume.as_deref(), Some("abc"));
        assert_eq!(tab.options.mcp_url, "http://127.0.0.1:4174/mcp");
        assert!(parse_create(r#"{"title":"no cwd"}"#).is_err());
        assert!(parse_create(r#"{"cwd":"/x","permissionMode":"bogus"}"#).is_err());
        assert!(parse_create("not json").is_err());
    }

    #[test]
    fn user_text_joins_the_text_blocks() {
        let line: Value = serde_json::from_str(r#"{"message":{"content":[{"type":"text","text":"a"},{"type":"image"},{"type":"text","text":"b"}]}}"#).unwrap();
        assert_eq!(user_text(&line), "a\nb");
        let plain: Value = serde_json::from_str(r#"{"message":{"content":"just text"}}"#).unwrap();
        assert_eq!(user_text(&plain), "just text");
    }
}
