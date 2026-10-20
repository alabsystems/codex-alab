// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The live upgrade by re-exec (CONTRACT §5 row "live upgrade", tui-life §4): the process
//! half. The autopilot decides when (`codex-rs/alab-autopilot`, `upgrade.rs`); this says
//! whether there is anything to move onto, builds the exec, asks the TUI to leave like
//! `/exit`, and execs once the teardown is done.
//!
//! - **Trigger** ([`launch_names_other_build`]): the build the launch path (`launchArgv0`,
//!   atpkg's twin) names NOW is not the file this image was started from. The launch path
//!   is followed through symlinks, and an atpkg shim (`#!/bin/sh` whose last line is
//!   `exec '<abs>' "$@"`) names the build it execs. Builds compare by file identity
//!   (device, inode), never by version strings (CONTRACT §7). Anything unreadable names no
//!   other build: no upgrade.
//! - **The exec** ([`exec_plan`]): `execve(resumeArgv[0], resumeArgv, <launch environment>)`
//!   from the launch cwd, `resumeArgv` rebuilt exactly as the session record's is (the
//!   primary thread's rollout must exist). The launch environment is the one captured first
//!   thing in `main`, before arg0 rewrote `PATH` and `.env` was merged; no variable is
//!   added (the new image learns `execGeneration` from its own pid's record).
//! - **Leave** ([`request`]): `exec_pending`, then the TUI's own `/exit`.
//! - **Exec** ([`exec_after_teardown`], from `life::at_exit` once the record was rewritten
//!   `status:"upgrading"`): MCP and other children get [`CHILDREN_GRACE`], then the exec
//!   runs anyway; signal dispositions and the mask are reset; a failed exec is `_exit(1)`
//!   with the record kept, so aterm's crash rule relaunches the session.

use crate::resume_argv;
use crate::session::rollout_materialized;
use codex_alab_core::session::Launch;
use codex_alab_core::session::Shared;
use std::ffi::CString;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

/// How long children (MCP servers, mostly) get to leave after the teardown.
pub const CHILDREN_GRACE: Duration = Duration::from_secs(2);

/// A build: the file a path finally names, by identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildId {
    pub dev: u64,
    pub ino: u64,
}

/// The identity of the regular file `path` names (symlinks followed).
pub fn build_id(path: &Path) -> Option<BuildId> {
    let metadata = std::fs::metadata(path).ok()?;
    metadata.is_file().then(|| BuildId {
        dev: metadata.dev(),
        ino: metadata.ino(),
    })
}

/// The binary an atpkg shim execs: the last line `exec '<absolute path>' "$@"` of a
/// `#!/bin/sh` script. Anything else names nothing.
pub fn shim_target(script: &str) -> Option<PathBuf> {
    if !script.starts_with("#!/bin/sh") {
        return None;
    }
    let last = script.lines().rev().find(|line| !line.trim().is_empty())?;
    let quoted = last
        .trim()
        .strip_prefix("exec '")?
        .strip_suffix("' \"$@\"")?;
    if quoted.contains('\'') || !quoted.starts_with('/') {
        return None;
    }
    Some(PathBuf::from(quoted))
}

/// The build `launch` names now: the file it resolves to, or the binary it execs when that
/// file is an atpkg shim.
pub fn named_build(launch: &Path) -> Option<PathBuf> {
    let resolved = std::fs::canonicalize(launch).ok()?;
    let head = read_head(&resolved, 64 * 1024)?;
    if head.starts_with(b"#!") {
        let target = shim_target(std::str::from_utf8(&head).ok()?)?;
        return std::fs::canonicalize(target).ok();
    }
    Some(resolved)
}

fn read_head(path: &Path, limit: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut head = Vec::new();
    file.take(limit).read_to_end(&mut head).ok()?;
    Some(head)
}

