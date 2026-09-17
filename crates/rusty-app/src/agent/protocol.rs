//! The host's socket protocol, NDJSON both ways. A client attaches with the sequence
//! number it already has, the host replays the log after it, says `caught_up`, then
//! streams. Lines with a `seq` are the log; the rest (`hello`, `caught_up`, `status`,
//! `error`, and the live-only `claude` lines without a `seq`) are of the moment.
//!
//! A line the process wrote travels inside `line` as parsed JSON, so its keys come back
//! in the order a map gives them, not the order Claude Code wrote them. Nothing reads
//! them by position; Claude Code's own transcript keeps the original.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The protocol's version, in `hello`.
pub const PROTOCOL: u32 = 1;

/// What a client sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Replay the log after `since`, then stream.
    Attach {
        #[serde(default)]
        since: u64,
    },
    /// Write this line to the process's stdin: a user message, a `control_response` or a
    /// `control_request`. A user message with no process spawns one.
    Send { line: Value },
    /// Spawn the process now if there is none.
    Start,
    /// Stop the process and the host.
    Stop,
    /// Answer with the state.
    Status,
}

/// The host's picture of the session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// `none`, `ready`, `working` or `stopping`.
    pub child: String,
    /// Permission requests nobody answered yet.
    pub pending: Vec<String>,
    /// The Claude session id, once a turn announced it.
    pub claude_session_id: Option<String>,
    pub permission_mode: String,
    pub model: Option<String>,
    /// Clients attached right now.
    pub clients: usize,
    /// The host's own pid.
    pub pid: u32,
}

/// Something the host did, logged like the process's lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum HostEvent {
    /// The host came up; `resumed` when the log already had lines.
    Started { resumed: bool },
    /// The process was spawned; `resume` names the conversation it continues.
    ChildStarted { pid: u32, resume: Option<String> },
    /// The process ended: its exit code or the signal, why (`exit`, `idle`, `stop`,
    /// `signal`), and the tail of its stderr.
    ChildExited {
        code: Option<i32>,
        signal: Option<i32>,
        reason: String,
        stderr: String,
    },
    /// The host is stopping: `client`, `signal`.
    Stopping { reason: String },
    /// A line worth the conversation.
    Note { text: String },
}

/// What the host sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMsg {
    /// The first line on every connection.
    Hello {
        protocol: u32,
        id: String,
        /// The last sequence number in the log.
        seq: u64,
        state: State,
    },
    /// The replay is done; what follows is live.
    CaughtUp {
        seq: u64,
    },
    /// A line the process wrote; without a `seq` it is live only and was not logged.
    Claude {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
        t: String,
        line: Value,
    },
    /// A line a client had the host write to the process.
    Sent {
        seq: u64,
        t: String,
        line: Value,
    },
    /// Something the host did.
    Host {
        seq: u64,
        t: String,
        #[serde(flatten)]
        event: HostEvent,
    },
    Status {
        state: State,
    },
    Error {
        message: String,
    },
}

impl HostMsg {
    /// The sequence number of a logged line.
    pub fn seq(&self) -> Option<u64> {
        match self {
            HostMsg::Claude { seq, .. } => *seq,
            HostMsg::Sent { seq, .. } | HostMsg::Host { seq, .. } => Some(*seq),
            _ => None,
        }
    }

    /// Set the sequence number when the line is logged.
    pub fn set_seq(&mut self, n: u64) {
        match self {
            HostMsg::Claude { seq, .. } => *seq = Some(n),
            HostMsg::Sent { seq, .. } | HostMsg::Host { seq, .. } => *seq = n,
            _ => {}
        }
    }
}

