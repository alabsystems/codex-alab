// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The repository probe: its parse ported from aterm's `repo_work_tests.rs`, the `unlanded`
//! count of the FOREVER LAND spec (§9 rule 2), and the probe over real scratch repositories;
//! a branch's own stashes, the person's excludes, `HOME` on every read and the work's state as
//! aterm main `e0da8d68df` tests them. The person's global configuration is always a scratch
//! home's, never the real one.

use std::path::Path;
use std::path::PathBuf;

use crate::repo::*;

/// `git <args>` in `dir`, hermetic: no system or global config, a fixed author.
pub(crate) fn scratch_git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=Andrew Yates"])
        .args(["-c", "user.email=a@example.invalid"])
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap_or_else(|err| panic!("git: {err}"));
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A repository at `<tmp>/work` with one commit, pushed to a bare remote at
/// `<tmp>/remote.git` (its `main`) when `remote`.
pub(crate) fn repo(tmp: &Path, remote: bool) -> PathBuf {
    let work = tmp.join("work");
    std::fs::create_dir_all(&work).unwrap_or_else(|err| panic!("work: {err}"));
    scratch_git(&work, &["init", "-q", "."]);
    std::fs::write(work.join("a.txt"), "one\n").unwrap_or_else(|err| panic!("a.txt: {err}"));
    scratch_git(&work, &["add", "a.txt"]);
    scratch_git(&work, &["commit", "-q", "-m", "one"]);
    if remote {
        let bare = tmp.join("remote.git");
        std::fs::create_dir_all(&bare).unwrap_or_else(|err| panic!("bare: {err}"));
        scratch_git(&bare, &["init", "-q", "--bare", "."]);
        let url = bare.display().to_string();
        scratch_git(&work, &["remote", "add", "origin", &url]);
        scratch_git(&work, &["push", "-q", "-u", "origin", "HEAD:main"]);
    }
    work
}

fn completed(read: RepoRead) -> Option<RepoWork> {
    match read {
        RepoRead::Read(found) => found,
        RepoRead::Unread => panic!("the scratch repository probe did not complete"),
    }
}

/// The probe with the person's `home` and `XDG_CONFIG_HOME` given.
fn probed(dir: &Path, home: &Path, xdg: Option<&Path>) -> Option<RepoWork> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| panic!("runtime: {err}"));
    completed(rt.block_on(probe_with(dir, home, xdg, PROBE_LIMIT)))
}

/// Actual Git discovery/status errors cannot erase an unpushed commit. Restoration
/// proves the same fixture still reads its work rather than landing being disabled.
#[test]
fn incomplete_repository_reads_are_unknown_and_a_verified_non_repository_is_clean() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let work = repo(tmp.path(), true);
    write(&work.join("b.txt"), "two\n");
    scratch_git(&work, &["add", "b.txt"]);
    scratch_git(&work, &["commit", "-q", "-m", "two"]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| panic!("{err}"));
    let read = |dir: &Path| runtime.block_on(probe_with(dir, tmp.path(), None, PROBE_LIMIT));
    let whole = read(&work);
    assert!(matches!(whole.as_ref(), RepoRead::Read(Some(found)) if found.unpushed == 1));
    for name in ["config", "index", "HEAD"] {
        let file = work.join(".git").join(name);
        let original = std::fs::read(&file).unwrap_or_else(|err| panic!("{err}"));
        std::fs::write(&file, b"incomplete repository metadata\0\n")
            .unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(
            read(&work),
            RepoRead::Unread,
            "failed real Git read: {name}"
        );
        std::fs::write(&file, original).unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(
            read(&work),
            whole,
            "the same repository after restoring {name}"
        );
    }
    let linked = tmp.path().join("linked-worktree");
    scratch_git(
        &work,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().expect("scratch path"),
        ],
    );
    let linked_whole = read(&linked);
    assert!(matches!(linked_whole.as_ref(), RepoRead::Read(Some(found)) if found.unpushed == 1));
    let gitfile = linked.join(".git");
    let original = std::fs::read(&gitfile).unwrap_or_else(|err| panic!("{err}"));
    write(&gitfile, "gitdir: missing-worktree-metadata\n");
    assert_eq!(
        read(&linked),
        RepoRead::Unread,
        "a broken existing worktree is no verified non-repository"
    );
    std::fs::write(&gitfile, original).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(read(&linked), linked_whole);
    let plain = tmp.path().join("no-repository");
    mkdir(&plain);
    assert_eq!(read(&plain), RepoRead::Read(None));
}

/// Git can return success while omitting an unreadable untracked directory.
/// Its warning is incomplete evidence, never a clean repository. Restoring
/// access must reveal the same real work rather than disabling completed reads.
#[cfg(unix)]
#[test]
fn a_successful_status_with_an_unread_directory_is_unknown() {
    use std::os::unix::fs::PermissionsExt;

    struct RestorePermissions {
        path: PathBuf,
        permissions: std::fs::Permissions,
    }
    impl Drop for RestorePermissions {
        fn drop(&mut self) {
            if let Err(error) = std::fs::set_permissions(&self.path, self.permissions.clone()) {
                eprintln!(
                    "restore fixture permissions for {}: {error}",
                    self.path.display()
                );
                if !std::thread::panicking() {
                    panic!("fixture directory permissions were not restored");
                }
            }
        }
    }

    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| panic!("runtime: {err}"));
    let read = || runtime.block_on(probe_with(&work, tmp.path(), None, PROBE_LIMIT));
    assert_eq!(
        read(),
        RepoRead::Read(None),
        "the pushed fixture starts clean"
    );
    let hidden = work.join("hidden");
    mkdir(&hidden);
    write(&hidden.join("work.txt"), "work that Git must not erase\n");
    let restore = RestorePermissions {
        path: hidden.clone(),
        permissions: std::fs::metadata(&hidden)
            .unwrap_or_else(|err| panic!("fixture directory metadata: {err}"))
            .permissions(),
    };
    std::fs::set_permissions(&hidden, std::fs::Permissions::from_mode(0o000))
        .unwrap_or_else(|err| panic!("make owned fixture unreadable: {err}"));
    let denied = match std::fs::read_dir(&hidden) {
        Ok(_) => false,
        Err(error) => {
            assert_eq!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied,
                "{error}"
            );
            true
        }
    };
    eprintln!("unread-directory fixture permission denied: {denied}");
    let raw = std::process::Command::new("git")
        .args([
            "--no-pager",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "safe.bareRepository=explicit",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
        ])
        .current_dir(&work)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").expect("test PATH"))
        .env("HOME", tmp.path())
        .env("XDG_CONFIG_HOME", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .output()
        .unwrap_or_else(|err| panic!("real Git status: {err}"));
    let restricted = read();
    // Restore before every outcome assertion, including the expected old-rule
    // failure. The Drop guard also restores access if a setup/read panics above.
    std::fs::set_permissions(&hidden, restore.permissions.clone())
        .unwrap_or_else(|err| panic!("restore owned fixture: {err}"));
    assert_eq!(
        std::fs::metadata(&hidden)
            .expect("restored fixture")
            .permissions()
            .mode(),
        restore.permissions.mode()
    );
    assert_eq!(raw.status.code(), Some(0), "{raw:?}");
    let warning = String::from_utf8_lossy(&raw.stderr);
    if denied {
        assert!(raw.stdout.is_empty(), "{raw:?}");
        assert!(
            warning.starts_with("warning: could not open directory 'hidden/'"),
            "{warning}"
        );
        assert!(warning.contains("Permission denied"), "{warning}");
    } else {
        // A privileged runner can read mode000. It still must see the real
        // untracked file: this is an exercised control, never a silent skip.
        assert_eq!(raw.stdout, b"?? hidden/\0", "{raw:?}");
    }
    let restored = match read() {
        RepoRead::Read(Some(found)) => found,
        other => panic!("restoring access must reveal the same work: {other:?}"),
    };
    assert_eq!(
        (
            restored.changed,
            restored.untracked,
            restored.unpushed,
            restored.unlanded
        ),
        (0, 1, 0, 0)
    );
    assert_eq!(restored.paths, ["hidden/"]);
    if denied {
        assert_eq!(
            restricted,
            RepoRead::Unread,
            "a successful but incomplete Git read cannot prove clean"
        );
    } else {
        let found = match restricted {
            RepoRead::Read(Some(found)) => found,
            other => {
                panic!("privileged access must find the real work, never prove clean: {other:?}")
            }
        };
        assert_eq!(found.untracked, 1);
        assert_eq!(found.paths, ["hidden/"]);
    }
}

