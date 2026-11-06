// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE LIVE UPGRADE ASKS FIRST UNDER `upgrade_ask` (CONTRACT §5 row "live upgrade", amended
//! 2026-10-28 after the parity sweep, gap 27), through the real evaluator, a loaded native core
//! thread and the fake aterm: off, the due upgrade moves with no notice typed; on, the due
//! upgrade types one readiness notice, and the re-exec is asked for only once the agent's
//! answer to it says ready. And the runtime's wiring of the round (review of gap 27, findings
//! 1, 2, 3, 6, 7, 9 and 10): what each of the root thread's turns says to it, a notice whose
//! turn does not start given back, a quiet session moved with no notice, a thread at rest
//! woken for a due notice, the clock held at a wall and the host's sleep counted, and a round
//! that cannot be read holding the re-exec.

use super::*;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::UserMessageItem;

const HANG: Duration = Duration::from_secs(60);

fn notices(h: &Harness) -> usize {
    h.fake
        .acts()
        .iter()
        .filter(|act| *act == words::RULE_UPGRADE_NOTICE)
        .count()
}

/// The agent's answer to the notice: the model's one reply, then nothing more answers.
async fn answer(h: &Harness, text: &str) {
    responses::mount_sse_once(
        &h._server,
        responses::sse(vec![
            responses::ev_response_created("notice-answer"),
            responses::ev_assistant_message("notice-answer-message", text),
            responses::ev_completed("notice-answer"),
        ]),
    )
    .await;
    h.hold_model_response().await;
}

/// The notice's turn ended with `reply` and its end was heard: `finish` books the session's
/// last turn after the round heard the answer.
async fn notice_answered(h: &Harness, reply: &str) {
    wait_for_supervisor(|| {
        h.shared
            .last_turn(h.rt.key())
            .is_some_and(|turn| turn.last_agent_message.as_deref() == Some(reply))
    })
    .await;
}

async fn idle(h: &Harness) {
    let session = codex_extension_api::ExtensionData::new("session");
    h.autopilot
        .on_thread_idle(ThreadIdleInput {
            cause: codex_extension_api::ThreadIdleCause::Completed,
            session_store: &session,
            thread_store: h.test.codex.thread_extension_data(),
        })
        .await;
}

/// NEGATIVE CONTROL for the tests below: `upgrade_ask = false` (the fake's default) moves the
/// due upgrade with no notice typed.
#[test]
fn without_upgrade_ask_the_due_upgrade_types_no_notice() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let mut h = Harness::new().await;
            assert!(!h.autopilot.sup.policy().expect("proven policy").upgrade_ask);
            h.immediate_point().await;
            idle(&h).await;
            let request = h.requests.recv().await.expect("conditional upgrade");
            assert_eq!(notices(&h), 0, "{:?}", h.fake.acts());
            assert!(!h.shared.turn_running_on(h.rt.key()));
            assert!(request.decide(true));
            wait_for_supervisor(|| h.shared.exec_pending()).await;
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("upgrade_ask off hang detector");
    });
}

/// `upgrade_ask = true`: the due upgrade types one readiness notice into the root thread on
/// screen, carrying its reference; an answer that does not say ready leaves the re-exec waiting
/// on the agent (the upgrade's check says why); one that says ready lets it go, its notice the
/// only one typed.
#[test]
fn upgrade_ask_types_one_notice_and_re_execs_only_on_a_ready_answer() {
    for (reply, ready) in [
        ("Not yet: the release build is still running.", false),
        ("Ready for the restart.", true),
    ] {
        native_test(move || async move {
            tokio::time::timeout(HANG, async {
                let mut h = Harness::new().await;
                h.fake.set_policy_key("upgrade_ask", "true");
                wait_for_supervisor(|| {
                    h.autopilot
                        .sup
                        .policy()
                        .is_some_and(|policy| policy.upgrade_ask && policy.upgrade)
                })
                .await;
                assert!(h.shared.is_root_thread(h.rt.key()) && h.shared.displays(h.rt.key()));
                h.immediate_point().await;
                answer(&h, reply).await;
                idle(&h).await;
                wait_for_supervisor(|| notices(&h) == 1).await;
                notice_answered(&h, reply).await;
                if ready {
                    let request = h.requests.recv().await.expect("conditional upgrade");
                    assert!(lock(&h.autopilot.ask).ready());
                    assert_eq!(notices(&h), 1, "{:?}", h.fake.acts());
                    assert!(request.decide(true));
                    wait_for_supervisor(|| h.shared.exec_pending()).await;
                } else {
                    let (asks, said_ready) = {
                        let round = lock(&h.autopilot.ask);
                        (round.asks(), round.ready())
                    };
                    assert_eq!((asks, said_ready), (1, false));
                    let facts = h.autopilot.upgrade_facts().await;
                    assert!(
                        matches!(
                            upgrade::check(&facts),
                            UpgradeCheck::Wait(why) if why.starts_with("upgrade_ask: waiting for the agent")
                        ),
                        "{facts:?}"
                    );
                    assert!(h.requests.try_recv().is_err(), "{reply}");
                    Autopilot::cancel_timer(&h.rt).await;
                }
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("native shutdown");
            })
            .await
            .expect("upgrade_ask native hang detector");
        });
    }
}

