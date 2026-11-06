// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use pretty_assertions::assert_eq;

const UPSTREAM: &str = "a = 1\n[history]\npersistence = \"save-all\"\n";

#[test]
fn alab_tables_merge_into_upstreams() {
    let merged = crate::merge_defaults(
        UPSTREAM,
        "b = 2\n[history]\nmax_bytes = 9\n[tui]\nx = true\n",
    )
    .unwrap_or_else(|| panic!("merge"));
    let merged: toml::Table = toml::from_str(&merged).unwrap_or_else(|err| panic!("{err}"));
    let expected: toml::Table = toml::from_str(
        "a = 1\nb = 2\n[history]\npersistence = \"save-all\"\nmax_bytes = 9\n[tui]\nx = true\n",
    )
    .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(merged, expected);
}

#[test]
fn alab_values_win_over_upstreams() {
    let merged = crate::merge_defaults(UPSTREAM, "a = 3\n").unwrap_or_else(|| panic!("merge"));
    let merged: toml::Table = toml::from_str(&merged).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(merged.get("a"), Some(&toml::Value::Integer(3)));
}

#[test]
fn alabs_own_defaults_parse() {
    let parsed: Result<toml::Table, _> = toml::from_str(crate::ALAB_DEFAULTS);
    assert!(parsed.is_ok(), "alab-core/defaults.toml: {parsed:?}");
    // Apply ALab's defaults to the actual compiled upstream layer in either
    // terminal environment, without activating this test process's global switch.
    for in_aterm in [false, true] {
        let merged = crate::alab_defaults(crate::UPSTREAM_DEFAULTS, in_aterm)
            .unwrap_or_else(|| panic!("merge compiled defaults"));
        let merged: toml::Table =
            toml::from_str(&merged).unwrap_or_else(|err| panic!("compiled defaults: {err}"));
        for feature in ["daemon_auto_start", "plugins"] {
            assert_eq!(merged["features"][feature].as_bool(), Some(false));
        }
        assert_eq!(merged["tui"]["notifications"].as_bool(), Some(false));
        assert_eq!(
            merged["tui"]
                .get("alab_progress")
                .and_then(toml::Value::as_bool),
            in_aterm.then_some(true)
        );
    }
}

