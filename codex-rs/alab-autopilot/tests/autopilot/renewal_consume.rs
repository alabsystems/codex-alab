// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

struct StopBarrier {
    calls: Arc<std::sync::atomic::AtomicU64>,
    durable_gate: Option<Arc<tokio::sync::Semaphore>>,
}

impl codex_extension_api::TurnLifecycleContributor for StopBarrier {
    fn on_turn_stop<'a>(
        &'a self,
        _input: codex_extension_api::TurnStopInput<'a>,
    ) -> codex_extension_api::ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 1
                && let Some(gate) = &self.durable_gate
            {
                gate.acquire()
                    .await
                    .unwrap_or_else(|error| panic!("closed durable stop gate: {error}"))
                    .forget();
            }
        })
    }
}

struct BeforeStopGate {
    calls: std::sync::atomic::AtomicU64,
    proceed: tokio::sync::Semaphore,
}

impl codex_extension_api::TurnLifecycleContributor for BeforeStopGate {
    fn on_turn_stop<'a>(
        &'a self,
        _input: codex_extension_api::TurnStopInput<'a>,
    ) -> codex_extension_api::ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
                self.proceed
                    .acquire()
                    .await
                    .unwrap_or_else(|error| panic!("closed stop gate: {error}"))
                    .forget();
            }
        })
    }
}

