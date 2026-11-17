// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Durable generation witness for host goal mutations whose rollout settlement is unknown.
//!
//! The marker is an append-only journal rather than a replaced file, so generation rotation has
//! no rename crash window. Rollback validates its exact suffix; restoring initial absence removes
//! the marker only under the stable separate lock and syncs its parent. A model stop may carry a
//! generation only while it owns the same native mutation permit as its SQLite commit.

use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::Read as _;
use std::io::Seek as _;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

const MARKER_DIR: [&str; 2] = ["alab", "goal-chronology"];
const MARKER_SUFFIX: &str = ".fence";
const JOURNAL_HEADER: &str = "alab-goal-mutation-fence-v3\n";
const MAX_JOURNAL_BYTES: u64 = 1024 * 1024;
const COMPACT_AFTER_BYTES: u64 = 64 * 1024;
const TERMINAL_RECORD_BYTES: u64 = 74;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FenceGeneration([u8; 32]);

impl FenceGeneration {
    pub fn from_hex(value: &str) -> Option<Self> {
        let value = value.as_bytes();
        if value.len() != 64 {
            return None;
        }
        let mut bytes = [0_u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            let high = hex_nibble(value[index * 2])?;
            let low = hex_nibble(value[index * 2 + 1])?;
            *byte = high << 4 | low;
        }
        Some(Self(bytes))
    }

    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn random() -> Self {
        Self(rand::random())
    }
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct FenceLease {
    generation: FenceGeneration,
    predecessor_len: u64,
}

enum DispatchState {
    Idle,
    Armed(FenceLease),
}

struct DispatchFence {
    codex_home: PathBuf,
    thread_id: String,
    state: Mutex<DispatchState>,
    // Pre-SQL classification and instruction persistence are part of the same
    // cross-process mutation interval as arm, SQL and settlement.
    _journal_lock: File,
}

/// A stable cross-process read of the latest host-mutation generation.
///
/// Keeping this value alive excludes another process's host mutation. Callers
/// may therefore carry a validated model/provider stop through final native
/// queue admission without a read-to-dispatch chronology gap.
pub struct ReadGuard {
    generation: Option<FenceGeneration>,
    _journal_lock: File,
}

impl ReadGuard {
    pub fn generation(&self) -> Option<FenceGeneration> {
        self.generation
    }
}

tokio::task_local! {
    static DISPATCH_FENCE: Arc<DispatchFence>;
}

/// Own this thread's stable cross-process journal lock before polling any
/// mutation work, and retain it until that work returns or is cancelled.
pub async fn scope<T>(
    codex_home: PathBuf,
    thread_id: String,
    future: impl std::future::Future<Output = T>,
) -> io::Result<T> {
    let journal_lock = acquire_journal_lock(&codex_home, &thread_id)?;
    Ok(DISPATCH_FENCE
        .scope(
            Arc::new(DispatchFence {
                codex_home,
                thread_id,
                state: Mutex::new(DispatchState::Idle),
                _journal_lock: journal_lock,
            }),
            future,
        )
        .await)
}

/// Rotate the durable generation only after every pre-SQL gate has succeeded.
pub fn arm_for_dispatch() -> io::Result<()> {
    DISPATCH_FENCE
        .try_with(|fence| {
            let mut state = fence
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if matches!(*state, DispatchState::Armed(_)) {
                return Err(io::Error::other("goal chronology dispatch was armed twice"));
            }
            let (lease, append_result) =
                append_arm_owned_locked(&fence.codex_home, &fence.thread_id)?;
            // Keep the lease even when append/fsync reports failure: the arm may
            // already be durable, and the caller's definite rollback must be
            // able to restore the exact predecessor.
            *state = DispatchState::Armed(lease);
            append_result
        })
        .unwrap_or(Ok(()))
}

/// A definitely uncommitted write restores the predecessor by popping only its own arm.
pub fn restore_dispatch_fence() -> io::Result<()> {
    DISPATCH_FENCE
        .try_with(|fence| {
            let mut state = fence
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let lease = match &*state {
                DispatchState::Idle => return Ok(()),
                DispatchState::Armed(lease) => *lease,
            };
            let result = truncate_to_predecessor_locked(&fence.codex_home, &fence.thread_id, lease);
            if result.is_ok() {
                *state = DispatchState::Idle;
            }
            result
        })
        .unwrap_or(Ok(()))
}

/// A durable host chronology retires the unresolved stack while retaining this
/// generation as the latest durable host-mutation serial.
pub fn settle_dispatch_fence() -> io::Result<()> {
    DISPATCH_FENCE
        .try_with(|fence| {
            let mut state = fence
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let lease = match &*state {
                DispatchState::Idle => return Ok(()),
                DispatchState::Armed(lease) => *lease,
            };
            let result = append_terminal_locked(
                &fence.codex_home,
                &fence.thread_id,
                "settle",
                lease.generation,
            );
            if result.is_ok() {
                *state = DispatchState::Idle;
            }
            result
        })
        .unwrap_or(Ok(()))
}

pub fn marker_path(codex_home: &Path, thread_id: &str) -> io::Result<PathBuf> {
    if codex_home.as_os_str().is_empty()
        || thread_id.is_empty()
        || !thread_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "goal chronology fence requires a Codex home and safe thread ID",
        ));
    }
    let directory = MARKER_DIR
        .iter()
        .fold(codex_home.to_path_buf(), |path, component| {
            path.join(component)
        });
    Ok(directory.join(format!("{thread_id}{MARKER_SUFFIX}")))
}

