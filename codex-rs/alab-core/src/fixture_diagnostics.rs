// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Bounded failure evidence, never test qualification or an alternate fixture root.

use serde::Serialize;
use std::collections::BTreeSet;
use std::ffi::CStr;
use std::ffi::CString;
use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

/// Stable basename for the runner's diagnostics-only artifact index.
pub const DIAGNOSTIC_MANIFEST_FILE: &str = "manifest.json";

/// The caller's observed native cleanup boundary; neither value certifies a snapshot.
/// ShutdownReturned means only that the native shutdown API returned success.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticProducerState {
    ShutdownReturned,
    Incomplete,
}

#[derive(Clone, Copy, Serialize)]
struct Limits {
    total_bytes: u64,
    manifest_bytes: usize,
    files: usize,
    depth: usize,
    entries: usize,
}

const LIMITS: Limits = Limits {
    total_bytes: 32 * 1024 * 1024,
    manifest_bytes: 1024 * 1024,
    files: 512,
    depth: 12,
    entries: 4096,
};
const ROOTS: usize = 8;
const PATH_BYTES: usize = 4096;
const MANIFEST_HEADROOM: usize = 4096;

#[derive(Serialize)]
struct Entry {
    root: String,
    /// Hex encodes the complete relative Unix path, preserving non-UTF8 bytes.
    path_hex: String,
    result: &'static str,
    reason: Option<&'static str>,
    errno: Option<i32>,
    bytes: u64,
    truncated: bool,
}

#[derive(Serialize)]
struct Manifest {
    version: u32,
    declared_producer_state: DiagnosticProducerState,
    producer_state: DiagnosticProducerState,
    limits: Limits,
    visited_entries: usize,
    file_attempts: usize,
    copied_files: usize,
    copied_bytes: u64,
    read_bytes: u64,
    omitted: usize,
    errors: usize,
    stopped: Option<&'static str>,
    entries: Vec<Entry>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity {
    device: libc::dev_t,
    inode: libc::ino_t,
}

fn identity(stat: &libc::stat) -> Identity {
    Identity {
        device: stat.st_dev,
        inode: stat.st_ino,
    }
}

fn stat(file: &File) -> io::Result<libc::stat> {
    let mut result = std::mem::MaybeUninit::uninit();
    // SAFETY: fstat initializes the pointed-to stat on success; file owns its fd.
    if unsafe { libc::fstat(file.as_raw_fd(), result.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fstat initialized every field.
    Ok(unsafe { result.assume_init() })
}

fn c_name(name: &[u8]) -> io::Result<CString> {
    CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"))
}

fn open_at(parent: &File, name: &[u8], flags: i32, mode: libc::mode_t) -> io::Result<File> {
    let name = c_name(name)?;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    let mode = libc::c_uint::from(mode);
    // SAFETY: the name is terminated and parent owns its anchored directory fd.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            mode,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new exclusively owned fd.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn directory_at(parent: &File, name: &[u8]) -> io::Result<File> {
    open_at(parent, name, libc::O_RDONLY | libc::O_DIRECTORY, 0)
}

/// Open every component without following a link, including ancestors of the leaf.
fn directory(path: &Path) -> io::Result<File> {
    if !path.is_absolute() || path.as_os_str().as_bytes().len() > PATH_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "root path must be absolute and bounded",
        ));
    }
    let anchor = if path.is_absolute() { c"/" } else { c"." };
    // SAFETY: the constant is terminated; open returns a fresh fd or an error.
    let fd = unsafe {
        libc::open(
            anchor.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the new fd is owned only here.
    let mut current = unsafe { File::from_raw_fd(fd) };
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => current = directory_at(&current, name.as_bytes())?,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "parent path component refused",
                ));
            }
        }
    }
    Ok(current)
}

fn mkdir(parent: &File, name: &[u8]) -> io::Result<File> {
    let name_c = c_name(name)?;
    // SAFETY: both the directory fd and terminated name remain valid during mkdirat.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name_c.as_ptr(), 0o700) } != 0 {
        return Err(io::Error::last_os_error());
    }
    directory_at(parent, name)
}

