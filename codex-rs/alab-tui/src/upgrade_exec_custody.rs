// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Same-process resource custody across exec. This channel transfers existing
//! close-only descriptors; it supplies no package admission or execution grant.
//! No environment, argument, pathname record, or guessed lease descriptor is an
//! ownership token. The receiving kernel creates the new owned descriptors.

use crate::upgrade_admission::Receipt;
use crate::upgrade_admission::Received;
use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::fd::AsFd;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::os::fd::RawFd;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

const MAGIC: &[u8; 16] = b"codex-upgrade-v2";
pub const PROTOCOL: u32 = 2;
/// Bound the source snapshot before allocating or reading destination state.
pub const MAX_STATE_BYTES: usize = 256 * 1024 * 1024;
struct Adopted {
    _lease: File,
    state: Mutex<Option<Vec<u8>>>,
}
static ADOPTED: OnceLock<Adopted> = OnceLock::new();

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<R = Receipt> {
    receipt: R,
    #[serde(deserialize_with = "required")]
    state_size: Option<u64>,
}

fn required<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    T::deserialize(deserializer)
}

/// Take source data once. This does not authorize native SDK construction or
/// model execution; fresh startup must independently validate its typed payload.
pub fn take_state() -> Option<Vec<u8>> {
    take_state_if(|_| true)
}
/// Select a typed payload without consuming another user's opaque state.
/// The predicate sees data only, under the existing custody owner lock.
pub fn take_state_if(matches: impl FnOnce(&[u8]) -> bool) -> Option<Vec<u8>> {
    let mut state = ADOPTED
        .get()?
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.as_deref().is_some_and(matches) {
        return None;
    }
    state.take()
}
static STARTED: AtomicBool = AtomicBool::new(false);

/// A library consumer cannot advertise this protocol without running its early
/// startup hook. Unsupported platforms report no adoption capability.
pub(crate) fn startup_protocol() -> u32 {
    if cfg!(any(target_os = "linux", target_os = "macos")) && STARTED.load(Ordering::Acquire) {
        PROTOCOL
    } else {
        0
    }
}

pub(crate) fn adopted() -> bool {
    ADOPTED.get().is_some()
}

/// The original lease, image and a connection keepalive are kernel-queued behind
/// this socket. Dropping an uncommitted channel closes its queued rights.
pub struct ExecCustody(UnixStream);

impl Received {
    pub fn prepare_exec_custody(&self) -> io::Result<ExecCustody> {
        self.prepare_exec_custody_with_state(None)
    }

    /// Retain a read-only, unlinked snapshot beside the original image/lease.
    /// Source data is never an SDK or account execution grant.
    pub fn prepare_exec_custody_with_state(&self, state: Option<&[u8]>) -> io::Result<ExecCustody> {
        use std::os::unix::fs::MetadataExt;
        self.validate(&self.receipt.executable, self.receipt.build)?;
        if state.is_some_and(|bytes| bytes.len() > MAX_STATE_BYTES) {
            return Err(invalid("oversized exec state"));
        }
        let mut snapshot = tempfile::NamedTempFile::new()?;
        snapshot.write_all(state.unwrap_or_default())?;
        let data = File::open(snapshot.path())?;
        let original = snapshot.as_file().metadata()?;
        let held = data.metadata()?;
        if (original.dev(), original.ino()) != (held.dev(), held.ino()) {
            return Err(invalid("exec state file changed during preparation"));
        }
        // Unlink and close the sole writable source owner before queuing rights.
        snapshot.close()?;
        let envelope = Envelope {
            receipt: &self.receipt,
            state_size: state.map(|bytes| bytes.len() as u64),
        };
        let mut bytes = serde_json::to_vec(&envelope)?;
        if bytes.len() > 16 * 1024 {
            return Err(invalid("oversized exec custody receipt"));
        }
        bytes.push(b'\n');
        let (receiver, mut sender) = UnixStream::pair()?;
        receiver.set_nonblocking(true)?;
        sender.set_nonblocking(true)?;
        let rights = [
            self.lease.as_fd(),
            self.image.as_fd(),
            sender.as_fd(),
            data.as_fd(),
        ];
        let mut storage = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(4))];
        let mut ancillary = rustix::net::SendAncillaryBuffer::new(&mut storage);
        if !ancillary.push(rustix::net::SendAncillaryMessage::ScmRights(&rights)) {
            return Err(invalid("exec custody rights did not fit"));
        }
        let sent = rustix::net::sendmsg(
            &sender,
            &[std::io::IoSlice::new(MAGIC)],
            &mut ancillary,
            rustix::net::SendFlags::empty(),
        )?;
        if sent != MAGIC.len() {
            return Err(invalid("incomplete exec custody marker"));
        }
        sender.write_all(&bytes)?;
        self.validate(&self.receipt.executable, self.receipt.build)?;
        // The queued sender descriptor keeps this self-connected socket alive
        // after its ordinary owner closes, including through exec. Darwin can
        // still authenticate its original peer when the next image receives it.
        drop(sender);
        Ok(ExecCustody(receiver))
    }
}

