// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Kernel connection creation only, not native SDK or authentication grants.
use super::*;
use std::sync::mpsc;

#[test]
fn control_socket_creation_waits_for_raw_exec_exclusion_and_finishes_cloexec() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("control.sock");
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let held = codex_alab_core::native_fd_custody::executing().unwrap();
    let (attempted, attempt) = mpsc::channel();
    let (completed, completion) = mpsc::channel();
    let connection = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            attempted.send(()).unwrap();
            let stream = connect_close_on_exec(&path).await.unwrap();
            // SAFETY: F_GETFD inspects the live stream owned by this scope.
            let flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFD) };
            completed.send(flags).unwrap();
        });
    });
    attempt.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        completion.recv_timeout(Duration::from_millis(50)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    drop(held);
    let flags = completion.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0);
    assert!(listener.accept().is_ok());
    connection.join().unwrap();
}
