// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The vendor daemon's stale packages (`ALAB(no-daemon)`, `tui/src/startup_orchestration.rs`).
//!
//! OpenAI's codex keeps a full copy of itself per version under
//! `$CODEX_HOME/packages/app-server-daemon/releases` (about 316 MB each) for the shared
//! app-server daemon, and prunes them only when its own daemon or installer runs
//! (`app-server-daemon/src/release_pruning.rs`, `prepare_install.rs`). codex-alab never starts
//! or attaches to that daemon, so copies a vendor codex left behind stay. At TUI startup, when
//! codex-alab's effective config has the daemon off, [`tidy`] removes the superseded ones on a
//! thread of its own, never delaying startup.
//!
//! Fail-closed: any doubt keeps every release. It keeps the one `current` names (a vendor codex
//! started later still finds its daemon package: drop-in safety) and the ones the vendor's
//! `selection-history` names (its rollback target). It is more thorough than the vendor here:
//! the vendor's pruning refuses to run without a `selection-history` that matches `current`,
//! and so keeps every copy, where this keeps only `current`'s (a vendor codex runs only that
//! one, and its installer fetches any other again). It keeps anything that is not a release the
//! vendor installer could have made (name, `codex-package.json` and entrypoint must agree),
//! anything crossing a mount (another device, or a mount point inside it: a Linux bind mount
//! keeps its source's device), and everything at all while a daemon may be running (a pid file
//! names a live process, a process runs from the package directory, or its control socket
//! answers), while either of the daemon's locks is held or a migration is under way, or
//! without a valid `current`.
//!
//! Whether anything is superseded is asked first, without a lock or a probe, so a vendor-daemon
//! user's every start (`current` and its rollback target) costs a directory read. The control
//! socket is probed last: it is the one check that reaches a running daemon. A release is
//! renamed out of the way before it is deleted, so a half-deleted copy never looks like a
//! release; a leftover of that rename is finished on a later start. The upstream files these
//! rules copy are in `alab/watched-paths`, so a sync that changes one says so.

use codex_alab_core::active;
use std::ffi::OsString;
use std::fs::File;
use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;

/// The prefix of a release renamed out of the way and not yet deleted (always ours).
const RETIRED_PREFIX: &str = ".alab-retired.";
const NO_PACKAGES: &str = "no vendor daemon packages";
const NOTHING_SUPERSEDED: &str = "nothing superseded";
const NO_CURRENT: &str = "no valid `current` release";
const OPERATION_LOCK_HELD: &str = "the daemon's operation lock is held";
const INSTALLER_DIRECTORY_LOCK_HELD: &str = "the daemon installer's directory lock is held";
const INSTALLER_LOCK_HELD: &str = "the daemon installer's lock is held";
const MIGRATING: &str = "a daemon migration is under way";
const SOCKET_ANSWERS: &str = "the daemon's control socket answers";
const CROSSES_A_MOUNT: &str = "crosses a mount";
/// The vendor daemon's pid files in its state directory (`app-server-daemon/src/lib.rs`).
const PID_FILES: [&str; 4] = [
    "daemon.pid",
    "daemon-updater.pid",
    "app-server.pid",
    "app-server-updater.pid",
];

/// The ALAB(no-daemon) hook, at TUI startup once the config is known: with the daemon off by
/// codex-alab's effective config (`[features] daemon_auto_start`), remove the vendor daemon's
/// superseded packages on a background thread, and log what was done. The thread, when one
/// started; the hook drops it.
pub fn tidy(codex_home: &Path, daemon_auto_start: bool) -> Option<std::thread::JoinHandle<()>> {
    if !should_tidy(active(), daemon_auto_start) {
        return None;
    }
    let codex_home = codex_home.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("alab-daemon-packages".to_string())
        .spawn(move || prune(&codex_home, &LiveHost).log());
    match spawned {
        Ok(thread) => Some(thread),
        Err(err) => {
            tracing::warn!("codex-alab: vendor daemon packages not tidied: no thread: {err}");
            None
        }
    }
}

/// Only codex-alab tidies, and only while its config keeps the daemon off.
pub fn should_tidy(active: bool, daemon_auto_start: bool) -> bool {
    active && !daemon_auto_start
}

