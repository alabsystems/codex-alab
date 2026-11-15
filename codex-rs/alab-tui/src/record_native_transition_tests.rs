// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native process custody and timer bounds, without an application or a drive lease.

#[path = "../../alab-core/glue/test_child_process.rs"]
mod test_child_process;

use super::FakeCli;
use super::THREAD;
use super::frame;
use crate::identity;
use crate::record;
use crate::session::ProgramState;
use crate::session::Writer;
use clap::CommandFactory;
use codex_alab_core::recovery_clock::Anchor;
use codex_alab_core::recovery_clock::Sample;
use codex_alab_core::session::Autopilot;
use codex_alab_core::session::InFlightRetry;
use codex_alab_core::session::Launch;
use codex_alab_core::session::ProcessIdentity;
use codex_alab_core::session::Recovery;
use codex_alab_core::session::RecoveryWall;
use codex_alab_core::session::Shared;
use codex_alab_core::session::now_ms;
use serde::Deserialize;
use serde::Serialize;
use std::ffi::OsString;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::task::Poll;
use std::time::Duration;
use std::time::Instant;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const EXACT: &str = "session_tests::record_native_transition_tests::native_process_transition_preserves_provider_waits";
const CHILD: &str = "ALAB_RECORD_NATIVE_TRANSITION_CHILD";
const PHASE: &str = "ALAB_RECORD_NATIVE_TRANSITION_PHASE";
const HOME_KEY: &str = "ALAB_RECORD_NATIVE_TRANSITION_HOME";
const CASE: &str = "ALAB_RECORD_NATIVE_TRANSITION_CASE";
const WRITTEN: &str = "ALAB_NATIVE_WAIT_WRITER_READY";
const RECOVERY_WAIT: Duration = Duration::from_secs(10);
const REQUEST_WAIT: Duration = Duration::from_secs(12);

#[derive(Serialize, Deserialize)]
struct NativeControl {
    pid: u32,
    identity: record::Identity,
    origin_ns: u64,
    recovery_deadline_ns: u64,
    request_deadline_ns: u64,
    recovery: record::Recovery,
    in_flight_retry: record::InFlightRetry,
}

#[derive(Serialize, Deserialize)]
struct ReaderObservation {
    case: String,
    pid: u32,
    identity: record::Identity,
    recovery_clock_verified: bool,
    request_clock_verified: bool,
    recovery_woke_ns: Option<u64>,
    request_woke_ns: Option<u64>,
}

fn required(value: Option<OsString>, name: &str) -> io::Result<OsString> {
    value.ok_or_else(|| io::Error::other(format!("missing private child input: {name}")))
}

fn shared(home: &Path, identity: ProcessIdentity) -> TestResult<&'static Shared> {
    let shared = Box::leak(Box::new(Shared::new()));
    assert!(
        shared.set_launch(Launch {
            args: ["codex", "resume", THREAD]
                .into_iter()
                .map(OsString::from)
                .collect(),
            cwd: Some(home.into()),
            current_exe: Some(std::env::current_exe()?),
            argv0: Some(PathBuf::from("codex")),
            pid: std::process::id(),
            identity: Some(identity),
            started_at_ms: now_ms(),
            ..Launch::default()
        })
    );
    shared.set_autopilot(Autopilot::Off);
    Ok(shared)
}

fn native_sample(identity: &ProcessIdentity) -> io::Result<Sample> {
    Sample::read(Some(identity)).ok_or_else(|| io::Error::other("native boot clock unavailable"))
}

fn native_deadline(origin: u64, wait: Duration) -> io::Result<u64> {
    u64::try_from(wait.as_nanos())
        .ok()
        .and_then(|wait| origin.checked_add(wait))
        .ok_or_else(|| io::Error::other("native deadline overflow"))
}

fn write_json(path: &Path, value: &impl Serialize) -> TestResult<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.sync_all()?;
    Ok(())
}

