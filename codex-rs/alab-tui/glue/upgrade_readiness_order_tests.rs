// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Isolate inactive readiness components from an active target-refusal case. Real Core
//! completion and App delivery are ordered separately; no case claims signed admission.

use codex_alab_aterm::Endpoint;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use serde::Deserialize;
use serde::Serialize;
use std::io::BufRead;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

const CHILD_TEST: &str =
    "app::alab_session::upgrade_readiness_order_tests::isolated_native_upgrade_finish_case";
const CHILD_ROOT: &str = "ALAB_UI_READINESS_FIXTURE";
const OUTCOME: &str = "ALAB_UI_READINESS_OUTCOME ";
const CONTINUE_TEXT: &str = "Continue the isolated fixture without changing its scope.";
const TARGET_REFUSAL: &str = "the candidate is not a native package entrypoint";

#[path = "upgrade_readiness_order_native_case.rs"]
mod native_case;
use crate::alab_upgrade_fixture_trace as upgrade_fixture_trace;

#[derive(Debug, Deserialize, Serialize)]
struct Fixture {
    version: u8,
    case: String,
    root: PathBuf,
    home: PathBuf,
    codex_home: PathBuf,
    project: PathBuf,
    runtime: PathBuf,
    config: PathBuf,
    state: PathBuf,
    cache: PathBuf,
    tmp: PathBuf,
    sid: String,
    sock: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
struct CaseOutcome {
    case: String,
    completed: bool,
    ui_ready: Vec<bool>,
    preparation_report: Option<String>,
    continuation_posts: usize,
}

#[test]
fn native_completion_delivery_changes_non_authorizing_tui_readiness() -> color_eyre::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let mut kept = codex_alab_tui::test_paths::upgrade_fixture_dir()?;
            kept.disable_cleanup(true);
            writeln!(
                std::io::stderr().lock(),
                "kept UI catch-up witness: {}",
                kept.path().display()
            )?;
            let delivered = run_child(kept.path(), "delivered-first").await?;
            assert!(delivered.completed, "the positive control completed");
            assert_eq!(delivered.ui_ready, [true]);
            assert_eq!(delivered.preparation_report, None);
            assert_eq!(delivered.continuation_posts, 0);
            let lagged = run_child(kept.path(), "held-completion").await?;
            assert!(
                lagged.completed,
                "the lagged case completed its causal controls"
            );
            assert_eq!(lagged.ui_ready, [false, true]);
            assert_eq!(lagged.preparation_report, None);
            assert_eq!(lagged.continuation_posts, 0);
            Ok(())
        })
}

/// Signed-package admission has its own explicit native-host prerequisites. This default
/// negative proves an unprepared target never reaches TUI leave, while real work continues.
#[test]
fn an_unprepared_target_reports_refusal_and_continues_native_work() -> color_eyre::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let mut kept = codex_alab_tui::test_paths::upgrade_fixture_dir()?;
            kept.disable_cleanup(true);
            writeln!(
                std::io::stderr().lock(),
                "kept target-refusal witness: {}",
                kept.path().display()
            )?;
            let outcome = run_child(kept.path(), "target-refused").await?;
            assert!(outcome.completed);
            assert!(
                outcome.ui_ready.is_empty(),
                "an unprepared target never asks the UI to leave"
            );
            assert_eq!(outcome.continuation_posts, 1);
            let report = outcome
                .preparation_report
                .expect("the real preparation diagnostic");
            assert!(report.contains(TARGET_REFUSAL), "{report}");
            Ok(())
        })
}

