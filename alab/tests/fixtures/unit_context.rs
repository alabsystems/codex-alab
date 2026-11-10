// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
// A harness=false library test observes entry context before Rust can normalize it.
#![no_main]

use serde_json::json;
use std::ffi::CStr;
use std::os::fd::RawFd;

#[unsafe(no_mangle)]
unsafe extern "C" fn main(argc: libc::c_int, argv: *const *const libc::c_char) -> libc::c_int {
    // SAFETY: C supplies argc readable NUL-terminated strings.
    let arguments = (0..argc)
        .map(|index| unsafe {
            CStr::from_ptr(*argv.add(index as usize))
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    if arguments.get(1).map(String::as_str) == Some("--inject-fd") {
        // Cargo/frontends may normalize inherited descriptors and signal masks.
        // Install controlled input at the Cargo-runner boundary for both paths;
        // the adapter must preserve it, while Python startup must be detected.
        for (signal, handler) in [
            (libc::SIGHUP, libc::SIG_IGN),
            (libc::SIGUSR2, libc::SIG_IGN),
            (libc::SIGXFSZ, libc::SIG_DFL),
        ] {
            // SAFETY: initialize a complete action for a valid fixture signal.
            let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
            action.sa_sigaction = handler;
            // SAFETY: the initialized action and mask live through each call.
            if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0
                || unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } != 0
            {
                return 14;
            }
        }
        // SAFETY: initialize and populate this process's controlled input mask.
        let mut blocked: libc::sigset_t = unsafe { std::mem::zeroed() };
        if unsafe { libc::sigemptyset(&mut blocked) } != 0
            || unsafe { libc::sigaddset(&mut blocked, libc::SIGUSR1) } != 0
            || unsafe { libc::sigaddset(&mut blocked, libc::SIGUSR2) } != 0
            || unsafe { libc::sigprocmask(libc::SIG_BLOCK, &blocked, std::ptr::null_mut()) } != 0
        {
            return 15;
        }
        let file = match std::env::var("ALAB_CONTEXT_FILE")
            .ok()
            .and_then(|path| std::ffi::CString::new(path).ok())
        {
            Some(path) => path,
            None => return 6,
        };
        // SAFETY: file is NUL-terminated; all descriptor operations use integers.
        let descriptor = unsafe { libc::open(file.as_ptr(), libc::O_RDONLY) };
        if descriptor < 0 {
            return 7;
        }
        // SAFETY: descriptor is owned, and fixture slot43 is deliberately reserved.
        if unsafe { libc::dup2(descriptor, 43) } < 0 {
            return 8;
        }
        if descriptor != 43 {
            // SAFETY: close only this prefix's successfully opened descriptor.
            unsafe { libc::close(descriptor) };
        }
        // SAFETY: the fixture owns slot43; lseek positions it before either path.
        if unsafe { libc::lseek(43, 7, libc::SEEK_SET) } != 7 {
            return 9;
        }
        // Capture this Cargo invocation's environment before the adapter. A
        // fresh Targo delegation endpoint belongs to each invocation, so two
        // independent launches cannot be compared for that changing value.
        let prefix_output = match std::env::var("ALAB_CONTEXT_PREFIX_OUT") {
            Ok(path) => path,
            Err(_) => return 12,
        };
        let environment = std::env::vars().collect::<std::collections::BTreeMap<_, _>>();
        if std::fs::write(prefix_output, json!(environment).to_string()).is_err() {
            return 13;
        }
        let encoded = arguments[2..]
            .iter()
            .filter_map(|argument| std::ffi::CString::new(argument.as_bytes()).ok())
            .collect::<Vec<_>>();
        if encoded.len() != arguments.len() - 2 || encoded.is_empty() {
            return 10;
        }
        let mut pointers = encoded
            .iter()
            .map(|argument| argument.as_ptr())
            .collect::<Vec<_>>();
        pointers.push(std::ptr::null());
        // SAFETY: strings are live NUL-terminated arguments with a trailing NULL.
        unsafe { libc::execv(encoded[0].as_ptr(), pointers.as_ptr()) };
        return 11;
    }
    // SAFETY: zero initialization is valid storage for libc output structures.
    let mut mask: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: a null new mask only queries into valid initialized output storage.
    if unsafe { libc::sigprocmask(0, std::ptr::null(), &mut mask) } != 0 {
        return 2;
    }
    let mut signals = Vec::new();
    for signal in 1..=64 {
        // SAFETY: zero initialization is valid storage for sigaction's output.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: a null new action queries the signal into valid output storage.
        if unsafe { libc::sigaction(signal, std::ptr::null(), &mut action) } == 0 {
            // SAFETY: mask was initialized by successful sigprocmask above.
            let blocked = unsafe { libc::sigismember(&mask, signal) };
            signals.push(json!([
                signal,
                action.sa_sigaction,
                action.sa_flags,
                blocked
            ]));
        }
    }
    let descriptors = (0..64)
        .filter_map(|fd| {
            // SAFETY: F_GETFD only queries the integer descriptor.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            (flags >= 0).then_some(json!([fd, flags]))
        })
        .collect::<Vec<_>>();
    let descriptor: RawFd = std::env::var("ALAB_CONTEXT_FD")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(-1);
    let mut bytes = [0u8; 32];
    // SAFETY: bytes is writable for its length; pread does not change file position.
    let read = unsafe { libc::pread(descriptor, bytes.as_mut_ptr().cast(), bytes.len(), 0) };
    // SAFETY: SEEK_CUR with zero is a read-only file-position query.
    let offset = unsafe { libc::lseek(descriptor, 0, libc::SEEK_CUR) };
    let environment = std::env::vars().collect::<std::collections::BTreeMap<_, _>>();
    let value = json!({"signals": signals, "descriptors": descriptors,
        "pid": std::process::id(), "parent_pid": unsafe { libc::getppid() },
        "session": unsafe { libc::getsid(0) }, "process_group": unsafe { libc::getpgrp() },
        "cwd": std::env::current_dir().ok(), "environment": environment,
        "arguments": arguments, "inherited_fd_offset": offset,
        "inherited_fd_bytes": if read >= 0 { &bytes[..read as usize] } else { &[] }});
    let output = match std::env::var("ALAB_CONTEXT_OUT") {
        Ok(path) => path,
        Err(_) => return 3,
    };
    if std::fs::write(output, value.to_string()).is_err() {
        return 4;
    }
    let result = b"native context probe executed\nrunning 1 test\ntest native::context ... FAILED\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n";
    // SAFETY: result is readable and write borrows it only during this call.
    if unsafe { libc::write(1, result.as_ptr().cast(), result.len()) } != result.len() as isize {
        return 5;
    }
    101
}
