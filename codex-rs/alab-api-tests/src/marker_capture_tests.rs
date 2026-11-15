// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A required stdout marker is stream evidence independent of retained output.

use crate::test_child_process::Report;
use std::io::Write;
use std::time::Duration;

const CHILD: &str = "ALAB_REQUIRED_MARKER_CAPTURE_CHILD";
const MODE: &str = "ALAB_REQUIRED_MARKER_CAPTURE_MODE";
const REQUIRED_MARKER: &str = "ALAB_REQUIRED_MARKER:complete\n";
const WRONG_MARKER: &[u8] = b"ALAB_REQUIRED_MARKER:complete-extra\n";
const RETAINED: usize = 64 * 1024;
const NOISE: usize = 96 * 1024;
const FAKE_PASS: &[u8] = b"\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";

fn write_noise(output: &mut impl Write, byte: u8) {
    let chunk = [byte; 4096];
    for _ in 0..NOISE / chunk.len() {
        output.write_all(&chunk).expect("child noise write");
    }
}

fn middle_marker_child() {
    let mut output = std::io::stdout().lock();
    write_noise(&mut output, b'p');
    output.write_all(b"\n").expect("leading noise newline");
    output
        .write_all(REQUIRED_MARKER.as_bytes())
        .expect("middle stdout marker write");
    write_noise(&mut output, b't');
    output.write_all(b"\n").expect("trailing noise newline");
    output.flush().expect("middle stdout marker flush");
    // Return normally: the passing footer must come from the real libtest.
}

fn held_marker_child() -> ! {
    {
        let mut output = std::io::stdout().lock();
        output
            .write_all(REQUIRED_MARKER.as_bytes())
            .expect("held stdout marker write");
        output.flush().expect("held stdout marker flush");
    }
    // Only the parent's process deadline can end this mode. The marker must
    // remain observed while the shared runner kills, reaps and drains the child.
    loop {
        std::thread::park();
    }
}

fn wrong_marker_child() {
    {
        let mut output = std::io::stdout().lock();
        output
            .write_all(WRONG_MARKER)
            .expect("wrong stdout marker write");
        output.flush().expect("wrong stdout marker flush");
    }
    let mut errors = std::io::stderr().lock();
    errors
        .write_all(REQUIRED_MARKER.as_bytes())
        .expect("exact stderr marker write");
    errors.flush().expect("exact stderr marker flush");
}

fn retention_gap_child() {
    let mut output = std::io::stdout().lock();
    write_noise(&mut output, b'Q');
    write_noise(&mut output, b'X');
    write_noise(&mut output, b'R');
    output.write_all(b"\n").expect("gap payload newline");
    output.flush().expect("gap payload flush");
}

fn marked_nonzero_child() -> ! {
    let mut output = std::io::stdout().lock();
    output
        .write_all(REQUIRED_MARKER.as_bytes())
        .expect("nonzero child marker write");
    output
        .write_all(FAKE_PASS)
        .expect("fake passing footer write");
    output.flush().expect("nonzero child output flush");
    // Do not let libtest write its real footer. An observed marker and parsed
    // passing summary cannot turn this actual exit status into success.
    std::process::exit(17);
}

fn contains(bytes: &[u8], marker: &[u8]) -> bool {
    bytes.windows(marker.len()).any(|window| window == marker)
}

fn child_command(exact: &str, mode: &str) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", exact, "--nocapture", "--test-threads=1"])
        .env(CHILD, exact)
        .env(MODE, mode);
    // The shared runner supplies null stdin, both pipes and kill-on-drop.
    command
}

async fn run_marked_child(exact: &str, mode: &str, marker: &str, timeout: Duration) -> Report {
    match crate::test_child_process::run_with_stdout_marker(
        child_command(exact, mode),
        timeout,
        Some(marker),
    )
    .await
    {
        Ok(report) => report,
        Err(error) => panic!("{mode} exact child failed bounded marker capture: {error:?}"),
    }
}

fn assert_eofs_and_bounds(report: &Report) {
    assert!(report.stdout.complete_eof(), "stdout reached real EOF");
    assert!(report.stderr.complete_eof(), "stderr reached real EOF");
    for output in [&report.stdout, &report.stderr] {
        assert!(output.retained_len() <= RETAINED, "retention stays bounded");
        assert!(
            output.prefix().len() + output.tail().len() <= RETAINED,
            "prefix plus tail stays bounded"
        );
    }
}

fn assert_one_pass_summary(report: &Report) {
    let summary = report.stdout.summary().expect("one-test summary observed");
    assert_eq!(summary.passed, 1, "one exact child test passed");
    assert_eq!(summary.failed, 0, "no exact child test failed");
}

fn assert_returned_child(report: &Report) {
    assert!(report.status.success(), "returning child exited zero");
    assert_eofs_and_bounds(report);
    assert_one_pass_summary(report);
}

fn assert_gate_rejects(report: &Report, exact: &str) {
    // Only the isolated single-test controller calls this helper. Keep its
    // expected panic silent and restore its hook before any assertion or await.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let rejected = std::panic::catch_unwind(|| report.assert_test_success(exact));
    std::panic::set_hook(hook);
    assert!(
        rejected.is_err(),
        "invalid marker result must fail the gate"
    );
}

