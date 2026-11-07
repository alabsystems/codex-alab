// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The process-life hooks (`ALAB(life)`, tui-life §1, §3.4, §4.3, §7):
//!
//! - `capture_launch`: first thing in `main`, the launch snapshot (argv, environment, cwd,
//!   exe, `argv[0]` resolved through the launch `PATH`, identity) before arg0 rewrites `PATH`
//!   and `.env` is merged.
//! - `capture_cli`: right after `MultitoolCli::parse()`, codex's own clap `Command` (as a
//!   function, built only when a `resumeArgv` is), for the resume rewrite.
//! - `on_terminate`: before the TUI's event loop, SIGTERM, SIGHUP and SIGINT take the `/exit`
//!   path (`AppEvent::Exit(ExitMode::ShutdownFirst)`) instead of killing the process with the
//!   terminal still in raw mode and the alternate screen. A 3 s watchdog (or a second signal)
//!   restores the terminal itself, ends the record and `_exit`s with `128+n`.
//! - `at_exit` and `reraise_signal`: in `handle_app_exit`, the record's ending (a signal's
//!   keeps the record, `exit:"signal:<n>"` and the status the signal found), then the
//!   claim given back (`meta unset supervisor`, while the process still lives: aterm reads
//!   the record at that change and so sees the orderly end), then the signal re-raised so
//!   the status is 143, 129 or 130.
//!
//! Life behavior is inert unless [`codex_alab_core::active`]. The launch hook also
//! installs the explicit managed credential adapter, independently of UI mode.

use crate::identity;
use crate::session;
use crate::session::Ending;
use codex_alab_core::session::Launch;
use codex_alab_core::session::now_ms;
use codex_alab_core::session::shared;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// How long a signalled TUI gets to leave through `/exit` before the watchdog ends it.
pub const WATCHDOG: Duration = Duration::from_secs(3);

/// How long the exit path waits for aterm to answer the claim's release.
pub const RELEASE_AT_EXIT: Duration = Duration::from_secs(2);

/// The watchdog's shorter wait for the same (the TUI is already late).
const RELEASE_AT_WATCHDOG: Duration = Duration::from_secs(1);

static EXITING: AtomicBool = AtomicBool::new(false);

/// ALAB(life) hook, top of `main`: the launch snapshot.
pub fn capture_launch() {
    #[cfg(unix)]
    codex_alab_aterm::managed_auth::install();
    #[cfg(unix)]
    crate::native_source::install();
    #[cfg(unix)]
    crate::assignment::install();
    if !session::active() {
        return;
    }
    let args: Vec<OsString> = std::env::args_os().collect();
    let mut vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let cwd = std::env::current_dir().ok();
    let argv0 = args.first().and_then(|argv0| {
        let path = vars
            .iter()
            .find(|(key, _)| key == "PATH")
            .map(|(_, value)| value.as_os_str());
        resolve_argv0(argv0, path, cwd.as_deref())
    });
    let identity = identity::current().ok();
    let started_at_ms = match &identity {
        Some(codex_alab_core::session::ProcessIdentity::MacOs {
            start_seconds,
            start_microseconds,
            ..
        }) => start_seconds * 1000 + start_microseconds / 1000,
        _ => now_ms(),
    };
    let current_exe = std::env::current_exe().ok();
    let managed_argv0 = current_exe
        .as_deref()
        .and_then(|exe| managed_launch_path(&vars, std::process::id(), exe));
    // Do not mutate the process environment: main can already have runtime threads.
    // PID binding makes inherited hints inert; a re-exec builds without these hints.
    vars.retain(|(key, _)| !managed_launch_var(key));
    let exe_id = current_exe
        .as_deref()
        .and_then(crate::upgrade::build_id)
        .map(|id| (id.dev, id.ino));
    shared().set_launch(Launch {
        args,
        vars,
        cwd,
        current_exe,
        exe_id,
        argv0,
        managed_argv0,
        pid: std::process::id(),
        proc_start: identity.as_ref().and_then(identity::proc_start),
        identity,
        started_at_ms,
    });
}

const MANAGED_PID: &str = "ATPKG_MANAGED_LAUNCH_PID";
const MANAGED_PATH: &str = "ATPKG_MANAGED_LAUNCH_PATH";
const MANAGED_TARGET: &str = "ATPKG_MANAGED_LAUNCH_TARGET";

