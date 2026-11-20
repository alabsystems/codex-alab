// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Raw unified shell output. Local durable threads retain the bytes a tool discloses.

use super::process::retired_readers::ReaderPool;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::fs;
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;

use crate::tools::sandboxing::ToolCtx;
use codex_alab_core::MAX_SHELL_OUTPUT_TAIL_BYTES;
use codex_alab_core::ShellOutputTail;
use codex_protocol::ThreadId;
use codex_thread_store::LocalThreadStore;
use once_cell::sync::OnceCell;
use tempfile::NamedTempFile;
use tempfile::TempDir;

const MAX_LOG_BYTES: usize = codex_alab_core::SHELL_OUTPUT_PREFIX_BYTES;
#[cfg(unix)]
use codex_alab_core::shell_log_store::MAX_THREAD_LOGS;
#[cfg(not(unix))]
const MAX_THREAD_LOGS: usize = 32;
const MEMORY_PREFIX_BYTES: usize = 8 * 1024;

#[derive(Default)]
pub(crate) struct ThreadLogs(OnceCell<Arc<LogDirectory>>);

#[derive(Debug)]
struct LogDirectory {
    directory: TempDir,
    owner: Option<(PathBuf, ThreadId)>,
    slots: Mutex<Slots>,
}

#[derive(Debug, Default)]
struct Slots {
    occupied: usize,
    live: HashMap<PathBuf, fs::Metadata>,
}

impl LogDirectory {
    fn create_log(&self) -> io::Result<Option<NamedTempFile>> {
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slots.occupied >= MAX_THREAD_LOGS || self.owner.is_some() {
            // Disclosed files may have been removed. Count every remaining
            // directory entry, including renamed logs, and every open capture
            // whose name disappeared. Never adopt a path found by the scan.
            let mut occupied = slots.live.len();
            if occupied >= MAX_THREAD_LOGS {
                return Ok(None);
            }
            for entry in self.directory.path().read_dir()? {
                let entry = entry?;
                if let Some(open) = slots.live.get(&entry.path())
                    && same_open_file(open, &entry.metadata()?)
                {
                    continue;
                }
                occupied += 1;
                if occupied >= MAX_THREAD_LOGS {
                    return Ok(None);
                }
            }
            #[cfg(unix)]
            if let Some((home, thread)) = &self.owner
                && let Some(dir) = crate::alab_shell_log_store::existing_directory(home, *thread)?
            {
                let count = dir.count(MAX_THREAD_LOGS)?;
                if count == MAX_THREAD_LOGS {
                    return Ok(None);
                }
                occupied += count;
                for (path, open) in &slots.live {
                    if path.parent() == Some(dir.path())
                        && let Some(name) = path.file_name()
                        && dir.names_metadata(name, open)?
                    {
                        occupied -= 1;
                    }
                }
                if occupied >= MAX_THREAD_LOGS {
                    return Ok(None);
                }
            }
            slots.occupied = occupied;
        }
        let mut file = new_log_file(&self.directory)?;
        // A pathname can be renamed or replaced by another command. Only
        // finish() may unlink it, after checking it still names our file.
        file.disable_cleanup(true);
        if slots.live.contains_key(file.path()) {
            slots.occupied = MAX_THREAD_LOGS;
            unlink_open_file(file);
            return Ok(None);
        }
        let metadata = match file.as_file().metadata() {
            Ok(metadata) => metadata,
            Err(err) => {
                slots.occupied = MAX_THREAD_LOGS;
                unlink_open_file(file);
                return Err(err);
            }
        };
        slots.live.insert(file.path().to_owned(), metadata);
        slots.occupied += 1;
        Ok(Some(file))
    }

    #[cfg(unix)]
    fn publish(
        &self,
        capture: &mut LogFile,
        bytes: usize,
        prepared: Option<&(String, String)>,
    ) -> io::Result<()> {
        let (Some((home, thread)), LogFile::Temporary(staging)) = (&self.owner, &*capture) else {
            return Ok(());
        };
        let directory = crate::alab_shell_log_store::directory(home, *thread)?;
        let (name, file) = if let Some((name, hash)) = prepared {
            let file = directory
                .claim_for_tool(name, bytes as u64, hash)?
                .ok_or_else(|| {
                    io::Error::other("watcher shell output changed before disclosure")
                })?;
            (std::ffi::OsString::from(name), file)
        } else {
            if !matches_open_path(staging, &staging.as_file().metadata()?) {
                return Err(io::Error::other("staging shell log path changed"));
            }
            directory.create_capture(|target| copy_prefix(staging, target, bytes))?
        };
        let path = directory.path().join(&name);
        let metadata = match file.metadata() {
            Ok(metadata) => metadata,
            Err(err) => {
                if prepared.is_none() {
                    directory.remove(&name, &file)?;
                }
                return Err(err);
            }
        };
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        slots.live.remove(staging.path());
        slots.live.insert(path.clone(), metadata);
        let retained = LogFile::Retained {
            directory,
            path,
            file,
        };
        if let LogFile::Temporary(staging) = std::mem::replace(capture, retained) {
            unlink_open_file(staging);
        }
        Ok(())
    }

