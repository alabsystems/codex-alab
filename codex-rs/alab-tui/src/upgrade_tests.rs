// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::upgrade;
use crate::upgrade::BuildId;
use clap::CommandFactory;
use clap::Parser;
use codex_alab_core::session::Launch;
use pretty_assertions::assert_eq;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

const THREAD: &str = "019a0000-0000-7000-8000-0000000000bb";

/// Displayed installed versions belong to the executable they name; an unrelated,
/// oversized or non-regular manifest supplies no version and cannot block the reader.
#[test]
fn installed_version_is_bounded_and_bound_to_its_entrypoint() {
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let exe = dir.path().join("bin/codex");
    executable(&exe, b"captured build; never executed");
    let manifest = dir.path().join("codex-package.json");
    let valid =
        r#"{"layoutVersion":1,"entrypoint":"bin/codex","version":"0.164.0-dev.12521+g3d3eb12d8"}"#;
    std::fs::write(&manifest, valid).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        upgrade::package_version(&exe).as_deref(),
        Some("0.164.0-dev.12521+g3d3eb12d8")
    );
    for invalid in [
        valid.replace("bin/codex", "bin/another"),
        valid.replace("bin/codex", "../codex"),
        valid.replace("0.164.0-dev.12521+g3d3eb12d8", "bad\\nversion"),
        valid.replace("0.164.0-dev.12521+g3d3eb12d8", "0.164.0 bad"),
        format!("{valid}{}", " ".repeat(64 * 1024)),
    ] {
        std::fs::write(&manifest, invalid).unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(upgrade::package_version(&exe), None);
    }
    std::fs::remove_file(&manifest).unwrap_or_else(|err| panic!("{err}"));
    std::fs::create_dir(&manifest).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(upgrade::package_version(&exe), None);
    std::fs::remove_dir(&manifest).unwrap_or_else(|err| panic!("{err}"));
    use std::os::unix::ffi::OsStrExt;
    let fifo = std::ffi::CString::new(manifest.as_os_str().as_bytes())
        .unwrap_or_else(|err| panic!("{err}"));
    // SAFETY: a valid NUL-terminated path in this test's private directory.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert_eq!(upgrade::package_version(&exe), None);
    assert_eq!(upgrade::named_build(&manifest), None);
}

/// atpkg's shim, as `pkg/agents/codex` holds it (the store path shortened).
const SHIM: &str = "#!/bin/sh\n\
# atpkg shim \u{2014} exec so the tool authenticates at its real path.\n\
case \"$1\" in\n\
  update)\n\
    __atpkg='/Applications/aterm.app/Contents/MacOS/atpkg'\n\
    if [ -x \"$__atpkg\" ]; then exec \"$__atpkg\" __selfupdate 'codex' -- \"$@\"; fi\n\
    ;;\n\
esac\n\
exec '/store/codex/1000000000163000000/bin/codex' \"$@\"\n";

#[test]
fn an_atpkg_shim_names_the_build_it_execs_and_nothing_else_does() {
    assert_eq!(
        upgrade::shim_target(SHIM),
        Some(PathBuf::from("/store/codex/1000000000163000000/bin/codex"))
    );
    assert_eq!(
        upgrade::shim_target(
            "#!/bin/sh\nexec '/homes/o'\\''hara/store/codex/b/bin/codex' \"$@\"\n"
        ),
        Some(PathBuf::from("/homes/o'hara/store/codex/b/bin/codex"))
    );
    for other in [
        "#!/bin/bash\nexec '/store/codex/b/bin/codex' \"$@\"\n",
        "#!/bin/shadow\nexec '/store/codex/b/bin/codex' \"$@\"\n",
        "#!/bin/sh/not-an-interpreter\nexec '/store/codex/b/bin/codex' \"$@\"\n",
        "#!/bin/sh\r\nexec '/store/codex/b/bin/codex' \"$@\"\n",
        "#!/bin/sh -e\nexec '/store/codex/b/bin/codex' \"$@\"\n",
        "#!/bin/sh\nexec 'relative/codex' \"$@\"\n",
        "#!/bin/sh\nexec '/a'\"'\"'b/codex' \"$@\"\n",
        "#!/bin/sh\nexec '/store/codex/b/bin/codex' \"$@\"\necho after\n",
        "#!/bin/sh\nexec /store/codex/b/bin/codex \"$@\"\n",
    ] {
        assert_eq!(upgrade::shim_target(other), None, "{other}");
    }
}

fn executable(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap_or_else(|| panic!("a parent")))
        .unwrap_or_else(|err| panic!("{err}"));
    std::fs::write(path, body).unwrap_or_else(|err| panic!("{err}"));
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|err| panic!("{err}"));
}

