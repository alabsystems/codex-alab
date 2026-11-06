// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Loaded roots keep independent failures, aggregated into codex-alab's one remote entry.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::*;
use codex_alab_core::session::Frame;
use codex_alab_core::session::ProgramState;
use codex_protocol::items::UserMessageItem;

fn need_authority(h: &Harness, rt: &ThreadRt) -> DecisionAuthority {
    DecisionAuthority {
        quota: None,
        quota_identity: None,
        policy: h.autopilot.sup.policy().expect("proven policy"),
        offs: h.autopilot.sup.offs(),
        point: rt.state().point_at(),
        accepted_turns: rt.accepted_turns.load(Ordering::SeqCst),
        native_origin: rt.state().native_origin,
    }
}

async fn begin(h: &Harness, thread: &Arc<CodexThread>, turn: &ExtensionData) {
    let mode = thread.thread_settings_snapshot().await.collaboration_mode;
    h.autopilot
        .on_turn_start(TurnStartInput {
            turn_id: turn.level_id(),
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &ExtensionData::new("session"),
            thread_store: thread.thread_extension_data(),
            turn_store: turn,
        })
        .await;
    h.autopilot
        .on_item_completed(
            thread.thread_extension_data(),
            turn,
            &TurnItem::UserMessage(UserMessageItem {
                id: format!("input-{}", turn.level_id()),
                client_id: None,
                content: vec![UserInput::Text {
                    text: "a corrected task in this root".into(),
                    text_elements: Vec::new(),
                }],
            }),
        )
        .await;
}

async fn complete(h: &Harness, thread: &Arc<CodexThread>, turn: &ExtensionData) {
    h.autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &ExtensionData::new("session"),
            thread_store: thread.thread_extension_data(),
            turn_store: turn,
        })
        .await;
}

