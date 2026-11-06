// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A description belongs to the exact root runtime that read its goal and step.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::*;

pub(super) async fn replace(h: &Harness) -> (ExtensionData, Arc<ThreadRt>) {
    let replacement = ExtensionData::new(h.rt.key());
    h.autopilot
        .on_thread_start(ThreadStartInput {
            config: &h.test.config,
            session_source: &codex_protocol::protocol::SessionSource::Cli,
            persistent_thread_state_available: true,
            environments: &[],
            mcp_resource_client: None,
            extension_metrics: None,
            session_store: &ExtensionData::new("session"),
            thread_store: &replacement,
        })
        .await;
    let fresh = replacement.get::<ThreadRt>().expect("replacement runtime");
    (replacement, fresh)
}

pub(super) async fn shutdown(h: &Harness, replacement: &ExtensionData) {
    h.autopilot
        .on_thread_stop(ThreadStopInput {
            session_store: &ExtensionData::new("session"),
            thread_store: replacement,
        })
        .await;
    h.test
        .codex
        .shutdown_and_wait()
        .await
        .expect("root shutdown");
}

#[test]
fn a_retired_runtime_cannot_replace_the_current_roots_description() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.immediate_point().await;
            h.rt.set_plan_step(Some("the old runtime's step"));
            h.autopilot.refresh_description(&h.rt).await;
            assert_eq!(
                h.fake.description().as_deref(),
                Some("plan: the old runtime's step")
            );

            let (replacement, fresh) = replace(&h).await;
            fresh.set_plan_step(Some("the current runtime's step"));
            h.autopilot.refresh_description(&fresh).await;
            assert_eq!(
                h.fake.description().as_deref(),
                Some("plan: the current runtime's step"),
                "a healthy replacement still publishes its own step"
            );

            h.autopilot.refresh_description(&h.rt).await;
            let shown = h.fake.description();
            shutdown(&h, &replacement).await;
            assert_eq!(
                shown.as_deref(),
                Some("plan: the current runtime's step"),
                "a late callback from the retired runtime has no publication authority"
            );
        })
        .await
        .expect("description runtime custody hang detector");
    });
}

#[test]
fn a_description_read_losing_its_runtime_preserves_the_replacements_description() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.immediate_point().await;
            h.rt.set_plan_step(Some("the old runtime's delayed step"));
            let (reached, read) = tokio::sync::oneshot::channel();
            let (resume, wait) = tokio::sync::oneshot::channel();
            *lock(&h.rt.after_description_read) = Some((reached, wait));
            let pilot = Arc::clone(&h.autopilot);
            let old = Arc::clone(&h.rt);
            let late = tokio::spawn(async move { pilot.refresh_description(&old).await });
            read.await.expect("old description reached its publication boundary");

            let (replacement, fresh) = replace(&h).await;
            fresh.set_plan_step(Some("the replacement's current step"));
            h.autopilot.refresh_description(&fresh).await;
            assert_eq!(
                h.fake.description().as_deref(),
                Some("plan: the replacement's current step")
            );
            resume.send(()).expect("release the modeled late goal read");
            late.await.expect("late description callback");
            let shown = h.fake.description();
            shutdown(&h, &replacement).await;
            assert_eq!(
                shown.as_deref(),
                Some("plan: the replacement's current step"),
                "the goal-read boundary models scheduling after the actual async read, before publication"
            );
        })
        .await
        .expect("late description read hang detector");
    });
}

