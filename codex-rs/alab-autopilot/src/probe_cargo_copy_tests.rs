// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Cargo copies are built during the session, using actual init/fetch/reset rather than
//! backdated files or fabricated reflogs. Their unchanged source is positively reproducible.

use super::*;

fn initial_copy(at: &Path, source: &Path, pin: &str) {
    mkdir(at);
    scratch_git(at, &["-c", "init.defaultBranch=master", "init", "-q"]);
    scratch_git(at, &["config", "core.logAllRefUpdates", "true"]);
    scratch_git(
        at,
        &[
            "remote",
            "add",
            "origin",
            &format!("file://{}", source.display()),
        ],
    );
    scratch_git(
        at,
        &[
            "fetch",
            "-q",
            "origin",
            pin,
            "+refs/tags/upstream/*:refs/tags/upstream/*",
        ],
    );
    scratch_git(at, &["reset", "--hard", pin]);
    write(&at.join(".git/FETCH_HEAD"), "");
    write(&at.join(".cargo-ok"), "");
}

fn fixture(tmp: &Path, empty_children: usize) -> (PathBuf, PathBuf, PathBuf, String) {
    populated_fixture(tmp, empty_children, 1)
}

fn populated_fixture(
    tmp: &Path,
    empty_children: usize,
    populated_children: usize,
) -> (PathBuf, PathBuf, PathBuf, String) {
    assert!(populated_children > 0);
    let lib_home = tmp.join("lib");
    mkdir(&lib_home);
    let lib = repo(&lib_home, false);
    let child_pin = scratch_git(&lib, &["rev-parse", "HEAD"]).trim().to_owned();
    // An imported side-history tag need not be an ancestor of the selected Cargo pin.
    let side = scratch_git(
        &lib,
        &["commit-tree", "HEAD^{tree}", "-m", "upstream side history"],
    );
    scratch_git(&lib, &["tag", "upstream/imported", side.trim()]);
    let app_home = tmp.join("app");
    mkdir(&app_home);
    let app = repo(&app_home, false);
    scratch_git(
        &app,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            lib.to_str().unwrap(),
            "build",
        ],
    );
    scratch_git(
        &app,
        &[
            "config",
            "--file",
            ".gitmodules",
            "submodule.tools/win.path",
            "tools/win",
        ],
    );
    scratch_git(
        &app,
        &[
            "config",
            "--file",
            ".gitmodules",
            "submodule.tools/win.update",
            "none",
        ],
    );
    scratch_git(
        &app,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{child_pin},tools/win"),
        ],
    );
    // Keep the placeholder present in the SOURCE too: commit -a would stage a missing
    // gitlink's deletion, leaving only its registration and invalidating the factory proof.
    mkdir(&app.join("tools/win"));
    scratch_git(&app, &["add", ".gitmodules"]);
    if empty_children > 0 {
        let mut modules = std::fs::read_to_string(app.join(".gitmodules")).unwrap();
        for n in 0..empty_children {
            mkdir(&app.join(format!("empty-{n}")));
            modules.push_str(&format!(
                "\n[submodule \"empty-{n}\"]\npath = empty-{n}\nupdate = none\n"
            ));
        }
        write(&app.join(".gitmodules"), &modules);
        scratch_git(&app, &["add", ".gitmodules"]);
        let links: Vec<String> = (0..empty_children)
            .map(|n| format!("160000,{child_pin},empty-{n}"))
            .collect();
        for chunk in links.chunks(256) {
            let mut args = vec!["update-index", "--add"];
            for link in chunk {
                args.extend(["--cacheinfo", link.as_str()]);
            }
            scratch_git(&app, &args);
        }
    }
    // Populate the complete SOURCE graph before its initial Cargo copy; adding a
    // child to the checked-out copy would itself be legitimate session work.
    for n in 1..populated_children {
        let name = format!("member-{n}");
        mkdir(&app.join(&name));
        scratch_git(
            &app,
            &[
                "config",
                "--file",
                ".gitmodules",
                &format!("submodule.{name}.path"),
                &name,
            ],
        );
        scratch_git(
            &app,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{child_pin},{name}"),
            ],
        );
    }
    scratch_git(&app, &["commit", "-q", "-am", "Cargo graph"]);
    let gitlinks = scratch_git(&app, &["ls-files", "--stage", "-z"]);
    assert_eq!(
        gitlinks
            .split('\0')
            .filter(|entry| entry.starts_with("160000 "))
            .count(),
        empty_children + populated_children + 1,
        "every source registration must retain its committed gitlink, including empty update-none placeholders"
    );
    assert!(
        gitlinks
            .split('\0')
            .any(|entry| entry == format!("160000 {child_pin} 0\ttools/win"))
    );
    let pin = scratch_git(&app, &["rev-parse", "HEAD"]).trim().to_owned();
    let git_home = tmp.join(".cargo/git");
    let db = git_home.join("db/project-012345");
    mkdir(&db);
    scratch_git(&db, &["init", "-q", "--bare"]);
    scratch_git(
        &db,
        &[
            "fetch",
            "-q",
            app.to_str().unwrap(),
            &format!("HEAD:refs/commit/{pin}"),
        ],
    );
    let checkout = git_home.join("checkouts/project-012345").join(&pin[..7]);
    initial_copy(&checkout, &db, &pin);
    let child = checkout.join("build");
    let child_db = git_home.join("db/library-012345");
    mkdir(&child_db);
    scratch_git(&child_db, &["init", "-q", "--bare"]);
    scratch_git(
        &child_db,
        &[
            "fetch",
            "-q",
            lib.to_str().unwrap(),
            &format!("HEAD:refs/commit/{child_pin}"),
            "+refs/tags/upstream/*:refs/tags/upstream/*",
        ],
    );
    initial_copy(&child, &child_db, &child_pin);
    for n in 1..populated_children {
        initial_copy(&checkout.join(format!("member-{n}")), &child_db, &child_pin);
    }
    mkdir(&checkout.join("tools/win"));
    for n in 0..empty_children {
        mkdir(&checkout.join(format!("empty-{n}")));
    }
    (checkout, child, db, pin)
}

