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
//! Every hook is inert unless [`codex_alab_core::active`].

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
    if !session::active() {
        return;
    }
    let args: Vec<OsString> = std::env::args_os().collect();
    let vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
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
        pid: std::process::id(),
        proc_start: identity.as_ref().and_then(identity::proc_start),
        identity,
        started_at_ms,
    });
}

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