#[test]
fn an_in_flight_old_write_reconciles_the_replacements_latest_description() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new_with_control_timeout(None, None, Duration::from_secs(60)).await;
            h.immediate_point().await;
            let gate = "meta set description plan: the old runtime's in-flight step";
            h.fake.gate(gate);
            h.rt.set_plan_step(Some("the old runtime's in-flight step"));
            let pilot = Arc::clone(&h.autopilot);
            let old = Arc::clone(&h.rt);
            let late = tokio::spawn(async move { pilot.refresh_description(&old).await });
            assert!(
                h.fake
                    .wait_for(|fake| fake.gated_requests().iter().any(|r| r.ends_with(gate)))
                    .await,
                "old write reached the owned control server"
            );

            let (replacement, fresh) = replace(&h).await;
            fresh.set_plan_step(Some("the replacement's latest step"));
            let pilot = Arc::clone(&h.autopilot);
            let current = Arc::clone(&fresh);
            let publish = tokio::spawn(async move { pilot.refresh_description(&current).await });
            upgrade_handoff_tests::wait_for_supervisor(|| {
                h.autopilot
                    .sup
                    .wants_description("plan: the replacement's latest step")
            })
            .await;
            h.fake.open_gate(gate);
            late.await.expect("old remote write");
            publish.await.expect("replacement remote write");
            let shown = h.fake.description();

            fresh.set_plan_step(Some("a later healthy current step"));
            h.autopilot.refresh_description(&fresh).await;
            let healthy = h.fake.description();
            shutdown(&h, &replacement).await;
            assert_eq!(
                shown.as_deref(),
                Some("plan: the replacement's latest step")
            );
            assert_eq!(
                healthy.as_deref(),
                Some("plan: a later healthy current step")
            );
        })
        .await
        .expect("in-flight description write hang detector");
    });
}

#[test]
fn a_passive_withdrawal_cannot_erase_identical_text_published_after_resume() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new_with_control_timeout(None, None, Duration::from_secs(60)).await;
            h.immediate_point().await;
            let text = "plan: the current step remains the same";
            h.rt.set_plan_step(Some("the current step remains the same"));
            h.autopilot.refresh_description(&h.rt).await;
            assert_eq!(h.fake.description().as_deref(), Some(text));
            let gate = "meta unset description";
            h.fake.gate(gate);
            h.fake.set_effective(false, "suspended");
            assert!(
                h.fake
                    .wait_for(|fake| fake.gated_requests().iter().any(|r| r.ends_with(gate)))
                    .await,
                "the passive withdrawal reached the owned control server"
            );
            h.fake.set_effective(true, "enabled");
            upgrade_handoff_tests::wait_for_supervisor(|| h.autopilot.sup.acting()).await;
            let pilot = Arc::clone(&h.autopilot);
            let rt = Arc::clone(&h.rt);
            let publish = tokio::spawn(async move { pilot.refresh_description(&rt).await });
            upgrade_handoff_tests::wait_for_supervisor(|| h.autopilot.sup.wants_description(text))
                .await;
            h.fake.open_gate(gate);
            publish.await.expect("the resumed description publisher");
            let shown = h.fake.description();
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("root shutdown");
            assert_eq!(shown.as_deref(), Some(text));
        })
        .await
        .expect("passive description cleanup hang detector");
    });
}

#[test]
fn closing_waits_for_an_in_flight_description_and_withdraws_it() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new_with_control_timeout(None, None, Duration::from_secs(60)).await;
            h.immediate_point().await;
            let gate = "meta set description plan: a step already being written";
            h.fake.gate(gate);
            h.rt.set_plan_step(Some("a step already being written"));
            let pilot = Arc::clone(&h.autopilot);
            let rt = Arc::clone(&h.rt);
            let write = tokio::spawn(async move { pilot.refresh_description(&rt).await });
            assert!(
                h.fake
                    .wait_for(|fake| fake.gated_requests().iter().any(|r| r.ends_with(gate)))
                    .await,
                "the in-flight write reached the owned control server"
            );
            let shared = h.shared;
            let release =
                tokio::task::spawn_blocking(move || shared.release_claim(Duration::from_secs(60)));
            upgrade_handoff_tests::wait_for_supervisor(|| !h.autopilot.sup.acting()).await;
            h.fake.open_gate(gate);
            write.await.expect("the in-flight description writer");
            let released = release.await.expect("orderly claim release");
            let shown = h.fake.description();
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("root shutdown");
            assert!(released, "the ordinary bounded release completed");
            assert_eq!(shown, None, "no late description survives process closing");
        })
        .await
        .expect("closing description cleanup hang detector");
    });
}
