// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Optional owner-managed OAuth refresh. The native credential document remains
//! unchanged; only an explicit slot binding delegates refresh to the host.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::future::Future;
use std::io;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

pub const SLOT_ENV: &str = "CODEX_AUTH_SLOT";
/// Read-only rollout capability; possession is never credential authority.
pub const CAPABILITY_MARK: &str = "aterm machine credential slot v1";
/// Native owners require this additional image capability before materializing
/// a slot whose initial refresh may have no known revision yet.
pub const PENDING_CAPABILITY_MARK: &str = "aterm machine credential slot pending v1";
pub const SLOT_MARKER_SUFFIX: &str = ".aterm-slot";
pub const SLOT_STATE_SUFFIX: &str = ".aterm-slot-state";
pub const REFRESH_JOURNAL_SUFFIX: &str = ".refreshed";
pub const SLOT_MARKER_PREFIX: &str = "ATERM-CODEX-AUTH-SLOT 1 ";
pub const MAX_DOCUMENT_BYTES: usize = 16 * 1024;
const MAX_SLOTS: usize = 256;
const MAX_PENDING_REVISIONS: usize = 128;

pub fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Canonical lowercase hex encoding of the UTF-8 registry key, never a token.
pub fn valid_slot(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.len().is_multiple_of(2)
        && valid_hex(value, value.len())
        && String::from_utf8(
            value
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| {
                    let nibble = |byte: u8| {
                        if byte.is_ascii_digit() {
                            byte - b'0'
                        } else {
                            byte - b'a' + 10
                        }
                    };
                    (nibble(pair[0]) << 4) | nibble(pair[1])
                })
                .collect(),
        )
        .is_ok_and(|key| !key.chars().any(char::is_control))
}

pub fn unavailable() -> io::Error {
    io::Error::other(format!(
        "{CAPABILITY_MARK}; {PENDING_CAPABILITY_MARK}: managed credential refresh is unavailable; no token was sent by Codex."
    ))
}

/// A malformed marked slot is an error, never permission to use native refresh.
pub fn binding_of(
    alab: bool,
    value: Option<&OsStr>,
    admitted_path: bool,
) -> io::Result<Option<String>> {
    if !alab {
        return Ok(None);
    }
    let Some(value) = value else { return Ok(None) };
    value
        .to_str()
        .filter(|value| admitted_path && valid_slot(value))
        .map(|value| Some(value.to_owned()))
        .ok_or_else(unavailable)
}

pub fn marker_path(auth_file: &Path) -> PathBuf {
    beside(auth_file, SLOT_MARKER_SUFFIX)
}

pub fn state_path(auth_file: &Path) -> PathBuf {
    beside(auth_file, SLOT_STATE_SUFFIX)
}

pub fn refresh_journal_path(auth_file: &Path) -> PathBuf {
    beside(auth_file, REFRESH_JOURNAL_SUFFIX)
}

fn beside(auth_file: &Path, suffix: &str) -> PathBuf {
    let mut name = auth_file.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn exists(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(unavailable()),
    }
}

/// The owning host resolves this journal. Its presence cannot authorize a
/// second exchange, including when it is unreadable or has an unknown version.
pub fn refresh_pending(auth_file: &Path) -> io::Result<bool> {
    exists(&refresh_journal_path(auth_file))
}

pub fn custody_present(auth_file: &Path) -> io::Result<bool> {
    Ok(exists(&marker_path(auth_file))? || exists(&state_path(auth_file))?)
}

/// The marker is a private explicit association, not discovery of arbitrary
/// credentials. An optional environment carrier cannot contradict it.
pub fn binding_for_path(
    alab: bool,
    value: Option<&OsStr>,
    admitted_path: Option<&Path>,
) -> io::Result<Option<String>> {
    let carrier = binding_of(alab, value, admitted_path.is_some())?;
    if !alab {
        return Ok(None);
    }
    let Some(path) = admitted_path else {
        return Ok(carrier);
    };
    let marker = marker_path(path);
    match std::fs::symlink_metadata(&marker) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return if exists(&state_path(path))? {
                Err(unavailable())
            } else {
                Ok(carrier)
            };
        }
        Err(_) => return Err(unavailable()),
        Ok(_) => {}
    }
    let bytes = read_document(&marker)?;
    let slot = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|line| line.strip_prefix(SLOT_MARKER_PREFIX))
        .and_then(|line| line.strip_suffix('\n'))
        .filter(|slot| valid_slot(slot))
        .ok_or_else(unavailable)?;
    if carrier.as_deref().is_some_and(|carrier| carrier != slot) {
        return Err(unavailable());
    }
    Ok(Some(slot.to_owned()))
}

