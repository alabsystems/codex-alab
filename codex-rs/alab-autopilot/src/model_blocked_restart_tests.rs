// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Carried native model-stop provenance survives startup work and an unread rollout.

use super::*;
use codex_alab_core::session::Pace;
use codex_alab_core::session::WallGoal;
use codex_history::RolloutItem;

/// Replace only the process-local autopilot runtime. The real native thread,
/// goal service, database and rollout remain available to the image after.
/// Its pace passes through the actual record codec before any resumed work.
pub(super) async fn next_image(h: &mut Harness) -> WallGoal {
    let carried = h
        .shared
        .pace(h.rt.key())
        .expect("the model stop was published");
    let encoded = serde_json::to_value(codex_alab_tui::record::Pace::from(carried))
        .expect("encode the prior image's pace");
    let decoded: codex_alab_tui::record::Pace =
        serde_json::from_value(encoded).expect("decode the image handoff");
    let carried = Pace::from(decoded);
    let binding = carried
        .model_blocked_goal
        .clone()
        .expect("record preserves native model-stop provenance");
    assert_eq!(binding.person_stops, 0);
    h.test
        .codex
        .flush_rollout()
        .await
        .expect("flush native provenance");
    Autopilot::cancel_timer(&h.rt).await;
    h.rt.stopped.store(true, Ordering::SeqCst);
    let next = Arc::new(ThreadRt::new(
        h.rt.id,
        h.rt.config.clone(),
        h.rt.token_budget,
        Duration::ZERO,
    ));
    h.test
        .codex
        .thread_extension_data()
        .insert_arc(Arc::clone(&next));
    h.autopilot.keep_thread(&next);
    h.rt = next;
    h.shared.set_pace(h.rt.key(), carried);
    assert!(h.shared.set_inherited(codex_alab_core::session::Inherited {
        exec_generation: 1,
        session_id: Some(h.rt.key.clone()),
        ..Default::default()
    }));
    h.shared.set_launch_witness(None);
    h.shared.set_swept();
    binding
}

async fn history(h: &Harness, binding: &WallGoal) -> Vec<RolloutItem> {
    let items = super::super::resumed_history(&h.test.codex)
        .await
        .expect("the native blocked turn is in the durable rollout");
    assert!(
        !crate::resume_history::person_goal_stop_since(&items, &binding.turn),
        "the model's own stop is no person's goal instruction"
    );
    items
}

#[test]
fn a_paginated_rollout_fallback_refuses_partial_history_until_repaired() {
    use codex_protocol::protocol::ThreadHistoryMode;
    use codex_thread_store::ThreadStoreError;
    use std::io::Write;

    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let server = responses::start_mock_server().await;
            let test = core_test_support::test_codex::test_codex()
                .with_history_mode(ThreadHistoryMode::Paginated)
                .build(&server)
                .await
                .expect("start native Core with paginated history");
            let thread = &test.codex;
            thread.ensure_rollout_materialized().await;
            thread
                .flush_rollout()
                .await
                .expect("flush the owned rollout");
            let path = thread.rollout_path().expect("the native rollout path");
            assert!(matches!(
                thread.load_history(false).await,
                Err(ThreadStoreError::Unsupported {
                    operation: "paginated_threads"
                })
            ));
            let healthy = super::super::resumed_history(thread)
                .await
                .expect("read the clean paginated fallback");
            assert!(!healthy.is_empty());
            let healthy = serde_json::to_value(healthy).expect("encode the healthy history");
            let file = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open the test's owned rollout");
            let original_len = file.metadata().unwrap().len();
            for invalid in [
                b"\n{\n".as_slice(),
                br#"
{"timestamp":"2026-11-18T00:00:00Z","type":"unknown_test_fixture_record","payload":{}}
"#,
            ] {
                (&file)
                    .write_all(invalid)
                    .expect("append an unreadable test record");
                let (_, _, errors) = codex_core::RolloutRecorder::load_rollout_items(&path)
                    .await
                    .expect("the underlying rollout loader reports partial success");
                assert_eq!(errors, 1);
                let error = super::super::resumed_history(thread)
                    .await
                    .expect_err("recovery cannot use only the readable records");
                assert!(error.contains("unreadable records (count 1)"), "{error}");
                file.set_len(original_len)
                    .expect("restore the original bytes");
                let repaired = super::super::resumed_history(thread)
                    .await
                    .expect("the clean rollout is readable again");
                assert_eq!(serde_json::to_value(repaired).unwrap(), healthy);
            }
            thread.shutdown_and_wait().await.expect("native shutdown");
        })
        .await
        .expect("paginated fallback hang detector");
    });
}