/// The probe with `dir` as the person's home and no `XDG_CONFIG_HOME`: no excludes of theirs.
fn found(dir: &Path) -> Option<RepoWork> {
    probed(dir, dir, None)
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
}

fn mkdir(path: &Path) {
    std::fs::create_dir_all(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
}

/// THE READS' OUTPUT, PARSED ([`RepoWork::of_git`]): a tracked change staged or not, a
/// rename (its source field skipped), an untracked path, stash lines, unpushed and unlanded
/// commits each count; ignored paths do not; at most [`NAMED_PATHS`] paths are named, each
/// printable; an unlanded count with no default branch named says `the remote main` (aterm's
/// `RepoWork::summary`). NEGATIVE CONTROLS: a clean status, no stash and nothing unpushed or
/// unlanded is no finding, a default branch named or not.
#[test]
fn the_reads_output_is_counted_and_a_clean_repository_is_no_finding() {
    let status = " M src/a.rs\0M  b.rs\0R  new.rs\0old.rs\0?? notes/\0!! target/\0";
    let w = RepoWork::of_git(
        "/r\n",
        status,
        "stash@{0}: WIP on main\n",
        2,
        3,
        "origin/main",
    )
    .unwrap_or_else(|| panic!("found"));
    assert_eq!(
        (
            w.repo.as_str(),
            w.changed,
            w.untracked,
            w.stashes,
            w.unpushed,
            w.unlanded
        ),
        ("/r", 3, 1, 1, 2, 3)
    );
    assert_eq!(w.paths, ["src/a.rs", "b.rs", "new.rs"]);
    assert_eq!(
        w.summary(),
        "/r: 3 changed, 1 untracked (src/a.rs, b.rs, new.rs, …), 1 stash, 2 unpushed commits, \
         3 commits not on origin/main"
    );
    // A path that would end a typed line is printed harmless.
    let w = RepoWork::of_git("/r", "?? evil\nname\0", "", 0, 0, "").unwrap_or_default();
    assert_eq!(w.paths, ["evil?name"]);
    assert_eq!(w.summary(), "/r: 1 untracked (evil?name)");
    let w = RepoWork::of_git("/r", "", "", 1, 0, "origin/main").unwrap_or_default();
    assert_eq!(w.summary(), "/r: 1 unpushed commit");
    let w = RepoWork::of_git("/r", "", "", 0, 1, "origin/trunk").unwrap_or_default();
    assert_eq!(w.summary(), "/r: 1 commit not on origin/trunk");
    let w = RepoWork::of_git("/r", "", "", 1, 1, "origin/master").unwrap_or_default();
    assert_eq!(
        w.summary(),
        "/r: 1 unpushed commit, 1 commit not on origin/master"
    );
    let w = RepoWork::of_git("/r", "", "", 0, 4, "").unwrap_or_default();
    assert_eq!(
        (w.unlanded, w.summary().as_str()),
        (4, "/r: 4 commits not on the remote main")
    );
    // NEGATIVE CONTROLS.
    assert_eq!(RepoWork::of_git("/r", "", "", 0, 0, ""), None);
    assert_eq!(RepoWork::of_git("/r", "", "", 0, 0, "origin/main"), None);
    assert_eq!(RepoWork::of_git("/r", "!! target/\0", "\n", 0, 0, ""), None);
    // Public findings accept the full count domain. A mixed finding must not
    // wrap to "nothing", and its path suffix still records omitted paths.
    let w = RepoWork::of_git("/r", "?? one\0", "", usize::MAX, 0, "").unwrap_or_default();
    assert!(w.found());
    assert_eq!((w.untracked, w.unpushed), (1, usize::MAX));
    let w = RepoWork {
        repo: "/r".into(),
        changed: usize::MAX,
        untracked: 1,
        paths: vec!["one".into()],
        ..RepoWork::default()
    };
    assert!(w.found());
    assert!(w.summary().contains("(one, …)"));
}

/// A long path keeps its end, and the whole stays within the bound; the suffix that names a
/// long finding cuts it after a whole word within 160 UTF-16 units, and one with no finding
/// named is the bare sentence (aterm's `repo_work_suffix`, `fit_units`).
#[test]
fn a_long_path_keeps_its_end() {
    let long = format!("{}/tail.rs", "d".repeat(200));
    let p = printable(&long);
    assert!(p.starts_with('…') && p.ends_with("/tail.rs"), "{p}");
    assert_eq!(p.chars().count(), 72);
    assert_eq!(printable("short.rs"), "short.rs");
    let w = RepoWork {
        repo: printable(&format!("/{}", "r".repeat(100))),
        changed: 3,
        paths: (0..3).map(|i| printable(&format!("{i}{long}"))).collect(),
        ..RepoWork::default()
    };
    let said = crate::words::repo_suffix(&w.summary());
    let tail = "…): commit, merge and push it to the remote main first.";
    let named = said
        .strip_prefix("Your repository still holds work no remote main has (")
        .and_then(|s| s.strip_suffix("): commit, merge and push it to the remote main first."))
        .unwrap_or_else(|| panic!("{said}"));
    assert!(said.ends_with(tail), "{said}");
    assert!(named.encode_utf16().count() <= 160, "{named}");
    assert!(w.summary().starts_with(named.trim_end_matches('…')));
    // NEGATIVE CONTROLS: a finding that fits is named whole; none named is the bare sentence.
    assert_eq!(
        crate::words::repo_suffix("/r: 1 unpushed commit"),
        "Your repository still holds work no remote main has (/r: 1 unpushed commit): commit, \
         merge and push it to the remote main first."
    );
    assert_eq!(
        crate::words::repo_suffix(""),
        "Your repository still holds work no remote main has: commit, merge and push it to the \
         remote main first."
    );
}

/// THE PROBE OVER A REAL REPOSITORY: an uncommitted file, a commit no remote has, a stash,
/// and a commit pushed only to a feature branch (not on the remote's main) are each found,
/// and once the work is on the remote main nothing is. NEGATIVE CONTROLS: a clean repository
/// on the remote main; a repository with no remote at all (nowhere to push); a folder that is
/// no repository.
#[test]
fn the_probe_finds_uncommitted_unpushed_stashed_and_unlanded_work() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    assert_eq!(found(&work), None, "clean and on origin/main");
    // An untracked file, read from a folder below the top level.
    std::fs::create_dir_all(work.join("sub")).unwrap_or_else(|err| panic!("sub: {err}"));
    std::fs::write(work.join("sub/new.rs"), "fn main() {}\n")
        .unwrap_or_else(|err| panic!("new: {err}"));
    let w = found(&work.join("sub")).unwrap_or_else(|| panic!("untracked"));
    assert_eq!((w.untracked, w.changed, w.unpushed), (1, 0, 0), "{w:?}");
    assert_eq!(w.paths, ["sub/"]);
    assert!(w.repo.ends_with("/work"), "{w:?}");
    // Committed, not pushed: unpushed and unlanded.
    scratch_git(&work, &["add", "sub/new.rs"]);
    scratch_git(&work, &["commit", "-q", "-m", "two"]);
    let w = found(&work).unwrap_or_else(|| panic!("unpushed"));
    assert_eq!((w.untracked, w.unpushed, w.unlanded), (0, 1, 1), "{w:?}");
    assert_eq!(w.main_ref, "origin/main");
    // Pushed to the remote main: nothing.
    scratch_git(&work, &["push", "-q", "origin", "HEAD:main"]);
    assert_eq!(found(&work), None, "pushed");
    // Pushed only to a feature branch: unlanded, not unpushed.
    scratch_git(&work, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(work.join("b.txt"), "b\n").unwrap_or_else(|err| panic!("b: {err}"));
    scratch_git(&work, &["add", "b.txt"]);
    scratch_git(&work, &["commit", "-q", "-m", "three"]);
    scratch_git(&work, &["push", "-q", "origin", "feature"]);
    let w = found(&work).unwrap_or_else(|| panic!("unlanded"));
    assert_eq!((w.unpushed, w.unlanded, w.changed), (0, 1, 0), "{w:?}");
    assert!(
        w.summary().ends_with("1 commit not on origin/main"),
        "{}",
        w.summary()
    );
    // `refs/remotes/origin/HEAD` names the default branch where it is set: pointed at the
    // feature branch, the feature's commit is landed there.
    scratch_git(&work, &["remote", "set-head", "origin", "feature"]);
    assert_eq!(found(&work), None, "origin/HEAD is origin/feature");
    scratch_git(&work, &["remote", "set-head", "origin", "main"]);
    assert_eq!(found(&work).map(|w| w.unlanded), Some(1));
    // Merged into the remote main: nothing.
    scratch_git(&work, &["push", "-q", "origin", "HEAD:main"]);
    assert_eq!(found(&work), None, "landed");
    // A tracked change, then stashed: the stash is found.
    std::fs::write(work.join("a.txt"), "two\n").unwrap_or_else(|err| panic!("edit: {err}"));
    let w = found(&work).unwrap_or_else(|| panic!("changed"));
    assert_eq!(
        (w.changed, w.paths.as_slice()),
        (1, &["a.txt".to_string()][..])
    );
    scratch_git(&work, &["stash", "-q"]);
    let w = found(&work).unwrap_or_else(|| panic!("stash"));
    assert_eq!((w.changed, w.stashes), (0, 1), "{w:?}");
    scratch_git(&work, &["stash", "drop", "-q"]);
    assert_eq!(found(&work), None, "the stash dropped");
    // NEGATIVE CONTROLS: no remote at all, and no repository.
    let local = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let alone = repo(local.path(), false);
    assert_eq!(found(&alone), None, "a repository with no remote");
    let plain = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    assert_eq!(found(plain.path()), None, "no repository");
}

/// A GUARDED FOLDER IS NEVER PROBED: a repository under the home's `Documents` (and the
/// other guarded roots) finds nothing, though it holds work. NEGATIVE CONTROL: the same
/// repository under `Developer` is found.
#[test]
fn a_guarded_folder_is_never_probed() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let home = tmp.path().join("home");
    let docs = home.join("Documents");
    let dev = home.join("Developer");
    for dir in [&docs, &dev] {
        std::fs::create_dir_all(dir).unwrap_or_else(|err| panic!("dir: {err}"));
        let work = repo(dir, false);
        std::fs::write(work.join("new.txt"), "x\n").unwrap_or_else(|err| panic!("new: {err}"));
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| panic!("runtime: {err}"));
    let probe = |dir: PathBuf| completed(rt.block_on(probe_within(&dir, &home, PROBE_LIMIT)));
    assert_eq!(probe(docs.join("work")), None, "under Documents");
    assert_eq!(
        probe(dev.join("work")).map(|w| w.untracked),
        Some(1),
        "under Developer"
    );
    for (path, guarded) in [
        ("/h/Documents/x", true),
        ("/h/Desktop", true),
        ("/h/Library/Mobile Documents/a", true),
        ("/h/Library/CloudStorage/p/a", true),
        ("/Volumes/ext/repo", true),
        ("/h/Documentsx/a", false),
        ("/h/Library", false),
        ("/h/Developer/repo", false),
        ("/h", false),
    ] {
        assert_eq!(
            under_protected_root(Path::new("/h"), Path::new(path)),
            guarded,
            "{path}"
        );
    }
}