/// What the tidy asks of the machine it runs on.
pub trait Host {
    /// The executable of every process this user can see, for "does anything run from here".
    fn executables(&self) -> std::io::Result<Vec<PathBuf>>;
    /// The mount points this process sees where a device cannot tell them apart (Linux: a bind
    /// mount keeps its source's device); none where every mount has a device of its own.
    fn mount_points(&self) -> std::io::Result<Vec<PathBuf>>;
}

/// The real machine.
pub struct LiveHost;

impl Host for LiveHost {
    fn executables(&self) -> std::io::Result<Vec<PathBuf>> {
        running_executables()
    }

    fn mount_points(&self) -> std::io::Result<Vec<PathBuf>> {
        mount_points()
    }
}

/// Why one tidy kept every release.
#[derive(Debug, PartialEq, Eq)]
pub enum KeptAll {
    /// No vendor daemon packages, or none superseded: the everyday answer (logged at DEBUG).
    Nothing(&'static str),
    /// The vendor's daemon or installer is, or may be, at work, and owns its packages then: a
    /// lock is held, a migration is under way, or a daemon may run (DEBUG: a vendor-daemon user
    /// can meet it on every start).
    Busy(String),
    /// Something that could not be told apart from a reason to keep (INFO: superseded copies
    /// stay, and the log says why).
    Doubt(String),
}

/// What one tidy did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Why every release was kept, when one was.
    pub kept_all: Option<KeptAll>,
    /// What was removed (names: superseded releases, and leftovers of an earlier tidy), and
    /// the bytes it held.
    pub removed: Vec<String>,
    pub freed_bytes: u64,
    /// Entries left in `releases/` besides `current`'s, each with why.
    pub kept: Vec<(String, String)>,
    /// Releases that could not be removed, each with the error.
    pub failed: Vec<(String, String)>,
}

impl Report {
    fn keep_all(kept: KeptAll) -> Self {
        Self {
            kept_all: Some(kept),
            ..Self::default()
        }
    }

    fn log(&self) {
        match &self.kept_all {
            Some(KeptAll::Nothing(reason)) => {
                tracing::debug!("codex-alab: vendor daemon packages kept: {reason}");
            }
            Some(KeptAll::Busy(reason)) => {
                tracing::debug!("codex-alab: vendor daemon packages kept: {reason}");
            }
            Some(KeptAll::Doubt(reason)) => {
                tracing::info!("codex-alab: vendor daemon packages kept: {reason}");
            }
            None => {}
        }
        if !self.removed.is_empty() {
            tracing::info!(
                removed = ?self.removed,
                freed_bytes = self.freed_bytes,
                kept = ?self.kept,
                "codex-alab: removed the vendor daemon's superseded packages"
            );
        }
        for (name, err) in &self.failed {
            tracing::warn!("codex-alab: vendor daemon package {name} not removed: {err}");
        }
    }
}

/// The vendor daemon's package directory under `codex_home`.
pub fn package_root(codex_home: &Path) -> PathBuf {
    codex_home.join("packages").join("app-server-daemon")
}