fn source_process(home: &Path) -> TestResult<()> {
    let identity = identity::current()?;
    let shared = shared(home, identity.clone())?;
    let rollout = home.join("rollout.jsonl");
    std::fs::write(&rollout, "{}\n")?;
    shared.observe(
        &frame(Some(&rollout), ProgramState::Working, home),
        now_ms(),
    );
    let sample = native_sample(&identity)?;
    let epoch = now_ms();
    let anchor = sample
        .anchor(epoch)
        .ok_or_else(|| io::Error::other("native anchor unavailable"))?;
    let recovery = Recovery {
        clock: Some(anchor.clone()),
        thread: THREAD.into(),
        turn_id: "failed-native-transition".into(),
        point_at_ms: epoch,
        not_before_ms: epoch + 10_000,
        by_autopilot: true,
        by_person: false,
        wall: RecoveryWall::Server {
            what: "private native transition fixture".into(),
            cut_off: false,
            retry_at_ms: Some(epoch + 10_000),
        },
    };
    let retry = InFlightRetry {
        clock: Some(anchor),
        thread: THREAD.into(),
        turn_id: "waiting-native-transition".into(),
        not_before_ms: epoch + 12_000,
    };
    shared.set_recovery(Some(recovery.clone()));
    shared.set_in_flight_retry(Some(retry.clone()));
    let writer = Writer::new(shared, home.into(), Duration::ZERO, Some(FakeCli::command));
    assert!(writer.write_now()?);
    let stored = record::read(&writer.path())
        .ok_or_else(|| io::Error::other("source record was not readable"))?;
    assert_eq!(stored.pid, std::process::id());
    assert_eq!(stored.identity, Some(record::Identity::from(&identity)));
    assert_eq!(stored.status, record::Status::Busy);
    assert_eq!(stored.exit, None);
    assert_eq!(
        stored.resume_argv,
        Some(["codex", "resume", THREAD].map(str::to_owned).to_vec())
    );
    assert_eq!(stored.recovery.map(Recovery::from), Some(recovery.clone()));
    assert_eq!(
        stored.in_flight_retry.map(InFlightRetry::from),
        Some(retry.clone())
    );
    write_json(
        &home.join("control.json"),
        &NativeControl {
            pid: std::process::id(),
            identity: record::Identity::from(&identity),
            origin_ns: sample.continuous_ns,
            // These controls do not use Anchor::instant_of or serialized epoch arithmetic.
            recovery_deadline_ns: native_deadline(sample.continuous_ns, RECOVERY_WAIT)?,
            request_deadline_ns: native_deadline(sample.continuous_ns, REQUEST_WAIT)?,
            recovery: recovery.into(),
            in_flight_retry: retry.into(),
        },
    )?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{WRITTEN}")?;
    stdout.flush()?;
    // No writer ending or Rust Drop runs: the Busy file is an actual dead process's record.
    std::process::exit(86);
}

fn bracket_deadline(
    anchor: &Anchor,
    bound: u64,
    sample: &Sample,
    before: Instant,
    after: Instant,
    origin_ns: u64,
    deadline_ns: u64,
) -> TestResult<Instant> {
    assert!(
        sample.continuous_ns > origin_ns,
        "actual process transition elapsed time"
    );
    let remaining = Duration::from_nanos(
        deadline_ns
            .checked_sub(sample.continuous_ns)
            .ok_or_else(|| io::Error::other("reader started after the original deadline"))?,
    );
    assert!(
        remaining > Duration::from_millis(500),
        "reader must observe a live wait"
    );
    let restored = anchor
        .instant_of(bound, sample, after)
        .ok_or_else(|| io::Error::other("matching native anchor was refused"))?;
    let lower = before + remaining;
    let upper = after + remaining;
    assert!(
        restored >= lower && restored <= upper,
        "original kernel deadline bracket"
    );
    assert!(
        restored < after + Duration::from_nanos(deadline_ns - origin_ns),
        "elapsed native time must not restart the full wait"
    );
    Ok(restored)
}

