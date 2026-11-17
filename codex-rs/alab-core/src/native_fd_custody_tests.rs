// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual ancillary rights and threads only; no SDK or execution permission.
use super::*;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::Duration;

fn queued_rights() -> UnixStream {
    let (sender, receiver) = UnixStream::pair().unwrap();
    let offered = std::fs::File::open("/dev/null").unwrap();
    let mut byte = b'F';
    let mut vector = libc::iovec {
        iov_base: std::ptr::addr_of_mut!(byte).cast(),
        iov_len: 1,
    };
    let mut ancillary = [0_usize; 16];
    // SAFETY: every msghdr pointer below is initialized with live aligned storage.
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut vector;
    message.msg_iovlen = 1;
    message.msg_control = ancillary.as_mut_ptr().cast();
    // SAFETY: storage contains space for three descriptors and the header.
    unsafe {
        message.msg_controllen = libc::CMSG_SPACE(12) as _;
        let header = libc::CMSG_FIRSTHDR(&message);
        assert!(!header.is_null());
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(12) as _;
        for index in 0..3 {
            libc::CMSG_DATA(header)
                .cast::<libc::c_int>()
                .add(index)
                .write(offered.as_raw_fd());
        }
        assert_eq!(libc::sendmsg(sender.as_raw_fd(), &message, 0), 1);
    }
    receiver
}

fn receive_rights(receiver: &UnixStream) -> [OwnedFd; 3] {
    let mut byte = 0_u8;
    let mut vector = libc::iovec {
        iov_base: std::ptr::addr_of_mut!(byte).cast(),
        iov_len: 1,
    };
    let mut ancillary = [0_usize; 16];
    // SAFETY: msghdr points to writable aligned storage of the stated size.
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut vector;
    message.msg_iovlen = 1;
    message.msg_control = ancillary.as_mut_ptr().cast();
    message.msg_controllen = std::mem::size_of_val(&ancillary) as _;
    // No MSG_CMSG_CLOEXEC: this deliberately exercises the actual receipt gap.
    assert_eq!(
        unsafe { libc::recvmsg(receiver.as_raw_fd(), &mut message, 0) },
        1
    );
    assert_eq!(byte, b'F');
    // SAFETY: the kernel initialized the complete checked SCM_RIGHTS payload;
    // each descriptor has exactly one owner created here.
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&message);
        assert!(!header.is_null());
        assert_eq!((*header).cmsg_level, libc::SOL_SOCKET);
        assert_eq!((*header).cmsg_type, libc::SCM_RIGHTS);
        assert_eq!((*header).cmsg_len as usize, libc::CMSG_LEN(12) as usize);
        std::array::from_fn(|index| {
            OwnedFd::from_raw_fd(*libc::CMSG_DATA(header).cast::<libc::c_int>().add(index))
        })
    }
}

fn flags(raw: libc::c_int) -> libc::c_int {
    // SAFETY: F_GETFD only inspects the descriptor number. Owners stay live.
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
    assert!(flags >= 0, "{}", io::Error::last_os_error());
    flags
}

#[test]
fn receipt_excludes_exec_until_every_actual_right_is_cloexec() {
    let receiver = queued_rights();
    let held = receiving().unwrap();
    let rights = receive_rights(&receiver);
    let raw = rights.each_ref().map(AsRawFd::as_raw_fd);
    for descriptor in raw {
        assert_eq!(flags(descriptor) & libc::FD_CLOEXEC, 0);
    }
    let (attempted, attempt) = mpsc::channel();
    let (observed, observation) = mpsc::channel();
    let contender = std::thread::spawn(move || {
        attempted.send(()).unwrap();
        let _exec = executing().unwrap();
        observed.send(raw.map(flags)).unwrap();
    });
    attempt.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        observation.recv_timeout(Duration::from_millis(50)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    for descriptor in raw {
        // SAFETY: each descriptor remains exclusively owned by rights.
        assert_eq!(
            unsafe { libc::fcntl(descriptor, libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
    }
    drop(held);
    let observed = observation.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        observed
            .into_iter()
            .all(|flag| flag & libc::FD_CLOEXEC != 0)
    );
    contender.join().unwrap();
}

#[test]
fn omitted_receipt_guard_exposes_the_real_inheritance_gap() {
    let receiver = queued_rights();
    let rights = receive_rights(&receiver);
    let raw = rights.each_ref().map(AsRawFd::as_raw_fd);
    let observed = std::thread::spawn(move || {
        let _exec = executing().unwrap();
        raw.map(flags)
    })
    .join()
    .unwrap();
    assert!(
        observed
            .into_iter()
            .all(|flag| flag & libc::FD_CLOEXEC == 0)
    );
}

#[test]
fn failed_exec_attempt_releases_exclusion_after_cleanup() {
    fn attempt() -> io::Error {
        let _exec = executing().unwrap();
        let arguments = [c"missing".as_ptr(), std::ptr::null()];
        let environment = [std::ptr::null::<libc::c_char>()];
        // SAFETY: an empty path cannot execute; both pointer arrays are live,
        // NUL terminated, and the failed call cannot replace this test process.
        assert_eq!(
            unsafe { libc::execve(c"".as_ptr(), arguments.as_ptr(), environment.as_ptr()) },
            -1
        );
        io::Error::last_os_error()
    }
    assert_eq!(attempt().raw_os_error(), Some(libc::ENOENT));
    let (send, receive) = mpsc::channel();
    let receipt = std::thread::spawn(move || {
        let _receipt = receiving().unwrap();
        send.send(()).unwrap();
    });
    receive.recv_timeout(Duration::from_secs(5)).unwrap();
    receipt.join().unwrap();
}

#[test]
fn interrupted_critical_section_fails_closed() {
    let exclusion = Mutex::new(());
    std::thread::scope(|scope| {
        let panic = scope.spawn(|| {
            let _held = acquire(&exclusion).unwrap();
            panic!("injected interruption while securing descriptors");
        });
        assert!(panic.join().is_err());
    });
    assert!(acquire(&exclusion).is_err());
}