/// Remove every release under [`package_root`] but `current`'s, unless anything is in doubt.
pub fn prune(codex_home: &Path, host: &dyn Host) -> Report {
    let root = package_root(codex_home);
    let releases = root.join("releases");
    if !is_plain_dir(&root) || !is_plain_dir(&releases) {
        return Report::keep_all(KeptAll::Nothing(NO_PACKAGES));
    }
    // Cheap first, without a lock or a probe: nothing superseded, nothing to do.
    match survey(&root, &releases) {
        Ok(survey) if !survey.has_work() => {
            return Report::keep_all(KeptAll::Nothing(NOTHING_SUPERSEDED));
        }
        Ok(_) => {}
        Err(kept) => return Report::keep_all(kept),
    }
    let state_dir = codex_home.join("app-server-daemon");
    if !is_plain_dir(&state_dir) {
        return Report::keep_all(KeptAll::Doubt("no daemon state directory".to_string()));
    }
    // The daemon's own locks: its lifecycle (start, stop, update, migration) and its installer.
    let _operation = match try_flock(&state_dir.join("daemon.lock")) {
        Ok(Some(lock)) => lock,
        Ok(None) => return Report::keep_all(KeptAll::Busy(OPERATION_LOCK_HELD.to_string())),
        Err(err) => {
            return Report::keep_all(KeptAll::Doubt(format!(
                "the daemon's operation lock: {err}"
            )));
        }
    };
    if root.join("install.lock.d").symlink_metadata().is_ok() {
        return Report::keep_all(KeptAll::Busy(INSTALLER_DIRECTORY_LOCK_HELD.to_string()));
    }
    let _install = match try_install_lock(&root.join("install.lock")) {
        Ok(Some(lock)) => lock,
        Ok(None) => return Report::keep_all(KeptAll::Busy(INSTALLER_LOCK_HELD.to_string())),
        Err(err) => {
            return Report::keep_all(KeptAll::Doubt(format!(
                "the daemon installer's lock: {err}"
            )));
        }
    };
    // A migration's prepared release, not yet `current` (`app-server-daemon/src/migration.rs`):
    // the vendor prunes nothing while it is there.
    if root.join(".migration-current").symlink_metadata().is_ok() {
        return Report::keep_all(KeptAll::Busy(MIGRATING.to_string()));
    }
    if let Some(kept) = daemon_may_run(codex_home, &state_dir, &root, host) {
        return Report::keep_all(kept);
    }
    // Under the locks: the selection again, and what it supersedes.
    let survey = match survey(&root, &releases) {
        Ok(survey) => survey,
        Err(kept) => return Report::keep_all(kept),
    };
    let mount_points = match host.mount_points() {
        Ok(mount_points) => mount_points,
        Err(err) => return Report::keep_all(KeptAll::Doubt(format!("the mount table: {err}"))),
    };
    let mut report = Report::default();
    for entry in survey.entries {
        let leftover = match entry.verdict {
            Verdict::Keep(reason) => {
                report.kept.push((entry.name, reason));
                continue;
            }
            Verdict::Leftover => true,
            Verdict::Superseded => false,
        };
        let canonical = survey.canonical.join(&entry.file_name);
        let bytes = match measure(&entry.path, &canonical, survey.device, &mount_points) {
            Ok(bytes) => bytes,
            Err(reason) => {
                report.kept.push((entry.name, reason));
                continue;
            }
        };
        let doomed = if leftover {
            entry.path
        } else {
            let retired = releases.join(format!(
                "{RETIRED_PREFIX}{}.{}.{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_nanos()),
                entry.name
            ));
            if let Err(err) = std::fs::rename(&entry.path, &retired) {
                report.failed.push((entry.name, err.to_string()));
                continue;
            }
            retired
        };
        match std::fs::remove_dir_all(&doomed) {
            Ok(()) => {
                report.removed.push(entry.name);
                report.freed_bytes += bytes;
            }
            Err(err) => report.failed.push((entry.name, err.to_string())),
        }
    }
    report.removed.sort();
    report.kept.sort();
    report
}

/// One look at `releases/`: every entry but `current`'s, each with what to do with it.
struct Survey {
    /// `releases`, canonical (as a mount table names its paths), and its device.
    canonical: PathBuf,
    device: u64,
    entries: Vec<Entry>,
}

struct Entry {
    file_name: OsString,
    name: String,
    path: PathBuf,
    verdict: Verdict,
}

enum Verdict {
    /// A release the installer made that `current` and `selection-history` do not name.
    Superseded,
    /// A release an earlier tidy renamed and did not finish deleting.
    Leftover,
    Keep(String),
}

impl Survey {
    fn has_work(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| !matches!(entry.verdict, Verdict::Keep(_)))
    }
}