#[test]
fn the_aterm_only_defaults_are_set_only_inside_aterm() {
    let tui = |in_aterm| {
        let merged = crate::alab_defaults(UPSTREAM, in_aterm).unwrap_or_else(|| panic!("merge"));
        let merged: toml::Table = toml::from_str(&merged).unwrap_or_else(|err| panic!("{err}"));
        merged
            .get("tui")
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default()
    };
    let (inside, outside) = (tui(true), tui(false));
    assert_eq!(inside.get("pet").and_then(toml::Value::as_str), None);
    assert_eq!(
        inside
            .get("notification_method")
            .and_then(toml::Value::as_str),
        Some("osc9")
    );
    assert_eq!(
        inside
            .get("disable_paste_burst")
            .and_then(toml::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        inside.get("alab_progress").and_then(toml::Value::as_bool),
        Some(true)
    );
    // Elsewhere each stays upstream's: no pet (so nothing is written into CODEX_HOME for
    // one), the terminal's own notification method, paste-burst detection on, no OSC 9;4 bar.
    for key in [
        "pet",
        "notification_method",
        "disable_paste_burst",
        "alab_progress",
    ] {
        assert_eq!(outside.get(key), None, "[tui] {key} outside aterm");
    }
    assert!(crate::is_aterm(" ATerm "));
    assert!(!crate::is_aterm("iTerm.app"));
}

#[test]
fn yolo_permission_defaults_are_native_and_limited_to_aterm() {
    let defaults = |inside| {
        let raw = crate::alab_defaults(crate::UPSTREAM_DEFAULTS, inside).expect("defaults");
        toml::from_str::<toml::Table>(&raw).expect("valid defaults")
    };
    let inside = defaults(true);
    assert_eq!(inside["approval_policy"].as_str(), Some("never"));
    assert_eq!(inside["sandbox_mode"].as_str(), Some("danger-full-access"));
    let outside = defaults(false);
    let upstream: toml::Table =
        toml::from_str(crate::UPSTREAM_DEFAULTS).expect("upstream defaults");
    for key in ["approval_policy", "sandbox_mode"] {
        assert_eq!(outside.get(key), upstream.get(key), "{key} outside aterm");
    }
}

#[test]
fn inactive_means_upstream() {
    // This test binary never activates: every switch reads upstream's behavior.
    assert!(!crate::active());
    assert_eq!(crate::packaged_defaults(UPSTREAM), UPSTREAM);
    assert_eq!(
        crate::compiled_packaged_defaults(),
        crate::UPSTREAM_DEFAULTS
    );
    assert!(crate::trace_diagnostics());
    assert!(crate::feedback_tags_logged());
    assert!(crate::turn_git_metadata());
    assert_eq!(crate::home::system_skills_dir(".system"), ".system");
    assert_eq!(
        crate::home::models_cache_file("models_cache.json"),
        "models_cache.json"
    );
    let five_minutes = std::time::Duration::from_secs(300);
    assert_eq!(crate::models_cache_ttl(five_minutes), five_minutes);
    let skills = std::path::Path::new("/h/skills");
    for rule in [".system/x/SKILL.md", ".alab-system/x/SKILL.md"] {
        let rule = skills.join(rule);
        assert_eq!(crate::home::own_skill_rule_path(&rule), rule);
        assert_eq!(crate::home::shared_skill_rule_path(&rule), rule);
    }
    assert!(!crate::home::in_other_system_skills(
        &skills.join(".alab-system"),
        &skills.join(".system/x/SKILL.md")
    ));
    let home = std::path::Path::new("/h");
    assert_eq!(crate::home::auth_file(home), home.join("auth.json"));
    assert_eq!(crate::home::auth_commit_home(home), home);
    assert_eq!(crate::home::store_key_target(home), home);
    assert!(!crate::home::has_auth_override_with(false, |_| {
        Some(std::ffi::OsString::from("/pool/a1/auth.json"))
    }));
}

#[test]
fn codex_auth_path_admits_only_an_absolute_file_when_active() {
    use std::ffi::OsStr;
    use std::ffi::OsString;
    use std::path::PathBuf;

    let scratch = std::env::temp_dir().join(format!("alab-auth-path-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap_or_else(|err| panic!("{err}"));
    let auth = scratch.join("acct-1").join("auth.json");
    let auth_os = auth.as_os_str();

    assert_eq!(
        crate::home::admitted_auth_path_of(true, Some(auth_os)),
        Some(auth.clone()),
        "a non-existent absolute file path is admitted so login can create it"
    );
    assert_eq!(
        crate::home::admitted_auth_path_of(false, Some(auth_os)),
        None,
        "upstream ignores CODEX_AUTH_PATH"
    );
    for rejected in [
        None,
        Some(OsStr::new("")),
        Some(OsStr::new("relative/auth.json")),
        Some(OsStr::new("/pool/acct-1/")),
        Some(OsStr::new("/pool/acct-1/auth.json/.")),
        Some(OsStr::new("/pool/acct-1/auth.json/..")),
        Some(OsStr::new("/pool/acct\n1/auth.json")),
        Some(OsStr::new("/pool/acct\u{2028}1/auth.json")),
        Some(OsStr::new("/pool/acct\u{2029}1/auth.json")),
        Some(scratch.as_os_str()),
    ] {
        assert_eq!(
            crate::home::admitted_auth_path_of(true, rejected),
            None,
            "rejected override: {rejected:?}"
        );
    }

    let lookup = |path: PathBuf| {
        move |name: &str| (name == crate::home::AUTH_PATH_ENV).then(|| OsString::from(&path))
    };
    assert!(
        !crate::home::has_auth_override_with(true, lookup(auth.clone())),
        "a missing file still delays startup composer for first login"
    );
    std::fs::create_dir_all(auth.parent().unwrap_or_else(|| panic!("parent")))
        .unwrap_or_else(|err| panic!("{err}"));
    std::fs::write(&auth, "{}\n").unwrap_or_else(|err| panic!("{err}"));
    assert!(
        crate::home::has_auth_override_with(true, lookup(auth.clone())),
        "an existing per-account auth file satisfies startup preflight"
    );
    assert!(
        !crate::home::has_auth_override_with(false, lookup(auth.clone())),
        "inactive keeps upstream preflight"
    );

    let saved = scratch.join("pool-save").join("acct-2").join("auth.json");
    crate::home::save_auth_bytes(&saved, br#"{"auth_mode":"chatgpt"}"#)
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        std::fs::read_to_string(&saved).unwrap_or_else(|err| panic!("{err}")),
        r#"{"auth_mode":"chatgpt"}"#
    );
    crate::home::save_auth_bytes(&saved, br#"{"auth_mode":"chatgpt","v":2}"#)
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        std::fs::read_to_string(&saved).unwrap_or_else(|err| panic!("{err}")),
        r#"{"auth_mode":"chatgpt","v":2}"#
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let file_mode = std::fs::metadata(&saved)
            .unwrap_or_else(|err| panic!("{err}"))
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        let dir_mode = std::fs::metadata(saved.parent().unwrap_or_else(|| panic!("parent")))
            .unwrap_or_else(|err| panic!("{err}"))
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);

        let readonly = scratch.join("readonly.json");
        std::fs::write(&readonly, "readonly-original").unwrap_or_else(|err| panic!("{err}"));
        std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o400))
            .unwrap_or_else(|err| panic!("{err}"));
        let err = crate::home::save_auth_bytes(&readonly, b"replacement")
            .expect_err("read-only credentials must not be replaced through their parent");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            std::fs::read(&readonly).unwrap_or_else(|err| panic!("{err}")),
            b"readonly-original"
        );
        assert_eq!(
            std::fs::metadata(&readonly)
                .unwrap_or_else(|err| panic!("{err}"))
                .permissions()
                .mode()
                & 0o777,
            0o400
        );

        let linked = scratch.join("hardlinked.json");
        let alias = scratch.join("hardlinked-alias.json");
        std::fs::write(&linked, "shared-original").unwrap_or_else(|err| panic!("{err}"));
        std::fs::hard_link(&linked, &alias).unwrap_or_else(|err| panic!("{err}"));
        let err = crate::home::save_auth_bytes(&linked, b"replacement")
            .expect_err("replacement must not split hard-linked credential identity");
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
        for path in [&linked, &alias] {
            assert_eq!(
                std::fs::read(path).unwrap_or_else(|err| panic!("{err}")),
                b"shared-original"
            );
        }
        use std::os::unix::fs::MetadataExt as _;
        assert_eq!(
            std::fs::metadata(&linked)
                .unwrap_or_else(|err| panic!("{err}"))
                .ino(),
            std::fs::metadata(&alias)
                .unwrap_or_else(|err| panic!("{err}"))
                .ino()
        );

        let directory = scratch.join("not-regular.json");
        std::fs::create_dir(&directory).unwrap_or_else(|err| panic!("{err}"));
        assert!(crate::home::save_auth_bytes(&directory, b"replacement").is_err());
        assert!(directory.is_dir());

        let target = scratch.join("victim.json");
        std::fs::write(&target, "untouched").unwrap_or_else(|err| panic!("{err}"));
        let link = scratch.join("symlink-auth.json");
        std::os::unix::fs::symlink(&target, &link).unwrap_or_else(|err| panic!("{err}"));
        let err = crate::home::save_auth_bytes(&link, b"clobber").expect_err("refuse symlink");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            std::fs::read_to_string(&target).unwrap_or_else(|err| panic!("{err}")),
            "untouched"
        );
    }

    std::fs::remove_dir_all(&scratch).unwrap_or_else(|err| panic!("{err}"));
}

