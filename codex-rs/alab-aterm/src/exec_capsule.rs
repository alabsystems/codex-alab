// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Nonpersistent state for a same-process native account replacement.
//! The descriptor is close-on-exec until the stopped runtime is ready to enter
//! its replacement. A capsule carries state, never execution authority.

use crate::exec_grant::Grant;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;
use std::ffi::CString;
use std::fs::File;
use std::io;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::fs::MetadataExt;

pub const ENV: &str = "ATERM_CODEX_EXEC_CAPSULE";
const LIMIT: usize = 256 * 1024 * 1024;

fn invalid() -> io::Error {
    io::Error::other("invalid native exec memory capsule")
}

/// Owns the anonymous shared-memory object until replacement or cancellation.
pub struct Capsule {
    file: File,
    reference: String,
}

fn close_on_exec(file: &File, enabled: bool) -> io::Result<()> {
    // SAFETY: fcntl only changes flags on this live, owned descriptor.
    let result = unsafe {
        libc::fcntl(
            file.as_raw_fd(),
            libc::F_SETFD,
            if enabled { libc::FD_CLOEXEC } else { 0 },
        )
    };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn anonymous_memory() -> io::Result<File> {
    let mut nonce = [0_u8; 12];
    File::open("/dev/urandom")?.read_exact(&mut nonce)?;
    let name = CString::new(format!("/cx{:x}", Sha256::digest(nonce))).map_err(|_| invalid())?;
    // Darwin limits POSIX shared-memory names to 31 bytes, including the NUL.
    let name = CString::new(&name.as_bytes()[..27]).map_err(|_| invalid())?;
    // SAFETY: name is NUL-terminated. O_EXCL prevents opening anyone else's
    // object; POSIX shm_open creates its descriptor with FD_CLOEXEC set.
    let fd = unsafe {
        libc::shm_open(
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )
    };
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful shm_open returned a newly owned descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    // Unlink before putting any conversation bytes in the object. The owned
    // descriptor keeps it alive; closing the last descriptor destroys it.
    // SAFETY: name remains a valid NUL-terminated string.
    if unsafe { libc::shm_unlink(name.as_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    close_on_exec(&file, true)?;
    Ok(file)
}

struct Mapping {
    address: *mut libc::c_void,
    size: usize,
}

impl Mapping {
    fn new(file: &File, size: usize, writable: bool) -> io::Result<Self> {
        if size == 0 || size > LIMIT {
            return Err(invalid());
        }
        let protection = libc::PROT_READ | if writable { libc::PROT_WRITE } else { 0 };
        // SAFETY: mmap retains the shared-memory object independently of the
        // descriptor. The returned mapping is used only for its exact length.
        let address = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                protection,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if address == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { address, size })
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: this instance owns exactly this successful mmap allocation.
        unsafe { libc::munmap(self.address, self.size) };
    }
}

impl Capsule {
    pub fn new(grant: &Grant, state: Value) -> io::Result<Self> {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "pid": std::process::id(),
            "id": grant.ticket.id,
            "thread": grant.origin.thread,
            "turn": grant.origin.turn,
            "state": state,
        }))
        .map_err(|_| invalid())?;
        if bytes.len() > LIMIT {
            return Err(invalid());
        }
        let file = anonymous_memory()?;
        file.set_len(bytes.len() as u64)?;
        let mapping = Mapping::new(&file, bytes.len(), true)?;
        // SAFETY: the writable mapping covers bytes.len() bytes, and the
        // source is a separate live allocation of exactly that length.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), mapping.address.cast(), bytes.len())
        };
        drop(mapping);
        let reference = format!(
            "{}.{}.{}.{}.{:x}",
            std::process::id(),
            file.as_raw_fd(),
            grant.ticket.id,
            bytes.len(),
            Sha256::digest(&bytes),
        );
        Ok(Self { file, reference })
    }

    pub fn reference(&self) -> &str {
        &self.reference
    }

    /// Only call after all old runtime writers and child launches have stopped.
    pub(crate) fn inherit(&self) -> io::Result<()> {
        close_on_exec(&self.file, false)
    }

    pub(crate) fn stop_inheriting(&self) -> io::Result<()> {
        close_on_exec(&self.file, true)
    }
}