/// `upgrade_ask = true` read by the policy.
async fn asking(h: &Harness) {
    h.fake.set_policy_key("upgrade_ask", "true");
    wait_for_supervisor(|| {
        h.autopilot
            .sup
            .policy()
            .is_some_and(|policy| policy.upgrade_ask && policy.upgrade)
    })
    .await;
}

/// The round's notice typed into the root thread, as codex-alab's act books it: the round
/// counts it and the thread's last act is the notice. Its reference.
fn notice_typed(h: &Harness) -> String {
    let marker = {
        let mut round = lock(&h.autopilot.ask);
        round.aim("/new-build");
        let marker = round.offer(h.rt.key());
        round.noticed(
            &words::upgrade_notice("/new-build", "0.1.0", &marker),
            clock(),
        );
        marker
    };
    h.rt.with_state(|s| s.last_rule = Some(words::RULE_UPGRADE_NOTICE));
    marker
}

/// ONE TURN OF THE ROOT THREAD THROUGH THE EXTENSION'S OWN HOOKS, no model behind it (the
/// round's hearing is the runtime's, whatever the policy): opened by codex-alab (`ours`, its
/// client id) or a person, ended by the reply `reply`, or at the wall `error` names.
async fn hooked(h: &Harness, id: &str, ours: bool, reply: &str, error: Option<CodexErrorInfo>) {
    let thread = h.test.codex.thread_extension_data();
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new(id);
    let mode = h
        .test
        .codex
        .thread_settings_snapshot()
        .await
        .collaboration_mode;
    h.autopilot
        .on_turn_start(TurnStartInput {
            turn_id: turn.level_id(),
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &session,
            thread_store: thread,
            turn_store: &turn,
        })
        .await;
    h.autopilot
        .on_item_completed(
            thread,
            &turn,
            &TurnItem::UserMessage(UserMessageItem {
                id: format!("{id}-input"),
                client_id: ours.then(|| format!("{CLIENT_PREFIX}{id}")),
                content: vec![UserInput::Text {
                    text: format!("{id}'s input"),
                    text_elements: Vec::new(),
                }],
            }),
        )
        .await;
    match error {
        Some(error) => {
            h.autopilot
                .on_turn_error(TurnErrorInput {
                    turn_id: turn.level_id(),
                    error,
                    error_details: &CodexErrorDetails::Fatal(format!("{id}'s wall")),
                    session_store: &session,
                    thread_store: thread,
                    turn_store: &turn,
                })
                .await;
        }
        None => {
            h.autopilot
                .on_item_completed(
                    thread,
                    &turn,
                    &TurnItem::AgentMessage(AgentMessageItem {
                        id: format!("{id}-reply"),
                        content: vec![AgentMessageContent::Text {
                            text: reply.to_string(),
                        }],
                        phase: Some(MessagePhase::FinalAnswer),
                        memory_citation: None,
                        delivery: None,
                        questions: None,
                    }),
                )
                .await;
        }
    }
    h.autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &session,
            thread_store: thread,
            turn_store: &turn,
        })
        .await;
}

fn ready(h: &Harness) -> bool {
    lock(&h.autopilot.ask).ready()
}

