// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual close/unlink owners across SAME-PID granted exec. Locator bytes are
//! only correlation; only genuine E1 +kernel-created rights admit an owner.
use super::inherited_sockets;
use super::peer_pid;
use crate::frontend_control::EnteredRetainedFrontend;
use crate::frontend_control::retained_service::FrontendCommandExecScope;
use crate::frontend_control::retained_service::PreparedFrontendSocket;
pub use crate::upgrade_host::retained::RetainedHost;
use serde::Deserialize;
use serde::Serialize;
use std::fs::File;
use std::fs::Metadata;
use std::io::Read;
use std::io::Write;
use std::io::{self};
use std::os::fd::AsFd;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Instant;
use tempfile::NamedTempFile;
use tempfile::TempPath;

fn invalid(why: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.to_string())
}

const MAGIC: &[u8; 16] = b"ALAB-IMAGE-FD1\n\0";
const UI_MAGIC: &[u8; 16] = b"ALAB-RETAIN-UI2\n";
const MAX_FILES: usize = 1024;
const MAX_CHANNELS: usize = 16;
const MAX_PATH: usize = 64 * 1024;
const SUPPORT_KEY: &str = "alab.retainedImageCustody";
static STARTED: AtomicBool = AtomicBool::new(false);
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Support {
    e1_version: u32,
    image_custody: u32,
    clipboard_custody: u32,
    pre_thread_pin: u32,
}
impl Support {
    fn validate(&self) -> io::Result<()> {
        if self.e1_version != 2
            || self.image_custody != 1
            || self.clipboard_custody != 1
            || self.pre_thread_pin != 1
        {
            return Err(invalid(
                "target cannot adopt the required image/E1 custody schema",
            ));
        }
        Ok(())
    }
}
/// The reporter never advertises adoption if its actual early hook did not run.
/// Extension compatibility is separate from admission and actual resource binding.
pub(crate) fn support_extensions() -> std::collections::BTreeMap<String, serde_json::Value> {
    let mut map = std::collections::BTreeMap::new();
    if STARTED.load(Ordering::Acquire) {
        map.insert(
            SUPPORT_KEY.into(),
            serde_json::json!({"e1Version":2,"imageCustody":1,"clipboardCustody":1,"preThreadPin":1}),
        );
    }
    map
}
fn require_support(response: &crate::upgrade_target::Response) -> io::Result<()> {
    let data = response
        .extensions
        .get(SUPPORT_KEY)
        .ok_or_else(|| invalid("target does not advertise image/E1 custody adoption"))?;
    let support: Support = serde_json::from_value(data.clone()).map_err(invalid)?;
    support.validate()
}
/// Observation of the exact signed candidate, retaining its SAME lease and open
/// image. Neither a response DTO nor Native protocol3 constructs this resource.
pub struct SupportedTarget {
    actual: Arc<crate::upgrade_admission::Received>,
}
impl SupportedTarget {
    /// Probe before entering actual command execution's bounded FD section.
    pub fn observe(
        actual: Arc<crate::upgrade_admission::Received>,
        deadline: Instant,
    ) -> io::Result<Self> {
        require_support(&actual.probe_retained_image(deadline)?)?;
        Ok(Self { actual })
    }
    fn recheck(&self, scope: &FrontendCommandExecScope<'_>) -> io::Result<()> {
        let receipt = self.actual.receipt();
        self.actual.validate(&receipt.executable, receipt.build)?;
        scope.require_target_image(&receipt.executable, &self.actual.image)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FileRef(u32);
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Link {
    path: Vec<u8>,
    device: u64,
    inode: u64,
    length: u64,
    mode: u32,
}
impl Link {
    fn capture(file: &NamedTempFile) -> io::Result<Self> {
        let held = file.as_file().metadata()?;
        let path = if file.path().is_absolute() {
            file.path().to_path_buf()
        } else {
            std::env::current_dir()?.join(file.path())
        };
        let data = Self {
            path: path.as_os_str().as_bytes().to_vec(),
            device: held.dev(),
            inode: held.ino(),
            length: held.len(),
            mode: held.mode(),
        };
        data.validate(file.as_file())?;
        Ok(data)
    }
    fn path(&self) -> io::Result<PathBuf> {
        if self.path.len() > MAX_PATH || self.path.contains(&0) {
            return Err(invalid("invalid original image-file link"));
        }
        let path = PathBuf::from(std::ffi::OsString::from_vec(self.path.clone()));
        if !path.is_absolute() {
            return Err(invalid("original image-file link is not absolute"));
        }
        Ok(path)
    }
    fn matches(&self, value: &Metadata) -> bool {
        value.is_file()
            && value.dev() == self.device
            && value.ino() == self.inode
            && value.len() == self.length
            && value.mode() == self.mode
            && value.nlink() > 0
    }
    fn validate(&self, actual: &File) -> io::Result<PathBuf> {
        let path = self.path()?;
        let held = actual.metadata()?;
        let linked = std::fs::symlink_metadata(&path)?;
        if !self.matches(&held) || !self.matches(&linked) || linked.file_type().is_symlink() {
            return Err(invalid("original image descriptor/inode/link differs"));
        }
        Ok(path)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Bundle {
    nonce: [u8; 32],
    files: Vec<Link>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocatorData {
    schema: u32,
    #[serde(deserialize_with = "required")]
    bundle: Option<Bundle>,
}
fn required<'de, D, T>(reader: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(reader)
}
impl LocatorData {
    pub fn validate_data(&self) -> io::Result<()> {
        if self.schema != 1
            || self
                .bundle
                .as_ref()
                .is_some_and(|b| b.files.is_empty() || b.files.len() > MAX_FILES)
        {
            return Err(invalid("image custody locator schema/capacity"));
        }
        if let Some(bundle) = &self.bundle {
            for file in &bundle.files {
                file.path()?;
            }
        }
        Ok(())
    }
}
/// Constructed only by reading the required resource leaf from THIS actual E1.
/// There is no API accepting an argv/string/path locator plus an unrelated entry.
pub struct SealedLocator {
    entry: Arc<EnteredRetainedFrontend>,
    data: LocatorData,
}
impl SealedLocator {
    pub fn from_entered(entry: Arc<EnteredRetainedFrontend>) -> io::Result<Self> {
        let header = image_header(entry.input_data(), entry.service_data())?;
        Ok(Self {
            entry,
            data: header.image_custody,
        })
    }
}
#[derive(Deserialize)]
struct ImageHeader {
    version: u32,
    service: String,
    image_custody: LocatorData,
    // Required opaque native field. Only the whole native decoder can validate
    // ClipboardData/bind its resource; this SDK header grants none of that.
    clipboard: Box<serde_json::value::RawValue>,
}
fn image_header(bytes: &[u8], service: &str) -> io::Result<ImageHeader> {
    if !bytes.starts_with(UI_MAGIC) {
        return Err(invalid("image custody has no required whole-UI2 E1"));
    }
    let header: ImageHeader = serde_json::from_slice(&bytes[UI_MAGIC.len()..]).map_err(invalid)?;
    if header.version != 2 || header.service != service {
        return Err(invalid("image custody E1/service differs"));
    }
    // Presence/object grammar only. This is not Clipboard schema validation,
    // application or a capability for the still-unadvertised whole App scene.
    if !header.clipboard.get().trim_start().starts_with('{') {
        return Err(invalid(
            "required whole-UI2 clipboard DATA is absent/null/nonobject",
        ));
    }
    header.image_custody.validate_data()?;
    Ok(header)
}
/// The SAME original NamedTempFile object, not merely an fd/path DATA record.
/// Tui keeps this Arc in its actual owner slot through refusal/cancel/Unknown.
pub struct OriginalFile {
    owned: Mutex<Option<NamedTempFile>>,
}
impl OriginalFile {
    pub fn retain_native(actual: NamedTempFile) -> Arc<Self> {
        Arc::new(Self {
            owned: Mutex::new(Some(actual)),
        })
    }
    fn vacant() -> Arc<Self> {
        Arc::new(Self {
            owned: Mutex::new(None),
        })
    }
    pub fn with_native<T>(
        &self,
        work: impl FnOnce(&NamedTempFile) -> io::Result<T>,
    ) -> io::Result<T> {
        let owned = self
            .owned
            .lock()
            .map_err(|_| invalid("original image-file owner interrupted"))?;
        work(
            owned
                .as_ref()
                .ok_or_else(|| invalid("original image-file owner not complete"))?,
        )
    }
}
/// Already-prepared actual rights. Preparation sends every byte/fd and closes
/// the source write-half BEFORE a locator/exec inheritance can be obtained.
pub struct Offer {
    socket: Option<PreparedFrontendSocket>,
    files: Vec<Arc<OriginalFile>>,
    data: LocatorData,
}
impl Offer {
    pub fn prepare(actual: &[Arc<OriginalFile>]) -> io::Result<Self> {
        let mut files: Vec<Arc<OriginalFile>> = Vec::new();
        for file in actual {
            if !files.iter().any(|old| Arc::ptr_eq(old, file)) {
                files.push(file.clone());
            }
        }
        if files.len() > MAX_FILES {
            return Err(invalid("image-file custody capacity"));
        }
        if files.is_empty() {
            return Ok(Self {
                socket: None,
                files,
                data: LocatorData {
                    schema: 1,
                    bundle: None,
                },
            });
        }
        let links = files
            .iter()
            .map(|file| file.with_native(Link::capture))
            .collect::<io::Result<Vec<_>>>()?;
        let mut nonce = [0; 32];
        for chunk in nonce.as_chunks_mut::<8>().0 {
            chunk.copy_from_slice(
                &codex_alab_core::native_source::connection_request_prefix().to_le_bytes(),
            );
        }
        let receiver = prepare_channel(&files, nonce)?;
        Ok(Self {
            socket: Some(PreparedFrontendSocket::from_owned(receiver)?),
            files,
            data: LocatorData {
                schema: 1,
                bundle: Some(Bundle {
                    nonce,
                    files: links,
                }),
            },
        })
    }
    pub fn locator_data(&self) -> io::Result<&LocatorData> {
        if self.data.bundle.is_some()
            && !self
                .socket
                .as_ref()
                .map(PreparedFrontendSocket::is_owned)
                .transpose()?
                .unwrap_or(false)
        {
            Err(invalid("image custody inheritance failed"))
        } else {
            Ok(&self.data)
        }
    }
    pub fn file_ref(&self, actual: &Arc<OriginalFile>) -> io::Result<FileRef> {
        self.files
            .iter()
            .position(|old| Arc::ptr_eq(old, actual))
            .map(|i| FileRef(i as u32))
            .ok_or_else(|| invalid("image-file owner absent from actual capture"))
    }
    /// Only bounded flag changes, under the SAME real command execution scope.
    pub fn inherit(
        &self,
        scope: &FrontendCommandExecScope<'_>,
        target: &SupportedTarget,
    ) -> io::Result<()> {
        target.recheck(scope)?;
        if let Some(socket) = &self.socket {
            scope.inherit_socket(socket)
        } else if self.data.bundle.is_none() {
            Ok(())
        } else {
            Err(invalid("image custody transport is no longer owned"))
        }
    }
}
// Shared actual kernel channel preparation. Tests exercise this lower resource
// slice without constructing an Entered/granted command or reconstructing owners.
fn prepare_channel(files: &[Arc<OriginalFile>], nonce: [u8; 32]) -> io::Result<UnixStream> {
    let (receiver, mut sender) = {
        let _custody = codex_alab_core::native_fd_custody::receiving()?;
        UnixStream::pair()?
    };
    receiver.set_nonblocking(true)?;
    sender.set_nonblocking(true)?;
    send_one(&sender, MAGIC, sender.as_fd())?;
    sender.write_all(&nonce)?;
    sender.write_all(&u32::try_from(files.len()).map_err(invalid)?.to_le_bytes())?;
    for file in files {
        file.with_native(|file| send_one(&sender, b"F", file.as_file().as_fd()))?;
    }
    sender.shutdown(std::net::Shutdown::Write)?;
    drop(sender); // queued SAME sender right preserves Darwin's original peer.
    Ok(receiver)
}
fn send_one(
    stream: &UnixStream,
    marker: &[u8],
    right: std::os::fd::BorrowedFd<'_>,
) -> io::Result<()> {
    let mut storage = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut ancillary = rustix::net::SendAncillaryBuffer::new(&mut storage);
    let rights = [right];
    if !ancillary.push(rustix::net::SendAncillaryMessage::ScmRights(&rights)) {
        return Err(invalid("image-file right did not fit"));
    }
    let sent = rustix::net::sendmsg(
        stream,
        &[std::io::IoSlice::new(marker)],
        &mut ancillary,
        rustix::net::SendFlags::empty(),
    )?;
    if sent != marker.len() {
        return Err(invalid("incomplete image-file rights marker"));
    }
    Ok(())
}

struct Progress {
    keepalive: Option<OwnedFd>,
    header: [u8; 36],
    read: usize,
    raw: Vec<Option<OwnedFd>>,
    names: Vec<Option<TempPath>>,
    owners: Option<Arc<ClaimedFiles>>,
    installed: Weak<ClaimedFiles>,
    complete: bool,
}
impl Default for Progress {
    fn default() -> Self {
        Self {
            keepalive: None,
            header: [0; 36],
            read: 0,
            raw: Vec::new(),
            names: Vec::new(),
            owners: None,
            installed: Weak::new(),
            complete: false,
        }
    }
}
struct Record {
    socket: UnixStream,
    state: Mutex<Progress>,
    entry: Mutex<Option<Weak<EnteredRetainedFrontend>>>,
}
static PINNED: OnceLock<Mutex<Vec<Arc<Record>>>> = OnceLock::new();
fn pinned() -> &'static Mutex<Vec<Arc<Record>>> {
    PINNED.get_or_init(|| Mutex::new(Vec::new()))
}

/// Pin only actual inherited self-peer mailboxes. This takes no resource rights
/// and authenticates no E1/permission. Claims wait for a genuine sealed entry.
/// # Safety
/// Invoke before runtime threads or any code can close/reuse inherited raw fds.
pub unsafe fn pin_before_threads() -> io::Result<()> {
    let mut book = pinned()
        .lock()
        .map_err(|_| invalid("image custody pinning interrupted"))?;
    let mut candidates = Vec::new();
    let Ok(sockets) = inherited_sockets() else {
        return Ok(());
    };
    for raw in sockets {
        if peer_pid(raw).ok() != Some(std::process::id()) {
            continue;
        }
        let mut marker = [0; 16];
        // SAFETY: same exclusive pre-thread inventory as original exec custody;
        // peeking neither consumes ancillary rights nor takes ownership.
        let size = unsafe {
            libc::recv(
                raw,
                marker.as_mut_ptr().cast(),
                marker.len(),
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        if size == 16 && marker == *MAGIC {
            candidates.push(raw);
        }
    }
    if book.len() + candidates.len() > MAX_CHANNELS {
        return Err(invalid("image custody channel capacity"));
    }
    for raw in candidates {
        // SAFETY: atomically duplicate while raw numbers are exclusively stable.
        let duplicate = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 3) };
        if duplicate < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fcntl just created this sole owned descriptor.
        let socket = UnixStream::from(unsafe { OwnedFd::from_raw_fd(duplicate) });
        socket.set_nonblocking(true)?;
        book.push(Arc::new(Record {
            socket,
            state: Mutex::new(Progress::default()),
            entry: Mutex::new(None),
        }));
        // SAFETY: this original inherited channel is exclusively owned here;
        // the durable CLOEXEC duplicate is stored before closing it.
        drop(unsafe { OwnedFd::from_raw_fd(raw) });
    }
    STARTED.store(true, Ordering::Release);
    Ok(())
}

struct FileTable {
    pending: Option<Vec<Arc<OriginalFile>>>,
    installed: Vec<Weak<OriginalFile>>,
}
impl FileTable {
    fn new(files: Vec<Arc<OriginalFile>>) -> Self {
        Self {
            pending: Some(files),
            installed: Vec::new(),
        }
    }
    fn is_empty(&self) -> bool {
        self.pending
            .as_ref()
            .map_or(self.installed.is_empty(), Vec::is_empty)
    }
    fn file(&self, id: FileRef) -> io::Result<Arc<OriginalFile>> {
        if let Some(files) = &self.pending {
            return files
                .get(id.0 as usize)
                .cloned()
                .ok_or_else(|| invalid("unknown actual image-file reference"));
        }
        self.installed
            .get(id.0 as usize)
            .and_then(Weak::upgrade)
            .ok_or_else(|| invalid("original image-file slot was retired by its actual scene"))
    }
    fn handoff(
        &mut self,
        actual: &[Arc<OriginalFile>],
    ) -> io::Result<Option<Vec<Arc<OriginalFile>>>> {
        let Some(pending) = &self.pending else {
            return Ok(None);
        };
        if pending
            .iter()
            .any(|file| !actual.iter().any(|held| Arc::ptr_eq(file, held)))
        {
            return Err(invalid(
                "actual scene did not adopt the complete original image-file census",
            ));
        }
        let installed = pending.iter().map(Arc::downgrade).collect();
        self.installed = installed;
        Ok(self.pending.take())
    }
}
pub struct ClaimedFiles {
    entry: Arc<EnteredRetainedFrontend>,
    files: Mutex<FileTable>,
    record: Weak<Record>,
}
impl ClaimedFiles {
    pub fn file(&self, id: FileRef) -> io::Result<Arc<OriginalFile>> {
        self.files
            .lock()
            .map_err(|_| invalid("actual image file table interrupted"))?
            .file(id)
    }
    fn is_empty(&self) -> io::Result<bool> {
        Ok(self
            .files
            .lock()
            .map_err(|_| invalid("actual image file table interrupted"))?
            .is_empty())
    }
    /// Every original owner must ALREADY be held by actual Tui scene slots.
    /// This demotes lookup custody only; no Native/Source/effect receipt exists.
    pub fn finish_scene_handoff(&self, actual: &[Arc<OriginalFile>]) -> io::Result<()> {
        let retired = self
            .files
            .lock()
            .map_err(|_| invalid("actual image scene handoff interrupted"))?
            .handoff(actual)?;
        drop(retired);
        Ok(())
    }
    fn pending_files(&self) -> io::Result<Vec<Arc<OriginalFile>>> {
        self.files
            .lock()
            .map_err(|_| invalid("actual pending image files interrupted"))?
            .pending
            .clone()
            .ok_or_else(|| invalid("image pending owners already handed to scene"))
    }
    pub fn same_entry(&self, actual: &Arc<EnteredRetainedFrontend>) -> bool {
        Arc::ptr_eq(&self.entry, actual)
    }
    /// The actual Tui owns this nonserializable storage before its graph install.
    /// Store SAME owners first; only then release the transport ledger's pin.
    pub fn install_into(self: &Arc<Self>, actual: &mut FileBindings) -> io::Result<()> {
        if !Arc::ptr_eq(&actual.entry, &self.entry) {
            return Err(invalid("image-file target has different actual E1"));
        }
        if let Some(old) = &actual.owned {
            if Arc::ptr_eq(old, self) || old.is_empty()? && self.is_empty()? {
                return Ok(());
            }
            return Err(invalid("image-file target already bound"));
        }
        actual.owned = Some(self.clone());
        if let Some(record) = self.record.upgrade() {
            let retired = {
                let mut state = record
                    .state
                    .lock()
                    .map_err(|_| invalid("image custody completion interrupted"))?;
                if !state.complete {
                    return Err(invalid("image custody claim incomplete"));
                }
                state.installed = Arc::downgrade(self);
                let owned = state.owners.take();
                let keepalive = state.keepalive.take();
                drop(keepalive);
                let _closed = record.socket.shutdown(std::net::Shutdown::Both);
                owned
            };
            drop(retired);
        }
        Ok(())
    }
}
/// Real Entry-owned storage, embedded in actual Tui renderer/resource state.
/// It is not deserializable and cannot be constructed from locator/path DATA.
pub struct FileBindings {
    entry: Arc<EnteredRetainedFrontend>,
    owned: Option<Arc<ClaimedFiles>>,
}
impl FileBindings {
    pub fn from_entered(entry: Arc<EnteredRetainedFrontend>) -> Self {
        Self { entry, owned: None }
    }
    pub fn file(&self, id: FileRef) -> io::Result<Arc<OriginalFile>> {
        self.owned
            .as_ref()
            .ok_or_else(|| invalid("actual image-file binding not installed"))?
            .file(id)
    }
}

// Real ancillary-stream lookahead shared by claim selection and its kernel
// control. Peeking must preserve MAGIC's queued SCM_RIGHTS for later ownership.
fn peek_nonce(socket: &UnixStream) -> io::Result<Option<[u8; 32]>> {
    let mut bytes = [0; 48];
    // SAFETY: strong socket owner keeps its descriptor stable; buffer is bounded
    // and MSG_PEEK consumes neither bytes nor queued ownership.
    let size = unsafe {
        libc::recv(
            socket.as_raw_fd(),
            bytes.as_mut_ptr().cast(),
            bytes.len(),
            libc::MSG_PEEK | libc::MSG_DONTWAIT,
        )
    };
    if size < 0 {
        let error = io::Error::last_os_error();
        if matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
        ) {
            return Ok(None);
        }
        return Err(error);
    }
    if size != 48 || bytes[..16] != MAGIC[..] {
        return Ok(None);
    }
    let mut nonce = [0; 32];
    nonce.copy_from_slice(&bytes[16..]);
    Ok(Some(nonce))
}
pub fn claim(actual: &SealedLocator) -> io::Result<Arc<ClaimedFiles>> {
    let Some(bundle) = &actual.data.bundle else {
        return Ok(Arc::new(ClaimedFiles {
            entry: actual.entry.clone(),
            files: Mutex::new(FileTable::new(Vec::new())),
            record: Weak::new(),
        }));
    };
    let records = pinned()
        .lock()
        .map_err(|_| invalid("image custody pins interrupted"))?
        .clone();
    let mut matching = Vec::new();
    for record in records {
        if peek_nonce(&record.socket)?.is_some_and(|nonce| nonce == bundle.nonce) {
            matching.push(record);
        } else {
            let entry = record
                .entry
                .lock()
                .map_err(|_| invalid("image custody entry interrupted"))?;
            if entry
                .as_ref()
                .is_some_and(|old| Weak::ptr_eq(old, &Arc::downgrade(&actual.entry)))
            {
                matching.push(record.clone());
            }
        }
    }
    if matching.len() != 1 {
        return Err(invalid("actual image custody channel missing/ambiguous"));
    }
    let record = matching
        .pop()
        .ok_or_else(|| invalid("actual image custody channel missing"))?;
    {
        let mut entry = record
            .entry
            .lock()
            .map_err(|_| invalid("image custody entry interrupted"))?;
        if entry
            .as_ref()
            .is_some_and(|old| !Weak::ptr_eq(old, &Arc::downgrade(&actual.entry)))
        {
            return Err(invalid("image custody belongs to another actual E1"));
        }
        if entry.is_none() {
            *entry = Some(Arc::downgrade(&actual.entry));
        }
    }
    receive_all(&record, bundle, &actual.entry)
}
fn receive_all(
    record: &Arc<Record>,
    bundle: &Bundle,
    entry: &Arc<EnteredRetainedFrontend>,
) -> io::Result<Arc<ClaimedFiles>> {
    let mut state = record
        .state
        .lock()
        .map_err(|_| invalid("image custody receipt interrupted"))?;
    if state.complete {
        return state
            .owners
            .clone()
            .or_else(|| state.installed.upgrade())
            .ok_or_else(|| invalid("image custody owner was retired by its actual target"));
    }
    let paths = receive_descriptors(&record.socket, &mut state, bundle)?;
    let count = paths.len();
    if state.owners.is_none() {
        let files = (0..count).map(|_| OriginalFile::vacant()).collect();
        state.owners = Some(Arc::new(ClaimedFiles {
            entry: entry.clone(),
            files: Mutex::new(FileTable::new(files)),
            record: Arc::downgrade(record),
        }));
        state.names = (0..count).map(|_| None).collect();
    }
    let owners = state
        .owners
        .as_ref()
        .ok_or_else(|| invalid("missing actual image owner slots"))?
        .clone();
    // Prelock every fallible destination before moving any fd/unlink ownership.
    let pending_files = owners.pending_files()?;
    let mut destinations = pending_files
        .iter()
        .map(|file| {
            file.owned
                .lock()
                .map_err(|_| invalid("image owner installation interrupted"))
        })
        .collect::<io::Result<Vec<_>>>()?;
    for (index, path) in paths.into_iter().enumerate() {
        if state.names[index].is_none() {
            let fd = state.raw[index]
                .as_ref()
                .ok_or_else(|| invalid("missing staged original image descriptor"))?;
            let verified = bundle.files[index].validate(&File::from(fd.try_clone()?))?;
            if verified != path {
                return Err(invalid("image unlink staging path changed"));
            }
            state.names[index] = Some(TempPath::try_from_path(path)?);
        }
    }
    for (index, target) in destinations.iter_mut().enumerate() {
        let fd = state.raw[index]
            .take()
            .ok_or_else(|| invalid("missing retained image descriptor"))?;
        let path = state.names[index]
            .take()
            .ok_or_else(|| invalid("missing retained image unlink owner"))?;
        **target = Some(NamedTempFile::from_parts(File::from(fd), path));
    }
    state.complete = true;
    drop(destinations);
    Ok(owners)
}

fn receive_descriptors(
    mut socket: &UnixStream,
    state: &mut Progress,
    bundle: &Bundle,
) -> io::Result<Vec<PathBuf>> {
    if state.keepalive.is_none() {
        let [right] = crate::upgrade_admission::receive_rights::<1>(socket, MAGIC, None)?;
        state.keepalive = Some(right);
        if peer_pid(
            state
                .keepalive
                .as_ref()
                .ok_or_else(|| invalid("missing image keepalive"))?
                .as_raw_fd(),
        )? != std::process::id()
        {
            return Err(invalid("image custody peer is not same process"));
        }
    }
    while state.read < state.header.len() {
        let start = state.read;
        let read = (socket).read(&mut state.header[start..])?;
        if read == 0 {
            return Err(invalid("incomplete image custody header"));
        }
        state.read += read;
    }
    let count = u32::from_le_bytes(state.header[32..36].try_into().map_err(invalid)?) as usize;
    if state.header[..32] != bundle.nonce || count != bundle.files.len() {
        return Err(invalid("image custody E1/physical descriptor cut differs"));
    }
    while state.raw.len() < count {
        let [right] = crate::upgrade_admission::receive_rights::<1>(socket, b"F", None)?;
        state.raw.push(Some(right)); // actual received ownership stored BEFORE checks.
    }
    let mut tail = [0];
    if (socket).read(&mut tail)? != 0 {
        return Err(invalid("image custody has a surplus tail"));
    }
    bundle
        .files
        .iter()
        .zip(&state.raw)
        .map(|(link, fd)| {
            let fd = fd
                .as_ref()
                .ok_or_else(|| invalid("image descriptor already consumed"))?;
            let file = File::from(fd.try_clone()?);
            link.validate(&file)
        })
        .collect::<io::Result<Vec<_>>>()
}
#[cfg(test)]
#[path = "retained_image_custody_tests.rs"]
mod tests;
