// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Receive original executable custody from the native package owner. Neither
//! transport success nor this receipt alone authorizes a process restart.

use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::os::fd::AsFd;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

// A cancelled caller does not cancel a blocking worker. Keep admission serialized
// until that worker has settled its original child and dropped its socket/rights.
static ADMISSION: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// Ask the authenticated native package host for an exact signed build. The caller
/// supplies that host's kernel-observed executable, never an executable found on PATH.
/// Receipt success alone does not authorize a restart.
pub async fn admit(
    native: PathBuf,
    expected: PathBuf,
    build: u64,
    deadline: Instant,
) -> std::io::Result<Received> {
    if !native.is_absolute() || !expected.is_absolute() || build == 0 {
        return Err(invalid(
            "native admission requires absolute paths and an exact build",
        ));
    }
    admit_with(expected, build, deadline, move |socket| {
        let mut command = std::process::Command::new(native);
        command
            .args(["pkg", "admit-executable", "--tool"])
            .arg(format!("codex={build}"))
            .arg("--socket")
            .arg(socket);
        command
    })
    .await
}

async fn admit_with(
    expected: PathBuf,
    build: u64,
    deadline: Instant,
    command: impl FnOnce(&Path) -> std::process::Command + Send + 'static,
) -> std::io::Result<Received> {
    let permit = tokio::time::timeout_at(
        tokio::time::Instant::from_std(deadline),
        ADMISSION.acquire(),
    )
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "native admission deadline"))?
    .map_err(std::io::Error::other)?;
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _entered = runtime.enter();
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "native admission deadline",
            ));
        }
        let directory = tempfile::Builder::new()
            .prefix("codex-upgrade-")
            .tempdir()?;
        let socket = directory.path().join("s");
        let listener = std::os::unix::net::UnixListener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let mut command = command(&socket);
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = Child {
            process: command.spawn()?,
            settled: false,
        };
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    if let Some(status) = child.process.try_wait()? {
                        child.settled = true;
                        return Err(std::io::Error::other(format!(
                            "native admission exited before connecting: {status}"
                        )));
                    }
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "native admission connection deadline",
                        ));
                    }
                    std::thread::sleep(remaining.min(std::time::Duration::from_millis(10)));
                }
                Err(error) => return Err(error),
            }
        };
        stream.set_nonblocking(true)?;
        let received = receive(
            tokio::net::UnixStream::from_std(stream)?,
            child.process.id(),
            &expected,
            build,
            deadline,
        )?;
        loop {
            if let Some(status) = child.process.try_wait()? {
                child.settled = true;
                if !status.success() {
                    return Err(std::io::Error::other(format!(
                        "native executable admission failed: {status}"
                    )));
                }
                received.validate(&expected, build)?;
                return Ok(received);
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "native admission child exit deadline",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    })
    .await
    .map_err(std::io::Error::other)?
}

/// Even when the async caller disappears, the blocking worker owns cleanup. Never
/// leave a helper or the original lease stranded behind an abandoned join handle.
struct Child {
    process: std::process::Child,
    settled: bool,
}