/// The time a line was written, for the log: UTC, to the millisecond.
pub fn now() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State {
            child: "ready".into(),
            pending: vec![],
            claude_session_id: Some("8b3f".into()),
            permission_mode: "default".into(),
            model: None,
            clients: 1,
            pid: 41822,
        }
    }

    #[test]
    fn the_wire_strings_parse_and_round_trip() {
        let hello = r#"{"type":"hello","protocol":1,"id":"8b3f","seq":812,"state":{"child":"ready","pending":[],"claude_session_id":"8b3f","permission_mode":"default","model":null,"clients":1,"pid":41822}}"#;
        let msg: HostMsg = serde_json::from_str(hello).unwrap();
        assert_eq!(
            msg,
            HostMsg::Hello {
                protocol: 1,
                id: "8b3f".into(),
                seq: 812,
                state: state()
            }
        );
        assert_eq!(serde_json::to_string(&msg).unwrap(), hello);
        let claude = r#"{"type":"claude","seq":7,"t":"2026-09-17T09:47:01.123Z","line":{"type":"system","subtype":"init","session_id":"8b3f"}}"#;
        let msg: HostMsg = serde_json::from_str(claude).unwrap();
        assert_eq!(msg.seq(), Some(7));
        // The embedded line is compared as JSON: it travels parsed, so its keys come
        // back sorted rather than in the order Claude Code wrote them.
        assert_eq!(
            serde_json::from_str::<Value>(&serde_json::to_string(&msg).unwrap()).unwrap(),
            serde_json::from_str::<Value>(claude).unwrap()
        );
        let live =
            r#"{"type":"claude","t":"2026-09-17T09:47:01.123Z","line":{"type":"stream_event"}}"#;
        let msg: HostMsg = serde_json::from_str(live).unwrap();
        assert_eq!(msg.seq(), None);
        assert_eq!(serde_json::to_string(&msg).unwrap(), live);
        let sent = r#"{"type":"sent","seq":8,"t":"t","line":{"type":"user","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}}"#;
        let msg: HostMsg = serde_json::from_str(sent).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&serde_json::to_string(&msg).unwrap()).unwrap(),
            serde_json::from_str::<Value>(sent).unwrap()
        );
        for host in [
            r#"{"type":"host","seq":1,"t":"t","event":"started","resumed":false}"#,
            r#"{"type":"host","seq":2,"t":"t","event":"child_started","pid":41830,"resume":null}"#,
            r#"{"type":"host","seq":90,"t":"t","event":"child_exited","code":0,"signal":null,"reason":"idle","stderr":""}"#,
            r#"{"type":"host","seq":91,"t":"t","event":"stopping","reason":"client"}"#,
            r#"{"type":"host","seq":92,"t":"t","event":"note","text":"the earlier conversation could not be resumed; the next message starts a new one"}"#,
        ] {
            let msg: HostMsg = serde_json::from_str(host).unwrap();
            assert!(matches!(msg, HostMsg::Host { .. }), "{host}");
            assert_eq!(serde_json::to_string(&msg).unwrap(), host);
        }
        let caught: HostMsg = serde_json::from_str(r#"{"type":"caught_up","seq":812}"#).unwrap();
        assert_eq!(caught, HostMsg::CaughtUp { seq: 812 });
        let error: HostMsg =
            serde_json::from_str(r#"{"type":"error","message":"no process to answer"}"#).unwrap();
        assert_eq!(
            error,
            HostMsg::Error {
                message: "no process to answer".into()
            }
        );
        for client in [
            r#"{"type":"attach","since":0}"#,
            r#"{"type":"send","line":{"type":"user"}}"#,
            r#"{"type":"start"}"#,
            r#"{"type":"stop"}"#,
            r#"{"type":"status"}"#,
        ] {
            let msg: ClientMsg = serde_json::from_str(client).unwrap();
            assert_eq!(serde_json::to_string(&msg).unwrap(), client);
        }
        let bare: ClientMsg = serde_json::from_str(r#"{"type":"attach"}"#).unwrap();
        assert_eq!(bare, ClientMsg::Attach { since: 0 });
    }

    #[test]
    fn a_sequence_number_is_set_when_logged() {
        let mut msg = HostMsg::Host {
            seq: 0,
            t: now(),
            event: HostEvent::Note { text: "x".into() },
        };
        msg.set_seq(5);
        assert_eq!(msg.seq(), Some(5));
        let mut hello = HostMsg::CaughtUp { seq: 1 };
        hello.set_seq(9);
        assert_eq!(hello.seq(), None);
        assert!(now().ends_with('Z') && now().len() == 24);
    }
}