async fn timer_fires_after_native_bound(
    identity: &ProcessIdentity,
    at: Instant,
    deadline_ns: u64,
) -> TestResult<u64> {
    let sleep = tokio::time::sleep_until(at.into());
    tokio::pin!(sleep);
    let pending = std::future::poll_fn(|context| {
        Poll::Ready(std::future::Future::poll(sleep.as_mut(), context).is_pending())
    })
    .await;
    assert!(
        pending,
        "restored timer must first be pending on the unpaused runtime"
    );
    sleep.await;
    let woke_ns = native_sample(identity)?.continuous_ns;
    assert!(
        woke_ns >= deadline_ns,
        "timer fired before the independently saved kernel deadline"
    );
    Ok(woke_ns)
}

async fn reader_process(home: &Path, case: &str) -> TestResult<()> {
    let control: NativeControl =
        serde_json::from_slice(&std::fs::read(home.join("control.json"))?)?;
    let old_identity = ProcessIdentity::from(control.identity.clone());
    assert!(!identity::is_live(control.pid, Some(&old_identity)));
    let identity = identity::current()?;
    assert_ne!(std::process::id(), control.pid);
    assert_ne!(identity, old_identity, "distinct actual native births");
    let source = record::read(&record::path_for(home, control.pid))
        .ok_or_else(|| io::Error::other("orphaned source record missing"))?;
    assert_eq!(source.identity, Some(control.identity.clone()));
    assert_eq!(source.session_id.as_deref(), Some(THREAD));
    assert_eq!(source.status, record::Status::Busy);
    assert_eq!(source.exit, None);
    let mut source_recovery = source
        .recovery
        .clone()
        .map(Recovery::from)
        .ok_or_else(|| io::Error::other("source recovery fact missing"))?;
    let mut source_retry = source
        .in_flight_retry
        .clone()
        .map(InFlightRetry::from)
        .ok_or_else(|| io::Error::other("source in-flight fact missing"))?;
    let source_clocks = [source_recovery.clock.take(), source_retry.clock.take()];
    let mut recovery_facts = Recovery::from(control.recovery.clone());
    let mut request_facts = InFlightRetry::from(control.in_flight_retry.clone());
    recovery_facts.clock = None;
    request_facts.clock = None;
    assert_eq!(source_recovery, recovery_facts);
    assert_eq!(source_retry, request_facts);
    let sample = native_sample(&identity)?;
    for clock in source_clocks {
        if case == "missing" {
            assert!(clock.is_none());
        } else {
            let clock =
                clock.ok_or_else(|| io::Error::other("source clock missing before import"))?;
            assert!(
                clock.valid(),
                "wrong-boot control must remain valid codec metadata"
            );
            assert_eq!(clock.boot_id == sample.boot_id, case == "matching");
        }
    }
    let shared = shared(home, identity.clone())?;
    let launch = shared
        .launch()
        .ok_or_else(|| io::Error::other("reader launch missing"))?;
    let expected_args = source
        .resume_argv
        .as_ref()
        .ok_or_else(|| io::Error::other("source record was not resumable"))?;
    assert_eq!(
        launch.args,
        expected_args.iter().map(OsString::from).collect::<Vec<_>>()
    );
    assert!(
        source.updated_at < launch.started_at_ms,
        "real reader birth follows source write"
    );
    assert!(!shared.swept());
    assert!(shared.inherited().is_none());
    assert!(shared.recovery().is_none());
    assert!(shared.in_flight_retry().is_none());
    let writer = Writer::new(shared, home.into(), Duration::ZERO, Some(FakeCli::command));
    writer.prepare();
    assert!(shared.swept());
    let predecessor = shared
        .predecessor(THREAD)
        .ok_or_else(|| io::Error::other("real dead writer was not selected"))?;
    assert_eq!(predecessor.pid, control.pid);
    let mut expected_recovery = Recovery::from(control.recovery.clone());
    let mut expected_retry = InFlightRetry::from(control.in_flight_retry.clone());
    if case != "matching" {
        expected_recovery.clock = None;
        expected_retry.clock = None;
    }
    let imported_recovery = shared
        .recovery()
        .ok_or_else(|| io::Error::other("imported recovery fact missing"))?;
    let imported_retry = shared
        .in_flight_retry()
        .ok_or_else(|| io::Error::other("imported request fact missing"))?;
    assert_eq!(imported_recovery, expected_recovery);
    assert_eq!(imported_retry, expected_retry);
    assert_eq!(shared.owed_act(), None);
    assert_eq!(shared.launch_witness(), None);
    assert!(!shared.launch_probed());
    assert_eq!(shared.autopilot(), Autopilot::Off);
    let (recovery_woke_ns, request_woke_ns) = if case == "matching" {
        let before = Instant::now();
        let sample = native_sample(&identity)?;
        let after = Instant::now();
        let recovery_clock = imported_recovery
            .clock
            .as_ref()
            .ok_or_else(|| io::Error::other("known recovery clock lost"))?;
        let request_clock = imported_retry
            .clock
            .as_ref()
            .ok_or_else(|| io::Error::other("known request clock lost"))?;
        assert_eq!(sample.boot_id, recovery_clock.boot_id);
        assert_eq!(sample.boot_id, request_clock.boot_id);
        let recovery_at = bracket_deadline(
            recovery_clock,
            imported_recovery.not_before_ms,
            &sample,
            before,
            after,
            control.origin_ns,
            control.recovery_deadline_ns,
        )?;
        let request_at = bracket_deadline(
            request_clock,
            imported_retry.not_before_ms,
            &sample,
            before,
            after,
            control.origin_ns,
            control.request_deadline_ns,
        )?;
        let (recovery, request) = tokio::join!(
            timer_fires_after_native_bound(&identity, recovery_at, control.recovery_deadline_ns),
            timer_fires_after_native_bound(&identity, request_at, control.request_deadline_ns),
        );
        (Some(recovery?), Some(request?))
    } else {
        assert!(imported_recovery.clock.is_none());
        assert!(imported_retry.clock.is_none());
        (None, None)
    };
    write_json(
        &home.join("reader.json"),
        &ReaderObservation {
            case: case.into(),
            pid: std::process::id(),
            identity: record::Identity::from(&identity),
            recovery_clock_verified: imported_recovery.clock.is_some(),
            request_clock_verified: imported_retry.clock.is_some(),
            recovery_woke_ns,
            request_woke_ns,
        },
    )?;
    println!("ALAB_NATIVE_WAIT_READER_OK {case}");
    io::stdout().flush()?;
    Ok(())
}

