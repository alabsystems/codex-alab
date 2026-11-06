// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The per-thread timers, under tokio's paused clock: one pending timer per thread, fired
//! at its deadline, replaced by a newer one, and cancelled (and joined) when a turn starts
//! and when the thread stops.

use std::sync::Arc;
use std::time::Duration;

use codex_alab_aterm::Options as ClientOptions;
use codex_alab_core::session::Shared;
use codex_extension_api::ExtensionData;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStopInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStartInput;
use codex_protocol::ThreadId;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;

use crate::Autopilot;
use crate::Supervisor;
use crate::SupervisorOptions;
use crate::decide::Timing;
use crate::extension::ThreadRt;
use crate::supervisor::clock;

const MIN: Duration = Duration::from_secs(60);

fn autopilot() -> Arc<Autopilot> {
    codex_alab_core::activate();
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(|| None),
        client: ClientOptions::default(),
    });
    Autopilot::new(sup, None, None, Timing::default(), |_| true)
}

fn thread() -> (ExtensionData, Arc<ThreadRt>) {
    let id = ThreadId::new();
    let store = ExtensionData::new(id.to_string());
    store.insert(ThreadRt::new(id, None, false, Duration::ZERO));
    let rt = store.get::<ThreadRt>().expect("inserted");
    (store, rt)
}