// Successful iterations still clean themselves up. A failed positive control
// can outlive the gate's temporary HOME only when it lives in the owned target.
fn cargo_copy_fixture() -> std::io::Result<tempfile::TempDir> {
    let Some(target) = std::env::var_os("CARGO_TARGET_DIR") else {
        return tempfile::tempdir();
    };
    let target = PathBuf::from(target);
    if !target.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Cargo-copy fixture target must be absolute",
        ));
    }
    let target = target.canonicalize()?;
    if !target.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Cargo-copy fixture target must be an existing directory",
        ));
    }
    tempfile::Builder::new()
        .prefix("alab-cargo-copy-")
        .suffix(".noindex")
        .tempdir_in(target)
}

fn report_copy_file(path: &Path) {
    use std::io::Read as _;
    use std::os::unix::fs::MetadataExt as _;

    let metadata = std::fs::symlink_metadata(path).map(|meta| {
        (
            meta.is_file(),
            meta.file_type().is_symlink(),
            meta.len(),
            meta.ctime(),
            meta.ctime_nsec(),
            meta.mtime(),
            meta.mtime_nsec(),
        )
    });
    let contents = (|| -> std::io::Result<String> {
        let file = std::fs::File::open(path)?;
        let mut text = String::new();
        file.take(4096).read_to_string(&mut text)?;
        Ok(text)
    })();
    eprintln!(
        "{path:?}: (file, symlink, bytes, ctime_s, ctime_ns, mtime_s, mtime_ns)={metadata:?}; first4096={contents:?}"
    );
}

fn retain_failed_copy(
    tmp: tempfile::TempDir,
    members: &[&Path],
    mutation: &str,
    candidate: &Path,
    observed: Option<RepoWork>,
) -> ! {
    let retained = tmp.keep();
    eprintln!("retained failed Cargo-copy positive control: {retained:?}");
    for member in members {
        for name in [
            ".git/HEAD",
            ".git/logs/HEAD",
            ".git/refs/heads/master",
            ".git/refs/tags/upstream/imported",
            ".git/packed-refs",
            ".cargo-ok",
        ] {
            report_copy_file(&member.join(name));
        }
    }
    assert_eq!(
        observed, None,
        "{mutation}: derived candidate {candidate:?} created after session start; fixture retained at {retained:?}"
    );
    unreachable!("the failed positive control must retain its assertion failure")
}