/// A DANGLING `origin/HEAD` IS NO DEFAULT BRANCH (review of 2a9241bbf, finding 14): the
/// remote's default renamed and pruned here leaves `origin/HEAD` naming a ref that is gone,
/// and the count falls back to `origin/main`, so a commit pushed only to a feature branch is
/// still found. NEGATIVE CONTROL: `origin/HEAD` naming a branch that exists is the default.
#[test]
fn a_dangling_origin_head_falls_back_to_origin_main() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    scratch_git(&work, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(work.join("b.txt"), "b\n").unwrap_or_else(|err| panic!("b: {err}"));
    scratch_git(&work, &["add", "b.txt"]);
    scratch_git(&work, &["commit", "-q", "-m", "two"]);
    scratch_git(&work, &["push", "-q", "origin", "feature"]);
    assert_eq!(found(&work).map(|w| w.unlanded), Some(1), "the control");
    scratch_git(
        &work,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/master",
        ],
    );
    let w = found(&work).unwrap_or_else(|| panic!("a dangling origin/HEAD hides nothing"));
    assert_eq!(
        (w.unlanded, w.main_ref.as_str()),
        (1, "origin/main"),
        "{w:?}"
    );
    // NEGATIVE CONTROL: origin/HEAD naming the feature branch, which exists.
    scratch_git(
        &work,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/feature",
        ],
    );
    assert_eq!(found(&work), None);
}

