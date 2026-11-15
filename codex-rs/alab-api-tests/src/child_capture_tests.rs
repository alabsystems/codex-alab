// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real child pipes must keep draining after retained output reaches its cap.

use crate::test_child_process::Report;
use std::io::Write;
use std::time::Duration;

const CHILD: &str = "ALAB_CHILD_CAPTURE_REGRESSION_CHILD";
const MODE: &str = "ALAB_CHILD_CAPTURE_REGRESSION_MODE";
const SMALL: usize = 1024;
const LARGE: usize = 4 * 1024 * 1024;
const RETAINED: usize = 64 * 1024;
const STDOUT_PREFIX: &[u8] = b"\nchild-capture-stdout-prefix\n";
const STDOUT_TAIL: &[u8] = b"\nchild-capture-stdout-tail\n";
const STDERR_PREFIX: &[u8] = b"\nchild-capture-stderr-prefix\n";
const STDERR_TAIL: &[u8] = b"\nchild-capture-stderr-tail\n";
const HELD_STDOUT: &[u8] = b"\nchild-capture-held-stdout\n";
const HELD_STDERR: &[u8] = b"\nchild-capture-held-stderr\n";
const FAKE_PASS: &[u8] = b"\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";

fn write_payload(mut output: impl Write, prefix: &[u8], tail: &[u8], length: usize, byte: u8) {
    output.write_all(prefix).expect("child prefix write");
    let chunk = [byte; 4096];
    let mut remaining = length;
    while remaining != 0 {
        let count = remaining.min(chunk.len());
        output
            .write_all(&chunk[..count])
            .expect("child payload write");
        remaining -= count;
    }
    output.write_all(tail).expect("child trailing marker write");
    output.flush().expect("child pipe flush");
}

fn child_output(length: usize) {
    // Both real OS pipes can backpressure independently. Join both writers
    // before returning, so only libtest can emit the successful test summary.
    let stdout = std::thread::spawn(move || {
        write_payload(
            std::io::stdout().lock(),
            STDOUT_PREFIX,
            STDOUT_TAIL,
            length,
            b'o',
        );
    });
    let stderr = std::thread::spawn(move || {
        write_payload(
            std::io::stderr().lock(),
            STDERR_PREFIX,
            STDERR_TAIL,
            length,
            b'e',
        );
    });
    stdout.join().expect("stdout writer joined");
    stderr.join().expect("stderr writer joined");
}

fn held_child() -> ! {
    write_payload(std::io::stdout().lock(), HELD_STDOUT, b"", 0, b'o');
    write_payload(std::io::stderr().lock(), HELD_STDERR, b"", 0, b'e');
    // Both markers are flushed before parking. Only the parent's real process
    // timeout/kill can end this mode; no timer or successful return exists here.
    loop {
        std::thread::park();
    }
}

fn injected_summary(overlong: bool) -> ! {
    let mut output = std::io::stdout().lock();
    output
        .write_all(FAKE_PASS)
        .expect("valid injected summary write");
    if overlong {
        output
            .write_all(b"test result: ")
            .expect("oversized summary anchor write");
        output
            .write_all(&[b'x'; 4097])
            .expect("oversized summary bytes write");
        output
            .write_all(b"\n")
            .expect("oversized summary newline write");
    } else {
        output
            .write_all(b"test result: ok. malformed counts\n")
            .expect("malformed summary write");
    }
    output.flush().expect("injected summary flush");
    // Do not return to libtest: its real passing footer would overwrite the
    // deliberately invalid final result candidate and hide the stale-summary bug.
    std::process::exit(0);
}

fn contains(bytes: &[u8], marker: &[u8]) -> bool {
    bytes.windows(marker.len()).any(|window| window == marker)
}

fn assert_summary_and_bounds(report: &Report, length: usize) {
    // Never render the captured payload in a parent assertion. All evidence
    // here is short status/count metadata or booleans about known markers.
    assert!(report.status.success(), "child exited unsuccessfully");
    let summary = report
        .stdout
        .summary()
        .expect("real libtest summary retained");
    assert_eq!(summary.passed, 1, "one exact child test ran");
    assert_eq!(summary.failed, 0, "the exact child test passed");
    assert!(
        report.stdout.total_bytes >= (length + STDOUT_PREFIX.len() + STDOUT_TAIL.len()) as u64,
        "all stdout payload bytes were drained and counted"
    );
    assert_eq!(
        report.stderr.total_bytes,
        (length + STDERR_PREFIX.len() + STDERR_TAIL.len()) as u64,
        "all stderr payload bytes were drained and counted"
    );
    for output in [&report.stdout, &report.stderr] {
        assert!(
            output.retained_len() <= RETAINED,
            "retained output is bounded"
        );
        assert!(
            output.prefix().len() + output.tail().len() <= RETAINED,
            "prefix plus rolling tail remains bounded"
        );
    }
    assert!(
        contains(report.stdout.prefix(), STDOUT_PREFIX),
        "stdout prefix retained"
    );
    assert!(
        contains(report.stderr.prefix(), STDERR_PREFIX),
        "stderr prefix retained"
    );
}

async fn run_child(exact: &str, mode: &str, timeout: Duration) -> Report {
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", exact, "--nocapture", "--test-threads=1"])
        .env(CHILD, exact)
        .env(MODE, mode);
    // The common helper supplies null stdin, piped outputs, the retained child
    // handle and bounded kill/reap. It must return only after real child exit.
    match crate::test_child_process::run(command, timeout).await {
        Ok(report) => report,
        Err(error) => panic!("{mode} exact child failed bounded capture: {error:?}"),
    }
}