#[test]
fn a_registered_roots_start_and_success_preserve_another_roots_attention() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            for key in ["wall", "limit", "goal"] {
                let h = Harness::new().await;
                h.immediate_point().await;
                let b = h
                    .test
                    .thread_manager
                    .start_thread(codex_core::StartThreadOptions {
                        environments: Some(Vec::new()),
                        ..codex_core::StartThreadOptions::new(h.test.config.clone())
                    })
                    .await
                    .expect("second loaded root");
                let b_rt = b
                    .thread
                    .thread_extension_data()
                    .get::<ThreadRt>()
                    .expect("second root runtime");
                for rt in [&b_rt, &h.rt] {
                    h.shared.observe(
                        &Frame {
                            primary_thread: Some(rt.key()),
                            displayed_thread: Some(rt.key()),
                            rollout_path: None,
                            cwd: h.test.cwd_path(),
                            model: "gpt-5.5",
                            codex_home: h.test.codex_home_path(),
                            embedded: true,
                            state: ProgramState::Idle,
                            draft_empty: true,
                            queued_input: false,
                            popup_open: false,
                            editor_open: false,
                            pending_user_turn: false,
                        },
                        now_ms(),
                    );
                }
                assert!(h.shared.is_root_thread(h.rt.key()));
                assert!(h.shared.is_root_thread(b_rt.key()));
                assert!(h.test.thread_manager.get_thread(b.thread_id).await.is_ok());
                h.fake
                    .set_attention_as("person", "the person's separate entry");
                let b_words = format!("{key}: root B still needs attention");
                h.autopilot
                    .sup
                    .attention_for(b_rt.key(), key, Some(b_words.clone()))
                    .await;
                h.autopilot
                    .sup
                    .attention_for(h.rt.key(), key, Some(format!("{key}: root A's old reason")))
                    .await;
                h.autopilot
                    .sup
                    .attention("zz-session", Some("session: an independent overlay".into()))
                    .await;
                let a_turn = ExtensionData::new("root-a-corrected");
                begin(&h, &h.test.codex, &a_turn).await;
                if key == "wall" {
                    assert_eq!(
                        h.autopilot.sup.standing_attention(),
                        Some(b_words.clone()),
                        "A's accepted correction clears only A's wall"
                    );
                }
                complete(&h, &h.test.codex, &a_turn).await;
                assert_eq!(
                    h.autopilot.sup.standing_attention(),
                    Some(b_words.clone()),
                    "A's success cannot clear B's {key}"
                );
                upgrade_handoff_tests::wait_for_supervisor(|| {
                    h.fake.attention().get("codex-alab") == Some(&b_words)
                })
                .await;

                // A session-scoped reason also survives root completion; it outranks
                // these root reasons but does not replace them in local custody.
                h.autopilot
                    .sup
                    .attention("aa-session", Some("session: sign-in overlay".into()))
                    .await;
                let another_a = ExtensionData::new("root-a-again");
                begin(&h, &h.test.codex, &another_a).await;
                complete(&h, &h.test.codex, &another_a).await;
                assert_eq!(
                    h.autopilot.sup.standing_attention().as_deref(),
                    Some("session: sign-in overlay")
                );
                h.autopilot.sup.attention("aa-session", None).await;
                assert_eq!(h.autopilot.sup.standing_attention(), Some(b_words));

                // Negative control: B's own accepted correction/success retires B.
                let b_turn = ExtensionData::new("root-b-corrected");
                begin(&h, &b.thread, &b_turn).await;
                complete(&h, &b.thread, &b_turn).await;
                assert_eq!(
                    h.autopilot.sup.standing_attention().as_deref(),
                    Some("session: an independent overlay")
                );
                h.autopilot
                    .sup
                    .attention("login", Some("login: a session sign-in overlay".into()))
                    .await;
                let overlay_a = ExtensionData::new("root-a-during-sign-in");
                begin(&h, &h.test.codex, &overlay_a).await;
                complete(&h, &h.test.codex, &overlay_a).await;
                assert_eq!(
                    h.autopilot.sup.standing_attention().as_deref(),
                    Some("login: a session sign-in overlay")
                );
                h.autopilot.sup.attention("login", None).await;
                if key == "wall" {
                    let (early, late) = if h.rt.key() < b_rt.key() {
                        (h.rt.key(), b_rt.key())
                    } else {
                        (b_rt.key(), h.rt.key())
                    };
                    h.autopilot
                        .sup
                        .attention_for(early, "z-root", Some("later reason on earlier root".into()))
                        .await;
                    h.autopilot
                        .sup
                        .attention_for(late, "a-root", Some("earlier reason on later root".into()))
                        .await;
                    assert_eq!(
                        h.autopilot.sup.standing_attention().as_deref(),
                        Some("earlier reason on later root"),
                        "reason priority precedes root identity"
                    );
                    h.autopilot.sup.attention_for(early, "z-root", None).await;
                    h.autopilot.sup.attention_for(late, "a-root", None).await;
                }
                assert_eq!(
                    h.fake.attention().get("person").map(String::as_str),
                    Some("the person's separate entry")
                );
                b.thread
                    .shutdown_and_wait()
                    .await
                    .expect("second root shutdown");
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("first root shutdown");
            }
        })
        .await
        .expect("cross-root attention hang detector");
    });
}

async fn another_root(h: &Harness) -> (Arc<CodexThread>, Arc<ThreadRt>) {
    let b = h
        .test
        .thread_manager
        .start_thread(codex_core::StartThreadOptions {
            environments: Some(Vec::new()),
            ..codex_core::StartThreadOptions::new(h.test.config.clone())
        })
        .await
        .expect("another loaded root");
    let rt = b
        .thread
        .thread_extension_data()
        .get::<ThreadRt>()
        .expect("root runtime");
    for key in [rt.key(), h.rt.key()] {
        h.shared.observe(
            &Frame {
                primary_thread: Some(key),
                displayed_thread: Some(key),
                rollout_path: None,
                cwd: h.test.cwd_path(),
                model: "gpt-5.5",
                codex_home: h.test.codex_home_path(),
                embedded: true,
                state: ProgramState::Idle,
                draft_empty: true,
                queued_input: false,
                popup_open: false,
                editor_open: false,
                pending_user_turn: false,
            },
            now_ms(),
        );
    }
    (b.thread, rt)
}