/// THE PROBE READS EVERY FOLDER IN A REPOSITORY (review of 2a9241bbf, finding 6; review of the
/// forever probe, land-probe-last-workdir, which this test pinned as first-folder-wins): the
/// worker worked in a worktree holding unlanded work while the thread's own folder is a clean
/// checkout: the worktree's work is found, whichever of the two the session touched last. A
/// folder that is gone, or in no repository, adds nothing. NEGATIVE CONTROL: the clean checkout
/// alone finds nothing.
#[test]
fn the_probe_reads_every_folder_in_a_repository() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    let tree = tmp.path().join("work-feat");
    let tree_s = tree.display().to_string();
    scratch_git(&work, &["worktree", "add", "-q", "-b", "feat", &tree_s]);
    std::fs::write(tree.join("c.txt"), "c\n").unwrap_or_else(|err| panic!("c: {err}"));
    scratch_git(&tree, &["add", "c.txt"]);
    scratch_git(&tree, &["commit", "-q", "-m", "feat"]);
    let plain = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let gone = tmp.path().join("gone");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| panic!("runtime: {err}"));
    let home = tmp.path();
    let first = |dirs: &[PathBuf]| {
        completed(
            rt.block_on(probe_session_within(
                dirs,
                home,
                &Used::default(),
                PROBE_LIMIT,
            ))
            .read,
        )
    };
    let w = first(&[tree.clone(), work.clone()]).unwrap_or_else(|| panic!("the worktree"));
    assert_eq!((w.unpushed, w.unlanded), (1, 1), "{w:?}");
    assert!(w.repo.ends_with("/work-feat"), "{w:?}");
    // The clean checkout first: the worktree after it is read too.
    let w = first(&[work.clone(), tree.clone()])
        .unwrap_or_else(|| panic!("the worktree after the clean checkout"));
    assert!(w.repo.ends_with("/work-feat"), "{w:?}");
    let w = first(&[gone, plain.path().to_path_buf(), tree])
        .unwrap_or_else(|| panic!("past a gone folder and no repository"));
    assert!(w.repo.ends_with("/work-feat"), "{w:?}");
    // NEGATIVE CONTROL: the clean checkout alone.
    assert_eq!(first(&[work]), None);
}

/// ONE BRANCH'S STASH ENTRIES ([`stashes_on`], over `git stash list --format=%gs`): those Git
/// wrote on the branch, a message given or not; on a detached `HEAD`, those written with no
/// branch. NEGATIVE CONTROLS: another branch's entry, and one whose branch only begins like it
/// (`mainline` for `main`), are not counted.
#[test]
fn a_branchs_stash_entries_are_those_written_on_it() {
    let list = "WIP on main: 1a2b3c4 one\nOn main: a message\nWIP on side: 5d6e7f8 two\n\
                WIP on mainline: 9a8b7c6 three\nWIP on (no branch): 0f1e2d3 four\n";
    assert_eq!(
        stashes_on(list, Some("main")),
        "WIP on main: 1a2b3c4 one\nOn main: a message\n"
    );
    assert_eq!(stashes_on(list, Some("side")), "WIP on side: 5d6e7f8 two\n");
    assert_eq!(stashes_on(list, None), "WIP on (no branch): 0f1e2d3 four\n");
    // NEGATIVE CONTROLS.
    assert_eq!(stashes_on(list, Some("other")), "");
    assert_eq!(stashes_on("", Some("main")), "");
}

/// A STASH ANOTHER WORKTREE MADE IS NONE OF THIS ONE'S (aterm d88a7bf289, the review's #6): a
/// repository's stash is shared by its worktrees, and an entry made in another worktree, on
/// its own branch, is another session's work, unless this session used that branch
/// (`work_this_session_parked_on_another_branch_holds_the_landing`, review of the forever
/// probe, land-probe-head-only). NEGATIVE CONTROLS: that worktree's probe finds it, and a stash
/// made here is found here.
#[test]
fn a_stash_another_worktree_made_is_none_of_this_ones() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    let other = tmp.path().join("other");
    let other_s = other.display().to_string();
    scratch_git(&work, &["worktree", "add", "-q", "-b", "side", &other_s]);
    write(&other.join("a.txt"), "side\n");
    scratch_git(&other, &["stash", "-q"]);
    assert_eq!(found(&work), None, "the other worktree's stash");
    // NEGATIVE CONTROLS.
    let w = found(&other).unwrap_or_else(|| panic!("its own stash"));
    assert_eq!((w.stashes, w.changed), (1, 0), "{w:?}");
    write(&work.join("a.txt"), "here\n");
    scratch_git(&work, &["stash", "-q"]);
    let w = found(&work).unwrap_or_else(|| panic!("this worktree's stash"));
    assert_eq!((w.stashes, w.changed), (1, 0), "{w:?}");
}

/// THE PERSON'S GLOBAL EXCLUDES HOLD (aterm d88a7bf289, the review's #7): a file their global
/// `core.excludesFile` names (`~/` expanded), or Git's default `<XDG_CONFIG_HOME>/git/ignore`
/// (`<home>/.config/git/ignore` where that is unset), ignores is no untracked work, as their
/// own `git status` says: `.DS_Store` read as untracked work kept Land from ever landing.
/// NEGATIVE CONTROLS: a home with no excludes finds it, and a file the excludes do not name is
/// found beside it.
#[test]
fn the_persons_global_excludes_hold() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    write(&work.join(".DS_Store"), "x");
    let home = tmp.path().join("home");
    mkdir(&home);
    let probe_at = |xdg: Option<&Path>| probed(&work, &home, xdg);
    // NEGATIVE CONTROL: no excludes.
    let w = probe_at(None).unwrap_or_else(|| panic!("untracked"));
    assert_eq!(
        (w.untracked, w.paths.as_slice()),
        (1, &[".DS_Store".to_string()][..])
    );
    // `core.excludesFile` in the global configuration.
    write(&home.join("my-ignore"), ".DS_Store\n");
    write(
        &home.join(".gitconfig"),
        "[core]\n\texcludesFile = ~/my-ignore\n",
    );
    assert_eq!(probe_at(None), None, "core.excludesFile");
    std::fs::remove_file(home.join(".gitconfig")).unwrap_or_else(|err| panic!("rm: {err}"));
    assert!(probe_at(None).is_some(), "the configuration gone");
    // Git's default, under XDG_CONFIG_HOME and under the home.
    let xdg = tmp.path().join("xdg");
    mkdir(&xdg.join("git"));
    write(&xdg.join("git/ignore"), ".DS_Store\n");
    assert_eq!(probe_at(Some(&xdg)), None, "XDG_CONFIG_HOME's");
    mkdir(&home.join(".config/git"));
    write(&home.join(".config/git/ignore"), ".DS_Store\n");
    assert_eq!(probe_at(None), None, "the home's");
    // NEGATIVE CONTROL: a file they do not ignore.
    write(&work.join("notes.md"), "x");
    let w = probe_at(None).unwrap_or_else(|| panic!("notes"));
    assert_eq!(
        (w.untracked, w.paths.as_slice()),
        (1, &["notes.md".to_string()][..])
    );
}