fn step_now(h: &Harness) -> AskStep {
    lock(&h.autopilot.ask).step(clock(), false)
}

/// CODEX-ALAB'S OWN ACT SUPERSEDES NOTHING (review of gap 27, finding 1; aterm's
/// `is_direction` passes over native automatic turns): a continuation typed after a ready
/// answer (a precondition held the re-exec) leaves the round asking nothing at once, and the
/// agent's reply to it answers the notice still, in ordinary language. NEGATIVE CONTROL (the
/// mutant run): the round superseded at every turn's start asks again at once, four notices in
/// four turns.
#[test]
fn codex_alabs_own_act_after_ready_supersedes_nothing_and_its_reply_answers() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let h = Harness::new().await;
            notice_typed(&h);
            hooked(&h, "notice", true, "Ready for the restart.", None).await;
            assert!(ready(&h));
            h.rt.with_state(|s| s.last_rule = Some(words::RULE_CONTINUE));
            hooked(
                &h,
                "continuation",
                true,
                "Continuing: the tests pass.",
                None,
            )
            .await;
            assert!(!ready(&h), "the latest reply did not say ready");
            assert!(
                matches!(step_now(&h), AskStep::Wait { .. }),
                "no re-ask before the notice's half hour"
            );
            hooked(
                &h,
                "continuation-2",
                true,
                "I'm ready to restart now.",
                None,
            )
            .await;
            assert!(
                ready(&h),
                "the reply to codex-alab's own act answers the notice"
            );
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("own act hang detector");
    });
}

/// A PERSON'S MESSAGE SUPERSEDES A READINESS (aterm's `ready_superseded`; review of gap 27,
/// finding 9): it takes the agent's ready answer back, the round asks again at once, and after
/// it the agent's ordinary words are no answer, its reference alone line still is. NEGATIVE
/// CONTROL (the mutant run): with the message heard as no direction the readiness stands.
#[test]
fn a_persons_message_after_ready_supersedes_it_and_the_round_asks_again() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let h = Harness::new().await;
            let marker = notice_typed(&h);
            hooked(&h, "notice", true, "Ready for the restart.", None).await;
            assert!(ready(&h));
            hooked(&h, "person", false, "Ready for the restart.", None).await;
            assert!(
                !ready(&h),
                "after a person's message ordinary words are no answer"
            );
            assert_eq!(
                step_now(&h),
                AskStep::Notice,
                "the round asks again at once"
            );
            hooked(&h, "person-2", false, &format!("Done.\n{marker}"), None).await;
            assert!(ready(&h), "the reference alone is readiness after it too");
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("person's message hang detector");
    });
}

/// A NOTICE MET BY A LOGIN WALL IS GIVEN BACK; ONE MET BY A SERVER'S IS NOT (aterm's
/// `notice_scan`; review of gap 27, findings 3 and 9): the login wall's notice never reached
/// the agent, so it is typed again at once, its ask uncounted; the server wall's stands, and the
/// agent's reply to codex-alab's try answers it. NEGATIVE CONTROLS (the mutant runs): every
/// wall giving the notice back, and none.
#[test]
fn a_login_walls_notice_is_given_back_and_a_server_walls_stands() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let h = Harness::new().await;
            notice_typed(&h);
            hooked(&h, "login", true, "", Some(CodexErrorInfo::Unauthorized)).await;
            assert_eq!(lock(&h.autopilot.ask).asks(), 0);
            assert_eq!(step_now(&h), AskStep::Notice, "typed again at once");
            notice_typed(&h);
            hooked(
                &h,
                "server",
                true,
                "",
                Some(CodexErrorInfo::ServerOverloaded),
            )
            .await;
            assert_eq!(lock(&h.autopilot.ask).asks(), 1, "the notice stands");
            assert!(!ready(&h));
            h.rt.with_state(|s| s.last_rule = Some(words::RULE_API_RETRY));
            hooked(&h, "retry", true, "I'm ready to restart now.", None).await;
            assert!(
                ready(&h),
                "the reply to codex-alab's try answers the notice"
            );
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("walled notice hang detector");
    });
}

