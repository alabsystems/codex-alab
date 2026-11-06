// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Accepted descriptions retain their exact runtime and refresh request custody.

use super::description_custody_tests::replace;
use super::description_custody_tests::shutdown;
use super::description_goal_read_tests::saved_status;
use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::*;
use codex_state::ThreadGoalStatus;

#[test]
fn retiring_an_accepted_description_owner_cancels_its_in_flight_write() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let mut results = Vec::new();
            for replaced in [false, true] {
                let h =
                    Harness::new_with_control_timeout(None, None, Duration::from_secs(60)).await;
                h.immediate_point().await;
                let gate = "meta set description plan: the retiring runtime's accepted step";
                h.fake.gate(gate);
                h.rt.set_plan_step(Some("the retiring runtime's accepted step"));
                let pilot = Arc::clone(&h.autopilot);
                let old = Arc::clone(&h.rt);
                let write = tokio::spawn(async move { pilot.refresh_description(&old).await });
                assert!(
                    h.fake
                        .wait_for(|fake| fake.gated_requests().iter().any(|r| r.ends_with(gate)))
                        .await
                );

                let replacement = if replaced {
                    Some(replace(&h).await)
                } else {
                    h.test
                        .codex
                        .shutdown_and_wait()
                        .await
                        .expect("actual native runtime stop");
                    None
                };
                assert!(
                    h.autopilot.sup.acting(),
                    "retirement leaves the process claim acting"
                );
                h.fake.open_gate(gate);
                write.await.expect("the retired runtime's in-flight write");
                let retired = h.fake.description();
                let healthy = if let Some((store, fresh)) = &replacement {
                    fresh.set_plan_step(Some("a healthy replacement step"));
                    h.autopilot.refresh_description(fresh).await;
                    let shown = h.fake.description();
                    shutdown(&h, store).await;
                    shown
                } else {
                    None
                };
                results.push((replaced, retired, healthy));
            }
            for (replaced, retired, healthy) in results {
                assert_eq!(
                    retired, None,
                    "accepted owner retired (replacement={replaced})"
                );
                if replaced {
                    assert_eq!(healthy.as_deref(), Some("plan: a healthy replacement step"));
                }
            }
        })
        .await
        .expect("accepted description owner retirement hang detector");
    });
}

#[test]
fn a_newer_refresh_supersedes_an_older_goal_read_even_if_the_new_read_fails() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let mut results = Vec::new();
            for change in ["objective", "complete", "unreadable"] {
                let h = Harness::new().await;
                h.immediate_point().await;
                let db = h.test.codex.state_db().expect("native goal database");
                db.thread_goals()
                    .replace_thread_goal(
                        h.rt.id,
                        "the last acknowledged task",
                        ThreadGoalStatus::Active,
                        None,
                    )
                    .await
                    .expect("persist acknowledged goal");
                h.autopilot.refresh_description(&h.rt).await;
                let known = h.fake.description();
                db.thread_goals()
                    .replace_thread_goal(
                        h.rt.id,
                        "an older pending read",
                        ThreadGoalStatus::Active,
                        None,
                    )
                    .await
                    .expect("persist older pending objective");
                let (reached, read) = tokio::sync::oneshot::channel();
                let (resume, wait) = tokio::sync::oneshot::channel();
                *lock(&h.rt.after_description_read) = Some((reached, wait));
                let pilot = Arc::clone(&h.autopilot);
                let rt = Arc::clone(&h.rt);
                let old = tokio::spawn(async move { pilot.refresh_description(&rt).await });
                read.await
                    .expect("older actual goal read reached publication boundary");

                let expected = if change == "unreadable" {
                    saved_status(&db, h.rt.id, "unreadable-new-description-request").await;
                    assert!(db.thread_goals().get_thread_goal(h.rt.id).await.is_err());
                    known
                } else {
                    db.thread_goals()
                        .replace_thread_goal(
                            h.rt.id,
                            "the current task",
                            if change == "complete" {
                                ThreadGoalStatus::Complete
                            } else {
                                ThreadGoalStatus::Active
                            },
                            None,
                        )
                        .await
                        .expect("persist newer goal facts");
                    crate::describe::describe(
                        (change != "complete").then_some("the current task"),
                        None,
                    )
                };
                h.autopilot.refresh_description(&h.rt).await;
                let current = h.fake.description();
                if change == "unreadable" {
                    saved_status(&db, h.rt.id, "active").await;
                }
                resume.send(()).expect("release older goal read");
                old.await.expect("older refresh returned");
                let final_text = h.fake.description();
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("native request custody shutdown");
                results.push((change, expected, current, final_text));
            }
            for (change, expected, current, final_text) in results {
                assert_eq!(
                    current, expected,
                    "healthy/new unknown request control: {change}"
                );
                assert_eq!(
                    final_text, expected,
                    "an older read cannot supersede the newer request: {change}"
                );
            }
        })
        .await
        .expect("description refresh request custody hang detector");
    });
}