impl ExecCustody {
    /// Only immediately before the already committed exec, after native starts
    /// are fenced and child teardown finishes. Ordinary children must not inherit it.
    pub fn inherit(&self) -> io::Result<()> {
        rustix::io::fcntl_setfd(&self.0, rustix::io::FdFlags::empty())?;
        Ok(())
    }
}

/// Adopt one controlled same-PID channel, before any code can reuse inherited
/// descriptors. A prior process's channel cannot authenticate as this process.
/// The original lease remains close-only and CLOEXEC throughout the new runtime.
///
/// # Safety
/// Call once at the first synchronous main hook. No other thread or callback may
/// close or replace the inherited channel while its raw descriptor is inspected
/// and consumed. It has no surviving Rust or foreign owner in the new image.
pub unsafe fn adopt_before_threads() -> io::Result<()> {
    // Descriptor inspection is optional for an ordinary launch. Leave the
    // advertised capability at zero when this host cannot inspect its own FDs;
    // an updater's typed preflight will then refuse this handoff protocol.
    let Ok(sockets) = inherited_sockets() else {
        return Ok(());
    };
    let mut channels = Vec::new();
    for fd in sockets {
        if peer_pid(fd).ok() != Some(std::process::id()) {
            continue;
        }
        let mut marker = [0; MAGIC.len()];
        // SAFETY: recv borrows this integer and the writable stack buffer;
        // MSG_PEEK neither consumes another socket's bytes nor takes ownership.
        let size = unsafe {
            libc::recv(
                fd,
                marker.as_mut_ptr().cast(),
                marker.len(),
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        if size == MAGIC.len() as isize && marker == *MAGIC {
            channels.push(fd);
        }
    }
    if channels.is_empty() {
        STARTED.store(true, Ordering::Release);
        return Ok(());
    }
    if channels.len() != 1 || ADOPTED.get().is_some() {
        return Err(invalid("ambiguous exec custody channels"));
    }
    let raw = channels[0];
    // SAFETY: fcntl atomically creates a new, uniquely owned CLOEXEC descriptor;
    // the caller keeps the inherited raw number stable until consumption below.
    let duplicate = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 3) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the successful duplication just created this sole owner.
    let stream = UnixStream::from(unsafe { OwnedFd::from_raw_fd(duplicate) });
    stream.set_nonblocking(true)?;
    let [lease, image, keepalive, data] =
        crate::upgrade_admission::receive_rights(&stream, MAGIC, None)?;
    if peer_pid(keepalive.as_raw_fd())? != std::process::id() {
        return Err(invalid("exec custody keepalive is not this process"));
    }
    let mut bytes = Vec::new();
    let mut input = &stream;
    loop {
        let mut byte = [0];
        input.read_exact(&mut byte)?;
        if byte == *b"\n" {
            break;
        }
        if bytes.len() == 16 * 1024 {
            return Err(invalid("oversized exec custody receipt"));
        }
        bytes.push(byte[0]);
    }
    let envelope: Envelope<Receipt> = serde_json::from_slice(&bytes)?;
    let state = read_state(File::from(data), envelope.state_size)?;
    let current = std::env::current_exe()?.canonicalize()?;
    let received = Received {
        receipt: envelope.receipt,
        lease: File::from(lease),
        image: File::from(image),
    };
    received.validate(&received.receipt.executable, received.receipt.build)?;
    if received.receipt.executable.canonicalize()? != current {
        return Err(invalid("exec custody does not name this image"));
    }
    ADOPTED
        .set(Adopted {
            _lease: received.lease,
            state: Mutex::new(state),
        })
        .map_err(|_| invalid("exec custody was already adopted"))?;
    // SAFETY: this is the exclusively inherited channel authenticated above.
    // The new lease owner already exists; closing the channel releases only its
    // keepalive/transport, never explicitly unlocking the original lease.
    drop(unsafe { OwnedFd::from_raw_fd(raw) });
    STARTED.store(true, Ordering::Release);
    Ok(())
}

fn read_state(mut file: File, size: Option<u64>) -> io::Result<Option<Vec<u8>>> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    let expected = size.unwrap_or_default();
    if expected > MAX_STATE_BYTES as u64
        || !metadata.is_file()
        || metadata.nlink() != 0
        || metadata.len() != expected
        || (rustix::fs::fcntl_getfl(&file)? & rustix::fs::OFlags::ACCMODE)
            != rustix::fs::OFlags::RDONLY
    {
        return Err(invalid("invalid exec state descriptor"));
    }
    // Do not trust the shared file offset, including on a replayed offer.
    use std::io::Seek;
    file.rewind()?;
    let mut bytes = Vec::new();
    file.take(expected + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != expected {
        return Err(invalid("incomplete exec state snapshot"));
    }
    Ok(size.map(|_| bytes))
}

#[cfg(target_os = "macos")]
fn inherited_sockets() -> io::Result<Vec<RawFd>> {
    // SAFETY: the null query asks for a byte count, not memory access.
    let size = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDLISTFDS,
            0,
            std::ptr::null_mut(),
            0,
        )
    };
    if size < 0 {
        return Err(io::Error::last_os_error());
    }
    if size == 0 {
        return Ok(Vec::new());
    }
    let count = usize::try_from(size).map_err(io::Error::other)?
        / std::mem::size_of::<libc::proc_fdinfo>()
        + 16;
    let mut rows = vec![
        libc::proc_fdinfo {
            proc_fd: 0,
            proc_fdtype: 0
        };
        count
    ];
    let capacity = i32::try_from(rows.len() * std::mem::size_of::<libc::proc_fdinfo>())
        .map_err(io::Error::other)?;
    // SAFETY: rows is initialized writable storage of exactly capacity bytes.
    let written = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDLISTFDS,
            0,
            rows.as_mut_ptr().cast(),
            capacity,
        )
    };
    if written < 0 {
        return Err(io::Error::last_os_error());
    }
    if written >= capacity
        || !(written as usize).is_multiple_of(std::mem::size_of::<libc::proc_fdinfo>())
    {
        return Err(invalid("incomplete startup descriptor inventory"));
    }
    let used = usize::try_from(written).map_err(io::Error::other)?
        / std::mem::size_of::<libc::proc_fdinfo>();
    Ok(rows
        .into_iter()
        .take(used)
        .filter(|row| i64::from(row.proc_fdtype) == i64::from(libc::PROX_FDTYPE_SOCKET))
        .map(|row| row.proc_fd)
        .filter(|fd| offered(*fd))
        .collect())
}

