// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Opt-in failing children for the protected diagnostic cleanup probe.
//! The outer probe requires actual failed native outcomes and surviving exports.

use super::*;

async fn fail_with_diagnostics(phase: &str) {
    assert_eq!(
        std::env::var("ALAB_NATIVE_DIAGNOSTICS_PROBE").as_deref(),
        Ok(phase),
        "only the explicit diagnostic probe may run this ignored failure fixture"
    );
    let mut h = Harness::new().await;
    h.fake.set_policy_key("upgrade", "false");
    h.fake.set_policy_key("autopilot", "off");
    h.fake.set_status("autopilot", "off");
    let _ = h.autopilot.sup.ready().await;
    Autopilot::cancel_timer(&h.rt).await;
    h.retain_artifacts();
    let scratch = std::env::var("ALAB_TEST_SCRATCH").expect("the real admitted scratch");
    std::fs::write(
        h.test.home.path().join("diagnostic-witness.json"),
        serde_json::to_vec(&serde_json::json!({
            "phase": phase,
            "scratch": scratch,
        }))
        .expect("encode native fixture witness"),
    )
    .expect("write the witness inside the registered native home");
    let outside = tempfile::tempdir().expect("owned outside-root control");
    std::fs::write(
        outside.path().join("must-not-copy"),
        b"outside registered roots",
    )
    .expect("write outside-root control");
    std::os::unix::fs::symlink(outside.path(), h.test.home.path().join("outside-link"))
        .expect("make the actual outside-root symlink");
    if phase == "shutdown" {
        h.shutdown_and_export()
            .await
            .expect("observe the native shutdown API return");
    }
    panic!("ALAB_NATIVE_DIAGNOSTICS_EXPECTED_FAILURE:{phase}");
}

#[test]
#[ignore = "the protected outer probe requires this intentional native failure"]
fn shutdown_returned_failure_fixture() {
    native_test(|| fail_with_diagnostics("shutdown"));
}

#[test]
#[ignore = "the protected outer probe requires this intentional native failure"]
fn incomplete_failure_fixture() {
    native_test(|| fail_with_diagnostics("incomplete"));
}
