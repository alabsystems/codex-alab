// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE FOREVER PROBE (review of the forever probe at codex-alab 26d37f9e91, rechecked at
//! adc376b3b9; owner, 2026-10-19, restated 2026-10-29: "if landing LAND EVERYTHING INTO REMOTE
//! MAIN NO EXCEPTIONS"): Land's repository probe reads a submodule's commits on no remote
//! (land-probe-submodule-unpushed), the session's own branches and their stash entries
//! (land-probe-head-only), every folder the session touched and the project around a submodule
//! (land-probe-last-workdir), every commit where a remote is configured but never fetched
//! (land-probe-unfetched-remote), and runs its reads beside its asker (hangs-1). Every
//! repository here is a scratch one over a `file://` bare origin; the person's global
//! configuration is a scratch home's.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::repo::*;
use crate::repo_tests::repo;
use crate::repo_tests::scratch_git;

#[path = "probe_cargo_copy_tests.rs"]
#[cfg(unix)]
mod cargo_copy_tests;

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
}

fn mkdir(path: &Path) {
    std::fs::create_dir_all(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| panic!("runtime: {err}"))
}

/// The session probe of `dirs` by a session that used `used`, with `home` and no
/// `XDG_CONFIG_HOME`.
fn read_session(dirs: &[PathBuf], home: &Path, used: &Used) -> RepoRead {
    runtime()
        .block_on(probe_session_with(dirs, home, None, used, PROBE_LIMIT))
        .read
}

/// [`read_session`]'s finding; a probe that did not complete fails the test.
fn found_in(dirs: &[PathBuf], home: &Path, used: &Used) -> Option<RepoWork> {
    match read_session(dirs, home, used) {
        RepoRead::Read(found) => found,
        RepoRead::Unread => panic!("the scratch probe of {dirs:?} did not complete"),
    }
}

/// The probe of `dir` alone by a session that used nothing else.
fn found(dir: &Path) -> Option<RepoWork> {
    found_in(&[dir.to_path_buf()], dir, &Used::default())
}

/// A commit made in `dir`, writing `name`.
fn local_commit(dir: &Path, name: &str) -> String {
    write(&dir.join(name), "x\n");
    scratch_git(dir, &["add", name]);
    scratch_git(dir, &["commit", "-q", "-m", name]);
    scratch_git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// `<tmp>/<name>/remote.git`, a bare repository, and its `file://` URL.
fn bare(tmp: &Path, name: &str) -> String {
    let at = tmp.join(name).join("remote.git");
    mkdir(&at);
    scratch_git(&at, &["init", "-q", "--bare", "."]);
    format!("file://{}", at.display())
}

/// A project at `<tmp>/app/work` with the library `<tmp>/lib/work` as its submodule
/// `vendor/lib`, both pushed to their `file://` origins.
fn app_with_lib(tmp: &Path) -> (PathBuf, PathBuf) {
    let (lib_dir, app_dir) = (tmp.join("lib"), tmp.join("app"));
    mkdir(&lib_dir);
    mkdir(&app_dir);
    let lib = repo(&lib_dir, true);
    let app = repo(&app_dir, true);
    let url = format!("file://{}", lib_dir.join("remote.git").display());
    scratch_git(
        &app,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            &url,
            "vendor/lib",
        ],
    );
    scratch_git(&app, &["commit", "-q", "-m", "lib"]);
    scratch_git(&app, &["push", "-q", "origin", "HEAD:main"]);
    (lib, app)
}

/// The submodule's checkout committed in `app` and pushed.
fn bump_pushed(app: &Path) {
    scratch_git(app, &["add", "vendor/lib"]);
    scratch_git(app, &["commit", "-q", "-m", "bump"]);
    scratch_git(app, &["push", "-q", "origin", "HEAD:main"]);
}

/// Now, in Unix seconds, less one: a session that begins now.
fn session_start() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
        .saturating_sub(1)
}

// --- land-probe-unfetched-remote (aterm's D2-5) ---

/// A REMOTE CONFIGURED BUT NEVER FETCHED HOLDS EVERY COMMIT (land-probe-unfetched-remote): a
/// clone of a new, empty repository, and a `git init` with `git remote add origin`, have a
/// remote to push to but no remote-tracking ref, so every commit on `HEAD` is one no remote
/// has. NEGATIVE CONTROLS: once pushed, nothing; a repository with no remote at all has nowhere
/// to push; a branch with no commit yet reads clean, not unread.
#[test]
fn a_configured_remote_never_fetched_holds_every_commit() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let url = bare(tmp.path(), "empty");
    scratch_git(tmp.path(), &["clone", "-q", &url, "clone"]);
    let clone = tmp.path().join("clone");
    // A clone of an empty repository, before its first commit: nothing to push yet.
    assert_eq!(
        read_session(std::slice::from_ref(&clone), &clone, &Used::default()),
        RepoRead::Read(None)
    );
    local_commit(&clone, "a.txt");
    local_commit(&clone, "b.txt");
    let w = found(&clone).unwrap_or_else(|| panic!("two commits no remote has"));
    assert_eq!((w.unpushed, w.unlanded, w.changed), (2, 0, 0), "{w:?}");
    assert!(
        w.summary().ends_with(": 2 unpushed commits"),
        "{}",
        w.summary()
    );
    scratch_git(&clone, &["push", "-q", "origin", "HEAD:main"]);
    assert_eq!(found(&clone), None, "pushed");

    let init = tmp.path().join("init");
    mkdir(&init);
    scratch_git(&init, &["init", "-q", "."]);
    let url = bare(tmp.path(), "later");
    scratch_git(&init, &["remote", "add", "origin", &url]);
    assert_eq!(
        read_session(std::slice::from_ref(&init), &init, &Used::default()),
        RepoRead::Read(None)
    );
    write(&init.join("new.txt"), "x\n");
    let w = found(&init).unwrap_or_else(|| panic!("untracked on an unborn branch"));
    assert_eq!((w.untracked, w.unpushed), (1, 0), "{w:?}");
    scratch_git(&init, &["add", "new.txt"]);
    scratch_git(&init, &["commit", "-q", "-m", "one"]);
    assert_eq!(found(&init).map(|w| w.unpushed), Some(1));
    // NEGATIVE CONTROL: no remote at all.
    let alone = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    assert_eq!(
        found(&repo(alone.path(), false)),
        None,
        "no remote configured"
    );
}

// --- land-probe-submodule-unpushed (aterm's D2-1) ---

/// A SUBMODULE'S COMMIT ON NO REMOTE IS WORK (land-probe-submodule-unpushed, case a): a commit
/// made in `vendor/lib` and recorded by a bump the superproject pushed, never pushed to the
/// submodule's own remote, is one nobody cloning the remote main can fetch, though `git status`
/// shows nothing. NEGATIVE CONTROLS: the submodule at its first record, and the same bump once
/// its commit is pushed, read clean.
#[test]
fn a_submodule_commit_on_no_remote_is_work() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let (_lib, app) = app_with_lib(tmp.path());
    assert_eq!(found(&app), None, "the submodule at a pushed record");
    let sub = app.join("vendor/lib");
    local_commit(&sub, "d.txt");
    bump_pushed(&app);
    assert_eq!(scratch_git(&app, &["status", "--porcelain"]), "");
    let w = found(&app).unwrap_or_else(|| panic!("the submodule's commit on no remote"));
    assert_eq!(
        (w.changed, w.unpushed, w.unlanded, w.sub_unpushed),
        (0, 0, 0, 1),
        "{w:?}"
    );
    assert!(
        w.summary().ends_with(": 1 unpushed commit in vendor/lib"),
        "{}",
        w.summary()
    );
    assert_eq!(w.commits_unpushed(), 1);
    scratch_git(&sub, &["push", "-q", "origin", "HEAD:main"]);
    assert_eq!(found(&app), None, "the submodule's commit pushed");
}

/// A LAGGING CHECKOUT AT A COMMIT ON NO REMOTE IS STILL WORK (land-probe-submodule-unpushed,
/// case b): the submodule's commit recorded only on a branch the superproject then left (a
/// switch that does not recurse) is a checkout the lag read drops, yet its commit is on no
/// remote. NEGATIVE CONTROL: that commit pushed, the lagging checkout reads clean.
#[test]
fn a_lagging_submodule_checkout_on_no_remote_is_work() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let (_lib, app) = app_with_lib(tmp.path());
    let sub = app.join("vendor/lib");
    scratch_git(&app, &["switch", "-q", "-c", "feat"]);
    let made = local_commit(&sub, "y.txt");
    scratch_git(&app, &["add", "vendor/lib"]);
    scratch_git(&app, &["commit", "-q", "-m", "feat: bump"]);
    scratch_git(
        &app,
        &["-c", "submodule.recurse=false", "switch", "-q", "main"],
    );
    assert_eq!(
        scratch_git(&app, &["status", "--porcelain"]),
        " M vendor/lib\n"
    );
    let w = found(&app).unwrap_or_else(|| panic!("the lagging checkout's own commit"));
    assert_eq!((w.changed, w.sub_unpushed), (0, 1), "{w:?}");
    scratch_git(
        &sub,
        &["push", "-q", "origin", &format!("{made}:refs/heads/side")],
    );
    assert_eq!(
        found(&app),
        None,
        "its commit pushed, the checkout only lags"
    );
}