/// Whether `launch` now names a build other than `current` (a file that exists and is
/// executable). `false` whenever anything cannot be read.
pub fn names_other_build(launch: &Path, current: BuildId) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Some(named) = named_build(launch) else {
        return false;
    };
    let executable =
        std::fs::metadata(&named).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    executable && build_id(&named).is_some_and(|id| id != current)
}

/// The trigger, for this process: its launch path names another build than it runs.
pub fn launch_names_other_build(launch: &Launch) -> bool {
    match (launch.argv0.as_deref(), launch.exe_id) {
        (Some(argv0), Some((dev, ino))) => names_other_build(argv0, BuildId { dev, ino }),
        _ => false,
    }
}

/// Everything the exec needs, decided before the teardown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecPlan {
    /// `resumeArgv[0]`: the launch path (atpkg's twin), never `current_exe()`.
    pub path: PathBuf,
    pub argv: Vec<OsString>,
    /// The launch environment, verbatim and in order.
    pub env: Vec<(OsString, OsString)>,
    pub cwd: Option<PathBuf>,
}

/// The exec for `launch` resuming `thread`, its argv rebuilt from codex's own clap
/// `Command` as the record's `resumeArgv` is. `Err(why)` when it cannot be exact.
pub fn plan_for(
    launch: &Launch,
    command: fn() -> clap::Command,
    thread: &str,
) -> Result<ExecPlan, String> {
    let argv0 = launch
        .argv0
        .as_deref()
        .ok_or("the launch path is unknown")?;
    let argv0_str = argv0.to_str().ok_or("the launch path is not UTF-8")?;
    let argv = resume_argv::rebuild(command(), &launch.args, argv0_str, thread)
        .ok_or("the launch command line cannot be rewritten to resume")?;
    if argv.first().map(String::as_str) != Some(argv0_str) {
        return Err("the resume argv does not start with the launch path".to_owned());
    }
    Ok(ExecPlan {
        path: argv0.to_path_buf(),
        argv: argv.into_iter().map(OsString::from).collect(),
        env: launch.vars.clone(),
        cwd: launch.cwd.clone(),
    })
}

/// The exec for this process now: its launch, codex's clap definition, and the primary
/// thread, whose rollout must be on disk (a resume cannot find it otherwise).
pub fn exec_plan(shared: &Shared) -> Result<ExecPlan, String> {
    let launch = shared.launch().ok_or("no launch snapshot")?;
    let command = resume_argv::command().ok_or("codex's command line was not captured")?;
    let view = shared.view();
    let thread = view.primary_thread.as_deref().ok_or("no primary thread")?;
    let materialized = view
        .primary()
        .and_then(|facts| facts.rollout_path.as_deref())
        .is_some_and(rollout_materialized);
    if !materialized {
        return Err("the primary thread's rollout is not on disk yet".to_owned());
    }
    plan_for(launch, command, thread)
}

/// Leave like `/exit` so the exit path execs. `false` (and nothing pending) when the TUI
/// never said how to leave.
pub fn request(shared: &Shared) -> bool {
    shared.set_exec_pending(true);
    if shared.leave() {
        true
    } else {
        shared.set_exec_pending(false);
        false
    }
}

/// The exit path of an upgrade: exec the plan, or `_exit(1)` with the record kept.
pub fn exec_after_teardown(shared: &Shared) -> ! {
    use std::io::Write;
    let plan = exec_plan(shared);
    wait_for_children(CHILDREN_GRACE);
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    let why = match plan {
        Ok(plan) => exec(&plan),
        Err(why) => why,
    };
    let _ = writeln!(
        std::io::stderr(),
        "codex-alab: the upgrade could not re-exec ({why}); aterm relaunches this session"
    );
    // SAFETY: _exit has no preconditions; it ends the process without unwinding.
    unsafe { libc::_exit(1) }
}

