// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A native resume's detached scanner can outlive stop while reading its carried route.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;
use codex_alab_core::session::Inherited;
use codex_alab_core::session::Recovery;
use codex_alab_core::session::RecoveryWall;
use core_test_support::responses;
use std::io::Write;

const HANG: Duration = Duration::from_secs(60);

enum DuringRoute {
    Current,
    Replace,
    Off,
}

async fn check_resume_needs(boundary: DuringRoute, old_needs: bool) {
    let replace = matches!(boundary, DuringRoute::Replace);
    let off = matches!(boundary, DuringRoute::Off);
    let h = Harness::new().await;
    h.immediate_point().await;
    responses::mount_sse_once(
        &h._server,
        responses::sse(vec![
            responses::ev_response_created("bootstrap-response"),
            responses::ev_assistant_message(
                "bootstrap-message",
                "The native rollout is materialized.",
            ),
            responses::ev_completed("bootstrap-response"),
        ]),
    )
    .await;
    h.test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Materialize this fixture's native rollout.".into(),
            text_elements: Vec::new(),
        }]))
        .await
        .expect("owned native bootstrap turn");
    core_test_support::wait_for_event_with_timeout(
        &h.test.codex,
        |event| matches!(event, EventMsg::TurnComplete(_)),
        HANG,
    )
    .await;
    h.test
        .codex
        .shutdown_and_wait()
        .await
        .expect("bootstrap shutdown");
    let rollout = h.test.codex.rollout_path().expect("native rollout");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&rollout)
        .expect("owned, closed native rollout");
    // The real native resume reads this persisted history. A successful earlier
    // turn may leave needs standing across the failed autopilot turn that follows.
    let mut events = Vec::new();
    if old_needs {
        events.extend([
            serde_json::json!({"type":"task_started", "turn_id":"earlier",
                "model_context_window":258400, "collaboration_mode_kind":"default",
                "turn_attribution":{"turn_id":"earlier", "turn_trigger":"user"}}),
            serde_json::json!({"type":"task_complete", "turn_id":"earlier",
                "last_agent_message":"NEEDS YOU: old approval"}),
        ]);
    }
    events.extend([
        serde_json::json!({"type":"task_started", "turn_id":"walled",
            "model_context_window":258400, "collaboration_mode_kind":"default",
            "turn_attribution":{"turn_id":"walled", "turn_trigger":"alab_autopilot"}}),
        serde_json::json!({"type":"task_complete", "turn_id":"walled",
        "last_agent_message":null, "error":{"message":"the API could not be reached",
        "codex_error_info":codex_protocol::protocol::CodexErrorInfo::HttpConnectionFailed {
            http_status_code: None
        }}}),
    ]);
    for event in events {
        writeln!(
            file,
            "{}",
            serde_json::json!({"timestamp":"2026-10-22T00:00:00Z",
            "type":"event_msg", "payload":event})
        )
        .expect("persist native history event");
    }
    drop(file);
    assert!(h.shared.set_inherited(Inherited {
        session_id: Some(h.rt.key.clone()),
        recovery: Some(Recovery {
            thread: h.rt.key.clone(),
            turn_id: "walled".into(),
            point_at_ms: now_ms() - 60_000,
            not_before_ms: now_ms() + 14 * 60_000,
            by_autopilot: true,
            by_person: false,
            wall: RecoveryWall::Network {
                what: "an API error (the API could not be reached)".into(),
                measured: true,
            },
        }),
        ..Default::default()
    }));
    // Model a pending system-proxy route read with an explicit awaited helper.
    // No production sleep or live host proxy/network timing determines the race.
    let (entered, entry) = tokio::sync::oneshot::channel();
    let (release, blocked) = tokio::sync::oneshot::channel();
    *lock(&h.autopilot.before_rearm_route) = Some((entered, blocked));
    let resumed = h
        .test
        .thread_manager
        .resume_legacy_thread_from_rollout(
            h.test.config.clone(),
            rollout.clone(),
            h.test.thread_manager.auth_manager(),
            None,
            Default::default(),
        )
        .await
        .expect("native resume spawns detached scan");
    let old = resumed
        .thread
        .thread_extension_data()
        .get::<ThreadRt>()
        .expect("resumed runtime");
    entry
        .await
        .expect("detached native resume reached carried route read");
    let restored_key = old.state().needs.key;
    assert_eq!(
        restored_key != 0,
        old_needs,
        "the real resume restored the expected historical needs"
    );
    assert!(!old.scanned.load(Ordering::SeqCst));

    if off {
        let needs = old.state().needs;
        h.autopilot.sup.needs(&needs.standing, needs.key).await;
        h.fake.set_effective_quietly(false, "autopilot-off");
        let client = h.autopilot.sup.client().expect("claimed client");
        h.autopilot
            .sup
            .release_for_off(
                &client,
                "fixture Off during route read".into(),
                codex_alab_core::session::HarnessWord::Off,
            )
            .await;
        assert_eq!(h.autopilot.sup.needs_key(), 0, "Off completed its clear");
        assert_eq!(
            h.shared.needs_off(),
            restored_key,
            "Off persisted the dismissed set"
        );
    }

    let replacement = if replace {
        resumed
            .thread
            .shutdown_and_wait()
            .await
            .expect("stop during route read");
        assert!(h.autopilot.thread_rt(old.key()).is_none());
        let fresh = h
            .test
            .thread_manager
            .resume_legacy_thread_from_rollout(
                h.test.config.clone(),
                rollout,
                h.test.thread_manager.auth_manager(),
                None,
                Default::default(),
            )
            .await
            .expect("genuine same-ID replacement");
        let rt = fresh
            .thread
            .thread_extension_data()
            .get::<ThreadRt>()
            .expect("replacement runtime");
        assert!(!Arc::ptr_eq(&old, &rt));
        wait_for_supervisor(|| rt.scanned.load(Ordering::SeqCst)).await;
        rt.with_state(|state| {
            state.needs = decide::Needs {
                standing: vec!["fresh approval".into()],
                key: 22,
                acked: None,
                ..Default::default()
            };
        });
        h.autopilot.sup.want_needs(&["fresh approval".into()], 22);
        Some(fresh)
    } else {
        None
    };
    release.send(()).expect("release the old route read");
    wait_for_supervisor(|| old.scanned.load(Ordering::SeqCst)).await;
    // The scanner stores scanned only after the synchronous needs publication.
    // Join its evaluation permit too, then retire every owned native thread before
    // checking the verdict, so a failing regression leaves no blocked producer.
    let finished = old.hold_eval_when_free().await;
    let actual = (h.autopilot.sup.needs_key(), h.shared.needs_off());
    drop(finished);
    let fresh_report = if off {
        // A genuinely new native report after Off may raise new needs even while
        // the claim is released. The fence cannot be a blanket acting() check.
        responses::mount_sse_once(
            &h._server,
            responses::sse(vec![
                responses::ev_response_created("fresh-response"),
                responses::ev_assistant_message("fresh-message", "NEEDS YOU: fresh approval"),
                responses::ev_completed("fresh-response"),
            ]),
        )
        .await;
        let start = resumed
            .thread
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Give a new report after Off.".into(),
                text_elements: Vec::new(),
            }]))
            .await
            .expect("new native person turn after Off");
        let codex_protocol::turn_input::TurnInputSubmission::Started { turn_id, .. } = start else {
            panic!("expected a new native turn after Off");
        };
        core_test_support::wait_for_event_with_timeout(
            &resumed.thread,
            |event| matches!(event, EventMsg::TurnComplete(_)),
            HANG,
        )
        .await;
        wait_for_supervisor(|| {
            h.autopilot
                .with_current_runtime(&old, || {
                    old.state()
                        .native_origin
                        .is_some_and(|origin| origin.turn_id == turn_id)
                        && !h.shared.turn_running_on(old.key())
                })
                .unwrap_or(false)
        })
        .await;
        Some((
            h.autopilot.sup.needs_key(),
            old.state().needs.key,
            h.shared.needs_off(),
        ))
    } else {
        None
    };
    match replacement {
        Some(fresh) => fresh
            .thread
            .shutdown_and_wait()
            .await
            .expect("replacement shutdown"),
        None => resumed
            .thread
            .shutdown_and_wait()
            .await
            .expect("current resume shutdown"),
    }
    if let Some((shown, reported, dismissed)) = fresh_report {
        assert_ne!(reported, 0, "the real new report names needs");
        assert_ne!(reported, restored_key, "the report names a new set");
        assert_eq!(
            (shown, dismissed),
            (reported, 0),
            "a fresh post-Off report is wanted and ends the old dismissal"
        );
    }
    assert_eq!(
        actual,
        if off {
            (0, restored_key)
        } else {
            (if replace { 22 } else { restored_key }, 0)
        },
        "a route read cannot authorize retired or dismissed historical needs; a current scanner must publish its own"
    );
}

#[test]
fn a_current_resume_surfaces_its_restored_needs() {
    native_test(|| async {
        tokio::time::timeout(HANG, check_resume_needs(DuringRoute::Current, true))
            .await
            .expect("current resume needs hang detector");
    });
}

#[test]
fn a_retired_resume_cannot_reraise_historical_needs() {
    native_test(|| async {
        tokio::time::timeout(HANG, check_resume_needs(DuringRoute::Replace, true))
            .await
            .expect("retired resume needs hang detector");
    });
}

#[test]
fn a_retired_resume_cannot_clear_fresh_needs() {
    native_test(|| async {
        tokio::time::timeout(HANG, check_resume_needs(DuringRoute::Replace, false))
            .await
            .expect("retired empty resume needs hang detector");
    });
}

#[test]
fn an_off_during_resume_keeps_historical_needs_dismissed_but_allows_a_fresh_report() {
    native_test(|| async {
        tokio::time::timeout(HANG, check_resume_needs(DuringRoute::Off, true))
            .await
            .expect("Off during resume needs hang detector");
    });
}