/// A SHALLOW SUBMODULE AT A FETCHED COMMIT IS NO WORK (aterm R-4): `--shallow-submodules`
/// fetches the record one commit deep, below a remote tip that does not reach it here; it came
/// from that remote and is not counted. NEGATIVE CONTROL: a commit made on it, recorded by a
/// pushed bump, is one commit on no remote.
#[test]
fn a_shallow_submodule_at_a_fetched_commit_is_no_work() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let (lib, _app) = app_with_lib(tmp.path());
    local_commit(&lib, "b.txt");
    scratch_git(&lib, &["push", "-q", "origin", "HEAD:main"]);
    let lib_bare = tmp.path().join("lib/remote.git");
    scratch_git(
        &lib_bare,
        &["config", "uploadpack.allowReachableSHA1InWant", "true"],
    );
    let url = format!("file://{}", tmp.path().join("app/remote.git").display());
    scratch_git(
        tmp.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "clone",
            "-q",
            "--recurse-submodules",
            "--shallow-submodules",
            &url,
            "clone",
        ],
    );
    let clone = tmp.path().join("clone");
    let sub = clone.join("vendor/lib");
    assert_eq!(
        scratch_git(&sub, &["rev-parse", "--is-shallow-repository"]).trim(),
        "true"
    );
    assert_eq!(
        found(&clone),
        None,
        "a shallow record that came from its remote"
    );
    local_commit(&sub, "d.txt");
    bump_pushed(&clone);
    let w = found(&clone).unwrap_or_else(|| panic!("a commit made on the shallow record"));
    assert_eq!(w.sub_unpushed, 1, "{w:?}");
}

/// A SUBMODULE READ GIT REFUSES IS NEVER CLEAN: a broken remote-tracking ref in the submodule
/// makes its count say so on its standard error, and the probe is unread. NEGATIVE CONTROL: the
/// ref removed, the same pushed submodule reads clean.
#[test]
fn a_submodule_count_git_refused_is_never_clean() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let (_lib, app) = app_with_lib(tmp.path());
    let sub = app.join("vendor/lib");
    let at = scratch_git(
        &sub,
        &["rev-parse", "--git-path", "refs/remotes/origin/broken"],
    );
    let bad = sub.join(at.trim());
    write(&bad, "not-a-commit\n");
    assert_eq!(scratch_git(&app, &["status", "--porcelain"]), "");
    assert_eq!(
        read_session(std::slice::from_ref(&app), &app, &Used::default()),
        RepoRead::Unread
    );
    std::fs::remove_file(&bad).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(found(&app), None);
}

// --- land-probe-head-only (aterm's D2-3) ---

/// WORK THE SESSION PARKED ON ANOTHER BRANCH HOLDS THE LANDING (land-probe-head-only): commits
/// on a branch it created and left, a stash made there, and a branch it left for a detached
/// `HEAD`, are counted once this worktree's `HEAD` reflog shows it moved there since the
/// session began. NEGATIVE CONTROLS: a session that began after those moves (another session's
/// work) and the same work once merged into the remote main read clean.
#[test]
fn work_this_session_parked_on_another_branch_holds_the_landing() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    let began = Used {
        since: Some(session_start()),
        ..Used::default()
    };
    let home = tmp.path();
    let dirs = [work.clone()];
    scratch_git(&work, &["switch", "-q", "-c", "fix/x"]);
    local_commit(&work, "b.txt");
    local_commit(&work, "c.txt");
    write(&work.join("a.txt"), "wip\n");
    scratch_git(&work, &["stash", "-q"]);
    scratch_git(&work, &["switch", "-q", "main"]);
    let w = found_in(&dirs, home, &began).unwrap_or_else(|| panic!("the parked branch"));
    assert_eq!((w.parked, w.stashes, w.unpushed), (2, 1, 0), "{w:?}");
    assert_eq!(w.parked_on, ["fix/x"]);
    assert!(
        w.summary()
            .contains("2 commits on fix/x not on origin/main"),
        "{}",
        w.summary()
    );
    // NEGATIVE CONTROL: a session that began after every move is not the one that made them.
    let later = Used {
        since: Some(session_start() + 3600),
        ..Used::default()
    };
    assert_eq!(
        found_in(&dirs, home, &later),
        None,
        "another session's branch"
    );
    // A detached HEAD left the branch too.
    scratch_git(&work, &["stash", "drop", "-q"]);
    scratch_git(&work, &["checkout", "-q", "--detach", "origin/main"]);
    let w = found_in(&dirs, home, &began).unwrap_or_else(|| panic!("detached"));
    assert_eq!(
        (w.parked, w.parked_on.as_slice()),
        (2, &["fix/x".to_string()][..])
    );
    // Merged into the remote main: nothing.
    scratch_git(&work, &["push", "-q", "origin", "fix/x:main"]);
    scratch_git(&work, &["fetch", "-q", "origin"]);
    assert_eq!(found_in(&dirs, home, &began), None, "landed");
}

/// THE BRANCHES THE SESSION'S COMMANDS NAMED ARE ITS OWN (land-probe-head-only): a worktree it
/// added on its own branch moves no `HEAD` of this worktree, yet the branch's commits and its
/// stash entry hold the landing. NEGATIVE CONTROL: the same worktree and branch, made by
/// another session (not named), are none of this one's (as
/// `repo_tests::a_stash_another_worktree_made_is_none_of_this_ones`).
#[test]
fn a_branch_the_sessions_commands_named_is_its_own() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    let other = tmp.path().join("side-tree");
    let other_s = other.display().to_string();
    let command = format!("git worktree add -b side {other_s}");
    scratch_git(&work, &["worktree", "add", "-q", "-b", "side", &other_s]);
    local_commit(&other, "s.txt");
    write(&other.join("a.txt"), "side\n");
    scratch_git(&other, &["stash", "-q"]);
    let argv = ["/bin/zsh".to_string(), "-lc".to_string(), command];
    let mut used = Used::default();
    assert!(used.note_command(&argv, Some(work.as_path()), None));
    assert_eq!(
        used.branches,
        [codex_alab_core::session::UsedBranch::new(
            "side",
            Some(work.as_path())
        )]
    );
    let w = found_in(std::slice::from_ref(&work), tmp.path(), &used)
        .unwrap_or_else(|| panic!("the session's own branch"));
    assert_eq!((w.parked, w.stashes), (1, 1), "{w:?}");
    // NEGATIVE CONTROL: not named, the branch is another session's.
    assert_eq!(found_in(&[work], tmp.path(), &Used::default()), None);
}

/// THE BRANCHES A COMMAND CREATES OR CHECKS OUT ([`command_reach`]): through a shell
/// wrapper's script, chained commands, `env` and git's global options; directly where `argv`
/// runs `git`. NEGATIVE CONTROLS: a branch deleted or listed, a revision expression, and a
/// command that is no git name none.
#[test]
fn the_branches_a_command_names() {
    let names = |argv: &[String]| -> Vec<String> {
        command_reach(argv, Some(Path::new("/work")), None)
            .branches
            .into_iter()
            .map(|b| b.name)
            .collect()
    };
    let shell = |script: &str| {
        names(&[
            "/bin/zsh".to_string(),
            "-lc".to_string(),
            script.to_string(),
        ])
    };
    assert_eq!(
        shell("git switch -c fix/x && git commit -qam x; git -C ../a checkout -b 'two words'"),
        ["fix/x"]
    );
    assert_eq!(shell("env A=1 git checkout -b feat"), ["feat"]);
    assert_eq!(shell("git worktree add ../w -b side"), ["side"]);
    assert_eq!(shell("git branch topic"), ["topic"]);
    let direct: Vec<String> = ["git", "switch", "main"].map(String::from).to_vec();
    assert_eq!(names(&direct), ["main"]);
    // NEGATIVE CONTROLS.
    assert!(shell("git branch -d topic").is_empty());
    assert!(shell("git branch --list").is_empty());
    assert!(shell("git checkout HEAD~1").is_empty());
    assert!(shell("ls -la && echo git switch -c no").is_empty());
}

/// A REFLOG THE PROBE MAY NOT OPEN IS NEVER A CLEAN READING: a linked worktree whose common
/// directory lies on a share holds its `HEAD` reflog there, and the session's moves cannot be
/// read. NEGATIVE CONTROL: the same worktree over the real mount table reads clean.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_reflog_off_local_mounts_is_never_a_clean_reading() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    let linked = tmp.path().join("linked");
    let linked_s = linked.display().to_string();
    scratch_git(&work, &["worktree", "add", "-q", "--detach", &linked_s]);
    let began = Used {
        since: Some(session_start()),
        ..Used::default()
    };
    let rt = runtime();
    let probe = |table: crate::mounts::Table| {
        rt.block_on(probe_session_in(
            std::slice::from_ref(&linked),
            tmp.path(),
            None,
            &began,
            PROBE_LIMIT,
            table,
        ))
        .read
    };
    assert_eq!(probe(crate::mounts::table()), RepoRead::Read(None));
    let real_git = std::fs::canonicalize(work.join(".git")).unwrap_or_else(|err| panic!("{err}"));
    let mut table = crate::mounts::table();
    table.mounts.push(crate::mounts::Mount {
        on: real_git,
        fstype: "smbfs".to_string(),
        local: false,
        automounted: false,
    });
    assert_eq!(probe(table), RepoRead::Unread);
}

// --- land-probe-last-workdir (aterm's D2-4) ---

/// EVERY FOLDER THE SESSION TOUCHED IS READ (land-probe-last-workdir): the last command ran in
/// a clean submodule, or in an unrelated clean clone, while the project holds uncommitted
/// edits; the project's work is found, and a folder inside a submodule reads the project around
/// it. NEGATIVE CONTROL: every folder clean reads clean.
#[test]
fn every_folder_the_session_touched_is_read() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let (_lib, app) = app_with_lib(tmp.path());
    let sub = app.join("vendor/lib");
    let other_dir = tmp.path().join("other");
    mkdir(&other_dir);
    let other = repo(&other_dir, true);
    let home = tmp.path();
    let none = Used::default();
    assert_eq!(found_in(&[sub.clone(), app.clone()], home, &none), None);
    write(&app.join("a.txt"), "edited\n");
    let w = found_in(&[sub.clone(), app.clone()], home, &none)
        .unwrap_or_else(|| panic!("the project behind the submodule"));
    assert!(w.repo.ends_with("/app/work"), "{w:?}");
    assert_eq!(
        (w.changed, w.paths.as_slice()),
        (1, &["a.txt".to_string()][..])
    );
    // The submodule alone: the project around it is read too.
    let w = found_in(std::slice::from_ref(&sub), home, &none).unwrap_or_else(|| panic!("climbed"));
    assert!(w.repo.ends_with("/app/work"), "{w:?}");
    // An unrelated clean clone the last command looked at first.
    let w = found_in(&[other.clone(), app.clone()], home, &none)
        .unwrap_or_else(|| panic!("past a clean clone"));
    assert!(w.repo.ends_with("/app/work"), "{w:?}");
    // NEGATIVE CONTROL: the edit undone.
    scratch_git(&app, &["checkout", "-q", "--", "a.txt"]);
    assert_eq!(found_in(&[other, sub, app], home, &none), None);
}

