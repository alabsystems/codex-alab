// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A native acceptance or stop wins while an asynchronous resume scan waits on startup.

use super::*;
use codex_alab_aterm::Options as ClientOptions;
use codex_alab_aterm::locate::Endpoint;
use codex_alab_core::session::Inherited;
use codex_alab_core::session::OwedAct;
use codex_alab_core::session::Pace;
use codex_alab_core::session::Shared;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;

fn fixture(
    endpoint: Arc<dyn Fn() -> Option<Endpoint> + Send + Sync>,
) -> (
    Arc<Autopilot>,
    &'static Shared,
    ExtensionData,
    Arc<ThreadRt>,
) {
    codex_alab_core::activate();
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let sup = Supervisor::new(crate::SupervisorOptions {
        shared,
        endpoint,
        client: ClientOptions::default(),
    });
    let autopilot = Autopilot::new(sup, None, None, Timing::default(), |_| true);
    let id = ThreadId::new();
    let store = ExtensionData::new(id.to_string());
    store.insert(ThreadRt::new(id, None, false, Duration::ZERO));
    let rt = store.get::<ThreadRt>().expect("native runtime");
    autopilot.keep_thread(&rt);
    rt.resumed.store(true, Ordering::SeqCst);
    rt.with_state(|state| {
        state.task = true;
        state.short_streak = 2;
        state.needs.standing = vec!["the current native turn's need".into()];
        state.needs.key = 42;
    });
    let owed = OwedAct {
        thread: rt.key.clone(),
        rule: Some(words::RULE_CONTINUE.into()),
        text: Some("obsolete inherited words".into()),
        mode: Some("cruise".into()),
        not_before_ms: now_ms() + 60_000,
        ..Default::default()
    };
    shared.set_inherited(Inherited {
        session_id: Some(rt.key.clone()),
        owed_act: Some(owed.clone()),
        needs_off: 777,
        ..Default::default()
    });
    shared.set_owed_act(Some(owed));
    shared.set_pace(
        &rt.key,
        Pace {
            short_streak: 99,
            ..Default::default()
        },
    );
    shared.note_predecessor(codex_alab_core::session::Predecessor {
        session_id: rt.key.clone(),
        carried_on: 1,
        ..Default::default()
    });
    (autopilot, shared, store, rt)
}

async fn accepted(autopilot: &Autopilot, store: &ExtensionData) {
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5".into(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    autopilot
        .on_turn_start(TurnStartInput {
            turn_id: "new-native-turn",
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &ExtensionData::new("session"),
            thread_store: store,
            turn_store: &ExtensionData::new("new-native-turn"),
        })
        .await;
}