/// An ancestor walk uses only actual parent fds, never names from a source tree.
fn below(file: &File, ancestor: Identity) -> io::Result<bool> {
    let mut current = directory_at(file, b".")?;
    for _ in 0..PATH_BYTES {
        let here = identity(&stat(&current)?);
        if here == ancestor {
            return Ok(true);
        }
        let parent = directory_at(&current, b"..")?;
        if identity(&stat(&parent)?) == here {
            return Ok(false);
        }
        current = parent;
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "directory ancestry limit",
    ))
}

struct DirectoryEntries(*mut libc::DIR);

impl DirectoryEntries {
    fn new(file: &File) -> io::Result<Self> {
        // SAFETY: fcntl duplicates the valid fd without borrowing its ownership.
        let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fdopendir takes ownership of this duplicate on success.
        let stream = unsafe { libc::fdopendir(fd) };
        if stream.is_null() {
            let error = io::Error::last_os_error();
            // SAFETY: failed fdopendir did not consume the duplicate.
            unsafe { libc::close(fd) };
            return Err(error);
        }
        Ok(Self(stream))
    }

    fn next(&mut self) -> io::Result<Option<Vec<u8>>> {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        // SAFETY: libc returns this thread's errno slot.
        let errno = unsafe { libc::__errno_location() };
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "dragonfly"
        ))]
        // SAFETY: libc returns this thread's errno slot.
        let errno = unsafe { libc::__error() };
        #[cfg(any(target_os = "netbsd", target_os = "openbsd"))]
        // SAFETY: libc returns this thread's errno slot.
        let errno = unsafe { libc::__errno() };
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "netbsd",
            target_os = "openbsd"
        )))]
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory enumeration unavailable",
        ));
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "netbsd",
            target_os = "openbsd"
        ))]
        {
            // SAFETY: the thread-local slot and owned DIR remain valid; copy the name
            // before another readdir can invalidate its borrowed storage.
            unsafe {
                *errno = 0;
                let entry = libc::readdir(self.0);
                if entry.is_null() {
                    return if *errno == 0 {
                        Ok(None)
                    } else {
                        Err(io::Error::last_os_error())
                    };
                }
                Ok(Some(
                    CStr::from_ptr((*entry).d_name.as_ptr()).to_bytes().to_vec(),
                ))
            }
        }
    }
}

impl Drop for DirectoryEntries {
    fn drop(&mut self) {
        // SAFETY: this object uniquely owns its fdopendir stream and duplicate fd.
        unsafe { libc::closedir(self.0) };
    }
}

#[derive(Default)]
struct ReadObserver {
    #[cfg(test)]
    after_read: Option<Box<dyn FnMut()>>,
}

impl ReadObserver {
    fn on_read(&mut self) {
        #[cfg(test)]
        if let Some(hook) = &mut self.after_read {
            hook();
        }
    }
}

struct Export {
    manifest: Manifest,
    detail_bytes: usize,
    leaf: Identity,
    output: Identity,
    observer: ReadObserver,
}

impl Export {
    fn stop(&mut self, reason: &'static str) {
        self.manifest.stopped.get_or_insert(reason);
    }

    fn visit(&mut self) -> bool {
        if self.manifest.stopped.is_some() {
            return false;
        }
        if self.manifest.visited_entries == self.manifest.limits.entries {
            self.stop("entry_limit");
            return false;
        }
        self.manifest.visited_entries += 1;
        true
    }

    fn record(
        &mut self,
        alias: &str,
        path: &[u8],
        result: &'static str,
        reason: Option<&'static str>,
        error: Option<&io::Error>,
        transfer: (u64, bool),
    ) {
        let entry = Entry {
            root: alias.to_owned(),
            path_hex: hex(path),
            result,
            reason,
            errno: error.and_then(io::Error::raw_os_error),
            bytes: transfer.0,
            truncated: transfer.1,
        };
        let size =
            serde_json::to_vec(&entry).map_or(usize::MAX, |value| value.len().saturating_add(1));
        if self
            .detail_bytes
            .saturating_add(size)
            .saturating_add(MANIFEST_HEADROOM)
            > self.manifest.limits.manifest_bytes
        {
            self.stop("manifest_limit");
            return;
        }
        self.detail_bytes += size;
        if result == "omitted" {
            self.manifest.omitted += 1;
        }
        if result == "error" {
            self.manifest.errors += 1;
        }
        self.manifest.entries.push(entry);
    }

