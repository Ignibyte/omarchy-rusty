//! The session host: `rusty agent host --id <id>`, what a session's transient unit runs.
//! It owns the one `claude` process (its stdin and stdout), appends every line it
//! forwards or receives to the session's log, and serves clients over the session's
//! socket: replay after a sequence number, then live. It respawns the process with
//! `--resume` after an exit, stops it when the session sits idle, notifies the desktop
//! when nobody is attached, and ends the process in order (interrupt, EOF, SIGTERM,
//! SIGKILL) when told to stop or when the unit sends SIGTERM.

use std::collections::{BTreeSet, HashMap};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use super::log::EventLog;
use super::protocol::{now, ClientMsg, HostEvent, HostMsg, State, PROTOCOL};
use super::registry::{self, Entry};
use super::spawn::{build_args, claude_binary, SessionArg};
use super::wire::{self, Event};
use super::{paths, SessionId};

/// The most of the process's stderr kept for the exit event.
const STDERR_TAIL: usize = 2000;

/// How long each step of the stop sequence waits before the next.
#[derive(Debug, Clone, Copy)]
pub struct Grace {
    pub interrupt: Duration,
    pub eof: Duration,
    pub term: Duration,
}

impl Default for Grace {
    fn default() -> Self {
        Self {
            interrupt: Duration::from_secs(5),
            eof: Duration::from_secs(3),
            term: Duration::from_secs(3),
        }
    }
}

/// A desktop notification, or a test's record of one.
pub type Notifier = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// What a host runs with.
#[derive(Clone)]
pub struct HostConfig {
    pub id: SessionId,
    pub state_dir: PathBuf,
    pub run_dir: PathBuf,
    /// The `claude` to run; none means find it.
    pub claude: Option<PathBuf>,
    pub grace: Grace,
    pub notify: Notifier,
    /// Spawn the process at start; a test may wait for the first message instead.
    pub eager_child: bool,
}

impl HostConfig {
    /// The configuration a unit runs with: the directories from the environment, the
    /// desktop notified through `notify-send`.
    pub fn from_env(id: SessionId) -> Self {
        Self {
            id,
            state_dir: paths::state_dir(),
            run_dir: paths::run_dir(),
            claude: None,
            grace: Grace::default(),
            notify: Arc::new(crate::terminals::send_notification),
            eager_child: true,
        }
    }
}

/// What reaches the loop: a connection, a client's line, a client gone, the process's
/// line, its exit, a signal.
#[derive(Debug)]
enum Msg {
    Accepted(UnixStream),
    Client(u64, String),
    ClientGone(u64),
    ChildLine(u64, String),
    ChildExited(u64, Option<i32>, Option<i32>, String),
    Signal,
}

struct Client {
    tx: mpsc::UnboundedSender<String>,
    attached: bool,
    /// The task that writes to the socket; awaited at the end so the last lines of a
    /// stopping host reach its clients before the runtime goes.
    writer: tokio::task::JoinHandle<()>,
}

struct ChildHandle {
    generation: u64,
    pid: u32,
    stdin: Option<tokio::process::ChildStdin>,
    working: bool,
    /// A user message went in during this process's life.
    saw_user: bool,
    arg: SessionArg,
}

/// Whether the loop goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continue,
    Stop(&'static str),
}

/// What the loop does next.
enum Next {
    Msg(Msg),
    Idle,
    Closed,
}

struct Host {
    config: HostConfig,
    entry: Entry,
    log: EventLog,
    tx: mpsc::UnboundedSender<Msg>,
    rx: mpsc::UnboundedReceiver<Msg>,
    clients: HashMap<u64, Client>,
    next_client: u64,
    child: Option<ChildHandle>,
    generations: u64,
    pending: BTreeSet<String>,
    claude_session_id: Option<String>,
    permission_mode: String,
    model: Option<String>,
    idle_deadline: Option<Instant>,
    /// The Rusty id was offered as `--session-id` once; a later fresh conversation
    /// takes a minted id.
    offered_own_id: bool,
    /// Respawns without a user message in between, to keep a broken start from looping.
    silent_respawns: u32,
    /// The host's own control requests, numbered.
    requests: u64,
    /// Why the host is ending, once it is.
    stopping: Option<&'static str>,
    /// Why the process is being ended, while it is.
    child_stop_reason: Option<&'static str>,
    /// A signal that arrived while the process was being ended for another reason.
    pending_signal: bool,
}

/// Run the host until it is told to stop; the unit's main process.
pub fn serve(config: HostConfig) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("the runtime: {e}"))?;
    runtime.block_on(serve_async(config))
}

async fn serve_async(config: HostConfig) -> Result<(), String> {
    let id = config.id.clone();
    let entry = registry::read_entry_in(&config.state_dir, &id)
        .map_err(|e| format!("no entry for session {id}: {e}"))?;
    let log_path = paths::log_path_in(&config.state_dir, &id);
    let log = EventLog::open(&log_path).map_err(|e| format!("opening the log: {e}"))?;
    let socket = bind_socket(&config.run_dir, &id)?;
    let listener = socket.listener;
    let (tx, rx) = mpsc::unbounded_channel::<Msg>();
    let accept_tx = tx.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            if accept_tx.send(Msg::Accepted(stream)).is_err() {
                break;
            }
        }
    });
    for kind in [
        tokio::signal::unix::SignalKind::terminate(),
        tokio::signal::unix::SignalKind::interrupt(),
    ] {
        if let Ok(mut signal) = tokio::signal::unix::signal(kind) {
            let signal_tx = tx.clone();
            tokio::spawn(async move {
                if signal.recv().await.is_some() {
                    let _ = signal_tx.send(Msg::Signal);
                }
            });
        }
    }
    let resumed = log.is_resumed();
    let mut host = Host {
        claude_session_id: entry.claude_session_id.clone(),
        permission_mode: entry.options.permission_mode.clone(),
        model: entry.options.model.clone(),
        offered_own_id: entry.claude_session_id.is_some(),
        config,
        entry,
        log,
        tx,
        rx,
        clients: HashMap::new(),
        next_client: 1,
        child: None,
        generations: 0,
        pending: BTreeSet::new(),
        idle_deadline: None,
        silent_respawns: 0,
        requests: 0,
        stopping: None,
        child_stop_reason: None,
        pending_signal: false,
    };
    host.log_host(HostEvent::Started { resumed });
    host.set_state("idle");
    if host.config.eager_child {
        host.spawn_child().await;
    }
    let reason = loop {
        match host.next().await {
            Next::Closed => break "closed",
            Next::Msg(msg) => {
                if let Flow::Stop(reason) = host.handle(msg).await {
                    break reason;
                }
            }
            Next::Idle => host.idle_fired().await,
        }
        if host.pending_signal {
            break "signal";
        }
    };
    host.shutdown(reason).await;
    host.drain_clients().await;
    let _ = std::fs::remove_file(&socket.path);
    Ok(())
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
        None => std::future::pending().await,
    }
}

