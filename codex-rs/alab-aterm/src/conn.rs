// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One control-socket connection: connect, authenticate, then request/response on a task
//! that owns the stream.
//!
//! The wire (protocol v1, `crates/aterm-ctl/src/lib.rs` module doc and `open_accepted`):
//! - The server never acknowledges `AUTH <token>`; a client proves it was accepted by
//!   sending `version` right after it and reading that one line (`OK version=…`). A refusal
//!   is `ERR auth` or `ERR control server busy; retry`, and the server closes.
//! - Then newline-delimited requests, one at a time, each answered before the next is read,
//!   on a connection that may idle indefinitely.
//! - A reply is framed per verb (`aterm_types::control_verbs::framing_of`): Status (one
//!   `OK …` / `ERR …` line), Lines (`OK <n>` then n lines), Bytes (`OK <n>` then n bytes),
//!   Push (`subscribe`: `OK subscribe <n>` and n `sub <local> <sid>` lines, then frames
//!   until the watch ends; the connection never returns to requests).
//! - A length-framed request (`post … len=<n>`) is its line, then exactly n bytes.
//! - A reply line is at most 8 MiB (`MAX_CONTROL_REPLY_LINE_BYTES`).

use std::os::fd::AsRawFd;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::io::AsyncBufRead;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::UnixStream;
use tokio::net::unix::OwnedReadHalf;
use tokio::net::unix::OwnedWriteHalf;
use tokio::sync::Notify;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::Error;
use crate::locate;
use crate::wire;

/// The longest reply line read (`aterm_types::control_verbs::MAX_CONTROL_REPLY_LINE_BYTES`).
pub const MAX_LINE_BYTES: usize = 8 << 20;

/// The most data lines a Lines reply may announce (`aterm-ctl`'s `MAX_STREAM_LINES`).
const MAX_LINES: usize = 200_000;

/// The longest Bytes reply read (`inbox get`'s 256 KiB body cap, with room to spare).
const MAX_BYTES: usize = 1 << 20;

/// How a reply is framed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// One status line.
    Status,
    /// `OK <n>` then n lines (`ERR …` alone).
    Lines,
    /// `OK <n> [fields…]` then exactly n raw bytes (`ERR …` alone).
    Bytes,
}

/// A reply: its status line (without the newline) and, for [`Framing::Lines`], its lines,
/// or for [`Framing::Bytes`], its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// `OK …` or `ERR …`.
    pub status: String,
    /// The data lines of a Lines reply.
    pub lines: Vec<String>,
    /// The payload of a Bytes reply.
    pub bytes: Vec<u8>,
}

impl Reply {
    /// The text after `OK` (`""` for a bare `OK`), or `None` for anything else.
    pub fn ok(&self) -> Option<&str> {
        if self.status == "OK" {
            return Some("");
        }
        self.status.strip_prefix("OK ")
    }

    /// Whether the reply is an `ERR`.
    pub fn is_err(&self) -> bool {
        self.status == "ERR" || self.status.starts_with("ERR ")
    }
}

/// An authenticated stream, before it is handed to a request task or a push reader.
pub(crate) struct Opened {
    pub reader: BufReader<OwnedReadHalf>,
    pub writer: OwnedWriteHalf,
    /// The `version` answer's text after `OK `.
    pub version: String,
    /// The kernel's word on the process serving the socket (its peer pid), `None` when it
    /// cannot be told.
    pub server_pid: Option<u32>,
    /// The exact capability this listener accepted during this connection's handshake.
    pub auth_identity: AuthIdentity,
}

/// A native listener provisions a fresh AUTH token whenever it binds. Retaining the
/// accepted identity lets observations reject a replacement at the same socket path,
/// even when its peer pid, restored entry id and observation token coincide. Never format
/// the AUTH token.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AuthIdentity(Arc<str>);

impl std::fmt::Debug for AuthIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthIdentity(REDACTED)")
    }
}

/// Connect to `sock`, mark the fd close-on-exec, read the token beside it, `AUTH` and
/// prove acceptance with `version`, all within `deadline`.
pub(crate) async fn open(sock: &Path, deadline: Duration) -> Result<Opened, Error> {
    tokio::time::timeout(deadline, open_inner(sock))
        .await
        .map_err(|_| Error::Unanswered)?
}