/// THE REPOSITORY'S OWN EXCLUDES WIN OVER THE PERSON'S ([`RepoWork`]'s status read, aterm
/// 02cb768483), as in their own `git status`: a repository whose local configuration sets
/// `core.excludesFile` has that file read, and the person's global one not, so what only its
/// own file ignores is no work, and what only theirs ignores is. NEGATIVE CONTROL: with no
/// local setting, the global file holds and the repository's file is no excludes at all.
#[test]
fn the_repositorys_own_excludes_win_over_the_persons() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    write(&work.join(".DS_Store"), "x");
    write(&work.join("local.tmp"), "x");
    let home = tmp.path().join("home");
    mkdir(&home);
    write(&home.join("my-ignore"), ".DS_Store\n");
    write(
        &home.join(".gitconfig"),
        "[core]\n\texcludesFile = ~/my-ignore\n",
    );
    let own = tmp.path().join("repo-ignore");
    write(&own, "local.tmp\n");
    let untracked = || {
        probed(&work, &home, None)
            .map(|w| w.paths)
            .unwrap_or_default()
    };
    // NEGATIVE CONTROL: no local setting, the person's file holds.
    assert_eq!(untracked(), ["local.tmp"]);
    let own = own.display().to_string();
    scratch_git(&work, &["config", "core.excludesFile", &own]);
    assert_eq!(untracked(), [".DS_Store"], "the repository's own file");
}

/// THE PERSON'S EXCLUDES SET THROUGH AN INCLUDE HOLD (the global read takes `--includes`,
/// aterm 02cb768483): a global configuration whose `[include]` names the file that sets
/// `core.excludesFile` ignores what that file names, as their own `git status` does.
/// NEGATIVE CONTROL: the include gone, the file is untracked work again.
#[test]
fn the_persons_excludes_set_through_an_include_hold() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    write(&work.join(".DS_Store"), "x");
    let home = tmp.path().join("home");
    mkdir(&home);
    write(&home.join("my-ignore"), ".DS_Store\n");
    write(
        &home.join("extra.gitconfig"),
        "[core]\n\texcludesFile = ~/my-ignore\n",
    );
    write(
        &home.join(".gitconfig"),
        "[include]\n\tpath = ~/extra.gitconfig\n",
    );
    assert_eq!(
        probed(&work, &home, None),
        None,
        "the included excludes hold"
    );
    // NEGATIVE CONTROL: no include.
    write(&home.join(".gitconfig"), "[user]\n\tname = A\n");
    assert_eq!(
        probed(&work, &home, None).map(|w| w.paths),
        Some(vec![".DS_Store".to_string()])
    );
}

/// THE PERSON'S EXCLUDES SET THROUGH AN `includeIf "gitdir:"` HOLD (the global read runs in the
/// repository, so the condition is judged against it; aterm fabf0a765a): a global
/// configuration that includes the file setting `core.excludesFile` only for repositories
/// under one folder ignores what that file names there, as their own `git status` does. Read
/// from `/`, no repository matches, and the ignored file counted as untracked work for good.
/// NEGATIVE CONTROL: a condition naming another folder includes nothing, and the file is work.
#[test]
fn the_persons_excludes_set_for_this_repository_by_an_include_if_hold() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    write(&work.join(".DS_Store"), "x");
    let home = tmp.path().join("home");
    mkdir(&home);
    write(&home.join("my-ignore"), ".DS_Store\n");
    write(
        &home.join("extra.gitconfig"),
        "[core]\n\texcludesFile = ~/my-ignore\n",
    );
    let gitdir = |dir: &Path| {
        let dir = std::fs::canonicalize(dir).unwrap_or_else(|err| panic!("canonical: {err}"));
        format!(
            "[includeIf \"gitdir:{}/\"]\n\tpath = ~/extra.gitconfig\n",
            dir.display()
        )
    };
    write(&home.join(".gitconfig"), &gitdir(&work));
    assert_eq!(
        probed(&work, &home, None),
        None,
        "the conditional excludes hold"
    );
    // NEGATIVE CONTROL: the condition names another folder.
    write(&home.join(".gitconfig"), &gitdir(&home));
    assert_eq!(
        probed(&work, &home, None).map(|w| w.paths),
        Some(vec![".DS_Store".to_string()])
    );
}

/// A REPOSITORY'S OWN `~/` PATH IS READ UNDER THE PERSON'S HOME (aterm fabf0a765a): Git
/// expands `~` in a path value (here the repository's own `core.excludesFile = ~/…`) with
/// `HOME`, and a probe's `git` with none dies on it (`fatal: failed to expand user dir`) at its
/// first read, so the probe found nothing and a dirty repository could land. The reads run
/// with the person's home as `HOME`, their global configuration still shut out. NEGATIVE
/// CONTROL: what the repository's own file does not ignore is still found, and the person's
/// global configuration is no part of the reads.
#[test]
fn a_repositorys_own_home_relative_path_is_read_under_the_persons_home() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    write(&work.join("local.tmp"), "x");
    write(&work.join("notes.md"), "x");
    let home = tmp.path().join("home");
    mkdir(&home);
    write(&home.join("repo-ignore"), "local.tmp\n");
    scratch_git(&work, &["config", "core.excludesFile", "~/repo-ignore"]);
    let w = probed(&work, &home, None).unwrap_or_else(|| panic!("the untracked file"));
    assert_eq!(w.paths, ["notes.md"], "{w:?}");
    // The person's own global configuration is no part of the reads.
    write(
        &home.join(".gitconfig"),
        "[core]\n\texcludesFile = ~/no-such-file\n[status]\n\tshowUntrackedFiles = no\n",
    );
    let w = probed(&work, &home, None).unwrap_or_else(|| panic!("still read"));
    assert_eq!(w.paths, ["notes.md"], "{w:?}");
}