struct BoundSocket {
    listener: tokio::net::UnixListener,
    path: PathBuf,
}

/// The session's socket: the directory private to the user, a stale file removed, a
/// live one refused.
fn bind_socket(run_dir: &Path, id: &SessionId) -> Result<BoundSocket, String> {
    std::fs::create_dir_all(run_dir).map_err(|e| format!("creating {}: {e}", run_dir.display()))?;
    let _ = std::fs::set_permissions(run_dir, std::fs::Permissions::from_mode(0o700));
    let path = paths::socket_path_in(run_dir, id);
    paths::check_socket_path(&path)?;
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            return Err(format!("a host for session {id} is already running"));
        }
        let _ = std::fs::remove_file(&path);
    }
    let listener = tokio::net::UnixListener::bind(&path)
        .map_err(|e| format!("binding {}: {e}", path.display()))?;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    Ok(BoundSocket { listener, path })
}

impl Host {
    /// The next thing to do: a message, the idle deadline, or the end of the channel.
    async fn next(&mut self) -> Next {
        let idle = self.idle_deadline;
        tokio::select! {
            msg = self.rx.recv() => match msg {
                Some(msg) => Next::Msg(msg),
                None => Next::Closed,
            },
            _ = sleep_until(idle), if idle.is_some() => Next::Idle,
        }
    }

    fn attached_count(&self) -> usize {
        self.clients.values().filter(|c| c.attached).count()
    }

    fn state(&self) -> State {
        State {
            child: match (&self.child, self.stopping) {
                (_, Some(_)) => "stopping".to_string(),
                (None, None) => "none".to_string(),
                (Some(c), None) if c.working => "working".to_string(),
                (Some(_), None) => "ready".to_string(),
            },
            pending: self.pending.iter().cloned().collect(),
            claude_session_id: self.claude_session_id.clone(),
            permission_mode: self.permission_mode.clone(),
            model: self.model.clone(),
            clients: self.attached_count(),
            pid: std::process::id(),
        }
    }

    /// Rewrite the entry with the state word and what the process announced.
    fn set_state(&mut self, state: &str) {
        self.entry.state = state.to_string();
        self.entry.updated = now();
        self.entry.claude_session_id = self.claude_session_id.clone();
        self.entry.options.permission_mode = self.permission_mode.clone();
        self.entry.options.model = self.model.clone();
        if let Err(e) = registry::write_entry_in(&self.config.state_dir, &self.entry) {
            eprintln!("rusty agent host: writing the entry: {e}");
        }
    }

    fn broadcast(&mut self, line: &str) {
        self.clients
            .retain(|_, client| !client.attached || client.tx.send(line.to_string()).is_ok());
    }

    /// Log a message and hand it to every attached client.
    fn record(&mut self, mut msg: HostMsg) {
        match self.log.append(&mut msg) {
            Ok(line) => self.broadcast(&line),
            Err(e) => eprintln!("rusty agent host: writing the log: {e}"),
        }
    }

    fn log_host(&mut self, event: HostEvent) {
        self.record(HostMsg::Host {
            seq: 0,
            t: now(),
            event,
        });
    }

    fn send_to(&mut self, client_id: u64, msg: &HostMsg) {
        if let Ok(line) = serde_json::to_string(msg) {
            if let Some(client) = self.clients.get(&client_id) {
                let _ = client.tx.send(line);
            }
        }
    }

    fn notify(&self, title: &str, body: &str) {
        if self.attached_count() == 0 {
            (self.config.notify)(title, body);
        }
    }

    async fn handle(&mut self, msg: Msg) -> Flow {
        match msg {
            Msg::Accepted(stream) => {
                self.accept(stream);
                Flow::Continue
            }
            Msg::Client(id, line) => self.client_line(id, &line).await,
            Msg::ClientGone(id) => {
                self.clients.remove(&id);
                Flow::Continue
            }
            Msg::ChildLine(generation, line) => {
                if self
                    .child
                    .as_ref()
                    .is_some_and(|c| c.generation == generation)
                {
                    self.child_line(&line);
                }
                Flow::Continue
            }
            Msg::ChildExited(generation, code, signal, stderr) => {
                if self
                    .child
                    .as_ref()
                    .is_some_and(|c| c.generation == generation)
                {
                    self.child_exited(code, signal, stderr).await;
                }
                Flow::Continue
            }
            Msg::Signal => Flow::Stop("signal"),
        }
    }