impl Drop for Child {
    fn drop(&mut self) {
        if !self.settled {
            let _ = self.process.kill();
            let _ = self.process.wait();
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub protocol: u32,
    pub program: String,
    pub build: u64,
    pub version: String,
    pub root: PathBuf,
    pub executable: PathBuf,
    pub tree_root: String,
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub modified_seconds: i64,
    pub modified_nanoseconds: i64,
    pub lease_path: PathBuf,
    pub lease_device: u64,
    pub lease_inode: u64,
}

/// Original close-only lease and opened image. Never unlock or relock the lease.
pub struct Received {
    pub(crate) receipt: Receipt,
    pub(crate) lease: File,
    pub(crate) image: File,
}

impl Received {
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }

    /// Run the typed compatibility probe through the exact admitted physical
    /// image while retaining its original native lease throughout the probe.
    /// This blocking operation belongs on the updater's blocking worker.
    pub fn probe(&self, deadline: Instant) -> std::io::Result<crate::upgrade_target::Response> {
        self.validate(&self.receipt.executable, self.receipt.build)?;
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "upgrade target probe deadline",
            ));
        }
        let mut child = Child {
            process: std::process::Command::new(&self.receipt.executable)
                .args(["debug", "upgrade-capabilities"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()?,
            settled: false,
        };
        let mut stdout = child
            .process
            .stdout
            .take()
            .ok_or_else(|| invalid("the upgrade target has no response pipe"))?;
        let response: crate::upgrade_target::Response =
            serde_json::from_slice(&read_frame(&mut stdout, deadline)?)?;
        response.validate(&self.receipt, child.process.id())?;
        loop {
            if let Some(status) = child.process.try_wait()? {
                child.settled = true;
                if !status.success() {
                    return Err(std::io::Error::other(format!(
                        "upgrade target probe failed: {status}"
                    )));
                }
                self.validate(&self.receipt.executable, self.receipt.build)?;
                return Ok(response);
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "upgrade target probe deadline",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Recheck original file identities, not a mutable launch shim.
    pub fn validate(&self, expected: &Path, requested_build: u64) -> std::io::Result<()> {
        let r = &self.receipt;
        if r.protocol != 1
            || r.program != "codex"
            || requested_build == 0
            || r.build != requested_build
            || r.version.is_empty()
            || r.executable != expected
            || !r.root.is_absolute()
            || !r.lease_path.is_absolute()
            || r.executable != r.root.join("bin/codex")
            || r.tree_root.len() != 64
            || !r
                .tree_root
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid(
                "the native receipt does not identify the requested executable",
            ));
        }
        let image = self.image.metadata()?;
        let named = std::fs::symlink_metadata(expected)?;
        if !image.is_file()
            || !named.is_file()
            || (
                image.dev(),
                image.ino(),
                image.len(),
                image.mtime(),
                image.mtime_nsec(),
            ) != (
                r.device,
                r.inode,
                r.size,
                r.modified_seconds,
                r.modified_nanoseconds,
            )
            || (
                named.dev(),
                named.ino(),
                named.len(),
                named.mtime(),
                named.mtime_nsec(),
            ) != (
                r.device,
                r.inode,
                r.size,
                r.modified_seconds,
                r.modified_nanoseconds,
            )
        {
            return Err(invalid("the admitted executable changed"));
        }
        let lease = self.lease.metadata()?;
        let named = std::fs::symlink_metadata(&r.lease_path)?;
        if !lease.is_file()
            || !named.is_file()
            || (lease.dev(), lease.ino()) != (r.lease_device, r.lease_inode)
            || (named.dev(), named.ino()) != (r.lease_device, r.lease_inode)
        {
            return Err(invalid("the original lease file changed"));
        }
        Ok(())
    }
}

/// Called from the blocking admission worker after accepting its original native
/// child. The caller still owns that child and must settle its successful exit.
pub fn receive(
    stream: tokio::net::UnixStream,
    child_pid: u32,
    expected: &Path,
    requested_build: u64,
    deadline: Instant,
) -> std::io::Result<Received> {
    let peer = stream
        .peer_cred()?
        .pid()
        .and_then(|pid| u32::try_from(pid).ok());
    if child_pid == 0 || peer != Some(child_pid) {
        return Err(invalid(
            "the admission socket is not the original native child",
        ));
    }
    let mut stream = stream.into_std()?;
    let received = receive_frame(stream.try_clone()?, expected, requested_build, deadline)?;
    stream.write_all(b"K")?;
    Ok(received)
}

fn receive_frame(
    mut stream: UnixStream,
    expected: &Path,
    requested_build: u64,
    deadline: Instant,
) -> std::io::Result<Received> {
    let [lease, image] = receive_rights(&stream, b"X", Some(deadline))?;
    let bytes = read_frame(&mut stream, deadline)?;
    let receipt = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
    let received = Received {
        receipt,
        lease: File::from(lease),
        image: File::from(image),
    };
    received.validate(expected, requested_build)?;
    Ok(received)
}

pub(crate) fn receive_rights<const N: usize>(
    stream: &UnixStream,
    expected_marker: &[u8],
    deadline: Option<Instant>,
) -> std::io::Result<[std::os::fd::OwnedFd; N]> {
    use rustix::net::RecvAncillaryBuffer;
    use rustix::net::RecvAncillaryMessage;
    use rustix::net::RecvFlags;
    use rustix::net::ReturnFlags;
    use std::io::IoSliceMut;
    // XNU can install up to 512 descriptors before reporting truncation. Reserve
    // all of them so rejected excess rights remain owned and close on refusal.
    let mut storage = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(512))];
    let mut ancillary = RecvAncillaryBuffer::new(&mut storage);
    let mut marker = vec![0; expected_marker.len()];
    #[cfg(target_os = "linux")]
    let flags = RecvFlags::DONTWAIT | RecvFlags::CMSG_CLOEXEC;
    #[cfg(not(target_os = "linux"))]
    let flags = RecvFlags::DONTWAIT;
    let result = loop {
        if let Some(deadline) = deadline {
            readable(stream, deadline)?;
        }
        match rustix::net::recvmsg(
            stream,
            &mut [IoSliceMut::new(&mut marker)],
            &mut ancillary,
            flags,
        ) {
            Err(rustix::io::Errno::AGAIN | rustix::io::Errno::INTR) if deadline.is_some() => {
                continue;
            }
            result => break result?,
        }
    };
    let mut descriptors = Vec::new();
    let mut unknown = false;
    for message in ancillary.drain() {
        match message {
            RecvAncillaryMessage::ScmRights(rights) => descriptors.extend(rights),
            _ => unknown = true,
        }
    }
    if result.bytes != expected_marker.len()
        || marker != expected_marker
        || unknown
        || result.flags.contains(ReturnFlags::CTRUNC)
        || descriptors.len() != N
    {
        return Err(invalid("invalid executable custody descriptors"));
    }
    for descriptor in &descriptors {
        rustix::io::fcntl_setfd(descriptor, rustix::io::FdFlags::CLOEXEC)?;
    }
    descriptors
        .try_into()
        .map_err(|_| invalid("invalid executable custody descriptor count"))
}