/// Own one complete journal operation across processes. The lockfile is stable
/// and never unlinked: removing a locked path could let a second process open a
/// different inode and bypass the first owner's lock.
fn acquire_journal_lock(codex_home: &Path, thread_id: &str) -> io::Result<File> {
    let marker = marker_path(codex_home, thread_id)?;
    let parent = marker
        .parent()
        .ok_or_else(|| io::Error::other("goal chronology marker has no parent"))?;
    create_marker_directories(codex_home, parent)?;
    let lock_path = marker.with_extension("lock");
    let existed = match std::fs::symlink_metadata(&lock_path) {
        Ok(metadata) if metadata.file_type().is_file() => true,
        Ok(_) => {
            return Err(io::Error::other(
                "goal chronology lock is not a regular file",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(&lock_path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::other(
            "goal chronology lock is not a regular file",
        ));
    }
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "goal chronology journal is locked",
            ));
        }
        Err(std::fs::TryLockError::Error(error)) => return Err(error),
    }
    if !existed {
        file.sync_all()?;
        sync_directory(parent)?;
    }
    Ok(file)
}

fn owns_dispatch_journal(codex_home: &Path, thread_id: &str) -> bool {
    DISPATCH_FENCE
        .try_with(|fence| fence.codex_home == codex_home && fence.thread_id == thread_id)
        .unwrap_or(false)
}

/// Read the latest unresolved generation. Unknown or truncated journals fail closed.
pub fn read_active(codex_home: &Path, thread_id: &str) -> io::Result<Option<FenceGeneration>> {
    if owns_dispatch_journal(codex_home, thread_id) {
        return read_active_locked(codex_home, thread_id);
    }
    let _journal = acquire_journal_lock(codex_home, thread_id)?;
    read_active_locked(codex_home, thread_id)
}

fn read_active_locked(codex_home: &Path, thread_id: &str) -> io::Result<Option<FenceGeneration>> {
    let Some(bytes) = read_journal_locked(codex_home, thread_id)? else {
        return Ok(None);
    };
    if bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "goal chronology journal has no latest mutation serial",
        ));
    }
    let state = replay(&bytes)?;
    if state.latest.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "goal chronology journal has no mutation serial",
        ));
    }
    Ok(state.active.last().copied())
}