/// Wait up to `grace` for this process's children to go (reaping the ones that did).
pub fn wait_for_children(grace: Duration) -> usize {
    let deadline = Instant::now() + grace;
    loop {
        // SAFETY: waitpid with WNOHANG on any child only reaps; a null status is allowed.
        while unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) } > 0 {}
        let left = live_children();
        if left == 0 || Instant::now() >= deadline {
            return left;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(target_os = "macos")]
fn live_children() -> usize {
    let mut pids = [0 as libc::pid_t; 256];
    let size = i32::try_from(std::mem::size_of_val(&pids)).unwrap_or(i32::MAX);
    // SAFETY: the buffer is valid for `size` bytes; the call writes at most that many.
    let n = unsafe {
        libc::proc_listchildpids(
            libc::getpid(),
            pids.as_mut_ptr().cast::<libc::c_void>(),
            size,
        )
    };
    usize::try_from(n).unwrap_or(0)
}

#[cfg(not(target_os = "macos"))]
fn live_children() -> usize {
    let Ok(tasks) = std::fs::read_dir("/proc/self/task") else {
        return 0;
    };
    tasks
        .flatten()
        .filter_map(|task| std::fs::read_to_string(task.path().join("children")).ok())
        .map(|children| children.split_whitespace().count())
        .sum()
}

fn cstring(bytes: &[u8]) -> Result<CString, String> {
    CString::new(bytes).map_err(|_| "an argument holds a NUL byte".to_owned())
}

/// `execve` the plan; returns only why it failed.
fn exec(plan: &ExecPlan) -> String {
    let path = match cstring(plan.path.as_os_str().as_bytes()) {
        Ok(path) => path,
        Err(why) => return why,
    };
    let argv: Result<Vec<CString>, String> = plan
        .argv
        .iter()
        .map(|arg| cstring(arg.as_bytes()))
        .collect();
    let env: Result<Vec<CString>, String> = plan
        .env
        .iter()
        .map(|(key, value)| {
            let mut entry = key.as_bytes().to_vec();
            entry.push(b'=');
            entry.extend_from_slice(value.as_bytes());
            cstring(&entry)
        })
        .collect();
    let (argv, env) = match (argv, env) {
        (Ok(argv), Ok(env)) => (argv, env),
        (Err(why), _) | (_, Err(why)) => return why,
    };
    let mut argv_ptrs: Vec<*const libc::c_char> = argv.iter().map(|a| a.as_ptr()).collect();
    argv_ptrs.push(std::ptr::null());
    let mut env_ptrs: Vec<*const libc::c_char> = env.iter().map(|e| e.as_ptr()).collect();
    env_ptrs.push(std::ptr::null());
    if let Some(cwd) = &plan.cwd {
        let _ = std::env::set_current_dir(cwd);
    }
    reset_signals();
    // SAFETY: every pointer is a NUL-terminated string kept alive above, and both arrays
    // end with a null pointer, as execve requires.
    unsafe { libc::execve(path.as_ptr(), argv_ptrs.as_ptr(), env_ptrs.as_ptr()) };
    format!(
        "execve {}: {}",
        plan.path.display(),
        std::io::Error::last_os_error()
    )
}

/// The new image starts as a launched process does: no signal blocked, and the signals a
/// runtime or a TUI ignores back at their default (an ignored disposition survives exec).
fn reset_signals() {
    // SAFETY: sigemptyset/sigprocmask/signal with valid arguments are always sound.
    unsafe {
        let mut set = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
        libc::sigemptyset(set.as_mut_ptr());
        libc::sigprocmask(libc::SIG_SETMASK, set.as_ptr(), std::ptr::null_mut());
        for signal in [
            libc::SIGPIPE,
            libc::SIGINT,
            libc::SIGTERM,
            libc::SIGHUP,
            libc::SIGQUIT,
            libc::SIGTSTP,
            libc::SIGTTIN,
            libc::SIGTTOU,
            libc::SIGCHLD,
            libc::SIGWINCH,
        ] {
            libc::signal(signal, libc::SIG_DFL);
        }
    }
}