    fn finish(&self, file: NamedTempFile, published: bool) {
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        slots.live.remove(file.path());
        if published {
            return;
        }
        let removed = unlink_open_file(file);
        // At capacity the next allocation recounts. Extra directory entries
        // may have made the previous scan reach its bound before enumeration ended.
        if !removed {
            slots.occupied = MAX_THREAD_LOGS;
        } else if slots.occupied < MAX_THREAD_LOGS {
            slots.occupied -= 1;
        }
    }
}

#[cfg(unix)]
fn copy_prefix(staging: &NamedTempFile, target: &mut fs::File, bytes: usize) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    // Positional reads preserve the original writer's offset when publication fails.
    let mut buffer = [0_u8; 8192];
    let mut offset = 0;
    while offset < bytes {
        let len = (bytes - offset).min(buffer.len());
        staging
            .as_file()
            .read_exact_at(&mut buffer[..len], offset as u64)?;
        target.write_all(&buffer[..len])?;
        offset += len;
    }
    Ok(())
}

fn matches_open_path(file: &NamedTempFile, open: &fs::Metadata) -> bool {
    fs::symlink_metadata(file.path()).is_ok_and(|named| same_open_file(open, &named))
}

fn unlink_open_file(file: NamedTempFile) -> bool {
    if !file
        .as_file()
        .metadata()
        .is_ok_and(|open| matches_open_path(&file, &open))
    {
        return false;
    }
    let (open, path) = file.into_parts();
    // Windows pins the name until the open handle closes.
    #[cfg(windows)]
    drop(open);
    let removed = path.close().is_ok();
    #[cfg(not(windows))]
    drop(open);
    removed
}

#[cfg(not(windows))]
fn new_log_file(directory: &TempDir) -> io::Result<NamedTempFile> {
    NamedTempFile::new_in(directory.path())
}

#[cfg(windows)]
fn new_log_file(directory: &TempDir) -> io::Result<NamedTempFile> {
    use std::os::windows::fs::OpenOptionsExt;

    // As with Codex's bound final-output file on Windows, other handles may
    // read or write, but the name cannot be replaced until we close ours.
    // This works on volumes that lack unique FILE_ID_INFO identifiers.
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const FILE_ATTRIBUTE_TEMPORARY: u32 = 0x100;
    tempfile::Builder::new().make_in(directory.path(), |path| {
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .attributes(FILE_ATTRIBUTE_TEMPORARY)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(path)
    })
}

// Reclaim capacity only when directory inspection can distinguish the
// original open file from a same-name replacement.
#[cfg(unix)]
fn same_open_file(open: &fs::Metadata, named: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    named.is_file() && (open.dev(), open.ino()) == (named.dev(), named.ino())
}

#[cfg(windows)]
fn same_open_file(_open: &fs::Metadata, named: &fs::Metadata) -> bool {
    // new_log_file holds a no-delete-share handle to this name.
    named.is_file()
}

#[cfg(not(any(unix, windows)))]
fn same_open_file(_open: &fs::Metadata, _named: &fs::Metadata) -> bool {
    // Portable Metadata exposes no stable file identifier here. Charge both.
    false
}

impl ThreadLogs {
    fn directory_with(
        &self,
        owner: Option<(PathBuf, ThreadId)>,
        create: impl FnOnce() -> io::Result<TempDir>,
    ) -> io::Result<Arc<LogDirectory>> {
        self.0
            .get_or_try_init(|| {
                Ok(Arc::new(LogDirectory {
                    directory: create()?,
                    owner,
                    slots: Mutex::default(),
                }))
            })
            .cloned()
    }

