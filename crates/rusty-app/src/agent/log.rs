//! The session's event log: one host message per line, numbered from 1, appended by the
//! host alone. A client's replay is "the lines after the sequence number it has".

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::protocol::HostMsg;

/// The log file, open for appending, with the last sequence number it holds.
pub struct EventLog {
    path: PathBuf,
    file: File,
    last_seq: u64,
}

impl EventLog {
    /// Open (or create) the log and recover its last sequence number; a final line cut
    /// mid-write is skipped, and the next line is appended after it.
    pub fn open(path: &Path) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
            // The log holds what was typed and what the tools answered: the user's own.
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
        let last_seq = match File::open(path) {
            Ok(existing) => BufReader::new(existing)
                .lines()
                .map_while(Result::ok)
                .filter_map(|line| seq_of(&line))
                .max()
                .unwrap_or(0),
            Err(e) if e.kind() == io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e),
        };
        let needs_newline = std::fs::read(path)
            .map(|bytes| !bytes.is_empty() && bytes.last() != Some(&b'\n'))
            .unwrap_or(false);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?;
        if needs_newline {
            file.write_all(b"\n")?;
        }
        Ok(Self {
            path: path.to_path_buf(),
            file,
            last_seq,
        })
    }

    /// Number the message, write it, and hand the line back for the clients.
    pub fn append(&mut self, msg: &mut HostMsg) -> io::Result<String> {
        let seq = self.last_seq + 1;
        msg.set_seq(seq);
        let line = serde_json::to_string(msg).map_err(io::Error::other)?;
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        self.last_seq = seq;
        Ok(line)
    }

    /// The logged lines after `since`, verbatim, in order.
    pub fn read_since(&self, since: u64) -> io::Result<Vec<String>> {
        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        Ok(BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .filter(|line| seq_of(line).is_some_and(|s| s > since))
            .collect())
    }

    /// Whether the log had lines before this host opened it.
    pub fn is_resumed(&self) -> bool {
        self.last_seq > 0
    }

    /// The last sequence number written.
    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }
}

/// The sequence number of a logged line; none for a line that is cut or not a message.
fn seq_of(line: &str) -> Option<u64> {
    serde_json::from_str::<HostMsg>(line).ok()?.seq()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::protocol::{now, HostEvent};

    fn note(text: &str) -> HostMsg {
        HostMsg::Host {
            seq: 0,
            t: now(),
            event: HostEvent::Note { text: text.into() },
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rusty_log_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("events.jsonl")
    }

    #[test]
    fn lines_are_numbered_from_one_and_recovered_on_reopen() {
        let path = scratch("numbered");
        let mut log = EventLog::open(&path).unwrap();
        assert!(!log.is_resumed());
        let first = log.append(&mut note("a")).unwrap();
        assert!(first.contains(r#""seq":1"#), "{first}");
        log.append(&mut note("b")).unwrap();
        assert_eq!(log.last_seq(), 2);
        drop(log);
        let log = EventLog::open(&path).unwrap();
        assert!(log.is_resumed());
        assert_eq!(log.last_seq(), 2);
        let after_one = log.read_since(1).unwrap();
        assert_eq!(after_one.len(), 1);
        assert!(after_one[0].contains(r#""text":"b""#));
        assert_eq!(log.read_since(0).unwrap().len(), 2);
        assert!(log.read_since(5).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_cut_last_line_is_skipped_and_written_past() {
        let path = scratch("cut");
        let mut log = EventLog::open(&path).unwrap();
        log.append(&mut note("whole")).unwrap();
        drop(log);
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(br#"{"type":"host","seq":2,"t":"t","event":"note","tex"#)
            .unwrap();
        drop(file);
        let mut log = EventLog::open(&path).unwrap();
        assert_eq!(log.last_seq(), 1, "the cut line does not count");
        log.append(&mut note("after")).unwrap();
        let lines = log.read_since(0).unwrap();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[1].contains(r#""seq":2"#) && lines[1].contains("after"));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_missing_log_reads_as_empty() {
        let path = scratch("missing");
        let log = EventLog::open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(log.read_since(0).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