    fn walk(&mut self, alias: &str, source: &File, destination: &File, path: &[u8], depth: usize) {
        let mut entries = match DirectoryEntries::new(source) {
            Ok(entries) => entries,
            Err(error) => {
                self.record(
                    alias,
                    path,
                    "error",
                    Some("read_directory"),
                    Some(&error),
                    (0, false),
                );
                return;
            }
        };
        while self.manifest.stopped.is_none() {
            let name = match entries.next() {
                Ok(Some(name)) => name,
                Ok(None) => break,
                Err(error) => {
                    if self.visit() {
                        self.record(
                            alias,
                            path,
                            "error",
                            Some("read_directory"),
                            Some(&error),
                            (0, false),
                        );
                    }
                    break;
                }
            };
            if !self.visit() {
                break;
            }
            if name == b"." || name == b".." {
                continue;
            }
            if name.contains(&b'/') || name.is_empty() {
                self.stop("invalid_directory_entry");
                break;
            }
            let mut relative = path.to_vec();
            if !relative.is_empty() {
                relative.push(b'/');
            }
            relative.extend_from_slice(&name);
            // Reserve enough metadata before creating any destination entry.
            if self
                .detail_bytes
                .saturating_add(relative.len().saturating_mul(2))
                .saturating_add(512 + MANIFEST_HEADROOM)
                > self.manifest.limits.manifest_bytes
            {
                self.stop("manifest_limit");
                break;
            }
            let name_c = match c_name(&name) {
                Ok(name) => name,
                Err(_) => {
                    self.stop("invalid_directory_entry");
                    break;
                }
            };
            let mut expected = std::mem::MaybeUninit::uninit();
            // SAFETY: fstatat writes stat on success and never follows the final link.
            if unsafe {
                libc::fstatat(
                    source.as_raw_fd(),
                    name_c.as_ptr(),
                    expected.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } != 0
            {
                let error = io::Error::last_os_error();
                self.record(
                    alias,
                    &relative,
                    "error",
                    Some("stat_entry"),
                    Some(&error),
                    (0, false),
                );
                continue;
            }
            // SAFETY: successful fstatat initialized the stat.
            let expected = unsafe { expected.assume_init() };
            let kind = expected.st_mode & libc::S_IFMT;
            if kind == libc::S_IFDIR {
                if depth == self.manifest.limits.depth {
                    self.record(
                        alias,
                        &relative,
                        "omitted",
                        Some("depth_limit"),
                        None,
                        (0, false),
                    );
                    continue;
                }
                let child = match directory_at(source, &name) {
                    Ok(child) => child,
                    Err(error) => {
                        self.record(
                            alias,
                            &relative,
                            "error",
                            Some("open_directory"),
                            Some(&error),
                            (0, false),
                        );
                        continue;
                    }
                };
                let actual = match stat(&child) {
                    Ok(stat) => identity(&stat),
                    Err(error) => {
                        self.record(
                            alias,
                            &relative,
                            "error",
                            Some("stat_directory"),
                            Some(&error),
                            (0, false),
                        );
                        continue;
                    }
                };
                if actual != identity(&expected) || actual == self.leaf || actual == self.output {
                    self.record(
                        alias,
                        &relative,
                        "omitted",
                        Some("changed_or_destination_directory"),
                        None,
                        (0, false),
                    );
                    continue;
                }
                match mkdir(destination, &name) {
                    Ok(target) => {
                        self.record(alias, &relative, "directory", None, None, (0, false));
                        self.walk(alias, &child, &target, &relative, depth + 1);
                    }
                    Err(error) => self.record(
                        alias,
                        &relative,
                        "error",
                        Some("create_directory"),
                        Some(&error),
                        (0, false),
                    ),
                }
            } else if kind == libc::S_IFREG {
                self.copy(alias, source, destination, &name, &relative, &expected);
            } else {
                self.record(
                    alias,
                    &relative,
                    "omitted",
                    Some("not_regular_file_or_directory"),
                    None,
                    (0, false),
                );
            }
        }
    }

    fn copy(
        &mut self,
        alias: &str,
        source: &File,
        destination: &File,
        name: &[u8],
        path: &[u8],
        expected: &libc::stat,
    ) {
        if self.manifest.file_attempts == self.manifest.limits.files {
            self.stop("file_limit");
            return;
        }
        self.manifest.file_attempts += 1;
        let mut input = match open_at(source, name, libc::O_RDONLY | libc::O_NONBLOCK, 0) {
            Ok(file) => file,
            Err(error) => {
                self.record(
                    alias,
                    path,
                    "error",
                    Some("open_file"),
                    Some(&error),
                    (0, false),
                );
                return;
            }
        };
        let before = match stat(&input) {
            Ok(stat) => stat,
            Err(error) => {
                self.record(
                    alias,
                    path,
                    "error",
                    Some("stat_file"),
                    Some(&error),
                    (0, false),
                );
                return;
            }
        };
        if before.st_mode & libc::S_IFMT != libc::S_IFREG || identity(&before) != identity(expected)
        {
            self.record(
                alias,
                path,
                "omitted",
                Some("changed_before_open"),
                None,
                (0, false),
            );
            return;
        }
        let mut output = match open_at(
            destination,
            name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        ) {
            Ok(file) => file,
            Err(error) => {
                self.record(
                    alias,
                    path,
                    "error",
                    Some("create_file"),
                    Some(&error),
                    (0, false),
                );
                return;
            }
        };
        let fingerprint = |file: &File| {
            file.metadata().map(|value| {
                (
                    value.len(),
                    value.mtime(),
                    value.mtime_nsec(),
                    value.ctime(),
                    value.ctime_nsec(),
                )
            })
        };
        let initial = fingerprint(&input);
        let cap = self
            .manifest
            .limits
            .total_bytes
            .saturating_sub(self.manifest.limits.manifest_bytes as u64);
        let mut copied = 0;
        let mut eof = false;
        let mut buffer = [0_u8; 8192];
        let copied_result = (|| -> io::Result<()> {
            while self.manifest.read_bytes < cap {
                let count =
                    usize::try_from((cap - self.manifest.read_bytes).min(buffer.len() as u64))
                        .unwrap_or(buffer.len());
                let count = input.read(&mut buffer[..count])?;
                if count == 0 {
                    eof = true;
                    break;
                }
                // Charge actual reads even when a later destination write fails.
                self.manifest.read_bytes += count as u64;
                self.observer.on_read();
                let mut written = 0;
                while written < count {
                    let progress = output.write(&buffer[written..count])?;
                    if progress == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "diagnostic write made no progress",
                        ));
                    }
                    written += progress;
                    copied += progress as u64;
                    self.manifest.copied_bytes += progress as u64;
                }
            }
            output.sync_all()
        })();
        if let Err(error) = copied_result {
            self.record(
                alias,
                path,
                "error",
                Some("copy_file"),
                Some(&error),
                (copied, true),
            );
            return;
        }
        let changed = match (initial, fingerprint(&input)) {
            (Ok(before), Ok(after)) => before != after,
            _ => true,
        };
        if changed {
            self.manifest.producer_state = DiagnosticProducerState::Incomplete;
        }
        self.manifest.copied_files += 1;
        self.record(
            alias,
            path,
            "copied",
            changed.then_some("changed_during_copy"),
            None,
            (copied, !eof),
        );
        if !eof {
            self.stop("byte_limit");
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(DIGITS[usize::from(byte >> 4)] as char);
        encoded.push(DIGITS[usize::from(byte & 15)] as char);
    }
    encoded
}