    pub(crate) fn start(&self, ctx: &ToolCtx, remote: bool) -> Option<ShellLog> {
        if !codex_alab_core::active() {
            return None;
        }
        // A host temp path would be unusable in a remote command's environment.
        let directory = if remote {
            None
        } else {
            let owner = (cfg!(unix)
                && ctx.session.live_thread().is_some()
                && ctx
                    .session
                    .services
                    .thread_store
                    .as_any()
                    .is::<LocalThreadStore>())
            .then(|| {
                (
                    ctx.step_context.turn.config.codex_home.to_path_buf(),
                    ctx.session.thread_id,
                )
            });
            self.directory_with(owner, || {
                let mut builder = tempfile::Builder::new();
                builder.prefix("codex-shell-");
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    builder.permissions(fs::Permissions::from_mode(0o700));
                }
                builder.tempdir()
            })
            .map_err(|err| tracing::warn!("cannot create shell output directory: {err}"))
            .ok()
        };
        Some(ShellLog::new(directory))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ShellLog {
    capture: Arc<Mutex<Capture>>,
    // Only the handle on the returned tool result freezes its disclosure.
    result_notice: Option<Option<LogNotice>>,
    result_can_disclose_tail: bool,
    launch_generation: Option<Weak<ReaderPool>>,
    first_exit_uncollected: bool,
    invocation: uuid::Uuid,
    drained: tokio_util::sync::CancellationToken,
    pub(super) diagnostic_writer: Arc<tokio::sync::Semaphore>,
}

#[cfg(unix)]
pub(super) struct PreparedDiagnostic {
    pub(super) name: String,
    pub(super) expected_bytes: u64,
    pub(super) prefix_sha256: String,
    pub(super) partial: bool,
    pub(super) tail: Option<ShellOutputTail>,
    lock: codex_alab_core::shell_log_store::PublicationLock,
}

#[cfg(unix)]
impl PreparedDiagnostic {
    pub(super) fn publish(&self) -> io::Result<bool> {
        self.lock
            .publish(&self.name, self.expected_bytes, &self.prefix_sha256)
    }

    pub(super) fn cleanup(&self, attached: bool) -> io::Result<()> {
        self.lock.cleanup(&self.name, attached)
    }
}

impl PartialEq for ShellLog {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.capture, &other.capture)
    }
}

#[derive(Debug)]
enum LogFile {
    Temporary(NamedTempFile),
    #[cfg(unix)]
    Retained {
        directory: crate::alab_shell_log_store::LogDirectory,
        path: PathBuf,
        file: fs::File,
    },
}

impl LogFile {
    fn writer(&mut self) -> &mut fs::File {
        match self {
            Self::Temporary(file) => file.as_file_mut(),
            #[cfg(unix)]
            Self::Retained { file, .. } => file,
        }
    }

    fn visible_path(&self) -> Option<PathBuf> {
        match self {
            Self::Temporary(file) => {
                let named = fs::symlink_metadata(file.path()).ok()?;
                if !named.is_file() {
                    return None;
                }
                #[cfg(any(unix, windows))]
                if !same_open_file(&file.as_file().metadata().ok()?, &named) {
                    return None;
                }
                Some(file.path().to_owned())
            }
            #[cfg(unix)]
            Self::Retained {
                directory,
                path,
                file,
            } => directory
                .names_file(path.file_name()?, file)
                .ok()?
                .then(|| path.clone()),
        }
    }
}

#[derive(Debug)]
struct Capture {
    exit_watch: super::shell_observation::ExitWatch,
    directory: Option<Arc<LogDirectory>>,
    // Keep brief output off disk until a clipped preview needs a path.
    prefix: Vec<u8>,
    file: Option<LogFile>,
    bytes: usize,
    observed_bytes: usize,
    suffix: Option<OutputSuffix>,
    raw_eof: bool,
    native_readers: Option<codex_utils_pty::NativeOutputEof>,
    collected_output_bytes: Option<usize>,
    final_poll: bool,
    final_disclosure: Option<DisclosedOutput>,
    reader_pool: Option<std::sync::Weak<super::process::retired_readers::ReaderPool>>,
    diagnostic_committed: bool,
    #[cfg(unix)]
    native_prefix: Option<codex_alab_core::shell_log_store::PrefixDigest>,
    // Retry the same inode and digest after an ambiguous SQLite receipt.
    #[cfg(unix)]
    prepared_diagnostic: Option<(String, String)>,
    published: bool,
    incomplete: bool,
    closed: bool,
    // A replay omitted bytes, or a clipped preview must retry its buffered log.
    needs_notice: bool,
}

#[derive(Debug)]
struct OutputSuffix {
    bytes: VecDeque<u8>,
    end: Option<u64>,
}

