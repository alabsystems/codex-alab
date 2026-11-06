// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Sleep at the actual error/finish/first-decision boundaries, with an injected host clock.

use super::*;
use crate::decide::ThreadState;
use crate::netwatch::Route;
use codex_alab_core::capacity::SamplingRetryAdvice;
use codex_alab_core::wake::Reading;
use codex_alab_core::wake::Wake;
use codex_extension_api::TurnErrorInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStopInput;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::error::UsageLimitReachedError;
use codex_protocol::protocol::CodexErrorInfo;
use std::sync::Mutex;

const HANG: Duration = Duration::from_secs(60);
const BASE: Duration = Duration::from_secs(5000);
const HOUR: Duration = Duration::from_secs(3600);

fn scripted_clock(h: &Harness, asleep: Duration) -> Arc<Mutex<Reading>> {
    let reading = Arc::new(Mutex::new(Reading { asleep, last: None }));
    let read = Arc::clone(&reading);
    h.autopilot.set_wake_read(Arc::new(move || {
        Some(
            *read
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }));
    reading
}

fn wake(reading: &Mutex<Reading>, asleep: Duration) {
    *reading
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Reading {
        asleep,
        last: Some(Wake {
            at: Instant::now(),
            slept: 2 * HOUR,
            seq: 1,
        }),
    };
}

fn usage_details() -> CodexErrorDetails {
    CodexErrorDetails::UsageLimitReached(UsageLimitReachedError {
        plan_type: None,
        resets_at: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
        limit_window_minutes: None,
        rate_limits: None,
        promo_message: None,
        rate_limit_reached_type: None,
    })
}

fn point_deadline(h: &Harness) -> Instant {
    match h
        .rt
        .state()
        .point
        .expect("the real callback finished its point")
        .wall
    {
        Some(Wall::Usage {
            reset_at: Some(at), ..
        })
        | Some(Wall::Server {
            retry_at: Some(at), ..
        }) => at,
        wall => panic!("the fixture must retain its provider deadline: {wall:?}"),
    }
}

async fn stop(h: &Harness, session: &ExtensionData, turn: &ExtensionData) {
    h.autopilot
        .on_turn_stop(TurnStopInput {
            session_store: session,
            thread_store: &h._store,
            turn_store: turn,
        })
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subscription_errors_retain_the_route_for_their_recovery() {
    tokio::time::timeout(HANG, async {
        for details in [
            CodexErrorDetails::QuotaExceeded,
            CodexErrorDetails::UsageNotIncluded,
        ] {
            let h = started_with_claim_watch(&[("resume_limits", "false")], |_, _| {}, false).await;
            wait_until("the isolated claim acts", || h.sup.acting()).await;
            let session = ExtensionData::new("session");
            let turn = ExtensionData::new("subscription-error");
            start_native_turn(&h, &session, &turn).await;
            let (entered, reached) = tokio::sync::oneshot::channel();
            let (release, resume) = tokio::sync::oneshot::channel();
            h.autopilot.hold_wall_route(entered, resume);
            let callback = h.autopilot.on_turn_error(TurnErrorInput {
                turn_id: turn.level_id(),
                error: CodexErrorInfo::UsageLimitExceeded,
                error_details: &details,
                session_store: &session,
                thread_store: &h._store,
                turn_store: &turn,
            });
            tokio::pin!(callback);
            tokio::select! {
                reached = reached => reached.expect("subscription recovery reads its route"),
                () = &mut callback => panic!("subscription recovery skipped its network route"),
            }
            release.send(()).expect("release the route read");
            callback.await;
            stop(&h, &session, &turn).await;
            assert!(matches!(
                h.rt.state()
                    .point
                    .as_ref()
                    .and_then(|point| point.wall.as_ref()),
                Some(Wall::Entitlement)
            ));
            assert!(
                matches!(h.rt.route_for_test(), Some(Route::Custom(_))),
                "finishing the subscription error must retain the route it read"
            );
        }
    })
    .await
    .expect("subscription route hang detector");
}

async fn finish_then_sleep(server: bool) {
    tokio::time::timeout(HANG, async {
        for sleeps in [false, true] {
            // No claim watcher can evaluate the point before the scripted first look.
            let h = started_with_claim_watch(&[("resume_limits", "false")], |_, _| {}, false).await;
            wait_until("the isolated claim acts", || h.sup.acting()).await;
            let reading = scripted_clock(&h, BASE);
            h.rt.with_state(ThreadState::observe_person_turn);
            let session = ExtensionData::new("session");
            let turn = ExtensionData::new("fresh-provider-point");
            start_native_turn(&h, &session, &turn).await;
            let details = if server {
                turn.insert(SamplingRetryAdvice::with_reading(
                    Some(Instant::now() + HOUR),
                    Some(*reading.lock().unwrap_or_else(std::sync::PoisonError::into_inner)),
                ));
                CodexErrorDetails::InternalServerError
            } else {
                usage_details()
            };
            h.autopilot.on_turn_error(TurnErrorInput {
                turn_id: turn.level_id(),
                error: if server { CodexErrorInfo::InternalServerError } else { CodexErrorInfo::UsageLimitExceeded },
                error_details: &details,
                session_store: &session,
                thread_store: &h._store,
                turn_store: &turn,
            }).await;
            stop(&h, &session, &turn).await;
            let original = point_deadline(&h);
            assert!(original > Instant::now(), "the received provider deadline is still future");
            if sleeps {
                wake(&reading, BASE + 2 * HOUR);
            }
            Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
            let after = point_deadline(&h);
            if sleeps {
                assert!(after <= Instant::now(), "sleep between finish and first evaluation must count: original={original:?}, after={after:?}");
            } else {
                assert_eq!(after, original, "without sleep the complete provider wait stands");
            }
            Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
            assert_eq!(point_deadline(&h), after, "a second evaluation cannot credit the same sleep again");
            Autopilot::cancel_timer(&h.rt).await;
        }
    }).await.expect("finish/first-decision hang detector");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_usage_sleep_after_finish_counts_before_the_points_first_evaluation() {
    finish_then_sleep(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_sleep_after_finish_counts_before_the_points_first_evaluation() {
    finish_then_sleep(true).await;
}

/// Advice retained by the retry handler may sleep before the terminal callback
/// sees it. Classification must consume its retained receipt baseline exactly once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retained_server_advice_counts_sleep_before_classification() {
    tokio::time::timeout(HANG, async {
        for case in ["no-sleep", "sleep-before-fresh-advice", "sleep-before-classification"] {
            let h = started_with_claim_watch(&[("retry_api_errors", "false")], |_, _| {}, false).await;
            wait_until("the isolated claim acts", || h.sup.acting()).await;
            let initial = if case == "sleep-before-fresh-advice" { BASE + 2 * HOUR } else { BASE };
            let reading = scripted_clock(&h, initial);
            h.rt.with_state(ThreadState::observe_person_turn);
            let session = ExtensionData::new("session");
            let turn = ExtensionData::new("retained-provider-advice");
            start_native_turn(&h, &session, &turn).await;
            let original = Instant::now() + HOUR;
            turn.insert(SamplingRetryAdvice::with_reading(
                Some(original),
                Some(*reading.lock().unwrap_or_else(std::sync::PoisonError::into_inner)),
            ));
            if case == "sleep-before-classification" {
                wake(&reading, BASE + 2 * HOUR);
            }
            h.autopilot.on_turn_error(TurnErrorInput {
                turn_id: turn.level_id(),
                error: CodexErrorInfo::InternalServerError,
                error_details: &CodexErrorDetails::InternalServerError,
                session_store: &session,
                thread_store: &h._store,
                turn_store: &turn,
            }).await;
            stop(&h, &session, &turn).await;
            let after = point_deadline(&h);
            if case == "sleep-before-classification" {
                assert!(after <= Instant::now(), "the callback must see the stored one-hour advice already due after two hours asleep: {after:?}");
            } else {
                assert_eq!(after, original, "{case}: only sleep after advice was retained can shorten it");
            }
            for _ in 0..2 {
                Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
                assert_eq!(point_deadline(&h), after, "{case}: the transferred sleep must not be credited again");
            }
            Autopilot::cancel_timer(&h.rt).await;
        }
    }).await.expect("retained advice/classification hang detector");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_usage_reset_counts_sleep_while_its_route_is_read() {
    tokio::time::timeout(HANG, async {
        for sleeps_during_route in [false, true] {
            let h = started_with_claim_watch(&[("resume_limits", "false")], |_, _| {}, false).await;
            wait_until("the isolated claim acts", || h.sup.acting()).await;
            // Earlier sleep must not shorten a newly received reset.
            let before = BASE + 2 * HOUR;
            let reading = scripted_clock(&h, before);
            h.rt.with_state(ThreadState::observe_person_turn);
            let session = ExtensionData::new("session");
            let turn = ExtensionData::new("classified-usage-point");
            start_native_turn(&h, &session, &turn).await;
            let details = usage_details();
            let (entered, reached) = tokio::sync::oneshot::channel();
            let (release, resume) = tokio::sync::oneshot::channel();
            h.autopilot.hold_wall_route(entered, resume);
            let callback = h.autopilot.on_turn_error(TurnErrorInput {
                turn_id: turn.level_id(),
                error: CodexErrorInfo::UsageLimitExceeded,
                error_details: &details,
                session_store: &session,
                thread_store: &h._store,
                turn_store: &turn,
            });
            tokio::pin!(callback);
            tokio::select! {
                reached = reached => reached.expect("classification reached its actual route read"),
                () = &mut callback => panic!("the error callback must await the held route read"),
            }
            if sleeps_during_route {
                wake(&reading, before + 2 * HOUR);
            }
            release.send(()).expect("release the actual route read");
            callback.await;
            stop(&h, &session, &turn).await;
            let after = point_deadline(&h);
            if sleeps_during_route {
                assert!(after <= Instant::now(), "sleep after classification and before finish counts toward the reset: {after:?}");
            } else {
                assert!(after > Instant::now() + HOUR / 2, "sleep before classification must leave the fresh reset intact: {after:?}");
            }
            Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
            assert_eq!(point_deadline(&h), after, "the transferred sleep must not be counted again at first evaluation");
            Autopilot::cancel_timer(&h.rt).await;
        }
    }).await.expect("classification/route/finish hang detector");
}
