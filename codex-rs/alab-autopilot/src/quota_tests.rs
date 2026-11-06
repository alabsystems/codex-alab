// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;
use core_test_support::responses::received_responses_requests;

fn identity(account: &str, owner: u64) -> QuotaIdentity {
    QuotaIdentity {
        account: account.into(),
        user: Some("native-user".into()),
        owner,
    }
}

#[test]
fn restored_quota_scope_requires_matching_auth_and_a_new_read() {
    native_test(|| async {
        let h = tokio::time::timeout(Duration::from_secs(60), Harness::new())
            .await
            .expect("native fixture hung");
        h.immediate_point().await;
        let quota = h.shared.quota();
        *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 4));
        let scope = codex_alab_core::quota::Scope {
            account: "account".into(),
            bucket: "premium".into(),
            user: Some("native-user".into()),
            ordinary_retry: true,
            ordinary_denied: true,
        };
        quota.start_read(1, clock());
        quota.accept_observation(
            1,
            "account",
            vec!["codex".into()],
            Some(true),
            vec![],
            clock(),
        );
        h.autopilot.restore_quota_scope(&h.rt, Some(&scope));
        let wall = lock(&h.rt.quota_wall)
            .clone()
            .expect("matching persisted account scope");
        assert!(!quota.restored(&wall.wall, clock()));
        quota.start_read(2, clock());
        quota.accept_observation(
            2,
            "account",
            vec!["codex".into()],
            Some(true),
            vec![],
            clock(),
        );
        assert!(quota.restored(&wall.wall, clock()));
        assert!(quota.ordinary_restored(&wall.wall, clock()));
        *lock(&h.autopilot.quota_test_identity) = Some(identity("other", 5));
        h.autopilot.restore_quota_scope(&h.rt, Some(&scope));
        assert!(lock(&h.rt.quota_wall).is_none());
        let mut other_user = identity("account", 6);
        other_user.user = Some("other-user".into());
        *lock(&h.autopilot.quota_test_identity) = Some(other_user);
        h.autopilot.restore_quota_scope(&h.rt, Some(&scope));
        assert!(lock(&h.rt.quota_wall).is_none());
        *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 6));
        h.autopilot.restore_quota_scope(&h.rt, None);
        assert!(lock(&h.rt.quota_wall).is_none());
    });
}

#[test]
fn accepted_post_wall_allowance_wakes_one_native_recovery_before_reset() {
    native_test(|| async {
        let h = tokio::time::timeout(Duration::from_secs(60), Harness::new())
            .await
            .expect("native fixture hung");
        let result = tokio::time::timeout(Duration::from_secs(60), async {
            h.fake.set_policy_key("upgrade", "false");
            h.fake.set_policy_key("limit_wait", "true");
            wait_for_supervisor(|| {
                h.autopilot
                    .sup
                    .policy()
                    .is_some_and(|p| !p.upgrade && p.limit_wait)
            })
            .await;
            h.immediate_point().await;
            h.hold_model_response().await;
            let quota = h.shared.quota();
            *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 0));
            let owner = quota.select_owner("account", Some("native-user"), 0);
            *lock(&h.rt.quota_request) = Some((identity("account", 0), owner));
            // An error from the admitted account cannot be rebound to a new
            // owner, including switching away and back during the request.
            *lock(&h.autopilot.quota_test_identity) = Some(identity("other", 1));
            assert!(
                h.autopilot
                    .bind_quota_wall(&h.rt, "codex".into(), clock())
                    .is_none()
            );
            *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 2));
            assert!(
                h.autopilot
                    .bind_quota_wall(&h.rt, "codex".into(), clock())
                    .is_none()
            );
            *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 0));
            quota.start_read(1, clock());
            let wall_at = clock();
            *lock(&h.rt.quota_wall) = Some(QuotaWall {
                wall: quota
                    .wall_for("account".into(), "premium".into(), wall_at)
                    .with_ordinary_retry(true, false),
                identity: identity("account", 0),
            });
            assert!(h.autopilot.quota_scope(&h.rt).is_some());
            let mut changed = identity("account", 0);
            changed.user = Some("other-user".into());
            *lock(&h.autopilot.quota_test_identity) = Some(changed);
            assert!(
                h.autopilot.quota_scope(&h.rt).is_none(),
                "old wall cannot be persisted under a new user"
            );
            *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 0));
            h.rt.with_state(|state| {
                state.point.as_mut().unwrap().wall = Some(Wall::Usage {
                    reset_at: Some(wall_at + Duration::from_secs(7 * 86400)),
                    reset_label: None,
                });
            });
            h.autopilot.watching.store(false, Ordering::SeqCst);
            h.autopilot.watch_claims();
            let before = h.rt.evaluations();
            // Even an accepted response cannot prove post-wall recovery if its
            // request began before the wall.
            quota.accept(1, "account", vec!["codex".into()], clock());
            wait_for_supervisor(|| h.rt.evaluations() > before).await;
            assert!(received_responses_requests(&h._server).await.is_empty());
            quota.start_read(2, clock());
            quota.accept_observation(2, "account", vec![], Some(false), vec![], clock());
            quota.start_read(3, clock());
            quota.accept_observation(
                3,
                "account",
                vec!["codex".into()],
                Some(true),
                vec![],
                clock(),
            );
            loop {
                let requests = received_responses_requests(&h._server).await;
                if !requests.is_empty() {
                    assert_eq!(requests.len(), 1);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(lock(&h.rt.quota_wall).is_none());
            assert!(
                h.fake
                    .acts()
                    .iter()
                    .any(|act| act == words::RULE_LIMIT_RESUME)
            );
            Autopilot::cancel_timer(&h.rt).await;
        })
        .await;
        assert!(
            result.is_ok(),
            "native quota recovery hung: state={:?}, requests={:?}",
            h.rt.state(),
            h.fake.requests()
        );
    });
}

