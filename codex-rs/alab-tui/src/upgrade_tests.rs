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
    for other in [
        "#!/bin/bash\nexec '/store/codex/b/bin/codex' \"$@\"\n",
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
    let shared: &'static codex_alab_core::session::Shared =
        Box::leak(Box::new(codex_alab_core::session::Shared::new()));
    shared.set_launch(launch());
    assert!(upgrade::exec_plan(shared).is_err(), "no primary thread yet");
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