async fn open_inner(sock: &Path) -> Result<Opened, Error> {
    let stream = UnixStream::connect(sock).await?;
    set_cloexec(&stream)?;
    let server_pid = stream
        .peer_cred()
        .ok()
        .and_then(|cred| cred.pid())
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid > 0);
    // After the connect, as aterm-ctl does: an update's successor writes its token before
    // it binds, so a token read now names the listener that took us.
    let token = locate::read_token_beside(sock)?;
    let (read, mut writer) = stream.into_split();
    let mut reader = BufReader::new(read);
    let hello = format!("AUTH {token}\nversion\n");
    writer.write_all(hello.as_bytes()).await?;
    writer.flush().await?;
    let line = read_line(&mut reader).await?.ok_or(Error::Closed)?;
    if let Some(version) = line.strip_prefix("OK ") {
        return Ok(Opened {
            reader,
            writer,
            version: version.to_string(),
            server_pid,
            auth_identity: AuthIdentity(token.into()),
        });
    }
    Err(refusal(line))
}

/// A refusal line as an [`Error`]: busy (with its `retry_ms=`) or refused.
pub(crate) fn refusal(line: String) -> Error {
    if line.starts_with("ERR busy") || line.starts_with("ERR control server busy") {
        let retry_ms = wire::retry_ms(&line);
        return Error::Busy { line, retry_ms };
    }
    Error::Refused(line)
}

/// Set `FD_CLOEXEC` on the stream and confirm it: the model's tools are this process's
/// children and must never inherit the connection (CONTRACT §0, law 2).
pub(crate) fn set_cloexec(stream: &UnixStream) -> Result<(), Error> {
    let fd = stream.as_raw_fd();
    // SAFETY: fcntl on a descriptor this stream owns, with integer arguments only.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: as above.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: as above.
    let now = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if now < 0 || now & libc::FD_CLOEXEC == 0 {
        return Err(Error::Refused(
            "could not mark the connection close-on-exec".into(),
        ));
    }
    Ok(())
}

/// One line without its `\n` (and a `\r` before it), bounded; `None` at end of stream.
pub(crate) async fn read_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Option<String>, Error> {
    let mut buf = Vec::new();
    let n = (&mut *reader)
        .take(MAX_LINE_BYTES as u64 + 1)
        .read_until(b'\n', &mut buf)
        .await?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() != Some(&b'\n') {
        if buf.len() > MAX_LINE_BYTES {
            return Err(Error::Malformed("reply line over 8 MiB".into()));
        }
        // End of stream inside a line: the reply is incomplete.
        return Err(Error::Closed);
    }
    buf.pop();
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

/// Read one reply framed as `framing`.
async fn read_reply<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    framing: Framing,
) -> Result<Reply, Error> {
    let status = read_line(reader).await?.ok_or(Error::Closed)?;
    let mut lines = Vec::new();
    let mut bytes = Vec::new();
    if framing != Framing::Status && !status.starts_with("ERR") {
        let max = if framing == Framing::Lines {
            MAX_LINES
        } else {
            MAX_BYTES
        };
        let n = status
            .strip_prefix("OK ")
            .and_then(|r| r.split_ascii_whitespace().next())
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| *n <= max)
            .ok_or_else(|| Error::Malformed(format!("{framing:?} header: {status}")))?;
        if framing == Framing::Lines {
            for _ in 0..n {
                lines.push(read_line(reader).await?.ok_or(Error::Closed)?);
            }
        } else {
            bytes = vec![0; n];
            reader.read_exact(&mut bytes).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    Error::Closed
                } else {
                    Error::Io(e)
                }
            })?;
        }
    }
    Ok(Reply {
        status,
        lines,
        bytes,
    })
}

struct Job {
    line: String,
    /// Raw bytes sent right after the line (a length-framed `post … len=<n>`'s body).
    body: Vec<u8>,
    framing: Framing,
    /// How long this exchange may take once it is on the wire.
    deadline: Duration,
    reply: oneshot::Sender<Result<Reply, Error>>,
}

/// The request side of one connection. The stream lives on a task; dropping every
/// [`Conn`] ends it, and the server then releases any claim bound to the connection.
#[derive(Clone)]
pub(crate) struct Conn {
    jobs: mpsc::Sender<Job>,
    open: Arc<AtomicBool>,
    kill: Arc<Notify>,
    ended: tokio::sync::watch::Receiver<bool>,
    deadline: Duration,
}