fn point(link: &Path, target: &Path) {
    let next = link.with_extension("next");
    let _ = std::fs::remove_file(&next);
    std::os::unix::fs::symlink(target, &next).unwrap_or_else(|err| panic!("{err}"));
    // A rename, as atpkg re-points a twin: the link is never missing in between.
    std::fs::rename(&next, link).unwrap_or_else(|err| panic!("{err}"));
}

fn id(path: &Path) -> BuildId {
    upgrade::build_id(path).unwrap_or_else(|| panic!("{} has an identity", path.display()))
}

/// A target named in a broken shim must not make the TUI leave a working image.
#[test]
fn a_malformed_interpreter_cannot_trigger_a_build_handoff() {
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let current = dir.path().join("current");
    let next = dir.path().join("next");
    let shim = dir.path().join("launcher");
    executable(&current, b"captured current image; never executed");
    executable(&next, b"#!/bin/sh\nprintf 'next build\\n'\n");
    let running = id(&current);
    executable(
        &shim,
        format!(
            "#!/bin/sh/not-an-interpreter\nexec '{}' \"$@\"\n",
            next.display()
        )
        .as_bytes(),
    );
    assert!(
        std::process::Command::new(&shim).output().is_err(),
        "the fixture's interpreter cannot be started"
    );
    assert!(
        !upgrade::names_other_build(&shim, running),
        "a launcher the OS cannot start must not trigger teardown"
    );
    assert_eq!(upgrade::named_build(&shim), None);

    executable(
        &shim,
        format!(
            "#!/bin/sh\n#{}\nexec '{}' \"$@\"\n",
            "x".repeat(64 * 1024),
            next.display()
        )
        .as_bytes(),
    );
    assert_eq!(upgrade::named_build(&shim), None);

    executable(
        &shim,
        format!("#!/bin/sh\nexec '{}' \"$@\"\n", next.display()).as_bytes(),
    );
    assert!(upgrade::names_other_build(&shim, running));
    let output = std::process::Command::new(&shim)
        .output()
        .unwrap_or_else(|err| panic!("start a valid shim: {err}"));
    assert!(output.status.success());
    assert_eq!(output.stdout, b"next build\n");
}

#[test]
fn the_trigger_is_the_launch_path_naming_another_file_than_this_image_runs() {
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let a = dir.path().join("store/a/bin/codex");
    let b = dir.path().join("store/b/bin/codex");
    executable(&a, b"\x7fELF build a");
    executable(&b, b"\x7fELF build b");
    let twin = dir.path().join("agents/codex");
    std::fs::create_dir_all(twin.parent().unwrap_or_else(|| panic!("a parent")))
        .unwrap_or_else(|err| panic!("{err}"));
    point(&twin, &a);
    let running = id(&a);

    assert!(!upgrade::names_other_build(&twin, running), "same build");
    point(&twin, &b);
    assert!(upgrade::names_other_build(&twin, running), "re-pointed");
    assert_eq!(upgrade::named_build(&twin), std::fs::canonicalize(&b).ok());

    // A shim names the build it execs.
    let shim = dir.path().join("agents/codex-shim");
    executable(
        &shim,
        format!("#!/bin/sh\nexec '{}' \"$@\"\n", a.display()).as_bytes(),
    );
    assert!(!upgrade::names_other_build(&shim, running));
    executable(
        &shim,
        format!("#!/bin/sh\nexec '{}' \"$@\"\n", b.display()).as_bytes(),
    );
    assert!(upgrade::names_other_build(&shim, running));

    // Negative controls: nothing to move onto, so no upgrade.
    point(&twin, &dir.path().join("store/gone/bin/codex"));
    assert!(
        !upgrade::names_other_build(&twin, running),
        "a dangling twin"
    );
    let plain = dir.path().join("store/c/bin/codex");
    executable(&plain, b"\x7fELF build c");
    std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644))
        .unwrap_or_else(|err| panic!("{err}"));
    point(&twin, &plain);
    assert!(
        !upgrade::names_other_build(&twin, running),
        "not executable"
    );
    executable(&shim, b"#!/bin/sh\nexec \"$HOME/codex\" \"$@\"\n");
    assert!(
        !upgrade::names_other_build(&shim, running),
        "an unreadable shim"
    );
    // The same file replaced in place under the same path is another build too.
    let in_place = dir.path().join("store/d/bin/codex");
    executable(&in_place, b"\x7fELF build d");
    let before = id(&in_place);
    let replacement = dir.path().join("store/d/bin/codex.new");
    executable(&replacement, b"\x7fELF build d2");
    std::fs::rename(&replacement, &in_place).unwrap_or_else(|err| panic!("{err}"));
    assert!(upgrade::names_other_build(&in_place, before));

    // A fallback captured the actual A image, while an admitted managed path can
    // still be absent. Repair to B changes the monitor target, not A's identity.
    let route = dir.path().join("store/codex/exec/codex");
    let captured = Launch {
        argv0: Some(a.clone()),
        current_exe: Some(a.clone()),
        exe_id: Some((running.dev, running.ino)),
        managed_argv0: Some(route.clone()),
        ..launch()
    };
    assert!(!upgrade::launch_names_other_build(&captured));
    std::fs::create_dir_all(route.parent().unwrap_or_else(|| panic!("a parent")))
        .unwrap_or_else(|err| panic!("{err}"));
    point(&route, &a);
    assert!(!upgrade::launch_names_other_build(&captured));
    point(&route, &b);
    assert!(upgrade::launch_names_other_build(&captured));
    assert_eq!(captured.argv0, Some(a.clone()));
    assert_eq!(captured.current_exe, Some(a));
    assert_eq!(captured.exe_id, Some((running.dev, running.ino)));
    let plan =
        upgrade::plan_for(&captured, Cli::command, THREAD).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(plan.path, route);
    assert_eq!(
        plan.argv.first().map(OsString::as_os_str),
        Some(plan.path.as_os_str())
    );
    assert_eq!(plan.env, captured.vars);
    let direct = Launch {
        managed_argv0: None,
        ..captured
    };
    assert!(
        !upgrade::launch_names_other_build(&direct),
        "intentional raw A stays raw"
    );
}