#[cfg(target_os = "linux")]
fn inherited_sockets() -> io::Result<Vec<RawFd>> {
    Ok(std::fs::read_dir("/proc/self/fd")?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .filter(|fd| offered(*fd))
        .collect())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn inherited_sockets() -> io::Result<Vec<RawFd>> {
    Ok(Vec::new())
}

fn offered(fd: RawFd) -> bool {
    if fd < 3 {
        return false;
    }
    // SAFETY: F_GETFD queries flags without taking ownership or accessing memory.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    flags >= 0 && flags & libc::FD_CLOEXEC == 0
}

fn peer_pid(fd: RawFd) -> io::Result<u32> {
    #[cfg(target_os = "macos")]
    {
        let mut pid: libc::pid_t = 0;
        let mut len = std::mem::size_of_val(&pid) as libc::socklen_t;
        // SAFETY: getsockopt writes the initialized pid and bounded length. A
        // descriptor of another kind simply refuses this Unix-specific option.
        let result = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_LOCAL,
                libc::LOCAL_PEEREPID,
                (&mut pid as *mut libc::pid_t).cast(),
                &mut len,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        if len as usize != std::mem::size_of_val(&pid) {
            return Err(invalid("invalid exec custody peer identity"));
        }
        u32::try_from(pid).map_err(io::Error::other)
    }
    #[cfg(target_os = "linux")]
    {
        let mut cred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = std::mem::size_of_val(&cred) as libc::socklen_t;
        // SAFETY: the bounded initialized ucred storage matches SO_PEERCRED.
        let result = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut len,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        if len as usize != std::mem::size_of_val(&cred) {
            return Err(invalid("invalid exec custody peer identity"));
        }
        u32::try_from(cred.pid).map_err(io::Error::other)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = fd;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "exec custody peer identity unavailable",
        ))
    }
}