/// A SESSION INSIDE A SUBMODULE STILL READS THE SUBMODULE (aterm `801e72779e`): its commit pushed
/// only to a branch of the submodule's remote, recorded by a bump the project pushed, is not on
/// that remote's main, which the project around it never counts; and a read of the project git
/// refuses is unread though the submodule reads clean. NEGATIVE CONTROLS: from the project the
/// commit is on a remote; once on the submodule's main, the session inside reads clean.
#[test]
fn a_session_inside_a_submodule_still_reads_the_submodule() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let (_lib, app) = app_with_lib(tmp.path());
    let sub = app.join("vendor/lib");
    let none = Used::default();
    scratch_git(&sub, &["switch", "-q", "-c", "feature"]);
    local_commit(&sub, "f.txt");
    scratch_git(&sub, &["push", "-q", "origin", "feature"]);
    bump_pushed(&app);
    assert_eq!(found(&app), None, "from the project");
    let w = found_in(std::slice::from_ref(&sub), tmp.path(), &none)
        .unwrap_or_else(|| panic!("the submodule's commit not on its main"));
    assert!(w.repo.ends_with("/app/work/vendor/lib"), "{w:?}");
    assert_eq!(
        (w.unlanded, w.main_ref.as_str()),
        (1, "origin/main"),
        "{w:?}"
    );
    scratch_git(&sub, &["push", "-q", "origin", "feature:main"]);
    assert_eq!(
        found_in(std::slice::from_ref(&sub), tmp.path(), &none),
        None
    );
    // A project whose submodule list git refuses to read is no clean reading.
    write(&app.join(".gitmodules"), "[submodule \"vendor/lib\"\n");
    scratch_git(&app, &["commit", "-q", "-am", "broken"]);
    scratch_git(&app, &["push", "-q", "origin", "HEAD:main"]);
    assert_eq!(read_session(&[sub], tmp.path(), &none), RepoRead::Unread);
}

/// THE FOLDERS A PROBE KEEPS ([`Probed::kept`]): each repository it read, by its top level (the
/// project and the submodule a folder was in), once; a folder in no repository and one that is
/// gone are dropped. NEGATIVE CONTROL: more folders than [`PROBE_DIRS`] read unread and keep
/// them all.
#[test]
fn a_probe_keeps_the_repositories_it_read() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let (_lib, app) = app_with_lib(tmp.path());
    let deep = app.join("vendor/lib");
    let plain = tmp.path().join("plain");
    mkdir(&plain);
    let gone = tmp.path().join("gone");
    let probed = runtime().block_on(probe_session_with(
        &[deep.clone(), plain, gone, app.clone()],
        tmp.path(),
        None,
        &Used::default(),
        PROBE_LIMIT,
    ));
    assert_eq!(probed.read, RepoRead::Read(None));
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(probed.kept, [canon(&app), canon(&deep)]);
    // NEGATIVE CONTROL: past the bound.
    let many: Vec<PathBuf> = (0..=PROBE_DIRS)
        .map(|i| tmp.path().join(format!("d{i}")))
        .collect();
    let past = runtime().block_on(probe_session_with(
        &many,
        tmp.path(),
        None,
        &Used::default(),
        PROBE_LIMIT,
    ));
    assert_eq!((past.read, past.kept), (RepoRead::Unread, many));
}

// --- hangs-1 ---

/// A STALLED FILE SYSTEM STEP NEVER HOLDS THE ASKER (hangs-1): the probe's reads run beside it,
/// so a synchronous step that blocks (a stat on a stalled share; here a stall that waits for the
/// test) holds its reader alone, and the asker has an unread answer at the limit while the
/// stall still waits. NEGATIVE CONTROL: the same probe with a stall that returns at once finds
/// the work.
#[test]
fn a_stalled_file_system_step_never_holds_the_asker() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    write(&work.join("new.txt"), "x\n");
    let (release, released) = std::sync::mpsc::channel::<()>();
    let released = Arc::new(std::sync::Mutex::new(released));
    let (said, heard) = std::sync::mpsc::channel::<bool>();
    let stall: Stall = Arc::new(move || {
        let waited = released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            // A one-minute hang detector, never a budget.
            .recv_timeout(Duration::from_secs(60));
        let _ = said.send(waited.is_ok());
    });
    let probed = runtime().block_on(probe_stalled(
        std::slice::from_ref(&work),
        tmp.path(),
        Duration::from_secs(1),
        stall,
    ));
    assert_eq!(probed.read, RepoRead::Unread, "answered at its limit");
    assert!(
        heard.try_recv().is_err(),
        "the asker had its answer while the stall still waited"
    );
    release.send(()).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        heard.recv_timeout(Duration::from_secs(60)),
        Ok(true),
        "the stall ended on its release"
    );
    // NEGATIVE CONTROL: a stall that returns at once.
    let quick: Stall = Arc::new(|| {});
    let probed = runtime().block_on(probe_stalled(&[work], tmp.path(), PROBE_LIMIT, quick));
    assert!(
        matches!(&probed.read, RepoRead::Read(Some(w)) if w.untracked == 1),
        "{probed:?}"
    );
}

/// A SUBMODULE CANDIDATE IS NO LINK (hangs-1): a tracked link retargeted at a folder holding a
/// `.git` lists as ` M`, and following it to look for that `.git` would stat whatever share it
/// leads onto; it is no submodule. NEGATIVE CONTROLS: a real folder holding `.git` is one, and a
/// folder without one, or a file, is not.
#[cfg(unix)]
#[test]
fn a_submodule_candidate_is_no_link() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let root = tmp.path();
    let target = root.join("elsewhere");
    mkdir(&target.join(".git"));
    std::os::unix::fs::symlink(&target, root.join("assets")).unwrap_or_else(|err| panic!("{err}"));
    assert!(
        !submodule_candidate(root, "assets"),
        "a link onto a folder with .git"
    );
    assert!(submodule_candidate(root, "elsewhere"), "a real checkout");
    mkdir(&root.join("plain"));
    write(&root.join("file"), "x\n");
    assert!(!submodule_candidate(root, "plain"));
    assert!(!submodule_candidate(root, "file"));
}

// --- the scope the probe reads (land-probe-last-workdir, land-probe-head-only) ---

/// An autopilot over a fresh shared state, and a thread it keeps.
fn autopilot_thread() -> (
    Arc<crate::Autopilot>,
    &'static codex_alab_core::session::Shared,
    codex_extension_api::ExtensionData,
    Arc<crate::extension::ThreadRt>,
) {
    codex_alab_core::activate();
    let shared: &'static codex_alab_core::session::Shared =
        Box::leak(Box::new(codex_alab_core::session::Shared::new()));
    let sup = crate::Supervisor::new(crate::SupervisorOptions {
        shared,
        endpoint: Arc::new(|| None),
        client: codex_alab_aterm::Options::default(),
    });
    let autopilot =
        crate::Autopilot::new(sup, None, None, crate::decide::Timing::default(), |_| true);
    let (store, rt) = scope_thread();
    autopilot.keep_thread(&rt);
    (autopilot, shared, store, rt)
}

fn scope_thread() -> (
    codex_extension_api::ExtensionData,
    Arc<crate::extension::ThreadRt>,
) {
    let id = codex_protocol::ThreadId::new();
    let store = codex_extension_api::ExtensionData::new(id.to_string());
    store.insert(crate::extension::ThreadRt::new(
        id,
        None,
        false,
        Duration::ZERO,
    ));
    let rt = store
        .get::<crate::extension::ThreadRt>()
        .unwrap_or_else(|| panic!("inserted"));
    (store, rt)
}

/// Command `argv` started by the thread of `store` in folder `dir`.
async fn command(
    autopilot: &crate::Autopilot,
    store: &codex_extension_api::ExtensionData,
    dir: &str,
    argv: &[&str],
) {
    use codex_extension_api::CommandStartInput;
    use codex_extension_api::ToolLifecycleContributor;
    let cwd = codex_utils_path_uri::PathUri::parse(&format!("file://{dir}"))
        .unwrap_or_else(|err| panic!("{dir}: {err}"));
    let session = codex_extension_api::ExtensionData::new("session");
    let turn = codex_extension_api::ExtensionData::new("turn-1");
    let command: Vec<String> = argv.iter().map(ToString::to_string).collect();
    autopilot
        .on_command_start(CommandStartInput {
            session_store: &session,
            thread_store: store,
            turn_store: &turn,
            turn_id: "turn-1",
            call_id: "call-1",
            command: &command,
            cwd: &cwd,
            file_system: codex_exec_server::LOCAL_FS.as_ref(),
        })
        .await;
}

/// An edit of the thread of `store` that changed `paths` (the second of a pair is where an
/// update moved the first).
async fn edit(
    autopilot: &crate::Autopilot,
    store: &codex_extension_api::ExtensionData,
    paths: &[(&str, Option<&str>)],
) {
    use codex_extension_api::TurnLifecycleContributor;
    use codex_protocol::protocol::FileChange;
    let changes = paths
        .iter()
        .map(|(path, to)| {
            let change = match to {
                Some(to) => FileChange::Update {
                    unified_diff: String::new(),
                    move_path: Some(PathBuf::from(to)),
                },
                None => FileChange::Add {
                    content: String::new(),
                },
            };
            (PathBuf::from(path), change)
        })
        .collect();
    let item = codex_protocol::items::TurnItem::FileChange(codex_protocol::items::FileChangeItem {
        id: "patch-1".to_string(),
        changes,
        status: None,
        auto_approved: None,
        stdout: None,
        stderr: None,
    });
    let turn = codex_extension_api::ExtensionData::new("turn-1");
    autopilot.on_item_completed(store, &turn, &item).await;
}