/// A NOTICE WHOSE TURN DOES NOT START IS GIVEN BACK (review of gap 27, finding 9): booked as it
/// goes, before native core starts its turn, it is withdrawn when core refuses the turn (a
/// person's message took the thread between the act's checks and its start). NEGATIVE CONTROL
/// (the mutant run): with no withdrawal the refused notice stays counted.
#[test]
fn a_notice_whose_turn_does_not_start_is_given_back() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let h = Harness::new().await;
            asking(&h).await;
            let (entered, at_start) = tokio::sync::oneshot::channel();
            let (release, released) = tokio::sync::oneshot::channel();
            *lock(&h.rt.before_start) = Some((entered, released));
            h.hold_model_response().await;
            h.immediate_point().await;
            idle(&h).await;
            at_start.await.expect("the notice reaches its start");
            assert_eq!(lock(&h.autopilot.ask).asks(), 1, "booked as it goes");
            h.test
                .codex
                .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "The person's own message.".into(),
                    text_elements: Vec::new(),
                }]))
                .await
                .expect("the person's message is admitted");
            wait_for_supervisor(|| h.shared.turn_running_on(h.rt.key())).await;
            release.send(()).expect("the notice's start resumes");
            wait_for_supervisor(|| lock(&h.autopilot.ask).asks() == 0).await;
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("refused notice hang detector");
    });
}

/// What leaves a session with no work in hand, set on its state.
type Quiet = fn(&mut ThreadState);

/// A QUIET SESSION MOVES WITH NO NOTICE (aterm's cooperative mode, its `Facts::quiet`; review
/// of gap 27, findings 2 and 10): with no task, or with its point left to a person's own
/// exchange (where no notice is typed: under Land, the session's opening one; Cruise leaves
/// none, gap 32), the re-exec waits on no readiness, and goes at idle with none typed. NEGATIVE
/// CONTROL: a task in hand at a clean point waits on the notice.
#[test]
fn a_quiet_session_moves_with_no_notice_under_upgrade_ask() {
    let quiet: [(&str, Option<&str>, Quiet); 2] = [
        ("no task", None, |s| s.task = false),
        ("left to a person", Some("land"), |s| {
            // Land's: the exchange that opened the session (edges-4: Land takes back any
            // other an Off left).
            s.exchange = true;
            s.exchange_opens = true;
            s.left = true;
        }),
    ];
    for (what, word, quiet) in quiet {
        native_test(move || async move {
            tokio::time::timeout(HANG, async {
                let mut h = Harness::new().await;
                asking(&h).await;
                let facts = h.autopilot.upgrade_facts().await;
                assert!(
                    facts
                        .unconfirmed
                        .as_deref()
                        .is_some_and(|why| why.contains("(1 of 4)")),
                    "{what}: {facts:?}"
                );
                h.rt.with_state(quiet);
                // The word the quiet state stands under, read once it stands.
                if let Some(word) = word {
                    let want = crate::decide::Mode::of_word(word);
                    h.fake.set_status("autopilot", word);
                    h.fake
                        .set_policy_key("answer_text", &format!("Pick%20one%20{word}."));
                    wait_for_supervisor(|| {
                        h.autopilot
                            .sup
                            .policy()
                            .is_some_and(|policy| Some(policy.autopilot) == want)
                    })
                    .await;
                }
                let facts = h.autopilot.upgrade_facts().await;
                assert_eq!(facts.unconfirmed, None, "{what}");
                h.immediate_point().await;
                idle(&h).await;
                let request = h.requests.recv().await.expect("conditional upgrade");
                assert_eq!(notices(&h), 0, "{what}: {:?}", h.fake.acts());
                assert!(request.decide(true));
                wait_for_supervisor(|| h.shared.exec_pending()).await;
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("native shutdown");
            })
            .await
            .expect("quiet upgrade hang detector");
        });
    }
}

/// A DUE NOTICE WAKES THE THREAD AT REST (aterm's host types its notice into an idle tab;
/// review of gap 27, finding 10): the upgrade's own look (the minute monitor's, a launch path's
/// change) at a point already decided has the root thread look again, and its decision types the
/// notice. NEGATIVE CONTROL (the mutant run): with no wake the point's own timer, minutes away,
/// is all that would look.
#[test]
fn a_due_notice_wakes_the_thread_at_rest() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let h = Harness::new().await;
            asking(&h).await;
            assert!(
                h.rt.timer_pending(),
                "the point is decided, its act minutes away"
            );
            answer(&h, "Not yet: the release build is still running.").await;
            Arc::clone(&h.autopilot).consider_upgrade().await;
            wait_for_supervisor(|| notices(&h) == 1).await;
            Autopilot::cancel_timer(&h.rt).await;
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("wake hang detector");
    });
}

