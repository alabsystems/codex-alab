// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A quota evaluation can outlive its point while its actual record write waits.

use super::*;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStopInput;
use std::future::Future;
use std::sync::Condvar;
use std::sync::Mutex;
use std::task::Poll;

struct RecordGate(Arc<(Mutex<bool>, Condvar)>);

impl RecordGate {
    fn install(shared: &'static Shared) -> (Self, tokio::sync::oneshot::Receiver<()>) {
        let gate = Self(Arc::new((Mutex::new(false), Condvar::new())));
        let waiting = Arc::clone(&gate.0);
        let (entered, reading) = tokio::sync::oneshot::channel();
        let entered = Mutex::new(Some(entered));
        assert!(shared.set_flush(Box::new(move || {
            let first = entered.lock().unwrap().take();
            if let Some(entered) = first {
                entered.send(()).expect("record observer remains");
                let (released, changed) = &*waiting;
                let ready = released.lock().unwrap();
                let (ready, _) = changed
                    .wait_timeout_while(ready, Duration::from_secs(60), |released| !*released)
                    .unwrap();
                assert!(*ready, "record write hang detector");
            }
            true
        })));
        (gate, reading)
    }

    fn release(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

impl Drop for RecordGate {
    fn drop(&mut self) {
        self.release();
    }
}

async fn quota_evaluation_after_record_wait(change: &str) {
    let h = started_with_claim_watch(&[("continue", "false")], |_, _| {}, false).await;
    wait_go(&h.sup).await;
    point(&h, ended(None, Some(Wall::Entitlement)));
    h.rt.with_state(|state| {
        state.point_turn_id = Some("quota-before-the-record-wait".into());
    });
    h.autopilot
        .set_recovery_write_budget(Duration::from_secs(60));
    let (gate, reading) = RecordGate::install(h.shared);
    let autopilot = Arc::clone(&h.autopilot);
    let runtime = Arc::clone(&h.rt);
    let evaluating = tokio::spawn(async move { autopilot.evaluate(runtime).await });
    reading
        .await
        .expect("quota evaluator reached its actual record write");
    assert!(h.shared.recovery().is_some(), "the quota wait was captured");

    match change {
        "native-start" | "native-success" => {
            let session = ExtensionData::new("session");
            let newer = ExtensionData::new("native-after-the-quota");
            start_native_turn(&h, &session, &newer).await;
            if change == "native-success" {
                h.autopilot
                    .on_turn_stop(TurnStopInput {
                        session_store: &session,
                        thread_store: &h._store,
                        turn_store: &newer,
                    })
                    .await;
                assert!(!h.shared.turn_running_on(h.rt.key()));
            }
        }
        "off-on" => {
            h.fake.set_effective(false, "autopilot-off");
            wait_until("Off releases the claim", || {
                h.fake.claim().is_none() && !h.sup.acting()
            })
            .await;
            act_again(&h).await;
        }
        "unchanged" => {}
        _ => unreachable!(),
    }

    // Queue behind this exact evaluation before letting it finish. Any fresh
    // evaluation it schedules then waits behind us, so the assertion distinguishes
    // the obsolete publisher from a legitimate new look at the current point.
    let mut next = Box::pin(h.rt.hold_eval_when_free());
    std::future::poll_fn(|cx| {
        assert!(next.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    gate.release();
    let _next = next.await;
    evaluating.await.expect("quota evaluation finishes");
    Autopilot::cancel_timer(&h.rt).await;
    h.sup.show_needs().await;
    let needs = h.fake.attention().get("needs").cloned();
    if change == "unchanged" {
        assert!(
            needs
                .as_deref()
                .is_some_and(|need| need.contains(crate::decide::QUOTA_NEED)),
            "an unchanged quota point still raises its own need: {needs:?}"
        );
    } else {
        assert_eq!(
            needs, None,
            "{change}: an obsolete quota evaluation must not restore its need"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_obsolete_quota_evaluation_cannot_restore_its_need() {
    tokio::time::timeout(Duration::from_secs(60), async {
        for change in ["native-start", "native-success", "off-on"] {
            quota_evaluation_after_record_wait(change).await;
        }
    })
    .await
    .expect("obsolete quota need hang detector");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unchanged_quota_evaluation_still_raises_its_need() {
    tokio::time::timeout(
        Duration::from_secs(60),
        quota_evaluation_after_record_wait("unchanged"),
    )
    .await
    .expect("unchanged quota need hang detector");
}