/// A turn of the thread of `store` starts.
async fn turn_starts(autopilot: &crate::Autopilot, store: &codex_extension_api::ExtensionData) {
    use codex_extension_api::TurnLifecycleContributor;
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5".to_string(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    let session = codex_extension_api::ExtensionData::new("session");
    let turn = codex_extension_api::ExtensionData::new("turn-1");
    autopilot
        .on_turn_start(codex_extension_api::TurnStartInput {
            turn_id: turn.level_id(),
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &session,
            thread_store: store,
            turn_store: &turn,
        })
        .await;
}

fn paths(dirs: &[&str]) -> Vec<PathBuf> {
    dirs.iter().map(PathBuf::from).collect()
}

/// EVERY FOLDER THE SESSION TOUCHED IS IN ITS SCOPE (land-probe-last-workdir): each command's
/// folder and each folder an edit landed in (a moved file's both), newest last, and the record
/// carries it. NEGATIVE CONTROLS: another thread's command leaves it, and an edit's relative path
/// names no folder.
#[tokio::test]
async fn the_scope_is_every_folder_the_threads_commands_and_edits_touched() {
    let (autopilot, shared, store, rt) = autopilot_thread();
    let (other, _other_rt) = scope_thread();
    command(&autopilot, &store, "/work/a", &["git", "status"]).await;
    command(&autopilot, &store, "/work/b", &["git", "status"]).await;
    edit(
        &autopilot,
        &store,
        &[
            ("/work/c/x.rs", None),
            ("/work/d/y.rs", Some("/work/e/y.rs")),
        ],
    )
    .await;
    command(&autopilot, &store, "/work/a", &["git", "status"]).await;
    // NEGATIVE CONTROLS: another thread's command, and a relative path.
    command(&autopilot, &other, "/elsewhere", &["git", "status"]).await;
    edit(&autopilot, &store, &[("relative/z.rs", None)]).await;
    let scope = rt.probe_scope();
    assert_eq!(
        scope.dirs,
        paths(&["/work/b", "/work/c", "/work/d", "/work/e", "/work/a"])
    );
    assert_eq!(shared.probe_scope(rt.key()), Some(scope));
}

/// A BRANCH THE SESSION'S COMMANDS NAMED IS IN ITS SCOPE (land-probe-head-only), newest first,
/// a shell script's included. NEGATIVE CONTROL: a command that names none adds none.
#[tokio::test]
async fn a_branch_a_command_names_is_in_the_scope() {
    let (autopilot, _shared, store, rt) = autopilot_thread();
    command(
        &autopilot,
        &store,
        "/work",
        &["git", "checkout", "-b", "feat"],
    )
    .await;
    command(
        &autopilot,
        &store,
        "/work",
        &["bash", "-lc", "git switch -c fix && git status"],
    )
    .await;
    // NEGATIVE CONTROL: a command that names no branch.
    command(&autopilot, &store, "/work", &["git", "log", "--oneline"]).await;
    let named =
        |name: &str| codex_alab_core::session::UsedBranch::new(name, Some(Path::new("/work")));
    assert_eq!(rt.probe_scope().branches, vec![named("fix"), named("feat")]);
}

/// THE SESSION BEGINS AT THE THREAD'S FIRST TURN (land-probe-head-only): its start is stamped
/// once, and a later turn keeps it. NEGATIVE CONTROL: a scope the record carried with an earlier
/// start keeps the earlier one.
#[tokio::test]
async fn the_session_begins_at_the_threads_first_turn() {
    let (autopilot, shared, store, rt) = autopilot_thread();
    let before = session_start();
    turn_starts(&autopilot, &store).await;
    let since = rt
        .probe_scope()
        .since
        .unwrap_or_else(|| panic!("the first turn stamps the start"));
    assert!(since >= before && since <= session_start() + 1, "{since}");
    turn_starts(&autopilot, &store).await;
    assert_eq!(rt.probe_scope().since, Some(since), "a later turn keeps it");
    // NEGATIVE CONTROL: a carried scope that began earlier (a start stamped at every turn would
    // write over it).
    let (store, rt) = scope_thread();
    autopilot.keep_thread(&rt);
    let carried = codex_alab_core::session::ProbeScope {
        since: Some(1_000),
        ..Default::default()
    };
    shared.carry_probe_scope(rt.key(), &carried);
    autopilot.seed_scope(&rt);
    turn_starts(&autopilot, &store).await;
    assert_eq!(rt.probe_scope().since, Some(1_000));
}

/// A SCOPE THE RECORD CARRIED IS THE NEW MEMORY'S (land-probe-head-only, land-probe-last-workdir):
/// a re-exec's or relaunch's folders, branches and start are merged into what this memory began,
/// older first. NEGATIVE CONTROL: another session's carried scope is not.
#[tokio::test]
async fn a_carried_scope_is_merged_into_the_new_memory() {
    let (autopilot, shared, store, rt) = autopilot_thread();
    command(&autopilot, &store, "/work/new", &["git", "switch", "now"]).await;
    let carried = codex_alab_core::session::ProbeScope {
        dirs: paths(&["/work/old"]),
        overflow: false,
        since: Some(1_000),
        branches: vec![codex_alab_core::session::UsedBranch::new(
            "then",
            Some(Path::new("/work/old")),
        )],
    };
    shared.carry_probe_scope(rt.key(), &carried);
    // NEGATIVE CONTROL: another session's.
    shared.carry_probe_scope(
        "019a0000-0000-7000-8000-0000000000ff",
        &codex_alab_core::session::ProbeScope {
            dirs: paths(&["/elsewhere"]),
            ..Default::default()
        },
    );
    autopilot.seed_scope(&rt);
    let scope = rt.probe_scope();
    assert_eq!(scope.dirs, paths(&["/work/old", "/work/new"]));
    let names: Vec<&str> = scope.branches.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["now", "then"]);
    assert_eq!(scope.since, Some(1_000));
    assert_eq!(shared.probe_scope(rt.key()), Some(scope));
}

/// A PROBE KEEPS ITS REPOSITORIES IN THE SCOPE (land-probe-last-workdir): the folders it saw
/// give way to the ones it kept, and a folder touched since it began stays; past the bound the
/// scope overflows, and an overflowed scope is never read clean. NEGATIVE CONTROL: a probe that
/// kept what it saw moves nothing.
#[tokio::test]
async fn a_probe_keeps_its_repositories_in_the_scope() {
    let (autopilot, _shared, store, rt) = autopilot_thread();
    for dir in ["/work/a/src", "/work/a/docs", "/work/b"] {
        command(&autopilot, &store, dir, &["git", "status"]).await;
    }
    let seen = paths(&["/work/a/src", "/work/a/docs"]);
    autopilot.compact_scope(&rt, &seen, &paths(&["/work/a"]));
    assert_eq!(rt.probe_scope().dirs, paths(&["/work/a", "/work/b"]));
    assert!(!rt.probe_scope().overflow);
    // NEGATIVE CONTROL: a probe that kept what it saw.
    let before = rt.probe_scope();
    autopilot.compact_scope(
        &rt,
        &before.dirs,
        &before.dirs.iter().rev().cloned().collect::<Vec<_>>(),
    );
    assert_eq!(rt.probe_scope(), before);
    // Past the bound.
    let many: Vec<PathBuf> = (0..=codex_alab_core::session::PROBE_SCOPE_DIRS)
        .map(|n| PathBuf::from(format!("/work/many/{n}")))
        .collect();
    autopilot.compact_scope(&rt, &[], &many);
    let scope = rt.probe_scope();
    assert!(scope.overflow);
    assert_eq!(scope.dirs.len(), codex_alab_core::session::PROBE_SCOPE_DIRS);
}

/// The scope's own bounds (`ProbeScope::touch`, `ProbeScope::merge`): a folder touched again
/// moves to the newest end; a new one past the bound overflows; a merge takes the earlier start
/// and an overflow of either. NEGATIVE CONTROL: touching the newest folder again moves nothing.
#[test]
fn the_scope_keeps_its_bounds() {
    use codex_alab_core::session::PROBE_SCOPE_DIRS;
    use codex_alab_core::session::ProbeScope;
    let mut scope = ProbeScope::default();
    assert!(scope.touch(Path::new("/a")));
    assert!(scope.touch(Path::new("/b")));
    assert!(scope.touch(Path::new("/a")));
    assert_eq!(scope.dirs, paths(&["/b", "/a"]));
    // NEGATIVE CONTROL: the newest again.
    assert!(!scope.touch(Path::new("/a")));
    for n in 0..PROBE_SCOPE_DIRS {
        scope.touch(Path::new(&format!("/n/{n}")));
    }
    assert_eq!(scope.dirs.len(), PROBE_SCOPE_DIRS);
    assert!(scope.overflow);
    let mut fresh = ProbeScope {
        since: Some(9),
        ..Default::default()
    };
    assert!(fresh.merge(&ProbeScope {
        since: Some(3),
        overflow: true,
        ..Default::default()
    }));
    assert_eq!(fresh.since, Some(3));
    assert!(fresh.overflow);
    assert!(!fresh.merge(&ProbeScope::default()));
}

/// A SCOPE THAT OVERFLOWED IS NEVER READ CLEAN (land-probe-last-workdir): a session that touched
/// more folders than its scope keeps has folders the probe no longer names, so where every folder
/// it still names is clean the probe is unread; work found is still named. NEGATIVE CONTROL: the
/// same clean folders under a scope that kept them all read clean.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scope_that_overflowed_is_never_read_clean() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    let dir = work.display().to_string();
    let (autopilot, _shared, store, rt) = autopilot_thread();
    command(&autopilot, &store, &dir, &["git", "status"]).await;
    // NEGATIVE CONTROL: every folder kept, all clean.
    assert_eq!(
        autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await,
        RepoRead::Read(None)
    );
    let many: Vec<PathBuf> = (0..=codex_alab_core::session::PROBE_SCOPE_DIRS)
        .map(|n| tmp.path().join(format!("gone-{n}")))
        .collect();
    autopilot.compact_scope(&rt, &[], &many);
    assert!(rt.probe_scope().overflow);
    command(&autopilot, &store, &dir, &["git", "status"]).await;
    assert_eq!(
        autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await,
        RepoRead::Unread
    );
    write(&work.join("new.txt"), "x\n");
    let found = autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await;
    assert!(
        matches!(&found, RepoRead::Read(Some(w)) if w.untracked == 1),
        "{found:?}"
    );
}