fn survey(root: &Path, releases: &Path) -> Result<Survey, KeptAll> {
    let Some(selected) = selected_release(root, releases) else {
        return Err(KeptAll::Doubt(NO_CURRENT.to_string()));
    };
    // The vendor's own pruning keeps its previous selection too (a rollback target).
    let previous: Vec<OsString> = match std::fs::read_to_string(root.join("selection-history")) {
        Ok(history) => history.lines().take(2).map(OsString::from).collect(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(KeptAll::Doubt(format!("selection-history: {err}"))),
    };
    let doubt = |err: std::io::Error| KeptAll::Doubt(format!("releases: {err}"));
    let canonical = std::fs::canonicalize(releases).map_err(doubt)?;
    let device = canonical.symlink_metadata().map_err(doubt)?.dev();
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(releases).map_err(doubt)? {
        let Ok(entry) = entry else {
            continue;
        };
        let file_name = entry.file_name();
        if file_name == selected {
            continue;
        }
        let path = entry.path();
        let name = file_name.to_string_lossy().into_owned();
        let verdict = if previous.contains(&file_name) {
            Verdict::Keep("named in selection-history".to_string())
        } else if name.starts_with(RETIRED_PREFIX) {
            Verdict::Leftover
        } else {
            match superseded_release(&path, &name) {
                Ok(()) => Verdict::Superseded,
                Err(reason) => Verdict::Keep(reason),
            }
        };
        entries.push(Entry {
            file_name,
            name,
            path,
            verdict,
        });
    }
    Ok(Survey {
        canonical,
        device,
        entries,
    })
}

fn is_plain_dir(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir())
}

/// The name of the release `root/current` names: a plain directory directly in `releases`
/// (so `current` is a symlink: as itself, its parent would be `root`).
fn selected_release(root: &Path, releases: &Path) -> Option<OsString> {
    let selected = std::fs::canonicalize(root.join("current")).ok()?;
    let releases = std::fs::canonicalize(releases).ok()?;
    if selected.parent() != Some(releases.as_path()) || !is_plain_dir(&selected) {
        return None;
    }
    selected.file_name().map(OsString::from)
}

/// Why a daemon may be running, if one may: a pid file names a live or unreadable process, a
/// process runs from the package directory (or the process table cannot be read), or, asked
/// last because asking reaches the daemon, its control socket answers (or cannot be told apart
/// from one that does).
fn daemon_may_run(
    codex_home: &Path,
    state_dir: &Path,
    root: &Path,
    host: &dyn Host,
) -> Option<KeptAll> {
    for name in PID_FILES {
        let path = state_dir.join(name);
        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Some(KeptAll::Doubt(format!("{name}: {err}"))),
        };
        let Some(pid) = recorded_pid(&contents) else {
            return Some(KeptAll::Doubt(format!("{name} names no pid")));
        };
        if process_may_live(pid) {
            return Some(KeptAll::Busy(format!("{name} names live process {pid}")));
        }
    }
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    match host.executables() {
        Ok(executables) => {
            if let Some(exe) = executables.iter().find(|exe| exe.starts_with(&root)) {
                return Some(KeptAll::Busy(format!("{} runs", exe.display())));
            }
        }
        Err(err) => return Some(KeptAll::Doubt(format!("the process table: {err}"))),
    }
    let socket = codex_home
        .join("app-server-control")
        .join("app-server-control.sock");
    match control_socket_answers(&socket) {
        Ok(false) => None,
        Ok(true) => Some(KeptAll::Busy(SOCKET_ANSWERS.to_string())),
        Err(err) => Some(KeptAll::Doubt(format!(
            "the daemon's control socket: {err}"
        ))),
    }
}

/// Whether a daemon answers on `socket`. A path longer than `sun_path` is followed through
/// its symlinks first, as codex's own client does (`codex_uds::connect_stream`); a socket file
/// that still cannot be reached is an error (a doubt), and no file at all answers nothing.
fn control_socket_answers(socket: &Path) -> std::io::Result<bool> {
    use std::io::ErrorKind;
    let connected = match std::os::unix::net::UnixStream::connect(socket) {
        Err(err) if err.kind() == ErrorKind::InvalidInput => match std::fs::canonicalize(socket) {
            Ok(target) => std::os::unix::net::UnixStream::connect(target),
            Err(err) => Err(err),
        },
        connected => connected,
    };
    match connected {
        Ok(_) => Ok(true),
        Err(err)
            if matches!(
                err.kind(),
                ErrorKind::NotFound | ErrorKind::ConnectionRefused
            ) =>
        {
            Ok(false)
        }
        Err(err) => Err(err),
    }
}