    fn accept(&mut self, stream: UnixStream) {
        let id = self.next_client;
        self.next_client += 1;
        let (reader, mut writer) = stream.into_split();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        let writer_task = tokio::spawn(async move {
            while let Some(line) = out_rx.recv().await {
                if writer.write_all(line.as_bytes()).await.is_err()
                    || writer.write_all(b"\n").await.is_err()
                {
                    break;
                }
            }
            let _ = writer.flush().await;
        });
        let in_tx = self.tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if in_tx.send(Msg::Client(id, line)).is_err() {
                    return;
                }
            }
            let _ = in_tx.send(Msg::ClientGone(id));
        });
        self.clients.insert(
            id,
            Client {
                tx: out_tx,
                attached: false,
                writer: writer_task,
            },
        );
        let hello = HostMsg::Hello {
            protocol: PROTOCOL,
            id: self.config.id.to_string(),
            seq: self.log.last_seq(),
            state: self.state(),
        };
        self.send_to(id, &hello);
    }

    async fn client_line(&mut self, client_id: u64, line: &str) -> Flow {
        let msg: ClientMsg = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(e) => {
                self.send_to(
                    client_id,
                    &HostMsg::Error {
                        message: format!("not a message: {e}"),
                    },
                );
                return Flow::Continue;
            }
        };
        match msg {
            ClientMsg::Attach { since } => {
                match self.log.read_since(since) {
                    Ok(lines) => {
                        if let Some(client) = self.clients.get(&client_id) {
                            for l in lines {
                                let _ = client.tx.send(l);
                            }
                        }
                    }
                    Err(e) => self.send_to(
                        client_id,
                        &HostMsg::Error {
                            message: format!("reading the log: {e}"),
                        },
                    ),
                }
                let seq = self.log.last_seq();
                self.send_to(client_id, &HostMsg::CaughtUp { seq });
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.attached = true;
                }
                Flow::Continue
            }
            ClientMsg::Send { line } => {
                if self.stopping.is_some() {
                    self.send_to(
                        client_id,
                        &HostMsg::Error {
                            message: "the host is stopping".into(),
                        },
                    );
                    return Flow::Continue;
                }
                match line["type"].as_str().unwrap_or("") {
                    "user" => {
                        if self.child.is_none() {
                            self.spawn_child().await;
                        }
                        if self.write_child(&line).await {
                            if let Some(child) = self.child.as_mut() {
                                child.working = true;
                                child.saw_user = true;
                            }
                            self.silent_respawns = 0;
                            self.idle_deadline = None;
                            self.record(HostMsg::Sent {
                                seq: 0,
                                t: now(),
                                line,
                            });
                            self.set_state("working");
                        } else {
                            self.send_to(
                                client_id,
                                &HostMsg::Error {
                                    message: "no process to answer".into(),
                                },
                            );
                        }
                    }
                    "control_response" => {
                        if self.write_child(&line).await {
                            if let Some(request_id) = line["response"]["request_id"].as_str() {
                                self.pending.remove(request_id);
                            }
                            self.record(HostMsg::Sent {
                                seq: 0,
                                t: now(),
                                line,
                            });
                            if self.pending.is_empty()
                                && self.child.as_ref().is_some_and(|c| c.working)
                            {
                                self.set_state("working");
                            }
                        } else {
                            self.send_to(
                                client_id,
                                &HostMsg::Error {
                                    message: "no process to answer".into(),
                                },
                            );
                        }
                    }
                    "control_request" => {
                        if self.write_child(&line).await {
                            self.record(HostMsg::Sent {
                                seq: 0,
                                t: now(),
                                line,
                            });
                        } else {
                            self.send_to(
                                client_id,
                                &HostMsg::Error {
                                    message: "no process to answer".into(),
                                },
                            );
                        }
                    }
                    other => self.send_to(
                        client_id,
                        &HostMsg::Error {
                            message: format!("a line of type '{other}' cannot be sent"),
                        },
                    ),
                }
                Flow::Continue
            }
            ClientMsg::Start => {
                if self.child.is_none() && self.stopping.is_none() {
                    self.spawn_child().await;
                }
                Flow::Continue
            }
            ClientMsg::Stop => Flow::Stop("client"),
            ClientMsg::Status => {
                let status = HostMsg::Status {
                    state: self.state(),
                };
                self.send_to(client_id, &status);
                Flow::Continue
            }
        }
    }

    /// Write one line to the process's stdin; false when there is none to take it.
    async fn write_child(&mut self, line: &Value) -> bool {
        let Some(child) = self.child.as_mut() else {
            return false;
        };
        let Some(stdin) = child.stdin.as_mut() else {
            return false;
        };
        let text = line.to_string();
        let ok = stdin.write_all(text.as_bytes()).await.is_ok()
            && stdin.write_all(b"\n").await.is_ok()
            && stdin.flush().await.is_ok();
        if !ok {
            child.stdin = None;
        }
        ok
    }

    /// Which conversation the next process opens.
    fn session_arg(&mut self) -> SessionArg {
        if let Some(sid) = self.claude_session_id.clone() {
            return SessionArg::Resume(sid);
        }
        if let Some(resume) = self
            .entry
            .options
            .resume
            .clone()
            .filter(|r| !r.trim().is_empty())
        {
            return SessionArg::Resume(resume);
        }
        if !self.offered_own_id {
            self.offered_own_id = true;
            return SessionArg::New(self.config.id.clone());
        }
        SessionArg::New(SessionId::new())
    }

    async fn spawn_child(&mut self) {
        if self.child.is_some() || self.stopping.is_some() {
            return;
        }
        let Some(binary) = self.config.claude.clone().or_else(claude_binary) else {
            self.log_host(HostEvent::Note {
                text: "Claude Code is not installed: `claude` is not on the host's PATH".into(),
            });
            return;
        };
        let arg = self.session_arg();
        let args = build_args(&self.entry.options, &arg);
        self.generations += 1;
        let generation = self.generations;
        let mut command = tokio::process::Command::new(&binary);
        command
            .args(&args)
            .current_dir(&self.entry.cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(false);
        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.log_host(HostEvent::Note {
                    text: format!("Claude Code could not start ({}): {e}", binary.display()),
                });
                return;
            }
        };
        let pid = child.id().unwrap_or(0);
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let tail: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        if let Some(stderr) = stderr {
            let tail = Arc::clone(&tail);
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut buf = [0u8; 1024];
                while let Ok(n) = reader.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                    for line in chunk.lines().filter(|l| !l.trim().is_empty()) {
                        eprintln!("claude: {line}");
                    }
                    if let Ok(mut t) = tail.lock() {
                        t.push_str(&chunk);
                        if t.len() > STDERR_TAIL {
                            let cut = t.len() - STDERR_TAIL;
                            let cut = t.floor_char_boundary(cut);
                            t.drain(..cut);
                        }
                    }
                }
            });
        }
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Some(stdout) = stdout {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if tx.send(Msg::ChildLine(generation, line)).is_err() {
                        return;
                    }
                }
            }
            let status = child.wait().await.ok();
            let code = status.and_then(|s| s.code());
            let signal = status.and_then(|s| {
                use std::os::unix::process::ExitStatusExt;
                s.signal()
            });
            let stderr = tail
                .lock()
                .map(|t| t.trim().to_string())
                .unwrap_or_default();
            let _ = tx.send(Msg::ChildExited(generation, code, signal, stderr));
        });
        let resume = match &arg {
            SessionArg::New(_) => None,
            SessionArg::Resume(sid) => Some(sid.clone()),
        };
        self.child = Some(ChildHandle {
            generation,
            pid,
            stdin,
            working: false,
            saw_user: false,
            arg,
        });
        self.log_host(HostEvent::ChildStarted { pid, resume });
        self.set_state("idle");
    }

    fn child_line(&mut self, line: &str) {
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => serde_json::json!({ "type": "raw", "text": line }),
        };
        let events = wire::parse_value(&value);
        if wire::is_live_only(&value) {
            if let Ok(text) = serde_json::to_string(&HostMsg::Claude {
                seq: None,
                t: now(),
                line: value,
            }) {
                self.broadcast(&text);
            }
        } else {
            self.record(HostMsg::Claude {
                seq: Some(0),
                t: now(),
                line: value,
            });
        }
        for event in events {
            match event {
                Event::Init {
                    session_id,
                    model,
                    permission_mode,
                } => {
                    let mut changed = false;
                    if !session_id.is_empty()
                        && self.claude_session_id.as_deref() != Some(&session_id)
                    {
                        self.claude_session_id = Some(session_id);
                        changed = true;
                    }
                    if !model.is_empty() && self.model.as_deref() != Some(&model) {
                        self.model = Some(model);
                        changed = true;
                    }
                    if !permission_mode.is_empty() && self.permission_mode != permission_mode {
                        self.permission_mode = permission_mode;
                        changed = true;
                    }
                    if changed {
                        let state = self.entry.state.clone();
                        self.set_state(&state);
                    }
                }
                Event::ModeChanged(mode) => {
                    if self.permission_mode != mode {
                        self.permission_mode = mode;
                        let state = self.entry.state.clone();
                        self.set_state(&state);
                    }
                }
                Event::Permission {
                    request_id, tool, ..
                } => {
                    self.pending.insert(request_id);
                    self.set_state("waiting");
                    let title = self.entry.title.clone();
                    self.notify(&title, &format!("Claude asks to use {tool}"));
                }
                Event::TurnDone { ok, text, .. } => {
                    if let Some(child) = self.child.as_mut() {
                        child.working = false;
                    }
                    self.set_state("idle");
                    self.arm_idle();
                    let title = self.entry.title.clone();
                    let body = if ok {
                        format!("Done: {}", brief(&text, 120))
                    } else {
                        format!("The turn failed: {}", brief(&text, 120))
                    };
                    self.notify(&title, &body);
                }
                _ => {}
            }
        }
    }

    fn arm_idle(&mut self) {
        let secs = self.entry.options.idle_timeout_secs;
        self.idle_deadline = (secs > 0 && self.pending.is_empty())
            .then(|| Instant::now() + Duration::from_secs(secs));
    }

    async fn idle_fired(&mut self) {
        self.idle_deadline = None;
        let idle = self
            .child
            .as_ref()
            .is_some_and(|c| !c.working && self.pending.is_empty());
        if idle && self.stopping.is_none() {
            self.end_child("idle", false).await;
        }
    }

    async fn child_exited(&mut self, code: Option<i32>, signal: Option<i32>, stderr: String) {
        let Some(child) = self.child.take() else {
            return;
        };
        let reason = self
            .child_stop_reason
            .take()
            .unwrap_or(if signal.is_some() { "signal" } else { "exit" });
        self.pending.clear();
        self.idle_deadline = None;
        self.log_host(HostEvent::ChildExited {
            code,
            signal,
            reason: reason.to_string(),
            stderr: stderr.clone(),
        });
        self.set_state(if self.stopping.is_some() {
            "stopped"
        } else {
            "sleeping"
        });
        if self.stopping.is_some() || child.saw_user || self.silent_respawns >= 2 {
            return;
        }
        // The process ended before its first message: a conversation that could not be
        // opened. A missing transcript starts a fresh one; an id already in use resumes.
        let lower = stderr.to_lowercase();
        if lower.contains("no conversation found") {
            let stale = match &child.arg {
                SessionArg::Resume(sid) => sid.clone(),
                SessionArg::New(id) => id.to_string(),
            };
            self.claude_session_id = None;
            self.entry.options.resume = None;
            self.offered_own_id = true;
            self.log_host(HostEvent::Note {
                text: format!(
                    "The earlier conversation ({stale}) could not be resumed; the next message starts a new one."
                ),
            });
            let state = self.entry.state.clone();
            self.set_state(&state);
        } else if lower.contains("already in use") {
            if let SessionArg::New(id) = &child.arg {
                self.claude_session_id = Some(id.to_string());
            }
            self.silent_respawns += 1;
            self.spawn_child().await;
        }
    }

    /// End the process in order: an interrupt when a turn runs (and asked for), the end
    /// of its stdin, SIGTERM, SIGKILL, each after its grace. Lines it still writes are
    /// handled on the way.
    async fn end_child(&mut self, reason: &'static str, interrupt: bool) {
        let Some((generation, pid, working)) = self
            .child
            .as_ref()
            .map(|c| (c.generation, c.pid, c.working))
        else {
            return;
        };
        self.child_stop_reason = Some(reason);
        let grace = self.config.grace;
        if working && interrupt {
            self.requests += 1;
            let request = wire::interrupt_request(self.requests);
            if let Ok(line) = serde_json::from_str::<Value>(&request) {
                if self.write_child(&line).await {
                    self.record(HostMsg::Sent {
                        seq: 0,
                        t: now(),
                        line,
                    });
                }
            }
            self.wait_until(generation, grace.interrupt, |host| {
                host.child.as_ref().is_none_or(|c| !c.working)
            })
            .await;
        }
        if self
            .child
            .as_ref()
            .is_some_and(|c| c.generation == generation)
        {
            if let Some(child) = self.child.as_mut() {
                child.stdin = None;
            }
            if !self
                .wait_until(generation, grace.eof, |host| host.child.is_none())
                .await
            {
                // SAFETY: `pid` is the process this host spawned and has not yet reaped
                // (its exit arrives through the channel), so the signal reaches it alone.
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGTERM);
                }
                if !self
                    .wait_until(generation, grace.term, |host| host.child.is_none())
                    .await
                {
                    // SAFETY: as above; the process ignored SIGTERM within its grace.
                    unsafe {
                        libc::kill(pid as libc::pid_t, libc::SIGKILL);
                    }
                    self.wait_until(generation, Duration::from_secs(5), |host| {
                        host.child.is_none()
                    })
                    .await;
                }
            }
        }
        self.child_stop_reason = None;
    }

    /// Handle messages until `done` holds or `timeout` passes; whether it held. A signal
    /// that arrives meanwhile is remembered for the loop.
    async fn wait_until(
        &mut self,
        generation: u64,
        timeout: Duration,
        done: impl Fn(&Host) -> bool,
    ) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if done(self)
                || self
                    .child
                    .as_ref()
                    .is_some_and(|c| c.generation != generation)
            {
                return true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            match tokio::time::timeout(remaining, self.rx.recv()).await {
                Ok(Some(Msg::Signal)) => self.pending_signal = true,
                Ok(Some(msg)) => {
                    let _ = self.handle(msg).await;
                }
                Ok(None) | Err(_) => return false,
            }
        }
    }

    /// Close every client's channel and wait for its writer, so the lines of the stop
    /// (the interrupt, the process's exit, the last of its output) are on the wire
    /// before the runtime goes with the host.
    async fn drain_clients(&mut self) {
        let writers: Vec<tokio::task::JoinHandle<()>> = self
            .clients
            .drain()
            .map(|(_, client)| {
                drop(client.tx);
                client.writer
            })
            .collect();
        for writer in writers {
            let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
        }
    }

    /// The stop sequence for the host itself.
    async fn shutdown(&mut self, reason: &'static str) {
        self.stopping = Some(reason);
        self.log_host(HostEvent::Stopping {
            reason: reason.to_string(),
        });
        self.end_child("stop", true).await;
        self.set_state("stopped");
    }
}