/// THE WORK'S STATE TELLS APART WORK THE COUNTS CANNOT ([`RepoWork::state`],
/// [`RepoWork::same`]; aterm 25cde204a6): an edit to a file already changed, and a new file in
/// a directory already untracked, leave the counts and the named paths as they were, and only
/// the state moves. NEGATIVE CONTROLS: the same work probed twice is the same; a finding with
/// no state read is never the same as anything, itself included; two probes that found
/// nothing are.
#[test]
fn the_works_state_tells_apart_work_the_counts_cannot() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    write(&work.join("a.txt"), "two\n");
    let first = found(&work).unwrap_or_else(|| panic!("changed"));
    assert!(first.state.is_some(), "{first:?}");
    let again = found(&work).unwrap_or_else(|| panic!("changed"));
    assert!(
        RepoWork::same(Some(&first), Some(&again)),
        "{first:?} {again:?}"
    );
    // The same file edited again: the same counts and paths.
    write(&work.join("a.txt"), "two, then three\n");
    let edited = found(&work).unwrap_or_else(|| panic!("changed"));
    assert_eq!(
        (edited.changed, edited.untracked, &edited.paths),
        (first.changed, first.untracked, &first.paths)
    );
    assert!(!RepoWork::same(Some(&first), Some(&edited)), "{edited:?}");
    // A new file in a directory already untracked: still `sub/`, one entry.
    mkdir(&work.join("sub"));
    write(&work.join("sub/one.rs"), "fn one() {}\n");
    let one = found(&work).unwrap_or_else(|| panic!("untracked"));
    write(&work.join("sub/two.rs"), "fn two() {}\n");
    let two = found(&work).unwrap_or_else(|| panic!("untracked"));
    assert_eq!((one.untracked, &one.paths), (two.untracked, &two.paths));
    assert!(!RepoWork::same(Some(&one), Some(&two)), "{two:?}");
    // NEGATIVE CONTROLS.
    let unread = RepoWork {
        state: None,
        ..two.clone()
    };
    assert!(!RepoWork::same(Some(&unread), Some(&unread.clone())));
    assert!(!RepoWork::same(Some(&two), None));
    assert!(RepoWork::same(None, None));
}

/// [`state_of`] OVER `git status -z`: a rename's source field is no entry of its own, a
/// deleted path hashes as no file, and past [`STATE_FILES`] entries the state is not read.
/// NEGATIVE CONTROL: another `HEAD` or another stash is another state over the same listing.
#[test]
fn the_state_reads_the_listing_by_its_entries() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let root = tmp.path();
    write(&root.join("new.rs"), "x\n");
    let listed = "R  new.rs\0old.rs\0 D gone.rs\0";
    let state = state_of(root, "abc", "", listed).unwrap_or_else(|| panic!("read"));
    // The rename's source is its own field, no entry of its own: another source reads the
    // same.
    assert_eq!(
        Some(state),
        state_of(root, "abc", "", "R  new.rs\0older.rs\0 D gone.rs\0")
    );
    assert_ne!(
        Some(state),
        state_of(root, "abc", "", "R  new.rs\0old.rs\0"),
        "the deletion counts"
    );
    assert_ne!(
        Some(state),
        state_of(root, "def", "", listed),
        "another HEAD"
    );
    assert_ne!(
        Some(state),
        state_of(root, "abc", "wip", listed),
        "another stash"
    );
    let many: String = (0..=STATE_FILES).map(|k| format!("?? f{k}.rs\0")).collect();
    assert_eq!(state_of(root, "abc", "", &many), None);
    let enough: String = (0..STATE_FILES).map(|k| format!("?? f{k}.rs\0")).collect();
    assert!(state_of(root, "abc", "", &enough).is_some());
}

/// The probe of `dirs`, with `home` and no `XDG_CONFIG_HOME`, over `table`.
fn probed_in(dirs: &[PathBuf], home: &Path, table: crate::mounts::Table) -> Option<RepoWork> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| panic!("runtime: {err}"));
    completed(
        rt.block_on(probe_session_in(
            dirs,
            home,
            None,
            &Used::default(),
            PROBE_LIMIT,
            table,
        ))
        .read,
    )
}

/// The machine's mount table with `on` mounted too, `local` or a network share, or an
/// automounted volume.
fn with_mount(on: &Path, local: bool, automounted: bool) -> crate::mounts::Table {
    let mut table = crate::mounts::table();
    table.mounts.push(crate::mounts::Mount {
        on: on.to_path_buf(),
        fstype: if automounted { "autofs" } else { "smbfs" }.to_string(),
        local,
        automounted,
    });
    table
}

/// A local parent does not admit a `.git` child mounted elsewhere. The candidate is
/// absent on disk: only the injected mount table makes discovery unknown, without
/// contacting a network share or an automounter. The same real plain folder is clean
/// under its actual local table.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn repository_discovery_guards_each_metadata_candidate_mount() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let dir = std::fs::canonicalize(tmp.path()).unwrap_or_else(|err| panic!("{err}"));
    let candidate = dir.join(".git");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| panic!("{err}"));
    let read = |table| {
        runtime
            .block_on(probe_session_in(
                std::slice::from_ref(&dir),
                &dir,
                None,
                &Used::default(),
                PROBE_LIMIT,
                table,
            ))
            .read
    };
    assert_eq!(read(crate::mounts::table()), RepoRead::Read(None));
    for (local, automounted) in [(false, false), (true, true)] {
        let table = with_mount(&candidate, local, automounted);
        assert!(!no_worktree_metadata(&dir, &table));
        assert_eq!(read(table), RepoRead::Unread);
    }
    assert!(!no_worktree_metadata(
        &dir,
        &crate::mounts::Table::default()
    ));
    let missing = crate::mounts::Table {
        complete: true,
        ..Default::default()
    };
    assert!(!no_worktree_metadata(&dir, &missing));
}

/// A FOLDER ON A SHARE OR AN AUTOMOUNTED VOLUME IS NEVER PROBED (parity review P2, aterm's
/// `resolve_local`): a repository holding work on a mount the table calls a network share, or
/// automounted, finds nothing, spelled there or reached by a link from a local folder. The
/// probe resolves folders over local mounts alone, never touching another (the
/// `canonicalize` before it followed a link onto a share, and could block on a stalled one).
/// NEGATIVE CONTROLS: the same repository on a local mount of its own (another volume) is
/// found, through the link too, and so it is over the machine's own table.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_folder_on_a_share_or_an_automounted_volume_is_never_probed() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let base = std::fs::canonicalize(tmp.path()).unwrap_or_else(|err| panic!("canon: {err}"));
    let share = base.join("share");
    mkdir(&share);
    let work = repo(&share, false);
    write(&work.join("new.txt"), "x\n");
    let local = base.join("local");
    mkdir(&local);
    let via = local.join("via");
    std::os::unix::fs::symlink(&work, &via).unwrap_or_else(|err| panic!("link: {err}"));
    let home = base.join("home");
    mkdir(&home);
    for (local, automounted) in [(false, false), (true, true)] {
        for dir in [&work, &via] {
            assert_eq!(
                probed_in(
                    std::slice::from_ref(dir),
                    &home,
                    with_mount(&share, local, automounted)
                ),
                None,
                "{} on a mount local={local} automounted={automounted}",
                dir.display()
            );
        }
    }
    // NEGATIVE CONTROLS: a local volume of its own, and the machine's table.
    for dir in [&work, &via] {
        assert_eq!(
            probed_in(
                std::slice::from_ref(dir),
                &home,
                with_mount(&share, true, false)
            )
            .map(|w| w.untracked),
            Some(1),
            "{} on another local volume",
            dir.display()
        );
        assert_eq!(
            probed_in(std::slice::from_ref(dir), &home, crate::mounts::table())
                .map(|w| w.untracked),
            Some(1),
            "{}",
            dir.display()
        );
    }
}