pub(crate) fn managed_launch_var(key: &OsStr) -> bool {
    [MANAGED_PID, MANAGED_PATH, MANAGED_TARGET]
        .iter()
        .any(|name| key == OsStr::new(name))
}

/// An explicit shim fallback, not a guess from a package-looking argv[0]. The PID only
/// fences inheritance; the package, physical target and managed primary bind the route.
pub(crate) fn managed_launch_path(
    vars: &[(OsString, OsString)],
    pid: u32,
    current_exe: &Path,
) -> Option<PathBuf> {
    let var = |name: &str| {
        let mut found = vars.iter().filter(|(key, _)| key == name);
        let value = found.next()?.1.to_str()?;
        found.next().is_none().then_some(value)
    };
    if var(MANAGED_PID)?.parse::<u32>().ok()? != pid || pid == 0 {
        return None;
    }
    let entry = Path::new(var(MANAGED_PATH)?);
    let fallback = Path::new(var(MANAGED_TARGET)?);
    if !normalized_absolute(entry) || !normalized_absolute(fallback) {
        return None;
    }
    let prefix = entry.ancestors().nth(4)?;
    if prefix.join("store/codex/exec/codex").as_os_str() != entry.as_os_str() {
        return None;
    }
    let prefix = std::fs::canonicalize(prefix).ok()?;
    let physical = std::fs::canonicalize(current_exe).ok()?;
    if std::fs::canonicalize(fallback).ok()? != physical {
        return None;
    }
    let root = alab_package_root(&physical)?;
    if !package_in_prefix(&root, &prefix) {
        return None;
    }
    // Publication may already have changed the primary or the dev-link marker from A
    // to B. Its logical route must still be ours; its current raw target need not be A.
    let primary = read_managed_text(&prefix.join("bin/codex"))?;
    let target = managed_primary_target(&primary, entry)?;
    let selected_root = alab_package_root(&target)?;
    if !package_in_prefix(&selected_root, &prefix) {
        return None;
    }
    Some(entry.to_path_buf())
}

fn normalized_absolute(path: &Path) -> bool {
    use std::path::Component;
    path.is_absolute()
        && path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
        && path.components().collect::<PathBuf>().as_os_str() == path.as_os_str()
}

/// Store packages must belong to this prefix. A dev package can live elsewhere, but
/// only explicit physical-target evidence and its ALab marker admit it above.
fn package_in_prefix(root: &Path, prefix: &Path) -> bool {
    let Some(program) = root.parent() else {
        return false;
    };
    if program.file_name() == Some(OsStr::new("codex"))
        && program.parent().and_then(Path::file_name) == Some(OsStr::new("store"))
    {
        return program == prefix.join("store/codex")
            && root.file_name().and_then(OsStr::to_str).is_some_and(|n| {
                !n.is_empty()
                    && n.bytes().all(|b| b.is_ascii_digit())
                    && (n.len() == 1 || !n.starts_with('0'))
                    && n.parse::<u64>().is_ok()
            });
    }
    true
}

fn alab_package_root(exe: &Path) -> Option<PathBuf> {
    let exe = std::fs::canonicalize(exe).ok()?;
    if !std::fs::metadata(&exe).ok()?.is_file() {
        return None;
    }
    exe.ancestors().skip(1).take(2).find_map(|root| {
        let manifest: serde_json::Value =
            serde_json::from_str(&read_managed_text(&root.join("codex-package.json"))?).ok()?;
        if manifest.get("layoutVersion")?.as_u64()? != 1 {
            return None;
        }
        let entry = Path::new(manifest.get("entrypoint")?.as_str()?);
        if !regular_entry_under(root, entry) || std::fs::canonicalize(root.join(entry)).ok()? != exe
        {
            return None;
        }
        let marker: serde_json::Value =
            serde_json::from_str(&read_managed_text(&root.join("alab-build.json"))?).ok()?;
        marker.is_object().then(|| root.to_path_buf())
    })
}

/// Every normal segment of `entry` under `root` must be a non-symlink directory,
/// and the final segment must be a regular file (never a symlink).
pub(crate) fn regular_entry_under(root: &Path, entry: &Path) -> bool {
    use std::path::Component;
    if entry.is_absolute()
        || !entry
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        || !entry
            .components()
            .any(|c| matches!(c, Component::Normal(_)))
    {
        return false;
    }
    let mut path = root.to_path_buf();
    let parts: Vec<_> = entry
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect();
    for (idx, part) in parts.iter().enumerate() {
        path.push(part);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            return false;
        };
        let ty = meta.file_type();
        let last = idx + 1 == parts.len();
        if (!last && !ty.is_dir()) || (last && !ty.is_file()) {
            return false;
        }
    }
    true
}