fn brief(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let cut: String = flat.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::{Connection, Incoming};
    use crate::agent::protocol::ClientMsg;
    use crate::agent::registry::{read_entry_in, write_entry_in, Entry};
    use crate::agent::spawn::SpawnOptions;
    use crate::agent::testing::{fake, scratch, SPAWN};
    use std::sync::mpsc::{Receiver, RecvTimeoutError};

    const INIT: &str = r#"{"type":"system","subtype":"init","session_id":"claude-sid","model":"m","permissionMode":"default"}"#;
    const DELTA: &str = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}}"#;
    const TEXT: &str =
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#;
    const RESULT: &str = r#"{"type":"result","subtype":"success","is_error":false,"num_turns":1,"total_cost_usd":0.01,"result":"hi","duration_ms":5}"#;
    const ASK: &str = r#"{"type":"control_request","request_id":"req-1","request":{"subtype":"can_use_tool","tool_name":"Write","input":{"file_path":"x"},"tool_use_id":"toolu_1"}}"#;

    /// A fake that logs its argv beside itself and answers each user line with a turn.
    fn turn_fake() -> String {
        format!(
            "printf '%s\\n' \"$@\" > \"$(dirname \"$0\")/argv.log\"\nwhile IFS= read -r line; do\n  case \"$line\" in\n    *'\"type\":\"user\"'*) echo '{INIT}'; echo '{DELTA}'; echo '{TEXT}'; echo '{RESULT}';;\n  esac\ndone\n"
        )
    }

    struct Harness {
        dir: PathBuf,
        state_dir: PathBuf,
        run_dir: PathBuf,
        id: SessionId,
        notifications: Arc<Mutex<Vec<String>>>,
        thread: Option<std::thread::JoinHandle<Result<(), String>>>,
    }

    impl Harness {
        fn start(name: &str, body: &str, idle_timeout_secs: u64, grace: Grace) -> Self {
            let dir = scratch(name);
            let state_dir = dir.join("s");
            let run_dir = dir.join("r");
            let claude = fake(&dir, body);
            let id = SessionId::parse("s1").unwrap();
            let entry = Entry::new(
                &id,
                "Probe",
                None,
                &dir.to_string_lossy(),
                SpawnOptions {
                    permission_mode: "default".into(),
                    model: None,
                    strict_mcp: true,
                    allowed_tools: Vec::new(),
                    system_prompt: String::new(),
                    mcp_url: "http://x".into(),
                    name: None,
                    idle_timeout_secs,
                    resume: None,
                },
            );
            write_entry_in(&state_dir, &entry).unwrap();
            let notifications: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            let record = Arc::clone(&notifications);
            let config = HostConfig {
                id: id.clone(),
                state_dir: state_dir.clone(),
                run_dir: run_dir.clone(),
                claude: Some(claude),
                grace,
                notify: Arc::new(move |title, body| {
                    record.lock().unwrap().push(format!("{title}: {body}"));
                }),
                eager_child: true,
            };
            let thread = std::thread::spawn(move || serve(config));
            let socket = run_dir.join("s1.sock");
            let deadline = Instant::now() + Duration::from_secs(10);
            while !socket.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            Self {
                dir,
                state_dir,
                run_dir,
                id,
                notifications,
                thread: Some(thread),
            }
        }

        fn socket(&self) -> PathBuf {
            self.run_dir.join("s1.sock")
        }

        /// Connect, read the hello, attach from `since`, and read on a thread.
        fn attach(&self, since: u64) -> (Connection, Receiver<Incoming>, String) {
            let mut connection =
                Connection::connect_with_retry(&self.socket(), Duration::from_secs(10)).unwrap();
            let hello = connection
                .read_line_timeout(Duration::from_secs(10))
                .unwrap()
                .expect("a hello");
            connection.send(&ClientMsg::Attach { since }).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            connection
                .spawn_reader(move |incoming| {
                    let _ = tx.send(incoming);
                })
                .unwrap();
            (connection, rx, hello)
        }

        fn argv(&self) -> String {
            std::fs::read_to_string(self.dir.join("argv.log")).unwrap_or_default()
        }

        fn entry(&self) -> Entry {
            read_entry_in(&self.state_dir, &self.id).unwrap()
        }

        /// Stop the host through `connection` and check it ended cleanly.
        fn finish(&mut self, connection: &mut Connection) {
            let _ = connection.send(&ClientMsg::Stop);
            self.join();
            assert!(!self.socket().exists(), "the socket is removed at the end");
            assert_eq!(self.entry().state, "stopped");
        }

        fn join(&mut self) {
            let thread = self.thread.take().expect("a running host");
            assert_eq!(thread.join().unwrap(), Ok(()));
        }

        /// Run a second host over the same directories, after a finish.
        fn restart(&mut self) {
            let record = Arc::clone(&self.notifications);
            let config = HostConfig {
                id: self.id.clone(),
                state_dir: self.state_dir.clone(),
                run_dir: self.run_dir.clone(),
                claude: Some(self.dir.join("claude-fake")),
                grace: Grace::default(),
                notify: Arc::new(move |title, body| {
                    record.lock().unwrap().push(format!("{title}: {body}"));
                }),
                eager_child: true,
            };
            self.thread = Some(std::thread::spawn(move || serve(config)));
            let socket = self.socket();
            let deadline = Instant::now() + Duration::from_secs(10);
            while !socket.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            // A test that panicked left its host running; ask it to stop before the
            // join, or the whole run waits for a process nobody will end.
            if let Some(thread) = self.thread.take() {
                if let Ok(mut connection) = Connection::connect(&self.socket()) {
                    let _ = connection.send(&ClientMsg::Stop);
                }
                let _ = thread.join();
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn user(text: &str) -> ClientMsg {
        ClientMsg::Send {
            line: serde_json::from_str(&wire::user_message(text)).unwrap(),
        }
    }

    /// Lines until one satisfies `pred`; none within `timeout`.
    fn wait_for(
        rx: &Receiver<Incoming>,
        timeout: Duration,
        pred: impl Fn(&str) -> bool,
    ) -> Option<String> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(remaining) {
                Ok(Incoming::Line(line)) if pred(&line) => return Some(line),
                Ok(Incoming::Line(_)) => continue,
                Ok(Incoming::Closed(_)) => return None,
                Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => {
                    return None
                }
            }
        }
    }

    fn expect(rx: &Receiver<Incoming>, what: &str) -> String {
        wait_for(rx, Duration::from_secs(10), |l| l.contains(what))
            .unwrap_or_else(|| panic!("no line with {what}"))
    }

    #[test]
    fn boot_replay_and_live_lines_in_order() {
        let _serial = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let mut h = Harness::start("boot", &turn_fake(), 0, Grace::default());
        let (mut a, rx, hello) = h.attach(0);
        assert!(
            hello.contains(r#""type":"hello""#) && hello.contains(r#""protocol":1"#),
            "{hello}"
        );
        let started = expect(&rx, r#""event":"started""#);
        assert!(started.contains(r#""resumed":false"#));
        expect(&rx, r#""event":"child_started""#);
        expect(&rx, r#""type":"caught_up""#);
        let argv = h.argv();
        assert!(
            argv.contains("--session-id\ns1"),
            "the first spawn offers the session's own id: {argv}"
        );
        assert!(argv.contains("--strict-mcp-config"));
        a.send(&user("hello there")).unwrap();
        let sent = expect(&rx, r#""type":"sent""#);
        assert!(sent.contains("hello there"));
        // In the order the process writes them: the turn's init, then the delta (live
        // only, so it carries no sequence number), then the result.
        expect(&rx, r#""subtype":"init""#);
        let live_delta = expect(&rx, "text_delta");
        assert!(
            !live_delta.contains(r#""seq""#),
            "a delta is live only: {live_delta}"
        );
        let result = expect(&rx, r#""type":"result""#);
        assert!(result.contains(r#""seq""#));
        assert_eq!(h.entry().claude_session_id.as_deref(), Some("claude-sid"));
        assert_eq!(h.entry().state, "idle");
        // A second client from the start sees the log without the delta; a third from
        // a later point sees only what followed.
        let (b, rx_b, _) = h.attach(0);
        let lines: Vec<String> =
            std::iter::from_fn(|| match rx_b.recv_timeout(Duration::from_secs(5)) {
                Ok(Incoming::Line(l)) => Some(l),
                _ => None,
            })
            .take_while(|l| !l.contains("caught_up"))
            .collect();
        assert!(lines.iter().any(|l| l.contains(r#""event":"started""#)));
        assert!(lines.iter().any(|l| l.contains("hello there")));
        assert!(lines.iter().any(|l| l.contains(r#""type":"result""#)));
        assert!(!lines.iter().any(|l| l.contains("text_delta")));
        let last_seq: u64 = serde_json::from_str::<Value>(lines.last().unwrap()).unwrap()["seq"]
            .as_u64()
            .unwrap();
        let (c, rx_c, _) = h.attach(last_seq - 1);
        let later = expect(&rx_c, r#""seq""#);
        assert!(later.contains(&format!(r#""seq":{last_seq}"#)), "{later}");
        expect(&rx_c, "caught_up");
        // A message from one client reaches every attached client.
        a.send(&user("again")).unwrap();
        assert!(expect(&rx_b, "again").contains(r#""type":"sent""#));
        expect(&rx_c, "again");
        expect(&rx, "again");
        b.close();
        c.close();
        assert!(
            h.notifications.lock().unwrap().is_empty(),
            "attached clients are not notified"
        );
        h.finish(&mut a);
    }

    #[test]
    fn a_detached_client_does_not_stop_the_turn_and_nobody_attached_means_a_notification() {
        let _serial = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let body = format!(
            "while IFS= read -r line; do\n  case \"$line\" in\n    *'\"type\":\"user\"'*) sleep 0.5; echo '{TEXT}'; echo '{RESULT}';;\n  esac\ndone\n"
        );
        let mut h = Harness::start("detach", &body, 0, Grace::default());
        let (mut a, rx, _) = h.attach(0);
        expect(&rx, "caught_up");
        a.send(&user("slow one")).unwrap();
        expect(&rx, r#""type":"sent""#);
        a.close();
        std::thread::sleep(Duration::from_millis(1200));
        let (mut b, rx_b, _) = h.attach(0);
        expect(&rx_b, "slow one");
        let result = expect(&rx_b, r#""type":"result""#);
        assert!(
            result.contains(r#""seq""#),
            "the answer landed in the log while nobody watched"
        );
        expect(&rx_b, "caught_up");
        let notes = h.notifications.lock().unwrap().clone();
        assert_eq!(notes, vec!["Probe: Done: hi".to_string()]);
        h.finish(&mut b);
    }

    #[test]
    fn a_permission_is_pending_until_answered_and_notified_when_nobody_is_attached() {
        let _serial = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let body = format!(
            "while IFS= read -r line; do\n  case \"$line\" in\n    *'\"type\":\"user\"'*) echo '{ASK}';;\n    *control_response*) echo '{{\"type\":\"user\",\"message\":{{\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\",\"content\":\"ok\",\"is_error\":false}}]}}}}'; echo '{RESULT}';;\n  esac\ndone\n"
        );
        let mut h = Harness::start("perm", &body, 0, Grace::default());
        let (mut a, rx, _) = h.attach(0);
        expect(&rx, "caught_up");
        a.send(&user("write it")).unwrap();
        expect(&rx, "can_use_tool");
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(h.entry().state, "waiting");
        a.send(&ClientMsg::Status).unwrap();
        let status = expect(&rx, r#""type":"status""#);
        assert!(status.contains(r#""pending":["req-1"]"#), "{status}");
        assert!(
            h.notifications.lock().unwrap().is_empty(),
            "a client is attached"
        );
        a.close();
        std::thread::sleep(Duration::from_millis(300));
        let (mut b, rx_b, hello) = h.attach(0);
        assert!(hello.contains(r#""pending":["req-1"]"#), "{hello}");
        expect(&rx_b, "caught_up");
        b.send(&ClientMsg::Send {
            line: serde_json::from_str(&wire::control_response("req-1", true, "{}", "{}")).unwrap(),
        })
        .unwrap();
        let sent = expect(&rx_b, r#""type":"sent""#);
        assert!(sent.contains("control_response"));
        expect(&rx_b, "tool_result");
        expect(&rx_b, r#""type":"result""#);
        b.send(&ClientMsg::Status).unwrap();
        let status = expect(&rx_b, r#""type":"status""#);
        assert!(status.contains(r#""pending":[]"#), "{status}");
        // The notification for the answer came while nobody was attached; the
        // permission itself was asked while a client was.
        let notes = h.notifications.lock().unwrap().clone();
        assert!(notes.is_empty(), "{notes:?}");
        h.finish(&mut b);
    }

    #[test]
    fn a_permission_asked_while_nobody_watches_notifies() {
        let _serial = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let body = format!(
            "while IFS= read -r line; do\n  case \"$line\" in\n    *'\"type\":\"user\"'*) sleep 0.4; echo '{ASK}';;\n  esac\ndone\n"
        );
        let mut h = Harness::start("permnote", &body, 0, Grace::default());
        let (mut a, rx, _) = h.attach(0);
        expect(&rx, "caught_up");
        a.send(&user("write it")).unwrap();
        expect(&rx, r#""type":"sent""#);
        a.close();
        std::thread::sleep(Duration::from_millis(900));
        let notes = h.notifications.lock().unwrap().clone();
        assert_eq!(notes, vec!["Probe: Claude asks to use Write".to_string()]);
        let (mut b, rx_b, _) = h.attach(0);
        expect(&rx_b, "caught_up");
        h.finish(&mut b);
    }

    #[test]
    fn an_exit_is_recorded_and_the_next_message_resumes() {
        let _serial = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let body = format!(
            "printf '%s\\n' \"$@\" >> \"$(dirname \"$0\")/argv.log\"\nwhile IFS= read -r line; do\n  case \"$line\" in\n    *'\"type\":\"user\"'*) echo '{INIT}'; echo '{RESULT}'; echo oops >&2; exit 3;;\n  esac\ndone\n"
        );
        let mut h = Harness::start("respawn", &body, 0, Grace::default());
        let (mut a, rx, _) = h.attach(0);
        expect(&rx, "caught_up");
        a.send(&user("first")).unwrap();
        let exited = expect(&rx, r#""event":"child_exited""#);
        assert!(
            exited.contains(r#""code":3"#)
                && exited.contains(r#""reason":"exit""#)
                && exited.contains("oops"),
            "{exited}"
        );
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(h.entry().state, "sleeping");
        a.send(&ClientMsg::Status).unwrap();
        assert!(expect(&rx, r#""type":"status""#).contains(r#""child":"none""#));
        a.send(&user("second")).unwrap();
        let started = expect(&rx, r#""event":"child_started""#);
        assert!(started.contains(r#""resume":"claude-sid""#), "{started}");
        expect(&rx, r#""type":"result""#);
        let argv = h.argv();
        let first = argv.find("--session-id\ns1").expect("the first spawn");
        let second = argv.find("--resume\nclaude-sid").expect("the second spawn");
        assert!(first < second, "{argv}");
        h.finish(&mut a);
    }

    #[test]
    fn a_stale_resume_starts_fresh_with_a_note() {
        let _serial = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let dir_marker = "resumed-once";
        let body = format!(
            "printf '%s\\n' \"$@\" >> \"$(dirname \"$0\")/argv.log\"\ncase \"$*\" in *--resume*) echo 'No conversation found with session ID: gone' >&2; echo '{{\"type\":\"result\",\"subtype\":\"error_during_execution\",\"is_error\":true,\"num_turns\":0,\"errors\":[\"No conversation found with session ID: gone\"]}}'; touch \"$(dirname \"$0\")/{dir_marker}\"; exit 1;; esac\nwhile IFS= read -r line; do\n  case \"$line\" in\n    *'\"type\":\"user\"'*) echo '{INIT}'; echo '{RESULT}';;\n  esac\ndone\n"
        );
        let mut h = Harness::start("stale", &body, 0, Grace::default());
        // The entry names a conversation to continue, as a page from before the host does.
        let mut entry = h.entry();
        entry.options.resume = Some("gone".into());
        write_entry_in(&h.state_dir, &entry).unwrap();
        let (mut a, rx, _) = h.attach(0);
        expect(&rx, "caught_up");
        // The eager spawn used the entry as it was at boot (no resume); make the host
        // read the resume by ending that child and asking for a start.
        a.send(&user("go")).unwrap();
        expect(&rx, r#""type":"result""#);
        h.finish(&mut a);
        // A second host over the same entry resumes the announced id, which is gone.
        let _ = std::fs::remove_file(h.dir.join("argv.log"));
        let mut entry = read_entry_in(&h.state_dir, &h.id).unwrap();
        entry.claude_session_id = Some("gone".into());
        write_entry_in(&h.state_dir, &entry).unwrap();
        h.restart();
        let h2 = &mut h;
        let (mut b, rx_b, _) = h2.attach(0);
        let note = expect(&rx_b, r#""event":"note""#);
        assert!(
            note.contains("gone") && note.contains("could not be resumed"),
            "{note}"
        );
        expect(&rx_b, "caught_up");
        assert!(h2.entry().claude_session_id.is_none());
        b.send(&user("fresh")).unwrap();
        let started = expect(&rx_b, r#""event":"child_started""#);
        assert!(started.contains(r#""resume":null"#), "{started}");
        expect(&rx_b, r#""type":"result""#);
        let argv = h2.argv();
        assert!(argv.contains("--resume\ngone"), "{argv}");
        assert!(
            argv.contains("--session-id\n"),
            "a fresh id after the failed resume: {argv}"
        );
        assert!(
            !argv.contains("--session-id\ns1\n"),
            "the own id was used up by the first host: {argv}"
        );
        h2.finish(&mut b);
    }

    #[test]
    fn an_idle_child_is_stopped_and_the_next_message_resumes_it() {
        let _serial = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let mut h = Harness::start("idle", &turn_fake(), 1, Grace::default());
        let (mut a, rx, _) = h.attach(0);
        expect(&rx, "caught_up");
        a.send(&user("one")).unwrap();
        expect(&rx, r#""type":"result""#);
        let exited = wait_for(&rx, Duration::from_secs(10), |l| {
            l.contains(r#""event":"child_exited""#)
        })
        .expect("the idle stop");
        assert!(exited.contains(r#""reason":"idle""#), "{exited}");
        a.send(&user("two")).unwrap();
        let started = expect(&rx, r#""event":"child_started""#);
        assert!(started.contains(r#""resume":"claude-sid""#), "{started}");
        expect(&rx, r#""type":"result""#);
        h.finish(&mut a);
    }

    #[test]
    fn stop_runs_the_sequence_and_a_stubborn_child_is_killed_within_the_grace() {
        let _serial = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        // A child that ignores SIGTERM and never reads its stdin to the end.
        let body = "trap '' TERM\nwhile :; do read -r -t 0.2 line || true; done\n";
        let grace = Grace {
            interrupt: Duration::from_millis(200),
            eof: Duration::from_millis(300),
            term: Duration::from_millis(300),
        };
        let mut h = Harness::start("stubborn", body, 0, grace);
        let (mut a, rx, _) = h.attach(0);
        expect(&rx, "caught_up");
        let began = Instant::now();
        a.send(&ClientMsg::Stop).unwrap();
        let stopping = expect(&rx, r#""event":"stopping""#);
        assert!(stopping.contains(r#""reason":"client""#));
        let exited = expect(&rx, r#""event":"child_exited""#);
        assert!(
            exited.contains(r#""signal":9"#) && exited.contains(r#""reason":"stop""#),
            "{exited}"
        );
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "the grace bounds the wait"
        );
        h.join();
        assert_eq!(h.entry().state, "stopped");
        assert!(!h.socket().exists());
    }

    #[test]
    fn a_polite_child_ends_on_eof_and_a_second_host_is_refused() {
        let _serial = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let mut h = Harness::start("polite", &turn_fake(), 0, Grace::default());
        let (mut a, rx, _) = h.attach(0);
        expect(&rx, "caught_up");
        let twin = HostConfig {
            id: h.id.clone(),
            state_dir: h.state_dir.clone(),
            run_dir: h.run_dir.clone(),
            claude: Some(h.dir.join("claude-fake")),
            grace: Grace::default(),
            notify: Arc::new(|_, _| {}),
            eager_child: false,
        };
        let err = serve(twin).unwrap_err();
        assert!(err.contains("already running"), "{err}");
        a.send(&ClientMsg::Send {
            line: serde_json::from_str(&wire::control_response("nobody", true, "{}", "{}"))
                .unwrap(),
        })
        .unwrap();
        // A control response with a child goes through; without one it is an error.
        expect(&rx, r#""type":"sent""#);
        a.send(&ClientMsg::Stop).unwrap();
        let exited = expect(&rx, r#""event":"child_exited""#);
        assert!(
            exited.contains(r#""code":0"#) && exited.contains(r#""reason":"stop""#),
            "{exited}"
        );
        h.join();
    }

    #[test]
    fn a_line_that_is_not_a_message_answers_an_error() {
        let _serial = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let mut h = Harness::start("errors", &turn_fake(), 0, Grace::default());
        let (mut a, rx, _) = h.attach(0);
        expect(&rx, "caught_up");
        a.send_line("not json").unwrap();
        assert!(expect(&rx, r#""type":"error""#).contains("not a message"));
        a.send(&ClientMsg::Send {
            line: serde_json::json!({ "type": "result" }),
        })
        .unwrap();
        assert!(expect(&rx, r#""type":"error""#).contains("cannot be sent"));
        h.finish(&mut a);
    }

    #[test]
    fn brief_flattens_and_cuts() {
        assert_eq!(brief("a  b\n c", 10), "a b c");
        assert_eq!(brief("abcdefghij", 5), "abcde…");
    }
}