/// A reset that arrives before the recorded deadline must survive the network
/// preflight's second decision. Checking reachability cannot restore the old wait.
#[test]
fn restored_quota_survives_network_preflight() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.fake.set_policy_key("upgrade", "false");
            h.fake.set_policy_key("limit_wait", "true");
            h.fake.set_policy_key("probe_api", "true");
            wait_for_supervisor(|| {
                h.autopilot
                    .sup
                    .policy()
                    .is_some_and(|p| !p.upgrade && p.limit_wait && p.probe_api)
            })
            .await;
            h.immediate_point().await;
            let quota = h.shared.quota();
            *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 0));
            quota.select_owner("account", Some("native-user"), 0);
            let at = clock();
            *lock(&h.rt.quota_wall) = Some(QuotaWall {
                wall: quota.wall_for("account".into(), "codex".into(), at),
                identity: identity("account", 0),
            });
            h.rt.with_state(|state| {
                state.point.as_mut().unwrap().wall = Some(Wall::Usage {
                    reset_at: Some(at + Duration::from_secs(7 * 86400)),
                    reset_label: None,
                });
                state.net_asked = None;
            });
            h.rt.set_route(Some(netwatch::Route::Direct(netwatch::Dest {
                host: "127.0.0.1".into(),
                port: 9,
            })));
            quota.start_read(1, clock());
            quota.accept_observation(
                1,
                "account",
                vec!["codex".into()],
                Some(true),
                vec![],
                clock(),
            );
            Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
            assert!(h.rt.state().net_asked.is_some(), "network preflight ran");
            let owed = h.rt.owed_timer().expect("network readiness wait");
            assert!(
                owed.not_before < at + Duration::from_secs(86400),
                "restored quota was lost and the old reset deadline returned: {owed:?}"
            );
            Autopilot::cancel_timer(&h.rt).await;
        })
        .await
        .expect("quota network preflight hung");
    });
}

#[test]
fn subscription_transition_wakes_one_native_continuation() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.fake.set_policy_key("upgrade", "false");
            wait_for_supervisor(|| h.autopilot.sup.policy().is_some_and(|p| !p.upgrade)).await;
            h.immediate_point().await;
            h.hold_model_response().await;
            let quota = h.shared.quota();
            *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 0));
            quota.select_owner("account", Some("native-user"), 0);
            *lock(&h.rt.quota_wall) = Some(QuotaWall {
                wall: quota.wall_for("account".into(), "codex".into(), clock()),
                identity: identity("account", 0),
            });
            h.rt.with_state(|state| state.point.as_mut().unwrap().wall = Some(Wall::Entitlement));
            h.autopilot.watching.store(false, Ordering::SeqCst);
            h.autopilot.watch_claims();

            // Ordinary access that already existed is not proof that a missing
            // subscription or premium entitlement changed.
            let before = h.rt.evaluations();
            quota.start_read(1, clock());
            quota.accept_observation(
                1,
                "account",
                vec!["codex".into()],
                Some(true),
                vec![],
                clock(),
            );
            wait_for_supervisor(|| h.rt.evaluations() > before).await;
            assert!(received_responses_requests(&h._server).await.is_empty());

            quota.start_read(2, clock());
            quota.accept_observation(2, "account", vec![], Some(false), vec![], clock());
            quota.start_read(3, clock());
            quota.accept_observation(
                3,
                "account",
                vec!["codex".into()],
                Some(true),
                vec![],
                clock(),
            );
            loop {
                let requests = received_responses_requests(&h._server).await;
                if !requests.is_empty() {
                    assert_eq!(requests.len(), 1);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(lock(&h.rt.quota_wall).is_none());
            // A slow resumed response must not turn each later account poll
            // into another model request. Drive each evaluation to completion,
            // rather than relying on a quiet-time delay to prove no duplicate.
            for id in 4..=8 {
                quota.start_read(id, clock());
                quota.accept_observation(
                    id,
                    "account",
                    vec!["codex".into()],
                    Some(true),
                    vec![],
                    clock(),
                );
                Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
                assert_eq!(received_responses_requests(&h._server).await.len(), 1);
                assert_eq!(
                    h.fake
                        .acts()
                        .iter()
                        .filter(|act| *act == words::RULE_LIMIT_RESUME)
                        .count(),
                    1
                );
                assert!(h.rt.owed_timer().is_none());
            }
            Autopilot::cancel_timer(&h.rt).await;
        })
        .await
        .expect("subscription watcher hung");
    });
}