/// A GUARDED FOLDER SPELLED IN ANOTHER CASE IS NEVER ENTERED (the guard stricter than aterm's,
/// kept: `canonical_outside`): on a file system that ignores case, `home/documents/work` is
/// the guarded `home/Documents/work`, and the folder is read in the file system's own
/// spelling before the guarded folders are checked, so no `git` runs in it. (aterm's check
/// lets that spelling through: its first `git rev-parse` runs there, inside the guarded
/// folder, and only the top level `git` names, in its own spelling, is refused, so the probe's
/// answer is the same and the read it must never make is made.) NEGATIVE CONTROL: the same
/// spelling change under `Developer` is entered and found. On a file system that keeps case
/// the other spelling is no folder, and nothing is entered either.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_guarded_folder_spelled_in_another_case_is_never_entered() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let home = std::fs::canonicalize(tmp.path())
        .unwrap_or_else(|err| panic!("canon: {err}"))
        .join("home");
    for dir in ["Documents", "Developer"] {
        mkdir(&home.join(dir));
        let work = repo(&home.join(dir), false);
        write(&work.join("new.txt"), "x\n");
    }
    let ignores_case = home.join("DOCUMENTS").exists();
    let table = crate::mounts::table();
    let entered = |dir: &str| unattended_dir(&home.join(dir), &home, &table);
    let first = |dir: &str| probed_in(&[home.join(dir)], &home, table.clone());
    assert_eq!(entered("documents/work"), None, "the guarded folder");
    assert_eq!(first("documents/work"), None, "the guarded folder");
    if ignores_case {
        // NEGATIVE CONTROL.
        assert_eq!(entered("developer/work"), Some(home.join("Developer/work")));
        assert_eq!(first("developer/work").map(|w| w.untracked), Some(1));
    }
}

/// A FOLDER INSIDE A PLANTED REPOSITORY DIRECTORY GIVES WAY (the guard stricter than aterm's,
/// kept: `safe.bareRepository=explicit`): a bare repository directory planted in a work tree
/// whose configuration names another work tree (`core.bare=false`, `core.worktree`) is
/// refused at discovery, so the probe goes on to the next folder (the thread's own) and never
/// probes the work tree that configuration names. NEGATIVE CONTROLS: the next folder's
/// repository is found past it, and a bare repository's own folder finds nothing (no work
/// tree, with or without the guard).
#[test]
fn a_folder_inside_a_planted_repository_directory_gives_way() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let base = std::fs::canonicalize(tmp.path()).unwrap_or_else(|err| panic!("canon: {err}"));
    let mine = base.join("mine");
    mkdir(&mine);
    let mine = repo(&mine, false);
    write(&mine.join("mine.txt"), "x\n");
    let other = base.join("other");
    mkdir(&other);
    let other = repo(&other, false);
    write(&other.join("a.txt"), "changed\n");
    let planted = mine.join("vendor/planted.git");
    mkdir(&planted);
    scratch_git(&planted, &["init", "-q", "--bare", "."]);
    scratch_git(&planted, &["config", "core.bare", "false"]);
    let named = other.display().to_string();
    scratch_git(&planted, &["config", "core.worktree", &named]);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| panic!("runtime: {err}"));
    let first = |dirs: &[PathBuf]| {
        completed(
            rt.block_on(probe_session_within(
                dirs,
                &base,
                &Used::default(),
                PROBE_LIMIT,
            ))
            .read,
        )
    };
    assert_eq!(
        first(std::slice::from_ref(&planted)),
        None,
        "the planted directory"
    );
    let w = first(&[planted, mine.clone()]).unwrap_or_else(|| panic!("the thread's own"));
    assert!(w.repo.ends_with("/mine/work") && w.untracked > 0, "{w:?}");
    let guarded = base.join("Documents/guarded");
    mkdir(&guarded);
    let redirected = base.join("redirected");
    mkdir(&redirected);
    scratch_git(&redirected, &["init", "-q", "."]);
    let guarded_str = guarded.display().to_string();
    scratch_git(&redirected, &["config", "core.worktree", &guarded_str]);
    let redirected_git = redirected.join(".git");
    assert_eq!(first(std::slice::from_ref(&redirected_git)), None);
    let w = first(&[redirected_git, mine]).unwrap_or_else(|| panic!("gives way after guarded top"));
    assert!(w.repo.ends_with("/mine/work") && w.untracked > 0, "{w:?}");
    // NEGATIVE CONTROL: a bare repository's own folder has no work tree.
    let bare = base.join("bare.git");
    mkdir(&bare);
    scratch_git(&bare, &["init", "-q", "--bare", "."]);
    assert_eq!(first(&[bare]), None);
}

/// An actual Git read with an expired shared deadline is not a completed clean
/// check. A later full read can still find work, then prove the repository clean.
#[test]
fn an_expired_probe_is_unread_and_a_later_probe_can_finish() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = repo(tmp.path(), true);
    write(&work.join("new.txt"), "work remains\n");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| panic!("runtime: {err}"));
    let read = rt.block_on(probe_with(
        &work,
        tmp.path(),
        None,
        std::time::Duration::ZERO,
    ));
    assert_eq!(read, RepoRead::Unread);
    assert!(!read.is_clean());
    let later = rt.block_on(probe_with(&work, tmp.path(), None, PROBE_LIMIT));
    assert_eq!(completed(later).map(|w| w.untracked), Some(1));
    scratch_git(&work, &["add", "new.txt"]);
    scratch_git(&work, &["commit", "-q", "-m", "land work"]);
    scratch_git(&work, &["push", "-q", "origin", "HEAD:main"]);
    assert!(
        rt.block_on(probe_with(&work, tmp.path(), None, PROBE_LIMIT))
            .is_clean()
    );
}

#[test]
fn unread_probes_never_prove_unchanged_work() {
    for other in [RepoRead::Read(None), RepoRead::Unread] {
        assert!(!RepoRead::same(RepoRead::Unread, other));
        assert!(!RepoRead::same(other, RepoRead::Unread));
    }
}

