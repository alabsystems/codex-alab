// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The native refusal cases observe a process-global update cache. Each case
//! gets an exact-test child so no other evaluator can supply its pending bit.

use std::time::Duration;

const CHILD: &str = "ALAB_PENDING_UPDATE_INDICATOR_CASE";
const SUCCESS: &str = "ALAB_PENDING_UPDATE_INDICATOR_OK";
const TEST: &str = "extension::upgrade_handoff_tests::an_immediate_continuation_survives_refusal_but_never_supersedes_new_input";
const CASES: [&str; 4] = ["refused", "draft", "native", "write-failed"];

/// Return the one child case, or run all isolated children from the parent.
pub(super) async fn isolated_case() -> Option<&'static str> {
    if let Some(case) = std::env::var_os(CHILD) {
        return Some(
            CASES
                .into_iter()
                .find(|expected| case.to_str() == Some(*expected))
                .expect("known pending indicator child case"),
        );
    }
    for case in CASES {
        let output = tokio::time::timeout(
            Duration::from_secs(60),
            tokio::process::Command::new(std::env::current_exe().expect("current test executable"))
                .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
                .env(CHILD, case)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .expect("isolated pending indicator child timeout")
        .expect("isolated pending indicator child output");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "{case}: isolated pending indicator child failed\n{stdout}\n{stderr}"
        );
        assert!(
            stdout.contains(&format!("{SUCCESS} {case}")),
            "{case}: isolated child did not finish its native assertions\n{stdout}\n{stderr}"
        );
    }
    None
}

pub(super) fn completed(case: &str) {
    println!("\n{SUCCESS} {case}");
}