#[test]
fn a_cargo_copy_created_during_the_thread_is_derived_only_with_complete_positive_evidence() {
    for mutation in [
        "edit",
        "child-edit",
        "ignored-child",
        "child-pin",
        "staged-pin",
        "commit-reset",
        "stash",
        "branch",
        "nonempty-marker",
        "missing-marker",
        "missing-log",
        "long-log",
        "disabled-logs",
        "missing-registration",
        "wrong-db",
        "wrong-ref",
        "not-bare",
        "untracked",
        "unknown-child",
        "nonempty-placeholder",
        "refused",
        "new-tag-old-commit",
        "new-tag-unpublished-commit",
        "tag-db-mismatch",
        "packed-tags",
        "symlink-marker",
        "missing-child-metadata",
        "hidden-tracked",
    ] {
        let tmp = cargo_copy_fixture().unwrap();
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let (checkout, child, db, pin) = fixture(tmp.path(), 0);
        for candidate in [&checkout, &child] {
            let looked = touched_at(since, &[candidate]);
            let observed = found_in(&[], tmp.path(), &looked);
            if observed.is_some() {
                retain_failed_copy(
                    tmp,
                    &[checkout.as_path(), child.as_path()],
                    mutation,
                    candidate,
                    observed,
                );
            }
            if mutation == "edit" {
                let whole = found_in(
                    std::slice::from_ref(candidate),
                    tmp.path(),
                    &Used {
                        since: Some(since),
                        ..Used::default()
                    },
                );
                assert!(whole.is_some(), "explicit whole ownership wins");
                let unrelated = Used {
                    branches: vec![codex_alab_core::session::UsedBranch::new(
                        "unrelated-deleted-worktree",
                        None,
                    )],
                    ..looked.clone()
                };
                assert_eq!(
                    found_in(&[], tmp.path(), &unrelated),
                    None,
                    "unrelated fallback branch is not ownership"
                );
                let owned = Used {
                    branches: vec![codex_alab_core::session::UsedBranch::new("master", None)],
                    ..looked
                };
                assert!(
                    found_in(&[], tmp.path(), &owned).is_some(),
                    "an existing matching owned branch wins"
                );
                let qualified = Used {
                    branches: vec![codex_alab_core::session::UsedBranch::new(
                        "refs/heads/master",
                        None,
                    )],
                    ..touched_at(since, &[candidate])
                };
                assert!(
                    found_in(&[], tmp.path(), &qualified).is_some(),
                    "fully-qualified owned branch wins too"
                );
            }
        }
        let looked = touched_at(since, &[&checkout]);
        match mutation {
            "edit" => write(&checkout.join("a.txt"), "mine"),
            "child-edit" => write(&child.join("a.txt"), "mine"),
            "hidden-tracked" => {
                scratch_git(&child, &["update-index", "--assume-unchanged", "a.txt"]);
                write(&child.join("a.txt"), "mine");
            }
            "ignored-child" => {
                scratch_git(&checkout, &["config", "submodule.build.ignore", "all"]);
                write(&child.join("a.txt"), "mine");
            }
            "child-pin" | "staged-pin" | "commit-reset" => {
                local_commit(&child, "mine");
                if mutation == "staged-pin" {
                    scratch_git(&checkout, &["add", "build"]);
                }
                if mutation == "commit-reset" {
                    scratch_git(&child, &["reset", "--hard", "HEAD^"]);
                }
            }
            "stash" => {
                write(&child.join("a.txt"), "mine");
                scratch_git(&child, &["stash", "push", "-q"]);
            }
            "branch" => {
                scratch_git(&child, &["branch", "parked"]);
            }
            "nonempty-marker" => write(&child.join(".cargo-ok"), "not empty"),
            "missing-marker" => std::fs::remove_file(child.join(".cargo-ok")).unwrap(),
            "missing-log" => std::fs::remove_file(child.join(".git/logs/HEAD")).unwrap(),
            "long-log" => {
                let log = child.join(".git/logs/HEAD");
                let initial = std::fs::read_to_string(&log).unwrap();
                write(&log, &format!("{}\n{initial}", "x".repeat(8192)));
            }
            "disabled-logs" => {
                scratch_git(&child, &["config", "core.logAllRefUpdates", "false"]);
            }
            "missing-registration" => std::fs::remove_file(checkout.join(".gitmodules")).unwrap(),
            "wrong-db" => {
                scratch_git(
                    &checkout,
                    &[
                        "remote",
                        "set-url",
                        "origin",
                        &format!("file://{}", db.parent().unwrap().display()),
                    ],
                );
            }
            "wrong-ref" => {
                scratch_git(&db, &["update-ref", "-d", &format!("refs/commit/{pin}")]);
            }
            "not-bare" => {
                scratch_git(&db, &["config", "core.bare", "false"]);
            }
            "untracked" => write(&child.join("mine"), "mine"),
            "unknown-child" => {
                let rogue = checkout.join("rogue");
                mkdir(&rogue);
                scratch_git(&rogue, &["init", "-q"]);
                write(&rogue.join("mine"), "mine");
            }
            "nonempty-placeholder" => write(&checkout.join("tools/win/mine"), "mine"),
            "new-tag-old-commit" | "new-tag-unpublished-commit" => {
                let object = if mutation == "new-tag-old-commit" {
                    scratch_git(&child, &["rev-parse", "HEAD"])
                } else {
                    scratch_git(
                        &child,
                        &[
                            "commit-tree",
                            "HEAD^{tree}",
                            "-p",
                            "HEAD",
                            "-m",
                            "my unpublished source",
                        ],
                    )
                };
                scratch_git(&child, &["tag", "upstream/mine", object.trim()]);
                let child_db = db.parent().unwrap().join("library-012345");
                scratch_git(
                    &child_db,
                    &[
                        "fetch",
                        "-q",
                        child.to_str().unwrap(),
                        "refs/tags/upstream/mine:refs/tags/upstream/mine",
                    ],
                );
                // Refreshing the marker cannot advance the first-HEAD initialization bound.
                write(&child.join(".cargo-ok"), "");
            }
            "tag-db-mismatch" => {
                let child_db = db.parent().unwrap().join("library-012345");
                let child_pin = scratch_git(&child, &["rev-parse", "HEAD"]);
                scratch_git(
                    &child_db,
                    &[
                        "update-ref",
                        "refs/tags/upstream/imported",
                        child_pin.trim(),
                    ],
                );
            }
            "packed-tags" => {
                scratch_git(&child, &["pack-refs", "--all", "--prune"]);
            }
            "symlink-marker" => {
                let target = tmp.path().join("empty-marker-target");
                write(&target, "");
                std::fs::remove_file(child.join(".cargo-ok")).unwrap();
                std::os::unix::fs::symlink(target, child.join(".cargo-ok")).unwrap();
            }
            "missing-child-metadata" => std::fs::remove_dir_all(child.join(".git")).unwrap(),
            "refused" => {
                // The production reader canonicalizes /var/... to /private/var/... on
                // macOS. Match its actual path and the new provenance read, not a temp alias.
                let blocked = std::fs::canonicalize(&checkout).unwrap();
                let fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let observed = fired.clone();
                let refuse: Refuse = Arc::new(move |dir: &Path, args: &[&str]| {
                    let refuse = dir == blocked.as_path()
                        && args
                            == [
                                "--no-replace-objects",
                                "config",
                                "--get-all",
                                "remote.origin.url",
                            ];
                    if refuse {
                        observed.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    refuse
                });
                let result = runtime().block_on(probe_refusing(&[], tmp.path(), &looked, refuse));
                assert!(
                    fired.load(std::sync::atomic::Ordering::SeqCst),
                    "the factory provenance read was actually refused"
                );
                assert_ne!(
                    result.read,
                    RepoRead::Read(None),
                    "refused provenance must hold landing"
                );
                continue;
            }
            _ => unreachable!(),
        }
        assert_ne!(
            read_session(&[], tmp.path(), &looked),
            RepoRead::Read(None),
            "{mutation}: derived-copy exemption must refuse actual work or missing evidence"
        );
    }
}

#[test]
fn a_derived_factory_graph_beyond_the_shared_proof_bound_is_not_exempted() {
    let tmp = tempfile::tempdir().unwrap();
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // These are genuine registered, intentionally empty update-none children, with the same
    // factory provenance as the small positive graph. Only their number exceeds the bound.
    let (checkout, _, _, _) = fixture(tmp.path(), STATE_FILES);
    assert_ne!(
        read_session(&[], tmp.path(), &touched_at(since, &[&checkout])),
        RepoRead::Read(None)
    );
}

/// Explicit host diagnostic: the production probe only reads the supplied real cache.
#[test]
#[ignore = "requires ALAB_CARGO_COPY_PROOF_ROOT, ALAB_CARGO_COPY_PROOF_CHILD and ALAB_CARGO_COPY_PROOF_SINCE"]
fn a_real_cargo_copy_proves_its_complete_graph_within_the_shared_deadline() {
    let root = PathBuf::from(
        std::env::var_os("ALAB_CARGO_COPY_PROOF_ROOT")
            .expect("explicit Cargo checkout root required"),
    );
    let relative = PathBuf::from(
        std::env::var_os("ALAB_CARGO_COPY_PROOF_CHILD")
            .expect("explicit registered child path required"),
    );
    let since = std::env::var("ALAB_CARGO_COPY_PROOF_SINCE")
        .expect("explicit thread creation time required")
        .parse::<u64>()
        .expect("thread creation time must be Unix seconds");
    assert!(root.is_absolute() && since > 0);
    assert!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
    );
    let child = root.join(relative);
    for candidate in [&root, &child] {
        let git = std::fs::symlink_metadata(candidate.join(".git"))
            .expect("actual Git metadata required");
        assert!(git.is_file() || git.is_dir());
        let marker = std::fs::symlink_metadata(candidate.join(".cargo-ok"))
            .expect("actual Cargo marker required");
        assert!(marker.is_file() && marker.len() == 0);
    }
    let home = PathBuf::from(std::env::var_os("HOME").expect("the probe's real HOME is required"));
    let started = std::time::Instant::now();
    let result = read_session(&[], &home, &touched_at(since, &[&root, &child]));
    eprintln!(
        "Cargo derived-copy complete probe elapsed={:.3}s result={result:?}",
        started.elapsed().as_secs_f64()
    );
    assert_eq!(
        result,
        RepoRead::Read(None),
        "the real root and registered child must both be proven derived, never unread or silently skipped"
    );
}

