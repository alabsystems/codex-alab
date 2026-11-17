// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Kernel resource fixtures, not signed package admission. The actual native
//! admission and real CLI custody checks live in the adjacent integration tests.

use super::*;
use crate::upgrade_admission::Receipt;
use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;

struct Fixture {
    _directory: Option<tempfile::TempDir>,
    root: PathBuf,
    image: PathBuf,
    launch: PathBuf,
    lease: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("bin")).unwrap();
        let image = root.join("bin/codex");
        std::fs::write(&image, b"resource fixture, never executed").unwrap();
        std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o700)).unwrap();
        let launch = root.join("launch");
        symlink(&image, &launch).unwrap();
        Self {
            lease: root.join("original.lease"),
            _directory: Some(directory),
            root,
            image,
            launch,
        }
    }

    fn assert_released(&self) {
        // Parallel exec fixtures can briefly inherit the original descriptor
        // between fork and exec. Queued Unix rights can also retire asynchronously.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match File::open(&self.lease).unwrap().try_lock() {
                Ok(()) => return,
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                other => panic!("original lease was not released: {other:?}"),
            }
        }
    }

    fn plan(&self) -> ExecPlan {
        ExecPlan {
            path: self.launch.clone(),
            argv: vec![
                self.launch.as_os_str().into(),
                "resume".into(),
                "thread-id".into(),
            ],
            env: vec![
                ("PATH".into(), "/original/path".into()),
                ("ORIGINAL".into(), "verbatim value".into()),
            ],
            cwd: Some(self.root.clone()),
        }
    }

    fn prepare(&self) -> PreparedExec {
        PreparedExec::finish(self.plan(), self.admitted()).unwrap()
    }

    fn admitted(&self) -> Received {
        let image = File::open(&self.image).unwrap();
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.lease)
            .unwrap();
        lease.try_lock().unwrap();
        let i = image.metadata().unwrap();
        let l = lease.metadata().unwrap();
        Received {
            receipt: Receipt {
                protocol: 1,
                program: "codex".into(),
                build: 7,
                version: "0.0.0".into(),
                root: self.root.clone(),
                executable: self.image.clone(),
                tree_root: "a".repeat(64),
                device: i.dev(),
                inode: i.ino(),
                size: i.len(),
                modified_seconds: i.mtime(),
                modified_nanoseconds: i.mtime_nsec(),
                lease_path: self.lease.clone(),
                lease_device: l.dev(),
                lease_inode: l.ino(),
            },
            lease,
            image,
        }
    }

    fn replace_launch(&self) {
        let replacement = self.root.join("replacement");
        std::fs::write(&replacement, b"a later release").unwrap();
        std::fs::remove_file(&self.launch).unwrap();
        symlink(replacement, &self.launch).unwrap();
    }

    fn shared(&self) -> &'static Shared {
        use codex_alab_core::session::Autopilot;
        use codex_alab_core::session::Frame;
        use codex_alab_core::session::ProgramState;
        let shared = Box::leak(Box::new(Shared::new()));
        shared.set_launch(Launch {
            argv0: Some(self.launch.clone()),
            cwd: Some(self.root.clone()),
            ..Default::default()
        });
        shared.set_autopilot(Autopilot::Active);
        shared.observe(
            &Frame {
                primary_thread: Some("thread-id"),
                displayed_thread: Some("thread-id"),
                rollout_path: None,
                cwd: &self.root,
                model: "fixture",
                codex_home: &self.root,
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
        shared
    }
}