#[derive(Parser)]
#[command(bin_name = "codex", subcommand_negates_reqs = true)]
struct Cli {
    #[arg(short = 'c', long = "config", action = clap::ArgAction::Append)]
    raw_overrides: Vec<String>,
    #[arg(long, short = 'm')]
    model: Option<String>,
    #[arg(value_name = "PROMPT")]
    prompt: Option<String>,
    #[command(subcommand)]
    subcommand: Option<Sub>,
}

#[derive(clap::Subcommand)]
enum Sub {
    Resume {
        session_id: Option<String>,
        #[arg(long)]
        last: bool,
    },
}

fn launch() -> Launch {
    Launch {
        args: ["codex", "-m", "gpt-5", "-c", "a=1", "fix the build"]
            .map(OsString::from)
            .to_vec(),
        vars: vec![
            (OsString::from("PATH"), OsString::from("/twin:/usr/bin")),
            (OsString::from("CODEX_HOME"), OsString::from("/h/.codex")),
            (
                OsString::from("ATERM_PARENT_SESSION_ID"),
                OsString::from("s-1"),
            ),
        ],
        cwd: Some(PathBuf::from("/work")),
        current_exe: Some(PathBuf::from("/store/a/bin/codex")),
        exe_id: Some((1, 2)),
        argv0: Some(PathBuf::from("/twin/codex")),
        pid: 4242,
        ..Default::default()
    }
}

#[test]
fn the_exec_runs_the_launch_path_with_the_resume_argv_the_launch_env_and_cwd() {
    let plan =
        upgrade::plan_for(&launch(), Cli::command, THREAD).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(plan.path, PathBuf::from("/twin/codex"));
    assert_eq!(
        plan.argv,
        ["/twin/codex", "-m", "gpt-5", "-c", "a=1", "resume", THREAD]
            .map(OsString::from)
            .to_vec(),
        "the prompt is dropped; every flag is kept; the session is resumed"
    );
    assert_eq!(plan.env, launch().vars, "the launch environment, verbatim");
    assert_eq!(plan.cwd, Some(PathBuf::from("/work")));

    let unknown = Launch {
        argv0: None,
        ..launch()
    };
    assert!(upgrade::plan_for(&unknown, Cli::command, THREAD).is_err());
}

#[test]
fn no_upgrade_is_planned_until_the_primary_rollout_is_on_disk() {
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared: &'static codex_alab_core::session::Shared =
        Box::leak(Box::new(codex_alab_core::session::Shared::new()));
    shared.set_launch(Launch {
        cwd: Some(dir.path().to_path_buf()),
        ..launch()
    });
    crate::resume_argv::set_command(Cli::command);
    assert_eq!(
        upgrade::exec_plan(shared).err().as_deref(),
        Some("no primary thread")
    );
}