#[test]
fn a_resume_acquires_registration_before_its_native_bookkeeping_boundary() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.immediate_point().await;
            let (b, rt) = another_root(&h).await;
            h.autopilot.set_history(Vec::new());
            let (entered, native_boundary) = tokio::sync::oneshot::channel();
            let (release, released) = std::sync::mpsc::channel();
            *lock(&rt.before_resume_bookkeeping) = Some(Box::new(move || {
                let _ = entered.send(());
                released
                    .recv_timeout(Duration::from_secs(60))
                    .expect("resume boundary release");
            }));
            let scan =
                tokio::spawn(Arc::clone(&h.autopilot).resume_scan_owned(rt, false, SWEPT_WAIT));
            native_boundary
                .await
                .expect("actual native resume boundary");
            // A separate root's finish already takes registration before entering the
            // native boundary. Resume must hold the same outer fence: otherwise that
            // finish can own registration while this closure owns native generation,
            // and this closure's sync_record_owed will wait on the reverse edge.
            let registration_owned = matches!(
                h.autopilot.threads.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            );
            release.send(()).expect("release without a blocked worker");
            scan.await.expect("resume settles");
            b.shutdown_and_wait().await.expect("second root shutdown");
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("root shutdown");
            assert!(
                registration_owned,
                "resume entered native bookkeeping before acquiring registration"
            );
        })
        .await
        .expect("resume lock-order hang detector");
    });
}

#[test]
fn a_cancelled_handoff_acquires_registration_before_native_restoration() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.immediate_point().await;
            let (entered, native_boundary) = tokio::sync::oneshot::channel();
            let (release, released) = std::sync::mpsc::channel();
            *lock(&h.autopilot.before_upgrade_restore) = Some(Box::new(move || {
                let _ = entered.send(());
                released
                    .recv_timeout(Duration::from_secs(60))
                    .expect("cancellation boundary release");
            }));
            let handoff = UpgradeHandoff {
                autopilot: Arc::clone(&h.autopilot),
                owed: None,
            };
            let cancellation = tokio::spawn(async move { drop(handoff) });
            native_boundary
                .await
                .expect("actual native cancellation boundary");
            // Cancellation's final record publication also enters registration when
            // no act needs restoration. A concurrent native start takes the reverse
            // edge unless registration already surrounds this native boundary.
            let registration_owned = matches!(
                h.autopilot.threads.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            );
            release.send(()).expect("release cancellation worker");
            cancellation.await.expect("handoff cancellation settles");
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("root shutdown");
            assert!(
                registration_owned,
                "handoff cancellation entered native restoration before registration"
            );
        })
        .await
        .expect("cancellation lock-order hang detector");
    });
}

/// A TEST HOOK THAT BLOCKS LEAVES ITS WORKER'S NEXT TASK TO RUN (the full lib run's hang of
/// the retired-evaluator test below, 2026-10-24): a task spawned or woken from a worker waits in
/// that worker's LIFO slot, which no other worker steals, so a hook that blocks the worker until
/// that task runs never returns. [`run_hook`] hands the worker's queue to another thread first.
/// NEGATIVE CONTROL: the hook called directly hangs here until its detector fires.
#[test]
fn a_blocking_test_hook_leaves_its_workers_next_task_to_run() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        tokio::spawn(async {
            let (ran, seen) = std::sync::mpsc::channel();
            tokio::spawn(async move {
                let _ = ran.send(());
            });
            run_hook(Box::new(move || {
                seen.recv_timeout(Duration::from_secs(60))
                    .expect("the task spawned before the hook ran while it blocked");
            }));
        })
        .await
        .expect("the hook returns");
    });
}