async fn run_child(root: &Path, case: &str) -> color_eyre::Result<CaseOutcome> {
    let case_root = root.join(case);
    std::fs::create_dir(&case_root)?;
    for name in [
        "home", "codex", "project", "runtime", "config", "state", "cache", "tmp",
    ] {
        std::fs::create_dir(case_root.join(name))?;
    }
    let runtime = case_root.join("runtime");
    std::fs::create_dir(runtime.join("aterm"))?;
    let fake = FakeAterm::start(&runtime.join("aterm"), Mode::Current).await?;
    fake.set_foreground_leader(None);
    fake.set_policy_key("human_grace_s", "0");
    fake.set_policy_key("continue_text", CONTINUE_TEXT);
    let mut handshake_witness = upgrade_fixture_trace::HandshakeWitness::new(
        &fake,
        case_root.join("private-handshake-events.json"),
    );
    let fixture = Fixture {
        version: 2,
        case: case.into(),
        root: case_root.clone(),
        home: case_root.join("home"),
        codex_home: case_root.join("codex"),
        project: case_root.join("project"),
        runtime,
        config: case_root.join("config"),
        state: case_root.join("state"),
        cache: case_root.join("cache"),
        tmp: case_root.join("tmp"),
        sid: fake.sid(),
        sock: fake.endpoint().sock,
    };
    std::fs::write(
        case_root.join("private-fixture.json"),
        serde_json::to_vec_pretty(&fixture)?,
    )?;
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            CHILD_TEST,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        // These configure only the exact private child; the parent's environment is untouched.
        .env(CHILD_ROOT, &case_root)
        .env("HOME", &fixture.home)
        .env("CODEX_HOME", &fixture.codex_home)
        .env("XDG_RUNTIME_DIR", &fixture.runtime)
        .env("XDG_CONFIG_HOME", &fixture.config)
        .env("XDG_STATE_HOME", &fixture.state)
        .env("XDG_CACHE_HOME", &fixture.cache)
        .env("TMPDIR", &fixture.tmp)
        .env("ATERM_PARENT_SESSION_ID", &fixture.sid)
        .env("TERM_PROGRAM", "aterm")
        .env("RUST_MIN_STACK", "8388608")
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .current_dir(&fixture.project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| color_eyre::eyre::eyre!("a spawned child has its exact PID"))?;
    fake.set_foreground_leader(Some(pid));
    let mut gate = child
        .stdin
        .take()
        .ok_or_else(|| color_eyre::eyre::eyre!("owned child startup gate"))?;
    gate.write_all(format!("READY {pid}\n").as_bytes()).await?;
    gate.shutdown().await?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| color_eyre::eyre::eyre!("owned stdout"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| color_eyre::eyre::eyre!("owned stderr"))?;
    let out = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).await.map(|_| bytes)
    });
    let err = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).await.map(|_| bytes)
    });
    let (status, hung) = match tokio::time::timeout(Duration::from_secs(60), child.wait()).await {
        Ok(status) => (status?, false),
        Err(_) => {
            child.kill().await?;
            (child.wait().await?, true)
        }
    };
    let stdout = out.await??;
    let stderr = err.await??;
    std::fs::write(case_root.join("child.stdout"), &stdout)?;
    std::fs::write(case_root.join("child.stderr"), &stderr)?;
    std::fs::write(case_root.join("child.exit"), status.to_string())?;
    std::fs::write(
        case_root.join("private-control-requests.json"),
        serde_json::to_vec_pretty(&fake.requests())?,
    )?;
    handshake_witness.persist()?;
    if hung {
        color_eyre::eyre::bail!("{case}: one-minute child hang detector; output retained");
    }
    let text = String::from_utf8(stdout)?;
    let completed = text.lines().find_map(|line| line.strip_prefix(OUTCOME));
    let Some(completed) = completed else {
        color_eyre::eyre::bail!(
            "{case}: child setup/cleanup did not produce its outcome; {status}; see {}",
            case_root.display()
        );
    };
    let outcome: CaseOutcome = serde_json::from_str(completed)?;
    assert_eq!(outcome.case, case);
    assert!(status.success(), "{case}: fixture child failed: {status}");
    Ok(outcome)
}

#[test]
#[ignore = "exact isolated helper; the parent supplies private routing and a PID gate"]
fn isolated_native_upgrade_finish_case() -> color_eyre::Result<()> {
    assert!(!codex_alab_core::active());
    let root = std::env::var_os(CHILD_ROOT)
        .map(PathBuf::from)
        .ok_or_else(|| color_eyre::eyre::eyre!("missing private child fixture"))?;
    let fixture: Fixture =
        serde_json::from_slice(&std::fs::read(root.join("private-fixture.json"))?)?;
    assert_eq!(fixture.version, 2);
    assert_eq!(fixture.root, root);
    assert!(matches!(
        fixture.case.as_str(),
        "delivered-first" | "held-completion" | "target-refused"
    ));
    for (name, expected) in [
        ("HOME", &fixture.home),
        ("CODEX_HOME", &fixture.codex_home),
        ("XDG_RUNTIME_DIR", &fixture.runtime),
        ("XDG_CONFIG_HOME", &fixture.config),
        ("XDG_STATE_HOME", &fixture.state),
        ("XDG_CACHE_HOME", &fixture.cache),
        ("TMPDIR", &fixture.tmp),
    ] {
        assert_eq!(
            std::env::var_os(name).map(PathBuf::from).as_ref(),
            Some(expected)
        );
        assert!(expected.starts_with(&root));
    }
    assert!(fixture.sock.starts_with(fixture.runtime.join("aterm")));
    assert_eq!(std::env::var("ATERM_PARENT_SESSION_ID")?, fixture.sid);
    assert_eq!(
        codex_alab_aterm::locate::locate_in(&fixture.runtime.join("aterm"), &fixture.sid),
        Some(Endpoint::explicit(&fixture.sock, &fixture.sid))
    );
    for name in [
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "ATPKG_BIN",
        "ATERM_CONTROL_SOCK",
    ] {
        assert!(std::env::var_os(name).is_none(), "no inherited {name}");
    }
    let mut ready = String::new();
    std::io::stdin().lock().read_line(&mut ready)?;
    assert_eq!(ready, format!("READY {}\n", std::process::id()));
    upgrade_fixture_trace::init()?;
    let outcome = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(native_case::run(&fixture))?;
    // libtest may have printed `test name ...` without a newline first.
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "\n{OUTCOME}{}", serde_json::to_string(&outcome)?)?;
    stdout.flush()?;
    Ok(())
}
