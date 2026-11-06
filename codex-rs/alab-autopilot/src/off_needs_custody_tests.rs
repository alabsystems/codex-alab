// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A real native report after the claim's release outruns the Off's alert cleanup.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;
use core_test_support::responses;

const HANG: Duration = Duration::from_secs(60);

struct StartGate(
    Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
);

impl TurnLifecycleContributor for StartGate {
    fn on_turn_start<'a>(&'a self, _input: TurnStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let gate = lock(&self.0).take();
            if let Some((entered, release)) = gate {
                entered.send(()).expect("native start observer remains");
                release.await.expect("native start release");
            }
        })
    }
}

async fn report(h: &Harness, id: &str, text: &str) {
    responses::mount_sse_once(
        &h._server,
        responses::sse(vec![
            responses::ev_response_created(id),
            responses::ev_assistant_message(&format!("{id}-message"), text),
            responses::ev_completed(id),
        ]),
    )
    .await;
    let start = h
        .test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: format!("Give the {id} report."),
            text_elements: Vec::new(),
        }]))
        .await
        .expect("owned native report turn");
    let codex_protocol::turn_input::TurnInputSubmission::Started { turn_id, .. } = start else {
        panic!("expected a new native turn");
    };
    core_test_support::wait_for_event_with_timeout(
        &h.test.codex,
        |event| matches!(event, EventMsg::TurnComplete(_)),
        HANG,
    )
    .await;
    wait_for_supervisor(|| {
        h.autopilot
            .with_current_runtime(&h.rt, || {
                h.rt.state()
                    .native_origin
                    .is_some_and(|origin| origin.turn_id == turn_id)
                    && !h.shared.turn_running_on(h.rt.key())
            })
            .unwrap_or(false)
    })
    .await;
}

#[test]
fn an_offs_late_cleanup_keeps_a_fresh_native_need_named_while_released() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let start_gate = Arc::new(StartGate(Mutex::new(None)));
            let h = Harness::new_with_goals_and_observer(None, Some(start_gate.clone())).await;
            h.immediate_point().await;
            report(&h, "old", "NEEDS YOU: old approval").await;
            let old = h.rt.state().needs.key;
            wait_for_supervisor(|| h.fake.attention().contains_key("needs")).await;
            // Core has accepted this automatic turn before Off. Its real response
            // finishes after the release. Starting a different turn after the release
            // would first wait for the earlier alert I/O in the native start hook.
            let (entered, entry) = tokio::sync::oneshot::channel();
            let (release, blocked) = tokio::sync::oneshot::channel();
            *lock(&start_gate.0) = Some((entered, blocked));
            h.shared.expect_own_turn("Give the fresh report.");
            let fresh_report = report(&h, "fresh", "NEEDS YOU: fresh approval");
            tokio::pin!(fresh_report);
            tokio::select! {
                _ = &mut fresh_report => panic!("native report finished before its held start"),
                entered = entry => entered.expect("actual native start reached observer"),
            }
            h.autopilot
                .sup
                .attention_for(h.rt.key(), "wall", Some("wall: old blocker".into()))
                .await;
            // The actual Off gives the claim back before clearing its own alert.
            // Hold that real control request, with no production test latch or delay.
            h.fake.gate("meta unset attention owner=codex-alab");
            h.fake.set_effective_quietly(false, "autopilot-off");
            let client = h.autopilot.sup.client().expect("claimed client");
            let sup = Arc::clone(&h.autopilot.sup);
            let off = tokio::spawn(async move {
                sup.release_for_off(&client, "fixture explicit Off".into(), codex_alab_core::session::HarnessWord::Off)
                    .await;
            });
            wait_for_supervisor(|| {
                h.fake.claim().is_none()
                    && h.fake.gated_requests().iter().any(|line| {
                        line.contains("meta unset attention owner=codex-alab")
                    })
            })
            .await;
            release.send(()).expect("release native response after Off");
            fresh_report.await;
            let fresh = h.rt.state().needs.key;
            let wanted_before_cleanup = h.autopilot.sup.needs_key();
            h.fake.open_gate("meta unset attention owner=codex-alab");
            off.await.expect("Off cleanup completes");
            let after_cleanup = (h.autopilot.sup.needs_key(), h.shared.needs_off());
            let shown_while_released = h.fake.attention().get("needs").cloned();
            h.fake.set_effective(true, "on");
            wait_for_supervisor(|| h.autopilot.sup.acting()).await;
            // Reconciliation of the new claim is complete only after its needs sync.
            wait_for_supervisor(|| {
                h.autopilot
                    .sup
                    .journal()
                    .iter()
                    .filter(|line| line.contains("claimed"))
                    .count()
                    >= 2
            })
            .await;
            h.autopilot.sup.show_needs().await;
            let shown = h.fake.attention().get("needs").cloned();
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("owned native shutdown before verdict");
            assert_ne!(old, 0);
            assert_ne!(fresh, 0);
            assert_ne!(fresh, old, "the new native report names a new need");
            assert_eq!(wanted_before_cleanup, fresh, "accepted after actual release");
            assert_eq!(
                after_cleanup,
                (fresh, 0),
                "an Off's delayed transport cleanup cannot dismiss a report named after its claim was given back"
            );
            assert!(shown_while_released.is_none(), "Off takes the old entry down");
            assert_eq!(
                shown.as_deref(),
                Some("Codex needs you: fresh approval — carrying on"),
                "a need named while released is shown once claimed again"
            );
        })
        .await
        .expect("Off native needs custody hang detector");
    });
}