#[test]
fn a_retired_idle_evaluators_off_epoch_cannot_clear_replacement_recovery() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            use codex_protocol::protocol::EventMsg;
            use core_test_support::responses;
            use core_test_support::wait_for_event;

            let h = Harness::new().await;
            h.immediate_point().await;
            let (entered, boundary) = tokio::sync::oneshot::channel();
            let (release, released) = std::sync::mpsc::channel();
            *lock(&h.rt.before_park) = Some(Box::new(move || {
                let _ = entered.send(());
                released
                    .recv_timeout(Duration::from_secs(60))
                    .expect("idle evaluation release");
            }));
            responses::mount_sse_once(
                &h._server,
                responses::sse(vec![
                    responses::ev_response_created("completed-task"),
                    responses::ev_assistant_message("reply", "There is independent work remaining"),
                    responses::ev_completed("completed-task"),
                ]),
            )
            .await;
            h.test
                .codex
                .start_turn_if_idle(TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "a real native task ending at idle".into(),
                    text_elements: Vec::new(),
                }]))
                .await
                .expect("native task starts");
            wait_for_event(&h.test.codex, |event| {
                matches!(event, EventMsg::TurnComplete(_))
            })
            .await;
            boundary.await.expect("native idle spawned its evaluation");
            let client = h.autopilot.sup.client().expect("claimed client");
            h.autopilot
                .sup
                .release_for_off(
                    &client,
                    "fixture Off".into(),
                    codex_alab_core::session::HarnessWord::Off,
                )
                .await;
            assert_ne!(h.rt.off_seen.load(Ordering::SeqCst), h.autopilot.sup.offs());
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("retire old native root");

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
            assert_eq!(
                fresh.off_seen.load(Ordering::SeqCst),
                h.autopilot.sup.offs()
            );
            let recovery = codex_alab_core::session::Recovery {
                thread: fresh.key().into(),
                turn_id: "replacement-failure".into(),
                point_at_ms: now_ms(),
                not_before_ms: now_ms() + 60_000,
                by_autopilot: true,
                by_person: false,
                wall: codex_alab_core::session::RecoveryWall::Network {
                    what: "new root's recovery".into(),
                    measured: false,
                },
            };
            h.shared.set_recovery(Some(recovery.clone()));
            release
                .send(())
                .expect("resume detached old idle evaluator");
            let _settled = h.rt.hold_eval_when_free().await;
            let after_old_callback = h.shared.recovery();
            h.autopilot
                .on_thread_stop(ThreadStopInput {
                    session_store: &ExtensionData::new("session"),
                    thread_store: &replacement,
                })
                .await;
            assert_eq!(
                after_old_callback,
                Some(recovery),
                "retired park must preserve replacement recovery"
            );
        })
        .await
        .expect("retired idle evaluation hang detector");
    });
}

#[test]
fn stopping_a_root_retires_its_reasons_and_revokes_prepared_writers() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            for signal in [None, Some(libc::SIGHUP), Some(libc::SIGTERM)] {
                let h = Harness::new().await;
                h.immediate_point().await;
                let (b, rt) = another_root(&h).await;
                // Project a sole carry-on onto the actual loaded runtime; its stop
                // must retire that custody, while a relaunchable signal retains it.
                rt.with_state(|state| state.observe_resume(true, None, true));
                h.autopilot.sync_owed();
                assert!(h.shared.carry_on_owed());
                h.fake.set_attention_as("person", "keep the person's entry");
                h.autopilot
                    .attention_for(&h.rt, "wall", Some("wall: A remains unresolved".into()))
                    .await;
                h.autopilot
                    .sup
                    .attention("zz-session", Some("session: keep this overlay".into()))
                    .await;
                for key in [
                    "api",
                    "context",
                    "goal",
                    "limit",
                    "login",
                    "plan",
                    "question",
                    "turn",
                    "wall",
                    "future-kind",
                ] {
                    h.autopilot
                        .attention_for(&rt, key, Some(format!("{key}: B's unresolved reason")))
                        .await;
                }
                let prepared = h.autopilot.attention_for(
                    &rt,
                    "api",
                    Some("api: an old prepared callback".into()),
                );
                if let Some(signal) = signal {
                    assert!(h.shared.set_signal(signal));
                }
                b.shutdown_and_wait().await.expect("genuine root shutdown");
                assert_eq!(h.shared.carry_on_owed(), signal.is_some());
                if signal.is_none() {
                    assert_eq!(
                        h.autopilot.sup.standing_attention().as_deref(),
                        Some("wall: A remains unresolved"),
                        "closed B must release all its scoped reasons"
                    );
                } else {
                    assert_eq!(
                        h.autopilot.sup.standing_attention().as_deref(),
                        Some("api: B's unresolved reason"),
                        "signal relaunch retains existing custody"
                    );
                }
                prepared.await;
                assert_ne!(
                    h.autopilot.sup.standing_attention().as_deref(),
                    Some("api: an old prepared callback"),
                    "a stopped callback cannot publish again"
                );
                if signal.is_none() {
                    h.autopilot.attention_for(&h.rt, "wall", None).await;
                    assert_eq!(
                        h.autopilot.sup.standing_attention().as_deref(),
                        Some("session: keep this overlay")
                    );
                }
                assert_eq!(
                    h.fake.attention().get("person").map(String::as_str),
                    Some("keep the person's entry")
                );
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("remaining root shutdown");
            }
        })
        .await
        .expect("root stop hang detector");
    });
}