fn pushed(work: &Path, name: &str, text: &str) -> String {
    write(&work.join(name), text);
    scratch_git(work, &["add", name]);
    scratch_git(work, &["commit", "-q", "-m", name]);
    scratch_git(work, &["push", "-q", "origin", "HEAD:main"]);
    scratch_git(work, &["rev-parse", "HEAD"]).trim().to_string()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_submodule_checkout_that_only_lags_its_record_is_no_work() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let (lib_dir, app_dir) = (tmp.path().join("lib"), tmp.path().join("app"));
    mkdir(&lib_dir);
    mkdir(&app_dir);
    let lib = repo(&lib_dir, true);
    let app = repo(&app_dir, true);
    let url = lib_dir.join("remote.git").display().to_string();
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
    assert_eq!(found(&app), None, "the submodule at its record");
    let sub = app.join("vendor/lib");
    let first = scratch_git(&sub, &["rev-parse", "HEAD"]).trim().to_string();
    let lagging = |why: &str| {
        assert_eq!(found(&app), None, "{why}");
    };
    let work = |why: &str| {
        let w = found(&app).unwrap_or_else(|| panic!("{why}"));
        assert_eq!(
            (w.changed, w.paths.as_slice()),
            (1, &["vendor/lib".to_string()][..]),
            "{why}: {w:?}"
        );
    };

    pushed(&lib, "b.txt", "two\n");
    scratch_git(&sub, &["fetch", "-q", "origin"]);
    scratch_git(&sub, &["checkout", "-q", "origin/main"]);
    work("a bump not yet committed");
    scratch_git(&app, &["commit", "-q", "-am", "lib two"]);
    scratch_git(&app, &["push", "-q", "origin", "HEAD:main"]);
    assert_eq!(found(&app), None, "the bump committed and pushed");
    scratch_git(&sub, &["checkout", "-q", &first]);
    lagging("a checkout behind its record");
    write(&sub.join("a.txt"), "edited\n");
    work("an edit inside a lagging submodule");
    scratch_git(&sub, &["checkout", "-q", "--", "a.txt"]);
    lagging("the edit undone");

    let unfetched = pushed(&lib, "c.txt", "three\n");
    scratch_git(
        &app,
        &[
            "update-index",
            "--cacheinfo",
            &format!("160000,{unfetched},vendor/lib"),
        ],
    );
    scratch_git(&app, &["commit", "-q", "-m", "lib three"]);
    scratch_git(&app, &["push", "-q", "origin", "HEAD:main"]);
    lagging("a published checkout whose record it never fetched");
    scratch_git(&lib, &["checkout", "-q", "-b", "feature", &first]);
    write(&lib.join("e.txt"), "five\n");
    scratch_git(&lib, &["add", "e.txt"]);
    scratch_git(&lib, &["commit", "-q", "-m", "feature"]);
    scratch_git(&lib, &["push", "-q", "origin", "feature"]);
    scratch_git(&sub, &["fetch", "-q", "origin", "feature"]);
    scratch_git(&sub, &["checkout", "-q", "FETCH_HEAD"]);
    work("a branch switched to, its record never fetched");
    scratch_git(&sub, &["checkout", "-q", &first]);
    lagging("back at a commit it recorded");
    write(&sub.join("d.txt"), "four\n");
    scratch_git(&sub, &["add", "d.txt"]);
    scratch_git(&sub, &["commit", "-q", "-m", "local"]);
    work("an unpushed commit in a submodule whose record it never fetched");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_submodule_checkout_a_merge_moved_past_is_no_work() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let (lib_dir, app_dir) = (tmp.path().join("lib"), tmp.path().join("app"));
    mkdir(&lib_dir);
    mkdir(&app_dir);
    let lib = repo(&lib_dir, true);
    let app = repo(&app_dir, true);
    let url = lib_dir.join("remote.git").display().to_string();
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
    let t2 = pushed(&lib, "b.txt", "two\n");
    let t3 = pushed(&lib, "c.txt", "three\n");
    let sub = app.join("vendor/lib");
    scratch_git(&sub, &["fetch", "-q", "origin"]);
    scratch_git(&app, &["checkout", "-q", "-b", "up"]);
    scratch_git(&sub, &["checkout", "-q", &t3]);
    scratch_git(&app, &["commit", "-q", "-am", "up: lib three"]);
    scratch_git(&app, &["checkout", "-q", "main"]);
    scratch_git(&sub, &["checkout", "-q", &t2]);
    scratch_git(&app, &["commit", "-q", "-am", "lib two"]);
    scratch_git(
        &app,
        &[
            "-c",
            "submodule.recurse=false",
            "merge",
            "-q",
            "--no-edit",
            "up",
        ],
    );
    scratch_git(&app, &["push", "-q", "origin", "HEAD:main"]);
    assert_eq!(
        scratch_git(&app, &["rev-parse", "HEAD:vendor/lib"]).trim(),
        t3,
        "the merge records upstream's bump"
    );
    assert_eq!(
        scratch_git(&app, &["status", "--porcelain"]),
        " M vendor/lib\n",
        "the checkout stays at this checkout's own bump"
    );
    assert_eq!(found(&app), None);
}

#[test]
fn a_reflogs_tail_starts_at_its_first_whole_line() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let size = usize::try_from(LAG_REFLOG).unwrap_or_else(|err| panic!("usize: {err}"));
    for (name, text) in [
        (
            "even",
            (0..(size / 16 + 100))
                .map(|i| format!("line {i:010}\n"))
                .collect::<String>(),
        ),
        (
            "odd",
            std::iter::once("x\n".to_string())
                .chain((0..(size / 12 + 100)).map(|i| format!("line {i:06}\n")))
                .collect(),
        ),
        ("short", "a b c\nd e f\n".to_string()),
    ] {
        let path = tmp.path().join(name);
        write(&path, &text);
        let mut tail = String::new();
        std::io::Read::read_to_string(
            &mut reflog_tail(&path).unwrap_or_else(|| panic!("a file")),
            &mut tail,
        )
        .unwrap_or_else(|err| panic!("read: {err}"));
        let from = text.len().saturating_sub(size);
        let want = (from..text.len())
            .find(|&i| i == 0 || text.as_bytes()[i - 1] == b'\n')
            .unwrap_or_else(|| panic!("a line"));
        assert_eq!(tail, text[want..], "{name}");
    }
}

#[test]
fn reflog_reads_refuse_non_regular_files() {
    let dir = tempfile::tempdir().unwrap();
    assert!(reflog_tail(dir.path()).is_none());
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        let fifo_path = dir.path().join("reflog-fifo");
        let fifo = std::ffi::CString::new(fifo_path.as_os_str().as_bytes()).unwrap();
        // SAFETY: valid NUL-terminated path owned by this private fixture.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(reflog_tail(&fifo_path).is_none());
    }
}