impl OutputSuffix {
    fn new() -> Self {
        Self {
            bytes: VecDeque::with_capacity(MAX_SHELL_OUTPUT_TAIL_BYTES),
            end: Some(MAX_LOG_BYTES as u64),
        }
    }

    fn append(&mut self, chunk: &[u8]) {
        self.end = self.end.and_then(|end| end.checked_add(chunk.len() as u64));
        if chunk.len() >= MAX_SHELL_OUTPUT_TAIL_BYTES {
            self.bytes.clear();
            self.bytes
                .extend(&chunk[chunk.len() - MAX_SHELL_OUTPUT_TAIL_BYTES..]);
        } else {
            let expired =
                (self.bytes.len() + chunk.len()).saturating_sub(MAX_SHELL_OUTPUT_TAIL_BYTES);
            self.bytes.drain(..expired);
            self.bytes.extend(chunk);
        }
    }
}

impl Capture {
    fn raw_failure_tail_available(&self) -> bool {
        self.raw_eof
            && !self
                .native_readers
                .as_ref()
                .is_some_and(codex_utils_pty::NativeOutputEof::forced_before_exit)
    }

    fn has_uncollected_output(&self) -> bool {
        if let Some(disclosed) = self.final_disclosure {
            return self.bytes > disclosed.bytes
                || (disclosed.partial && self.closed && !self.incomplete)
                || (self.raw_failure_tail_available()
                    && self.suffix.as_ref().and_then(|suffix| suffix.end) != disclosed.tail_end);
        }
        self.collected_output_bytes
            .is_some_and(|collected| self.observed_bytes > collected)
    }

    fn append_suffix(&mut self, chunk: &[u8], previous: usize) {
        // A short write or lost in-memory prefix never establishes the offset.
        if self.bytes != MAX_LOG_BYTES
            || !self
                .directory
                .as_ref()
                .is_some_and(|dir| dir.owner.is_some())
        {
            return;
        }
        let skip = MAX_LOG_BYTES.saturating_sub(previous).min(chunk.len());
        let beyond_prefix = &chunk[skip..];
        if !beyond_prefix.is_empty() {
            self.suffix
                .get_or_insert_with(OutputSuffix::new)
                .append(beyond_prefix);
        }
    }

    #[cfg(unix)]
    fn durable_tail(&self) -> Option<ShellOutputTail> {
        if !self.raw_failure_tail_available() {
            return None;
        }
        let LogFile::Retained {
            path,
            directory,
            file,
        } = self.file.as_ref()?
        else {
            return None;
        };
        if !directory.names_file(path.file_name()?, file).ok()?
            || file.metadata().ok()?.len() != MAX_LOG_BYTES as u64
        {
            return None;
        }
        self.verified_prefix_tail()
    }

    #[cfg(unix)]
    fn verified_prefix_tail(&self) -> Option<ShellOutputTail> {
        if !self.raw_failure_tail_available() || self.bytes != MAX_LOG_BYTES {
            return None;
        }
        let suffix = self.suffix.as_ref()?;
        let start = suffix.end?.checked_sub(suffix.bytes.len() as u64)?;
        ShellOutputTail::from_bytes(start, suffix.bytes.iter().copied().collect())
    }

    #[cfg(not(unix))]
    fn durable_tail(&self) -> Option<ShellOutputTail> {
        None
    }

    fn has_pending_diagnostic(&self) -> bool {
        !self.diagnostic_committed
            && (self.has_uncollected_output()
                || (!self.final_poll && self.published && self.incomplete))
    }

    fn try_spill_prefix(&mut self) -> bool {
        let Some(file) = self.directory.as_ref().and_then(|directory| {
            directory.create_log().unwrap_or_else(|err| {
                tracing::warn!("cannot create shell output log: {err}");
                None
            })
        }) else {
            return false;
        };
        self.file = Some(LogFile::Temporary(file));
        let prefix = std::mem::take(&mut self.prefix);
        self.write_bytes(&prefix);
        true
    }

    #[cfg(unix)]
    fn publish_file(&mut self) -> io::Result<()> {
        if !self.published
            && let Some(directory) = &self.directory
            && let Some(file) = self.file.as_mut()
        {
            directory.publish(file, self.bytes, self.prepared_diagnostic.as_ref())?;
        }
        Ok(())
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        let Some(file) = self.file.as_mut() else {
            self.incomplete = true;
            return;
        };
        #[cfg(unix)]
        let before = self.bytes;
        write_captured_bytes(file.writer(), bytes, &mut self.bytes, &mut self.incomplete);
        #[cfg(unix)]
        if let Some(digest) = &mut self.native_prefix {
            digest.append(&bytes[..self.bytes - before]);
        }
    }
}

