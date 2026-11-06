// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;

type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize)]
struct State {
    mode: String,
    rounds: u64,
    libraries: Vec<Library>,
    copies: Vec<Copy>,
}

#[derive(Deserialize)]
struct Library {
    key: String,
    binary: String,
    cwd: String,
    snapshot: [u64; 4],
}

#[derive(Deserialize)]
struct Copy {
    key: String,
    library_key: String,
    round: u64,
    test: String,
}

fn io(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}

fn read_regular_file(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    const MAX_STATE_BYTES: u64 = 64 * 1024 * 1024;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(io)?;
    let metadata = file.metadata().map_err(io)?;
    require(metadata.is_file(), "unit-test state is not a regular file")?;
    require(
        metadata.len() <= MAX_STATE_BYTES,
        "unit-test state exceeds 64 MiB limit",
    )?;
    let mut bytes = Vec::new();
    file.take(MAX_STATE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    require(
        bytes.len() as u64 <= MAX_STATE_BYTES,
        "unit-test state exceeds 64 MiB limit",
    )?;
    Ok(bytes)
}

fn snapshot(path: &Path) -> Result<[u64; 4]> {
    let metadata = path.metadata().map_err(io)?;
    require(
        metadata.is_file(),
        "unit-test artifact is not a regular file",
    )?;
    let timestamp =
        i128::from(metadata.mtime()) * 1_000_000_000 + i128::from(metadata.mtime_nsec());
    Ok([
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        u64::try_from(timestamp).map_err(io)?,
    ])
}

/// Copy C's argv without initializing Rust's process or argument runtime.
///
/// # Safety
/// `argv` must contain `argc` readable pointers to NUL-terminated byte strings.
pub(super) unsafe fn entry(argc: libc::c_int, argv: *const *const libc::c_char) -> Result<()> {
    require(
        argc > 0 && !argv.is_null(),
        "missing native runner arguments",
    )?;
    let mut arguments = Vec::new();
    for index in 1..argc {
        // SAFETY: the caller guarantees argc readable pointers; index is in range.
        let argument = unsafe { *argv.add(index as usize) };
        require(!argument.is_null(), "null native runner argument")?;
        // SAFETY: the caller guarantees each non-null argument is NUL-terminated.
        arguments.push(OsString::from_vec(
            unsafe { CStr::from_ptr(argument) }.to_bytes().to_vec(),
        ));
    }
    execute(arguments)
}

fn execute(arguments: Vec<OsString>) -> Result<()> {
    require(
        arguments.len() >= 5 && arguments[0] == "--state" && arguments[2] == "--round",
        "expected --state DIR --round NUMBER BINARY [ARGUMENTS]",
    )?;
    let state_dir = Path::new(&arguments[1]);
    let round = arguments[3]
        .to_str()
        .ok_or("non-Unicode execution round")?
        .parse::<u64>()
        .map_err(io)?;
    let launch_binary = &arguments[4];
    let binary = Path::new(launch_binary).canonicalize().map_err(io)?;
    let cwd = std::env::current_dir()
        .map_err(io)?
        .canonicalize()
        .map_err(io)?;
    let state: State =
        serde_json::from_slice(&read_regular_file(&state_dir.join("state.json"))?).map_err(io)?;
    require(
        round < state.rounds,
        "unexpected unit-test comparison round",
    )?;
    let mut found = state
        .libraries
        .iter()
        .filter(|library| Path::new(&library.binary) == binary && Path::new(&library.cwd) == cwd);
    let library = found
        .next()
        .ok_or("Cargo invoked an unexpected artifact or package directory")?;
    require(found.next().is_none(), "ambiguous Cargo unit-test artifact")?;
    require(
        snapshot(&binary)? == library.snapshot,
        "unit-test artifact changed after Cargo preparation",
    )?;
    if let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR") {
        require(
            PathBuf::from(manifest).canonicalize().map_err(io)? == cwd,
            "Cargo's unit-test manifest directory does not match its package",
        )?;
    }
    require(
        state.mode == "record" || state.mode == "compare",
        "unknown unit-test execution mode",
    )?;
    let mut copies = state
        .copies
        .iter()
        .filter(|copy| copy.library_key == library.key && copy.round == round);
    let copy = copies.next();
    require(
        copies.next().is_none(),
        "more than one test copy occupies an execution round",
    )?;
    if state.mode == "compare" {
        require(
            arguments.len() == 5,
            "isolated comparisons cannot replace their original selectors",
        )?;
    }
    let mut helpers = BTreeMap::new();
    for (name, value) in std::env::vars_os() {
        if name.as_bytes().starts_with(b"CARGO_BIN_EXE_") {
            helpers.insert(
                name.into_string().map_err(|_| "non-Unicode helper name")?,
                value.into_string().map_err(|_| "non-Unicode helper path")?,
            );
        }
    }
    // SAFETY: these read-only libc process queries take no pointers or ownership.
    let (parent_pid, process_group, session) =
        unsafe { (libc::getppid(), libc::getpgrp(), libc::getsid(0)) };
    require(session >= 0, "could not read Cargo child's session")?;
    let receipt = json!({
        "library_key": library.key, "binary": binary,
        "launch_binary": launch_binary.to_str().ok_or("non-Unicode Cargo executable")?,
        "cwd": cwd, "pid": std::process::id(), "parent_pid": parent_pid,
        "process_group": process_group, "session": session, "mode": state.mode,
        "round": round, "copy_key": copy.map(|copy| &copy.key), "helpers": helpers,
    });
    {
        let path = state_dir.join(format!("library-{}-{round}.json", library.key));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(io)?;
        serde_json::to_writer(&mut file, &receipt).map_err(io)?;
        file.write_all(b"\n").map_err(io)?;
        file.flush().map_err(io)?;
    }
    let selector;
    let child_arguments: Vec<&std::ffi::OsStr> = if state.mode == "record" {
        arguments[4..].iter().map(OsString::as_os_str).collect()
    } else if let Some(copy) = copy {
        require(
            !copy.test.is_empty()
                && !copy.test.starts_with('-')
                && !copy.test.chars().any(char::is_whitespace),
            "invalid unit-test selector",
        )?;
        selector = OsString::from(&copy.test);
        vec![launch_binary, std::ffi::OsStr::new("--exact"), &selector]
    } else {
        return Ok(());
    };
    let marker = if state.mode == "record" {
        &library.key
    } else {
        &copy.ok_or("missing selected unit-test copy")?.key
    };
    write_descriptor(
        libc::STDOUT_FILENO,
        format!("ALAB_UNIT {marker}\n").as_bytes(),
    )?;
    let executable = CString::new(launch_binary.as_bytes()).map_err(io)?;
    let encoded = child_arguments
        .iter()
        .map(|argument| CString::new(argument.as_bytes()).map_err(io))
        .collect::<Result<Vec<_>>>()?;
    let mut pointers = encoded
        .iter()
        .map(|argument| argument.as_ptr())
        .collect::<Vec<_>>();
    pointers.push(std::ptr::null());
    // SAFETY: both the executable and each argument are live NUL-terminated
    // strings, and pointers ends in NULL. execv retains the inherited environment
    // and never returns on success. No Rust Command signal/descriptor edits occur.
    unsafe { libc::execv(executable.as_ptr(), pointers.as_ptr()) };
    Err(format!(
        "unit-test exec failed: {}",
        std::io::Error::last_os_error()
    ))
}

pub(super) fn write_descriptor(descriptor: libc::c_int, mut bytes: &[u8]) -> Result<()> {
    while !bytes.is_empty() {
        // SAFETY: bytes is readable for its length; write borrows it synchronously.
        let count = unsafe { libc::write(descriptor, bytes.as_ptr().cast(), bytes.len()) };
        if count < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(io(error));
        }
        require(count > 0, "zero-length write of unit-test receipt marker")?;
        bytes = &bytes[usize::try_from(count).map_err(io)?..];
    }
    Ok(())
}
