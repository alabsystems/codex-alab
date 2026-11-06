// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Live policy events reconsider an existing wait without a turn or timer firing.

use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retry_settings_reconsider_an_armed_wait_in_the_same_mode_and_under_hold() {
    let mut observed = Vec::new();
    for held in [false, true] {
        for limit in [false, true] {
            // Install the original wait deterministically, then use only the real
            // claims watcher for policy changes; never manually evaluate those changes.
            let h = started_with_claim_watch(&[], |_, _| {}, false).await;
            wait_until("the original claim acts", || h.sup.acting()).await;
            let deadline = Instant::now() + Duration::from_secs(3600);
            let wall = if limit {
                Wall::Entitlement
            } else {
                Wall::Server {
                    what: "model at capacity".into(),
                    cut_off: false,
                    retry_at: Some(deadline),
                }
            };
            point(&h, ended(None, Some(wall)));
            Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
            let original = h.rt.owed_timer().expect("the original paced retry");
            assert!(h.rt.timer_pending());
            assert!(h.shared.owed_act().is_some());
            let mode = h.sup.policy().expect("original policy").autopilot;
            if held {
                h.fake.set_status("hold", "1");
                h.fake.set_effective(false, "hold");
                wait_until("the kept claim reads the hold", || {
                    h.sup.journal().iter().any(|line| {
                        line.contains("policy effective=off why=hold: the claim is kept")
                    })
                })
                .await;
            }
            h.autopilot.watch_claims();
            let setting = if limit {
                "resume_limits"
            } else {
                "retry_api_errors"
            };
            h.fake.set_policy_key(setting, "false");
            wait_until(
                "the policy event retires the wait and raises its refusal",
                || {
                    h.sup.policy().is_some_and(|policy| {
                        policy.autopilot == mode
                            && if limit {
                                !policy.resume_limits
                            } else {
                                !policy.retry_api_errors
                            }
                    }) && shown(&h).is_some_and(|text| text.contains(setting))
                        && !h.rt.timer_pending()
                        && h.rt.owed_timer().is_none()
                        && h.shared.owed_act().is_none()
                },
            )
            .await;
            // Enabling the setting is also a same-mode event. The unresolved
            // point remains, so it regains its paced retry, including provider time.
            h.fake.set_policy_key(setting, "true");
            wait_until(
                "the enabled policy restores the original unspent wait",
                || {
                    h.rt.owed_timer().is_some_and(|owed| {
                        owed.rule == original.rule && owed.not_before == original.not_before
                    }) && h.rt.timer_pending()
                        && h.shared.owed_act().is_some()
                },
            )
            .await;
            observed.push((
                held,
                limit,
                h.sup.policy().expect("enabled policy").autopilot == mode,
            ));
            tokio::time::timeout(Duration::from_secs(60), Autopilot::cancel_timer(&h.rt))
                .await
                .expect("policy wait cleanup hang detector");
            let root = h._dir.keep();
            eprintln!(
                "retained policy wake fixture (hold={held}, limit={limit}): {}",
                root.display()
            );
        }
    }
    assert!(
        observed.iter().all(|(_, _, same_mode)| *same_mode),
        "{observed:?}"
    );
}