/// A ROOT'S OWN NEED STAYS WITH IT, AND IS ONE NEED TO A PERSON (parity-5, N): the quota's
/// need, a need of the `owner=needs` entry, is kept with the root that raised it, as that
/// root's reasons are. Two roots at one account's quota show it once, never `(+1 more)` of the
/// same words; another root's turn past its wall leaves it; its own root's stop (no relaunch)
/// takes it down, and its own root's turn that ends with no wall does too (the negative control
/// of the first: the same retire, from the root that raised it).
#[test]
fn a_roots_own_need_stays_with_it_and_is_one_need_to_a_person() {
    use crate::decide::QUOTA_NEED;
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let quota = format!("Codex needs you: {QUOTA_NEED} \u{2014} carrying on");
            let needs = |h: &Harness| h.fake.attention().get("needs").cloned();
            let h = Harness::new().await;
            h.immediate_point().await;
            let (b, b_rt) = another_root(&h).await;
            for rt in [&h.rt, &b_rt] {
                h.autopilot
                    .need_for(
                        rt,
                        &need_authority(&h, rt),
                        "limit",
                        Some(QUOTA_NEED.to_string()),
                    )
                    .await;
            }
            upgrade_handoff_tests::wait_for_supervisor(|| needs(&h).is_some()).await;
            assert_eq!(needs(&h), Some(quota.clone()), "one need, said once");

            // A's turn gets past the wall: B's need stands.
            let a_turn = ExtensionData::new("root-a-past-the-wall");
            begin(&h, &h.test.codex, &a_turn).await;
            complete(&h, &h.test.codex, &a_turn).await;
            h.autopilot.sup.show_needs().await;
            assert_eq!(
                needs(&h),
                Some(quota.clone()),
                "A's success cannot clear B's need"
            );

            // B stops: its need goes with its custody, and the journal says why.
            b.shutdown_and_wait().await.expect("second root shutdown");
            upgrade_handoff_tests::wait_for_supervisor(|| needs(&h).is_none()).await;
            let why = format!("CLEARED needs ({})", crate::supervisor::NEEDS_OWN_PAST);
            assert!(
                h.autopilot.sup.journal().iter().any(|l| l.contains(&why)),
                "{:?}",
                h.autopilot.sup.journal()
            );

            // NEGATIVE CONTROL: A's own turn past the wall takes A's need down.
            h.autopilot
                .need_for(
                    &h.rt,
                    &need_authority(&h, &h.rt),
                    "limit",
                    Some(QUOTA_NEED.to_string()),
                )
                .await;
            upgrade_handoff_tests::wait_for_supervisor(|| needs(&h) == Some(quota.clone())).await;
            let again = ExtensionData::new("root-a-past-the-wall-again");
            begin(&h, &h.test.codex, &again).await;
            complete(&h, &h.test.codex, &again).await;
            upgrade_handoff_tests::wait_for_supervisor(|| needs(&h).is_none()).await;
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("first root shutdown");
        })
        .await
        .expect("own need custody hang detector");
    });
}