#[test]
fn prepared_exec_child() {
    let Some(root) = std::env::var_os("ALAB_TEST_PREPARED_EXEC_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let fixture = Fixture {
        _directory: None,
        image: root.join("bin/codex"),
        launch: root.join("launch"),
        lease: root.join("original.lease"),
        root,
    };
    let mut plan = fixture.plan();
    plan.argv = vec![
        fixture.launch.as_os_str().into(),
        "debug".into(),
        "upgrade-capabilities".into(),
        "--hold".into(),
    ];
    plan.env = std::env::vars_os().collect();
    let mut admitted = fixture.admitted();
    admitted.receipt.version = std::env::var("ALAB_TEST_UPGRADE_TARGET_VERSION")
        .expect("set ALAB_TEST_UPGRADE_TARGET_VERSION to the built CLI's product version");
    let prepared =
        PreparedExec::prepare(plan, admitted, Instant::now() + Duration::from_secs(60)).unwrap();
    let shared = fixture.shared();
    let (queue, mut events) = tokio::sync::mpsc::unbounded_channel();
    shared.set_upgrade_leave(Box::new(move |request| {
        let _ = queue.send(request);
    }));
    let generation = shared.upgrade_generation();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let waiter = tokio::spawn(request_admitted(shared, prepared, move || {
                shared.commit_upgrade(generation)
            }));
            assert!(events.recv().await.unwrap().decide(true));
            assert!(waiter.await.unwrap());
        });
    // The committed execution must ignore this now-unrunnable replacement.
    fixture.replace_launch();
    exec_after_teardown(shared);
}