#[test]
fn an_unrelated_native_reply_before_the_resume_scan_preserves_model_stop_custody() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (mut h, goals) = harness().await;
            let before_rt = Arc::clone(&h.rt);
            let before = before_rt.hold_eval_when_free().await;
            let original = seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
            let binding = next_image(&mut h).await;
            let evaluation = h.rt.hold_eval_when_free().await;
            h.autopilot
                .on_thread_resume(ThreadResumeInput {
                    session_store: &ExtensionData::new("restarted-session"),
                    thread_store: h.test.codex.thread_extension_data(),
                })
                .await;
            responses::mount_sse_once(
                &h._server,
                responses::sse(vec![
                    responses::ev_response_created("before-resume-scan"),
                    responses::ev_assistant_message(
                        "before-resume-scan-message",
                        "The missing temporary build-slot stopped validation.",
                    ),
                    responses::ev_completed("before-resume-scan"),
                ]),
            )
            .await;
            h.test
                .codex
                .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "Which validation step stopped?".into(),
                    text_elements: Vec::new(),
                }]))
                .await
                .expect("the unrelated native question beats the suspended resume scan");
            completed(&h).await;
            h.test
                .codex
                .flush_rollout()
                .await
                .expect("flush the later reply");
            assert!(!h.rt.scanned.load(Ordering::SeqCst));
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), 1);
            assert_eq!(goal(&h).await.status, ThreadGoalStatus::Blocked);
            assert_eq!(
                h.shared
                    .pace(h.rt.key())
                    .expect("native finish retains carried goal custody")
                    .model_blocked_goal,
                Some(binding.clone()),
                "an accepted reply supersedes old turn pacing, not the original goal's provenance"
            );
            drop(before);
            drop(evaluation);
            wait_for_supervisor(|| h.rt.scanned.load(Ordering::SeqCst)).await;
            assert_eq!(
                h.autopilot
                    .current_model_goal(&h.rt)
                    .await
                    .expect("read native custody")
                    .map(|bound| bound.goal),
                Some(binding),
                "readable history still validates the original model stop after a newer reply"
            );
            due(&h).await;
            evaluate(&h).await;
            wait_requests(&h, 4).await;
            let resumed = goal(&h).await;
            assert_eq!(resumed.goal_id, original.goal_id);
            assert_eq!(resumed.objective, original.objective);
            assert_eq!(resumed.status, ThreadGoalStatus::Active);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), 2);
            let requests = responses::received_responses_requests(&h._server).await;
            assert_eq!(requests.len(), 4);
            assert!(requests[3].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
            finish(&h).await;
        })
        .await
        .expect("native early-reply restart custody hang detector");
    });
}

