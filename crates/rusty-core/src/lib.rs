//! Rusty core: the manager layer behind the MCP server and the CLI. To-do lists, notes,
//! memories, the brain vault and its index, skills, secrets and settings, the change log
//! and export and import live here. Nothing in this crate knows about a transport.

pub mod brain;
mod core;
pub mod engine;
pub mod events;
mod git;
pub mod notes;
pub mod skills;
pub mod transfer;

pub use core::Core;

use events::{AppEvent, EventBus};
use std::path::PathBuf;

/// Watch the notes vault, brain vault, skills dir, and a sentinel file for
/// changes, emitting a debounced [`AppEvent::DataChanged`] so any connected UI
/// can refetch — lets terminal/MCP edits show up without a manual refresh.
pub fn start_data_watcher(events: EventBus, notes: PathBuf, brain: PathBuf, skills: PathBuf) {
    std::thread::spawn(move || {
        use notify::{RecursiveMode, Watcher};

        // Sentinel the terminal touches (via `rusty-cli refresh`) to signal
        // DB-only changes (tasks/memories) that aren't markdown files.
        let sentinel = dirs::home_dir()
            .unwrap_or_default()
            .join(".rusty")
            .join(".changed");
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&sentinel);

        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        }) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[watch] failed to init watcher: {e}");
                return;
            }
        };

        // A notes folder inside the vault is covered by the vault's watch already.
        let notes_apart = !notes.starts_with(&brain);
        let mut roots = vec![&brain, &skills];
        if notes_apart {
            roots.insert(0, &notes);
        }
        for path in roots {
            if let Err(e) = watcher.watch(path, RecursiveMode::Recursive) {
                eprintln!("[watch] cannot watch {}: {e}", path.display());
            }
        }
        let _ = watcher.watch(&sentinel, RecursiveMode::NonRecursive);
        eprintln!(
            "[watch] watching {}brain + skills + sentinel for live refresh",
            if notes_apart { "notes + " } else { "" }
        );

        // Loop until the watcher is dropped (the channel closes).
        while let Ok(first) = rx.recv() {
            let mut relevant = event_is_relevant(&first);
            // Debounce: drain until ~600ms of quiet, then emit once.
            while let Ok(ev) = rx.recv_timeout(std::time::Duration::from_millis(600)) {
                relevant |= event_is_relevant(&ev);
            }
            if relevant {
                events.emit(AppEvent::DataChanged);
            }
        }
    });
}

/// True unless the change only touches git internals (brain vault auto-commit).
fn event_is_relevant(event: &Result<notify::Event, notify::Error>) -> bool {
    match event {
        Ok(ev) => ev
            .paths
            .iter()
            .any(|p| !p.components().any(|c| c.as_os_str() == ".git")),
        Err(_) => false,
    }
}