/// Even a materialized, resumable thread must keep its current session when the
/// launch directory is missing or replaced. Planning never changes the process cwd.
#[test]
fn the_upgrade_preflight_requires_a_known_accessible_launch_directory() {
    use codex_alab_core::session::Frame;
    use codex_alab_core::session::ProgramState;
    use codex_alab_core::session::Shared;

    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let cwd = dir.path().join("work");
    std::fs::create_dir(&cwd).unwrap_or_else(|err| panic!("{err}"));
    let rollout = dir.path().join("rollout.jsonl");
    std::fs::write(&rollout, b"materialized thread").unwrap_or_else(|err| panic!("{err}"));
    crate::resume_argv::set_command(Cli::command);
    let check = |cwd: Option<PathBuf>| {
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        shared.set_launch(Launch { cwd, ..launch() });
        shared.observe(
            &Frame {
                primary_thread: Some(THREAD),
                displayed_thread: Some(THREAD),
                rollout_path: Some(&rollout),
                cwd: dir.path(),
                model: "gpt-5",
                codex_home: dir.path(),
                embedded: true,
                state: ProgramState::Idle,
                draft_empty: true,
                queued_input: false,
                popup_open: false,
                editor_open: false,
                pending_user_turn: false,
            },
            0,
        );
        upgrade::exec_plan(shared)
    };

    let before = std::env::current_dir().unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        check(Some(cwd.clone()))
            .unwrap_or_else(|why| panic!("{why}"))
            .cwd,
        Some(cwd.clone())
    );
    let refused = |cwd| {
        check(cwd)
            .err()
            .unwrap_or_else(|| panic!("unavailable cwd was accepted"))
    };
    std::fs::remove_dir(&cwd).unwrap_or_else(|err| panic!("{err}"));
    assert!(refused(Some(cwd.clone())).contains("working directory"));
    std::fs::write(&cwd, b"a file replaced the directory").unwrap_or_else(|err| panic!("{err}"));
    assert!(refused(Some(cwd)).contains("not a directory"));
    assert!(refused(None).contains("working directory is unknown"));
    assert_eq!(
        std::env::current_dir().ok().as_deref(),
        Some(before.as_path())
    );
}

#[test]
fn an_upgrade_is_requested_only_through_the_tuis_own_exit() {
    let shared: &'static codex_alab_core::session::Shared =
        Box::leak(Box::new(codex_alab_core::session::Shared::new()));
    assert!(!upgrade::request(shared), "no TUI said how to leave");
    assert!(!shared.exec_pending());
    let left = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = std::sync::Arc::clone(&left);
    shared.set_leave(Box::new(move || {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }));
    assert!(upgrade::request(shared));
    assert!(shared.exec_pending());
    assert!(left.load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test]
async fn a_queued_upgrade_is_acknowledged_only_after_fresh_native_facts_accept_it() {
    use codex_alab_core::session::Autopilot;
    use codex_alab_core::session::Frame;
    use codex_alab_core::session::ProgramState;
    use codex_alab_core::session::Shared;

    for changed in [false, true] {
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        shared.set_autopilot(Autopilot::Active);
        shared.observe(
            &Frame {
                primary_thread: Some(THREAD),
                displayed_thread: Some(THREAD),
                rollout_path: None,
                cwd: Path::new("/work"),
                model: "gpt-5",
                codex_home: Path::new("/home"),
                embedded: true,
                state: ProgramState::Idle,
                draft_empty: true,
                queued_input: false,
                popup_open: false,
                editor_open: false,
                pending_user_turn: false,
            },
            10,
        );
        let checked = shared.upgrade_generation();
        let (queue, mut events) = tokio::sync::mpsc::unbounded_channel();
        shared.set_upgrade_leave(Box::new(move |request| {
            let _ = queue.send(request);
        }));
        let waiter = tokio::spawn(upgrade::request_checked(shared, move || {
            shared.commit_upgrade(checked)
        }));
        let request = events
            .recv()
            .await
            .unwrap_or_else(|| panic!("queued upgrade"));
        assert!(
            !waiter.is_finished(),
            "enqueueing alone never commits leave"
        );
        if changed {
            shared.human_input(20);
        }
        assert_eq!(request.decide(true), !changed);
        assert_eq!(waiter.await.unwrap_or_else(|err| panic!("{err}")), !changed);
        assert_eq!(shared.exec_pending(), !changed);
    }
}

#[tokio::test(start_paused = true)]
async fn cancelled_or_unacknowledged_requests_cannot_exit_when_their_late_event_arrives() {
    use codex_alab_core::session::Shared;
    for cancel in [true, false] {
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let (queue, mut events) = tokio::sync::mpsc::unbounded_channel();
        shared.set_upgrade_leave(Box::new(move |request| {
            let _ = queue.send(request);
        }));
        let waiter = tokio::spawn(upgrade::request_checked(shared, || {
            panic!("cancelled request cannot commit")
        }));
        let request = events
            .recv()
            .await
            .unwrap_or_else(|| panic!("queued upgrade"));
        if cancel {
            waiter.abort();
            assert!(waiter.await.is_err());
        } else {
            tokio::time::advance(std::time::Duration::from_secs(60)).await;
            assert!(!waiter.await.unwrap_or_else(|err| panic!("{err}")));
        }
        assert!(!request.decide(true));
        assert!(!shared.exec_pending());
    }
}