fn validate(roots: &[(&str, &Path)], limits: Limits) -> io::Result<()> {
    let mut aliases = BTreeSet::new();
    if roots.len() > ROOTS
        || limits.manifest_bytes < MANIFEST_HEADROOM
        || limits.total_bytes < limits.manifest_bytes as u64
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid diagnostic limits or root count",
        ));
    }
    for (alias, _) in roots {
        if alias.is_empty()
            || alias.len() > 64
            || !alias
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            || !aliases.insert(*alias)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid or duplicate diagnostic root alias",
            ));
        }
    }
    Ok(())
}

/// Export only when the contained native runner assigned its exact diagnostic leaf.
/// No caller-selected destination, link traversal, overwrite, or proof claim is accepted.
/// Returns the exclusive child's manifest path, or None outside that runner.
/// The total budget includes the manifest reservation; limits and omissions are
/// recorded even when only a prefix can be exported. A returned manifest is never
/// evidence of writer durability, producer success, or a consistent snapshot.
pub fn export_fixture_diagnostics(
    roots: &[(&str, &Path)],
    producer: DiagnosticProducerState,
) -> io::Result<Option<PathBuf>> {
    let Some(leaf) = std::env::var_os("ALAB_UNIT_DIAGNOSTICS_DIR") else {
        return Ok(None);
    };
    export_to(
        Path::new(&leaf),
        roots,
        producer,
        LIMITS,
        ReadObserver::default(),
    )
    .map(|directory| Some(directory.join(DIAGNOSTIC_MANIFEST_FILE)))
}