#[test]
#[ignore = "requires an explicitly built Codex CLI with the early custody hook and its product version"]
fn committed_plan_executes_the_original_admitted_image_after_the_launch_shim_changes() {
    struct Reap(Option<std::process::Child>);
    impl Drop for Reap {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let fixture = Fixture::new();
    std::fs::copy(
        std::env::var_os("ALAB_TEST_UPGRADE_TARGET").unwrap(),
        &fixture.image,
    )
    .unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "upgrade::prepared_tests::prepared_exec_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("ALAB_TEST_PREPARED_EXEC_ROOT", &fixture.root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut owner = Reap(Some(child));
    let child = owner.0.as_mut().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let response: crate::upgrade_target::Response = loop {
        let frame = crate::upgrade_admission::read_frame(&mut stdout, deadline).unwrap();
        if let Some(start) = frame.iter().position(|byte| *byte == b'{') {
            break serde_json::from_slice(&frame[start..]).unwrap();
        }
    };
    assert_eq!(response.pid, child.id());
    assert_eq!(response.executable, fixture.image);
    assert!(response.exec_custody_adopted);
    assert!(matches!(
        File::open(&fixture.lease).unwrap().try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    drop(child.stdin.take());
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "committed image did not exit after EOF"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(owner.0.take());
    fixture.assert_released();
}

#[test]
fn prepared_plan_keeps_exact_image_argv_environment_and_directory_after_shim_change() {
    let fixture = Fixture::new();
    let original = fixture.plan();
    let prepared = fixture.prepare();
    fixture.replace_launch();
    prepared.validate().unwrap();
    assert_eq!(prepared.plan.path, fixture.image);
    assert_eq!(prepared.plan.argv, original.argv);
    assert_eq!(prepared.plan.env, original.env);
    assert_eq!(prepared.plan.cwd, original.cwd);
    assert!(matches!(
        File::open(&fixture.lease).unwrap().try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    drop(prepared);
    fixture.assert_released();
}

#[tokio::test]
async fn only_successful_native_commit_installs_the_admitted_plan() {
    for changed in [false, true] {
        let fixture = Fixture::new();
        let prepared = fixture.prepare();
        let shared = fixture.shared();
        let (queue, mut events) = tokio::sync::mpsc::unbounded_channel();
        shared.set_upgrade_leave(Box::new(move |request| {
            let _ = queue.send(request);
        }));
        let generation = shared.upgrade_generation();
        let waiter = tokio::spawn(request_admitted(shared, prepared, move || {
            shared.commit_upgrade(generation)
        }));
        let request = events.recv().await.unwrap();
        assert!(shared.take_upgrade_execution().is_none());
        if changed {
            fixture.replace_launch();
        }
        assert_eq!(request.decide(true), !changed);
        assert_eq!(waiter.await.unwrap(), !changed);
        assert_eq!(shared.exec_pending(), !changed);
        let installed = shared.take_upgrade_execution();
        assert_eq!(installed.is_some(), !changed);
        drop(installed);
    }
}

#[tokio::test]
async fn native_input_image_mutation_and_cancellation_never_commit_the_plan() {
    for cause in ["input", "image", "cancel"] {
        let fixture = Fixture::new();
        let prepared = fixture.prepare();
        let shared = fixture.shared();
        let (queue, mut events) = tokio::sync::mpsc::unbounded_channel();
        shared.set_upgrade_leave(Box::new(move |request| {
            let _ = queue.send(request);
        }));
        let generation = shared.upgrade_generation();
        let waiter = tokio::spawn(request_admitted(shared, prepared, move || {
            shared.commit_upgrade(generation)
        }));
        let request = events.recv().await.unwrap();
        match cause {
            "input" => shared.human_input(20),
            "image" => {
                std::fs::write(&fixture.image, b"changed physical image").unwrap();
            }
            _ => waiter.abort(),
        }
        if cause == "cancel" {
            assert!(waiter.await.err().unwrap().is_cancelled());
            assert!(!request.decide(true));
        } else {
            assert!(!request.decide(true));
            assert!(!waiter.await.unwrap());
        }
        assert!(!shared.exec_pending());
        assert!(shared.take_upgrade_execution().is_none());
        fixture.assert_released();
    }
}

#[test]
fn captured_plan_retains_recovery_until_execution_and_cleans_up_every_refusal() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    for cause in ["uncommitted", "occupied", "mutated", "retained"] {
        let fixture = Fixture::new();
        let prepared = fixture.prepare();
        let shared = fixture.shared();
        if cause != "uncommitted" {
            assert!(shared.commit_upgrade(shared.upgrade_generation()));
        }
        if cause == "occupied" {
            assert!(shared.commit_upgrade_execution(Box::new(|| "prior plan".into()), || true));
        }
        if cause == "mutated" {
            std::fs::write(&fixture.image, b"replaced after capture").unwrap();
        }
        let restored = Arc::new(AtomicUsize::new(0));
        let observed = restored.clone();
        let installed = prepared.install_captured(shared, b"opaque captured state", move || {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        assert_eq!(installed.is_ok(), cause == "retained");
        assert_eq!(
            restored.load(Ordering::SeqCst),
            usize::from(cause != "retained")
        );
        match (cause, shared.take_upgrade_execution()) {
            ("retained", Some(execution)) => {
                assert!(matches!(
                    File::open(&fixture.lease).unwrap().try_lock(),
                    Err(std::fs::TryLockError::WouldBlock)
                ));
                // Dropping an installed plan models teardown failure. No fake
                // executable runs and the actual recovery owner must still fire.
                drop(execution);
                assert_eq!(restored.load(Ordering::SeqCst), 1);
            }
            ("occupied", Some(prior)) => assert_eq!(prior(), "prior plan"),
            ("uncommitted" | "mutated", None) => {}
            _ => panic!("captured installation replaced or lost another plan"),
        }
        fixture.assert_released();
    }
}

#[test]
fn native_command_changes_only_the_fresh_constructor_token() {
    use codex_alab_core::native_source::LAUNCH_ENV;
    use std::collections::BTreeMap;
    use std::os::unix::ffi::OsStrExt;
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
    fn encode(fields: &BTreeMap<&str, String>) -> String {
        hex(fields
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join(" ")
            .as_bytes())
    }
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let receipt = prepared.admitted.receipt();
    let mut expected = prepared.plan.clone();
    expected
        .env
        .push((LAUNCH_ENV.into(), "previous token".into()));
    expected.env.push(("HOME".into(), "/original/home".into()));
    let mut fields = BTreeMap::from([
        ("exec-path", hex(fixture.image.as_os_str().as_bytes())),
        ("exec-dev", receipt.device.to_string()),
        ("exec-ino", receipt.inode.to_string()),
        ("program", hex(fixture.image.as_os_str().as_bytes())),
        ("argv", hex(br#"["resume","thread-id"]"#)),
        (
            "env",
            hex(serde_json::to_string(&vec![
                ("HOME", "/original/home"),
                (LAUNCH_ENV, &"a".repeat(32)),
            ])
            .unwrap()
            .as_bytes()),
        ),
        ("unset", hex(b"null")),
        ("cwd", hex(fixture.root.as_os_str().as_bytes())),
        ("heal", "-".to_owned()),
        ("dialect", "zsh".to_owned()),
    ]);
    let plan = crate::upgrade_native_command::decode(&encode(&fields), &expected, receipt).unwrap();
    assert_eq!(plan.path, expected.path);
    assert_eq!(
        plan.argv, expected.argv,
        "argv0 retains the monitored launch path"
    );
    assert_eq!(plan.cwd, expected.cwd);
    let mut environment = expected.env.clone();
    environment.retain(|(name, _)| name != LAUNCH_ENV);
    environment.push((LAUNCH_ENV.into(), "a".repeat(32).into()));
    assert_eq!(plan.env, environment);
    for cause in [
        "image",
        "inode",
        "argv",
        "cwd",
        "home",
        "missing-home",
        "duplicate",
        "extra",
        "unset",
        "heal",
        "token",
    ] {
        let mut changed = fields.clone();
        match cause {
            "image" => {
                changed.insert("program", hex(b"/other/codex"));
            }
            "inode" => {
                changed.insert("exec-ino", (receipt.inode + 1).to_string());
            }
            "argv" => {
                changed.insert("argv", hex(br#"["resume","another-thread"]"#));
            }
            "cwd" => {
                changed.insert("cwd", hex(b"/other/workspace"));
            }
            "home" => {
                changed.insert(
                    "env",
                    hex(serde_json::to_string(&vec![
                        ("HOME", "/other/home"),
                        (LAUNCH_ENV, &"a".repeat(32)),
                    ])
                    .unwrap()
                    .as_bytes()),
                );
            }
            "missing-home" => {
                changed.insert(
                    "env",
                    hex(serde_json::to_string(&vec![(LAUNCH_ENV, "a".repeat(32))])
                        .unwrap()
                        .as_bytes()),
                );
            }
            "duplicate" => {
                changed.insert(
                    "env",
                    hex(serde_json::to_string(&vec![
                        (LAUNCH_ENV, "a".repeat(32)),
                        (LAUNCH_ENV, "b".repeat(32)),
                    ])
                    .unwrap()
                    .as_bytes()),
                );
            }
            "extra" => {
                changed.insert("account", hex(b"not an execution grant"));
            }
            "unset" => {
                changed.insert("unset", hex(br#"["HOME"]"#));
            }
            "heal" => {
                changed.insert("heal", hex(b"/another/program"));
            }
            _ => {
                changed.insert(
                    "env",
                    hex(serde_json::to_string(&vec![
                        ("HOME", "/original/home"),
                        (LAUNCH_ENV, "malformed capability"),
                    ])
                    .unwrap()
                    .as_bytes()),
                );
            }
        }
        assert!(
            crate::upgrade_native_command::decode(&encode(&changed), &expected, receipt).is_err(),
            "{cause}"
        );
    }
    fields.insert("argv", hex(br#"[ "resume", "thread-id" ]"#));
    assert!(
        crate::upgrade_native_command::decode(&encode(&fields), &expected, receipt).is_err(),
        "noncanonical command data"
    );
}

#[test]
fn failed_prepared_exec_waits_for_fd_receipt_and_releases_actual_custody() {
    const CHILD: &str = "ALAB_TEST_RAW_EXEC_EXCLUSION_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // reset_signals is process-wide: contain the actual exec failure in an
        // isolated test process, using this existing binary in place.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "upgrade::prepared_tests::failed_prepared_exec_waits_for_fd_receipt_and_releases_actual_custody",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("RAW_EXEC_CUSTODY_RELEASED"));
        return;
    }
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let held = codex_alab_core::native_fd_custody::receiving().unwrap();
    let (attempted, attempt) = std::sync::mpsc::channel();
    let (completed, completion) = std::sync::mpsc::channel();
    let execution = std::thread::spawn(move || {
        attempted.send(()).unwrap();
        completed.send(prepared.execute()).unwrap();
    });
    attempt.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        completion.recv_timeout(Duration::from_millis(50)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    drop(held);
    let error = completion.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(error.starts_with("execve "), "{error}");
    execution.join().unwrap();
    fixture.assert_released();
    let _next_receipt = codex_alab_core::native_fd_custody::receiving().unwrap();
    println!("RAW_EXEC_CUSTODY_RELEASED");
}