/// Let every task that can run, run.
async fn settle() {
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

async fn turn_start(autopilot: &Autopilot, thread_store: &ExtensionData) {
    let rt = thread_store.get::<ThreadRt>().expect("fixture runtime");
    if autopilot.thread_rt(rt.key()).is_none() {
        autopilot.keep_thread(&rt);
    }
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5".to_string(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new("turn-1");
    autopilot
        .on_turn_start(TurnStartInput {
            turn_id: "turn-1",
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &session,
            thread_store,
            turn_store: &turn,
        })
        .await;
}

#[tokio::test(start_paused = true)]
async fn a_timer_fires_at_its_deadline_and_leaves_the_slot() {
    let autopilot = autopilot();
    let (_store, rt) = thread();
    autopilot.schedule(&rt, clock() + 5 * MIN);
    assert!(rt.timer_pending());
    tokio::time::advance(4 * MIN).await;
    settle().await;
    assert_eq!(rt.evaluations(), 0);
    tokio::time::advance(MIN + Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(rt.evaluations(), 1);
    assert!(!rt.timer_pending());
}

/// A TIMER THE HOST SLEPT THROUGH LOOKS AGAIN SOON AFTER THE WAKE (2026-10-24): tokio's clock
/// stops while the host sleeps, so a wait of an hour armed ten minutes before a two-hour sleep
/// came due 50 awake minutes after the wake. The timer counts the time slept toward its
/// deadline: it looks again `AFTER_WAKE` after the wake (where the decision settles and
/// measures first). POSITIVE CONTROL: with no sleep the same timer waits its whole hour.
#[tokio::test(start_paused = true)]
async fn a_timer_the_host_slept_through_looks_again_soon_after_the_wake() {
    use codex_alab_core::wake::AFTER_WAKE;
    use codex_alab_core::wake::Reading;
    use codex_alab_core::wake::Wake;
    let base = Duration::from_secs(1000);
    let slept = 120 * MIN;
    for sleeps in [true, false] {
        let autopilot = autopilot();
        let (_store, rt) = thread();
        let start = tokio::time::Instant::now();
        let woke_at = start + 10 * MIN;
        autopilot.set_wake_read(Arc::new(move || {
            let asleep = sleeps && tokio::time::Instant::now() >= woke_at;
            Some(Reading {
                asleep: if asleep { base + slept } else { base },
                last: asleep.then(|| Wake {
                    at: woke_at.into_std(),
                    slept,
                    seq: 1,
                }),
            })
        }));
        autopilot.schedule(&rt, clock() + 60 * MIN);
        tokio::time::advance(10 * MIN).await;
        settle().await;
        assert_eq!(
            rt.evaluations(),
            0,
            "sleeps={sleeps}: the network's own wake first"
        );
        tokio::time::advance(AFTER_WAKE + Duration::from_secs(1)).await;
        settle().await;
        if sleeps {
            assert_eq!(
                rt.evaluations(),
                1,
                "soon after the wake, not 50 minutes later"
            );
            assert!(!rt.timer_pending());
            continue;
        }
        // POSITIVE CONTROL: no sleep, the whole hour.
        assert_eq!(rt.evaluations(), 0, "no sleep: not yet");
        tokio::time::advance(50 * MIN).await;
        settle().await;
        assert_eq!(rt.evaluations(), 1, "no sleep: at its deadline");
    }
}

#[tokio::test(start_paused = true)]
async fn owed_sleep_origins_survive_record_sync_rearm_firing_and_state_fallback() {
    use codex_alab_core::session::now_ms;
    use codex_alab_core::wake::Reading;
    use codex_alab_core::wake::Wake;

    fn published_at(autopilot: &Autopilot, at: std::time::Instant) {
        let before = now_ms();
        autopilot.sync_record_owed();
        let record = autopilot
            .supervisor()
            .shared()
            .owed_act()
            .expect("published intent");
        let after = now_ms();
        let wait = at.saturating_duration_since(clock());
        let millis = u64::try_from(wait.as_millis()).expect("fixture wait fits the record");
        let rounded = millis + u64::from(!wait.subsec_nanos().is_multiple_of(1_000_000)) + 1;
        assert!(record.not_before_ms >= before.saturating_add(millis));
        assert!(record.not_before_ms <= after.saturating_add(rounded));
        assert_eq!(record.text.as_deref(), Some("Defend it, as decided."));
    }

    let autopilot = autopilot();
    let (_store, rt) = thread();
    autopilot.keep_thread(&rt);
    let base = Duration::from_secs(5_000);
    let sec = Duration::from_secs(1);
    let reading = Arc::new(std::sync::Mutex::new(None::<Reading>));
    let read = Arc::clone(&reading);
    autopilot.set_wake_read(Arc::new(move || *read.lock().unwrap()));
    let at = clock() + 100 * sec;
    autopilot.schedule_act(&rt, at, Some(owed_at(at)));
    published_at(&autopilot, at);
    *reading.lock().unwrap() = Some(Reading {
        asleep: base,
        last: None,
    });
    published_at(&autopilot, at);
    for asleep in [base - sec, base + sec / 2] {
        *reading.lock().unwrap() = Some(Reading { asleep, last: None });
        published_at(&autopilot, at);
    }
    *reading.lock().unwrap() = Some(Reading {
        asleep: base + 10 * sec,
        last: None,
    });
    published_at(&autopilot, at - 10 * sec);
    published_at(&autopilot, at - 10 * sec);
    let (taken, carried) = autopilot
        .take_owed_act()
        .await
        .expect("tracked timer is carried");
    autopilot.put_back(&taken, &carried);
    let restored = rt.owed_timer().expect("record rollback rearmed the act");
    published_at(&autopilot, restored.not_before);
    published_at(&autopilot, restored.not_before);
    settle().await;

    let permit = rt.hold_eval();
    *reading.lock().unwrap() = Some(Reading {
        asleep: base + 20 * sec,
        last: Some(Wake {
            at: clock(),
            slept: 10 * sec,
            seq: 2,
        }),
    });
    tokio::time::advance(81 * sec).await;
    settle().await;
    assert!(
        !rt.timer_pending(),
        "the newly measured sleep made the timer due"
    );
    assert_eq!(rt.evaluations(), 1, "evaluation is held behind its permit");
    autopilot.sync_record_owed();
    assert!(
        autopilot
            .supervisor()
            .shared()
            .owed_act()
            .expect("fired intent is published")
            .not_before_ms
            <= now_ms()
    );
    let (_, fired) = autopilot
        .take_owed_act()
        .await
        .expect("fired intent can be carried");
    assert!(fired.not_before_ms <= now_ms());
    assert_eq!(fired.text.as_deref(), Some(restored.text.as_str()));

    let state_at = clock() + 100 * sec;
    rt.with_state(|state| {
        state.observe_resume(true, Some("a resumed task"), false);
        state.observe_owed(Some(owed_at(state_at)), false, clock());
        state.asleep_mark = Some(base + 20 * sec);
        assert_eq!(
            state.slept_to(Some(base + 30 * sec), clock()),
            Some(10 * sec)
        );
    });
    *reading.lock().unwrap() = Some(Reading {
        asleep: base + 30 * sec,
        last: None,
    });
    published_at(&autopilot, state_at - 10 * sec);
    published_at(&autopilot, state_at - 10 * sec);
    *reading.lock().unwrap() = Some(Reading {
        asleep: base + 40 * sec,
        last: None,
    });
    published_at(&autopilot, state_at - 20 * sec);
    published_at(&autopilot, state_at - 20 * sec);
    rt.with_state(|state| {
        assert_eq!(
            state.slept_to(Some(base + 40 * sec), clock()),
            Some(10 * sec)
        );
    });
    published_at(&autopilot, state_at - 20 * sec);
    drop(permit);
    settle().await;
}

#[tokio::test(start_paused = true)]
async fn an_initially_unread_wait_uses_a_baseline_established_by_record_sync() {
    use codex_alab_core::wake::POLL;
    use codex_alab_core::wake::Reading;
    use codex_alab_core::wake::Wake;
    let autopilot = autopilot();
    let (_store, rt) = thread();
    autopilot.keep_thread(&rt);
    let sec = Duration::from_secs(1);
    let base = 5_000 * sec;
    let reading = Arc::new(std::sync::Mutex::new(None::<Reading>));
    let read = Arc::clone(&reading);
    autopilot.set_wake_read(Arc::new(move || *read.lock().unwrap()));
    let at = clock() + 100 * sec;
    autopilot.schedule_act(&rt, at, Some(owed_at(at)));
    settle().await;
    *reading.lock().unwrap() = Some(Reading {
        asleep: base,
        last: None,
    });
    autopilot.sync_record_owed();
    *reading.lock().unwrap() = Some(Reading {
        asleep: base + 10 * sec,
        last: Some(Wake {
            at: clock(),
            slept: 10 * sec,
            seq: 1,
        }),
    });
    let permit = rt.hold_eval();
    tokio::time::advance(90 * sec + POLL).await;
    settle().await;
    tokio::time::advance(POLL).await;
    settle().await;
    assert!(
        !rt.timer_pending(),
        "the sync's first baseline preserves later measured sleep"
    );
    let (_, carried) = autopilot
        .take_owed_act()
        .await
        .expect("the fired act is carried");
    assert!(carried.not_before_ms <= codex_alab_core::session::now_ms());
    assert_eq!(carried.text.as_deref(), Some("Defend it, as decided."));
    drop(permit);
    settle().await;
}

#[tokio::test(start_paused = true)]
async fn a_newer_timer_replaces_the_older() {
    let autopilot = autopilot();
    let (_store, rt) = thread();
    autopilot.schedule(&rt, clock() + 5 * MIN);
    autopilot.schedule(&rt, clock() + 10 * MIN);
    tokio::time::advance(6 * MIN).await;
    settle().await;
    assert_eq!(rt.evaluations(), 0, "the replaced timer never fires");
    tokio::time::advance(5 * MIN).await;
    settle().await;
    assert_eq!(rt.evaluations(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_turn_start_cancels_and_joins_the_pending_timer() {
    let autopilot = autopilot();
    let (store, rt) = thread();
    autopilot.schedule(&rt, clock() + 5 * MIN);
    turn_start(&autopilot, &store).await;
    assert!(!rt.timer_pending());
    tokio::time::advance(10 * MIN).await;
    settle().await;
    assert_eq!(rt.evaluations(), 0);
    // Negative control: without the turn start the same timer fires.
    autopilot.schedule(&rt, clock() + 5 * MIN);
    tokio::time::advance(6 * MIN).await;
    settle().await;
    assert_eq!(rt.evaluations(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_thread_stop_cancels_and_joins_the_pending_timer() {
    let autopilot = autopilot();
    let (store, rt) = thread();
    autopilot.schedule(&rt, clock() + 5 * MIN);
    let session = ExtensionData::new("session");
    autopilot
        .on_thread_stop(ThreadStopInput {
            session_store: &session,
            thread_store: &store,
        })
        .await;
    assert!(!rt.timer_pending());
    tokio::time::advance(10 * MIN).await;
    settle().await;
    assert_eq!(rt.evaluations(), 0);
}

/// AN ACT OWED AT REST OUTLIVES AN EXIT aterm RELAUNCHES: SIGHUP (the terminal went away under
/// aterm's own crash, which its restore relaunches) and SIGTERM (aterm's stall or memory
/// remedy) take the `/exit` path, whose teardown stops every thread; the stop leaves the
/// record's `owed` as it stood, so the record the signal keeps hands the act to aterm's
/// relaunch (`Relaunch::Owed`), as a crash at rest does. Nothing is typed in the leaving
/// process: its timer is cancelled. NEGATIVE CONTROLS: a stop with no signal (a person's
/// `/exit`, a thread that ends) and a person's SIGINT take the act out with the thread; and a
/// turn that starts first takes it out whatever comes after (someone else's turn superseded
/// it).
#[tokio::test(start_paused = true)]
async fn an_act_owed_at_rest_outlives_an_exit_aterm_relaunches() {
    const SIGHUP: i32 = 1;
    const SIGINT: i32 = 2;
    const SIGTERM: i32 = 15;
    for (signal, kept) in [
        (Some(SIGHUP), true),
        (Some(SIGTERM), true),
        (Some(SIGINT), false),
        (None, false),
    ] {
        let autopilot = autopilot();
        let (store, rt) = thread();
        autopilot.keep_thread(&rt);
        // A second thread, owing nothing, stops after it: its stop leaves the act too.
        let (other_store, other) = thread();
        autopilot.keep_thread(&other);
        let shared = autopilot.supervisor().shared();
        let at = clock() + 5 * MIN;
        autopilot.schedule_act(&rt, at, Some(owed_at(at)));
        assert!(shared.owed_act().is_some());
        if let Some(signal) = signal {
            assert!(shared.set_signal(signal));
        }
        let session = ExtensionData::new("session");
        for thread_store in [&store, &other_store] {
            autopilot
                .on_thread_stop(ThreadStopInput {
                    session_store: &session,
                    thread_store,
                })
                .await;
        }
        assert!(
            !rt.timer_pending(),
            "nothing is typed in the leaving process"
        );
        let owed = shared.owed_act();
        assert_eq!(owed.is_some(), kept, "signal {signal:?}");
        if kept {
            let owed = owed.unwrap_or_else(|| panic!("kept"));
            assert_eq!(owed.thread, rt.key());
            assert_eq!(owed.text.as_deref(), Some("Defend it, as decided."));
        }
        tokio::time::advance(10 * MIN).await;
        settle().await;
        assert_eq!(rt.evaluations() + other.evaluations(), 0);
    }
    // NEGATIVE CONTROL: a turn started before the signal superseded the act.
    let autopilot = autopilot();
    let (store, rt) = thread();
    autopilot.keep_thread(&rt);
    let shared = autopilot.supervisor().shared();
    autopilot.schedule_act(&rt, clock() + 5 * MIN, Some(owed_at(clock() + 5 * MIN)));
    turn_start(&autopilot, &store).await;
    assert!(shared.set_signal(SIGHUP));
    let session = ExtensionData::new("session");
    autopilot
        .on_thread_stop(ThreadStopInput {
            session_store: &session,
            thread_store: &store,
        })
        .await;
    assert_eq!(shared.owed_act(), None);
}

#[test]
fn aterm_harness_lease_is_looked_at_again_soon() {
    use crate::supervisor::HAND_RECHECK;
    use crate::supervisor::RECHECK;
    use crate::supervisor::recheck_after;
    use codex_alab_aterm::status::NotReady;
    // aterm's relaunch types under its harness's lease: the carry-on behind it looks soon.
    assert_eq!(
        recheck_after(&NotReady::Hand("lease:aterm-harness@4242".to_string())),
        HAND_RECHECK
    );
    // Negative control: another driver's hand is a longer wait.
    assert_eq!(
        recheck_after(&NotReady::Hand("lease:someone@1".to_string())),
        RECHECK
    );
}

/// A paced act the old image owed, as its decision left it.
fn owed_at(at: std::time::Instant) -> crate::decide::Owed {
    crate::decide::Owed {
        native_origin: None,
        rule: crate::words::RULE_DONE_CHECK,
        text: "Defend it, as decided.".to_string(),
        mode: crate::decide::Mode::Cruise,
        not_before: at,
        repo_owed: Some(Vec::new()),
        resume_goal: false,
        landing: false,
    }
}

/// A WALL'S TRY OF A LAND ACT CROSSES THE RECORD WITH ITS LANDING (parity-3 review, F4): the
/// try waiting on its rung writes `landing` into the record's `owed`, and the record's act
/// reads back as this image's with it, so aterm's relaunch after a crash at rest types a
/// retry whose clean defense lands. NEGATIVE CONTROL: a paced act's record never reads a
/// landing, whatever it says.
#[tokio::test(start_paused = true)]
async fn a_walls_try_of_a_land_act_crosses_the_record_with_its_landing() {
    let autopilot = autopilot();
    let (_store, rt) = thread();
    autopilot.keep_thread(&rt);
    let shared = autopilot.supervisor().shared();
    let at = clock() + 5 * MIN;
    let retry = crate::decide::Owed {
        rule: crate::words::RULE_API_RETRY,
        text: "Codex reported an API error (500); carry on.".to_string(),
        mode: crate::decide::Mode::Land,
        repo_owed: None,
        landing: true,
        ..owed_at(at)
    };
    autopilot.schedule_act(&rt, at, Some(retry));
    let record = shared
        .owed_act()
        .unwrap_or_else(|| panic!("the record carries the try"));
    assert_eq!(
        (record.rule.as_deref(), record.landing),
        (Some(crate::words::RULE_API_RETRY), true)
    );
    let read = Autopilot::owed_of(&record).unwrap_or_else(|| panic!("read back"));
    assert_eq!(
        (read.rule, read.mode, read.landing),
        (
            crate::words::RULE_API_RETRY,
            crate::decide::Mode::Land,
            true
        )
    );
    let paced = codex_alab_core::session::OwedAct {
        rule: Some(crate::words::RULE_LAND_NUDGE.to_string()),
        ..record
    };
    assert_eq!(
        Autopilot::owed_of(&paced).map(|owed| owed.landing),
        Some(false)
    );
}

/// THE RECORD CARRIES THE PACED ACT OWED NOW (FOREVER CRUISE: the back-off's wait is rest,
/// so the record says `idle` there and a crash at rest left nothing to carry; end-to-end run
/// 2026-10-20): a paced act's wait puts the act, as decided, into the record's `owed`, and so
/// does an act the image before handed over; its turn's start, the timer's firing (its
/// evaluation decides it now) and a wait that is no paced act's take it out. NEGATIVE CONTROL: a wait that is no paced act's carries nothing.
#[tokio::test(start_paused = true)]
async fn the_record_carries_the_paced_act_owed_now() {
    let autopilot = autopilot();
    let (store, rt) = thread();
    autopilot.keep_thread(&rt);
    let shared = autopilot.supervisor().shared();
    let at = clock() + 5 * MIN;
    autopilot.schedule_act(&rt, at, Some(owed_at(at)));
    let owed = shared
        .owed_act()
        .unwrap_or_else(|| panic!("the record carries the act"));
    assert_eq!(owed.thread, rt.key());
    assert_eq!(owed.rule.as_deref(), Some(crate::words::RULE_DONE_CHECK));
    assert_eq!(owed.text.as_deref(), Some("Defend it, as decided."));
    assert_eq!(owed.mode.as_deref(), Some("cruise"));
    assert!(!owed.landed);
    // Its turn's start takes it out.
    turn_start(&autopilot, &store).await;
    assert_eq!(shared.owed_act(), None);
    // The next paced act belongs to rest after that fake native turn ended.
    shared.turn_ended(rt.key(), "turn-1");
    // So does the timer's firing: its evaluation decides it now.
    let at = clock() + 5 * MIN;
    autopilot.schedule_act(&rt, at, Some(owed_at(at)));
    assert!(shared.owed_act().is_some());
    tokio::time::advance(6 * MIN).await;
    settle().await;
    assert_eq!(rt.evaluations(), 1);
    assert_eq!(shared.owed_act(), None);
    // An act the image before handed over (a re-exec's, a relaunch's), not yet waited on.
    rt.with_state(|s| s.owed = Some(owed_at(clock() + 5 * MIN)));
    autopilot.sync_record_owed();
    assert_eq!(
        shared.owed_act().and_then(|owed| owed.text),
        Some("Defend it, as decided.".to_string())
    );
    rt.with_state(|s| s.owed = None);
    // NEGATIVE CONTROL: a wait that is no paced act's.
    autopilot.schedule_act(&rt, clock() + 5 * MIN, Some(owed_at(clock() + 5 * MIN)));
    autopilot.schedule(&rt, clock() + 5 * MIN);
    assert_eq!(shared.owed_act(), None);
}

/// THE RECORD CARRIES THE THREAD'S PACE, AND A RESUME TAKES IT UP (parity review S2): a turn
/// end's short streak reaches the pace the record writes; a resumed thread's new memory takes
/// up the pace the record writer's start left for it (the image before a re-exec, the dead
/// process aterm relaunched), its ladder's times turned into this clock and Land's hold
/// standing included (review of gap 11, finding 1), and carries it on.
/// NEGATIVE CONTROLS: a pace at rest is written as none; another thread's pace is not this
/// one's.
#[tokio::test]
async fn the_record_carries_the_threads_pace_and_a_resume_takes_it_up() {
    use codex_alab_core::session::LandHold;
    use codex_alab_core::session::Pace;
    use codex_alab_core::session::WallMark;
    use codex_alab_core::session::now_ms;
    use codex_extension_api::TurnStopInput;
    let autopilot = autopilot();
    let (store, rt) = thread();
    autopilot.keep_thread(&rt);
    let shared = autopilot.supervisor().shared();
    assert_eq!(shared.pace(rt.key()), None);
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new("turn-1");
    autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &session,
            thread_store: &store,
            turn_store: &turn,
        })
        .await;
    assert_eq!(shared.pace(rt.key()).map(|pace| pace.short_streak), Some(1));
    // A resumed thread takes up what the record writer's start left for it.
    let (_store, resumed) = thread();
    let first = now_ms() - 20 * 60_000;
    let last = now_ms() - 5 * 60_000;
    let answered = now_ms() - 3 * 60_000;
    // The acts `continue_per_hour` counts: one within the hour, one past it.
    let typed = now_ms() - 10 * 60_000;
    let stale = now_ms() - 61 * 60_000;
    shared.set_pace(
        resumed.key(),
        Pace {
            short_streak: 3,
            wall: Some(WallMark {
                class: "server".to_string(),
                attempts: 2,
                first_seen_ms: first,
                last_try_ms: Some(last),
                since_ms: Some(answered),
                answer_owed: true,
            }),
            typed_ms: vec![stale, typed],
            cut: None,
            checks: 4,
            land_hold: Some(LandHold {
                since_ms: 4_000,
                nudges: 2,
            }),
            ..Pace::default()
        },
    );
    autopilot.seed_pace(&resumed, None);
    let state = resumed.state();
    assert_eq!(state.short_streak, 3);
    assert_eq!(state.checks, 4, "the done checks the process before typed");
    assert_eq!(
        (state.land.hold.since_ms, state.land.nudges),
        (Some(4_000), 2),
        "Land's hold the process before said"
    );
    let wall = state.wall.unwrap_or_else(|| panic!("the ladder"));
    assert_eq!(
        (wall.carried_class(), wall.attempts, wall.answer_owed),
        (Some("server"), 2, true)
    );
    let ago = |at: Option<std::time::Instant>| {
        at.map(|at| clock().saturating_duration_since(at))
            .unwrap_or_default()
    };
    let second = Duration::from_secs(1);
    let near = |got: Duration, want: Duration| got + second >= want && got <= want + second;
    assert!(near(ago(wall.last_try), 5 * MIN), "{wall:?}");
    assert!(near(ago(Some(wall.since)), 3 * MIN), "{wall:?}");
    assert_eq!(state.typed.len(), 1, "only the act within the hour counts");
    assert!(near(ago(state.typed.front().copied()), 10 * MIN));
    let back = shared
        .pace(resumed.key())
        .unwrap_or_else(|| panic!("carried on"));
    assert_eq!(back.short_streak, 3);
    assert_eq!(back.checks, 4, "the done checks carried on");
    assert_eq!(
        back.land_hold,
        Some(LandHold {
            since_ms: 4_000,
            nudges: 2,
        }),
        "Land's hold carried on"
    );
    assert!(
        back.typed_ms.len() == 1 && back.typed_ms[0].abs_diff(typed) <= 1_000,
        "{back:?}"
    );
    let mark = back.wall.unwrap_or_else(|| panic!("the ladder carried on"));
    assert_eq!(
        (mark.class.as_str(), mark.attempts, mark.answer_owed),
        ("server", 2, true)
    );
    assert!(
        mark.last_try_ms
            .is_some_and(|ms| ms.abs_diff(last) <= 1_000)
            && mark
                .since_ms
                .is_some_and(|ms| ms.abs_diff(answered) <= 1_000)
            && mark.first_seen_ms.abs_diff(first) <= 1_000,
        "{mark:?}"
    );
    // NEGATIVE CONTROL: at rest.
    resumed.with_state(|s| {
        s.short_streak = 0;
        s.checks = 0;
        s.wall = None;
        s.typed.clear();
        s.land = crate::decide::LandEpisode::default();
    });
    autopilot.sync_pace(&resumed);
    assert_eq!(shared.pace(resumed.key()), None);
    // NEGATIVE CONTROL: another thread's pace.
    let (_store, other) = thread();
    autopilot.seed_pace(&other, None);
    assert_eq!(other.state().short_streak, 0);
    assert_eq!(shared.pace(other.key()), None);
}

/// RULING F'S REPLIES OUTLIVE A RE-EXEC AND A `/resume` BACK, NOT ATERM'S RELAUNCH (aterm's
/// `seed_replies` and `seed_replies_reset` from its books, and `launched_since_books`: a
/// program launched after the newest row is another run): the image after a live upgrade
/// holds the same reply as the image before would have, its replies, the time of the last
/// answer and the reset it showed (still to come) taken from the record's pace and written
/// back, with the held point's words; a `/resume` away and back in one process keeps what
/// this process counted (parity review of the stage-1 port, finding 5). NEGATIVE CONTROL:
/// aterm's relaunch from a dead record of this tab, whose pace is handed on, seeds no replies,
/// no reset and no point, while the short streak still carries.
#[tokio::test]
async fn a_re_exec_keeps_ruling_fs_replies_and_a_relaunch_starts_them_empty() {
    use codex_alab_core::session::ATERM_SESSION_ENV;
    use codex_alab_core::session::Inherited;
    use codex_alab_core::session::KeptReply;
    use codex_alab_core::session::Launch;
    use codex_alab_core::session::Pace;
    use codex_alab_core::session::Predecessor;
    use codex_alab_core::session::ReplyPoint;
    use codex_alab_core::session::now_ms;
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Start {
        ReExec,
        ResumeBack,
        Relaunch,
    }
    let said = crate::decide::TurnEnd {
        at: clock(),
        by_autopilot: true,
        by_person: false,
        text: Some("Still on the parser.".to_string()),
        earlier: Some("Rebuilt.".to_string()),
        questions: false,
        worked: Duration::ZERO,
        wall: None,
        aborted: false,
        ran_tools: false,
        restored: false,
    };
    let answered = now_ms() - 2 * 60_000;
    let reset = now_ms() + 10 * 60_000;
    let pace = Pace {
        short_streak: 2,
        replies: vec![
            KeptReply {
                mark: 0x11,
                answers: 1,
                walled: false,
                tries: 0,
            },
            KeptReply {
                mark: crate::replies::mark_of(&said),
                answers: 2,
                walled: false,
                tries: 0,
            },
        ],
        replies_run: 3,
        replies_at_ms: Some(answered),
        replies_reset_ms: Some(reset),
        replies_point: Some(ReplyPoint {
            text: said.text.clone(),
            earlier: said.earlier.clone(),
            questions: false,
        }),
        ..Pace::default()
    };
    for start in [Start::ReExec, Start::ResumeBack, Start::Relaunch] {
        let autopilot = autopilot();
        let shared = autopilot.supervisor().shared();
        let (_store, rt) = thread();
        match start {
            Start::ReExec => {
                assert!(shared.set_inherited(Inherited {
                    exec_generation: 1,
                    ..Inherited::default()
                }));
            }
            // This process's own pace, which its sync wrote: no predecessor's.
            Start::ResumeBack => {}
            // A dead record of this tab whose pace the writer's start took up.
            Start::Relaunch => {
                assert!(shared.set_launch(Launch {
                    vars: vec![(ATERM_SESSION_ENV.into(), "s-ab12".into())],
                    pid: std::process::id(),
                    started_at_ms: now_ms(),
                    ..Launch::default()
                }));
                shared.note_predecessor(Predecessor {
                    pid: 41,
                    session_id: rt.key().to_string(),
                    updated_at_ms: answered,
                    pace: Some(pace.clone()),
                    aterm_session: Some("s-ab12".to_string()),
                    ..Predecessor::default()
                });
            }
        }
        shared.set_pace(rt.key(), pace.clone());
        autopilot.seed_pace(&rt, None);
        let state = rt.state();
        assert_eq!(
            state.short_streak, 2,
            "{start:?}: the streak carries either way"
        );
        if start == Start::Relaunch {
            assert_eq!(
                state.replies,
                crate::replies::Replies::default(),
                "a relaunch keeps none"
            );
            assert_eq!(
                (state.replies_at, state.replies_reset, &state.held_seed),
                (None, None, &None)
            );
            continue;
        }
        let kept: Vec<(u64, usize, bool, usize)> = state
            .replies
            .kept()
            .map(|k| (k.mark, k.answers, k.walled, k.tries))
            .collect();
        assert_eq!(
            kept,
            [
                (0x11, 1, false, 0),
                (crate::replies::mark_of(&said), 2, false, 0)
            ],
            "{start:?}"
        );
        assert_eq!(state.replies.run, 3);
        assert_eq!(state.held_by(), Some(crate::replies::HeldBy::SameReply));
        let ago = state
            .replies_at
            .map(|at| clock().saturating_duration_since(at))
            .unwrap_or_default();
        assert!(
            ago + Duration::from_secs(1) >= 2 * MIN && ago <= 2 * MIN + Duration::from_secs(1),
            "{ago:?}"
        );
        let ahead = state
            .replies_reset
            .map(|at| at.saturating_duration_since(clock()))
            .unwrap_or_default();
        assert!(
            ahead + Duration::from_secs(1) >= 10 * MIN
                && ahead <= 10 * MIN + Duration::from_secs(1),
            "{start:?}: the reset still to come: {ahead:?}"
        );
        assert_eq!(
            state
                .held_point()
                .map(|p| (p.text.clone(), p.earlier.clone())),
            Some((said.text.clone(), said.earlier.clone())),
            "{start:?}: the held point's words"
        );
        let back = shared
            .pace(rt.key())
            .unwrap_or_else(|| panic!("carried on"));
        assert_eq!(
            (
                back.replies.clone(),
                back.replies_run,
                back.replies_point.clone()
            ),
            (pace.replies.clone(), 3, pace.replies_point.clone())
        );
        assert!(
            back.replies_at_ms
                .is_some_and(|ms| ms.abs_diff(answered) <= 1_000)
                && back
                    .replies_reset_ms
                    .is_some_and(|ms| ms.abs_diff(reset) <= 1_000),
            "{back:?}"
        );
        drop(state);
        // Once the resume scan runs and drops the held seed (here over a taskless history),
        // the synced pace drops `replies_point` with it.
        rt.with_state(|s| {
            crate::extension::resume_bound(s, &[], false, None, 0, clock());
        });
        autopilot.sync_pace(&rt);
        assert_eq!(
            shared.pace(rt.key()).and_then(|p| p.replies_point),
            None,
            "{start:?}: dropped held seed clears replies_point"
        );
    }
}

/// A PERSON'S ESC OUTLIVES A RE-EXEC AND A RELAUNCH (the forever-persist lane, item a; aterm's
/// `person_hold`, FOREVER CRUISE): the image before held the point a person's Esc stopped for
/// the grace from the Esc, and wrote the Esc and its turn into the record's pace; the image after
/// a live upgrade (a resume of any other kind), and aterm's relaunch after a signal's teardown,
/// take it up ([`Autopilot::seed_pace`]) and, where the history's abort ended that turn, hold the
/// restored point from the same Esc and then carry on, writing it back. NEGATIVE CONTROLS: with
/// no Esc carried, or one carried for another turn, the image after a re-exec parks the
/// interrupted point for good (main before this lane), and the relaunch reads the abort as the
/// signal's teardown (its carry-on owed at once).
#[tokio::test]
async fn a_persons_esc_outlives_a_re_exec_and_a_relaunch() {
    use crate::decide::Act;
    use crate::resume_history::ResumeSource;
    use codex_alab_core::session::ATERM_SESSION_ENV;
    use codex_alab_core::session::EscHold;
    use codex_alab_core::session::Inherited;
    use codex_alab_core::session::Launch;
    use codex_alab_core::session::Pace;
    use codex_alab_core::session::Predecessor;
    use codex_alab_core::session::now_ms;
    const TURN: &str = "the-turn-a-person-stopped";
    let event = |value: serde_json::Value| {
        codex_history::RolloutItem::EventMsg(
            serde_json::from_value::<codex_protocol::protocol::EventMsg>(value)
                .expect("native event"),
        )
    };
    let items = [
        event(
            serde_json::json!({"type":"task_started", "turn_id":"the-turn-before",
            "model_context_window":258400, "collaboration_mode_kind":"default",
            "turn_attribution": {"turn_id": "the-turn-before", "turn_trigger": "user"}}),
        ),
        event(
            serde_json::json!({"type":"task_complete", "turn_id":"the-turn-before",
            "last_agent_message":"Stage 1 is in."}),
        ),
        event(serde_json::json!({"type":"task_started", "turn_id":TURN,
            "model_context_window":258400, "collaboration_mode_kind":"default",
            "turn_attribution": {"turn_id": TURN, "turn_trigger": "user"}})),
        event(serde_json::json!({"type":"turn_aborted", "turn_id":TURN, "reason":"interrupted"})),
    ];
    let grace = Duration::from_secs(15);
    let cruise = crate::decide::Policy {
        continue_on: true,
        continue_text: Some("keep going".to_string()),
        continue_per_hour: None,
        approve_all: true,
        answer_questions: true,
        answer_text: None,
        questions: crate::decide::Questions::Recommended,
        retry_api_errors: true,
        probe_api: true,
        resume_limits: true,
        limit_wait: true,
        compact_on_context_wall: true,
        upgrade: true,
        upgrade_ask: false,
        facts: false,
        human_grace: grace,
        autopilot: crate::decide::Mode::Cruise,
        texts: crate::decide::Texts::default(),
    };
    let timing = Timing::default();
    let esc_ms = now_ms() - 5_000;
    for relaunch in [false, true] {
        for carried in [Some(TURN), Some("another-turn"), None] {
            let case = format!("relaunch={relaunch} carried={carried:?}");
            let autopilot = autopilot();
            let shared = autopilot.supervisor().shared();
            let (_store, rt) = thread();
            let pace = Pace {
                short_streak: 1,
                esc: carried.map(|turn| EscHold {
                    at_ms: esc_ms,
                    turn: turn.to_string(),
                }),
                ..Pace::default()
            };
            if relaunch {
                assert!(shared.set_launch(Launch {
                    vars: vec![(ATERM_SESSION_ENV.into(), "s-ab12".into())],
                    pid: std::process::id(),
                    started_at_ms: now_ms(),
                    ..Launch::default()
                }));
                shared.note_predecessor(Predecessor {
                    pid: 41,
                    session_id: rt.key().to_string(),
                    updated_at_ms: esc_ms,
                    pace: Some(pace.clone()),
                    aterm_session: Some("s-ab12".to_string()),
                    ..Predecessor::default()
                });
            } else {
                assert!(shared.set_inherited(Inherited {
                    exec_generation: 1,
                    ..Inherited::default()
                }));
            }
            shared.set_pace(rt.key(), pace);
            autopilot.seed_pace(&rt, None);
            let source = if relaunch {
                ResumeSource::Signal
            } else {
                ResumeSource::Other
            };
            let now = clock();
            rt.with_state(|s| {
                crate::extension::resume_bound(s, &items, source, None, 0, now);
            });
            let state = rt.state();
            assert!(state.esc_seed.is_none(), "{case}: the seed is spent");
            if carried == Some(TURN) {
                let esc_at = state.esc_at.unwrap_or_else(|| panic!("{case}: held"));
                let ago = now.saturating_duration_since(esc_at);
                assert!(
                    ago + Duration::from_secs(1) >= Duration::from_secs(5)
                        && ago <= Duration::from_secs(6),
                    "{case}: from the same Esc: {ago:?}"
                );
                assert!(!state.carry_on, "{case}: no teardown's carry-on");
                let held = crate::decide::decide(&state, &cruise, &timing, now, Duration::ZERO);
                assert_eq!(
                    (&held.act, held.why.as_str()),
                    (&Act::Nothing, crate::decide::ESC_HOLD),
                    "{case}"
                );
                assert_eq!(held.look_again, Some(esc_at + grace), "{case}");
                assert_eq!(
                    crate::decide::decide(&state, &cruise, &timing, esc_at + grace, Duration::ZERO)
                        .act,
                    Act::Type {
                        rule: crate::words::RULE_CONTINUE,
                        text: "keep going".to_string(),
                    },
                    "{case}: carried on past the grace"
                );
                drop(state);
                autopilot.sync_pace(&rt);
                let back = shared.pace(rt.key()).and_then(|pace| pace.esc);
                assert!(
                    back.as_ref()
                        .is_some_and(|esc| esc.turn == TURN && esc.at_ms.abs_diff(esc_ms) <= 1_000),
                    "{case}: written back: {back:?}"
                );
                continue;
            }
            assert_eq!(state.esc_at, None, "{case}");
            if relaunch {
                assert!(state.carry_on, "{case}: the teardown's carry-on");
            } else {
                assert_eq!(
                    crate::decide::decide(
                        &state,
                        &cruise,
                        &timing,
                        now + 24 * 60 * MIN,
                        Duration::ZERO
                    )
                    .act,
                    Act::Nothing,
                    "{case}: parked"
                );
            }
        }
    }
}

/// THE PERSON'S HOUR AND THE GO-AHEAD'S COUNT CROSS THE RECORD (review of the go-ahead port,
/// findings 3 and 9; aterm's `go-ahead-hour@v1`, booked and seeded at every start): the image
/// after a re-exec holds the hour the image before held (one that ran from a turn's end the
/// window does not keep) and bounds the standing park where it left it, and writes both back.
/// aterm's relaunch from a dead record of this tab holds the hour too, but takes no count up, as
/// it takes no reply up. NEGATIVE CONTROL: an hour already over holds nothing and is not written.
#[tokio::test]
async fn the_record_carries_the_persons_hour_and_the_go_ahead_count() {
    use codex_alab_core::session::ATERM_SESSION_ENV;
    use codex_alab_core::session::GoAheadCount;
    use codex_alab_core::session::Inherited;
    use codex_alab_core::session::Launch;
    use codex_alab_core::session::Pace;
    use codex_alab_core::session::Predecessor;
    use codex_alab_core::session::now_ms;
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Start {
        ReExec,
        ResumeBack,
        Relaunch,
    }
    let near = |ms: Option<u64>, want: u64| ms.is_some_and(|ms| ms.abs_diff(want) <= 1_000);
    let ago_ms = |at: Option<std::time::Instant>| {
        at.map(|at| {
            let ago = clock().saturating_duration_since(at).as_millis();
            now_ms().saturating_sub(u64::try_from(ago).unwrap_or(u64::MAX))
        })
    };
    for over in [false, true] {
        let from = now_ms() - if over { 61 } else { 10 } * 60_000;
        let first = now_ms() - 5 * 60_000;
        let pace = Pace {
            short_streak: 1,
            go_ahead_hour_ms: Some(from),
            go_ahead: Some(GoAheadCount {
                acts: 2,
                first_ms: Some(first),
            }),
            ..Pace::default()
        };
        for start in [Start::ReExec, Start::ResumeBack, Start::Relaunch] {
            let autopilot = autopilot();
            let shared = autopilot.supervisor().shared();
            let (_store, rt) = thread();
            match start {
                Start::ReExec => {
                    assert!(shared.set_inherited(Inherited {
                        exec_generation: 1,
                        ..Inherited::default()
                    }));
                }
                Start::ResumeBack => {}
                Start::Relaunch => {
                    assert!(shared.set_launch(Launch {
                        vars: vec![(ATERM_SESSION_ENV.into(), "s-ab12".into())],
                        pid: std::process::id(),
                        started_at_ms: now_ms(),
                        ..Launch::default()
                    }));
                    shared.note_predecessor(Predecessor {
                        pid: 41,
                        session_id: rt.key().to_string(),
                        updated_at_ms: now_ms() - 60_000,
                        pace: Some(pace.clone()),
                        aterm_session: Some("s-ab12".to_string()),
                        ..Predecessor::default()
                    });
                }
            }
            shared.set_pace(rt.key(), pace.clone());
            autopilot.seed_pace(&rt, None);
            let state = rt.state();
            if over {
                assert_eq!(state.go_ahead_person, None, "{start:?}: an hour over");
            } else {
                assert!(
                    near(ago_ms(state.go_ahead_person), from),
                    "{start:?}: the hour seeded: {:?}",
                    state.go_ahead_person
                );
            }
            let counted = start != Start::Relaunch;
            assert_eq!(
                state.go_aheads.acts,
                if counted { 2 } else { 0 },
                "{start:?}: the count"
            );
            if counted {
                assert!(near(ago_ms(state.go_aheads.first), first), "{start:?}");
            }
            drop(state);
            autopilot.sync_pace(&rt);
            let back = shared.pace(rt.key()).unwrap_or_default();
            if over {
                assert_eq!(
                    back.go_ahead_hour_ms, None,
                    "{start:?}: an hour over is not written"
                );
            } else {
                assert!(near(back.go_ahead_hour_ms, from), "{start:?}: {back:?}");
            }
            assert_eq!(
                back.go_ahead.map(|count| count.acts),
                counted.then_some(2),
                "{start:?}: {back:?}"
            );
        }
    }
}

/// THE ACT A WALL CUT CROSSES THE RECORD (B2): the pace a resumed thread takes up names the
/// defend prompt a wall cut and the word it was typed under; the thread's memory holds it
/// again under that word, and its record carries it on. NEGATIVE CONTROL: a cut whose word
/// names no mode is none.
#[tokio::test]
async fn a_resume_takes_up_the_act_a_wall_cut() {
    use crate::decide::Mode;
    use codex_alab_core::session::CutAct;
    use codex_alab_core::session::Pace;
    let autopilot = autopilot();
    let shared = autopilot.supervisor().shared();
    for (word, taken) in [("cruise", true), ("off", false)] {
        let (_store, resumed) = thread();
        let cut = CutAct {
            text: "Defend it.".to_string(),
            mode: word.to_string(),
        };
        shared.set_pace(
            resumed.key(),
            Pace {
                cut: Some(cut.clone()),
                ..Default::default()
            },
        );
        autopilot.seed_pace(&resumed, None);
        let state = resumed.state();
        assert_eq!(
            (state.cut.as_deref(), state.mode),
            if taken {
                (Some("Defend it."), Some(Mode::Cruise))
            } else {
                (None, None)
            },
            "{word}"
        );
        assert_eq!(
            shared.pace(resumed.key()).and_then(|pace| pace.cut),
            taken.then_some(cut),
            "{word}: carried on"
        );
    }
}

/// THE REPOSITORY'S SUFFIX OWED CROSSES THE RECORD: a done claim's act decided before the
/// repository was read waits with the open work it names, is taken for the record so and read
/// back so, and the next image probes once it is due (`decide`'s `owed_act`); a defend
/// prompt from a record that names no open work (an image from before the key) is owed the
/// suffix all the same, as that image read the repository at every owed defend prompt.
/// NEGATIVE CONTROL: any other act whose record names no open work is typed as decided.
#[tokio::test(start_paused = true)]
async fn the_repository_suffix_owed_crosses_the_record() {
    use crate::words;
    use codex_alab_core::session::OwedAct;
    let autopilot = autopilot();
    let (_store, rt) = thread();
    autopilot.keep_thread(&rt);
    let at = clock() + 5 * MIN;
    let open = Some(vec!["not landed".to_string()]);
    let nudge = crate::decide::Owed {
        native_origin: None,
        rule: words::RULE_LAND_NUDGE,
        text: "Still landing, as decided.".to_string(),
        mode: crate::decide::Mode::Land,
        not_before: at,
        repo_owed: open.clone(),
        resume_goal: false,
        landing: false,
    };
    autopilot.schedule_act(&rt, at, Some(nudge));
    assert_eq!(
        autopilot
            .supervisor()
            .shared()
            .owed_act()
            .and_then(|owed| owed.repo_owed),
        open
    );
    let (_, carried) = autopilot
        .take_owed_act()
        .await
        .unwrap_or_else(|| panic!("an act to carry"));
    assert_eq!(carried.repo_owed, open);
    let back = Autopilot::owed_of(&carried).map(|owed| owed.repo_owed);
    assert_eq!(back, Some(open));
    let legacy = OwedAct {
        rule: Some(words::RULE_DONE_CHECK.to_string()),
        repo_owed: None,
        resume_goal: false,
        ..carried.clone()
    };
    assert_eq!(
        Autopilot::owed_of(&legacy).map(|owed| owed.repo_owed),
        Some(Some(Vec::new()))
    );
    // NEGATIVE CONTROL.
    let whole = OwedAct {
        repo_owed: None,
        resume_goal: false,
        ..carried
    };
    assert_eq!(
        Autopilot::owed_of(&whole).map(|owed| owed.repo_owed),
        Some(None)
    );
}

/// THE LIVE UPGRADE CARRIES A PACED ACT'S WAIT (FOREVER CRUISE, aterm owner direction
/// 2026-10-19: one is almost always pending, so holding the upgrade on it would starve it):
/// such a wait counts as an act to carry, not a wait that holds the upgrade, and taking it for
/// the record (the old image's decision: its rule, words and mode) cancels its timer; a Land
/// landing is carried too. NEGATIVE CONTROL: any other wait (a wall's, a not-now) holds the
/// upgrade and carries nothing.
#[tokio::test(start_paused = true)]
async fn a_paced_acts_wait_is_carried_and_any_other_wait_holds_the_upgrade() {
    use crate::words;
    let autopilot = autopilot();
    let (_store, rt) = thread();
    autopilot.keep_thread(&rt);
    let at = clock() + 5 * MIN;
    autopilot.schedule_act(&rt, at, Some(owed_at(at)));
    assert_eq!(rt.owed_timer(), Some(owed_at(at)));
    let facts = autopilot.upgrade_facts().await;
    assert_eq!((facts.waits_pending, facts.carried), (false, 1));
    let before = codex_alab_core::session::now_ms();
    let (taken, owed) = autopilot
        .take_owed_act()
        .await
        .unwrap_or_else(|| panic!("an act to carry"));
    assert_eq!(taken.key(), rt.key());
    assert_eq!(owed.thread, rt.key());
    assert_eq!(owed.rule.as_deref(), Some(words::RULE_DONE_CHECK));
    assert_eq!(owed.text.as_deref(), Some("Defend it, as decided."));
    assert_eq!(owed.mode.as_deref(), Some("cruise"));
    assert!(!owed.landed);
    assert!(
        owed.not_before_ms >= before + 4 * 60_000 && owed.not_before_ms <= before + 6 * 60_000,
        "{owed:?}"
    );
    assert!(!rt.timer_pending(), "the act is the next image's now");
    // A landing is carried, with no act.
    rt.with_state(|s| s.landed = true);
    let facts = autopilot.upgrade_facts().await;
    assert_eq!((facts.waits_pending, facts.carried), (false, 1));
    let (_, owed) = autopilot
        .take_owed_act()
        .await
        .unwrap_or_else(|| panic!("a landing to carry"));
    assert_eq!((owed.rule, owed.text, owed.landed), (None, None, true));
    rt.with_state(|s| s.landed = false);
    // NEGATIVE CONTROL: a wait that is no paced act's.
    autopilot.schedule(&rt, at);
    assert_eq!(rt.owed_timer(), None);
    let facts = autopilot.upgrade_facts().await;
    assert_eq!((facts.waits_pending, facts.carried), (true, 0));
    assert!(autopilot.take_owed_act().await.is_none());
    assert!(rt.timer_pending(), "an uncarried wait is left running");
}

/// A WALL'S TRY'S WAIT HOLDS THE LIVE UPGRADE AGAIN (parity-3 review, F3, F10, F11): the
/// record carries the try for aterm's relaunch after a crash at rest, but the re-exec does not
/// take it: a measured outage's hold is this image's measure (the next image never measures
/// the route before its try, so it would wait blind for the whole hold), and an image older
/// than the record's wall tries (a rollback) reads none and would stall. So the wait holds the
/// upgrade, counts as nothing to carry, and take_owed_act leaves it on its timer, and a wall's
/// try whose timer fired holds it too until its evaluation decides it. NEGATIVE CONTROL: a
/// paced act's wait is still carried.
#[tokio::test(start_paused = true)]
async fn a_walls_try_waiting_holds_the_upgrade_and_is_not_carried() {
    use crate::words;
    let autopilot = autopilot();
    let (_store, rt) = thread();
    autopilot.keep_thread(&rt);
    let at = clock() + 15 * MIN;
    for rule in [
        words::RULE_API_RETRY,
        words::RULE_API_CUTOFF,
        words::RULE_LIMIT_RESUME,
    ] {
        let wall = crate::decide::Owed {
            rule,
            text: "Codex reported an API error; carry on.".to_string(),
            repo_owed: None,
            ..owed_at(at)
        };
        autopilot.schedule_act(&rt, at, Some(wall.clone()));
        assert_eq!(rt.owed_timer(), Some(wall), "the record still carries it");
        let facts = autopilot.upgrade_facts().await;
        assert_eq!((facts.waits_pending, facts.carried), (true, 0), "{rule}");
        assert!(autopilot.take_owed_act().await.is_none(), "{rule}");
        assert!(rt.timer_pending(), "{rule}: left on its timer");
    }
    // Its timer fired, its evaluation behind the upgrade's permit: still held.
    let permit = rt.hold_eval();
    tokio::time::advance(16 * MIN).await;
    settle().await;
    assert!(!rt.timer_pending(), "fired");
    let facts = autopilot.upgrade_facts().await;
    assert_eq!((facts.waits_pending, facts.carried), (true, 0), "fired");
    assert!(autopilot.take_owed_act().await.is_none(), "fired");
    drop(permit);
    settle().await;
    // NEGATIVE CONTROL: a paced act's wait is carried.
    let at = clock() + 5 * MIN;
    autopilot.schedule_act(&rt, at, Some(owed_at(at)));
    let facts = autopilot.upgrade_facts().await;
    assert_eq!((facts.waits_pending, facts.carried), (false, 1));
    assert!(autopilot.take_owed_act().await.is_some());
}

/// A TIMER THAT FIRED WHILE THE UPGRADE HOLDS THE THREAD'S PERMIT IS CARRIED (review of
/// 2a9241bbf, findings 5 and 9): out of its slot, its evaluation waits for the permit, and
/// the act it was about to decide is still the record's to carry; its evaluation, once it runs,
/// finds the act taken. NEGATIVE CONTROL: an evaluation that held the permit first decides
/// the act itself, and nothing is left for the record.
#[tokio::test(start_paused = true)]
async fn a_timer_that_fired_under_the_upgrades_permit_is_carried() {
    let autopilot = autopilot();
    let (_store, rt) = thread();
    autopilot.keep_thread(&rt);
    let at = clock() + 5 * MIN;
    autopilot.schedule_act(&rt, at, Some(owed_at(at)));
    let permit = rt.hold_eval();
    tokio::time::advance(6 * MIN).await;
    settle().await;
    assert!(!rt.timer_pending(), "the timer left its slot");
    assert_eq!(rt.evaluations(), 1, "its evaluation waits for the permit");
    let facts = autopilot.upgrade_facts().await;
    assert_eq!((facts.waits_pending, facts.carried), (false, 1));
    let (_, owed) = autopilot
        .take_owed_act()
        .await
        .unwrap_or_else(|| panic!("the fired act is carried"));
    assert_eq!(owed.text.as_deref(), Some("Defend it, as decided."));
    assert_eq!(rt.owed_timer(), None, "taken");
    drop(permit);
    settle().await;
    // NEGATIVE CONTROL: the evaluation holds the permit first; nothing is left to carry.
    autopilot.schedule_act(&rt, clock() + 5 * MIN, Some(owed_at(clock() + 5 * MIN)));
    tokio::time::advance(6 * MIN).await;
    settle().await;
    assert_eq!(rt.owed_timer(), None);
    assert!(autopilot.take_owed_act().await.is_none());
}

/// AN UPGRADE THAT DOES NOT GO PUTS THE OWED ACT BACK (review of 2a9241bbf, finding 23): the
/// act taken for the record waits on its thread again, at its time, as decided. NEGATIVE
/// CONTROL: a landing alone puts back no wait.
#[tokio::test(start_paused = true)]
async fn an_upgrade_that_does_not_go_puts_the_owed_act_back() {
    let autopilot = autopilot();
    let (_store, rt) = thread();
    autopilot.keep_thread(&rt);
    let at = clock() + 5 * MIN;
    autopilot.schedule_act(&rt, at, Some(owed_at(at)));
    let (taken, owed) = autopilot
        .take_owed_act()
        .await
        .unwrap_or_else(|| panic!("an act to carry"));
    assert!(!rt.timer_pending());
    autopilot.put_back(&taken, &owed);
    let back = rt.owed_timer().unwrap_or_else(|| panic!("put back"));
    assert_eq!(
        (back.rule, back.text.as_str(), back.mode),
        (
            crate::words::RULE_DONE_CHECK,
            "Defend it, as decided.",
            crate::decide::Mode::Cruise
        )
    );
    assert!(
        back.not_before <= at + Duration::from_secs(1)
            && back.not_before + Duration::from_secs(1) >= at,
        "{back:?}"
    );
    // NEGATIVE CONTROL: a landing alone.
    Autopilot::cancel_timer(&rt).await;
    let landed = codex_alab_core::session::OwedAct {
        thread: rt.key().to_string(),
        landed: true,
        ..codex_alab_core::session::OwedAct::default()
    };
    autopilot.put_back(&rt, &landed);
    assert!(!rt.timer_pending());
}

/// The act the previous image carried across the re-exec goes to its own thread's first
/// resume only, its rule, words and mode read back and its time turned into this image's
/// clock; the thread a re-exec resumed with nothing owed is handed over too (its needs are
/// restored from its history). NEGATIVE CONTROLS: another thread's resume gets nothing (and
/// spends nothing); a second resume gets nothing; a rule that is neither a paced act's nor a
/// wall's try's (a compaction's), or a record without the words, carries no act.
#[tokio::test(start_paused = true)]
async fn the_carried_act_goes_to_its_threads_first_resume_only() {
    use crate::words;
    use codex_alab_core::session::Inherited;
    use codex_alab_core::session::OwedAct;
    let autopilot = autopilot();
    let (_store, rt) = thread();
    let shared = autopilot.supervisor().shared();
    assert!(shared.set_inherited(Inherited {
        owed_act: Some(OwedAct {
            renewal: None,
            native_origin: None,
            thread: rt.key().to_string(),
            rule: Some(words::RULE_LAND_NUDGE.to_string()),
            text: Some(words::LAND_NUDGE.to_string()),
            mode: Some("land".to_string()),
            not_before_ms: codex_alab_core::session::now_ms() + 60_000,
            repo_owed: None,
            resume_goal: false,
            landing: false,
            landed: false,
        }),
        ..Inherited::default()
    }));
    assert_eq!(autopilot.inherited_owed("019a-other"), None);
    let now = clock();
    let (owed, landed) = autopilot
        .inherited_owed(rt.key())
        .unwrap_or_else(|| panic!("handed over"));
    let owed = owed.unwrap_or_else(|| panic!("an act"));
    assert_eq!(
        (owed.rule, owed.text.as_str(), owed.mode),
        (
            words::RULE_LAND_NUDGE,
            words::LAND_NUDGE,
            crate::decide::Mode::Land
        )
    );
    assert!(!landed);
    assert!(
        owed.not_before > now + Duration::from_secs(50)
            && owed.not_before <= now + Duration::from_secs(61),
        "{owed:?}"
    );
    assert_eq!(autopilot.inherited_owed(rt.key()), None, "spent");
    // A rule that is neither a paced act's nor a wall's try's, or no words: no act carried.
    for (rule, text) in [
        (words::RULE_COMPACT, Some("/compact")),
        (words::RULE_CONTINUE, None),
    ] {
        let autopilot = self::autopilot();
        let shared = autopilot.supervisor().shared();
        assert!(shared.set_inherited(Inherited {
            owed_act: Some(OwedAct {
                renewal: None,
                native_origin: None,
                thread: rt.key().to_string(),
                rule: Some(rule.to_string()),
                text: text.map(str::to_string),
                mode: Some("cruise".to_string()),
                not_before_ms: 0,
                repo_owed: None,
                resume_goal: false,
                landing: false,
                landed: true,
            }),
            ..Inherited::default()
        }));
        assert_eq!(autopilot.inherited_owed(rt.key()), Some((None, true)));
    }
    // The thread a re-exec resumed with nothing to carry.
    let autopilot = self::autopilot();
    let shared = autopilot.supervisor().shared();
    assert!(shared.set_inherited(Inherited {
        session_id: Some(rt.key().to_string()),
        ..Inherited::default()
    }));
    assert_eq!(autopilot.inherited_owed("019a-other"), None);
    assert_eq!(autopilot.inherited_owed(rt.key()), Some((None, false)));
}

/// THE RECORD'S NEEDS OFF AND FOLDER REACH THE RESUMED THREAD (review of the forever
/// autopilot, 2026-10-20): at the re-exec the previous image's record (its session the
/// thread's) names the set an Off took down and the folder the worker last worked in; at
/// aterm's relaunch the dead process's record does. The folder seeds the thread's, and this
/// process's record carries it on. NEGATIVE CONTROLS: another session's record carries
/// nothing to the thread; a resume that is neither re-exec nor relaunch reads no set; a
/// folder the thread already has is kept.
#[tokio::test]
async fn the_records_needs_off_and_folder_reach_the_resumed_thread() {
    use codex_alab_core::session::Inherited;
    use codex_alab_core::session::Predecessor;
    let repo = std::path::PathBuf::from("/work/repo");
    let id = ThreadId::new();
    let rt_of = |id: ThreadId| {
        let store = ExtensionData::new(id.to_string());
        store.insert(ThreadRt::new(id, None, false, Duration::ZERO));
        let rt = store.get::<ThreadRt>().expect("inserted");
        (store, rt)
    };
    let (_store, rt) = rt_of(id);
    let autopilot = autopilot();
    let shared = autopilot.supervisor().shared();
    assert!(shared.set_inherited(Inherited {
        session_id: Some(rt.key().to_string()),
        needs_off: 7,
        workdir: Some(repo.clone()),
        ..Inherited::default()
    }));
    assert_eq!(autopilot.carried_needs_off(rt.key(), true, false), 7);
    assert_eq!(autopilot.carried_needs_off(rt.key(), false, false), 0);
    assert_eq!(autopilot.carried_needs_off("019a-other", true, false), 0);
    autopilot.seed_workdir(&rt);
    assert_eq!(rt.workdir(), Some(repo.clone()));
    assert_eq!(shared.workdir(rt.key()), Some(repo.clone()));
    // A folder the thread has is kept.
    shared.set_workdir(rt.key(), std::path::Path::new("/work/later"));
    autopilot.seed_workdir(&rt);
    assert_eq!(rt.workdir(), Some(repo.clone()));
    // NEGATIVE CONTROL: another thread finds nothing in this record.
    let (_other_store, other) = thread();
    autopilot.seed_workdir(&other);
    assert_eq!(other.workdir(), None);

    // aterm's relaunch: the dead process's record.
    let autopilot = self::autopilot();
    let shared = autopilot.supervisor().shared();
    shared.note_predecessor(Predecessor {
        session_id: rt.key().to_string(),
        needs_off: 9,
        workdir: Some(repo.clone()),
        ..Predecessor::default()
    });
    assert_eq!(autopilot.carried_needs_off(rt.key(), false, true), 9);
    assert_eq!(autopilot.carried_needs_off(rt.key(), true, true), 9);
    assert_eq!(autopilot.carried_needs_off(rt.key(), false, false), 0);
    assert_eq!(autopilot.carried_needs_off(other.key(), false, true), 0);
    let (_fresh_store, fresh) = rt_of(id);
    autopilot.seed_workdir(&fresh);
    assert_eq!(fresh.workdir(), Some(repo));

    // A crash relaunch carrying a provider Recovery (`Relaunch::Recovery`) also keeps a
    // still-down `needs_off` in `Shared::needs_off` so the relaunched process's record
    // preserves `needsOff` across a subsequent re-exec or relaunch.
    let dismissed_key = crate::needs::needs_key(&["old approval".to_string()]);
    for (carried_off, want_down) in [(dismissed_key, true), (999, false)] {
        let autopilot = self::autopilot();
        let shared = autopilot.supervisor().shared();
        let (_recovery_store, recovery_rt) = rt_of(id);
        autopilot.keep_thread(&recovery_rt);
        let started = codex_alab_core::session::now_ms() - 1_000;
        let written_at = started - 5_000;
        let line = vec![
            "/twin/codex".to_string(),
            "resume".into(),
            recovery_rt.key().to_string(),
        ];
        shared.set_launch(codex_alab_core::session::Launch {
            args: line.iter().map(std::ffi::OsString::from).collect(),
            started_at_ms: started,
            ..Default::default()
        });
        shared.note_predecessor(Predecessor {
            pid: 77,
            session_id: recovery_rt.key().to_string(),
            exit: Some("signal:15".into()),
            resume_argv: Some(line),
            status: Some("idle".into()),
            updated_at_ms: written_at,
            needs_off: carried_off,
            recovery: Some(codex_alab_core::session::Recovery {
                thread: recovery_rt.key().to_string(),
                turn_id: "walled".into(),
                point_at_ms: written_at,
                not_before_ms: started + 600_000,
                by_autopilot: true,
                by_person: false,
                wall: codex_alab_core::session::RecoveryWall::Network {
                    what: "an API error (the API could not be reached)".into(),
                    measured: false,
                },
            }),
            ..Predecessor::default()
        });
        shared.set_swept();
        shared.set_launch_witness(Some(codex_alab_core::session::LaunchWitness {
            hand: Some("lease:aterm-harness@812".into()),
            human: codex_alab_core::session::HumanSeen::Never,
            read_at_ms: started + 1,
            server_pid: Some(812),
            hand_pid: codex_alab_core::session::HandPid::Absent,
        }));
        let event = |value: serde_json::Value| {
            codex_history::RolloutItem::EventMsg(
                serde_json::from_value::<codex_protocol::protocol::EventMsg>(value)
                    .expect("native event"),
            )
        };
        autopilot.set_history(vec![
            event(serde_json::json!({
                "type": "task_started",
                "turn_id": "earlier",
                "model_context_window": 258400,
                "collaboration_mode_kind": "default",
                "turn_attribution": {"turn_id": "earlier", "turn_trigger": "user"}
            })),
            event(serde_json::json!({
                "type": "task_complete",
                "turn_id": "earlier",
                "last_agent_message": "NEEDS YOU: old approval"
            })),
            event(serde_json::json!({
                "type": "task_started",
                "turn_id": "walled",
                "model_context_window": 258400,
                "collaboration_mode_kind": "default",
                "turn_attribution": {"turn_id": "walled", "turn_trigger": "alab_autopilot"}
            })),
            event(serde_json::json!({
                "type": "task_complete",
                "turn_id": "walled",
                "last_agent_message": null,
                "error": {
                    "message": "the API could not be reached",
                    "codex_error_info": codex_protocol::protocol::CodexErrorInfo::HttpConnectionFailed {
                        http_status_code: None
                    }
                }
            })),
        ]);
        Arc::clone(&autopilot)
            .resume_scan(Arc::clone(&recovery_rt))
            .await;
        assert!(recovery_rt.state().point.is_some());
        if want_down {
            assert!(recovery_rt.state().needs.standing.is_empty());
            assert_eq!(shared.needs_off(), dismissed_key);
        } else {
            assert_eq!(recovery_rt.state().needs.key, dismissed_key);
            assert_eq!(shared.needs_off(), 0);
        }
    }
}

/// THE UPGRADE LOOKS AGAIN AFTER AN EVALUATION IT MET (review of 2a9241bbf, findings 5 and 9,
/// whose relook went untested until the review of the forever autopilot, 2026-10-20; and that
/// review's finding 4): an evaluation holding a thread's permit, or a point none has decided
/// yet (one queued for the permit), holds the upgrade, which looks again
/// [`UPGRADE_RELOOK`] later. NEGATIVE CONTROL: with neither, the upgrade's wait (here: no
/// thread manager) sets no look again.
#[tokio::test(start_paused = true)]
async fn the_upgrade_looks_again_after_an_evaluation_it_met() {
    use crate::extension::UPGRADE_RELOOK;
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let autopilot = autopilot();
    let (_store, rt) = thread();
    autopilot.keep_thread(&rt);
    autopilot.supervisor().shared().observe(
        &codex_alab_core::session::Frame {
            primary_thread: Some(rt.key()),
            displayed_thread: Some(rt.key()),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: home.path(),
            embedded: true,
            state: codex_alab_core::session::ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        codex_alab_core::session::now_ms(),
    );
    let deciding = format!("upgrade waits: {} is deciding", rt.key());
    let noted = |needle: &str| {
        autopilot
            .supervisor()
            .journal()
            .iter()
            .filter(|l| l.contains(needle))
            .count()
    };
    // NEGATIVE CONTROL: no point, no evaluation.
    assert!(!autopilot.try_upgrade().await);
    assert!(!autopilot.upgrade_relook_pending());
    // An evaluation holds the permit.
    let permit = rt.hold_eval();
    assert!(!autopilot.try_upgrade().await);
    assert_eq!(noted(&deciding), 1);
    assert!(autopilot.upgrade_relook_pending());
    drop(permit);
    tokio::time::advance(UPGRADE_RELOOK - Duration::from_millis(100)).await;
    settle().await;
    assert!(autopilot.upgrade_relook_pending(), "not before its time");
    tokio::time::advance(Duration::from_millis(200)).await;
    settle().await;
    assert!(!autopilot.upgrade_relook_pending(), "looked again");
    // A point no evaluation has decided.
    rt.with_state(|s| {
        s.task = true;
        s.point = Some(crate::decide::TurnEnd {
            at: clock(),
            by_autopilot: true,
            by_person: false,
            text: Some("Stage 2 is in; stage 3 is next.".to_string()),
            earlier: None,
            questions: false,
            worked: Duration::from_secs(600),
            wall: None,
            aborted: false,
            ran_tools: false,
            restored: false,
        });
    });
    assert!(rt.undecided());
    assert_eq!(
        autopilot.upgrade_facts().await.undecided.as_deref(),
        Some(rt.key())
    );
    assert!(!autopilot.try_upgrade().await);
    assert_eq!(noted(&deciding), 1, "no evaluation held the permit");
    assert!(autopilot.upgrade_relook_pending());
}

// A failed initial sample is transient on a platform that counts sleep. Drive the
// actual timer, without a socket, through either an old wake or a genuinely new one.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
async fn timer_after_an_initial_unread_sample(historical: bool) -> (u64, u64) {
    use codex_alab_core::wake::POLL;
    use codex_alab_core::wake::Reading;
    use codex_alab_core::wake::Wake;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    let pilot = autopilot();
    let (_store, rt) = thread();
    let start = tokio::time::Instant::now();
    let first = AtomicBool::new(true);
    let (sampled, baseline) = tokio::sync::oneshot::channel();
    let first_valid = std::sync::Mutex::new(Some(sampled));
    let woke_at = if historical {
        start - Duration::from_secs(10)
    } else {
        start + Duration::from_secs(10)
    };
    pilot.set_wake_read(Arc::new(move || {
        if first.swap(false, Ordering::SeqCst) {
            return None;
        }
        let woke = historical || tokio::time::Instant::now() >= woke_at;
        let reading = Reading {
            asleep: Duration::from_secs(if woke { 5120 } else { 5000 }),
            last: woke.then_some(Wake {
                at: woke_at.into_std(),
                slept: Duration::from_secs(120),
                seq: 1,
            }),
        };
        if let Some(sampled) = first_valid
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            sampled
                .send((tokio::time::Instant::now(), reading))
                .expect("the first valid sample observer remains");
        }
        Some(reading)
    }));
    pilot.schedule(&rt, clock() + Duration::from_secs(60));
    settle().await;
    // The first valid sample establishes a baseline; it cannot prove sleep from
    // before it. Positively observe it before the scripted new wake, rather than
    // jumping the paused clock over both events in one advance.
    tokio::time::advance(POLL).await;
    settle().await;
    let (sampled_at, reading) = tokio::time::timeout(Duration::from_secs(60), baseline)
        .await
        .expect("first valid sample hang detector")
        .expect("the actual timer sampled after its initial unread result");
    assert_eq!(
        reading.asleep,
        Duration::from_secs(if historical { 5120 } else { 5000 }),
        "the first valid sample is the timer's initial cumulative baseline"
    );
    assert!(historical || sampled_at < woke_at);
    tokio::time::advance(
        (start + Duration::from_secs(10)).saturating_duration_since(tokio::time::Instant::now()),
    )
    .await;
    settle().await;
    tokio::time::advance(Duration::from_secs(3)).await;
    settle().await;
    let early = rt.evaluations();
    tokio::time::advance(Duration::from_secs(60)).await;
    settle().await;
    (early, rt.evaluations())
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
#[tokio::test(start_paused = true)]
async fn an_initial_unread_sample_does_not_disable_later_wake_polling() {
    assert_eq!(timer_after_an_initial_unread_sample(false).await, (1, 1));
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
#[tokio::test(start_paused = true)]
async fn an_initial_unread_sample_does_not_credit_a_historical_wake() {
    assert_eq!(timer_after_an_initial_unread_sample(true).await, (0, 1));
}

// A pre-arm sleep can be first discovered after a rejected arm sample. Use the
// actual wake-state stamping rather than a prebuilt Wake with an old timestamp.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
async fn timer_after_unobserved_pre_arm_sleep(later_sleep: bool) -> (u64, u64, u64, Option<u64>) {
    use codex_alab_core::wake::Reading;
    use codex_alab_core::wake::WakeState;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    let pilot = autopilot();
    let (_store, rt) = thread();
    let start = tokio::time::Instant::now();
    let mut state = WakeState::default();
    assert_eq!(
        state.observe(
            (start - Duration::from_secs(10)).into_std(),
            Duration::from_secs(5000)
        ),
        None
    );
    let state = Arc::new(Mutex::new(state));
    let reader = Arc::clone(&state);
    let first = AtomicBool::new(true);
    pilot.set_wake_read(Arc::new(move || {
        if first.swap(false, Ordering::SeqCst) {
            return None;
        }
        let now = tokio::time::Instant::now();
        let asleep =
            Duration::from_secs(if later_sleep && now >= start + Duration::from_secs(10) {
                5240
            } else {
                5120
            });
        let mut state = reader
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Some(Reading {
            asleep,
            last: state.observe(now.into_std(), asleep),
        })
    }));
    pilot.schedule(&rt, clock() + Duration::from_secs(60));
    settle().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    settle().await;
    tokio::time::advance(Duration::from_secs(3)).await;
    settle().await;
    let early = rt.evaluations();
    tokio::time::advance(Duration::from_secs(6)).await;
    settle().await;
    tokio::time::advance(Duration::from_secs(3)).await;
    settle().await;
    let after_later_sleep = rt.evaluations();
    tokio::time::advance(Duration::from_secs(60)).await;
    settle().await;
    let last_seq = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .last()
        .map(|wake| wake.seq);
    (early, after_later_sleep, rt.evaluations(), last_seq)
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
#[tokio::test(start_paused = true)]
async fn an_unread_timer_arm_does_not_credit_sleep_first_discovered_after_arm() {
    assert_eq!(
        timer_after_unobserved_pre_arm_sleep(false).await,
        (0, 0, 1, Some(1))
    );
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
#[tokio::test(start_paused = true)]
async fn an_unread_timer_arm_still_counts_sleep_after_its_first_valid_baseline() {
    assert_eq!(
        timer_after_unobserved_pre_arm_sleep(true).await,
        (0, 1, 1, Some(2))
    );
}