fn invalid(why: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::Duration;
    use std::time::Instant;

    fn received(root: &Path) -> Received {
        let executable = root.join("bin/codex");
        let image = File::open(&executable).unwrap();
        let lease_path = root.join("original.lease");
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lease_path)
            .unwrap();
        lease.try_lock().unwrap();
        let i = image.metadata().unwrap();
        let l = lease.metadata().unwrap();
        Received {
            receipt: Receipt {
                protocol: 1,
                program: "codex".into(),
                build: 7,
                version: "0.0.0".into(),
                root: root.into(),
                executable,
                tree_root: "a".repeat(64),
                device: i.dev(),
                inode: i.ino(),
                size: i.len(),
                modified_seconds: i.mtime(),
                modified_nanoseconds: i.mtime_nsec(),
                lease_path,
                lease_device: l.dev(),
                lease_inode: l.ino(),
            },
            lease,
            image,
        }
    }

    // A deliberately invalid goal body proves that early terminal recovery
    // precedes configuration decoding. No source/SDK permission is encoded.
    fn runtime_payload() -> Vec<u8> {
        codex_alab_core::activate();
        crate::terminal_modes::enable_raw_mode().unwrap();
        // The source owns both keyboard stacks and active terminal protocols.
        std::io::stdout().write_all(b"\x1b[?2004h\x1b[?1004h\x1b[>1u\x1b[?1049h\x1b[>1u\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h\x1b[?25l").unwrap();
        std::io::stdout().flush().unwrap();
        crate::input::buffer_input(b"\x1b[200~pending").unwrap();
        let stream = crate::input::InputStream::default();
        let mut input = stream.prepare_upgrade().unwrap();
        let mut modes = crate::terminal_modes::prepare_upgrade().unwrap();
        let startup = serde_json::json!({
            "version":1,"cursor":[0,0],"colors":null,"enhanced_keys":false,
            "terminal_app_over_ssh":false,"reports_key_releases":true,"da1_supported":false,"tmux_image_base":null,
            "viewport":[0,0,80,24],"saved_viewport":null,"hidden_cursor":true,"visible_history_rows":0,
            "screen":{"active":true,"mouse_active":true,"mouse_capture_disabled":false,"input_configured":true,"keyboard_active":true,"parent_reports_key_releases":true,"pointer":0},
            "focused":true,"owned_screen":true,"alt_screen_enabled":true,"mouse_capture":0
        });
        let body = serde_json::json!({"version":1,"primary":"00000000-0000-4000-8000-000000000001","goals":null,
            "terminal":{"input":serde_json::from_slice::<serde_json::Value>(input.snapshot()).unwrap(),"modes":modes.snapshot(),"startup":startup}});
        let mut frame = b"codex-runtime-state-v1\n".to_vec();
        serde_json::to_writer(&mut frame, &body).unwrap();
        assert!(modes.commit_if(|| input.commit_if(|| true)).unwrap());
        frame
    }

    fn blocked(path: &Path) {
        assert!(
            matches!(
                File::open(path).unwrap().try_lock(),
                Err(std::fs::TryLockError::WouldBlock)
            ),
            "original lease must remain locked"
        );
    }

    fn released(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            match File::open(path).unwrap().try_lock() {
                Ok(()) => return,
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                other => panic!("lease did not release after final custody closed: {other:?}"),
            }
        }
    }

    fn child_command(executable: &Path, root: &Path, phase: &str) -> std::process::Command {
        let mut command = std::process::Command::new(executable);
        command
            .args([
                "--exact",
                "upgrade_exec_custody::tests::custody_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("ALAB_TEST_EXEC_CUSTODY_ROOT", root)
            .env("ALAB_TEST_EXEC_CUSTODY_PHASE", phase)
            .stdout(std::process::Stdio::piped());
        command
    }

    fn run(mut command: std::process::Command) {
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "custody child failed: {status}");
                let mut output = String::new();
                child
                    .stdout
                    .take()
                    .unwrap()
                    .read_to_string(&mut output)
                    .unwrap();
                assert!(
                    output.contains("ALAB_EXEC_STATE_OK"),
                    "selected custody child did not finish: {output}"
                );
                return;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("custody child exceeded its watchdog");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn custody_child() {
        let Some(root) = std::env::var_os("ALAB_TEST_EXEC_CUSTODY_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let phase = std::env::var("ALAB_TEST_EXEC_CUSTODY_PHASE").unwrap();
        if phase == "no-hook" {
            println!("ALAB_NO_TERMINAL_RECOVERY");
            std::io::stdout().flush().unwrap();
            std::process::exit(7);
        }
        if phase == "source" {
            let offered = received(&root);
            let mode = std::env::var("ALAB_TEST_EXEC_CUSTODY_MODE").unwrap();
            let payload = if mode.starts_with("runtime-cli") {
                runtime_payload()
            } else {
                payload()
            };
            let channel = offered
                .prepare_exec_custody_with_state(Some(&payload))
                .unwrap();
            drop(offered);
            blocked(&root.join("original.lease"));
            channel.inherit().unwrap();
            let target = if mode == "wrong" {
                std::env::current_exe().unwrap()
            } else {
                root.join("bin/codex")
            };
            let mut command = if mode == "runtime-cli-no-hook" {
                child_command(&target, &root, "no-hook")
            } else if mode.starts_with("runtime-cli") {
                let mut command = std::process::Command::new(&target);
                if mode == "runtime-cli-constructor" {
                    command
                        .args(["resume", "00000000-0000-4000-8000-000000000001"])
                        .env("ATERM_CODEX_SOURCE_LAUNCH", "invalid-native-constructor");
                } else {
                    command
                        .arg("--alab-invalid-startup-argument")
                        .env_remove("ATERM_CODEX_SOURCE_LAUNCH");
                }
                command
            } else if mode == "cli" {
                let mut command = std::process::Command::new(&target);
                command.args(["debug", "upgrade-capabilities", "--hold"]);
                command
            } else {
                child_command(&target, &root, &mode)
            };
            // exec must retain the source parent's output pipe. Creating a new
            // piped stdout here would leave its read end with no Child owner.
            command.stdout(std::process::Stdio::inherit());
            command.env("ALAB_TEST_EXEC_CUSTODY_PID", std::process::id().to_string());
            panic!("exec returned: {}", command.exec());
        }
        // SAFETY: the new test image has exclusive custody of its inherited
        // channel. No harness code owns, closes, or replaces that raw number.
        let result = unsafe { adopt_before_threads() };
        if phase == "wrong" {
            assert!(result.is_err());
            assert!(ADOPTED.get().is_none());
            assert!(take_state().is_none());
            println!("ALAB_EXEC_STATE_OK");
            return;
        }
        result.unwrap();
        if phase == "foreign" {
            assert!(ADOPTED.get().is_none());
            assert!(take_state().is_none());
            println!("ALAB_EXEC_STATE_OK");
            return;
        }
        assert_eq!(
            std::env::var("ALAB_TEST_EXEC_CUSTODY_PID").unwrap(),
            std::process::id().to_string()
        );
        let lease = &ADOPTED.get().unwrap()._lease;
        assert!(
            rustix::io::fcntl_getfd(lease)
                .unwrap()
                .contains(rustix::io::FdFlags::CLOEXEC)
        );
        assert_eq!(
            take_state_if(|_| false),
            None,
            "a foreign payload stays owned"
        );
        assert_eq!(take_state_if(|bytes| bytes == payload()), Some(payload()));
        assert_eq!(take_state(), None, "state is consumed exactly once");
        blocked(&root.join("original.lease"));
        println!("ALAB_EXEC_STATE_OK");
    }

    fn payload() -> Vec<u8> {
        (0..1024 * 1024).map(|index| (index % 251) as u8).collect()
    }

    #[test]
    fn exec_state_refuses_linked_writable_oversized_and_mismatched_snapshots() {
        let mut source = tempfile::NamedTempFile::new().unwrap();
        source.write_all(b"state").unwrap();
        let readonly = File::open(source.path()).unwrap();
        assert!(read_state(readonly.try_clone().unwrap(), Some(5)).is_err());
        let writable = source.reopen().unwrap();
        source.close().unwrap();
        assert!(read_state(writable, Some(5)).is_err());
        assert!(
            read_state(
                readonly.try_clone().unwrap(),
                Some(MAX_STATE_BYTES as u64 + 1)
            )
            .is_err()
        );
        assert!(read_state(readonly.try_clone().unwrap(), Some(4)).is_err());
        assert!(read_state(readonly.try_clone().unwrap(), None).is_err());
        assert_eq!(
            read_state(readonly.try_clone().unwrap(), Some(5)).unwrap(),
            Some(b"state".to_vec())
        );
        // The first read advanced the original shared open-file offset.
        assert_eq!(
            read_state(readonly, Some(5)).unwrap(),
            Some(b"state".to_vec())
        );
    }

    #[test]
    fn exec_adopts_same_pid_custody_and_refuses_another_image_or_process() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("bin")).unwrap();
        std::fs::copy(std::env::current_exe().unwrap(), root.join("bin/codex")).unwrap();
        for mode in ["target", "wrong"] {
            let mut command = child_command(&std::env::current_exe().unwrap(), &root, "source");
            command.env("ALAB_TEST_EXEC_CUSTODY_MODE", mode);
            run(command);
            released(&root.join("original.lease"));
        }
        let offered = received(&root);
        let channel = offered.prepare_exec_custody().unwrap();
        drop(offered);
        channel.inherit().unwrap();
        run(child_command(&root.join("bin/codex"), &root, "foreign"));
        blocked(&root.join("original.lease"));
        drop(channel);
        released(&root.join("original.lease"));
    }

    #[test]
    #[ignore = "requires an explicitly built Codex CLI with the early custody hook"]
    fn real_cli_adopts_queued_custody_before_its_runtime_starts() {
        struct Reap(Option<std::process::Child>);
        impl Drop for Reap {
            fn drop(&mut self) {
                if let Some(child) = self.0.as_mut() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("bin")).unwrap();
        std::fs::copy(
            std::env::var_os("ALAB_TEST_UPGRADE_TARGET").unwrap(),
            root.join("bin/codex"),
        )
        .unwrap();
        let mut command = child_command(&std::env::current_exe().unwrap(), &root, "source");
        command
            .env("ALAB_TEST_EXEC_CUSTODY_MODE", "cli")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped());
        let mut owner = Reap(Some(command.spawn().unwrap()));
        let child = owner.0.as_mut().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let response: crate::upgrade_target::Response = loop {
            let frame = crate::upgrade_admission::read_frame(&mut stdout, deadline).unwrap();
            if let Some(start) = frame.iter().position(|b| *b == b'{') {
                break serde_json::from_slice(&frame[start..]).unwrap();
            }
        };
        assert_eq!(
            response.pid,
            child.id(),
            "the source process must exec without a PID change"
        );
        assert_eq!(response.protocol, crate::upgrade_target::PROTOCOL);
        assert_eq!(response.exec_custody, PROTOCOL);
        assert!(
            response.exec_custody_adopted,
            "the real main hook must own the original lease"
        );
        blocked(&root.join("original.lease"));
        drop(child.stdin.take());
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "real target did not exit after EOF"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        owner.0.take();
        released(&root.join("original.lease"));
    }

    #[test]
    #[ignore = "requires an explicitly built Codex CLI with typed runtime staging"]
    fn real_cli_restores_the_terminal_when_constructor_or_arguments_fail() {
        use std::os::fd::FromRawFd;
        for mode in [
            "runtime-cli-no-hook",
            "runtime-cli-constructor",
            "runtime-cli-arguments",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().canonicalize().unwrap();
            std::fs::create_dir(root.join("bin")).unwrap();
            std::fs::copy(
                if mode == "runtime-cli-no-hook" {
                    std::env::current_exe().unwrap().into_os_string()
                } else {
                    std::env::var_os("ALAB_TEST_UPGRADE_TARGET").unwrap()
                },
                root.join("bin/codex"),
            )
            .unwrap();
            let (mut master, mut slave) = (-1, -1);
            assert_eq!(
                unsafe {
                    libc::openpty(
                        &raw mut master,
                        &raw mut slave,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                },
                0
            );
            let (mut master, slave) =
                unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
            for file in [&master, &slave] {
                assert_eq!(
                    unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) },
                    0
                );
            }
            assert_eq!(
                unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) },
                0
            );
            let mut before = std::mem::MaybeUninit::uninit();
            assert_eq!(
                unsafe { libc::tcgetattr(master.as_raw_fd(), before.as_mut_ptr()) },
                0
            );
            let before = unsafe { before.assume_init() };
            let mut command = child_command(&std::env::current_exe().unwrap(), &root, "source");
            command
                .env("ALAB_TEST_EXEC_CUSTODY_MODE", mode)
                .stdin(slave.try_clone().unwrap())
                .stdout(slave.try_clone().unwrap())
                .stderr(slave.try_clone().unwrap());
            // SAFETY: only this child's private terminal/session are changed.
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY.into(), 0) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            struct Reap(Option<std::process::Child>);
            impl Drop for Reap {
                fn drop(&mut self) {
                    if let Some(child) = &mut self.0 {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }
            }
            let mut child = Reap(Some(command.spawn().unwrap()));
            let deadline = Instant::now() + Duration::from_secs(60);
            let mut output = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                match master.read(&mut buffer) {
                    Ok(size) => output.extend_from_slice(&buffer[..size]),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("PTY read failed: {error}"),
                }
                if let Some(status) = child.0.as_mut().unwrap().try_wait().unwrap() {
                    assert!(
                        !status.success(),
                        "unexpected CLI success: {}",
                        String::from_utf8_lossy(&output)
                    );
                    child.0.take();
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "CLI failure did not finish: {}",
                    String::from_utf8_lossy(&output)
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            loop {
                let mut buffer = [0; 4096];
                match master.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(size) => output.extend_from_slice(&buffer[..size]),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => panic!("could not drain CLI failure: {error}"),
                }
            }
            let text = String::from_utf8_lossy(&output);
            let expected = if mode == "runtime-cli-no-hook" {
                "ALAB_NO_TERMINAL_RECOVERY"
            } else if mode == "runtime-cli-constructor" {
                "managed credential refresh is unavailable"
            } else {
                "--alab-invalid-startup-argument"
            };
            assert!(
                text.contains(expected),
                "wrong failure phase for {mode}: {text}"
            );
            let mut after = std::mem::MaybeUninit::uninit();
            assert_eq!(
                unsafe { libc::tcgetattr(master.as_raw_fd(), after.as_mut_ptr()) },
                0
            );
            let after = unsafe { after.assume_init() };
            if mode == "runtime-cli-no-hook" {
                assert_eq!(
                    after.c_lflag & libc::ICANON,
                    0,
                    "kernel cleanup must not make a missing recovery owner pass"
                );
                assert!(
                    !text.contains("\x1b[?1049l"),
                    "negative control unexpectedly left the alternate screen"
                );
                released(&root.join("original.lease"));
                continue;
            }
            assert_eq!(
                (
                    after.c_iflag,
                    after.c_oflag,
                    after.c_cflag,
                    after.c_lflag & !libc::PENDIN
                ),
                (
                    before.c_iflag,
                    before.c_oflag,
                    before.c_cflag,
                    before.c_lflag & !libc::PENDIN
                ),
                "{mode}: {}",
                String::from_utf8_lossy(&output)
            );
            for reset in [
                "\x1b[?1049l",
                "\x1b[?1000l",
                "\x1b[?2004l",
                "\x1b[?1004l",
                "\x1b[?25h",
            ] {
                assert!(
                    text.contains(reset),
                    "{mode} omitted protocol cleanup {reset:?}: {text}"
                );
            }
            assert_eq!(after.c_cc, before.c_cc);
            assert_eq!(unsafe { libc::cfgetispeed(&after) }, unsafe {
                libc::cfgetispeed(&before)
            });
            assert_eq!(unsafe { libc::cfgetospeed(&after) }, unsafe {
                libc::cfgetospeed(&before)
            });
            released(&root.join("original.lease"));
        }
    }

    #[test]
    fn queued_rights_keep_original_lease_and_uncommitted_drop_releases_it() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("bin")).unwrap();
        std::fs::write(root.join("bin/codex"), b"fixture, never executed").unwrap();
        let offered = received(&root);
        let channel = offered.prepare_exec_custody().unwrap();
        drop(offered);
        blocked(&root.join("original.lease"));
        assert!(
            rustix::io::fcntl_getfd(&channel.0)
                .unwrap()
                .contains(rustix::io::FdFlags::CLOEXEC)
        );
        drop(channel);
        released(&root.join("original.lease"));
    }
}