// --- the review of the forever probe at f48a215d7b ---

/// The clock's next whole second, once it comes (a minute at most: a hang detector), so what was
/// made before it predates a session that begins then.
fn next_second() -> u64 {
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    };
    let start = now();
    let detector = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        let second = now();
        if second > start {
            return second;
        }
        assert!(
            std::time::Instant::now() < detector,
            "the clock stood still"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A session that began at `since` and touched `touched` alone.
fn touched_at(since: u64, touched: &[&Path]) -> Used {
    Used {
        since: Some(since),
        touched: touched.iter().map(|d| d.to_path_buf()).collect(),
        ..Used::default()
    }
}

/// A shell wrapper's argv for `script`.
fn shell(script: &str) -> Vec<String> {
    ["/bin/zsh", "-lc", script].map(String::from).to_vec()
}

/// A CHECKOUT THE SESSION ONLY LOOKED IN HOLDS NOTHING IT DID NOT CHANGE (R2): a dependency's
/// checkout in cargo's shape (`origin` configured with no remote-tracking ref, so every commit
/// on `HEAD` counts, and `.cargo-ok` untracked), made before the session began and only touched
/// by it, holds nothing; read whole (the thread's own folder) it holds as before. NEGATIVE
/// CONTROLS: a file the session wrote there holds it (and once removed, nothing again), as does
/// a commit it made there.
#[test]
fn a_checkout_the_session_only_looked_in_holds_nothing_it_did_not_change() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let proj_dir = tmp.path().join("proj");
    mkdir(&proj_dir);
    let proj = repo(&proj_dir, true);
    let db = bare(tmp.path(), "db");
    let dep = tmp.path().join("dep");
    mkdir(&dep);
    scratch_git(&dep, &["init", "-q", "."]);
    for name in ["lib.rs", "mod.rs", "x.rs"] {
        local_commit(&dep, name);
    }
    scratch_git(&dep, &["push", "-q", &db, "HEAD:refs/heads/master"]);
    scratch_git(&dep, &["remote", "add", "origin", &db]);
    write(&dep.join(".cargo-ok"), "{\"v\":1}");
    let since = next_second();
    let looked = touched_at(since, &[&dep]);
    let home = tmp.path();
    assert_eq!(
        found_in(std::slice::from_ref(&proj), home, &looked),
        None,
        "looked in, never changed"
    );
    // Read whole, as the thread's own folder is: as before.
    let whole = Used {
        since: Some(since),
        ..Used::default()
    };
    let w = found_in(&[proj.clone(), dep.clone()], home, &whole)
        .unwrap_or_else(|| panic!("the thread's own folder"));
    assert_eq!((w.untracked, w.unpushed), (1, 3), "{w:?}");
    // NEGATIVE CONTROLS: what the session changed there holds it.
    write(&dep.join("notes.txt"), "mine\n");
    let w = found_in(std::slice::from_ref(&proj), home, &looked)
        .unwrap_or_else(|| panic!("a file the session wrote"));
    assert!(w.repo.ends_with("/dep"), "{w:?}");
    assert_eq!(w.untracked, 2, "{w:?}");
    std::fs::remove_file(dep.join("notes.txt")).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(found_in(std::slice::from_ref(&proj), home, &looked), None);
    local_commit(&dep, "mine.rs");
    let w = found_in(std::slice::from_ref(&proj), home, &looked)
        .unwrap_or_else(|| panic!("a commit the session made"));
    assert_eq!(w.unpushed, 4, "{w:?}");
}

/// A TOUCHED REPOSITORY IS CLEAN ONLY WHERE ITS RECORDS SAY THE SESSION LEFT IT ALONE (R2): a
/// stash entry the session stored (`git stash store` moves no `HEAD`) holds it; one that
/// predates it does not. NEGATIVE CONTROL, the other way: a repository that keeps no `HEAD`
/// reflog cannot say, and holds its old work.
#[test]
fn a_touched_repository_is_clean_only_where_its_records_say_so() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    let stash = |dir: &Path, text: &str| {
        write(&dir.join("a.txt"), text);
        let made = scratch_git(dir, &["stash", "create"]);
        // Named as `git stash` names an entry made on `main`.
        let message = format!("On main: {}", text.trim());
        scratch_git(dir, &["stash", "store", "-q", "-m", &message, made.trim()]);
        scratch_git(dir, &["checkout", "-q", "--", "a.txt"]);
    };
    stash(&work, "theirs\n");
    let since = next_second();
    let used = touched_at(since, &[&work]);
    assert_eq!(found_in(&[], tmp.path(), &used), None, "an old stash entry");
    stash(&work, "mine\n");
    let w = found_in(&[], tmp.path(), &used).unwrap_or_else(|| panic!("a stash entry stored"));
    assert_eq!(w.stashes, 2, "{w:?}");
    // NEGATIVE CONTROL: no `HEAD` reflog kept.
    let quiet_dir = tmp.path().join("quiet");
    mkdir(&quiet_dir);
    scratch_git(
        &quiet_dir,
        &["-c", "core.logAllRefUpdates=false", "init", "-q", "."],
    );
    scratch_git(&quiet_dir, &["config", "core.logAllRefUpdates", "false"]);
    local_commit(&quiet_dir, "q.txt");
    write(&quiet_dir.join("old.txt"), "old\n");
    let since = next_second();
    let w = found_in(&[], tmp.path(), &touched_at(since, &[&quiet_dir]))
        .unwrap_or_else(|| panic!("no reflog says the session left it alone"));
    assert_eq!(w.untracked, 1, "{w:?}");
}

/// A BRANCH NAMED IN ONE REPOSITORY NEVER READS ANOTHER'S (R3): the session created `fix` in
/// repository `a` and landed it, while repository `b` holds another session's unlanded `fix`,
/// made before this one began; `b`'s is none of this session's. NEGATIVE CONTROL: a branch whose
/// folder could not be placed is read in every repository, and finds `b`'s.
#[test]
fn a_branch_named_in_one_repository_never_reads_anothers() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let (a_dir, b_dir) = (tmp.path().join("a"), tmp.path().join("b"));
    mkdir(&a_dir);
    mkdir(&b_dir);
    let (a, b) = (repo(&a_dir, true), repo(&b_dir, true));
    scratch_git(&b, &["switch", "-q", "-c", "fix"]);
    local_commit(&b, "theirs.txt");
    scratch_git(&b, &["switch", "-q", "main"]);
    let since = next_second();
    let mut used = Used {
        since: Some(since),
        ..Used::default()
    };
    scratch_git(&a, &["switch", "-q", "-c", "fix"]);
    assert!(used.note_command(&shell("git switch -c fix"), Some(a.as_path()), None));
    local_commit(&a, "mine.txt");
    scratch_git(&a, &["push", "-q", "origin", "fix:main"]);
    scratch_git(&a, &["fetch", "-q", "origin"]);
    let home = tmp.path();
    assert_eq!(found_in(&[a.clone(), b.clone()], home, &used), None);
    // NEGATIVE CONTROL: no folder to place it by.
    let unplaced = Used {
        since: Some(since),
        branches: vec![codex_alab_core::session::UsedBranch::new("fix", None)],
        ..Used::default()
    };
    let w = found_in(&[a, b], home, &unplaced).unwrap_or_else(|| panic!("read everywhere"));
    assert!(w.repo.ends_with("/b/work"), "{w:?}");
    assert_eq!(w.parked, 1, "{w:?}");
}

/// THE BRANCH A SESSION LEFT IS ITS OWN (R5, D2-3's `from`): it began on a branch made before
/// it, committed there and switched to main; the reflog's move away names the branch. NEGATIVE
/// CONTROL: once merged into the remote main, nothing.
#[test]
fn the_branch_a_session_left_is_its_own() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    scratch_git(&work, &["switch", "-q", "-c", "feat"]);
    let since = next_second();
    let used = Used {
        since: Some(since),
        ..Used::default()
    };
    local_commit(&work, "mine.txt");
    scratch_git(&work, &["switch", "-q", "main"]);
    let dirs = [work.clone()];
    let w = found_in(&dirs, tmp.path(), &used).unwrap_or_else(|| panic!("the branch it left"));
    assert_eq!(
        (w.parked, w.parked_on.as_slice()),
        (1, &["feat".to_string()][..])
    );
    scratch_git(&work, &["push", "-q", "origin", "feat:main"]);
    scratch_git(&work, &["fetch", "-q", "origin"]);
    assert_eq!(found_in(&dirs, tmp.path(), &used), None);
}

/// A BRANCH PUSHED ONLY UNDER ITS OWN NAME IS STILL PARKED (R5, D2-3's R-2): the session pushed
/// its branch to `origin/<name>`, deleted it here and went back to main; origin's branch of that
/// name holds the commit no main has. NEGATIVE CONTROL: merged into the remote main, nothing.
#[test]
fn a_branch_pushed_only_under_its_own_name_is_still_parked() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    let mut used = Used {
        since: Some(session_start()),
        ..Used::default()
    };
    scratch_git(&work, &["switch", "-q", "-c", "feat"]);
    assert!(used.note_command(&shell("git switch -c feat"), Some(work.as_path()), None));
    local_commit(&work, "mine.txt");
    scratch_git(&work, &["push", "-q", "-u", "origin", "feat"]);
    scratch_git(&work, &["switch", "-q", "main"]);
    scratch_git(&work, &["branch", "-q", "-D", "feat"]);
    let dirs = [work.clone()];
    let w = found_in(&dirs, tmp.path(), &used).unwrap_or_else(|| panic!("origin's feat"));
    assert_eq!(
        (w.parked, w.parked_on.as_slice()),
        (1, &["feat".to_string()][..])
    );
    scratch_git(
        &work,
        &["push", "-q", "origin", "origin/feat:refs/heads/main"],
    );
    scratch_git(&work, &["fetch", "-q", "origin"]);
    assert_eq!(found_in(&dirs, tmp.path(), &used), None);
}