fn write_captured_bytes(
    writer: &mut impl Write,
    bytes: &[u8],
    accepted: &mut usize,
    incomplete: &mut bool,
) {
    let count = bytes.len().min(MAX_LOG_BYTES - *accepted);
    *incomplete |= count < bytes.len();
    let mut remaining = &bytes[..count];
    while !remaining.is_empty() {
        let err = match writer.write(remaining) {
            Ok(0) => io::Error::from(io::ErrorKind::WriteZero),
            Ok(written) => {
                // A later write may fail. Publication must still copy this prefix.
                *accepted += written;
                remaining = &remaining[written..];
                continue;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => err,
        };
        *incomplete = true;
        tracing::warn!("cannot write shell output log: {err}");
        break;
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(file) = self.file.take()
            && let Some(directory) = &self.directory
        {
            match file {
                LogFile::Temporary(file) => directory.finish(file, self.published),
                #[cfg(unix)]
                LogFile::Retained {
                    directory: owner,
                    path,
                    file,
                } => {
                    let mut slots = directory
                        .slots
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    slots.live.remove(&path);
                    if !self.published
                        && let Some(name) = path.file_name()
                    {
                        let _ = owner.remove(name, &file);
                    }
                }
            }
        }
    }
}

impl ShellLog {
    fn new(directory: Option<Arc<LogDirectory>>) -> Self {
        Self {
            capture: Arc::new(Mutex::new(Capture {
                exit_watch: Default::default(),
                incomplete: directory.is_none(),
                #[cfg(unix)]
                native_prefix: directory
                    .as_ref()
                    .is_some_and(|d| d.owner.is_some())
                    .then(Default::default),
                directory,
                prefix: Vec::new(),
                file: None,
                bytes: 0,
                observed_bytes: 0,
                suffix: None,
                raw_eof: false,
                native_readers: None,
                collected_output_bytes: None,
                final_poll: false,
                final_disclosure: None,
                reader_pool: None,
                diagnostic_committed: false,
                #[cfg(unix)]
                prepared_diagnostic: None,
                published: false,
                closed: false,
                needs_notice: false,
            })),
            result_notice: None,
            result_can_disclose_tail: false,
            launch_generation: None,
            first_exit_uncollected: false,
            invocation: uuid::Uuid::new_v4(),
            drained: tokio_util::sync::CancellationToken::new(),
            diagnostic_writer: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    pub(crate) fn invocation_id(&self) -> uuid::Uuid {
        self.invocation
    }

    pub(super) fn with_exit_watch<T>(
        &self,
        update: impl FnOnce(&mut super::shell_observation::ExitWatch) -> T,
    ) -> T {
        let mut capture = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        update(&mut capture.exit_watch)
    }

    pub(crate) fn retained_basename(&self) -> Option<String> {
        #[cfg(unix)]
        {
            let capture = self
                .capture
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if capture.published
                && let Some(LogFile::Retained { path, .. }) = &capture.file
            {
                return Some(path.file_name()?.to_str()?.to_owned());
            }
        }
        None
    }

    pub(crate) fn append(&self, bytes: &[u8]) {
        let mut capture = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if capture.closed {
            return;
        }
        let previous = capture.observed_bytes;
        capture.observed_bytes = previous.saturating_add(bytes.len());
        if bytes.is_empty() {
            return;
        }
        if capture.incomplete {
            capture.append_suffix(bytes, previous);
            return;
        }
        if capture.file.is_none() {
            let room = MEMORY_PREFIX_BYTES - capture.prefix.len();
            if bytes.len() <= room {
                capture.prefix.extend_from_slice(bytes);
                return;
            }
            if !capture.try_spill_prefix() {
                // Keep a contiguous prefix. Once a byte is lost, later chunks
                // cannot extend it and only a tool response retries the file.
                capture.prefix.extend_from_slice(&bytes[..room]);
                capture.incomplete = true;
                return;
            }
            if capture.incomplete {
                return;
            }
        }
        capture.write_bytes(bytes);
        capture.append_suffix(bytes, previous);
    }

    pub(crate) fn close_raw(&self, readers: codex_utils_pty::NativeOutputEof) {
        let native_eof = readers.observed();
        self.close_capture(
            native_eof.unwrap_or(true),
            native_eof == Some(true),
            Some(readers),
        );
    }

    pub(crate) fn close(&self, complete: bool) {
        self.close_capture(complete, false, None);
    }

    fn close_capture(
        &self,
        complete: bool,
        native_eof: bool,
        readers: Option<codex_utils_pty::NativeOutputEof>,
    ) {
        let mut capture = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if capture.closed {
            return;
        }
        capture.closed = true;
        capture.raw_eof = native_eof;
        capture.native_readers = readers;
        capture.incomplete |= !complete;
        drop(capture);
        self.drained.cancel();
        super::shell_observation::output_drained(self);
    }

    pub(super) fn durable_owner(&self) -> bool {
        self.capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .directory
            .as_ref()
            .is_some_and(|directory| directory.owner.is_some())
    }

    pub(super) fn bind_launch_generation(&mut self, readers: &Arc<ReaderPool>) {
        self.launch_generation = Some(Arc::downgrade(readers));
    }

    pub(super) fn launch_pool(&self) -> Option<Arc<ReaderPool>> {
        self.launch_generation.as_ref()?.upgrade()
    }

    pub(super) fn raw_closed(&self) -> bool {
        self.drained.is_cancelled()
    }

    pub(super) fn launched_in(&self, current: &Arc<ReaderPool>) -> bool {
        self.launch_generation
            .as_ref()
            .is_some_and(|original| original.ptr_eq(&Arc::downgrade(current)))
    }

    pub(super) fn first_failure_result(&self) -> bool {
        self.result_can_disclose_tail && self.launch_generation.is_some()
    }

    /// Freeze exactly the first collection; a terminal launch had no previous
    /// running result to seed the cumulative watermark.
    pub(super) fn collect_first_exit(
        &self,
        bytes: usize,
        disclosed: Option<DisclosedOutput>,
        code: i32,
    ) {
        let mut capture = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        capture.collected_output_bytes = Some(bytes);
        capture.final_poll = true;
        capture.final_disclosure = disclosed;
        capture.exit_watch.confirm_first_exit(code);
    }

    pub(super) fn remember_first_admission(&mut self) {
        let capture = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.first_exit_uncollected =
            capture.reader_pool.is_some() && (!capture.closed || capture.has_pending_diagnostic());
    }

    /// A closed launch generation cannot admit a reader. Its original bounded
    /// capture may already own a newer prefix or a proven coverage correction.
    pub(super) fn remember_sealed_first_exit(&mut self) {
        let capture = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.first_exit_uncollected = capture.closed
            && capture.has_pending_diagnostic()
            && (capture.bytes != 0 || !capture.prefix.is_empty());
    }

    pub(super) fn first_exit_uncollected(&self) -> bool {
        self.first_exit_uncollected
    }

    pub(super) fn initial_response(&self, bytes: usize) {
        let mut capture = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        capture.collected_output_bytes.get_or_insert(bytes);
    }

    pub(super) fn retain_reader(
        &self,
        pool: std::sync::Weak<super::process::retired_readers::ReaderPool>,
    ) -> bool {
        let mut capture = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if capture.collected_output_bytes.is_none()
            || !capture
                .directory
                .as_ref()
                .is_some_and(|dir| dir.owner.is_some())
            || (capture.closed && !capture.has_pending_diagnostic())
        {
            return false;
        }
        capture.reader_pool = Some(pool);
        true
    }

    /// Each successful native poll returns the next bytes drained from its
    /// original buffer. Advance by the raw count, including omitted middle bytes.
    pub(super) fn collect_poll(
        &self,
        count: usize,
        failed_terminal: bool,
        disclosed: Option<DisclosedOutput>,
    ) -> bool {
        let mut capture = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(previous) = capture.collected_output_bytes else {
            return false;
        };
        let collected = previous.saturating_add(count);
        capture.collected_output_bytes = Some(collected);
        if failed_terminal {
            capture.final_poll = true;
            capture.final_disclosure = disclosed;
        }
        failed_terminal
            && capture.reader_pool.is_some()
            && (!capture.closed || capture.has_uncollected_output())
    }

    pub(super) fn release_drained_reader(&self, rejected: bool) {
        let pool = {
            let mut capture = self
                .capture
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !capture.closed || (!rejected && capture.has_pending_diagnostic()) {
                return;
            }
            capture.reader_pool.take().and_then(|pool| pool.upgrade())
        };
        if let Some(pool) = pool {
            pool.release(self.invocation_id());
        }
    }

    pub(super) async fn finish_observed_failure(&self) {
        let eligible = {
            let capture = self
                .capture
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            capture.collected_output_bytes.is_some()
                && capture
                    .directory
                    .as_ref()
                    .is_some_and(|dir| dir.owner.is_some())
        };
        if eligible
            && tokio::time::timeout(
                super::async_watcher::TRAILING_OUTPUT_GRACE,
                self.drained.cancelled(),
            )
            .await
            .is_err()
        {
            // A descendant may keep the pipe open through shutdown. Freeze the
            // actual prefix before aborting its reader and describe it as partial.
            self.close(false);
        }
    }

    pub(super) fn diagnostic_ready(&self) -> bool {
        let capture = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        capture.closed
            && capture
                .directory
                .as_ref()
                .is_some_and(|dir| dir.owner.is_some())
            && capture.collected_output_bytes.is_some()
            && capture.has_pending_diagnostic()
    }

    #[cfg(unix)]
    pub(super) fn prepare_diagnostic(&self) -> io::Result<Option<PreparedDiagnostic>> {
        if !self.diagnostic_ready() {
            return Ok(None);
        }
        let mut capture = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if capture.file.is_none() && !capture.try_spill_prefix() {
            return Err(io::Error::other("shell diagnostic staging is unavailable"));
        }
        if capture.bytes == 0 {
            return Ok(None);
        }
        let Some((home, thread)) = capture.directory.as_ref().and_then(|d| d.owner.as_ref()) else {
            return Ok(None);
        };
        let lock = crate::alab_shell_log_store::directory(home, *thread)?
            .try_publication_lock()?
            .ok_or_else(|| io::Error::from(io::ErrorKind::WouldBlock))?;
        let expected_bytes = capture.bytes as u64;
        let Some(prefix_sha256) = capture
            .native_prefix
            .as_ref()
            .map(codex_alab_core::shell_log_store::PrefixDigest::hex)
        else {
            return Ok(None);
        };
        let name = if let Some((name, hash)) = &capture.prepared_diagnostic
            && *hash == prefix_sha256
            && lock.contains_prefix(name, expected_bytes, hash)?
        {
            name.clone()
        } else {
            let name = match &capture.file {
                Some(LogFile::Temporary(staging))
                    if matches_open_path(staging, &staging.as_file().metadata()?) =>
                {
                    let (name, file_hash) = lock.prepare_copy(expected_bytes, |target| {
                        copy_prefix(staging, target, capture.bytes)
                    })?;
                    if file_hash != prefix_sha256 {
                        lock.cleanup(&name, false)?;
                        return Ok(None);
                    }
                    name
                }
                Some(LogFile::Retained { path, .. }) => {
                    // The tool already disclosed this inode-bound name. Reserve its native
                    // digest even after a replacement so definitive rejection can also
                    // withdraw unsafe coverage from the prior SQLite observation.
                    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                        return Ok(None);
                    };
                    name.to_owned()
                }
                _ => return Ok(None),
            };
            capture.prepared_diagnostic = Some((name.clone(), prefix_sha256.clone()));
            name
        };
        let tail = capture
            .exit_watch
            .confirmed_failure()
            .then(|| capture.verified_prefix_tail())
            .flatten();
        Ok(Some(PreparedDiagnostic {
            name,
            expected_bytes,
            prefix_sha256,
            partial: capture.incomplete,
            tail,
            lock,
        }))
    }

    pub(super) fn finish_diagnostic(&self, attached: bool) {
        let mut capture = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        capture.diagnostic_committed |= attached;
        #[cfg(unix)]
        {
            if attached {
                if let Err(err) = capture.publish_file() {
                    // Keep the committed name for a later poll. Never allocate another
                    // path for an already recorded observation.
                    tracing::warn!("could not adopt committed shell diagnostic: {err}");
                    return;
                }
                capture.published |= matches!(capture.file, Some(LogFile::Retained { .. }));
            }
            capture.prepared_diagnostic = None;
        }
    }

    pub(crate) fn mark_replay_clipped(&self) {
        self.capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .needs_notice = true;
    }

    pub(super) fn allow_tool_tail(
        &mut self,
        process: &super::UnifiedExecProcess,
        session: Option<i32>,
        code: Option<i32>,
    ) {
        self.result_can_disclose_tail = session.is_none()
            && code.is_some_and(|code| code != 0 && code != -1)
            && process.has_exited()
            && process.exit_code() == code
            && !process.timed_out()
            && !process
                .termination_requested
                .load(std::sync::atomic::Ordering::Acquire);
    }

    pub(crate) fn frozen_tail(&self) -> Option<&ShellOutputTail> {
        self.result_notice.as_ref()?.as_ref()?.tail.as_ref()
    }

    pub(crate) fn undelivered_tail(&self) -> Option<&ShellOutputTail> {
        self.result_notice
            .as_ref()?
            .as_ref()?
            .withheld_tail
            .as_ref()
    }

    pub(super) async fn wait_for_local_drain(&self) {
        self.drained.cancelled().await;
    }

    pub(crate) fn freeze_result_notice(&mut self, notice: Option<LogNotice>) {
        self.result_notice = Some(notice);
    }

    pub(crate) fn notice(&self, clipped: bool, running: bool) -> Option<LogNotice> {
        if let Some(notice) = &self.result_notice {
            let notice = notice.as_ref()?;
            if notice.path.is_some() {
                let capture = self
                    .capture
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if capture.file.as_ref().and_then(LogFile::visible_path) != notice.path
                    || (notice.tail.is_some()
                        && capture.durable_tail().as_ref() != notice.tail.as_ref())
                {
                    return Some(LogNotice::unavailable());
                }
            }
            return Some(notice.clone());
        }
        let mut capture = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !clipped
            && !capture.needs_notice
            && !capture.published
            && !(self.result_can_disclose_tail && capture.raw_eof && capture.suffix.is_some())
        {
            return None;
        }
        if !capture.prefix.is_empty() && !capture.try_spill_prefix() {
            capture.needs_notice = true;
        }
        #[cfg(unix)]
        if let Err(err) = capture.publish_file() {
            tracing::warn!("cannot retain shell output log: {err}");
            capture.needs_notice = true;
            return Some(LogNotice::unavailable());
        }
        let path = capture.file.as_ref().and_then(LogFile::visible_path);
        capture.needs_notice |= path.is_none();
        let status = if path.is_none() {
            Some("unavailable")
        } else if capture.incomplete || (!running && !capture.closed) {
            Some("partial")
        } else if !capture.closed {
            Some("in_progress")
        } else {
            None
        };
        capture.published |= path.is_some();
        let tail = (!running && self.result_can_disclose_tail && path.is_some())
            .then(|| capture.durable_tail())
            .flatten();
        let disclosed = path.as_ref().map(|path| DisclosedOutput {
            exact_path: path.to_str().is_some(),
            bytes: capture.bytes,
            partial: status == Some("partial"),
            tail_end: tail.as_ref().map(ShellOutputTail::end_byte),
        });
        if !running && capture.reader_pool.is_some() {
            // EOF can run before the native tool records its result. Give the
            // original reader this exact disclosure while it still owns the lock.
            capture.final_disclosure = disclosed;
        }
        Some(LogNotice {
            withheld_tail: None,
            tail,
            disclosed,
            path,
            status,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DisclosedOutput {
    /// The tool response can name this exact path without lossy conversion.
    pub(super) exact_path: bool,
    pub(super) bytes: usize,
    partial: bool,
    tail_end: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct LogNotice {
    withheld_tail: Option<ShellOutputTail>,
    pub(crate) tail: Option<ShellOutputTail>,
    pub(crate) disclosed: Option<DisclosedOutput>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) status: Option<&'static str>,
}

impl LogNotice {
    fn unavailable() -> Self {
        Self {
            withheld_tail: None,
            tail: None,
            path: None,
            status: Some("unavailable"),
            disclosed: None,
        }
    }

    pub(crate) fn withhold_tail(&mut self) {
        self.withheld_tail = self.tail.take();
        if let Some(disclosed) = self.disclosed.as_mut() {
            disclosed.tail_end = None;
        }
    }

    pub(crate) fn text(&self) -> String {
        let mut text = match (&self.path, self.status) {
            (Some(path), Some("partial")) => {
                format!("Partial shell output saved to: {}", path.display())
            }
            (Some(path), Some("in_progress")) => {
                format!("Shell output is being saved to: {}", path.display())
            }
            (Some(path), _) => format!("Full shell output saved to: {}", path.display()),
            (None, _) => "Shell output log unavailable".to_string(),
        };
        if let Some(tail) = &self.tail {
            text.push_str("\nShell output tail: ");
            text.push_str(&tail.json_text());
        }
        text
    }
}

impl<const MAX_BYTES: usize> super::process::OutputBuffers<MAX_BYTES> {
    pub(super) fn denied_shell_log(&self) -> Option<ShellLog> {
        let log = self.shell_log.as_ref()?;
        // The early denial preview snapshots pending output before a collection
        // result can report its omitted byte count.
        if self.pending.omitted_bytes() > 0 {
            log.mark_replay_clipped();
        }
        Some(log.clone())
    }
}

#[cfg(test)]
#[path = "shell_output_tests.rs"]
mod tests;