async fn marker_rejection_controls(exact: &str) {
    // Four five-second children plus one failed child's five-second cleanup
    // fit inside the controller's thirty-second deadline; first failure stops.
    let timeout = Duration::from_secs(5);

    let missing = run_marked_child(exact, "missing", REQUIRED_MARKER, timeout).await;
    assert_returned_child(&missing);
    assert_eq!(missing.stdout.required_marker_seen(), Some(false));
    assert_gate_rejects(&missing, exact);

    let wrong = run_marked_child(exact, "wrong", REQUIRED_MARKER, timeout).await;
    assert_returned_child(&wrong);
    assert_eq!(wrong.stdout.required_marker_seen(), Some(false));
    assert!(contains(wrong.stdout.prefix(), WRONG_MARKER));
    assert!(contains(wrong.stderr.prefix(), REQUIRED_MARKER.as_bytes()));
    assert_gate_rejects(&wrong, exact);

    let gap = run_marked_child(exact, "gap", "QR", timeout).await;
    assert_returned_child(&gap);
    assert!(gap.stdout.is_truncated(), "gap noise exceeds retention");
    assert!(gap.stdout.total_bytes >= (3 * NOISE) as u64);
    assert!(gap.stdout.prefix().ends_with(b"Q"));
    assert!(gap.stdout.tail().starts_with(b"R"));
    assert!(!contains(gap.stdout.prefix(), b"QR"));
    assert!(!contains(gap.stdout.tail(), b"QR"));
    let retained = [gap.stdout.prefix(), gap.stdout.tail()].concat();
    assert!(
        contains(&retained, b"QR"),
        "retention gap invents adjacency"
    );
    assert_eq!(gap.stdout.required_marker_seen(), Some(false));
    assert_gate_rejects(&gap, exact);

    let nonzero = run_marked_child(exact, "nonzero", REQUIRED_MARKER, timeout).await;
    assert_eq!(nonzero.status.code(), Some(17), "real nonzero child status");
    assert_eofs_and_bounds(&nonzero);
    assert_one_pass_summary(&nonzero);
    assert_eq!(nonzero.stdout.required_marker_seen(), Some(true));
    assert!(contains(nonzero.stdout.prefix(), FAKE_PASS));
    assert_gate_rejects(&nonzero, exact);
}

#[tokio::test(flavor = "current_thread")]
async fn marker_capture_requires_observed_stdout_and_reaped_success() {
    let exact = format!(
        "{}::marker_capture_requires_observed_stdout_and_reaped_success",
        module_path!().split_once("::").unwrap().1,
    );
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(exact.as_str()));
        match std::env::var(MODE).unwrap().as_str() {
            "middle" => middle_marker_child(),
            "held" => held_marker_child(),
            "missing" => {}
            "wrong" => wrong_marker_child(),
            "gap" => retention_gap_child(),
            "nonzero" => marked_nonzero_child(),
            "gate-controls" => marker_rejection_controls(&exact).await,
            _ => panic!("unknown exact marker child mode"),
        }
        return;
    }

    let middle = run_marked_child(&exact, "middle", REQUIRED_MARKER, Duration::from_secs(30)).await;
    assert_returned_child(&middle);
    assert_eq!(middle.stdout.required_marker_seen(), Some(true));
    assert!(middle.stdout.total_bytes >= (2 * NOISE + REQUIRED_MARKER.len() + 2) as u64);
    assert!(
        middle.stdout.is_truncated(),
        "middle marker noise exceeds retention"
    );
    for output in [&middle.stdout, &middle.stderr] {
        assert!(!contains(output.prefix(), REQUIRED_MARKER.as_bytes()));
        assert!(!contains(output.tail(), REQUIRED_MARKER.as_bytes()));
    }
    middle.assert_test_success(&exact);

    let failure = match crate::test_child_process::run_with_stdout_marker(
        child_command(&exact, "held"),
        Duration::from_secs(2),
        Some(REQUIRED_MARKER),
    )
    .await
    {
        Err(failure) => failure,
        Ok(_) => panic!("marked parked child must time out"),
    };
    // No assertion runs until the runner has completed its kill/reap/drain.
    // Inspect only short metadata and marker booleans, never render the noise.
    assert_eq!(failure.kind, "timeout");
    assert!(
        !failure
            .status
            .as_ref()
            .expect("parked child reaped")
            .success(),
        "parked child was terminated"
    );
    assert!(
        failure.cleanup_error.is_none(),
        "parked child cleanup completed"
    );
    assert!(
        failure.stdout.complete_eof(),
        "cleanup drained stdout to EOF"
    );
    assert!(
        failure.stderr.complete_eof(),
        "cleanup drained stderr to EOF"
    );
    assert_eq!(failure.stdout.required_marker_seen(), Some(true));
    assert!(
        failure.stdout.summary().is_none(),
        "parked child wrote no footer"
    );
    assert!(failure.stdout.total_bytes >= REQUIRED_MARKER.len() as u64);
    assert!(failure.stdout.retained_len() <= RETAINED);
    assert!(!failure.stdout.is_truncated(), "short held output retained");
    let retained = [failure.stdout.prefix(), failure.stdout.tail()].concat();
    assert!(contains(&retained, REQUIRED_MARKER.as_bytes()));

    // The parent's global panic hook is untouched. All expected gate panics
    // occur in this separate exact child, after its own children are reaped.
    let controller = match crate::test_child_process::run(
        child_command(&exact, "gate-controls"),
        Duration::from_secs(30),
    )
    .await
    {
        Ok(report) => report,
        Err(error) => panic!("marker controller failed bounded capture: {error:?}"),
    };
    assert_returned_child(&controller);
    assert_eq!(controller.stdout.required_marker_seen(), None);
    controller.assert_test_success(&exact);
}