/// A RECORDED SUBMODULE COMMIT ON NO REMOTE IS WORK WHEREVER THE CHECKOUT WENT (R6, D2-1's
/// record): a bump the project pushed records a commit no remote of the submodule has, though
/// its checkout went back to a pushed commit. NEGATIVE CONTROL: that commit pushed, nothing.
#[test]
fn a_recorded_submodule_commit_on_no_remote_is_work_wherever_the_checkout_went() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let (_lib, app) = app_with_lib(tmp.path());
    let sub = app.join("vendor/lib");
    let pushed = scratch_git(&sub, &["rev-parse", "HEAD"]).trim().to_string();
    let made = local_commit(&sub, "d.txt");
    bump_pushed(&app);
    scratch_git(&sub, &["checkout", "-q", &pushed]);
    let w = found(&app).unwrap_or_else(|| panic!("the recorded commit"));
    assert_eq!(w.sub_unpushed, 1, "{w:?}");
    scratch_git(
        &sub,
        &["push", "-q", "origin", &format!("{made}:refs/heads/main")],
    );
    assert_eq!(found(&app), None);
}

/// A SHALLOW SUBMODULE'S COMMIT FETCHED BY ITSELF IS NO WORK (R6, aterm `e24fe994ad`): two deep,
/// its record (behind the remote main by more than that, so no fetched branch reaches it and it
/// is no shallow root) came from its remote by a fetch of its own, which `FETCH_HEAD` names.
/// NEGATIVE CONTROL: a commit made on it, recorded by a pushed bump, is one on no remote.
#[test]
fn a_shallow_submodules_commit_fetched_by_itself_is_no_work() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let (lib, _app) = app_with_lib(tmp.path());
    for name in ["b.txt", "c.txt", "d.txt"] {
        local_commit(&lib, name);
    }
    scratch_git(&lib, &["push", "-q", "origin", "HEAD:main"]);
    scratch_git(
        &tmp.path().join("lib/remote.git"),
        &["config", "uploadpack.allowReachableSHA1InWant", "true"],
    );
    let url = format!("file://{}", tmp.path().join("app/remote.git").display());
    scratch_git(tmp.path(), &["clone", "-q", &url, "clone"]);
    let clone = tmp.path().join("clone");
    scratch_git(
        &clone,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "update",
            "-q",
            "--init",
            "--depth",
            "2",
        ],
    );
    let sub = clone.join("vendor/lib");
    let record = scratch_git(&sub, &["rev-parse", "HEAD"]);
    assert_eq!(
        scratch_git(&sub, &["rev-list", "HEAD", "--not", "--remotes"]),
        record,
        "no fetched branch reaches the record"
    );
    assert_eq!(found(&clone), None, "fetched by itself from its remote");
    local_commit(&sub, "e.txt");
    bump_pushed(&clone);
    let w = found(&clone).unwrap_or_else(|| panic!("a commit made on it"));
    assert_eq!(w.sub_unpushed, 1, "{w:?}");
}

/// A NESTED SUBMODULE'S COMMIT ON NO REMOTE IS WORK (R6, D2-1 at every depth): `vendor/lib`'s own
/// submodule `deep` holds a commit no remote of it has, recorded by bumps both projects pushed.
/// NEGATIVE CONTROL: that commit pushed, nothing.
#[test]
fn a_nested_submodules_commit_on_no_remote_is_work() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let deep_dir = tmp.path().join("deep");
    mkdir(&deep_dir);
    let _deep = repo(&deep_dir, true);
    let (lib, app) = app_with_lib(tmp.path());
    let deep_url = format!("file://{}", deep_dir.join("remote.git").display());
    scratch_git(
        &lib,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            &deep_url,
            "deep",
        ],
    );
    scratch_git(&lib, &["commit", "-q", "-m", "deep"]);
    scratch_git(&lib, &["push", "-q", "origin", "HEAD:main"]);
    let sub = app.join("vendor/lib");
    scratch_git(&sub, &["pull", "-q", "origin", "main"]);
    scratch_git(
        &sub,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "update",
            "-q",
            "--init",
        ],
    );
    bump_pushed(&app);
    assert_eq!(found(&app), None, "every record pushed");
    let nested = sub.join("deep");
    let made = local_commit(&nested, "n.txt");
    scratch_git(&sub, &["add", "deep"]);
    scratch_git(&sub, &["commit", "-q", "-m", "bump deep"]);
    scratch_git(&sub, &["push", "-q", "origin", "HEAD:main"]);
    bump_pushed(&app);
    let w = found(&app).unwrap_or_else(|| panic!("the nested commit"));
    assert_eq!(w.sub_unpushed, 1, "{w:?}");
    assert_eq!(w.subs, ["vendor/lib/deep"], "{w:?}");
    scratch_git(
        &nested,
        &["push", "-q", "origin", &format!("{made}:refs/heads/main")],
    );
    assert_eq!(found(&app), None);
}

/// A FOLDER WHOSE READ WAS CUT IS KEPT (R7): git refuses the climb out of a folder's
/// repository, and the probe keeps that folder as well as the repository, so the next one reads
/// it again. NEGATIVE CONTROL: unrefused, only the repository is kept.
#[test]
fn a_folder_whose_read_was_cut_is_kept() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    let src = work.join("src");
    mkdir(&src);
    let refuse: Refuse = Arc::new(|_: &Path, args: &[&str]| {
        args == ["rev-parse", "--show-superproject-working-tree"]
    });
    let probed = runtime().block_on(probe_refusing(
        std::slice::from_ref(&src),
        tmp.path(),
        &Used::default(),
        refuse,
    ));
    let canon = std::fs::canonicalize(&work).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(probed.read, RepoRead::Unread);
    assert_eq!(probed.kept, [src.clone(), canon.clone()]);
    // NEGATIVE CONTROL: nothing refused.
    let none: Refuse = Arc::new(|_: &Path, _: &[&str]| false);
    let probed = runtime().block_on(probe_refusing(&[src], tmp.path(), &Used::default(), none));
    assert_eq!(
        (probed.read, probed.kept),
        (RepoRead::Read(None), vec![canon])
    );
}

/// A REFUSED READ OF THE CONFIGURED REMOTES IS NO CLEAN ONE (R7): `git remote` is a strict read,
/// and with it refused (git's own exit 128), whether a remote never fetched holds every commit
/// cannot be said. NEGATIVE CONTROL: unrefused, the pushed repository reads clean.
#[test]
fn a_refused_read_of_the_remotes_is_no_clean_one() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    let probe = |refused: bool| {
        let refuse: Refuse = Arc::new(move |_: &Path, args: &[&str]| refused && args == ["remote"]);
        runtime()
            .block_on(probe_refusing(
                std::slice::from_ref(&work),
                tmp.path(),
                &Used::default(),
                refuse,
            ))
            .read
    };
    assert_eq!(probe(true), RepoRead::Unread);
    assert_eq!(probe(false), RepoRead::Read(None));
}