/// Read one exact private native document, without following a final symlink or
/// blocking on a special file. The host hashes and seals these same bytes.
pub fn read_document(path: &Path) -> io::Result<zeroize::Zeroizing<Vec<u8>>> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|_| unavailable())?;
    let metadata = file.metadata().map_err(|_| unavailable())?;
    if !metadata.is_file() || metadata.len() > MAX_DOCUMENT_BYTES as u64 {
        return Err(unavailable());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        // SAFETY: geteuid takes no arguments and has no failure mode.
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
        {
            return Err(unavailable());
        }
    }
    // One bounded allocation avoids leaving previous plaintext allocations behind
    // while read_to_end grows a buffer. Zeroizing also covers read/size failures.
    let mut bytes = zeroize::Zeroizing::new(Vec::with_capacity(MAX_DOCUMENT_BYTES + 1));
    file.take(MAX_DOCUMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unavailable())?;
    if bytes.is_empty() || bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(unavailable());
    }
    Ok(bytes)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub slot: String,
    pub operation: String,
    pub expected_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    pub generation: u64,
    pub digest: String,
}

pub type RefreshFuture<'a> = Pin<Box<dyn Future<Output = io::Result<Revision>> + Send + 'a>>;

pub trait Host: Send + Sync {
    fn refresh<'a>(&'a self, request: &'a Request) -> RefreshFuture<'a>;
}

struct Pending {
    request: Request,
    ready: Option<Revision>,
}

#[derive(Default)]
struct Slot {
    serial: tokio::sync::Mutex<()>,
    receipts: Mutex<BTreeMap<String, Arc<Mutex<Pending>>>>,
}

/// Serializes callers of one slot while retaining an uncertain operation for
/// exact retry. Other slots proceed independently. The host remains the durable
/// authority across process exit, cancellation and machines.
pub struct Coordinator {
    host: Arc<dyn Host>,
    slots: Mutex<BTreeMap<String, Arc<Slot>>>,
}

impl Coordinator {
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self {
            host,
            slots: Mutex::default(),
        }
    }

    #[expect(
        clippy::await_holding_invalid_type,
        reason = "one slot's owner operation must stay serialized until acknowledgement; other slots have separate guards"
    )]
    pub async fn refresh(&self, slot: &str, expected_digest: &str) -> io::Result<Revision> {
        if !valid_slot(slot) || !valid_hex(expected_digest, 64) {
            return Err(unavailable());
        }
        let selected = {
            let mut slots = self.slots.lock().map_err(|_| unavailable())?;
            if !slots.contains_key(slot) && slots.len() >= MAX_SLOTS {
                return Err(unavailable());
            }
            Arc::clone(slots.entry(slot.to_owned()).or_default())
        };
        // Capture an immutable receipt before waiting for serialization. A
        // queued new revision must never mutate or detach an older receipt.
        let entry = {
            let mut receipts = selected.receipts.lock().map_err(|_| unavailable())?;
            if !receipts.contains_key(expected_digest) && receipts.len() >= MAX_PENDING_REVISIONS {
                return Err(unavailable());
            }
            Arc::clone(
                receipts
                    .entry(expected_digest.to_owned())
                    .or_insert_with(|| {
                        Arc::new(Mutex::new(Pending {
                            request: request(slot, expected_digest),
                            ready: None,
                        }))
                    }),
            )
        };
        let _serial = selected.serial.lock().await;
        let request = {
            let pending = entry.lock().map_err(|_| unavailable())?;
            if let Some(ready) = &pending.ready {
                return Ok(ready.clone());
            }
            pending.request.clone()
        };
        let revision = self
            .host
            .refresh(&request)
            .await
            .map_err(|_| unavailable())?;
        if revision.generation == 0 || !valid_hex(&revision.digest, 64) {
            return Err(unavailable());
        }
        entry.lock().map_err(|_| unavailable())?.ready = Some(revision.clone());
        // Queued callers retain this completed entry, but a later forced refresh
        // must be a new operation even if publication preserved identical bytes.
        let mut receipts = selected.receipts.lock().map_err(|_| unavailable())?;
        if receipts
            .get(expected_digest)
            .is_some_and(|current| Arc::ptr_eq(current, &entry))
        {
            receipts.remove(expected_digest);
        }
        Ok(revision)
    }
}

fn request(slot: &str, expected_digest: &str) -> Request {
    Request {
        slot: slot.to_owned(),
        operation: format!("{:032x}", rand::random::<u128>()),
        expected_digest: expected_digest.to_owned(),
    }
}

static HOST: OnceLock<Coordinator> = OnceLock::new();

/// Installed by the final CLI through its owned launch hook, never by a model tool.
pub fn install(host: Arc<dyn Host>) {
    let _ = HOST.set(Coordinator::new(host));
}

pub async fn refresh(slot: &str, expected_digest: &str) -> io::Result<Revision> {
    HOST.get()
        .ok_or_else(unavailable)?
        .refresh(slot, expected_digest)
        .await
}

#[cfg(test)]
#[path = "managed_auth_tests.rs"]
mod tests;