/// Consume an inherited capsule after independently validating native admission.
/// Invalid references never close a descriptor supplied by the caller.
///
/// # Safety
/// The descriptor named by `reference` must have crossed exec with no remaining
/// Rust owner. The caller must exclusively own it during this call. In-process
/// callers must first relinquish its previous owner; an arbitrary environment
/// value alone is not evidence that the descriptor is unowned.
pub unsafe fn load(grant: &Grant, reference: &str) -> io::Result<Value> {
    let parts: Vec<_> = reference.split('.').collect();
    if parts.len() != 5
        || parts[0].parse::<u32>().ok() != Some(std::process::id())
        || parts[2] != grant.ticket.id
        || parts[4].len() != 64
    {
        return Err(io::Error::other("exec capsule process or ticket mismatch"));
    }
    let fd = parts[1].parse::<i32>().map_err(|_| invalid())?;
    let size = parts[3].parse::<usize>().map_err(|_| invalid())?;
    if fd < 3 || size > LIMIT {
        return Err(io::Error::other(
            "exec capsule descriptor or size is invalid",
        ));
    }
    // SAFETY: duplicating a descriptor does not assume ownership of the input.
    let copied = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
    if copied == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fcntl returned a fresh descriptor owned by this function.
    let file = unsafe { File::from_raw_fd(copied) };
    let metadata = file.metadata()?;
    // Darwin reports the allocated page-rounded length of a shared-memory
    // object, while Linux reports its requested length. Only the authenticated
    // payload bytes are mapped and hashed; padding never becomes state.
    // SAFETY: sysconf reads a process-independent system constant.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let page_size = u64::try_from(page_size)
        .ok()
        .filter(|size| *size > 0)
        .ok_or_else(invalid)?;
    let rounded_size = (size as u64).div_ceil(page_size) * page_size;
    // SAFETY: geteuid has no arguments or side effects.
    if metadata.len() < size as u64
        || metadata.len() > rounded_size
        || metadata.mode() & 0o077 != 0
        || metadata.uid() != unsafe { libc::geteuid() }
    {
        return Err(io::Error::other("exec capsule memory metadata mismatch"));
    }
    let mapping = Mapping::new(&file, size, false)?;
    // SAFETY: the read-only mapping covers size bytes and remains owned until
    // the copied Vec has been constructed.
    let bytes = unsafe { std::slice::from_raw_parts(mapping.address.cast::<u8>(), size) }.to_vec();
    drop(mapping);
    if format!("{:x}", Sha256::digest(&bytes)) != parts[4] {
        return Err(io::Error::other("exec capsule content digest mismatch"));
    }
    let mut value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if value.as_object().map(serde_json::Map::len) != Some(6)
        || value["version"] != 1
        || value["pid"].as_u64() != Some(u64::from(std::process::id()))
        || value["id"].as_str() != Some(&grant.ticket.id)
        || value["thread"].as_str() != Some(&grant.origin.thread)
        || value["turn"].as_str() != Some(&grant.origin.turn)
    {
        return Err(io::Error::other("exec capsule identity envelope mismatch"));
    }
    // This descriptor was retained across exec, so no Rust owner exists in the
    // replacement. Close it only after all identity and content checks pass.
    // SAFETY: native admission and the capsule envelope validated this inherited
    // descriptor; the independent duplicate remains owned above until return.
    drop(unsafe { File::from_raw_fd(fd) });
    Ok(value["state"].take())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec_grant::Stage;
    use crate::exec_grant::Ticket;
    use crate::renewal::Identity;
    use crate::rotation::Origin;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    fn grant() -> Grant {
        Grant {
            ticket: Ticket {
                id: "a".repeat(32),
                capability: "b".repeat(64),
            },
            stage: Stage::Loaded,
            origin: Origin {
                thread: "original-thread".into(),
                turn: "original-turn".into(),
                identity: Identity {
                    account: "source".into(),
                    user: "user".into(),
                },
            },
            destination: Identity {
                account: "destination".into(),
                user: "user".into(),
            },
            auth_path: "/unused/auth.json".into(),
            checkpoint: Value::Null,
            deadline_us: 1,
        }
    }

    fn descriptor_flags(fd: i32) -> i32 {
        // SAFETY: F_GETFD reads descriptor flags and does not take ownership.
        unsafe { libc::fcntl(fd, libc::F_GETFD) }
    }

    const CAPSULE_CHILD: &str = "exec_capsule::tests::capsule_exec_child";

    fn run_capsule_child(phase: &str) {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                CAPSULE_CHILD,
                "--ignored",
                "--nocapture",
                "--test-threads=1",
                "--color",
                "never",
            ])
            .env("ALAB_CAPSULE_TEST_PHASE", phase)
            .env("RUST_TEST_THREADS", "1")
            .env_remove(ENV)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let failure = loop {
            match child.try_wait() {
                Ok(Some(_)) => break None,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Ok(None) => break Some("capsule child timed out".to_owned()),
                Err(error) => break Some(format!("capsule child wait failed: {error}")),
            }
        };
        if let Some(failure) = failure {
            // The child has not been reaped; retain ownership through stopping
            // and waiting. The create phase execs in place, with no grandchild.
            let stopped = child.kill();
            let output = child.wait_with_output();
            panic!("{failure}: stop={stopped:?}, output={output:?}");
        }
        // try_wait established the terminal status. Never signal after reap.
        let result = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(
            result.status.success() && stdout.contains(&format!("test {CAPSULE_CHILD} ... ok")),
            "{}{}",
            stdout,
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    fn rejected_identity_does_not_consume_descriptor() {
        // Descriptor numbers may be reused immediately by parallel tests.
        // Keep this complete ownership sequence in its own exact child.
        run_capsule_child("reject");
    }

    fn rejected_identity_sequence() {
        let original = grant();
        let capsule = Capsule::new(&original, serde_json::json!({"private": "fixture"})).unwrap();
        let fd = capsule.file.as_raw_fd();
        assert_ne!(descriptor_flags(fd) & libc::FD_CLOEXEC, 0);
        capsule.inherit().unwrap();
        assert_eq!(descriptor_flags(fd) & libc::FD_CLOEXEC, 0);
        capsule.stop_inheriting().unwrap();
        assert_ne!(descriptor_flags(fd) & libc::FD_CLOEXEC, 0);
        let reference = capsule.reference().to_owned();
        // Model the ownership transfer performed by exec without replacing
        // this test process. No other object owns or uses this descriptor.
        std::mem::forget(capsule);
        let mut wrong = grant();
        wrong.origin.turn = "different-turn".into();
        // SAFETY: the previous owner was relinquished above.
        assert!(unsafe { load(&wrong, &reference) }.is_err());
        assert_ne!(descriptor_flags(fd), -1);
        let altered = format!("{reference}0");
        // SAFETY: rejected loads retain the exclusively owned descriptor.
        assert!(unsafe { load(&original, &altered) }.is_err());
        assert_ne!(descriptor_flags(fd), -1);
        // SAFETY: neither rejected call consumed the unowned descriptor.
        assert!(unsafe { load(&original, &reference) }.is_ok());
        assert_eq!(descriptor_flags(fd), -1);
    }

    #[test]
    fn state_survives_real_exec_and_is_consumed_once() {
        run_capsule_child("create");
    }

    #[test]
    #[ignore = "subprocess helper invoked by exec capsule tests"]
    fn capsule_exec_child() {
        let phase = std::env::var("ALAB_CAPSULE_TEST_PHASE");
        if phase.as_deref() == Ok("reject") {
            rejected_identity_sequence();
            return;
        }
        let state = serde_json::json!({
            "turn": "original-turn",
            "history": "private fixture ".repeat(80_000),
            "completed_tools": ["must not replay"],
        });
        let grant = grant();
        match phase.as_deref() {
            Ok("create") => {
                let capsule = Capsule::new(&grant, state).unwrap();
                capsule.inherit().unwrap();
                let error = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "exec_capsule::tests::capsule_exec_child",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("ALAB_CAPSULE_TEST_PHASE", "restore")
                    .env(ENV, capsule.reference())
                    .exec();
                panic!("capsule test exec failed: {error}");
            }
            Ok("restore") => {
                let reference = std::env::var(ENV).unwrap();
                // SAFETY: this exact descriptor crossed the exec above and
                // has no Rust owner in the replacement image.
                assert_eq!(unsafe { load(&grant, &reference) }.unwrap(), state);
                // The first load consumed the descriptor; check the kernel
                // state rather than claiming ownership a second time.
                let fd = reference.split('.').nth(1).unwrap().parse().unwrap();
                assert_eq!(descriptor_flags(fd), -1);
            }
            other => panic!("unexpected capsule test phase: {other:?}"),
        }
    }
}