/// Explicit host diagnostic: reproduce a saved session's complete production scope.
#[test]
#[ignore = "requires ALAB_CARGO_COPY_PROOF_SNAPSHOT and ALAB_CARGO_COPY_PROOF_CWD"]
fn a_saved_real_scope_completes_within_the_shared_deadline() {
    let snapshot = PathBuf::from(
        std::env::var_os("ALAB_CARGO_COPY_PROOF_SNAPSHOT")
            .expect("explicit saved session snapshot required"),
    );
    let cwd = PathBuf::from(
        std::env::var_os("ALAB_CARGO_COPY_PROOF_CWD")
            .expect("explicit actual session cwd required"),
    );
    let home = PathBuf::from(std::env::var_os("HOME").expect("the probe's real HOME is required"));
    assert!(snapshot.is_absolute() && cwd.is_absolute() && home.is_absolute());
    assert!(cwd.is_dir() && home.is_dir());
    let saved: serde_json::Value = serde_json::from_slice(
        &std::fs::read(snapshot)
            .expect("the explicitly supplied session snapshot must be readable"),
    )
    .expect("the snapshot must contain valid JSON");
    let scope = &saved["probeScope"];
    let dirs: Vec<PathBuf> = serde_json::from_value(scope["dirs"].clone())
        .expect("the snapshot must contain typed probeScope dirs");
    let since = scope["since"]
        .as_u64()
        .expect("the original since is required");
    let branches = scope["branches"]
        .as_array()
        .expect("the complete original branch list is required");
    assert!(since > 0 && !dirs.is_empty());
    // Match extension::probe_repo: newest touched folders first, deduplicated,
    // with only the actual thread cwd promoted to a whole-repository input.
    let mut touched = Vec::new();
    for dir in dirs.into_iter().rev() {
        assert!(dir.is_absolute());
        if dir != cwd && !touched.contains(&dir) {
            touched.push(dir);
        }
    }
    let used = Used {
        since: Some(since),
        branches: branches
            .iter()
            .map(|branch| {
                let name = branch["name"].as_str().expect("an original branch name");
                let dir: Option<PathBuf> = serde_json::from_value(branch["dir"].clone())
                    .expect("an original optional branch directory");
                codex_alab_core::session::UsedBranch::new(name, dir.as_deref())
            })
            .collect(),
        touched,
    };
    let started = std::time::Instant::now();
    let result = runtime().block_on(probe_session_within(&[cwd], &home, &used, PROBE_LIMIT));
    eprintln!(
        "Saved complete session probe elapsed={:.3}s result={result:?}",
        started.elapsed().as_secs_f64()
    );
    assert_eq!(
        result.read,
        RepoRead::Read(None),
        "the complete actual scope must finish clean, never cut or conservatively hold inherited Cargo copies"
    );
}