pub(crate) fn read_frame(
    stream: &mut (impl Read + AsFd),
    deadline: Instant,
) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        readable(stream, deadline)?;
        let mut byte = [0];
        match stream.read(&mut byte) {
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
            Ok(0) => return Err(invalid("incomplete upgrade frame")),
            Ok(_) => {}
        }
        if byte == *b"\n" {
            break;
        }
        if bytes.len() == 16 * 1024 {
            return Err(invalid("oversized upgrade frame"));
        }
        bytes.push(byte[0]);
    }
    Ok(bytes)
}

fn readable(stream: &impl AsFd, deadline: Instant) -> std::io::Result<()> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "native admission deadline",
            ));
        }
        let timeout = i32::try_from(remaining.as_millis().saturating_add(1)).unwrap_or(i32::MAX);
        let mut descriptor = libc::pollfd {
            fd: stream.as_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll borrows one live descriptor and the writable stack record.
        let result = unsafe { libc::poll(&mut descriptor, 1, timeout) };
        if result > 0 {
            return Ok(());
        }
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
}

fn invalid(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::net::SendAncillaryBuffer;
    use rustix::net::SendAncillaryMessage;
    use rustix::net::SendFlags;
    use std::io::IoSlice;
    use std::io::Write;

    struct Fixture {
        _directory: tempfile::TempDir,
        receipt: Receipt,
        lease: File,
        image: File,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().canonicalize().unwrap();
            std::fs::create_dir(root.join("bin")).unwrap();
            let executable = root.join("bin/codex");
            std::fs::write(&executable, b"fixture image, never executed").unwrap();
            let image = File::open(&executable).unwrap();
            let lease_path = root.join("original.lease");
            let lease = File::create(&lease_path).unwrap();
            let i = image.metadata().unwrap();
            let l = lease.metadata().unwrap();
            Self {
                receipt: Receipt {
                    protocol: 1,
                    program: "codex".into(),
                    build: 7,
                    version: "0.6.0".into(),
                    root,
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
                _directory: directory,
                lease,
                image,
            }
        }

        fn send(&self, stream: &mut UnixStream, extra: bool, complete: bool) {
            Self::send_files(
                &self.receipt,
                &self.lease,
                &self.image,
                stream,
                extra,
                complete,
            );
        }

        fn send_files(
            receipt: &Receipt,
            lease: &File,
            image: &File,
            stream: &mut UnixStream,
            extra: bool,
            complete: bool,
        ) {
            let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(3))];
            let mut ancillary = SendAncillaryBuffer::new(&mut space);
            let mut rights = vec![lease.as_fd(), image.as_fd()];
            if extra {
                rights.push(lease.as_fd());
            }
            assert!(ancillary.push(SendAncillaryMessage::ScmRights(&rights)));
            assert_eq!(
                rustix::net::sendmsg(
                    &*stream,
                    &[IoSlice::new(b"X")],
                    &mut ancillary,
                    SendFlags::empty()
                )
                .unwrap(),
                1
            );
            stream
                .write_all(&serde_json::to_vec(receipt).unwrap())
                .unwrap();
            if complete {
                stream.write_all(b"\n").unwrap();
            }
        }
    }

    // Re-exec only this test to exercise the actual original-child kernel peer,
    // acknowledgement, and child-exit requirement without treating a fixture as
    // a signed package. A normal test run does no child work here.
    #[test]
    fn native_admission_child() {
        let Some(socket) = std::env::var_os("ALAB_TEST_ADMISSION_SOCKET") else {
            return;
        };
        let receipt: Receipt =
            serde_json::from_str(&std::env::var("ALAB_TEST_ADMISSION_RECEIPT").unwrap()).unwrap();
        let lease = File::open(&receipt.lease_path).unwrap();
        let image = File::open(&receipt.executable).unwrap();
        let mut stream = UnixStream::connect(socket).unwrap();
        Fixture::send_files(&receipt, &lease, &image, &mut stream, false, true);
        let mut acknowledgement = [0];
        stream.read_exact(&mut acknowledgement).unwrap();
        assert_eq!(&acknowledgement, b"K");
        if let Some(hold) = std::env::var_os("ALAB_TEST_ADMISSION_HOLD") {
            let hold = PathBuf::from(hold);
            std::fs::write(&hold, b"ready").unwrap();
            while std::fs::read(&hold).unwrap() != b"release" {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        if std::env::var_os("ALAB_TEST_ADMISSION_FAIL").is_some() {
            std::process::exit(7);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_native_child_exiting_before_connecting_is_settled_without_waiting_for_the_connection_deadline()
     {
        let fixture = Fixture::new();
        let error = admit_with(
            fixture.receipt.executable.clone(),
            7,
            Instant::now() + std::time::Duration::from_secs(60),
            |_| {
                let mut command = std::process::Command::new(std::env::current_exe().unwrap());
                command
                    .args([
                        "--exact",
                        "upgrade_admission::tests::native_admission_child",
                        "--nocapture",
                    ])
                    .env_remove("ALAB_TEST_ADMISSION_SOCKET");
                command
            },
        )
        .await
        .err()
        .unwrap();
        assert!(
            error
                .to_string()
                .contains("native admission exited before connecting"),
            "{error}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn original_child_custody_requires_its_acknowledged_successful_exit() {
        for fail in [false, true] {
            let fixture = Fixture::new();
            let receipt = serde_json::to_string(&fixture.receipt).unwrap();
            let result = admit_with(
                fixture.receipt.executable.clone(),
                7,
                Instant::now() + std::time::Duration::from_secs(60),
                move |socket| {
                    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
                    command
                        .args([
                            "--exact",
                            "upgrade_admission::tests::native_admission_child",
                            "--nocapture",
                        ])
                        .env("ALAB_TEST_ADMISSION_SOCKET", socket)
                        .env("ALAB_TEST_ADMISSION_RECEIPT", receipt);
                    if fail {
                        command.env("ALAB_TEST_ADMISSION_FAIL", "1");
                    } else {
                        command.env_remove("ALAB_TEST_ADMISSION_FAIL");
                    }
                    command
                },
            )
            .await;
            if fail {
                assert!(
                    result
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("native executable admission failed")
                );
            } else {
                result
                    .unwrap()
                    .validate(&fixture.receipt.executable, 7)
                    .unwrap();
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires an explicit native host and an installed signed codex build"]
    async fn signed_public_package_through_original_native_child() {
        let native = PathBuf::from(std::env::var_os("ALAB_TEST_ADMISSION_NATIVE").unwrap());
        let expected = PathBuf::from(std::env::var_os("ALAB_TEST_ADMISSION_IMAGE").unwrap());
        let build = std::env::var("ALAB_TEST_ADMISSION_BUILD")
            .unwrap()
            .parse()
            .unwrap();
        let received = admit(
            native,
            expected.clone(),
            build,
            Instant::now() + std::time::Duration::from_secs(60),
        )
        .await
        .unwrap();
        received.validate(&expected, build).unwrap();
        assert_eq!(received.receipt().build, build);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_caller_cannot_release_the_original_childs_admission_slot() {
        struct Release(PathBuf);
        impl Drop for Release {
            fn drop(&mut self) {
                let _ = std::fs::write(&self.0, b"release");
            }
        }
        let fixture = Fixture::new();
        let release = Release(fixture.receipt.root.join("hold"));
        let hold = release.0.clone();
        let receipt = serde_json::to_string(&fixture.receipt).unwrap();
        let expected = fixture.receipt.executable.clone();
        let task = tokio::spawn(admit_with(
            expected.clone(),
            7,
            Instant::now() + std::time::Duration::from_secs(60),
            move |socket| {
                let mut command = std::process::Command::new(std::env::current_exe().unwrap());
                command
                    .args([
                        "--exact",
                        "upgrade_admission::tests::native_admission_child",
                        "--nocapture",
                    ])
                    .env("ALAB_TEST_ADMISSION_SOCKET", socket)
                    .env("ALAB_TEST_ADMISSION_RECEIPT", receipt)
                    .env("ALAB_TEST_ADMISSION_HOLD", hold)
                    .env_remove("ALAB_TEST_ADMISSION_FAIL");
                command
            },
        ));
        tokio::time::timeout(std::time::Duration::from_secs(60), async {
            while !release.0.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        task.abort();
        assert!(task.await.err().unwrap().is_cancelled());
        assert!(
            ADMISSION.try_acquire().is_err(),
            "the detached worker still owns its original child"
        );
        let error = admit_with(expected, 7, Instant::now(), |_| {
            panic!("a duplicate helper must not spawn")
        })
        .await
        .err()
        .unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        drop(release);
        let _settled =
            tokio::time::timeout(std::time::Duration::from_secs(60), ADMISSION.acquire())
                .await
                .unwrap()
                .unwrap();
    }

    #[test]
    fn target_compatibility_is_bound_to_the_admitted_image_and_original_child() {
        let fixture = Fixture::new();
        let response = || crate::upgrade_target::Response {
            protocol: crate::upgrade_target::PROTOCOL,
            exec_custody: crate::upgrade_exec_custody::PROTOCOL,
            exec_custody_adopted: false,
            pid: 42,
            executable: fixture.receipt.executable.clone(),
            device: fixture.receipt.device,
            inode: fixture.receipt.inode,
            product_version: fixture.receipt.version.clone(),
        };
        response().validate(&fixture.receipt, 42).unwrap();
        assert!(response().validate(&fixture.receipt, 0).is_err());
        for field in 0..7 {
            let mut wrong = response();
            match field {
                0 => wrong.protocol = 1,
                1 => wrong.pid = 43,
                2 => wrong.executable = fixture.receipt.root.join("another-image"),
                3 => wrong.device += 1,
                4 => wrong.inode += 1,
                5 => wrong.product_version = "different".into(),
                _ => wrong.exec_custody = 0,
            }
            assert!(wrong.validate(&fixture.receipt, 42).is_err());
        }
    }

    #[test]
    #[ignore = "requires an explicitly built Codex CLI with the typed target command"]
    fn compiled_target_probe_matches_the_retained_image() {
        let mut fixture = Fixture::new();
        std::fs::remove_file(&fixture.receipt.executable).unwrap();
        // A hard link can retain the original vnode's reported executable path
        // on Darwin. Model a package's independent physical image instead.
        std::fs::copy(
            std::env::var_os("ALAB_TEST_UPGRADE_TARGET").unwrap(),
            &fixture.receipt.executable,
        )
        .unwrap();
        fixture.image = File::open(&fixture.receipt.executable).unwrap();
        let metadata = fixture.image.metadata().unwrap();
        fixture.receipt.device = metadata.dev();
        fixture.receipt.inode = metadata.ino();
        fixture.receipt.size = metadata.len();
        fixture.receipt.modified_seconds = metadata.mtime();
        fixture.receipt.modified_nanoseconds = metadata.mtime_nsec();
        fixture.receipt.version = std::env::var("ALAB_TEST_UPGRADE_TARGET_VERSION").unwrap();
        let Fixture {
            _directory,
            receipt,
            lease,
            image,
        } = fixture;
        let received = Received {
            receipt,
            lease,
            image,
        };
        received
            .probe(Instant::now() + std::time::Duration::from_secs(60))
            .unwrap();
    }

    #[test]
    fn received_custody_matches_opened_files_and_detects_later_mutation() {
        let fixture = Fixture::new();
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        fixture.send(&mut sender, false, true);
        drop(sender);
        let received = receive_frame(
            receiver,
            &fixture.receipt.executable,
            7,
            Instant::now() + std::time::Duration::from_secs(60),
        )
        .unwrap();
        assert_eq!(received.receipt().version, "0.6.0");
        assert!(
            rustix::io::fcntl_getfd(&received.lease)
                .unwrap()
                .contains(rustix::io::FdFlags::CLOEXEC)
        );
        assert!(
            rustix::io::fcntl_getfd(&received.image)
                .unwrap()
                .contains(rustix::io::FdFlags::CLOEXEC)
        );
        assert!(received.validate(&fixture.receipt.executable, 8).is_err());
        std::fs::write(&fixture.receipt.executable, b"changed after admission").unwrap();
        assert!(received.validate(&fixture.receipt.executable, 7).is_err());
    }

    #[test]
    fn excess_rights_incomplete_frames_and_expired_requests_refuse() {
        for (extra, complete, expired) in [
            (true, true, false),
            (false, false, false),
            (false, true, true),
        ] {
            let fixture = Fixture::new();
            let (mut sender, receiver) = UnixStream::pair().unwrap();
            fixture.send(&mut sender, extra, complete);
            drop(sender);
            let deadline = if expired {
                Instant::now()
            } else {
                Instant::now() + std::time::Duration::from_secs(60)
            };
            assert!(receive_frame(receiver, &fixture.receipt.executable, 7, deadline).is_err());
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn authenticated_peer_delivers_custody_through_public_receiver() {
        let fixture = Fixture::new();
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        fixture.send(&mut sender, false, true);
        receiver.set_nonblocking(true).unwrap();
        let received = receive(
            tokio::net::UnixStream::from_std(receiver).unwrap(),
            std::process::id(),
            &fixture.receipt.executable,
            7,
            Instant::now() + std::time::Duration::from_secs(60),
        )
        .unwrap();
        assert_eq!(received.receipt().build, 7);
        let mut acknowledgement = [0];
        sender.read_exact(&mut acknowledgement).unwrap();
        assert_eq!(&acknowledgement, b"K");
        received.validate(&fixture.receipt.executable, 7).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_unrelated_peer_cannot_supply_a_native_receipt() {
        let fixture = Fixture::new();
        let (_sender, receiver) = tokio::net::UnixStream::pair().unwrap();
        let error = receive(
            receiver,
            u32::MAX,
            &fixture.receipt.executable,
            7,
            Instant::now() + std::time::Duration::from_secs(60),
        )
        .err()
        .expect("unrelated peer rejected");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }
}