/// Read the generation of the latest host mutation, whether unresolved or settled.
/// A settled generation remains durable so another process cannot round-trip a
/// goal to identical fields and thereby resurrect older recovery proof.
pub fn read_current(codex_home: &Path, thread_id: &str) -> io::Result<Option<FenceGeneration>> {
    if owns_dispatch_journal(codex_home, thread_id) {
        return read_current_locked(codex_home, thread_id);
    }
    let _journal = acquire_journal_lock(codex_home, thread_id)?;
    read_current_locked(codex_home, thread_id)
}

/// Read the latest durable generation while retaining the stable journal lock.
/// Contention is fail-closed (`WouldBlock`), as it is for host mutation scope.
pub fn lock_current(codex_home: &Path, thread_id: &str) -> io::Result<ReadGuard> {
    let journal_lock = acquire_journal_lock(codex_home, thread_id)?;
    let generation = read_current_locked(codex_home, thread_id)?;
    Ok(ReadGuard {
        generation,
        _journal_lock: journal_lock,
    })
}

fn read_current_locked(codex_home: &Path, thread_id: &str) -> io::Result<Option<FenceGeneration>> {
    let Some(bytes) = read_journal_locked(codex_home, thread_id)? else {
        return Ok(None);
    };
    if bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "goal chronology journal has no latest mutation serial",
        ));
    }
    let state = replay(&bytes)?;
    state
        .latest
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "goal chronology journal has no mutation serial",
            )
        })
        .map(Some)
}

pub fn is_armed(codex_home: &Path, thread_id: &str) -> io::Result<bool> {
    read_active(codex_home, thread_id).map(|generation| generation.is_some())
}

/// Rotate a marker outside task-local dispatch scope, for recovery tests.
pub fn arm(codex_home: &Path, thread_id: &str) -> io::Result<FenceGeneration> {
    let _journal = acquire_journal_lock(codex_home, thread_id)?;
    let (lease, append_result) = append_arm_owned_locked(codex_home, thread_id)?;
    append_result?;
    Ok(lease.generation)
}

pub fn clear_if_current(
    codex_home: &Path,
    thread_id: &str,
    expected: FenceGeneration,
) -> io::Result<bool> {
    let _journal = acquire_journal_lock(codex_home, thread_id)?;
    if read_active_locked(codex_home, thread_id)? != Some(expected) {
        return Ok(false);
    }
    append_terminal_locked(codex_home, thread_id, "settle", expected)?;
    Ok(true)
}

pub fn clear(codex_home: &Path, thread_id: &str) -> io::Result<()> {
    let _journal = acquire_journal_lock(codex_home, thread_id)?;
    if let Some(generation) = read_active_locked(codex_home, thread_id)? {
        append_terminal_locked(codex_home, thread_id, "settle", generation)?;
    }
    Ok(())
}

fn append_arm_owned_locked(
    codex_home: &Path,
    thread_id: &str,
) -> io::Result<(FenceLease, io::Result<()>)> {
    // Validate the complete predecessor before extending it. A torn or unknown
    // journal can never be converted into permission by a later request.
    let predecessor_len = match read_journal_locked(codex_home, thread_id)? {
        None => 0,
        Some(bytes) => {
            if !bytes.is_empty() {
                let _ = replay(&bytes)?;
            }
            bytes.len() as u64
        }
    };
    let generation = FenceGeneration::random();
    let lease = FenceLease {
        generation,
        predecessor_len,
    };
    let result = append_record_locked(codex_home, thread_id, "arm", generation);
    Ok((lease, result))
}