#[test]
fn resumed_native_stop_releases_only_the_exact_confirmed_consumed_custody() {
    big_stack(|| async {
        for case in [
            "acknowledged",
            "lost-ack-held",
            "lost-ack-reconciled",
            "wrong-account",
            "newer-intent",
            "retry-passive",
            "retry-newer-intent",
            "retry-new-native",
            "retry-thread-stop",
            "retry-claim-lost",
            "retry-no-claim-at-stop",
        ] {
            let (login, identity, _refresh) = subscription_fixture(case).await;
            let refused = wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error":{"message":"expired","type":"invalid_request_error","code":"invalid_api_key"}
            }));
            let stopped = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let durable_stop = Arc::new(tokio::sync::Semaphore::new(0));
            let before_stop = Arc::new(BeforeStopGate {
                calls: std::sync::atomic::AtomicU64::new(0),
                proceed: tokio::sync::Semaphore::new(0),
            });
            let h = harness_login_with_error_hook(
                Mode::Current,
                vec![
                    refused.clone(),
                    refused,
                    responses::sse_response(reply("recovered", "Original work.")),
                ],
                &[("continue", "false"), ("human_grace_s", "0")],
                &[],
                Some(0),
                Some(login),
                Timing {
                    min_work: Duration::ZERO,
                    ..Timing::default()
                },
                |_| {},
                None,
                ErrorHooks {
                    before: (case == "retry-no-claim-at-stop").then(|| {
                        Arc::clone(&before_stop)
                            as Arc<dyn codex_extension_api::TurnLifecycleContributor>
                    }),
                    after: Some(Arc::new(StopBarrier {
                        calls: Arc::clone(&stopped),
                        durable_gate: (case == "retry-passive").then(|| Arc::clone(&durable_stop)),
                    })),
                    ..Default::default()
                },
            )
            .await;
            h.test
                .submit_turn("Recover this original native request.")
                .await
                .expect("original failure");
            wait_until("original stop hook", || stopped.load(Ordering::SeqCst) == 1).await;
            let wall = h.shared.latest_turn(&h.thread).expect("original native ID");
            assert!(
                h.test
                    .codex
                    .state_db()
                    .expect("custody DB")
                    .thread_goals()
                    .alab_capture_auth_standalone(h.test.session_configured.thread_id, &wall)
                    .await
                    .expect("actual standalone native proof")
            );
            let marker = OwedAct {
                thread: h.thread.clone(),
                native_origin: Some(NativeOrigin {
                    turn_id: wall.clone(),
                    phase: NativePhase::Complete,
                }),
                renewal: Some(AccountHandoff {
                    purpose: codex_alab_core::account_handoff::Purpose::Login,
                    destination: None,
                    openai_scope: true,
                    local: false,
                    account: identity.account.clone(),
                    user: identity.user.clone(),
                    receipt: Some("c".repeat(32)),
                    turn: None,
                    accepted: false,
                    starts: 1,
                    goal_id: None,
                }),
                rule: Some("login-back@v1".into()),
                text: Some("Restored native custody.".into()),
                mode: Some("land".into()),
                ..Default::default()
            };
            h.shared.set_owed_act(Some(marker));
            let flushed = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let writes = Arc::clone(&flushed);
            assert!(h.shared.set_flush(Box::new(move || {
                writes.fetch_add(1, Ordering::SeqCst);
                true
            })));
            let command = format!(
                "@{} codex-renewal consume id={} turn={wall}",
                h.fake.sid(),
                "c".repeat(32)
            );
            let account = if case == "wrong-account" {
                "d".repeat(64)
            } else {
                identity.account.clone()
            };
            h.fake.reply_once(&command, &format!(
                "OK codex-renewal state=consumed id={} thread={} wall={wall} account={account} user={}\n",
                "c".repeat(32), h.thread, identity.user));
            if case == "lost-ack-reconciled" {
                h.fake.reply_once(&format!("@{} codex-renewal status id={}", h.fake.sid(), "c".repeat(32)),
                    &format!("OK codex-renewal state=consumed id={} thread={} wall={wall} account={} user={}\n",
                        "c".repeat(32), h.thread, identity.account, identity.user));
            }
            h.fake.gate("codex-renewal consume");
            if case == "lost-ack-held" {
                // A closed claim connection cannot authorize even status until
                // the supervisor normally restores it; custody must remain.
                h.fake.drop_next_reply("codex-renewal consume");
            } else if case == "lost-ack-reconciled" || case.starts_with("retry-") {
                // The mutation's ACK is unreadable while the claim survives;
                // the independently typed status response proves completion.
                h.fake
                    .reply_once(&command, "OK codex-renewal state=unreadable\n");
            }
            let shared = h.shared;
            let expected = wall.clone();
            let mut init = ExtensionDataInit::new();
            init.insert(codex_alab_core::renewal::TurnAdmission::new(move |turn| {
                let expected = expected.clone();
                Box::pin(async move {
                    if turn != expected {
                        return Err("wrong original turn".into());
                    }
                    let mut owed = shared.owed_act().ok_or("missing restored custody")?;
                    let thread = owed.thread.clone();
                    owed.renewal.as_mut().ok_or("missing native receipt")?.turn = Some(turn);
                    if !shared.publish_renewal(shared.upgrade_generation(), owed)
                        || !shared.flush_record()
                        || !shared.admit_recovered_turn(&thread, &expected)
                    {
                        return Err("admission changed".into());
                    }
                    Ok(())
                })
            }));
            let accepted = h
                .test
                .codex
                .recover_turn_if_idle(WithTurnExtensionData::new(
                    RecoverTurnRequest {
                        turn_id: wall.clone(),
                        thread_settings: Default::default(),
                        trace: None,
                        cyber_access_program: None,
                    },
                    init,
                ))
                .await
                .expect("original-ID admission");
            assert!(
                matches!(accepted, StartIfIdleSubmission::Started { turn_id, .. } if turn_id == wall)
            );
            if case == "retry-no-claim-at-stop" {
                wait_until("actual core stop before acknowledgment", || {
                    before_stop.calls.load(Ordering::SeqCst) == 2
                })
                .await;
                h.fake.claim_as("a-real-other-holder");
                wait_until("native claim unavailable at stop", || {
                    !h.autopilot.supervisor().held()
                })
                .await;
                before_stop.proceed.add_permits(1);
                wait_until("unclaimed stop completed", || {
                    stopped.load(Ordering::SeqCst) == 2
                })
                .await;
            } else {
                assert!(
                    h.fake
                        .wait_for(|fake| fake.gated_requests().iter().any(|line| line == &command))
                        .await,
                    "{case}: actual stop reached bounded Consume"
                );
            }
            let acknowledged = h
                .shared
                .owed_act()
                .expect("custody stays until acknowledgment");
            assert!(
                acknowledged
                    .renewal
                    .as_ref()
                    .is_some_and(|renewal| renewal.accepted && renewal.starts == 1)
            );
            let before_ack_writes = flushed.load(Ordering::SeqCst);
            let mut expected_after = Some(acknowledged.clone());
            if case == "newer-intent" {
                // The real core stop has been reached. Reflect that terminal
                // native event as the TUI can before the ACK finishes, then
                // publish the independently owned replacement while idle.
                h.shared.turn_ended(&h.thread, &wall);
                // Keep the genuine standalone goal witness: inventing a goal
                // association would correctly trigger provenance retirement.
                assert_eq!(
                    h.test
                        .codex
                        .state_db()
                        .expect("DB")
                        .thread_goals()
                        .alab_auth_goal_context(h.test.session_configured.thread_id, &wall)
                        .await
                        .expect("context"),
                    Some(None)
                );
                let mut newer = acknowledged;
                newer.not_before_ms = now_ms() + 1_000;
                assert!(
                    h.shared
                        .publish_renewal(h.shared.upgrade_generation(), newer.clone())
                );
                expected_after = Some(newer);
            } else if matches!(case, "acknowledged" | "lost-ack-reconciled") {
                expected_after = None;
            }
            h.fake.open_gate("codex-renewal consume");
            wait_until("stop acknowledgment completed", || {
                stopped.load(Ordering::SeqCst) == 2
            })
            .await;
            assert_eq!(
                h.shared.owed_act(),
                expected_after,
                "{case}: only exact positive ACK releases custody"
            );
            assert_eq!(
                flushed.load(Ordering::SeqCst),
                before_ack_writes
                    + u64::from(matches!(case, "acknowledged" | "lost-ack-reconciled")),
                "{case}: confirmed release is written through; held/new intent is preserved"
            );
            assert_eq!(h.mock.requests().len(), 3);
            assert_eq!(
                h.mock.requests()[1].body_json()["input"],
                h.mock.requests()[2].body_json()["input"],
                "{case}: completed original request is not replaced"
            );
            assert_eq!(
                h.shared.latest_turn(&h.thread).as_deref(),
                Some(wall.as_str())
            );
            if case.starts_with("retry-") {
                let rt = h.autopilot.thread_rt(&h.thread).expect("current runtime");
                assert!(
                    rt.renewal_ack_attempts().is_some(),
                    "{case}: uncertainty has an independent retry"
                );
                if case == "retry-no-claim-at-stop" {
                    wait_until("unclaimed custody observation retried", || {
                        rt.renewal_ack_attempts()
                            .is_some_and(|attempts| attempts > 0)
                    })
                    .await;
                    assert!(
                        h.fake
                            .requests()
                            .iter()
                            .all(|line| !line.contains("codex-renewal")),
                        "no claim is acquired or bypassed"
                    );
                    assert_eq!(h.shared.owed_act(), expected_after);
                    continue;
                }
                if case == "retry-passive" {
                    let calls = h
                        .fake
                        .requests()
                        .iter()
                        .filter(|line| line.contains("codex-renewal"))
                        .count();
                    wait_until("typed stop observed before durable completion", || {
                        rt.renewal_ack_attempts()
                            .is_some_and(|attempts| attempts > 0)
                    })
                    .await;
                    assert_eq!(
                        h.fake
                            .requests()
                            .iter()
                            .filter(|line| line.contains("codex-renewal"))
                            .count(),
                        calls,
                        "a lifecycle stop alone cannot grant a retry RPC before durable Complete"
                    );
                    assert_eq!(h.shared.owed_act(), expected_after);
                    durable_stop.add_permits(1);
                }
                h.fake.set_effective(false, "suspended");
                wait_until("passive with native claim retained", || {
                    h.autopilot.supervisor().standby().is_some()
                })
                .await;
                assert!(!h.autopilot.supervisor().acting());
                assert!(h.autopilot.supervisor().policy().is_none());
                let status = format!(
                    "@{} codex-renewal status id={}",
                    h.fake.sid(),
                    "c".repeat(32)
                );
                let renewal_calls = || {
                    h.fake
                        .requests()
                        .iter()
                        .filter(|line| line.contains("codex-renewal"))
                        .count()
                };
                if case == "retry-claim-lost" {
                    h.fake.claim_as("a-real-other-holder");
                    wait_until("native claim loss processed", || {
                        !h.autopilot.supervisor().held()
                    })
                    .await;
                    assert!(
                        h.fake
                            .claim()
                            .is_some_and(|claim| claim.holder == "a-real-other-holder")
                    );
                    let calls = renewal_calls();
                    let attempts = rt.renewal_ack_attempts().expect("held retry");
                    wait_until("custody-only attempt without native claim", || {
                        rt.renewal_ack_attempts().is_some_and(|now| now > attempts)
                    })
                    .await;
                    assert_eq!(
                        renewal_calls(),
                        calls,
                        "no RPC bypasses the missing native claim"
                    );
                    assert_eq!(
                        h.shared.owed_act(),
                        expected_after,
                        "unknown claim keeps evidence"
                    );
                } else {
                    h.fake.gate("codex-renewal status");
                    assert!(
                        h.fake
                            .wait_for(|fake| fake
                                .gated_requests()
                                .iter()
                                .any(|line| line == &status))
                            .await,
                        "{case}: passive custody-only status observation occurs"
                    );
                    h.fake.reply_once(&status, &format!(
                        "OK codex-renewal state=consumed id={} thread={} wall={wall} account={} user={}\n",
                        "c".repeat(32), h.thread, identity.account, identity.user));
                    let before_cleanup = flushed.load(Ordering::SeqCst);
                    if case == "retry-newer-intent" {
                        let mut newer = h.shared.owed_act().expect("held original");
                        assert_eq!(
                            h.test
                                .codex
                                .state_db()
                                .expect("DB")
                                .thread_goals()
                                .alab_auth_goal_context(h.test.session_configured.thread_id, &wall)
                                .await
                                .expect("context"),
                            Some(None)
                        );
                        newer.not_before_ms = now_ms() + 1_000;
                        assert!(
                            h.shared
                                .publish_renewal(h.shared.upgrade_generation(), newer.clone())
                        );
                        expected_after = Some(newer);
                    } else if case == "retry-new-native" {
                        assert!(
                            h.shared
                                .turn_started(&h.thread, "019a0000-0000-7000-8000-000000000099")
                        );
                        expected_after = None;
                    } else if case == "retry-thread-stop" {
                        h.test
                            .codex
                            .submit(Op::Shutdown)
                            .await
                            .expect("real thread shutdown");
                        wait_for_event(&h.test.codex, |event| {
                            matches!(event, EventMsg::ShutdownComplete)
                        })
                        .await;
                    } else {
                        expected_after = None;
                    }
                    h.fake.open_gate("codex-renewal status");
                    wait_until("exact retry completed or cancelled", || {
                        rt.renewal_ack_attempts().is_none()
                    })
                    .await;
                    assert_eq!(
                        h.shared.owed_act(),
                        expected_after,
                        "{case}: stale observations cannot release current custody"
                    );
                    if case == "retry-passive" {
                        assert_eq!(flushed.load(Ordering::SeqCst), before_cleanup + 1);
                        assert!(
                            !h.autopilot.supervisor().acting(),
                            "cleanup never acquires execution authority"
                        );
                    }
                    assert_eq!(
                        h.mock.requests().len(),
                        3,
                        "{case}: cleanup never starts another model request"
                    );
                    assert_eq!(
                        h.fake
                            .requests()
                            .iter()
                            .filter(|line| line.contains("codex-renewal consume"))
                            .count(),
                        1,
                        "{case}: confirmed status requires no new Consume mutation"
                    );
                }
            }
        }
    });
}