#[test]
fn an_unread_resume_history_retains_model_stop_provenance_until_it_can_be_verified() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (mut h, goals) = harness().await;
            let before_rt = Arc::clone(&h.rt);
            let before = before_rt.hold_eval_when_free().await;
            let original = seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
            let binding = next_image(&mut h).await;
            let readable = history(&h, &binding).await;
            let evaluation = h.rt.hold_eval_when_free().await;
            // This is exactly the failed resume read's seed and terminal state:
            // no native point was reconstructed from unread history.
            h.rt.resumed.store(true, Ordering::SeqCst);
            h.autopilot.seed_pace(&h.rt, None);
            h.rt.scanned.store(true, Ordering::SeqCst);
            assert!(h.rt.state().point.is_none());
            assert!(lock(&h.rt.model_blocked_goal).is_none());
            assert_eq!(goal(&h).await.status, ThreadGoalStatus::Blocked);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), 0);
            h.autopilot.sync_pace(&h.rt);
            assert_eq!(
                h.shared
                    .pace(h.rt.key())
                    .expect("unread history retains pending provenance")
                    .model_blocked_goal,
                Some(binding.clone()),
                "an unavailable read withholds authority without erasing the native source fact"
            );
            h.autopilot
                .set_history_error("temporary rollout read fault");
            drop(before);
            drop(evaluation);
            evaluate(&h).await;
            assert!(h.rt.state().point.is_none());
            assert!(lock(&h.rt.model_blocked_goal).is_none());
            assert_eq!(goal(&h).await.status, ThreadGoalStatus::Blocked);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), 0);
            assert!(
                h.rt.timer_pending(),
                "an unread lazy history read arms a retry"
            );
            assert_eq!(
                responses::received_responses_requests(&h._server)
                    .await
                    .len(),
                2
            );
            assert_eq!(
                h.shared
                    .pace(h.rt.key())
                    .expect("the failed lazy read retains provenance")
                    .model_blocked_goal,
                Some(binding),
            );
            h.autopilot.set_history(readable);
            evaluate(&h).await;
            assert!(
                h.rt.state().point.is_some(),
                "a fresh image retries the unread history even with no current point"
            );
            due(&h).await;
            evaluate(&h).await;
            wait_requests(&h, 3).await;
            let resumed = goal(&h).await;
            assert_eq!(resumed.goal_id, original.goal_id);
            assert_eq!(resumed.objective, original.objective);
            assert_eq!(resumed.status, ThreadGoalStatus::Active);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), 1);
            let requests = responses::received_responses_requests(&h._server).await;
            assert_eq!(requests.len(), 3);
            assert!(requests[2].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
            finish(&h).await;
        })
        .await
        .expect("unread resume model-stop custody hang detector");
    });
}

#[test]
fn pending_restart_provenance_cannot_recover_across_a_persons_stop_or_missing_origin() {
    native_test(|| async {
        for change in ["history stop", "missing origin", "stop while pending"] {
            tokio::time::timeout(HANG, async {
                let (mut h, goals) = harness().await;
                let before_rt = Arc::clone(&h.rt);
                let before = before_rt.hold_eval_when_free().await;
                seed(&h, &goals).await;
                model_stop(&h, "blocked").await;
                let binding = next_image(&mut h).await;
                let mut readable = history(&h, &binding).await;
                let evaluation = h.rt.hold_eval_when_free().await;
                h.rt.resumed.store(true, Ordering::SeqCst);
                h.autopilot.seed_pace(&h.rt, None);
                h.rt.scanned.store(true, Ordering::SeqCst);
                assert!(lock(&h.rt.model_blocked_goal).is_none());
                if change == "history stop" {
                    readable.push(RolloutItem::ResponseItem(
                        codex_core::context::ContextualUserFragment::into(
                            codex_core::context::UserGoalUpdate::Set {
                                objective: None,
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Blocked),
                            },
                        )
                        .into(),
                    ));
                } else if change == "missing origin" {
                    readable.retain(|item| {
                        !matches!(item, RolloutItem::EventMsg(EventMsg::TurnStarted(started))
                            if started.turn_id == binding.turn)
                    });
                } else {
                    // The native app-server's provenance hook also revokes a
                    // same-status person's stop while history is unavailable.
                    h.shared.person_goal_set(h.rt.key(), true);
                }
                h.autopilot.set_history(readable);
                assert_eq!(
                    h.autopilot
                        .current_model_goal(&h.rt)
                        .await
                        .expect("read the stop evidence"),
                    None,
                    "{change}: carried provenance grants no authority over a person's stop"
                );
                h.autopilot.sync_pace(&h.rt);
                assert!(
                    h.shared
                        .pace(h.rt.key())
                        .is_none_or(|pace| pace.model_blocked_goal.is_none()),
                    "{change}: rejected authority must leave the durable handoff"
                );
                drop(before);
                drop(evaluation);
                evaluate(&h).await;
                assert_eq!(goal(&h).await.status, ThreadGoalStatus::Blocked);
                assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), 0);
                assert_eq!(
                    responses::received_responses_requests(&h._server)
                        .await
                        .len(),
                    2
                );
                finish(&h).await;
            })
            .await
            .unwrap_or_else(|error| {
                panic!("{change}: pending provenance stop hang detector: {error}")
            });
        }
    });
}