#[test]
fn five_derived_members_share_one_bounded_source_read_but_not_the_next_probe() {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    let tmp = tempfile::tempdir().unwrap();
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (checkout, child, _, _) = populated_fixture(tmp.path(), 0, 4);
    let members = [
        checkout.clone(),
        child.clone(),
        checkout.join("member-1"),
        checkout.join("member-2"),
        checkout.join("member-3"),
    ];
    let looked = touched_at(
        since,
        &members.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
    );
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&reads);
    // The index-flags read is required evidence for every populated factory
    // member. Permit one full graph's actual Git reads, refusing repeated scans.
    let refuse: Refuse = Arc::new(move |_dir: &Path, args: &[&str]| {
        args.ends_with(&["ls-files", "-v", "-z"]) && counted.fetch_add(1, Ordering::SeqCst) >= 5
    });
    let result = runtime().block_on(probe_refusing(&[], tmp.path(), &looked, refuse));
    assert_eq!(
        result.read,
        RepoRead::Read(None),
        "five members must fit one source-read allowance"
    );
    assert_eq!(
        reads.load(Ordering::SeqCst),
        5,
        "the complete graph was actually inspected"
    );
    write(&child.join("new-session-work"), "keep this new source work");
    assert_ne!(
        read_session(&[], tmp.path(), &looked),
        RepoRead::Read(None),
        "a fresh probe must not retain the previous clean graph observation"
    );
}