fn child_command(home: &Path, phase: &str, case: &str) -> io::Result<tokio::process::Command> {
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .args(["--exact", EXACT, "--nocapture", "--test-threads=1"])
        .env(CHILD, EXACT)
        .env(PHASE, phase)
        .env(HOME_KEY, home)
        .env(CASE, case)
        .env("CODEX_HOME", home);
    Ok(command)
}

#[test]
fn native_process_transition_preserves_provider_waits() -> TestResult<()> {
    // Dispatch before building any Tokio runtime; the writer exits without runtime teardown.
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child, OsString::from(EXACT));
        let home = PathBuf::from(required(std::env::var_os(HOME_KEY), HOME_KEY)?);
        let case = required(std::env::var_os(CASE), CASE)?;
        let phase = required(std::env::var_os(PHASE), PHASE)?;
        if phase == "writer" {
            assert_eq!(case, "source");
            return source_process(&home);
        }
        assert_eq!(phase, "reader");
        let case = case
            .to_str()
            .ok_or_else(|| io::Error::other("non-UTF8 reader case"))?;
        assert!(matches!(case, "matching" | "missing" | "wrong_boot"));
        return tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(reader_process(&home, case));
    }
    for key in [PHASE, HOME_KEY, CASE] {
        assert!(
            std::env::var_os(key).is_none(),
            "unexpected parent child input: {key}"
        );
    }
    let scratch = tempfile::tempdir()?;
    let source_home = scratch.path().join("source home");
    record::ensure_dir(&source_home)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let encoded: TestResult<String> = runtime.block_on(async {
        let source = test_child_process::run_with_stdout_marker(
            child_command(&source_home, "writer", "source")?,
            Duration::from_secs(10),
            Some(WRITTEN),
        )
        .await
        .map_err(|error| io::Error::other(error.to_string()))?;
        assert!(
            source.status.code() == Some(86)
                && source.stdout.required_marker_seen() == Some(true)
                && source.stdout.complete_eof()
                && source.stderr.complete_eof(),
            "abrupt writer custody: {source:?}"
        );
        let control: NativeControl =
            serde_json::from_slice(&std::fs::read(source_home.join("control.json"))?)?;
        let old_identity = ProcessIdentity::from(control.identity.clone());
        assert!(!identity::is_live(control.pid, Some(&old_identity)));
        let original = record::read(&record::path_for(&source_home, control.pid))
            .ok_or_else(|| io::Error::other("abrupt writer did not leave its Busy record"))?;
        assert_eq!(original.status, record::Status::Busy);
        assert_eq!(original.exit, None);
        let newer = Instant::now() + Duration::from_secs(2);
        while now_ms() <= original.updated_at && Instant::now() < newer {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(
            now_ms() > original.updated_at,
            "real time must advance before the reader launch"
        );
        let mut homes = Vec::new();
        for case in ["matching", "missing", "wrong_boot"] {
            let home = scratch.path().join(case);
            record::ensure_dir(&home)?;
            let mut record = original.clone();
            for clock in [
                record.recovery.as_mut().map(|wait| &mut wait.clock),
                record.in_flight_retry.as_mut().map(|wait| &mut wait.clock),
            ]
            .into_iter()
            .flatten()
            {
                if case == "missing" {
                    *clock = None;
                } else if case == "wrong_boot"
                    && let Some(clock) = clock
                {
                    clock.boot_id.push_str("-different-boot");
                    assert!(Anchor::from(clock.clone()).valid());
                }
            }
            record::write_atomic(&home, &record)?;
            std::fs::copy(source_home.join("control.json"), home.join("control.json"))?;
            homes.push((case, home));
        }
        let mut observations = Vec::new();
        for (case, home) in homes {
            let marker = format!("ALAB_NATIVE_WAIT_READER_OK {case}");
            let timeout = if case == "matching" { 30 } else { 15 };
            let reader = test_child_process::run_with_stdout_marker(
                child_command(&home, "reader", case)?,
                Duration::from_secs(timeout),
                Some(&marker),
            )
            .await
            .map_err(|error| io::Error::other(error.to_string()))?;
            reader.assert_test_success(EXACT);
            let actual: ReaderObservation =
                serde_json::from_slice(&std::fs::read(home.join("reader.json"))?)?;
            assert_eq!(actual.case, case);
            let birth = ProcessIdentity::from(actual.identity.clone());
            assert_ne!(birth, old_identity);
            assert!(!identity::is_live(actual.pid, Some(&birth)));
            assert_eq!(actual.recovery_clock_verified, case == "matching");
            assert_eq!(actual.request_clock_verified, case == "matching");
            if case == "matching" {
                assert!(
                    actual
                        .recovery_woke_ns
                        .is_some_and(|ns| ns >= control.recovery_deadline_ns)
                );
                assert!(
                    actual
                        .request_woke_ns
                        .is_some_and(|ns| ns >= control.request_deadline_ns)
                );
            } else {
                assert_eq!(actual.recovery_woke_ns, None);
                assert_eq!(actual.request_woke_ns, None);
            }
            observations.push(actual);
        }
        assert_eq!(observations.len(), 3);
        let encoded = serde_json::to_string(&serde_json::json!({
            "source_pid": control.pid,
            "source_identity": control.identity,
            "origin_ns": control.origin_ns,
            "recovery_kernel_deadline_ns": control.recovery_deadline_ns,
            "request_kernel_deadline_ns": control.request_deadline_ns,
            "observations": observations,
        }))?;
        assert!(encoded.len() < 8 * 1024, "bounded native observation");
        Ok(encoded)
    });
    drop(runtime);
    let encoded = encoded?;
    scratch.close()?;
    println!("ALAB_NATIVE_WAIT_TRANSITION {encoded}");
    Ok(())
}
