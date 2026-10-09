//! The application event bus: managers emit [`AppEvent`]s into a broadcast channel and
//! every listener (the MCP server's change notifier) subscribes, so a write from any
//! manager reaches every connected client without the manager knowing who is attached.

use serde::Serialize;
use tokio::sync::broadcast;

/// An event broadcast to every listener.
///
/// Serializes to `{ "event": "<name>", "payload": <data> }`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "payload", rename_all = "kebab-case")]
pub enum AppEvent {
    /// On-disk data (notes/brain) or the DB change-sentinel was modified.
    DataChanged,
}

impl AppEvent {
    /// The event's name.
    pub fn name(&self) -> &'static str {
        match self {
            AppEvent::DataChanged => "data-changed",
        }
    }

    /// The event's JSON payload.
    pub fn payload(&self) -> serde_json::Value {
        match self {
            AppEvent::DataChanged => serde_json::Value::Null,
        }
    }
}

/// A cloneable handle for broadcasting [`AppEvent`]s to all subscribers.
///
/// Cloning shares the same underlying channel; dropping every clone closes it.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<AppEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    /// Broadcast channel capacity. Subscribers that fall further behind than
    /// this drop the oldest events (surfaced as `RecvError::Lagged`, which the
    /// bridge ignores).
    const CAPACITY: usize = 256;

    /// Create a new, empty event bus.
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(Self::CAPACITY);
        Self { tx }
    }

    /// Broadcast an event to all current subscribers.
    ///
    /// Silently drops the event when there are no subscribers — emitters never
    /// need to know whether a frontend is currently attached.
    pub fn emit(&self, event: AppEvent) {
        let _ = self.tx.send(event);
    }

    /// Subscribe to the event stream. Each receiver observes every event sent
    /// after it subscribed.
    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.tx.subscribe()
    }
}