impl Conn {
    /// Hand an authenticated stream to its request task.
    pub(crate) fn spawn(opened: Opened, deadline: Duration) -> Conn {
        let (jobs, rx) = mpsc::channel(16);
        let open = Arc::new(AtomicBool::new(true));
        let open_task = Arc::clone(&open);
        let kill = Arc::new(Notify::new());
        let kill_task = Arc::clone(&kill);
        let (ended_tx, ended) = tokio::sync::watch::channel(false);
        tokio::spawn(async move {
            serve(opened.reader, opened.writer, rx, open_task, kill_task).await;
            let _ = ended_tx.send(true);
        });
        Conn {
            jobs,
            open,
            kill,
            ended,
            deadline,
        }
    }

    /// Close the connection now (the server then releases a claim bound to it).
    pub(crate) fn close(&self) {
        self.open.store(false, Ordering::SeqCst);
        self.kill.notify_one();
    }

    /// Whether the connection still serves requests.
    pub(crate) fn is_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }

    /// Resolves once the connection stopped serving requests (a timeout, a close, the
    /// server gone).
    pub(crate) async fn ended(&self) {
        let mut ended = self.ended.clone();
        let _ = ended.wait_for(|ended| *ended).await;
    }

    /// Send one request line and wait for its reply, at most the deadline (queueing
    /// included). A timeout is [`Error::Unanswered`]; the connection is then closed.
    pub(crate) async fn request(&self, line: &str, framing: Framing) -> Result<Reply, Error> {
        self.request_within(line, framing, self.deadline).await
    }

    /// [`Self::request`] with its own deadline, for a request aterm may legitimately hold
    /// longer than the connection's usual one.
    pub(crate) async fn request_within(
        &self,
        line: &str,
        framing: Framing,
        deadline: Duration,
    ) -> Result<Reply, Error> {
        self.exchange(line, Vec::new(), framing, deadline).await
    }

    /// [`Self::request`] whose line announces `body.len()` bytes that follow it on the wire
    /// (`post … len=<n>`: the text never travels inside the line).
    pub(crate) async fn request_with_body(
        &self,
        line: &str,
        body: Vec<u8>,
        framing: Framing,
    ) -> Result<Reply, Error> {
        self.exchange(line, body, framing, self.deadline).await
    }

    async fn exchange(
        &self,
        line: &str,
        body: Vec<u8>,
        framing: Framing,
        deadline: Duration,
    ) -> Result<Reply, Error> {
        if line.is_empty() || line.contains(['\n', '\r']) {
            return Err(Error::BadRequest(line.to_string()));
        }
        if !self.is_open() {
            return Err(Error::Closed);
        }
        let (tx, rx) = oneshot::channel();
        let job = Job {
            line: line.to_string(),
            body,
            framing,
            deadline,
            reply: tx,
        };
        let wait = async {
            self.jobs.send(job).await.map_err(|_| Error::Closed)?;
            rx.await.map_err(|_| Error::Closed)?
        };
        match tokio::time::timeout(deadline, wait).await {
            Ok(result) => result,
            Err(_) => {
                // Unanswered is passive: the connection, and any claim on it, ends now.
                self.close();
                Err(Error::Unanswered)
            }
        }
    }
}

async fn serve(
    mut reader: BufReader<OwnedReadHalf>,
    mut writer: OwnedWriteHalf,
    mut jobs: mpsc::Receiver<Job>,
    open: Arc<AtomicBool>,
    kill: Arc<Notify>,
) {
    loop {
        let job = tokio::select! {
            job = jobs.recv() => job,
            () = kill.notified() => None,
        };
        let Some(job) = job else {
            break;
        };
        let exchange = async {
            let mut line = job.line.into_bytes();
            line.push(b'\n');
            line.extend_from_slice(&job.body);
            writer.write_all(&line).await?;
            writer.flush().await?;
            read_reply(&mut reader, job.framing).await
        };
        let result = tokio::select! {
            result = tokio::time::timeout(job.deadline, exchange) => match result {
                Ok(result) => result,
                Err(_) => Err(Error::Unanswered),
            },
            () = kill.notified() => Err(Error::Closed),
        };
        // A failed or late exchange leaves the stream out of step with its replies: close
        // it (fail closed) rather than read the next request's answer from it.
        let failed = result.is_err();
        let _ = job.reply.send(result);
        if failed {
            break;
        }
    }
    open.store(false, Ordering::SeqCst);
    jobs.close();
    while let Ok(job) = jobs.try_recv() {
        let _ = job.reply.send(Err(Error::Closed));
    }
}