/// The pid a daemon pid file records: its JSON record's `pid`, or a bare number.
fn recorded_pid(contents: &str) -> Option<i32> {
    let pid = match serde_json::from_str::<serde_json::Value>(contents.trim()) {
        Ok(serde_json::Value::Object(record)) => record.get("pid")?.as_u64()?,
        Ok(serde_json::Value::Number(pid)) => pid.as_u64()?,
        _ => return None,
    };
    i32::try_from(pid).ok().filter(|pid| *pid > 0)
}

/// Whether `pid` may be a live process: anything but "no such process" says it may.
fn process_may_live(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists and may be signalled.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Whether `path` (an entry of `releases/` named `name`) is a release the vendor installer
/// made: a plain directory whose name is `<version>-<target>` or `local-<blake3 hex>-<target>`
/// per its `codex-package.json`, with the `bin/codex` entrypoint.
fn superseded_release(path: &Path, name: &str) -> Result<(), String> {
    #[derive(serde::Deserialize)]
    struct Manifest {
        version: String,
        target: String,
        entrypoint: String,
    }
    if !is_plain_dir(path) {
        return Err("not a directory".to_string());
    }
    let manifest = std::fs::read_to_string(path.join("codex-package.json"))
        .map_err(|err| format!("codex-package.json: {err}"))?;
    let manifest: Manifest =
        serde_json::from_str(&manifest).map_err(|err| format!("codex-package.json: {err}"))?;
    let local = name
        .strip_prefix("local-")
        .and_then(|rest| rest.strip_suffix(&format!("-{}", manifest.target)))
        .is_some_and(|digest| digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()));
    let versioned = !manifest.version.is_empty()
        && !manifest.target.is_empty()
        && name == format!("{}-{}", manifest.version, manifest.target);
    if !(versioned || local) {
        return Err("not named for its codex-package.json".to_string());
    }
    if manifest.entrypoint != "bin/codex" {
        return Err(format!("entrypoint {}", manifest.entrypoint));
    }
    Ok(())
}

/// The bytes under `path` (an entry of `releases/`, `canonical` as the mount table would name
/// it), if no mount point lies inside it and every entry (never followed) is on `device`; else
/// why not.
fn measure(
    path: &Path,
    canonical: &Path,
    device: u64,
    mount_points: &[PathBuf],
) -> Result<u64, String> {
    if let Some(mount_point) = mount_points
        .iter()
        .find(|mount| mount.starts_with(canonical))
    {
        return Err(format!("mount point {}", mount_point.display()));
    }
    let metadata = path.symlink_metadata().map_err(|err| err.to_string())?;
    if !metadata.is_dir() {
        return Err("not a directory".to_string());
    }
    if metadata.dev() != device {
        return Err(CROSSES_A_MOUNT.to_string());
    }
    let unreadable = |path: &Path, err: std::io::Error| format!("{}: {err}", path.display());
    let mut bytes = 0;
    let mut pending = vec![path.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|err| unreadable(&dir, err))? {
            let entry = entry.map_err(|err| unreadable(&dir, err))?;
            let entry = entry.path();
            let metadata = entry
                .symlink_metadata()
                .map_err(|err| unreadable(&entry, err))?;
            if metadata.dev() != device {
                return Err(CROSSES_A_MOUNT.to_string());
            }
            if metadata.is_dir() {
                pending.push(entry);
            } else {
                bytes += metadata.len();
            }
        }
    }
    Ok(bytes)
}

/// An exclusive `flock` on `path` without waiting: `None` when another holds it.
fn try_flock(path: &Path) -> std::io::Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    // SAFETY: a plain flock(2) on a descriptor this function owns.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(Some(file));
    }
    let err = std::io::Error::last_os_error();
    if err.kind() == std::io::ErrorKind::WouldBlock {
        Ok(None)
    } else {
        Err(err)
    }
}