fn truncate_to_predecessor_locked(
    codex_home: &Path,
    thread_id: &str,
    lease: FenceLease,
) -> io::Result<()> {
    let marker = marker_path(codex_home, thread_id)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = match options.open(&marker) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound && lease.predecessor_len == 0 => {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let current_len = file.metadata()?.len();
    if current_len < lease.predecessor_len {
        return Err(io::Error::other(
            "goal chronology journal shrank before rollback",
        ));
    }
    let arm_record = format!("arm {}\n", lease.generation.to_hex());
    let expected_suffix = if lease.predecessor_len == 0 {
        format!("{JOURNAL_HEADER}{arm_record}")
    } else {
        arm_record
    };
    let tail_len = usize::try_from(current_len - lease.predecessor_len)
        .map_err(|_| io::Error::other("goal chronology rollback length overflow"))?;
    if tail_len > expected_suffix.len() {
        return Err(io::Error::other(
            "goal chronology journal advanced before rollback",
        ));
    }
    file.seek(io::SeekFrom::Start(lease.predecessor_len))?;
    let mut tail = vec![0_u8; tail_len];
    file.read_exact(&mut tail)?;
    if tail != expected_suffix.as_bytes()[..tail_len] {
        return Err(io::Error::other(
            "goal chronology journal changed before rollback",
        ));
    }
    // Absence is the exact predecessor of the first arm. An empty journal is
    // deliberately invalid because arbitrary truncation must fail closed, so
    // remove this exact arm while retaining the stable lock. A crash exposes
    // either the old fail-closed arm or the safe absent predecessor.
    if lease.predecessor_len == 0 {
        let parent = marker
            .parent()
            .ok_or_else(|| io::Error::other("goal chronology marker has no parent"))?;
        drop(file);
        std::fs::remove_file(&marker)?;
        return sync_directory(parent);
    }

    file.set_len(lease.predecessor_len)?;
    file.sync_all()
}

fn append_terminal_locked(
    codex_home: &Path,
    thread_id: &str,
    operation: &str,
    generation: FenceGeneration,
) -> io::Result<()> {
    if read_active_locked(codex_home, thread_id)? != Some(generation) {
        return Err(io::Error::other(
            "goal chronology generation changed before settlement",
        ));
    }
    append_record_locked(codex_home, thread_id, operation, generation)?;
    compact_if_inactive_locked(codex_home, thread_id)
}

#[derive(Default)]
struct JournalState {
    latest: Option<FenceGeneration>,
    active: Vec<FenceGeneration>,
}

fn replay(bytes: &[u8]) -> io::Result<JournalState> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 goal fence"))?;
    if !text.ends_with('\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "truncated goal chronology journal",
        ));
    }
    let mut lines = text.lines();
    if lines.next() != Some(JOURNAL_HEADER.trim_end()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unknown goal chronology journal",
        ));
    }
    let mut state = JournalState::default();
    for line in lines {
        let (operation, value) = line.split_once(' ').ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "malformed goal fence record")
        })?;
        let generation = FenceGeneration::from_hex(value).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "malformed goal fence generation",
            )
        })?;
        match operation {
            "arm" => {
                state.latest = Some(generation);
                state.active.push(generation);
            }
            "settle" => {
                if state.active.last().copied() != Some(generation) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "stale goal fence settlement",
                    ));
                }
                state.active.clear();
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown goal fence operation",
                ));
            }
        }
    }
    Ok(state)
}