/// A STOPPED ROOT'S LATE NEED IS REFUSED (parity-5 review, R4): the quota's need goes up only
/// under the registration fence its root's `attention` reasons take (`Autopilot::need_for`,
/// `with_current_runtime`), so an evaluator that outlived its root's stop raises nothing: its
/// stop already retired what that root held, and no later turn of it could take a late one
/// down. RED with the fence removed: the next needs set, the live root's worker need, came up
/// behind the stopped root's quota need (`(+1 more)`). NEGATIVE CONTROL: the live root's own
/// raise goes up.
#[test]
fn a_stopped_roots_late_need_is_refused() {
    use crate::decide::QUOTA_NEED;
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let needs = |h: &Harness| h.fake.attention().get("needs").cloned();
            let h = Harness::new().await;
            h.immediate_point().await;
            let (b, b_rt) = another_root(&h).await;
            b.shutdown_and_wait().await.expect("second root shutdown");
            h.autopilot
                .need_for(
                    &b_rt,
                    &need_authority(&h, &b_rt),
                    "limit",
                    Some(QUOTA_NEED.to_string()),
                )
                .await;
            h.autopilot
                .sup
                .needs(&["the signing key password".to_string()], 7)
                .await;
            upgrade_handoff_tests::wait_for_supervisor(|| needs(&h).is_some()).await;
            assert_eq!(
                needs(&h).as_deref(),
                Some("Codex needs you: the signing key password \u{2014} carrying on"),
                "{:?}",
                h.autopilot.sup.journal()
            );

            // NEGATIVE CONTROL: the live root's raise goes up, first.
            h.autopilot
                .need_for(
                    &h.rt,
                    &need_authority(&h, &h.rt),
                    "limit",
                    Some(QUOTA_NEED.to_string()),
                )
                .await;
            let both = format!("Codex needs you: {QUOTA_NEED} (+1 more) \u{2014} carrying on");
            upgrade_handoff_tests::wait_for_supervisor(|| needs(&h) == Some(both.clone())).await;
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("first root shutdown");
        })
        .await
        .expect("stopped root need hang detector");
    });
}

#[test]
fn replacement_drops_the_retired_runtimes_sole_carry_on() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.immediate_point().await;
            let (b, old) = another_root(&h).await;
            // This is a lifecycle control. The native resume/Writer regression
            // separately proves how this pending carry-on is restored.
            old.with_state(|state| state.observe_resume(true, None, true));
            h.autopilot.sync_owed();
            assert!(h.shared.carry_on_owed());
            let replacement = ExtensionData::new(old.key());
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
            assert!(!h.shared.carry_on_owed());
            h.autopilot
                .on_thread_stop(ThreadStopInput {
                    session_store: &ExtensionData::new("session"),
                    thread_store: &replacement,
                })
                .await;
            b.shutdown_and_wait().await.expect("old root shutdown");
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("root shutdown");
        })
        .await
        .expect("replacement carry-on hang detector");
    });
}

#[test]
fn reloaded_root_identity_fences_old_starts_completions_stops_and_writers() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.immediate_point().await;
            let (b, old) = another_root(&h).await;
            let old_turn = ExtensionData::new("old-instance-turn");
            begin(&h, &b, &old_turn).await;
            h.autopilot
                .attention_for(&old, "api", Some("api: the old instance".into()))
                .await;
            let replacement = ExtensionData::new(old.key());
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
            assert!(
                h.autopilot.sup.standing_attention().is_none(),
                "takeover retires old scoped kinds before the replacement can publish"
            );
            let fresh_turn = ExtensionData::new("fresh-instance-turn");
            let mode = b.thread_settings_snapshot().await.collaboration_mode;
            h.autopilot
                .on_turn_start(TurnStartInput {
                    turn_id: fresh_turn.level_id(),
                    collaboration_mode: &mode,
                    token_usage_at_turn_start: None,
                    session_store: &ExtensionData::new("session"),
                    thread_store: &replacement,
                    turn_store: &fresh_turn,
                })
                .await;
            h.autopilot
                .attention_for(&fresh, "wall", Some("wall: the fresh instance".into()))
                .await;
            let recovery = codex_alab_core::session::Recovery {
                thread: fresh.key().into(),
                turn_id: fresh_turn.level_id().into(),
                point_at_ms: now_ms(),
                not_before_ms: now_ms() + 60_000,
                by_autopilot: true,
                by_person: false,
                wall: codex_alab_core::session::RecoveryWall::Network {
                    what: "fresh failure".into(),
                    measured: false,
                },
            };
            h.shared.set_recovery(Some(recovery.clone()));
            // A retired runtime cannot mutate a replacement even if an old callback
            // carries its exact current native ID.
            complete(&h, &b, &fresh_turn).await;
            begin(&h, &b, &ExtensionData::new("late-old-start")).await;
            b.shutdown_and_wait().await.expect("old instance shutdown");
            h.autopilot.attention_for(&old, "wall", None).await;
            h.autopilot
                .attention_for(&old, "api", Some("api: stale writer".into()))
                .await;
            assert!(
                h.shared.turn_running_on(fresh.key()),
                "old callbacks cannot end the replacement's running turn"
            );
            assert_eq!(h.shared.recovery(), Some(recovery));
            assert_eq!(
                h.autopilot.sup.standing_attention().as_deref(),
                Some("wall: the fresh instance")
            );
            assert!(h.shared.last_turn(fresh.key()).is_none());
            assert!(
                h.autopilot
                    .thread_rt(fresh.key())
                    .is_some_and(|rt| Arc::ptr_eq(&rt, &fresh))
            );
            h.autopilot
                .on_thread_stop(ThreadStopInput {
                    session_store: &ExtensionData::new("session"),
                    thread_store: &replacement,
                })
                .await;
            assert!(!h.shared.turn_running_on(fresh.key()));
            assert!(h.autopilot.sup.standing_attention().is_none());
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("other root shutdown");
        })
        .await
        .expect("runtime takeover hang detector");
    });
}