fn assert_gate_rejects(report: &Report, exact: &str) {
    // Called only in the single-test gate-controls child. Suppress the expected
    // panic's formatted captured output without changing the parallel parent's
    // global hook. Restore it before any subsequent assertion or await.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let rejected = std::panic::catch_unwind(|| report.assert_test_success(exact));
    std::panic::set_hook(hook);
    assert!(
        rejected.is_err(),
        "invalid child result must fail the success gate"
    );
}

async fn summary_rejection_controls(exact: &str) {
    // These tiny nested controls must leave room for the runner's five-second
    // cleanup before the gate-controls parent's thirty-second deadline.
    let inner_timeout = Duration::from_secs(5);
    for mode in ["malformed-summary", "oversized-summary"] {
        let report = run_child(exact, mode, inner_timeout).await;
        assert!(report.status.success(), "injected result child exits zero");
        assert!(report.stdout.complete_eof() && report.stderr.complete_eof());
        assert!(
            contains(report.stdout.prefix(), FAKE_PASS),
            "valid earlier summary was actually written"
        );
        assert!(
            report.stdout.summary().is_none(),
            "later invalid candidate clears earlier passing summary"
        );
        assert_gate_rejects(&report, exact);
    }

    let absent = "child_capture_tests::intentionally_absent_exact_filter_7e4c";
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", absent, "--nocapture", "--test-threads=1"])
        .env_remove(CHILD)
        .env_remove(MODE);
    let report = match crate::test_child_process::run(command, inner_timeout).await {
        Ok(report) => report,
        Err(error) => panic!("zero-test exact child failed bounded capture: {error:?}"),
    };
    assert!(
        report.status.success(),
        "libtest exits zero for an unmatched filter"
    );
    assert!(report.stdout.complete_eof() && report.stderr.complete_eof());
    let summary = report
        .stdout
        .summary()
        .expect("real zero-test libtest summary");
    assert_eq!(summary.passed, 0);
    assert_eq!(summary.failed, 0);
    assert_gate_rejects(&report, absent);
}

#[tokio::test(flavor = "current_thread")]
async fn noisy_exact_child_drains_both_pipes_and_preserves_real_summary() {
    let exact = format!(
        "{}::noisy_exact_child_drains_both_pipes_and_preserves_real_summary",
        module_path!().split_once("::").unwrap().1,
    );
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(exact.as_str()));
        let length = match std::env::var(MODE).unwrap().as_str() {
            "small" => SMALL,
            "large" => LARGE,
            "held" => held_child(),
            "malformed-summary" => injected_summary(false),
            "oversized-summary" => injected_summary(true),
            "gate-controls" => {
                summary_rejection_controls(&exact).await;
                return;
            }
            _ => panic!("unknown exact child output mode"),
        };
        child_output(length);
        return;
    }

    // The small complete child proves the exact filter, process startup,
    // summary parser and normal stdout/stderr fixture before the noisy case.
    let small = run_child(&exact, "small", Duration::from_secs(30)).await;
    assert_summary_and_bounds(&small, SMALL);
    assert!(!small.stdout.is_truncated());
    assert!(!small.stderr.is_truncated());
    for (output, marker) in [(&small.stdout, STDOUT_TAIL), (&small.stderr, STDERR_TAIL)] {
        let retained = [output.prefix(), output.tail()].concat();
        assert!(
            contains(&retained, marker),
            "small trailing marker retained"
        );
    }

    // Each payload exceeds both the retention cap and ordinary pipe capacity.
    // Assertions occur after run() has retained and reaped the real process.
    let large = run_child(&exact, "large", Duration::from_secs(30)).await;
    assert_summary_and_bounds(&large, LARGE);
    assert!(
        large.stdout.is_truncated(),
        "large stdout retention truncated"
    );
    assert!(
        large.stderr.is_truncated(),
        "large stderr retention truncated"
    );
    assert!(
        contains(large.stdout.tail(), STDOUT_TAIL),
        "stdout trailing marker retained"
    );
    assert!(
        contains(large.stderr.tail(), STDERR_TAIL),
        "stderr trailing marker retained"
    );

    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
        .env(CHILD, &exact)
        .env(MODE, "held");
    let failure = match crate::test_child_process::run(command, Duration::from_secs(2)).await {
        Err(failure) => failure,
        Ok(_) => panic!("held child must time out instead of returning successfully"),
    };
    // run() has killed/reaped the actual parked process and drained both pipes
    // before returning this failure. Inspect only metadata and marker booleans.
    assert_eq!(failure.kind, "timeout");
    assert!(
        !failure
            .status
            .as_ref()
            .expect("held child was reaped")
            .success()
    );
    assert!(
        failure.cleanup_error.is_none(),
        "held child cleanup completed"
    );
    assert!(
        failure.stdout.complete_eof(),
        "stdout drained to real EOF after cleanup"
    );
    assert!(
        failure.stderr.complete_eof(),
        "stderr drained to real EOF after cleanup"
    );
    assert!(
        failure.stdout.summary().is_none(),
        "parked child never returned to libtest"
    );
    assert!(failure.stdout.total_bytes >= HELD_STDOUT.len() as u64);
    assert_eq!(failure.stderr.total_bytes, HELD_STDERR.len() as u64);
    for (output, marker) in [
        (&failure.stdout, HELD_STDOUT),
        (&failure.stderr, HELD_STDERR),
    ] {
        assert!(output.retained_len() <= RETAINED);
        assert!(
            !output.is_truncated(),
            "short held marker output is complete"
        );
        let retained = [output.prefix(), output.tail()].concat();
        assert!(
            contains(&retained, marker),
            "actual held child partial output retained"
        );
    }

    // Keep expected panic-hook suppression inside a separate exact child;
    // other API parent tests can continue running concurrently without changes.
    let gates = run_child(&exact, "gate-controls", Duration::from_secs(30)).await;
    gates.assert_test_success(&exact);
}
