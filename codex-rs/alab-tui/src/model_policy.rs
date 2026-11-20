// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A same-login, process-bound application settings endpoint. Fleet authentication,
//! destination selection and authorization all belong to aterm.
use crate::identity;
use crate::record;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::UnixListener;
use tokio::net::UnixStream;
use tokio::sync::oneshot;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub pid: u32,
    pub identity: record::Identity,
    pub generation: u64,
    pub thread: String,
}

impl Target {
    pub fn read(dir: &Path, pid: u32) -> Option<Self> {
        let record = record::read(&record::path_for(dir, pid))?;
        let identity = record.identity?;
        (record.pid == pid
            && identity == (&identity::of_pid(pid).ok()?).into()
            && !matches!(
                record.status,
                record::Status::Exited | record::Status::Upgrading
            ))
        .then_some(Self {
            pid,
            identity,
            generation: record.exec_generation,
            thread: record.session_id?,
        })
    }
}

/// There is deliberately no generic command, file, configuration key or address on this wire.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Apply {
    pub operation: String,
    pub target: Target,
    pub selection: Selection,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Selection {
    #[serde(rename = "latest-sol-ultra")]
    LatestSolUltra,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "lowercase", deny_unknown_fields)]
pub enum Outcome {
    Applied { model: String, effort: String },
    Pending,
    Held,
    Unsupported,
    Overridden,
    Replaced,
}

pub fn operation_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

#[derive(Debug)]
pub enum Delivery {
    Identify(oneshot::Sender<Option<String>>),
    Apply {
        apply: Apply,
        reply: oneshot::Sender<Outcome>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "method", rename_all = "lowercase", deny_unknown_fields)]
enum Request {
    Identify,
    Apply(Apply),
}

fn owns_socket(stream: &UnixStream, pid: u32) -> io::Result<bool> {
    Ok(stream
        .peer_cred()?
        .pid()
        .and_then(|p| u32::try_from(p).ok())
        == Some(pid))
}

pub async fn identify(dir: &Path, pid: u32) -> Option<Target> {
    let read = async {
        let mut stream = UnixStream::connect(path(dir, pid)).await.ok()?;
        if !owns_socket(&stream, pid).ok()? {
            return None;
        }
        send(&mut stream, &Request::Identify).await.ok()?;
        let target = frame::<_, Option<Target>>(&mut stream).await.ok()??;
        (target.pid == pid && target.identity == (&identity::of_pid(pid).ok()?).into())
            .then_some(target)
    };
    tokio::time::timeout(DEADLINE, read).await.ok().flatten()
}

const DEADLINE: Duration = Duration::from_secs(10);

fn path(dir: &Path, pid: u32) -> PathBuf {
    dir.join(format!("{pid}.model.sock"))
}

async fn frame<S: tokio::io::AsyncRead + Unpin, T: serde::de::DeserializeOwned>(
    s: S,
) -> io::Result<T> {
    let mut line = Vec::new();
    BufReader::new(s.take(4097))
        .read_until(b'\n', &mut line)
        .await?;
    if line.len() > 4096 || line.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid model policy frame",
        ));
    }
    serde_json::from_slice(&line).map_err(io::Error::other)
}

async fn send<S: tokio::io::AsyncWrite + Unpin, T: Serialize>(
    mut s: S,
    value: &T,
) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    s.write_all(&bytes).await
}

pub async fn apply(dir: &Path, request: &Apply) -> Outcome {
    let io = async {
        let mut stream = UnixStream::connect(path(dir, request.target.pid)).await?;

        // A copied / old record or a socket rebound by a different process has no authority.
        if !owns_socket(&stream, request.target.pid)?
            || identity::of_pid(request.target.pid)
                .ok()
                .as_ref()
                .map(record::Identity::from)
                .as_ref()
                != Some(&request.target.identity)
        {
            return Ok(Outcome::Replaced);
        }
        send(&mut stream, &Request::Apply(request.clone())).await?;
        frame(&mut stream).await
    };
    match tokio::time::timeout(DEADLINE, io).await {
        Ok(Ok(receipt)) => receipt,
        Ok(Err(e)) if e.kind() == io::ErrorKind::NotFound => Outcome::Unsupported,
        _ => Outcome::Pending,
    }
}

pub struct Endpoint {
    task: tokio::task::JoinHandle<()>,
    socket: PathBuf,
    inode: u64,
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.task.abort();
        if std::fs::symlink_metadata(&self.socket).is_ok_and(|m| m.ino() == self.inode) {
            let _ = std::fs::remove_file(&self.socket);
        }
    }
}

pub fn listen(
    home: &Path,
    generation: u64,
    deliver: impl Fn(Delivery) + Send + 'static,
) -> io::Result<Endpoint> {
    let dir = record::sessions_dir(home);
    record::ensure_dir(&dir)?;
    let uid = std::fs::metadata(&dir)?.uid();
    let pid = std::process::id();
    let identity = record::Identity::from(&identity::current()?);
    let socket = path(&dir, pid);
    // Only the live owner of this PID can reach here. Refuse other file kinds and owners.
    if let Ok(meta) = std::fs::symlink_metadata(&socket) {
        if !meta.file_type().is_socket() || meta.uid() != uid {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "foreign endpoint",
            ));
        }
        std::fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    let inode = std::fs::symlink_metadata(&socket)?.ino();
    let task = tokio::spawn(async move {
        let mut receipts = BTreeMap::<String, (Target, Outcome)>::new();
        while let Ok((mut stream, _)) = listener.accept().await {
            if !stream.peer_cred().is_ok_and(|p| p.uid() == uid) {
                continue;
            }
            let Ok(Ok(request)) =
                tokio::time::timeout(DEADLINE, frame::<_, Request>(&mut stream)).await
            else {
                continue;
            };
            let apply = match request {
                Request::Identify => {
                    let (reply, wait) = oneshot::channel();
                    deliver(Delivery::Identify(reply));
                    let thread = tokio::time::timeout(DEADLINE, wait)
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .flatten();
                    let target = thread.map(|thread| Target {
                        pid,
                        identity: identity.clone(),
                        generation,
                        thread,
                    });
                    let _ = tokio::time::timeout(DEADLINE, send(&mut stream, &target)).await;
                    continue;
                }
                Request::Apply(apply) => apply,
            };
            if !operation_id(&apply.operation) || apply.target.pid != pid {
                continue;
            }
            let target = apply.target.clone();
            let operation = apply.operation.clone();
            let outcome = if target.generation != generation || target.identity != identity {
                Outcome::Replaced
            } else if let Some((original, receipt)) = receipts.get(&operation) {
                if *original == target {
                    receipt.clone()
                } else {
                    Outcome::Replaced
                }
            } else {
                let (reply, wait) = oneshot::channel();
                deliver(Delivery::Apply { apply, reply });
                let mut disconnected = [0u8; 1];
                tokio::select! {
                    result = tokio::time::timeout(DEADLINE, wait) => result.ok().and_then(Result::ok).unwrap_or(Outcome::Pending),
                    _ = stream.read(&mut disconnected) => continue,
                }
            };
            if matches!(outcome, Outcome::Applied { .. }) {
                receipts.insert(operation, (target, outcome.clone()));
            }
            let _ = tokio::time::timeout(DEADLINE, send(&mut stream, &outcome)).await;
        }
    });
    Ok(Endpoint {
        task,
        socket,
        inode,
    })
}

#[cfg(test)]
#[path = "model_policy_tests.rs"]
mod tests;