#[tokio::test]
async fn a_native_start_or_stop_ends_the_inherited_hold_before_a_late_resume() {
    codex_alab_core::activate();
    for end in ["current", "accepted", "stopped"] {
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let supervisor = Supervisor::new(crate::SupervisorOptions {
            shared,
            endpoint: Arc::new(|| None),
            client: ClientOptions::default(),
        });
        let autopilot = Autopilot::new(supervisor, None, None, Timing::default(), |_| true);
        let id = ThreadId::new();
        let store = ExtensionData::new(id.to_string());
        store.insert(ThreadRt::new(id, None, false, Duration::ZERO));
        let rt = store.get::<ThreadRt>().unwrap();
        autopilot.keep_thread(&rt);
        rt.resumed.store(true, Ordering::SeqCst);
        autopilot.set_wake_read(Arc::new(|| {
            Some(codex_alab_core::wake::Reading {
                asleep: Duration::from_secs(5000),
                last: None,
            })
        }));
        let history = [
            serde_json::json!({"type":"task_started", "turn_id":"carried-native", "model_context_window":258400,
                "collaboration_mode_kind":"default", "turn_attribution":{"turn_id":"carried-native", "turn_trigger":"user"}}),
            serde_json::json!({"type":"user_message", "message":"the carried fixture task", "images":null,
                "local_images":[], "text_elements":[]}),
            serde_json::json!({"type":"task_complete", "turn_id":"carried-native", "last_agent_message":"work remains"}),
        ].map(|value| codex_history::RolloutItem::EventMsg(
            serde_json::from_value::<codex_protocol::protocol::EventMsg>(value).unwrap()
        )).to_vec();
        let carried = OwedAct {
            thread: rt.key.clone(),
            native_origin: Some(codex_alab_core::session::NativeOrigin {
                turn_id: "carried-native".into(),
                phase: codex_alab_core::session::NativePhase::Complete,
            }),
            rule: Some(words::RULE_CONTINUE.into()),
            text: Some("keep going".into()),
            mode: Some("cruise".into()),
            not_before_ms: now_ms() + 60_000,
            ..Default::default()
        };
        assert!(
            crate::owed_origin::record(&carried, &crate::resume_history::latest(&history), None)
                .unwrap()
                .carries_automatic_intent()
        );
        assert!(shared.set_inherited(Inherited {
            session_id: Some(rt.key.clone()),
            owed_act: Some(carried),
            ..Default::default()
        }));
        autopilot.set_history(history);
        if end != "current" {
            *lock(&rt.inherited_settle) = InheritedSettle::new(clock(), None, true);
            assert!(lock(&rt.inherited_settle).is_some());
        }
        if end == "stopped" {
            autopilot
                .on_thread_stop(ThreadStopInput {
                    session_store: &ExtensionData::new("session"),
                    thread_store: &store,
                })
                .await;
        } else if end == "accepted" {
            accepted(&autopilot, &store).await;
        }
        assert!(lock(&rt.inherited_settle).is_none());
        Arc::clone(&autopilot).resume_scan(Arc::clone(&rt)).await;
        assert_eq!(
            lock(&rt.inherited_settle).is_some(),
            end == "current",
            "only the otherwise-admissible scan can seed; a native boundary cannot be undone: {end}"
        );
        autopilot
            .on_thread_stop(ThreadStopInput {
                session_store: &ExtensionData::new("cleanup"),
                thread_store: &store,
            })
            .await;
    }
}

#[tokio::test]
async fn a_turn_accepted_before_the_resume_task_is_polled_spends_the_old_handoff() {
    let (autopilot, shared, store, rt) = fixture(Arc::new(|| None));
    accepted(&autopilot, &store).await;
    let before = rt.state();
    Arc::clone(&autopilot).resume_scan(Arc::clone(&rt)).await;
    assert_eq!(rt.state(), before);
    assert!(rt.scanned.load(Ordering::SeqCst));
    assert!(shared.relaunch_spent());
    assert_eq!(shared.owed_act(), None);
    assert_eq!(
        shared.carried_on(),
        0,
        "an old process cannot replace the accepted native turn's reset"
    );
    assert!(
        autopilot.inherited_owed(&rt.key).is_none(),
        "a later fresh /resume cannot revive the old image's words"
    );
}