/// Read a bounded regular metadata file without following its final component or
/// waiting for a FIFO. Failure supplies no managed launch authority.
pub(crate) fn read_managed_text(path: &Path) -> Option<String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    const LIMIT: u64 = 64 * 1024;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > LIMIT as usize {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn shell_quote(path: &Path) -> Option<String> {
    Some(format!("'{}'", path.to_str()?.replace('\'', "'\\''")))
}

fn shell_path(quoted: &str) -> Option<PathBuf> {
    let path = PathBuf::from(shell_value(quoted)?);
    normalized_absolute(&path).then_some(path)
}

pub(crate) fn shell_value(quoted: &str) -> Option<String> {
    let inner = quoted.strip_prefix('\'')?.strip_suffix('\'')?;
    if inner.split("'\\''").any(|part| part.contains('\'')) {
        return None;
    }
    Some(inner.replace("'\\''", "'"))
}

fn managed_primary_target(body: &str, entry: &Path) -> Option<PathBuf> {
    // Match the produced statements, not a suffix that arbitrary earlier shell
    // commands can shadow. Standalone comments and blank lines have no authority.
    let statements = primary_format::statements(body.strip_prefix("#!/bin/sh\n")?)?;
    let mut body = statements.as_str();
    if body.starts_with("case \"$1\" in\n") {
        body = primary_format::after_store_prelude(body, entry.ancestors().nth(4)?)?;
    }
    let mut names = Vec::new();
    while let Some(export) = body.strip_prefix("export ") {
        let (line, rest) = export.split_once('\n')?;
        let (name, quoted) = line.split_once('=')?;
        let value = shell_value(quoted)?;
        if name.is_empty()
            || name.as_bytes()[0].is_ascii_digit()
            || !name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            || value.is_empty()
            || value.chars().any(char::is_control)
            || value.trim() != value
            || value.contains(['"', '%'])
            || name.len() + value.len() + 1 > 256
            || names.len() == 8
            || names.contains(&name)
        {
            return None;
        }
        names.push(name);
        body = rest;
    }
    let tail: Vec<_> = body.strip_suffix('\n')?.split('\n').collect();
    if tail.len() != 5 {
        return None;
    }
    let quoted = shell_quote(entry)?;
    if tail[0] != format!("[ -L {quoted} ] && [ -x {quoted} ] && exec {quoted} \"$@\"")
        || tail[1] != "export ATPKG_MANAGED_LAUNCH_PID=\"$$\""
        || shell_path(tail[2].strip_prefix("export ATPKG_MANAGED_LAUNCH_PATH=")?)?.as_path()
            != entry
    {
        return None;
    }
    let target = shell_path(tail[3].strip_prefix("export ATPKG_MANAGED_LAUNCH_TARGET=")?)?;
    let final_target = shell_path(tail[4].strip_prefix("exec ")?.strip_suffix(" \"$@\"")?)?;
    (target == final_target).then_some(target)
}

#[path = "managed_primary_format.rs"]
mod primary_format;

/// `argv[0]` as the launch `PATH` resolved it: a name with a `/` is that path (made absolute
/// against the launch cwd), a bare name is the first executable file of that name on `PATH`.
/// Never canonicalized: the twin or reroute path is the point.
pub fn resolve_argv0(argv0: &OsStr, path: Option<&OsStr>, cwd: Option<&Path>) -> Option<PathBuf> {
    let absolute = |candidate: PathBuf| -> Option<PathBuf> {
        if candidate.is_absolute() {
            Some(candidate)
        } else {
            Some(cwd?.join(candidate))
        }
    };
    let name = Path::new(argv0);
    if argv0.as_encoded_bytes().contains(&b'/') {
        return absolute(name.to_path_buf());
    }
    if argv0.is_empty() {
        return None;
    }
    std::env::split_paths(path?)
        .map(|dir| {
            if dir.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                dir
            }
        })
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable_file(candidate))
        .and_then(absolute)
}

fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// ALAB(life) hook, after `MultitoolCli::parse()`: codex's clap definition.
pub fn capture_cli(command: fn() -> clap::Command) {
    if session::active() {
        crate::resume_argv::set_command(command);
    }
}