fn read_journal_locked(codex_home: &Path, thread_id: &str) -> io::Result<Option<Vec<u8>>> {
    let marker = marker_path(codex_home, thread_id)?;
    let metadata = match std::fs::symlink_metadata(&marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_JOURNAL_BYTES {
        return Err(io::Error::other(
            "goal chronology journal is not a bounded regular file",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(marker)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(io::Error::other(
            "goal chronology journal grew while reading",
        ));
    }
    Ok(Some(bytes))
}

fn append_record_locked(
    codex_home: &Path,
    thread_id: &str,
    operation: &str,
    generation: FenceGeneration,
) -> io::Result<()> {
    let marker = marker_path(codex_home, thread_id)?;
    let parent = marker
        .parent()
        .ok_or_else(|| io::Error::other("goal chronology marker has no parent"))?;
    create_marker_directories(codex_home, parent)?;
    let record = format!("{operation} {}\n", generation.to_hex());
    let current_len = std::fs::metadata(&marker).map_or(0, |metadata| metadata.len());
    let header_len = if current_len == 0 {
        JOURNAL_HEADER.len() as u64
    } else {
        0
    };
    let appended_len = header_len
        .checked_add(record.len() as u64)
        .and_then(|length| current_len.checked_add(length))
        .ok_or_else(|| io::Error::other("goal chronology journal size overflow"))?;
    let required_len = if operation == "arm" {
        appended_len.checked_add(TERMINAL_RECORD_BYTES)
    } else {
        Some(appended_len)
    }
    .ok_or_else(|| io::Error::other("goal chronology journal size overflow"))?;
    if required_len > MAX_JOURNAL_BYTES {
        return Err(io::Error::other("goal chronology journal is full"));
    }
    let existed = std::fs::symlink_metadata(&marker).is_ok();
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(&marker)?;
    if file.metadata()?.len() == 0 {
        file.write_all(JOURNAL_HEADER.as_bytes())?;
    }
    file.write_all(record.as_bytes())?;
    file.flush()?;
    file.sync_all()?;
    if !existed {
        sync_directory(parent)?;
    }
    Ok(())
}

fn compact_if_inactive_locked(codex_home: &Path, thread_id: &str) -> io::Result<()> {
    let marker = marker_path(codex_home, thread_id)?;
    let metadata = std::fs::metadata(&marker)?;
    if metadata.len() < COMPACT_AFTER_BYTES || read_active_locked(codex_home, thread_id)?.is_some()
    {
        return Ok(());
    }
    // Preserve the latest settled generation while bounding journal growth.
    // It is the durable serial that invalidates proof held by another process.
    let latest = read_current_locked(codex_home, thread_id)?;
    let canonical = latest.map_or_else(String::new, |generation| {
        let generation = generation.to_hex();
        format!("{JOURNAL_HEADER}arm {generation}\nsettle {generation}\n")
    });
    let mut options = OpenOptions::new();
    options.write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(marker)?;
    file.write_all(canonical.as_bytes())?;
    file.flush()?;
    file.sync_all()
}

fn create_marker_directories(codex_home: &Path, parent: &Path) -> io::Result<()> {
    let mut directories = std::fs::DirBuilder::new();
    directories.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        directories.mode(0o700);
    }
    directories.create(parent)?;
    sync_directory_chain(codex_home, parent)
}

/// Make the directory entry for a previously synced file durable through
/// `codex_home` where the platform exposes a directory synchronization API.
pub fn sync_parent_chain(codex_home: &Path, file: &Path) -> io::Result<()> {
    let parent = file
        .parent()
        .ok_or_else(|| io::Error::other("durable file has no parent directory"))?;
    sync_directory_chain(codex_home, parent)
}

fn sync_directory_chain(codex_home: &Path, parent: &Path) -> io::Result<()> {
    let relative = parent.strip_prefix(codex_home).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "durable file is outside the Codex home",
        )
    })?;
    let mut directories = vec![codex_home.to_path_buf()];
    let mut current = codex_home.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "durable path contains a non-normal component",
            ));
        };
        current.push(component);
        directories.push(current.clone());
    }
    for directory in directories.iter().rev() {
        sync_directory(directory)?;
    }
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(windows)]
fn sync_directory(_path: &Path) -> io::Result<()> {
    // Windows exposes no documented standard-user directory-flush sequence.
    // File::sync_all above still calls FlushFileBuffers for every journal and
    // record write, including its file metadata. Treat that as Windows' native
    // durability boundary instead of making every goal mutation unsupported.
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn sync_directory(path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "durable directory flush is unsupported on this platform: {}",
            path.display()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREAD: &str = "019a1234-5678-7abc-8def-0123456789ab";
    const LOCK_PROBE_HOME: &str = "ALAB_GOAL_FENCE_LOCK_PROBE_HOME";
    const LOCK_PROBE_BLOCKED: &str = "ALAB_GOAL_FENCE_LOCK_PROBE_BLOCKED";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "alab-goal-fence-{}-{:032x}",
                std::process::id(),
                rand::random::<u128>()
            ));
            std::fs::create_dir(&path).expect("create goal fence scratch directory");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn assert_other_process_lock(home: &Path, blocked: bool) {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("journal_lock_probe_process")
            .arg("--nocapture")
            .env(LOCK_PROBE_HOME, home)
            .env(LOCK_PROBE_BLOCKED, if blocked { "1" } else { "0" })
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "cross-process lock probe failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    #[test]
    fn journal_lock_probe_process() {
        let Some(home) = std::env::var_os(LOCK_PROBE_HOME) else {
            return;
        };
        let blocked =
            std::env::var_os(LOCK_PROBE_BLOCKED).as_deref() == Some(std::ffi::OsStr::new("1"));
        match (blocked, acquire_journal_lock(Path::new(&home), THREAD)) {
            (true, Err(error)) => assert_eq!(error.kind(), io::ErrorKind::WouldBlock),
            (false, Ok(_lock)) => {}
            (true, Ok(_lock)) => panic!("another process bypassed the mutation-interval lock"),
            (false, Err(error)) => panic!("released journal lock stayed unavailable: {error}"),
        }
    }

    #[test]
    fn path_is_stable_and_ids_are_validated() {
        let home = Path::new("/codex-home");
        assert_eq!(
            marker_path(home, THREAD).unwrap(),
            Path::new(
                "/codex-home/alab/goal-chronology/019a1234-5678-7abc-8def-0123456789ab.fence"
            )
        );
        assert!(marker_path(home, "../escape").is_err());
        assert!(marker_path(Path::new(""), THREAD).is_err());
    }

    #[test]
    fn generations_rotate_and_stale_settlement_cannot_remove_a_successor() {
        let scratch = Scratch::new();
        let first = arm(&scratch.0, THREAD).unwrap();
        let second = arm(&scratch.0, THREAD).unwrap();
        assert_ne!(first, second);
        assert!(!clear_if_current(&scratch.0, THREAD, first).unwrap());
        assert_eq!(read_active(&scratch.0, THREAD).unwrap(), Some(second));
        assert!(clear_if_current(&scratch.0, THREAD, second).unwrap());
        assert_eq!(read_active(&scratch.0, THREAD).unwrap(), None);
        assert_eq!(read_current(&scratch.0, THREAD).unwrap(), Some(second));
    }

    #[test]
    fn generation_hex_rejects_non_ascii_and_uppercase_without_panicking() {
        assert!(FenceGeneration::from_hex(&"é".repeat(32)).is_none());
        assert!(FenceGeneration::from_hex(&"A".repeat(64)).is_none());
    }

    #[test]
    fn malformed_truncated_and_non_file_entries_fail_closed() {
        let scratch = Scratch::new();
        let marker = marker_path(&scratch.0, THREAD).unwrap();
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, b"unknown\n").unwrap();
        assert!(read_active(&scratch.0, THREAD).is_err());
        std::fs::write(&marker, JOURNAL_HEADER.as_bytes()).unwrap();
        assert!(read_active(&scratch.0, THREAD).is_err());
        assert!(read_current(&scratch.0, THREAD).is_err());
        let mut file = OpenOptions::new().append(true).open(&marker).unwrap();
        file.write_all(b"arm 00").unwrap();
        assert!(read_active(&scratch.0, THREAD).is_err());
        drop(file);
        std::fs::remove_file(&marker).unwrap();
        std::fs::create_dir(&marker).unwrap();
        assert!(read_active(&scratch.0, THREAD).is_err());
    }

    #[test]
    fn journal_lock_contention_fails_closed_without_mutating_the_marker() {
        let scratch = Scratch::new();
        let generation = arm(&scratch.0, THREAD).unwrap();
        let marker = marker_path(&scratch.0, THREAD).unwrap();
        let before = std::fs::read(&marker).unwrap();
        let held = acquire_journal_lock(&scratch.0, THREAD).unwrap();

        for error in [
            read_active(&scratch.0, THREAD).unwrap_err(),
            read_current(&scratch.0, THREAD).unwrap_err(),
            arm(&scratch.0, THREAD).unwrap_err(),
            clear_if_current(&scratch.0, THREAD, generation).unwrap_err(),
            clear(&scratch.0, THREAD).unwrap_err(),
        ] {
            assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        }
        assert_eq!(std::fs::read(&marker).unwrap(), before);

        drop(held);
        assert_eq!(read_active(&scratch.0, THREAD).unwrap(), Some(generation));
    }

    #[test]
    fn current_read_guard_retains_the_exact_generation_through_queue_admission() {
        let scratch = Scratch::new();
        let generation = arm(&scratch.0, THREAD).unwrap();
        assert!(clear_if_current(&scratch.0, THREAD, generation).unwrap());

        let guard = lock_current(&scratch.0, THREAD).expect("lock the stable chronology journal");
        assert_eq!(guard.generation(), Some(generation));
        assert_other_process_lock(&scratch.0, true);
        assert_eq!(
            arm(&scratch.0, THREAD).unwrap_err().kind(),
            io::ErrorKind::WouldBlock,
            "a host mutation cannot cross the final admission span"
        );
        assert_eq!(guard.generation(), Some(generation));

        drop(guard);
        assert_other_process_lock(&scratch.0, false);
        let successor = arm(&scratch.0, THREAD).expect("released admission admits host mutation");
        assert_ne!(successor, generation);
    }

    #[tokio::test]
    async fn definite_rollback_reveals_the_predecessor_generation() {
        let scratch = Scratch::new();
        let predecessor = arm(&scratch.0, THREAD).unwrap();
        assert!(clear_if_current(&scratch.0, THREAD, predecessor).unwrap());
        scope(scratch.0.clone(), THREAD.to_owned(), async {
            arm_for_dispatch().unwrap();
            assert_ne!(read_active(&scratch.0, THREAD).unwrap(), Some(predecessor));
            restore_dispatch_fence().unwrap();
        })
        .await
        .unwrap();
        assert_eq!(read_active(&scratch.0, THREAD).unwrap(), None);
        assert_eq!(read_current(&scratch.0, THREAD).unwrap(), Some(predecessor));
    }

    #[tokio::test]
    async fn definite_first_dispatch_rollback_restores_absent_journal() {
        let scratch = Scratch::new();
        scope(scratch.0.clone(), THREAD.to_owned(), async {
            arm_for_dispatch().unwrap();
            assert!(read_active(&scratch.0, THREAD).unwrap().is_some());
            restore_dispatch_fence().unwrap();
            assert_eq!(read_active(&scratch.0, THREAD).unwrap(), None);
            assert_eq!(read_current(&scratch.0, THREAD).unwrap(), None);
        })
        .await
        .unwrap();
        assert_eq!(read_active(&scratch.0, THREAD).unwrap(), None);
        assert_eq!(read_current(&scratch.0, THREAD).unwrap(), None);
        assert!(!marker_path(&scratch.0, THREAD).unwrap().exists());
    }

    #[tokio::test]
    async fn dispatch_lock_excludes_a_second_owner_until_settlement() {
        let scratch = Scratch::new();
        scope(scratch.0.clone(), THREAD.to_owned(), async {
            arm_for_dispatch().unwrap();
            let first = read_active(&scratch.0, THREAD).unwrap().unwrap();
            assert_eq!(read_current(&scratch.0, THREAD).unwrap(), Some(first));
            assert_other_process_lock(&scratch.0, true);

            let error = scope(scratch.0.clone(), THREAD.to_owned(), async {
                arm_for_dispatch().unwrap_err()
            })
            .await
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
            assert_eq!(
                read_active_locked(&scratch.0, THREAD).unwrap(),
                Some(first),
                "a second owner cannot arm and settle ahead of the first"
            );
            settle_dispatch_fence().unwrap();
        })
        .await
        .unwrap();
        assert_other_process_lock(&scratch.0, false);
        let first = read_current(&scratch.0, THREAD).unwrap().unwrap();
        scope(scratch.0.clone(), THREAD.to_owned(), async {
            arm_for_dispatch().unwrap();
            assert_ne!(read_current(&scratch.0, THREAD).unwrap(), Some(first));
            settle_dispatch_fence().unwrap();
        })
        .await
        .unwrap();
        assert_eq!(read_active(&scratch.0, THREAD).unwrap(), None);
        assert_ne!(read_current(&scratch.0, THREAD).unwrap(), Some(first));
    }

    #[tokio::test]
    async fn failed_settlement_retains_the_interval_lock_and_exact_lease() {
        let scratch = Scratch::new();
        scope(scratch.0.clone(), THREAD.to_owned(), async {
            arm_for_dispatch().unwrap();
            let marker = marker_path(&scratch.0, THREAD).unwrap();
            let valid = std::fs::read(&marker).unwrap();
            OpenOptions::new()
                .append(true)
                .open(&marker)
                .unwrap()
                .write_all(b"corrupt\n")
                .unwrap();
            assert!(settle_dispatch_fence().is_err());
            let error = scope(scratch.0.clone(), THREAD.to_owned(), async {
                arm_for_dispatch().unwrap_err()
            })
            .await
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::WouldBlock);

            std::fs::write(&marker, valid).unwrap();
            settle_dispatch_fence().unwrap();
        })
        .await
        .unwrap();
        assert_eq!(read_active(&scratch.0, THREAD).unwrap(), None);
    }

    #[tokio::test]
    async fn definite_rollback_repairs_a_partial_arm_append() {
        let scratch = Scratch::new();
        let predecessor = arm(&scratch.0, THREAD).unwrap();
        scope(scratch.0.clone(), THREAD.to_owned(), async {
            arm_for_dispatch().unwrap();
            let marker = marker_path(&scratch.0, THREAD).unwrap();
            let file = OpenOptions::new().write(true).open(marker).unwrap();
            let partial_len = file.metadata().unwrap().len() - 9;
            file.set_len(partial_len).unwrap();
            file.sync_all().unwrap();
            restore_dispatch_fence().unwrap();
        })
        .await
        .unwrap();
        assert_eq!(read_active(&scratch.0, THREAD).unwrap(), Some(predecessor));
        assert_eq!(read_current(&scratch.0, THREAD).unwrap(), Some(predecessor));
    }

    #[tokio::test]
    async fn exact_settlement_retires_the_whole_older_stack() {
        let scratch = Scratch::new();
        let _older = arm(&scratch.0, THREAD).unwrap();
        let mut settled = None;
        scope(scratch.0.clone(), THREAD.to_owned(), async {
            arm_for_dispatch().unwrap();
            settled = read_active(&scratch.0, THREAD).unwrap();
            settle_dispatch_fence().unwrap();
        })
        .await
        .unwrap();
        assert_eq!(read_active(&scratch.0, THREAD).unwrap(), None);
        assert_eq!(read_current(&scratch.0, THREAD).unwrap(), settled);
    }

    #[test]
    fn inactive_compaction_remains_readable_and_accepts_the_next_arm() {
        let scratch = Scratch::new();
        let marker = marker_path(&scratch.0, THREAD).unwrap();
        let mut compacted = false;
        let mut grew = false;
        let mut latest = None;
        let canonical_len = JOURNAL_HEADER.len() as u64 + (4 + 64 + 1) + (7 + 64 + 1);
        for _ in 0..1_000 {
            let generation = arm(&scratch.0, THREAD).unwrap();
            assert!(clear_if_current(&scratch.0, THREAD, generation).unwrap());
            latest = Some(generation);
            let length = std::fs::metadata(&marker).unwrap().len();
            grew |= length > canonical_len;
            if grew && length == canonical_len {
                compacted = true;
                break;
            }
        }
        assert!(compacted, "journal never reached its compaction threshold");
        assert_eq!(read_active(&scratch.0, THREAD).unwrap(), None);
        assert_eq!(read_current(&scratch.0, THREAD).unwrap(), latest);
        let generation = arm(&scratch.0, THREAD).unwrap();
        assert_eq!(read_active(&scratch.0, THREAD).unwrap(), Some(generation));
        assert_eq!(read_current(&scratch.0, THREAD).unwrap(), Some(generation));
    }
}