#[cfg(unix)]
fn lock_contender(contender: &std::fs::File) -> std::io::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match contender.try_lock() {
            Ok(()) => return Ok(()),
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(err) => return Err(std::io::Error::from(err)),
        }
    }
}

#[cfg(unix)]
#[test]
fn auth_commit_lock_is_private_stable_and_exclusive_until_its_file_is_dropped()
-> std::io::Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    let scratch =
        std::env::temp_dir().join(format!("alab-auth-lock-{}.noindex", std::process::id()));
    std::fs::create_dir(&scratch)?;
    let auth = scratch.join("private-account/raw-slot.json");
    let sidecar = crate::home::auth_commit_lock_path(&auth);
    let held = crate::home::lock_auth_file(&auth)?;
    assert_eq!(
        std::fs::metadata(auth.parent().unwrap_or_else(|| panic!("parent")))?.mode() & 0o777,
        0o700
    );
    assert_eq!(held.metadata()?.mode() & 0o777, 0o600);
    assert!(
        !auth.exists(),
        "taking a commit lock creates no credentials"
    );
    let inode = held.metadata()?.ino();
    assert_eq!(inode, std::fs::metadata(&sidecar)?.ino());
    let contender = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&sidecar)?;
    assert!(matches!(
        contender.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    drop(held);
    lock_contender(&contender)?;
    drop(contender);
    std::fs::write(&sidecar, b"stable-lock-sentinel")?;
    let again = crate::home::lock_auth_file(&auth)?;
    assert_eq!(
        again.metadata()?.ino(),
        inode,
        "reacquiring never replaces the lock file"
    );
    assert_eq!(std::fs::read(&sidecar)?, b"stable-lock-sentinel");
    drop(again);
    std::fs::remove_dir_all(scratch)
}

#[cfg(unix)]
#[test]
fn auth_commit_lock_refuses_slot_and_sidecar_symlinks_without_outside_writes() -> std::io::Result<()>
{
    use std::os::unix::fs::MetadataExt as _;
    use std::os::unix::fs::PermissionsExt as _;
    let scratch = std::env::temp_dir().join(format!(
        "alab-auth-lock-links-{}.noindex",
        std::process::id()
    ));
    std::fs::create_dir(&scratch)?;
    for slot_link in [true, false] {
        let world = scratch.join(if slot_link { "slot" } else { "sidecar" });
        std::fs::create_dir(&world)?;
        let outside = world.join("outside.json");
        std::fs::write(&outside, b"outside-original")?;
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o640))?;
        let before = std::fs::metadata(&outside)?;
        let auth = world.join("raw-slot.json");
        let sidecar = crate::home::auth_commit_lock_path(&auth);
        let planted = if slot_link { &auth } else { &sidecar };
        std::os::unix::fs::symlink(&outside, planted)?;
        assert!(crate::home::lock_auth_file(&auth).is_err());
        assert_eq!(std::fs::read(&outside)?, b"outside-original");
        let after = std::fs::metadata(&outside)?;
        assert_eq!((after.ino(), after.mode()), (before.ino(), before.mode()));
        assert!(std::fs::symlink_metadata(planted)?.file_type().is_symlink());
        assert!(!crate::home::auth_commit_lock_path(&outside).exists());
        assert!(!crate::auth_lock::lock_path(&outside).exists());
        if slot_link {
            assert!(
                !sidecar.exists(),
                "reject the raw slot before creating any lock"
            );
        } else {
            assert!(!auth.exists());
        }
    }
    std::fs::remove_dir_all(scratch)
}