fn export_to(
    leaf_path: &Path,
    roots: &[(&str, &Path)],
    producer: DiagnosticProducerState,
    limits: Limits,
    observer: ReadObserver,
) -> io::Result<PathBuf> {
    validate(roots, limits)?;
    if !leaf_path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "diagnostic leaf must be absolute",
        ));
    }
    let leaf = directory(leaf_path)?;
    let mut created = None;
    for _ in 0..64 {
        let name = format!(
            "fixture-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        );
        match mkdir(&leaf, name.as_bytes()) {
            Ok(output) => {
                created = Some((name, output));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let (name, output) = created.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "exclusive diagnostic child unavailable",
        )
    })?;
    let mut export = Export {
        manifest: Manifest {
            version: 1,
            declared_producer_state: producer,
            producer_state: producer,
            limits,
            visited_entries: 0,
            file_attempts: 0,
            copied_files: 0,
            copied_bytes: 0,
            read_bytes: 0,
            omitted: 0,
            errors: 0,
            stopped: None,
            entries: Vec::new(),
        },
        detail_bytes: 0,
        leaf: identity(&stat(&leaf)?),
        output: identity(&stat(&output)?),
        observer,
    };
    for (alias, path) in roots {
        if !export.visit() {
            break;
        }
        let source = match directory(path) {
            Ok(source) => source,
            Err(error) => {
                export.record(
                    alias,
                    &[],
                    "error",
                    Some("open_root"),
                    Some(&error),
                    (0, false),
                );
                continue;
            }
        };
        let source_id = match stat(&source) {
            Ok(stat) => identity(&stat),
            Err(error) => {
                export.record(
                    alias,
                    &[],
                    "error",
                    Some("stat_root"),
                    Some(&error),
                    (0, false),
                );
                continue;
            }
        };
        match (below(&leaf, source_id), below(&source, export.leaf)) {
            (Ok(false), Ok(false)) => {}
            (Ok(_), Ok(_)) => {
                export.record(
                    alias,
                    &[],
                    "omitted",
                    Some("source_destination_overlap"),
                    None,
                    (0, false),
                );
                continue;
            }
            (Err(error), _) | (_, Err(error)) => {
                export.record(
                    alias,
                    &[],
                    "error",
                    Some("check_ancestry"),
                    Some(&error),
                    (0, false),
                );
                continue;
            }
        }
        match mkdir(&output, alias.as_bytes()) {
            Ok(target) => export.walk(alias, &source, &target, &[], 0),
            Err(error) => export.record(
                alias,
                &[],
                "error",
                Some("create_root"),
                Some(&error),
                (0, false),
            ),
        }
    }
    let bytes = serde_json::to_vec(&export.manifest)?;
    if bytes.len() > limits.manifest_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "manifest limit exceeded",
        ));
    }
    let mut manifest = open_at(
        &output,
        DIAGNOSTIC_MANIFEST_FILE.as_bytes(),
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )?;
    manifest.write_all(&bytes)?;
    manifest.sync_all()?;
    output.sync_all()?;
    Ok(leaf_path.join(name))
}

#[cfg(test)]
#[path = "fixture_diagnostics_tests.rs"]
mod tests;
