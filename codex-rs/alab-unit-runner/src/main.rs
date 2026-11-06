// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

// A Cargo runner must not initialize another language runtime before exec: both
// CPython and Rust's ordinary entrypoint change inherited signal dispositions.
// The C entrypoint deliberately skips Rust's runtime initialization, including
// its SIGPIPE policy and standard-FD repair. We use no threads, args iterator,
// buffered standard streams or process::Command; libc::execv preserves Cargo's
// exact PID, parent, session/group, environment, descriptors and signal context.
#![cfg_attr(unix, no_main)]

#[cfg(unix)]
mod runtime;

#[cfg(unix)]
#[unsafe(no_mangle)]
unsafe extern "C" fn main(argc: libc::c_int, argv: *const *const libc::c_char) -> libc::c_int {
    // SAFETY: the C runtime supplies argc readable, NUL-terminated argv strings.
    // runtime::entry copies them before using them and never changes their storage.
    match unsafe { runtime::entry(argc, argv) } {
        Ok(()) => 0,
        Err(error) => {
            let message = format!("alab/check.sh: unit-test attribution refused: {error}\n");
            let _ = runtime::write_descriptor(libc::STDERR_FILENO, message.as_bytes());
            1
        }
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("alab/check.sh: qualified unit-test execution currently requires POSIX exec");
    std::process::exit(1);
}