/// THE ROUND COUNTS THE HOST'S SLEEP, AND ITS CLOCK IS HELD AT A WALL (aterm's drive reads unix
/// seconds, and its `clock_held`; review of gap 27, findings 6 and 10): a half hour the host
/// slept through brings the next notice due at the wake; a sleep while the root thread's point
/// stands at a wall brings none, the clock held there. NEGATIVE CONTROLS (the mutant runs): no
/// sleep counted, and no clock held.
#[test]
fn the_rounds_clock_counts_the_hosts_sleep_and_is_held_at_a_wall() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let h = Harness::new().await;
            asking(&h).await;
            let asleep = Arc::new(AtomicU64::new(1000));
            let read = Arc::clone(&asleep);
            h.autopilot.set_wake_read(Arc::new(move || {
                Some(codex_alab_core::wake::Reading {
                    asleep: Duration::from_secs(read.load(Ordering::SeqCst)),
                    last: None,
                })
            }));
            let policy = h.autopilot.sup.policy().expect("proven policy");
            let (key, _) = h
                .autopilot
                .ask_target(&policy)
                .expect("the launch path names another build");
            let book = || {
                let mut round = lock(&h.autopilot.ask);
                round.aim(&key);
                let marker = round.offer(h.rt.key());
                round.noticed(
                    &words::upgrade_notice("/new-build", "0.1.0", &marker),
                    clock(),
                );
            };
            let why = || async {
                h.autopilot
                    .upgrade_facts()
                    .await
                    .unconfirmed
                    .unwrap_or_default()
            };
            assert!(why().await.contains("(1 of 4) goes in"));
            book();
            assert!(
                why().await.starts_with("waiting for the agent"),
                "{}",
                why().await
            );
            asleep.fetch_add(8 * 3600, Ordering::SeqCst);
            assert!(why().await.contains("(2 of 4) goes in"), "the sleep counts");
            book();
            assert!(!h.autopilot.ask_held());
            h.rt.with_state(|s| {
                if let Some(end) = s.point.as_mut() {
                    end.wall = Some(Wall::Login);
                }
            });
            assert!(h.autopilot.ask_held());
            asleep.fetch_add(8 * 3600, Ordering::SeqCst);
            let held = why().await;
            assert!(
                held.starts_with("waiting for the agent") && held.contains("(notice 2 of 4)"),
                "{held}"
            );
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("sleep and held clock hang detector");
    });
}

/// THE ROUND FAILS CLOSED (review of gap 27, finding 7): where the launch path, read again, no
/// longer names another build (atpkg rolled it back, or its read failed) while the facts the
/// check reads still say another, or the policy is gone, the facts hold the re-exec on the
/// round's account rather than let it go with no readiness.
#[test]
fn the_round_fails_closed_where_it_cannot_be_read() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let h = Harness::new().await;
            asking(&h).await;
            let launch = h.shared.launch().expect("launch").clone();
            let old = launch.current_exe.clone().expect("running build");
            let new = launch.argv0.clone().expect("launch path");
            std::fs::remove_file(&new).expect("the new build's file");
            std::fs::hard_link(&old, &new).expect("the launch path names the running build");
            assert!(!codex_alab_tui::upgrade::launch_names_other_build(&launch));
            let facts = h.autopilot.upgrade_facts().await;
            assert!(facts.other_build);
            let held = matches!(
                upgrade::check(&facts),
                UpgradeCheck::Wait(why) if why.starts_with("upgrade_ask:")
            );
            assert!(held, "{facts:?}");
            h.fake.set_effective(false, "autopilot-off");
            wait_for_supervisor(|| h.autopilot.sup.policy().is_none()).await;
            let facts = h.autopilot.upgrade_facts().await;
            assert!(facts.unconfirmed.is_some(), "{facts:?}");
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("fail closed hang detector");
    });
}