#[test]
fn identical_text_keeps_the_new_owners_custody_through_old_cleanup_and_stop() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new_with_control_timeout(None, None, Duration::from_secs(60)).await;
            h.immediate_point().await;
            let text = "plan: identical work in the replacement";
            h.rt.set_plan_step(Some("identical work in the replacement"));
            h.autopilot.refresh_description(&h.rt).await;
            assert_eq!(h.fake.description().as_deref(), Some(text));
            let gate = "meta unset description";
            h.fake.gate(gate);
            let (store, fresh) = replace(&h).await;
            assert!(
                h.fake
                    .wait_for(|fake| fake.gated_requests().iter().any(|r| r.ends_with(gate)))
                    .await
            );
            fresh.set_plan_step(Some("identical work in the replacement"));
            let pilot = Arc::clone(&h.autopilot);
            let current = Arc::clone(&fresh);
            let publish = tokio::spawn(async move { pilot.refresh_description(&current).await });
            upgrade_handoff_tests::wait_for_supervisor(|| h.autopilot.sup.wants_description(text))
                .await;
            h.fake.open_gate(gate);
            publish
                .await
                .expect("the new owner's identical description");
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("the old native instance stops after replacement");
            let shown = h.fake.description();
            h.autopilot
                .on_thread_stop(ThreadStopInput {
                    session_store: &ExtensionData::new("session"),
                    thread_store: &store,
                })
                .await;
            assert_eq!(
                shown.as_deref(),
                Some(text),
                "old cleanup and late stop preserve the current owner"
            );
        })
        .await
        .expect("identical description owner custody hang detector");
    });
}

#[test]
fn delayed_cleanup_keeps_a_fresh_acknowledgement_and_reconciles_an_acknowledged_unset() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let mut results = Vec::new();
            for replacement in [false, true] {
                for gate in ["meta", "meta unset description"] {
                    let h = Harness::new_with_control_timeout(None, None, Duration::from_secs(60))
                        .await;
                    h.immediate_point().await;
                    let text = "plan: an acknowledged task";
                    h.rt.set_plan_step(Some("an acknowledged task"));
                    h.autopilot.refresh_description(&h.rt).await;
                    assert_eq!(h.fake.description().as_deref(), Some(text));
                    h.fake.gate(gate);
                    let (store, current) = if replacement {
                        let (store, rt) = replace(&h).await;
                        (Some(store), rt)
                    } else {
                        h.fake.set_effective(false, "suspended");
                        (None, Arc::clone(&h.rt))
                    };
                    assert!(
                        h.fake
                            .wait_for(|fake| fake
                                .gated_requests()
                                .iter()
                                .any(|r| r.ends_with(gate)))
                            .await
                    );
                    if !replacement {
                        h.fake.set_effective(true, "enabled");
                        upgrade_handoff_tests::wait_for_supervisor(|| h.autopilot.sup.acting())
                            .await;
                    }
                    current.set_plan_step(Some("an acknowledged task"));
                    let pilot = Arc::clone(&h.autopilot);
                    let rt = Arc::clone(&current);
                    let adopt = tokio::spawn(async move { pilot.refresh_description(&rt).await });
                    upgrade_handoff_tests::wait_for_supervisor(|| {
                        h.autopilot.sup.wants_description(text)
                    })
                    .await;
                    adopt.abort();
                    assert!(
                        adopt
                            .await
                            .expect_err("adopting publisher was queued")
                            .is_cancelled()
                    );

                    current.set_plan_step(Some("a later desired task"));
                    let pilot = Arc::clone(&h.autopilot);
                    let rt = Arc::clone(&current);
                    let later = tokio::spawn(async move { pilot.refresh_description(&rt).await });
                    upgrade_handoff_tests::wait_for_supervisor(|| {
                        h.autopilot
                            .sup
                            .wants_description("plan: a later desired task")
                    })
                    .await;
                    later.abort();
                    assert!(
                        later
                            .await
                            .expect_err("later publisher was queued")
                            .is_cancelled()
                    );
                    h.fake.open_gate(gate);
                    h.autopilot.sup.descriptions_settled().await;
                    let shown = h.fake.description();
                    if let Some(store) = store {
                        shutdown(&h, &store).await;
                    } else {
                        h.test
                            .codex
                            .shutdown_and_wait()
                            .await
                            .expect("same-runtime cleanup fixture shutdown");
                    }
                    results.push((replacement, gate, shown));
                }
            }
            for (replacement, gate, shown) in results {
                let expected = if gate == "meta" {
                    "plan: an acknowledged task"
                } else {
                    "plan: a later desired task"
                };
                assert_eq!(
                    shown.as_deref(),
                    Some(expected),
                    "replacement={replacement}, cleanup gate={gate}"
                );
            }
        })
        .await
        .expect("queued description cleanup identity hang detector");
    });
}
