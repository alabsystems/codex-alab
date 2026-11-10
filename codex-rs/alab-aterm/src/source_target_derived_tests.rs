// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Destination route/instance transport controls, not native grant proofs.

use super::*;
use codex_alab_core::native_source_use::UseAttempt;
use codex_alab_core::native_source_use::UseOutcome;

#[tokio::test]
async fn derived_target_retains_its_credential_route_after_parent_use_ends() -> io::Result<()> {
    let fixture = Fixture::start(|_, _| {}).await?;
    let target = target::context_at(&fixture.endpoint, &"e".repeat(32), BIRTH).await?;
    let parent = target.open(&"a".repeat(32)).await?;
    preload_reader(&parent).await?;
    let attempt = UseAttempt::default();
    let receipt = match parent.begin_use(&attempt, REGISTRATION).await? {
        UseOutcome::Active(receipt) => receipt,
        _ => panic!("fixture parent use was not admitted"),
    };
    attempt.finish_admission()?;
    let child = parent.open_derived(&"c".repeat(32), &receipt).await?;
    parent.end_use(&receipt).await?;
    let preload = child.test_preload(&"d".repeat(32), None).await?;
    child
        .test_ready(
            &preload.source.operation,
            Some(&preload.registration),
            &core::Loaded::ReaderSource {
                source: Box::new(preload.source.clone()),
            },
        )
        .await?;
    let requests = fixture.requests.lock().unwrap().clone();
    for action in ["member-preload-target", "member-reader-ready-target"] {
        let line = requests
            .iter()
            .find(|line| {
                line.contains(&format!(" {action} "))
                    && line.contains(&format!(" member={}", child.id()))
            })
            .unwrap();
        assert!(line.contains(&format!(
            " assignment={} context={}",
            "e".repeat(32),
            "c".repeat(64)
        )));
        assert!(!line.contains("parent_capability="));
        assert_eq!(
            line.contains(" assignment_generation=5"),
            action == "member-reader-ready-target"
        );
    }
    child.retire(Some(&preload.registration)).await?;
    parent.retire(Some(REGISTRATION)).await?;
    drop(target);
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn derived_target_refuses_a_replacement_native_instance() -> io::Result<()> {
    let fixture = Fixture::start(|action, frame| {
        // Derived OPEN's response is member-open; the explicit target OPEN and
        // target context are separate actions and retain the original epoch.
        if action == "member-open" {
            frame["session"]["epoch"] = json!("replacement-native-instance");
        }
    })
    .await?;
    let target = target::context_at(&fixture.endpoint, &"e".repeat(32), BIRTH).await?;
    let parent = target.open(&"a".repeat(32)).await?;
    preload_reader(&parent).await?;
    let attempt = UseAttempt::default();
    let receipt = match parent.begin_use(&attempt, REGISTRATION).await? {
        UseOutcome::Active(receipt) => receipt,
        _ => panic!("fixture parent use was not admitted"),
    };
    attempt.finish_admission()?;
    assert!(
        parent
            .open_derived(&"c".repeat(32), &receipt)
            .await
            .is_err()
    );
    parent.end_use(&receipt).await?;
    parent.retire(Some(REGISTRATION)).await?;
    drop(target);
    fixture.finish().await;
    Ok(())
}