/// A FOLDER WITH A `.git` OF ITS OWN IS NEVER FOLDED INTO THE PROJECT AROUND IT (R1's
/// compaction, [`compact`]'s and every probe's placement): fifty folders of a project cost one
/// discovery, yet a nested clone inside it (which the project ignores) is discovered and read.
/// NEGATIVE CONTROL: its `.git` gone, the nested folder is one of the project's, ignored, and
/// clean.
#[test]
fn a_folder_with_its_own_git_is_never_folded_into_the_project_around_it() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let outer = repo(tmp.path(), true);
    write(&outer.join(".gitignore"), "nested/\n");
    scratch_git(&outer, &["add", ".gitignore"]);
    scratch_git(&outer, &["commit", "-q", "-m", "ignore"]);
    scratch_git(&outer, &["push", "-q", "origin", "HEAD:main"]);
    let nested = outer.join("nested");
    mkdir(&nested);
    scratch_git(&nested, &["init", "-q", "."]);
    write(&nested.join("left.txt"), "x\n");
    let mut dirs: Vec<PathBuf> = (0..50)
        .map(|n| {
            let dir = outer.join(format!("d{n}/src"));
            mkdir(&dir);
            dir
        })
        .collect();
    dirs.push(nested.clone());
    let discoveries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let probe = |dirs: &[PathBuf]| {
        discoveries.store(0, std::sync::atomic::Ordering::SeqCst);
        let counted = Arc::clone(&discoveries);
        let count: Refuse = Arc::new(move |_: &Path, args: &[&str]| {
            if args == ["rev-parse", "--show-toplevel"] {
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            false
        });
        runtime()
            .block_on(probe_refusing(dirs, tmp.path(), &Used::default(), count))
            .read
    };
    let read = probe(&dirs);
    assert!(
        matches!(&read, RepoRead::Read(Some(w)) if w.repo.ends_with("/work/nested") && w.untracked == 1),
        "{read:?}"
    );
    assert_eq!(discoveries.load(std::sync::atomic::Ordering::SeqCst), 2);
    // NEGATIVE CONTROL: no `.git` of its own.
    std::fs::remove_dir_all(nested.join(".git")).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(probe(&dirs), RepoRead::Read(None));
    assert_eq!(discoveries.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// WHAT A COMMAND REACHES ([`command_reach`], R8): a folder its script `cd`s into, one git's
/// `-C` names, a subshell's (whose `cd` ends at its `)`), `~/`, `pushd`/`popd`, the folder a
/// `worktree add` makes, and each branch by the folder its git ran in; a folder the words cannot
/// place carries none. NEGATIVE CONTROLS: a command that moves nowhere and runs no git, and a
/// `cd` that is no command's first word, reach nothing.
#[test]
fn what_a_command_reaches() {
    use codex_alab_core::session::UsedBranch;
    let reach = |script: &str| {
        command_reach(
            &shell(script),
            Some(Path::new("/work")),
            Some(Path::new("/home/me")),
        )
    };
    let at = |name: &str, dir: Option<&str>| UsedBranch::new(name, dir.map(Path::new));
    let r = reach("cd proj && git commit -qam x");
    assert_eq!(r.folders, paths(&["/work/proj"]));
    assert!(r.branches.is_empty());
    let r = reach("git -C ../lib switch -c fix");
    assert_eq!(
        (r.folders, r.branches),
        (paths(&["/lib"]), vec![at("fix", Some("/lib"))])
    );
    let r = reach("(cd sub && git switch -c y); git switch -c z");
    assert_eq!(r.folders, paths(&["/work/sub", "/work"]));
    assert_eq!(
        r.branches,
        [at("y", Some("/work/sub")), at("z", Some("/work"))]
    );
    let r = reach("cd ~/p; git checkout -b t");
    assert_eq!(
        (r.folders, r.branches),
        (paths(&["/home/me/p"]), vec![at("t", Some("/home/me/p"))])
    );
    let r = reach("cd \"$X\" && git switch -c w");
    assert_eq!((r.folders, r.branches), (Vec::new(), vec![at("w", None)]));
    let r = reach("pushd a && popd && git switch -c q");
    assert_eq!(r.folders, paths(&["/work/a", "/work"]));
    assert_eq!(r.branches, [at("q", Some("/work"))]);
    let r = reach("git worktree add ../wt -b side");
    assert_eq!(r.folders, paths(&["/work", "/wt"]));
    assert_eq!(r.branches, [at("side", Some("/work"))]);
    let direct: Vec<String> = ["git", "-C", "/abs", "switch", "-c", "d"]
        .map(String::from)
        .to_vec();
    let r = command_reach(&direct, Some(Path::new("/work")), None);
    assert_eq!(
        (r.folders, r.branches),
        (paths(&["/abs"]), vec![at("d", Some("/abs"))])
    );
    // NEGATIVE CONTROLS.
    assert_eq!(reach("cargo test --workspace"), Reach::default());
    assert_eq!(reach("echo cd x"), Reach::default());
}

/// A FOLDER A COMMAND CDS INTO IS IN THE SCOPE (R8): its folder, and its branch by that folder.
/// NEGATIVE CONTROL: a command that reaches no other folder adds only its own.
#[tokio::test]
async fn a_folder_a_command_cds_into_is_in_the_scope() {
    let (autopilot, _shared, store, rt) = autopilot_thread();
    command(&autopilot, &store, "/work", &["git", "status"]).await;
    assert_eq!(rt.probe_scope().dirs, paths(&["/work"]));
    command(
        &autopilot,
        &store,
        "/work",
        &["bash", "-lc", "cd proj && git switch -c x"],
    )
    .await;
    let scope = rt.probe_scope();
    assert_eq!(scope.dirs, paths(&["/work", "/work/proj"]));
    assert_eq!(
        scope.branches,
        [codex_alab_core::session::UsedBranch::new(
            "x",
            Some(Path::new("/work/proj"))
        )]
    );
}

/// Waits while a compaction of `rt`'s scope runs (a minute at most: a hang detector).
async fn compacted(rt: &crate::extension::ThreadRt) {
    let detector = std::time::Instant::now() + Duration::from_secs(60);
    while crate::Autopilot::compacting(rt) {
        assert!(std::time::Instant::now() < detector, "the compaction hung");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// THE PROBE READS EVERY FOLDER THE SCOPE HOLDS (R4, land-probe-last-workdir through
/// `Autopilot::probe_repo`): the session left a file in repository `a`, then its last command
/// ran in the clean repository `b`; `a`'s work is found. NEGATIVE CONTROL: the file removed,
/// nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_probe_reads_every_folder_the_scope_holds() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let (a_dir, b_dir) = (tmp.path().join("a"), tmp.path().join("b"));
    mkdir(&a_dir);
    mkdir(&b_dir);
    let (a, b) = (repo(&a_dir, true), repo(&b_dir, true));
    let (autopilot, _shared, store, rt) = autopilot_thread();
    turn_starts(&autopilot, &store).await;
    command(
        &autopilot,
        &store,
        &a.display().to_string(),
        &["git", "status"],
    )
    .await;
    write(&a.join("left.txt"), "x\n");
    command(
        &autopilot,
        &store,
        &b.display().to_string(),
        &["git", "status"],
    )
    .await;
    let found = autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await;
    assert!(
        matches!(&found, RepoRead::Read(Some(w)) if w.repo.ends_with("/a/work") && w.untracked == 1),
        "{found:?}"
    );
    std::fs::remove_file(a.join("left.txt")).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await,
        RepoRead::Read(None)
    );
}

/// A PROBE COMPACTS THE SCOPE TO ITS REPOSITORIES (R4, through `Autopilot::probe_repo`): two
/// folders of one repository give way to it. NEGATIVE CONTROL: a second probe moves nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_probe_compacts_the_scope_to_its_repositories() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    let (autopilot, _shared, store, rt) = autopilot_thread();
    for sub in ["src", "docs"] {
        let dir = work.join(sub);
        mkdir(&dir);
        command(
            &autopilot,
            &store,
            &dir.display().to_string(),
            &["git", "status"],
        )
        .await;
    }
    assert_eq!(rt.probe_scope().dirs.len(), 2);
    assert_eq!(
        autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await,
        RepoRead::Read(None)
    );
    let canon = std::fs::canonicalize(&work).unwrap_or_else(|err| panic!("{err}"));
    let scope = rt.probe_scope();
    assert_eq!(scope.dirs, [canon]);
    // NEGATIVE CONTROL: again, nothing moves.
    let _ = autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await;
    assert_eq!(rt.probe_scope(), scope);
}

/// A WIDE CHANGE ACROSS ONE REPOSITORY NEVER OVERFLOWS THE SCOPE (R1): commands in more folders
/// of one clean repository than the scope keeps, between two probes; the scope is compacted to
/// its repository as it grows, never overflows, and the probe reads clean. (An overflowed scope
/// is never read clean: `a_scope_that_overflowed_is_never_read_clean`.) NEGATIVE CONTROL: work
/// left in one of those folders is found.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wide_change_across_one_repository_never_overflows_the_scope() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    let (autopilot, _shared, store, rt) = autopilot_thread();
    for n in 0..codex_alab_core::session::PROBE_SCOPE_DIRS + 88 {
        let dir = work.join(format!("crates/c{n}/src"));
        mkdir(&dir);
        command(
            &autopilot,
            &store,
            &dir.display().to_string(),
            &["cargo", "test"],
        )
        .await;
        // A session's commands come seconds apart: a compaction begun meanwhile has ended.
        compacted(&rt).await;
    }
    let scope = rt.probe_scope();
    assert!(!scope.overflow, "{} folders", scope.dirs.len());
    assert!(
        scope.dirs.len() < codex_alab_core::session::PROBE_SCOPE_COMPACT_AT,
        "{} folders",
        scope.dirs.len()
    );
    assert_eq!(
        autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await,
        RepoRead::Read(None)
    );
    // NEGATIVE CONTROL: work left there.
    write(&work.join("crates/c7/src/left.rs"), "x\n");
    let found = autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await;
    assert!(
        matches!(&found, RepoRead::Read(Some(w)) if w.untracked == 1),
        "{found:?}"
    );
}

/// WORK A COMMAND DID IN A FOLDER IT CDED INTO HOLDS THE LANDING (R8): the thread works from a
/// folder that is no repository and commits nothing; its command `cd`s into the project, where
/// it leaves a file. NEGATIVE CONTROL: the file removed, nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_a_command_did_in_a_folder_it_cded_into_holds_the_landing() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let parent = tmp.path().join("dev");
    let proj_dir = parent.join("proj");
    mkdir(&proj_dir);
    let proj = repo(&proj_dir, true);
    let (autopilot, _shared, store, rt) = autopilot_thread();
    turn_starts(&autopilot, &store).await;
    command(
        &autopilot,
        &store,
        &parent.display().to_string(),
        &["bash", "-lc", "cd proj/work && make notes"],
    )
    .await;
    write(&proj.join("notes.txt"), "x\n");
    let found = autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await;
    assert!(
        matches!(&found, RepoRead::Read(Some(w)) if w.repo.ends_with("/proj/work") && w.untracked == 1),
        "{found:?}"
    );
    std::fs::remove_file(proj.join("notes.txt")).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await,
        RepoRead::Read(None)
    );
}