#[tokio::test]
async fn first_resume_reserves_authority_before_poll_and_across_registry_replacement() {
    let (autopilot, shared, _store, rt) = fixture(Arc::new(|| None));
    let first = Arc::clone(&autopilot).resume_scan(Arc::clone(&rt));
    assert!(!shared.relaunch_spent(), "reservation is not a verdict");
    let replacement = Autopilot::new(
        Arc::clone(&autopilot.sup),
        None,
        None,
        Timing::default(),
        |_| true,
    );
    // Constructing a later registry's scan cannot reserve the same Shared again. Cancelling
    // the first future fails closed rather than donating its launch to a later /resume.
    let later = Arc::clone(&replacement).resume_scan(Arc::clone(&rt));
    assert!(!shared.reserve_relaunch());
    drop(first);
    rt.stopped.store(true, Ordering::SeqCst);
    shared.set_swept();
    shared.set_launch_witness(None);
    later.await;
    assert!(
        !shared.relaunch_spent(),
        "the later scan had no launch verdict to spend"
    );
    assert!(
        !replacement.owed_spent.load(Ordering::SeqCst),
        "a later stopped scan cannot consume the reserved owner's inherited handoff"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_sweep_wait_keeps_the_first_resume_pending_without_spending_it() {
    let (seen, arrived) = tokio::sync::oneshot::channel();
    let seen = Mutex::new(Some(seen));
    let looks = AtomicU64::new(0);
    let endpoint = Arc::new(move || {
        // The second loop check follows an expired zero-duration wait: an event, not
        // a real five-second sleep or a scheduler-speed assertion.
        if looks.fetch_add(1, Ordering::SeqCst) == 1
            && let Some(seen) = lock(&seen).take()
        {
            let _ = seen.send(());
        }
        Some(Endpoint::explicit("/never-connected", "s-1"))
    });
    let (autopilot, shared, _store, rt) = fixture(endpoint);
    assert!(shared.reserve_relaunch());
    let scan = tokio::spawn(Arc::clone(&autopilot).resume_scan_owned(
        Arc::clone(&rt),
        true,
        Duration::ZERO,
    ));
    tokio::time::timeout(Duration::from_secs(60), arrived)
        .await
        .expect("sweep wait hang detector")
        .expect("the first bounded wait expired");
    assert!(!rt.scanned.load(Ordering::SeqCst));
    assert!(!shared.relaunch_spent());
    assert!(!scan.is_finished());
    shared.set_launch_witness(None);
    shared.set_swept();
    tokio::time::timeout(Duration::from_secs(60), scan)
        .await
        .expect("settled scan hang detector")
        .expect("scan task");
    assert!(rt.scanned.load(Ordering::SeqCst));
    assert!(
        shared.relaunch_spent(),
        "settled absence is a final verdict"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_acceptance_or_stop_during_the_startup_wait_supersedes_the_resume_scan() {
    for stop in [false, true] {
        let (seen, arrived) = tokio::sync::oneshot::channel();
        let seen = Mutex::new(Some(seen));
        let endpoint = Arc::new(move || {
            if let Some(seen) = lock(&seen).take() {
                let _ = seen.send(());
            }
            Some(Endpoint::explicit("/never-connected", "s-1"))
        });
        let (autopilot, shared, store, rt) = fixture(endpoint);
        let mut before = rt.state();
        let scan = tokio::spawn(Arc::clone(&autopilot).resume_scan(Arc::clone(&rt)));
        tokio::time::timeout(Duration::from_secs(60), arrived)
            .await
            .expect("startup wait reached")
            .expect("scan sampled its initial acceptance epoch");
        if stop {
            autopilot
                .on_thread_stop(ThreadStopInput {
                    session_store: &ExtensionData::new("session"),
                    thread_store: &store,
                })
                .await;
        } else {
            accepted(&autopilot, &store).await;
            before.native_origin = Some(codex_alab_core::session::NativeOrigin {
                turn_id: "new-native-turn".into(),
                phase: codex_alab_core::session::NativePhase::Running,
            });
        }
        shared.set_swept();
        shared.set_launch_witness(None);
        tokio::time::timeout(Duration::from_secs(60), scan)
            .await
            .expect("resume scan hang detector")
            .expect("scan task");
        assert_eq!(
            rt.state(),
            before,
            "stop={stop}: startup state cannot replace native state or needs"
        );
        assert!(rt.scanned.load(Ordering::SeqCst));
        assert_eq!(shared.owed_act(), None);
        assert_eq!(
            shared.carried_on(),
            0,
            "stop={stop}: the relaunch helper cannot import old count after the native boundary"
        );
        assert!(
            autopilot.inherited_owed(&rt.key).is_none(),
            "stop={stop}: the old image's handoff was spent"
        );
        assert!(
            !shared
                .seed_before_native_work(|| panic!("a late writer revived old startup metadata"))
        );
    }
}

/// A RESTORED NETWORK WALL WHOSE ROUTE WAS MEASURED IS MEASURED AGAIN (B2): the record's
/// recovery names a network wall the process before measured (`measured`), and the resume
/// scan that restores its point reads the route again from the thread's config, so the wait
/// measures the API's reach and a measured return ends the hold a measured outage decided.
/// Before, the restored wall had no route, nothing measured it, and the relaunch waited the
/// whole hold blind. NEGATIVE CONTROL: a wall whose route was not measured (a TLS or proxy
/// refusal's, or a record before the key) gets none.
#[tokio::test]
async fn a_restored_measured_network_wall_measures_its_route_again() {
    use codex_alab_core::session::Recovery;
    use codex_alab_core::session::RecoveryWall;
    use codex_protocol::protocol::CodexErrorInfo;
    for measured in [true, false] {
        codex_alab_core::activate();
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let sup = Supervisor::new(crate::SupervisorOptions {
            shared,
            endpoint: Arc::new(|| None),
            client: ClientOptions::default(),
        });
        let autopilot = Autopilot::new(sup, None, None, Timing::default(), |_| true);
        // Every route's proxy decision read as direct; nothing here completes a handshake.
        assert!(autopilot.set_net_probe(
            Arc::new(|| Err(std::io::Error::other("no trust in this test"))),
            netwatch::NetTiming::default(),
        ));
        let home = tempfile::tempdir().expect("scratch codex home");
        let mut config = core_test_support::load_default_config_for_test(&home).await;
        config.model_provider.base_url = Some("https://127.0.0.1:9/v1".to_string());
        let id = ThreadId::new();
        let store = ExtensionData::new(id.to_string());
        store.insert(ThreadRt::new(
            id,
            Some(Arc::new(config)),
            false,
            Duration::ZERO,
        ));
        let rt = store.get::<ThreadRt>().expect("native runtime");
        autopilot.keep_thread(&rt);
        rt.resumed.store(true, Ordering::SeqCst);
        let event = |value: serde_json::Value| {
            codex_history::RolloutItem::EventMsg(
                serde_json::from_value::<codex_protocol::protocol::EventMsg>(value)
                    .expect("native event"),
            )
        };
        autopilot.set_history(vec![
            event(
                serde_json::json!({"type":"task_started", "turn_id":"walled",
                "model_context_window":258400, "collaboration_mode_kind":"default",
                "turn_attribution":{"turn_id":"walled", "turn_trigger":"user"}}),
            ),
            event(
                serde_json::json!({"type":"task_complete", "turn_id":"walled",
                "last_agent_message":null, "error":{"message":"the API could not be reached",
                "codex_error_info":CodexErrorInfo::HttpConnectionFailed {
                    http_status_code: None
                }}}),
            ),
        ]);
        shared.set_inherited(Inherited {
            session_id: Some(rt.key.clone()),
            recovery: Some(Recovery {
                thread: rt.key.clone(),
                turn_id: "walled".into(),
                point_at_ms: now_ms() - 60_000,
                not_before_ms: now_ms() + 14 * 60_000,
                by_autopilot: true,
                by_person: false,
                wall: RecoveryWall::Network {
                    what: "an API error (the API could not be reached)".into(),
                    measured,
                },
            }),
            ..Default::default()
        });
        Arc::clone(&autopilot).resume_scan(Arc::clone(&rt)).await;
        assert!(
            rt.state()
                .point
                .is_some_and(|point| matches!(point.wall, Some(Wall::Network { .. }))),
            "measured={measured}: the recovery restored the wall's point"
        );
        assert_eq!(
            lock(&rt.net).clone(),
            measured.then(|| netwatch::Route::Direct(netwatch::Dest {
                host: "127.0.0.1".into(),
                port: 9,
            })),
            "measured={measured}"
        );
    }
}

/// A restored epoch deadline already includes earlier sleep. Its fresh baseline
/// must precede a route-rearm await, so only sleep during that await is credited.
#[tokio::test]
async fn restored_deadlines_begin_counting_sleep_at_materialization() {
    use codex_alab_core::session::Recovery;
    use codex_alab_core::session::RecoveryWall;
    use codex_alab_core::wake::Reading;
    use codex_alab_core::wake::Wake;
    tokio::time::timeout(Duration::from_secs(60), async {
        codex_alab_core::activate();
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let sup = Supervisor::new(crate::SupervisorOptions {
            shared,
            endpoint: Arc::new(|| None),
            client: ClientOptions::default(),
        });
        let autopilot = Autopilot::new(sup, None, None, Timing::default(), |_| true);
        let id = ThreadId::new();
        let store = ExtensionData::new(id.to_string());
        store.insert(ThreadRt::new(id, None, false, Duration::ZERO));
        let rt = store.get::<ThreadRt>().expect("resumed runtime");
        autopilot.keep_thread(&rt);
        rt.resumed.store(true, Ordering::SeqCst);
        let base = Duration::from_secs(5000);
        let slept = Duration::from_secs(7200);
        rt.with_state(|state| state.asleep_mark = Some(base));
        let reading = Arc::new(Mutex::new(Reading { asleep: base + slept, last: None }));
        let read = Arc::clone(&reading);
        autopilot.set_wake_read(Arc::new(move || Some(*lock(&read))));
        let event = |value: serde_json::Value| {
            codex_history::RolloutItem::EventMsg(serde_json::from_value::<codex_protocol::protocol::EventMsg>(value).expect("native event"))
        };
        autopilot.set_history(vec![
            event(serde_json::json!({"type":"task_started", "turn_id":"usage-turn", "model_context_window":258400,
                "collaboration_mode_kind":"default", "turn_attribution":{"turn_id":"usage-turn", "turn_trigger":"user"}})),
            event(serde_json::json!({"type":"task_complete", "turn_id":"usage-turn", "last_agent_message":null,
                "error":{"message":"usage window is spent", "codex_error_info":"usage_limit_exceeded"}})),
        ]);
        let epoch = now_ms();
        shared.set_inherited(Inherited {
            session_id: Some(rt.key.clone()),
            recovery: Some(Recovery {
                thread: rt.key.clone(), turn_id: "usage-turn".into(), point_at_ms: epoch - 60_000,
                not_before_ms: epoch + 3_600_000, by_autopilot: true, by_person: false,
                wall: RecoveryWall::Usage { quota: None, reset_at_ms: Some(epoch + 3_600_000), reset_label: Some("12:00".into()) },
            }),
            ..Default::default()
        });
        let (entered, reached) = tokio::sync::oneshot::channel();
        let (release, resume) = tokio::sync::oneshot::channel();
        *lock(&autopilot.before_rearm_route) = Some((entered, resume));
        let scan = Arc::clone(&autopilot).resume_scan(Arc::clone(&rt));
        tokio::pin!(scan);
        tokio::select! {
            reached = reached => reached.expect("the restored usage point reached its actual rearm await"),
            () = &mut scan => panic!("the actual resume scan must await the held route"),
        }
        let reset = |state: &ThreadState| match state.point.as_ref().and_then(|point| point.wall.as_ref()) {
            Some(Wall::Usage { reset_at: Some(reset), .. }) => *reset,
            wall => panic!("the exact usage recovery must have restored: {wall:?}"),
        };
        let original = reset(&rt.state());
        assert!(original > clock() + Duration::from_secs(1800), "a reset restored after old sleep keeps its fresh wait");
        assert_eq!(rt.state().asleep_mark, Some(base + slept), "materialization replaces the older point's sleep mark");
        rt.with_state(|state| {
            assert_eq!(state.slept_to(Some(base + slept), clock()), None);
            assert_eq!(reset(state), original, "old sleep cannot be credited again");
        });
        *lock(&reading) = Reading {
            asleep: base + slept + slept,
            last: Some(Wake { at: clock(), slept, seq: 1 }),
        };
        release.send(()).expect("release the actual route rearm");
        scan.await;
        assert!(rt.scanned.load(Ordering::SeqCst));
        rt.with_state(|state| {
            assert_eq!(state.slept_to(Some(base + slept + slept), clock()), Some(slept));
            let after = reset(state);
            assert!(after <= clock(), "sleep during rearm counts before the first active decision");
            assert!(state.recovery_not_before.is_some_and(|deadline| deadline <= clock()));
            assert_eq!(state.slept_to(Some(base + slept + slept), clock()), None);
            assert_eq!(reset(state), after, "the restored sleep counts once");
        });
    }).await.expect("restored deadline materialization hang detector");
}

/// THE START TAKES UP A DEAD PROCESS'S PACE ONLY WHERE ATERM'S BOOKS WOULD HAND IT ON (review
/// of CX-B, CXR-2): the record writer's start holds the swept predecessor's pace in custody
/// either way, and the resume scan took it up whatever proved it, so a person's resume days
/// later, in another tab, started on the dead process's rung and streak, which the writer
/// judged not this start's and withheld from the record. Now a predecessor's pace is taken up
/// where its record was written in this process's own aterm session (aterm's loop takes up a
/// tab's books at every start, however old) or aterm's relaunch typed this line, and dropped
/// otherwise; a pace taken up puts the session past its opening (gap 32). NEGATIVE CONTROLS:
/// another tab's record, and one that names no session, at a person's resume.
#[tokio::test]
async fn a_dead_processs_pace_is_taken_up_only_in_its_tab_or_at_aterms_relaunch() {
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::Launch;
    use codex_alab_core::session::LaunchWitness;
    use codex_alab_core::session::Predecessor;
    use codex_alab_core::session::WallMark;
    // (the dead record's aterm session, aterm's relaunch witnessed at this start)
    for (theirs, relaunch) in [
        (Some("s-this-tab"), false),
        (Some("s-another-tab"), false),
        (None, false),
        (Some("s-another-tab"), true),
        (None, true),
    ] {
        let case = format!("{theirs:?} relaunch={relaunch}");
        codex_alab_core::activate();
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let sup = Supervisor::new(crate::SupervisorOptions {
            shared,
            endpoint: Arc::new(|| None),
            client: ClientOptions::default(),
        });
        let autopilot = Autopilot::new(sup, None, None, Timing::default(), |_| true);
        let id = ThreadId::new();
        let store = ExtensionData::new(id.to_string());
        store.insert(ThreadRt::new(id, None, false, Duration::ZERO));
        let rt = store.get::<ThreadRt>().expect("native runtime");
        autopilot.keep_thread(&rt);
        rt.resumed.store(true, Ordering::SeqCst);
        let written_at = now_ms() - if relaunch { 60_000 } else { 3 * 86_400_000 };
        let started = now_ms() - 1_000;
        let line = vec!["/twin/codex".to_string(), "resume".into(), rt.key.clone()];
        shared.set_launch(Launch {
            args: line.iter().map(std::ffi::OsString::from).collect(),
            vars: vec![(
                codex_alab_core::session::ATERM_SESSION_ENV.into(),
                "s-this-tab".into(),
            )],
            started_at_ms: started,
            ..Default::default()
        });
        let pace = Pace {
            short_streak: 4,
            wall: Some(WallMark {
                class: "entitlement".into(),
                attempts: 3,
                first_seen_ms: written_at - 600_000,
                last_try_ms: Some(written_at - 60_000),
                since_ms: Some(written_at - 60_000),
                answer_owed: false,
            }),
            ..Default::default()
        };
        // The record writer's start: the swept record, and its pace held in custody.
        shared.note_predecessor(Predecessor {
            pid: 41,
            session_id: rt.key.clone(),
            exit: Some("signal:1".into()),
            resume_argv: Some(line.clone()),
            status: Some("idle".into()),
            updated_at_ms: written_at,
            pace: Some(pace.clone()),
            aterm_session: theirs.map(str::to_string),
            ..Default::default()
        });
        shared.set_pace(&rt.key, pace.clone());
        shared.set_swept();
        shared.set_launch_witness(Some(LaunchWitness {
            hand: Some("lease:aterm-harness@812".into()),
            human: if relaunch {
                HumanSeen::Never
            } else {
                HumanSeen::At(started - 1)
            },
            read_at_ms: started + 1,
            server_pid: Some(812),
            hand_pid: HandPid::Absent,
        }));
        Arc::clone(&autopilot).resume_scan(Arc::clone(&rt)).await;
        let handed = theirs == Some("s-this-tab") || relaunch;
        let state = rt.state();
        assert_eq!(
            (
                state.short_streak,
                state.wall.as_ref().map(|wall| wall.attempts)
            ),
            if handed { (4, Some(3)) } else { (0, None) },
            "{case}: the thread's memory"
        );
        // A pace handed on is the session's work before this process (gap 32): a person's
        // message here is never the one that opens it. No history is read here, so only the
        // pace says so.
        assert_eq!(state.past_opening, handed, "{case}: past its opening");
        assert_eq!(
            shared.pace(&rt.key).map(|pace| pace.short_streak),
            handed.then_some(4),
            "{case}: the pace the record may carry"
        );
    }
}