/// The installer's lock on `install.lock`, as the daemon takes it: `lockf` on macOS (the shell
/// installer's `lockf`), `flock` elsewhere.
fn try_install_lock(path: &Path) -> std::io::Result<Option<File>> {
    #[cfg(target_os = "macos")]
    {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        // SAFETY: a plain lockf(3) on a descriptor this function owns.
        if unsafe { libc::lockf(file.as_raw_fd(), libc::F_TLOCK, 0) } == 0 {
            return Ok(Some(file));
        }
        let err = std::io::Error::last_os_error();
        if matches!(err.raw_os_error(), Some(libc::EAGAIN) | Some(libc::EACCES)) {
            Ok(None)
        } else {
            Err(err)
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        try_flock(path)
    }
}

/// The mount points this process sees where a device cannot tell them apart: Linux's mount
/// table (a bind mount keeps its source's device). Elsewhere every mount has a device of its
/// own, which [`measure`] compares.
pub fn mount_points() -> std::io::Result<Vec<PathBuf>> {
    #[cfg(target_os = "linux")]
    {
        mountinfo_points(&std::fs::read("/proc/self/mountinfo")?)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(Vec::new())
    }
}

/// The mount points of a Linux `mountinfo` table (proc(5)): each line's fifth field, with
/// space, tab, newline and backslash escaped as `\ooo`. A line without one is an error.
#[cfg(any(target_os = "linux", test))]
fn mountinfo_points(table: &[u8]) -> std::io::Result<Vec<PathBuf>> {
    use std::os::unix::ffi::OsStringExt;
    let mut mount_points = Vec::new();
    for line in table.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Some(field) = line.split(|byte| *byte == b' ').nth(4) else {
            return Err(std::io::Error::other(format!(
                "a mountinfo line without a mount point: {}",
                String::from_utf8_lossy(line)
            )));
        };
        let mut path = Vec::with_capacity(field.len());
        let mut rest = field;
        while let Some((&byte, tail)) = rest.split_first() {
            match tail {
                [a @ b'0'..=b'3', b @ b'0'..=b'7', c @ b'0'..=b'7', ..] if byte == b'\\' => {
                    path.push(((a - b'0') << 6) | ((b - b'0') << 3) | (c - b'0'));
                    rest = &tail[3..];
                }
                _ => {
                    path.push(byte);
                    rest = tail;
                }
            }
        }
        mount_points.push(PathBuf::from(OsString::from_vec(path)));
    }
    Ok(mount_points)
}

/// The executable of every process this user can see.
pub fn running_executables() -> std::io::Result<Vec<PathBuf>> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        // SAFETY: a null buffer asks proc_listallpids for the number of pids.
        let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        if count <= 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut pids = vec![0 as libc::pid_t; count as usize * 2 + 64];
        let size = libc::c_int::try_from(pids.len() * std::mem::size_of::<libc::pid_t>())
            .map_err(std::io::Error::other)?;
        // SAFETY: `pids` holds `size` bytes; the call writes at most that many.
        let filled = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), size) };
        if filled <= 0 {
            return Err(std::io::Error::last_os_error());
        }
        pids.truncate(filled as usize);
        let mut executables = Vec::with_capacity(pids.len());
        let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        for pid in pids.into_iter().filter(|pid| *pid > 0) {
            // SAFETY: `buffer` holds PROC_PIDPATHINFO_MAXSIZE bytes.
            let len =
                unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
            if len > 0 {
                let path = std::ffi::OsStr::from_bytes(&buffer[..len as usize]);
                executables.push(PathBuf::from(path));
                continue;
            }
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::ESRCH) {
                return Err(std::io::Error::other(format!("pid {pid}: {err}")));
            }
        }
        Ok(executables)
    }
    #[cfg(target_os = "linux")]
    {
        let mut executables = Vec::new();
        for entry in std::fs::read_dir("/proc")? {
            let entry = entry?;
            if !entry
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_digit())
            {
                continue;
            }
            match std::fs::read_link(entry.path().join("exe")) {
                Ok(exe) => executables.push(exe),
                // Gone, a kernel thread, or another user's (which cannot run from here).
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                    ) => {}
                Err(err) => return Err(err),
            }
        }
        Ok(executables)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(std::io::Error::other("no process table on this platform"))
    }
}

#[cfg(test)]
#[path = "daemon_packages_tests.rs"]
mod tests;