#[cfg(unix)]
#[test]
fn commit_locks_nest_inside_the_pool_lock_and_do_not_alias_slot_extensions() -> std::io::Result<()>
{
    use std::os::unix::fs::MetadataExt as _;
    let scratch = std::env::temp_dir().join(format!(
        "alab-auth-nested-lock-{}.noindex",
        std::process::id()
    ));
    std::fs::create_dir(&scratch)?;
    let auth = scratch.join("auth.json");
    let other = scratch.join("auth.tokens");
    let pool = crate::auth_lock::AuthLock::try_take(&auth)?
        .unwrap_or_else(|| panic!("private pool lock free"));
    let commit = crate::home::lock_auth_file(&auth)?;
    let separate_slot = crate::home::lock_auth_file(&other)?;
    let pool_path = crate::auth_lock::lock_path(&auth);
    let commit_path = crate::home::auth_commit_lock_path(&auth);
    let other_path = crate::home::auth_commit_lock_path(&other);
    assert_ne!(pool_path, commit_path);
    assert_ne!(commit_path, other_path);
    assert_ne!(
        std::fs::metadata(&pool_path)?.ino(),
        commit.metadata()?.ino()
    );
    assert_ne!(commit.metadata()?.ino(), separate_slot.metadata()?.ino());
    assert!(crate::auth_lock::AuthLock::try_take(&auth)?.is_none());
    let contender = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&commit_path)?;
    assert!(matches!(
        contender.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    drop(commit);
    lock_contender(&contender)?;
    assert!(
        crate::auth_lock::AuthLock::try_take(&auth)?.is_none(),
        "inner release must retain outer hold"
    );
    drop(contender);
    drop(separate_slot);
    drop(pool);
    std::fs::remove_dir_all(scratch)
}

/// A missing skills root whose nearest existing ancestor is the home directory (or above it) is
/// not watched: its FSEvents stream would carry every file event in the home directory. A root
/// that exists, or whose watch lands below the home directory, is; and upstream watches all.
#[test]
fn a_missing_skills_root_in_the_home_directory_is_not_watched() {
    let scratch = std::env::temp_dir().join(format!("alab-skills-watch-{}", std::process::id()));
    let home = scratch.join("home");
    std::fs::create_dir_all(home.join(".codex/skills")).unwrap_or_else(|err| panic!("{err}"));
    std::fs::create_dir_all(home.join("repo")).unwrap_or_else(|err| panic!("{err}"));
    let watches = |active, root: &std::path::Path| {
        crate::watches_skill_root_when(active, root, Some(home.clone()))
    };
    let agents = home.join(".agents/skills");
    assert!(!watches(true, &agents), "the home directory's whole tree");
    assert!(
        !watches(true, &scratch.join("elsewhere/skills")),
        "above the home directory"
    );
    assert!(
        watches(true, &home.join(".codex/skills")),
        "an existing root"
    );
    assert!(
        watches(true, &home.join(".codex/skills/team")),
        "a missing root watched below the home directory"
    );
    assert!(
        watches(true, &home.join("repo/.agents/skills")),
        "a repository's missing root"
    );
    assert!(watches(false, &agents), "upstream");
    assert!(
        crate::watches_skill_root_when(true, &agents, None),
        "no home directory to compare"
    );
    std::fs::remove_dir_all(&scratch).unwrap_or_else(|err| panic!("{err}"));
}

/// The trust store each HTTP client chose is logged only when a CA override is set, where it
/// is the diagnostic a failing request needs; upstream logs it always.
#[test]
fn the_ca_choice_is_logged_only_with_an_override() {
    let none = |_: &str| None;
    assert!(!crate::ca_choice_logged_when(true, none), "no override");
    assert!(
        !crate::ca_choice_logged_when(true, |_| Some(std::ffi::OsString::new())),
        "an empty override is unset"
    );
    for set in ["CODEX_CA_CERTIFICATE", "SSL_CERT_FILE"] {
        assert!(
            crate::ca_choice_logged_when(true, |name| (name == set)
                .then(|| std::ffi::OsString::from("/etc/proxy-ca.pem"))),
            "{set}"
        );
    }
    assert!(crate::ca_choice_logged_when(false, none), "upstream");
}

#[test]
fn vanilla_switch_restores_upstream_defaults_and_behavior() {
    const CHILD: &str = "ALAB_TEST_VANILLA_CHILD";
    if std::env::var_os(CHILD).is_some() {
        crate::activate();
        let mut experience = crate::experience_changes();
        assert!(*experience.borrow_and_update());
        assert!(crate::active());
        assert_ne!(crate::packaged_defaults(UPSTREAM), UPSTREAM);
        crate::set_enhanced(false);
        assert!(experience.has_changed().unwrap());
        assert!(!*experience.borrow_and_update());
        assert!(!crate::active());
        crate::configure_enhanced(true);
        assert!(
            !crate::active(),
            "a background config read cannot undo a live choice"
        );
        assert!(crate::is_alab_binary());
        assert!(crate::own_tui_key(crate::INTERFACE_KEY));
        assert_eq!(crate::packaged_defaults(UPSTREAM), UPSTREAM);
        assert!(crate::trace_diagnostics());
        assert!(crate::turn_git_metadata());
        assert!(crate::feedback_tags_logged());
        crate::set_enhanced(true);
        assert!(experience.has_changed().unwrap());
        assert!(*experience.borrow_and_update());
        assert!(crate::active());
        assert_ne!(crate::packaged_defaults(UPSTREAM), UPSTREAM);
        return;
    }
    // Process-global mode changes cannot race unrelated tests in this binary.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::vanilla_switch_restores_upstream_defaults_and_behavior",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