#[test]
fn a_later_scoped_owner_and_whole_intent_cannot_reuse_an_unowned_copy_proof() {
    let tmp = tempfile::tempdir().unwrap();
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (checkout, child, _, _) = populated_fixture(tmp.path(), 0, 4);
    let looked = touched_at(since, &[&checkout, &child]);
    assert_eq!(read_session(&[], tmp.path(), &looked), RepoRead::Read(None));
    let owned = Used {
        branches: vec![codex_alab_core::session::UsedBranch::new(
            "master",
            Some(&child),
        )],
        ..looked.clone()
    };
    // Root is inspected first with no effective owned names; the child's later
    // matching branch cannot inherit that observation's weaker ownership scope.
    assert_ne!(read_session(&[], tmp.path(), &owned), RepoRead::Read(None));
    assert_ne!(
        read_session(std::slice::from_ref(&child), tmp.path(), &looked),
        RepoRead::Read(None),
        "explicit whole intent still owns the inherited history"
    );
}

#[test]
fn an_absent_factory_origin_is_not_a_refused_factory_read() {
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    let tmp = tempfile::tempdir().unwrap();
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (checkout, child, _, _) = fixture(tmp.path(), 0);
    for root in [&checkout, &child] {
        scratch_git(root, &["remote", "remove", "origin"]);
        std::fs::remove_file(root.join(".cargo-ok")).unwrap();
    }
    let looked = touched_at(since, &[&checkout, &child]);
    assert_eq!(
        read_session(&[], tmp.path(), &looked),
        RepoRead::Read(None),
        "quiet missing Cargo provenance must leave ordinary clean repositories clean"
    );
    let root = checkout.canonicalize().unwrap();
    let fired = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&fired);
    let refuse: Refuse = Arc::new(move |dir: &Path, args: &[&str]| {
        let refused = dir == root
            && args
                == [
                    "--no-replace-objects",
                    "config",
                    "--get-all",
                    "remote.origin.url",
                ];
        if refused {
            observed.store(true, Ordering::SeqCst);
        }
        refused
    });
    let result = runtime().block_on(probe_refusing(&[], tmp.path(), &looked, refuse));
    assert!(
        fired.load(Ordering::SeqCst),
        "the actual factory origin read was refused"
    );
    assert_eq!(
        result.read,
        RepoRead::Unread,
        "a refused provenance read must not become quiet absence"
    );
}