#[test]
fn a_retired_evaluators_act_ack_preserves_replacement_recovery_and_pace() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.fake.set_policy_key("limit_wait", "true");
            upgrade_handoff_tests::wait_for_supervisor(|| {
                h.autopilot
                    .sup
                    .policy()
                    .is_some_and(|policy| policy.limit_wait)
            })
            .await;
            h.immediate_point().await;
            h.hold_model_response().await;
            let at = clock() - Duration::from_secs(3600);
            h.rt.with_state(|state| {
                state.observe_turn_end(
                    TurnEnd {
                        at,
                        by_autopilot: true,
                        by_person: false,
                        text: None,
                        earlier: None,
                        questions: false,
                        worked: Duration::from_secs(600),
                        wall: Some(Wall::Usage {
                            reset_at: Some(at),
                            reset_label: None,
                        }),
                        aborted: false,
                        ran_tools: false,
                        restored: false,
                    },
                    &h.autopilot.timing,
                )
            });
            h.fake.gate("meta set harness_act");
            let old_eval = tokio::spawn(Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)));
            upgrade_handoff_tests::wait_for_supervisor(|| !h.fake.gated_requests().is_empty())
                .await;
            assert!(h.shared.turn_running_on(h.rt.key()));

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
            let fresh_turn = ExtensionData::new("replacement-native-turn");
            let mode = h
                .test
                .codex
                .thread_settings_snapshot()
                .await
                .collaboration_mode;
            h.autopilot
                .on_turn_start(TurnStartInput {
                    turn_id: fresh_turn.level_id(),
                    collaboration_mode: &mode,
                    token_usage_at_turn_start: None,
                    session_store: &ExtensionData::new("session"),
                    thread_store: &replacement,
                    turn_store: &fresh_turn,
                })
                .await;
            let recovery = codex_alab_core::session::Recovery {
                thread: fresh.key().into(),
                turn_id: fresh_turn.level_id().into(),
                point_at_ms: now_ms(),
                not_before_ms: now_ms() + 60_000,
                by_autopilot: true,
                by_person: false,
                wall: codex_alab_core::session::RecoveryWall::Network {
                    what: "fresh failure".into(),
                    measured: false,
                },
            };
            fresh.with_state(|state| state.short_streak = 7);
            h.autopilot.sync_pace(&fresh);
            let pace = h.shared.pace(fresh.key());
            h.shared.set_recovery(Some(recovery.clone()));
            h.fake.open_gate("meta set harness_act");
            old_eval.await.expect("old evaluator settles");
            assert_eq!(
                (h.shared.recovery(), h.shared.pace(fresh.key())),
                (Some(recovery), pace),
                "retired evaluator must preserve fresh recovery and pace"
            );
            assert_eq!(h.fake.acts(), vec![words::RULE_LIMIT_RESUME.to_string()]);
            h.autopilot
                .on_thread_stop(ThreadStopInput {
                    session_store: &ExtensionData::new("session"),
                    thread_store: &replacement,
                })
                .await;
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("retired evaluator hang detector");
    });
}
