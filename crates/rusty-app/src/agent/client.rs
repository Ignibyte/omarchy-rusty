//! A client of a host: one Unix socket, the lines read on a thread, the messages written
//! from wherever the client is (the app's `Assistant`, the `attach` verb).

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use super::protocol::ClientMsg;

/// What the reader thread hands back: a line, or the end with a reason.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Line(String),
    Closed(String),
}

/// A connection to a host.
pub struct Connection {
    stream: UnixStream,
    reader: Option<BufReader<UnixStream>>,
}

impl Connection {
    /// Connect once.
    pub fn connect(socket: &Path) -> io::Result<Self> {
        let stream = UnixStream::connect(socket)?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self {
            stream,
            reader: Some(reader),
        })
    }

    /// Connect, trying again until `timeout` passes (a host that is still coming up).
    pub fn connect_with_retry(socket: &Path, timeout: Duration) -> io::Result<Self> {
        let deadline = Instant::now() + timeout;
        loop {
            match Self::connect(socket) {
                Ok(c) => return Ok(c),
                Err(e) if Instant::now() >= deadline => return Err(e),
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    }

    /// The next line, blocking; none at the end of the stream. Only until the reader
    /// thread is started.
    pub fn read_line(&mut self) -> io::Result<Option<String>> {
        let Some(reader) = self.reader.as_mut() else {
            return Err(io::Error::other("the reader thread owns the stream"));
        };
        let mut line = String::new();
        match reader.read_line(&mut line)? {
            0 => Ok(None),
            _ => Ok(Some(line.trim_end_matches(['\n', '\r']).to_string())),
        }
    }

    /// Wait at most `timeout` for the next line.
    pub fn read_line_timeout(&mut self, timeout: Duration) -> io::Result<Option<String>> {
        self.stream.set_read_timeout(Some(timeout))?;
        let got = self.read_line();
        self.stream.set_read_timeout(None)?;
        got
    }

    /// Hand every further line to `on` from a thread, then the end.
    pub fn spawn_reader(
        &mut self,
        mut on: impl FnMut(Incoming) + Send + 'static,
    ) -> io::Result<()> {
        let Some(reader) = self.reader.take() else {
            return Err(io::Error::other("the reader thread is already running"));
        };
        std::thread::Builder::new()
            .name("agent-client".to_string())
            .spawn(move || {
                let mut reader = reader;
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => {
                            on(Incoming::Closed("the host closed the connection".into()));
                            break;
                        }
                        Ok(_) => on(Incoming::Line(
                            line.trim_end_matches(['\n', '\r']).to_string(),
                        )),
                        Err(e) => {
                            on(Incoming::Closed(e.to_string()));
                            break;
                        }
                    }
                }
            })
            .map(|_| ())
    }

    /// Write one message.
    pub fn send(&mut self, msg: &ClientMsg) -> io::Result<()> {
        let line = serde_json::to_string(msg).map_err(io::Error::other)?;
        self.send_line(&line)
    }

    /// Write one line as given.
    pub fn send_line(&mut self, line: &str) -> io::Result<()> {
        self.stream.write_all(line.as_bytes())?;
        self.stream.write_all(b"\n")?;
        self.stream.flush()
    }

    /// End the connection; the reader thread sees the end.
    pub fn close(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn socket(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rusty_cl_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{name}.sock"));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn a_missing_socket_refuses_and_a_late_one_is_waited_for() {
        let path = socket("late");
        assert!(Connection::connect(&path).is_err());
        let server_path = path.clone();
        let server = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let listener = UnixListener::bind(&server_path).unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .write_all(b"{\"type\":\"caught_up\",\"seq\":0}\n")
                .unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            line
        });
        let mut client = Connection::connect_with_retry(&path, Duration::from_secs(5)).unwrap();
        assert_eq!(
            client
                .read_line_timeout(Duration::from_secs(5))
                .unwrap()
                .as_deref(),
            Some(r#"{"type":"caught_up","seq":0}"#)
        );
        client.send(&ClientMsg::Status).unwrap();
        let got = server.join().unwrap();
        assert_eq!(got.trim(), r#"{"type":"status"}"#);
        client.close();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_reader_thread_reports_lines_then_the_end() {
        let path = socket("reader");
        let server_path = path.clone();
        let listener = UnixListener::bind(&server_path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"one\ntwo\n").unwrap();
        });
        let mut client = Connection::connect(&path).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        client
            .spawn_reader(move |incoming| {
                let _ = tx.send(incoming);
            })
            .unwrap();
        assert!(
            client.read_line().is_err(),
            "the thread owns the reader now"
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Incoming::Line("one".into())
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Incoming::Line("two".into())
        );
        server.join().unwrap();
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Incoming::Closed(_)
        ));
        let _ = std::fs::remove_file(&path);
    }
}