/// ALAB(life) hook, before the TUI's event loop: TERM/HUP/INT leave like `/exit`. `exit`
/// sends that event; `restore` is the TUI's own `restore_after_exit` (a private module, so
/// it is handed in) for the watchdog. The signals are read on a thread of their own with
/// its own runtime, so a wedged TUI runtime cannot keep the watchdog from firing.
pub fn on_terminate(exit: impl Fn() + Send + Sync + 'static, restore: fn() -> std::io::Result<()>) {
    if !session::active() {
        return;
    }
    // The same `/exit` is how the live upgrade leaves (`upgrade::request`).
    let exit = std::sync::Arc::new(exit);
    let leave = std::sync::Arc::clone(&exit);
    let _ = shared().set_leave(Box::new(move || leave()));
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    let signals = {
        let _context = runtime.enter();
        use tokio::signal::unix::SignalKind;
        use tokio::signal::unix::signal;
        match (
            signal(SignalKind::terminate()),
            signal(SignalKind::hangup()),
            signal(SignalKind::interrupt()),
        ) {
            (Ok(term), Ok(hup), Ok(int)) => (term, hup, int),
            _ => return,
        }
    };
    let _ = std::thread::Builder::new()
        .name("alab-signals".to_owned())
        .spawn(move || {
            runtime.block_on(async move {
                let (mut term, mut hup, mut int) = signals;
                let first = tokio::select! {
                    _ = term.recv() => libc::SIGTERM,
                    _ = hup.recv() => libc::SIGHUP,
                    _ = int.recv() => libc::SIGINT,
                };
                shared().set_signal(first);
                session::hold_status_for_signal();
                exit();
                let _ = tokio::time::timeout(WATCHDOG, async {
                    tokio::select! {
                        _ = term.recv() => {}
                        _ = hup.recv() => {}
                        _ = int.recv() => {}
                    }
                })
                .await;
                watchdog(first, restore);
            });
        });
}

/// Install the upgrade-only conditional event sender. Signals retain their own
/// unconditional /exit path; a queued update is checked when the TUI handles it.
pub fn on_upgrade(
    leave: impl Fn(codex_alab_core::session::UpgradeRequest) + Send + Sync + 'static,
) {
    if session::active() {
        shared().set_upgrade_leave(Box::new(leave));
    }
}

/// The TUI did not leave in time (or a second signal came): restore the terminal, end the
/// record and leave now, unless the exit path already took over.
fn watchdog(signal: i32, restore: fn() -> std::io::Result<()>) {
    if EXITING.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = restore();
    session::finalize(&Ending::of(Some(signal), false, false));
    shared().release_claim(RELEASE_AT_WATCHDOG);
    // SAFETY: _exit has no preconditions; it ends the process without unwinding.
    unsafe { libc::_exit(128 + signal) };
}

/// ALAB(life) hook, `handle_app_exit` once the exit kind is known: end the record (remove it,
/// or keep it as the crash witness for a TERM/HUP/INT signal, a fatal exit or an upgrade), THEN
/// give the claim back while the process still lives (CONTRACT §4: without that change
/// aterm's last copy still shows the record, and an orderly exit reads as a crash). The
/// live upgrade execs here, the `/exit` teardown done and before any hint is printed; it
/// does not return (`upgrade::exec_after_teardown`).
pub fn at_exit(is_fatal: bool) {
    if !session::active() || EXITING.swap(true, Ordering::SeqCst) {
        return;
    }
    let shared = shared();
    let ending = Ending::of(shared.signal(), shared.exec_pending(), is_fatal);
    session::finalize(&ending);
    shared.release_claim(RELEASE_AT_EXIT);
    if ending == Ending::Upgrading {
        crate::upgrade::exec_after_teardown(shared);
    }
}

/// ALAB(life) hook, the end of `handle_app_exit`: a TUI that left for a signal ends as that
/// signal (status 143, 129 or 130), as it would have without the handler.
pub fn reraise_signal() {
    if !session::active() {
        return;
    }
    let Some(signal) = shared().signal() else {
        return;
    };
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // SAFETY: restoring the default disposition, unblocking and raising are async-signal-safe
    // libc calls with valid arguments.
    unsafe {
        libc::signal(signal, libc::SIG_DFL);
        let mut set = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
        libc::sigemptyset(set.as_mut_ptr());
        libc::sigaddset(set.as_mut_ptr(), signal);
        libc::pthread_sigmask(libc::SIG_UNBLOCK, set.as_ptr(), std::ptr::null_mut());
        libc::raise(signal);
    }
    std::process::exit(128 + signal);
}
