//! The `Agents` QML type: the sessions on this machine, as the registry lists them,
//! refreshed when the state directory changes. The bridge lives here because cxx-qt
//! takes every bridge from one directory; the entries themselves are
//! `agent::registry`'s.

use core::pin::Pin;
use std::time::Duration;

use cxx_qt::Threading;
use cxx_qt_lib::QString;

use crate::agent::registry::{self, entry_path_in, list_entries, rows_json};
use crate::agent::{paths, SessionId};

#[cxx_qt::bridge]
mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        /// Qt's string type.
        type QString = cxx_qt_lib::QString;
    }

    #[auto_cxx_name]
    unsafe extern "RustQt" {
        /// The sessions on this machine, for QML: a JSON array of
        /// `{id, title, page, cwd, state, alive, updated, model, mode}`, newest first.
        #[qobject]
        #[qml_element]
        #[qproperty(QString, sessions)]
        type Agents = super::AgentsRust;

        /// Read the entries again.
        #[qinvokable]
        fn refresh(self: Pin<&mut Agents>);
        /// Whether an entry exists for this id.
        #[qinvokable]
        fn exists(self: &Agents, id: &QString) -> bool;
        /// Whether a host answers for this id.
        #[qinvokable]
        fn alive(self: &Agents, id: &QString) -> bool;
        /// Watch the state directory and refresh when an entry appears, changes or goes.
        #[qinvokable]
        fn watch(self: Pin<&mut Agents>);

        /// The list changed.
        #[qsignal]
        fn changed(self: Pin<&mut Agents>);
    }

    impl cxx_qt::Threading for Agents {}
}

/// The Rust side of [`qobject::Agents`].
pub struct AgentsRust {
    sessions: QString,
}

impl Default for AgentsRust {
    fn default() -> Self {
        Self {
            sessions: QString::from(&rows_json(&list_entries())),
        }
    }
}

impl qobject::Agents {
    /// See the bridge.
    pub fn refresh(mut self: Pin<&mut Self>) {
        let rows = rows_json(&list_entries());
        self.as_mut().set_sessions(QString::from(&rows));
        self.as_mut().changed();
    }

    /// See the bridge.
    pub fn exists(&self, id: &QString) -> bool {
        SessionId::parse(&id.to_string())
            .map(|id| entry_path_in(&paths::state_dir(), &id).is_file())
            .unwrap_or(false)
    }

    /// See the bridge.
    pub fn alive(&self, id: &QString) -> bool {
        SessionId::parse(&id.to_string())
            .map(|id| registry::alive(&id))
            .unwrap_or(false)
    }

    /// See the bridge. The directory is watched non-recursively, so the appends to a
    /// session's log below it never wake the app; a burst of entry writes is coalesced.
    pub fn watch(self: Pin<&mut Self>) {
        let qt_thread = self.qt_thread();
        let dir = paths::state_dir();
        std::thread::spawn(move || {
            use notify::Watcher;
            let _ = std::fs::create_dir_all(&dir);
            let (tx, rx) = std::sync::mpsc::channel();
            let Ok(mut watcher) = notify::recommended_watcher(move |event| {
                let _ = tx.send(event);
            }) else {
                return;
            };
            if watcher
                .watch(&dir, notify::RecursiveMode::NonRecursive)
                .is_err()
            {
                return;
            }
            while rx.recv().is_ok() {
                std::thread::sleep(Duration::from_millis(400));
                while rx.try_recv().is_ok() {}
                let _ = qt_thread.queue(|agents| agents.refresh());
            }
        });
    }
}