/// A FOLDER ANOTHER TOOL IS TOLD TO WORK IN IS REACHED (the forever probe's follow-up, item e:
/// `command_reach` read `cd` and git's own folders, never `make -C`): each common tool's
/// folder option, its separate and attached spellings, relative to the folder the command runs
/// in (`env -C` and `env --chdir` move that folder for the one command), a manifest's or a
/// justfile's folder, the folder `cargo new` or `git clone`/`git init` makes, a nested shell's
/// `cd`, and a tool run by path or argv directly. A folder the words cannot place is none.
/// NEGATIVE CONTROLS: the same tools with no folder option, an option's word that is a
/// package's or a target's name, and words after `--` (the program's own) reach nothing.
#[test]
fn a_folder_another_tool_is_told_to_work_in_is_reached() {
    let reach = |script: &str| {
        command_reach(
            &shell(script),
            Some(Path::new("/work")),
            Some(Path::new("/home/me")),
        )
        .folders
    };
    for (script, folders) in [
        ("make -C sub all", &["/work/sub"][..]),
        ("make -Csub", &["/work/sub"]),
        ("gmake --directory=sub -j8", &["/work/sub"]),
        ("make --directory sub", &["/work/sub"]),
        ("make -C a -C b", &["/work/a/b"]),
        ("ninja -C build", &["/work/build"]),
        ("cargo build --manifest-path ../lib/Cargo.toml", &["/lib"]),
        (
            "cargo test --manifest-path=crates/x/Cargo.toml -q",
            &["/work/crates/x"],
        ),
        (
            "targo --unverified test --manifest-path lib/Cargo.toml",
            &["/work/lib"],
        ),
        ("cargo -C ../other build", &["/other"]),
        (
            "cargo -C ../other build --manifest-path x/Cargo.toml",
            &["/other", "/other/x"],
        ),
        ("cargo new --lib tool", &["/work/tool"]),
        ("cargo init ../fresh", &["/fresh"]),
        ("npm --prefix web install", &["/work/web"]),
        ("npm run build --prefix=web", &["/work/web"]),
        ("npm -C web test", &["/work/web"]),
        ("pnpm -C ui build", &["/work/ui"]),
        ("pnpm --dir ui install", &["/work/ui"]),
        ("pnpm --dir=ui install", &["/work/ui"]),
        ("yarn --cwd site build", &["/work/site"]),
        ("yarn --cwd=site build", &["/work/site"]),
        ("bun --cwd app test", &["/work/app"]),
        (
            "just -d tools --justfile tools/justfile fmt",
            &["/work/tools"],
        ),
        ("just --justfile ../j/justfile", &["/j"]),
        ("just -f ops/justfile deploy", &["/work/ops"]),
        (
            "just --working-directory=w --justfile=w/justfile",
            &["/work/w"],
        ),
        ("go -C svc test ./...", &["/work/svc"]),
        ("uv --directory py run pytest", &["/work/py"]),
        ("uv run --project=py pytest", &["/work/py"]),
        ("poetry -C api install", &["/work/api"]),
        ("mvn -f backend/pom.xml package", &["/work/backend"]),
        ("./gradlew -p android assemble", &["/work/android"]),
        ("gradle --project-dir=android build", &["/work/android"]),
        ("tar -xzf a.tgz -C out", &["/work/out"]),
        ("env -C ../lib make", &["/lib"]),
        ("env --chdir=../lib cargo build", &["/lib"]),
        (
            "env A=1 -u B --chdir ../lib make -C sub",
            &["/lib", "/lib/sub"],
        ),
        ("timeout 600 make -C slow", &["/work/slow"]),
        ("nice -n 5 make -C nice", &["/work/nice"]),
        ("/usr/bin/make -C abs", &["/work/abs"]),
        ("make -C ~/proj", &["/home/me/proj"]),
        ("bash -c 'cd inner && make'", &["/work/inner"]),
        ("sh -euc \"make -C deep\"", &["/work/deep"]),
        ("git clone file:///b/x.git lib2", &["/work", "/work/lib2"]),
        (
            "git clone -q --depth 1 file:///b/repo.git",
            &["/work", "/work/repo"],
        ),
        ("git init fresh", &["/work", "/work/fresh"]),
        ("cd proj && make -C sub", &["/work/proj", "/work/proj/sub"]),
        // `env -C` runs the one command there; a `cd` it runs is no shell's builtin.
        ("env -C ../lib cd x && git status", &["/lib", "/work"]),
    ] {
        assert_eq!(reach(script), paths(folders), "{script}");
    }
    let direct: Vec<String> = ["make", "-C", "/abs/dir", "all"].map(String::from).to_vec();
    assert_eq!(
        command_reach(&direct, Some(Path::new("/work")), None).folders,
        paths(&["/abs/dir"])
    );
    let direct: Vec<String> = ["cargo", "build", "--manifest-path", "m/Cargo.toml"]
        .map(String::from)
        .to_vec();
    assert_eq!(
        command_reach(&direct, Some(Path::new("/work")), None).folders,
        paths(&["/work/m"])
    );
    // A folder the words cannot place is none.
    assert!(reach("make -C \"$OUT\"").is_empty());
    assert!(reach("npm --prefix ~other/x i").is_empty());
    // NEGATIVE CONTROLS.
    for script in [
        "make all",
        "cargo test -p codex-alab-autopilot",
        "cargo build --target-dir /tmp/t",
        "npm install",
        "pnpm -F ui build",
        "yarn build",
        "just fmt",
        "go test ./...",
        "tar -czf out.tgz src",
        "echo make -C x",
        "cargo run -- -C x",
    ] {
        assert!(reach(script).is_empty(), "{script}: {:?}", reach(script));
    }
}

/// WORK A TOOL DID IN A FOLDER IT WAS TOLD TO WORK IN HOLDS THE LANDING (item e, through
/// `Autopilot::probe_repo`): the thread works from a folder that is no repository and runs
/// `make -C` into the project, where a file is left. NEGATIVE CONTROL: the file removed,
/// nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_a_tool_did_in_a_folder_it_was_told_to_work_in_holds_the_landing() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let parent = tmp.path().join("dev");
    let proj_dir = parent.join("proj");
    mkdir(&proj_dir);
    let proj = repo(&proj_dir, true);
    let (autopilot, _shared, store, rt) = autopilot_thread();
    turn_starts(&autopilot, &store).await;
    for argv in [
        &["bash", "-lc", "make -C proj/work notes"][..],
        &["npm", "--prefix", "proj/work", "run", "notes"],
    ] {
        command(&autopilot, &store, &parent.display().to_string(), argv).await;
    }
    write(&proj.join("notes.txt"), "x\n");
    let found = autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await;
    assert!(
        matches!(&found, RepoRead::Read(Some(w)) if w.repo.ends_with("/proj/work") && w.untracked == 1),
        "{found:?}"
    );
    std::fs::remove_file(proj.join("notes.txt")).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await,
        RepoRead::Read(None)
    );
}

/// A CUT WALK NEVER READS CLEAN (item e: a refused or cut read never reads clean): a command that
/// reaches more folders than [`command_reach`] keeps (as many as a probe scope keeps, F1), or
/// runs a shell within more shells than it reads into, is cut, and the scope that takes it in
/// lost a folder (`overflow`), so its probe never reads clean. NEGATIVE CONTROLS: as many folders
/// as are kept, and shells as deep as are read, are no cut, and a command across seventeen
/// folders (the first bound's cut, F1) leaves the scope with no overflow.
#[tokio::test]
async fn a_cut_walk_leaves_the_scope_never_read_clean() {
    let walk = |script: String| command_reach(&shell(&script), Some(Path::new("/work")), None);
    let cds = |n: usize| {
        (0..n)
            .map(|i| format!("cd /work/d{i}"))
            .collect::<Vec<_>>()
            .join(" && ")
    };
    // `shell` is one shell's `-lc`: `n` shells within it, one per level, around `make -C deep`
    // (eight shells in all are read).
    let nested = |n: usize| {
        let mut script = "make -C deep".to_string();
        for _ in 0..n {
            script = format!("sh -c {}", shell_quote(&script));
        }
        script
    };
    let kept = walk(cds(REACH_FOLDERS));
    assert_eq!((kept.folders.len(), kept.cut), (REACH_FOLDERS, false));
    let cut = walk(cds(REACH_FOLDERS + 1));
    assert_eq!((cut.folders.len(), cut.cut), (REACH_FOLDERS, true));
    let deep = walk(nested(7));
    assert_eq!((deep.folders, deep.cut), (paths(&["/work/deep"]), false));
    let too_deep = walk(nested(8));
    assert_eq!((too_deep.folders, too_deep.cut), (Vec::new(), true));
    // Through the thread's scope.
    let (autopilot, _shared, store, rt) = autopilot_thread();
    let script = cds(17);
    command(&autopilot, &store, "/work", &["bash", "-lc", &script]).await;
    assert!(!rt.probe_scope().overflow, "{:?}", rt.probe_scope());
    let script = cds(REACH_FOLDERS + 1);
    command(&autopilot, &store, "/work", &["bash", "-lc", &script]).await;
    assert!(rt.probe_scope().overflow, "{:?}", rt.probe_scope());
}

/// ONE COMMAND ACROSS MANY FOLDERS OF A CLEAN REPOSITORY STILL READS CLEAN (review of the
/// forever-persist lane, F1): `git -C <folder> status` over seventeen folders in one line (a
/// worker reading its worktrees) reached one more folder than the walk's first bound kept, the
/// walk dropped the folder it had placed and overflowed the scope, and every later probe of the
/// clean repository read `Unread`, so Land held until the session ended. Now the walk keeps every
/// folder a scope keeps: the scope holds all seventeen with no overflow, and the probe reads the
/// repository clean, again and again. NEGATIVE CONTROL: a file left in the seventeenth folder
/// holds the landing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_command_across_many_folders_of_a_clean_repository_still_reads_clean() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let proj = repo(&tmp.path().join("proj"), true);
    let folders: Vec<PathBuf> = (0..17).map(|i| proj.join(format!("d{i}"))).collect();
    for folder in &folders {
        mkdir(folder);
    }
    let (autopilot, _shared, store, rt) = autopilot_thread();
    turn_starts(&autopilot, &store).await;
    let at = proj.display().to_string();
    command(&autopilot, &store, &at, &["git", "status"]).await;
    assert_eq!(
        autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await,
        RepoRead::Read(None)
    );
    let script = folders
        .iter()
        .map(|folder| format!("git -C {} status", folder.display()))
        .collect::<Vec<_>>()
        .join(" && ");
    command(&autopilot, &store, &at, &["bash", "-lc", &script]).await;
    let scope = rt.probe_scope();
    assert!(!scope.overflow, "{scope:?}");
    assert!(
        folders.iter().all(|folder| scope.dirs.contains(folder)),
        "{scope:?}"
    );
    for _ in 0..3 {
        assert_eq!(
            autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await,
            RepoRead::Read(None)
        );
    }
    write(&folders[16].join("notes.txt"), "x\n");
    let found = autopilot.probe_repo(&rt, "test", PROBE_LIMIT).await;
    assert!(
        matches!(&found, RepoRead::Read(Some(w)) if w.untracked == 1),
        "{found:?}"
    );
}

/// `script` as one shell word.
fn shell_quote(script: &str) -> String {
    format!("'{}'", script.replace('\'', r#"'\''"#))
}
