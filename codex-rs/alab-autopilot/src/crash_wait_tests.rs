// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A real evaluated wall wait through the session-record boundary. The process's death
//! and aterm's relaunch lease are injected facts, never signals sent to this test process.

use super::ended;
use super::harness;
use super::point;
use crate::decide::Act;
use crate::decide::Decision;
use crate::decide::ThreadState;
use crate::decide::Timing;
use crate::decide::Wall;
use crate::decide::decide;
use crate::extension::Relaunch;
use crate::extension::relaunch;
use crate::extension::resume_into;
use crate::supervisor::clock;
use codex_alab_core::session::HandPid;
use codex_alab_core::session::HumanSeen;
use codex_alab_core::session::Launch;
use codex_alab_core::session::LaunchWitness;
use codex_alab_core::session::Predecessor;
use codex_alab_core::session::ProgramState;
use codex_alab_core::session::Shared;
use codex_alab_core::session::now_ms;
use codex_alab_tui::record;
use codex_alab_tui::session::Ending;
use codex_alab_tui::session::Writer;
use codex_extension_api::ExtensionData;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStopInput;
use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(30 * 60);

#[test]
fn tied_predecessor_records_cannot_restore_a_cleared_provider_wait() {
    use codex_alab_core::session::Autopilot;
    use codex_alab_core::session::Frame;
    use codex_alab_core::session::Recovery;
    use codex_alab_core::session::RecoveryWall;

    const THREAD: &str = "019a0000-0000-7000-8000-000000000001";
    // Whichever file the directory yields last, one assignment gives it the old
    // intent. Neither equal-millisecond ordering proves which process cleared it.
    for recovery_pid in [1_000_000_001, 1_000_000_002] {
        let home = tempfile::tempdir().expect("scratch records");
        let old: &'static Shared = Box::leak(Box::new(Shared::new()));
        old.set_launch(Launch {
            pid: 1_000_000_001,
            ..Default::default()
        });
        old.set_autopilot(Autopilot::Active);
        old.observe(
            &Frame {
                primary_thread: Some(THREAD),
                displayed_thread: Some(THREAD),
                rollout_path: None,
                cwd: home.path(),
                model: "gpt-5",
                codex_home: home.path(),
                embedded: true,
                state: ProgramState::Error,
                draft_empty: true,
                queued_input: false,
                popup_open: false,
                editor_open: false,
                pending_user_turn: false,
            },
            now_ms(),
        );
        old.set_recovery(Some(Recovery {
            thread: THREAD.into(),
            turn_id: "failed-turn".into(),
            point_at_ms: 1_000,
            not_before_ms: 1_800_000,
            by_autopilot: true,
            by_person: false,
            wall: RecoveryWall::Server {
                what: "model at capacity".into(),
                cut_off: false,
                retry_at_ms: Some(1_700_000),
            },
        }));
        let writer = Writer::new(old, home.path().into(), Duration::ZERO, None);
        writer.write_now().expect("original provider wait");
        let original = record::read(&writer.path()).expect("actual writer record");
        let args = vec!["codex".to_owned(), "resume".into(), THREAD.into()];
        let mut predecessors = Vec::new();
        for pid in [1_000_000_001, 1_000_000_002] {
            let mut candidate = original.clone();
            candidate.pid = pid;
            candidate.resume_argv = Some(args.clone());
            if pid != recovery_pid {
                candidate.recovery = None;
            }
            record::write_atomic(home.path(), &candidate).expect("tied candidate");
            predecessors.push(Predecessor {
                pid,
                session_id: THREAD.into(),
                status: Some("idle".into()),
                resume_argv: candidate.resume_argv,
                updated_at_ms: candidate.updated_at,
                recovery: candidate.recovery.map(Into::into),
                ..Default::default()
            });
        }
        let started = original.updated_at + 1;
        let next: &'static Shared = Box::leak(Box::new(Shared::new()));
        next.set_launch(Launch {
            pid: 1_000_000_003,
            args: args.iter().map(OsString::from).collect(),
            started_at_ms: started,
            ..Default::default()
        });
        let next_writer = Writer::new(next, home.path().into(), Duration::ZERO, None);
        next_writer.prepare();
        assert!(next.recovery().is_none(), "the writer rejects the tie");
        next.set_launch_witness(Some(LaunchWitness {
            hand: Some("lease:aterm-harness@812".into()),
            human: HumanSeen::Never,
            read_at_ms: started + 1,
            server_pid: Some(812),
            hand_pid: HandPid::Absent,
        }));
        // Also replay both legal sweep callback orders directly: the assertion
        // must catch the arbitrary tie winner independently of filesystem order.
        for reverse in [false, true] {
            let ordered = Shared::new();
            ordered.set_launch(next.launch().expect("successor launch").clone());
            ordered.set_launch_witness(next.launch_witness());
            for index in if reverse { [1, 0] } else { [0, 1] } {
                ordered.note_predecessor(predecessors[index].clone());
            }
            assert_eq!(
                relaunch(&ordered, THREAD, started + 1),
                Relaunch::Nothing,
                "both callback orders reject ambiguity (reverse={reverse})"
            );
        }
        assert_eq!(
            relaunch(next, THREAD, started + 1),
            Relaunch::Nothing,
            "ambiguous records cannot override the clearing (old intent pid={recovery_pid})"
        );
    }
}

#[test]
fn a_newer_clearing_survives_another_crash_before_resume_scan() {
    use codex_alab_core::session::Autopilot;
    use codex_alab_core::session::Frame;
    use codex_alab_core::session::OwedAct;
    use codex_alab_core::session::Pace;

    fn command() -> clap::Command {
        clap::Command::new("codex")
            .subcommand(clap::Command::new("resume").arg(clap::Arg::new("session_id")))
    }

    const THREAD: &str = "019a0000-0000-7000-8000-000000000001";
    for (probed, framed) in [(false, false), (false, true), (true, false), (true, true)] {
        let home = tempfile::tempdir().expect("scratch records");
        let rollout = home.path().join("rollout.jsonl");
        std::fs::write(&rollout, "{}\n").expect("materialized rollout");
        let args = vec!["codex".to_owned(), "resume".into(), THREAD.into()];
        let launch = |pid, started_at_ms| Launch {
            pid,
            args: args.iter().map(OsString::from).collect(),
            argv0: Some("codex".into()),
            started_at_ms,
            ..Default::default()
        };
        let frame = |shared: &Shared| {
            shared.observe(
                &Frame {
                    primary_thread: Some(THREAD),
                    displayed_thread: Some(THREAD),
                    rollout_path: Some(&rollout),
                    cwd: home.path(),
                    model: "gpt-5",
                    codex_home: home.path(),
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
        };
        let old: &'static Shared = Box::leak(Box::new(Shared::new()));
        old.set_launch(launch(1_000_000_001, now_ms()));
        old.set_autopilot(Autopilot::Active);
        frame(old);
        old.set_owed_act(Some(OwedAct {
            thread: THREAD.into(),
            rule: Some(crate::words::RULE_DONE_CHECK.into()),
            text: Some("Already cancelled continuation.".into()),
            mode: Some("cruise".into()),
            not_before_ms: now_ms() + 60_000,
            ..Default::default()
        }));
        old.set_pace(
            THREAD,
            Pace {
                short_streak: 5,
                ..Default::default()
            },
        );
        let writer = Writer::new(old, home.path().into(), Duration::ZERO, Some(command));
        writer.write_now().expect("original owed continuation");
        let mut cleared = record::read(&writer.path()).expect("actual writer record");
        assert!(cleared.owed.is_some());
        assert!(cleared.pace.is_some());
        assert_eq!(cleared.resume_argv, Some(args.clone()));
        cleared.pid = 1_000_000_002;
        cleared.updated_at += 1;
        cleared.owed = None;
        cleared.pace = None;
        cleared.autopilot = record::Autopilot::Off;
        record::write_atomic(home.path(), &cleared).expect("newer explicit clearing");

        let started = cleared.updated_at + 1;
        let next: &'static Shared = Box::leak(Box::new(Shared::new()));
        next.set_launch(launch(1_000_000_003, started));
        let next_writer = Writer::new(next, home.path().into(), Duration::ZERO, Some(command));
        next_writer.prepare();
        if probed {
            next.set_launch_witness(Some(LaunchWitness {
                hand: Some("lease:aterm-harness@812".into()),
                human: HumanSeen::Never,
                read_at_ms: started + 1,
                server_pid: Some(812),
                hand_pid: HandPid::Pid(812),
            }));
        }
        // The native widget may publish its first frame before the async resume
        // scan has judged the inherited continuation. Its record must retain the
        // latest clearing throughout that startup window, including another crash.
        if framed {
            frame(next);
        }
        next_writer
            .write_now()
            .expect("successor before resume scan");
        let successor = record::read(&next_writer.path()).expect("successor record");
        assert_eq!(successor.resume_argv, probed.then(|| args.clone()));
        for pid in [1_000_000_001, 1_000_000_002] {
            assert_eq!(
                record::path_for(home.path(), pid).exists(),
                !probed,
                "unprobed startup keeps both snapshots; concrete publication retires them"
            );
        }
        let second_started = successor.updated_at.max(started) + 1;
        let second: &'static Shared = Box::leak(Box::new(Shared::new()));
        second.set_launch(launch(1_000_000_004, second_started));
        let second_writer = Writer::new(second, home.path().into(), Duration::ZERO, Some(command));
        second_writer.prepare();
        assert_eq!(
            second.predecessor(THREAD).map(|old| old.pid),
            Some(if probed { 1_000_000_003 } else { 1_000_000_002 }),
            "a pre-probe crash must select the retained newest clearing, not its older intent"
        );
        second.set_launch_witness(Some(LaunchWitness {
            hand: Some("lease:aterm-harness@812".into()),
            human: HumanSeen::Never,
            read_at_ms: second_started + 1,
            server_pid: Some(812),
            hand_pid: HandPid::Absent,
        }));
        let restored = relaunch(second, THREAD, second_started + 1);
        assert_eq!(
            (successor.owed, successor.pace, restored),
            (None, None, Relaunch::Nothing),
            "an intervening crash cannot turn an older continuation back into authority"
        );
    }
}

#[test]
fn a_second_startup_carries_only_an_authenticated_busy_predecessor() {
    use codex_alab_core::session::Frame;

    fn command() -> clap::Command {
        clap::Command::new("codex")
            .subcommand(clap::Command::new("resume").arg(clap::Arg::new("session_id")))
    }

    const THREAD: &str = "019a0000-0000-7000-8000-000000000001";
    for (status, authentic) in [
        (record::Status::Idle, true),
        (record::Status::Waiting, true),
        (record::Status::Upgrading, true),
        (record::Status::Exited, true),
        (record::Status::Busy, false),
        (record::Status::Busy, true),
    ] {
        let home = tempfile::tempdir().expect("scratch records");
        let rollout = home.path().join("rollout.jsonl");
        std::fs::write(&rollout, "{}\n").expect("materialized rollout");
        let args = vec!["codex".to_owned(), "resume".into(), THREAD.into()];
        let launch = |pid, started_at_ms| Launch {
            pid,
            args: args.iter().map(OsString::from).collect(),
            argv0: Some("codex".into()),
            started_at_ms,
            ..Default::default()
        };
        let prove = |shared: &Shared, authentic| {
            let started = shared.launch().expect("launch").started_at_ms;
            shared.set_launch_witness(Some(LaunchWitness {
                hand: Some("lease:aterm-harness@812".into()),
                human: HumanSeen::Never,
                read_at_ms: started + 1,
                server_pid: Some(812),
                hand_pid: HandPid::Pid(if authentic { 812 } else { 813 }),
            }));
        };
        let old: &'static Shared = Box::leak(Box::new(Shared::new()));
        old.set_launch(launch(1_000_000_001, now_ms()));
        old.observe(
            &Frame {
                primary_thread: Some(THREAD),
                displayed_thread: Some(THREAD),
                rollout_path: Some(&rollout),
                cwd: home.path(),
                model: "gpt-5",
                codex_home: home.path(),
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
        let writer = Writer::new(old, home.path().into(), Duration::ZERO, Some(command));
        writer.write_now().expect("original record");
        let mut source = record::read(&writer.path()).expect("record");
        source.status = status;
        if status == record::Status::Exited {
            source.exit = Some("fatal".into());
        }
        record::write_atomic(home.path(), &source).expect("predecessor state");

        let next: &'static Shared = Box::leak(Box::new(Shared::new()));
        next.set_launch(launch(1_000_000_002, source.updated_at + 1));
        let writer = Writer::new(next, home.path().into(), Duration::ZERO, Some(command));
        writer.prepare();
        prove(next, authentic);
        writer.write_now().expect("settled before the first Frame");
        let current = record::read(&writer.path()).expect("current record");
        assert_eq!(current.status, record::Status::Busy);
        assert_eq!(current.resume_argv, Some(args.clone()));
        assert_eq!(
            next.carried_on(),
            0,
            "serialization does not spend live authority"
        );
        let second: &'static Shared = Box::leak(Box::new(Shared::new()));
        second.set_launch(launch(1_000_000_003, current.updated_at + 1));
        let writer = Writer::new(second, home.path().into(), Duration::ZERO, Some(command));
        writer.prepare();
        prove(second, true);
        assert_eq!(
            relaunch(second, THREAD, current.updated_at + 2),
            if status == record::Status::Busy && authentic {
                Relaunch::CarryOn
            } else {
                Relaunch::Nothing
            },
            "an identity-only startup cannot mint busy crash authority (status={status:?}, authentic={authentic})"
        );
    }
}

#[derive(Clone, Copy)]
enum WaitKind {
    Server,
    Usage,
}

/// Evaluate first, capture the actual writer's record, then feed its JSON through the
/// same dead-record judge and launch-witness checks the next image uses. The CLI parser
/// is outside this fixture: its known resume argv is supplied after writer serialization.
async fn after_recorded_wait(
    kind: WaitKind,
    human: bool,
    signal: Option<i32>,
) -> (Decision, Decision) {
    let h = harness(&[
        ("retry_api_errors", "true"),
        ("resume_limits", "true"),
        ("limit_wait", "true"),
    ])
    .await;
    let at = clock();
    let provider_deadline = at + WAIT;
    let wall = match kind {
        WaitKind::Server => Wall::Server {
            what: "model at capacity".into(),
            cut_off: false,
            retry_at: Some(provider_deadline),
        },
        WaitKind::Usage => Wall::Usage {
            reset_at: Some(provider_deadline),
            reset_label: Some("the original reset".into()),
        },
    };
    point(&h, ended(None, Some(wall)));
    h.rt.with_state(|state| state.point_turn_id = Some("failed-turn".into()));
    h.shared.set_last_turn(
        h.rt.key(),
        codex_alab_core::session::LastTurn {
            turn_id: Some("failed-turn".into()),
            ended_at_ms: now_ms(),
            ..Default::default()
        },
    );
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(
        h.rt.timer_pending(),
        "the real evaluator scheduled its wait"
    );
    let policy = h.sup.policy().expect("proven policy");
    let timing = Timing::default();
    let before = decide(&h.rt.state(), &policy, &timing, at, Duration::ZERO);
    assert!(
        before
            .not_before
            .is_some_and(|deadline| deadline >= provider_deadline)
    );

    let home = tempfile::tempdir().expect("scratch record directory");
    let rollout = home.path().join("rollout.jsonl");
    let error_info = match kind {
        WaitKind::Server => "server_overloaded",
        WaitKind::Usage => "usage_limit_exceeded",
    };
    let history = format!(
        "{}\n{}\n",
        serde_json::json!({"timestamp":"2026-10-21T00:00:00Z", "type":"event_msg", "payload":{
            "type":"task_started", "turn_id":"failed-turn", "model_context_window":258400,
            "collaboration_mode_kind":"default", "turn_attribution":{
                "turn_id":"failed-turn", "turn_trigger":"user", "parent_turn_id":null,
                "initiating_agent_path":null,"root_turn_id":"failed-turn"}}}),
        serde_json::json!({"timestamp":"2026-10-21T00:00:01Z", "type":"event_msg", "payload":{
            "type":"task_complete", "turn_id":"failed-turn", "last_agent_message":null,
            "error":{"message":"the provider ended the turn", "codex_error_info":error_info}}}),
    );
    std::fs::write(&rollout, history).expect("persisted failed turn");
    let (items, _, _) = codex_core::RolloutRecorder::load_rollout_items(&rollout)
        .await
        .expect("the native rollout reader");
    h.shared.observe(
        &codex_alab_core::session::Frame {
            primary_thread: Some(h.rt.key()),
            displayed_thread: Some(h.rt.key()),
            rollout_path: Some(&rollout),
            cwd: home.path(),
            model: "gpt-5",
            codex_home: home.path(),
            embedded: true,
            state: ProgramState::Error,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        now_ms(),
    );
    let writer = Writer::new(h.shared, home.path().into(), Duration::ZERO, None);
    if let Some(signal) = signal {
        h.shared.set_signal(signal);
        h.autopilot
            .on_thread_stop(ThreadStopInput {
                session_store: &ExtensionData::new("session"),
                thread_store: &h._store,
            })
            .await;
        writer
            .finalize(&Ending::Signalled(signal))
            .expect("signal record");
    } else {
        writer
            .write_now()
            .expect("record while the wall timer waits");
    }
    let mut recorded = record::read(&writer.path()).expect("writer's record");
    assert_eq!(
        recorded.status,
        record::Status::Idle,
        "a wall wait is at rest"
    );
    let args = vec![
        "/abs/bin/codex".to_string(),
        "resume".into(),
        h.rt.key().into(),
    ];
    recorded.resume_argv = Some(args.clone());
    record::write_atomic(home.path(), &recorded).expect("resume argv fixture");
    crate::Autopilot::cancel_timer(&h.rt).await;

    let started = recorded.updated_at + 1_000;
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let argv: Vec<_> = args.into_iter().map(OsString::from).collect();
    shared.set_launch(Launch {
        args: argv.clone(),
        started_at_ms: started,
        ..Default::default()
    });
    record::sweep_with(
        home.path(),
        recorded.pid.wrapping_add(1),
        started,
        |_, _| false,
        |candidate| record::judge(candidate, &argv, started, 86_400_000),
        |pid, candidate| {
            shared.note_predecessor(Predecessor {
                pid,
                session_id: candidate.session_id.clone().expect("primary thread"),
                exit: candidate.exit.clone(),
                resume_argv: candidate.resume_argv.clone(),
                status: Some("idle".into()),
                updated_at_ms: candidate.updated_at,
                owed_act: candidate.owed.clone().map(Into::into),
                recovery: candidate.recovery.clone().map(Into::into),
                ..Predecessor::default()
            })
        },
    );
    shared.set_swept();
    shared.set_launch_witness(Some(LaunchWitness {
        hand: Some("lease:aterm-harness@812".into()),
        human: if human {
            HumanSeen::At(started - 1_000)
        } else {
            HumanSeen::Never
        },
        read_at_ms: started + 200,
        server_pid: Some(812),
        hand_pid: HandPid::Absent,
    }));
    let inherited = relaunch(shared, h.rt.key(), started + 500);
    let mut state = ThreadState::default();
    let carry = matches!(inherited, Relaunch::CarryOn);
    let recovery = match &inherited {
        Relaunch::Recovery(recovery) => Some(recovery.clone()),
        _ => None,
    };
    let owed = match inherited {
        Relaunch::Owed(owed) => Some((crate::Autopilot::owed_of(&owed), false)),
        _ => None,
    };
    let resume_epoch = now_ms();
    let resume_now = clock();
    assert!(resume_into(
        &mut state,
        &items,
        carry,
        owed.or_else(|| recovery.as_ref().map(|_| (None, false))),
        0,
        resume_now
    ));
    if let Some(recovery) = recovery {
        assert!(crate::recovery::restore(
            &mut state,
            &items,
            &recovery,
            resume_now,
            resume_epoch,
            &timing
        ));
    }
    let after = decide(&state, &policy, &timing, resume_now, Duration::ZERO);
    (before, after)
}

#[tokio::test]
async fn accepted_new_work_cannot_record_or_replay_a_restored_old_act() {
    use codex_extension_api::TurnLifecycleContributor;
    use codex_extension_api::TurnStartInput;
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;
    use std::sync::Mutex;

    tokio::time::timeout(Duration::from_secs(60), async {
        let h = harness(&[]).await;
        let at = clock();
        let deadline = at + WAIT;
        point(&h, ended(Some("old work finished"), None));
        let owed = crate::decide::Owed {
            native_origin: None,
            rule: crate::words::RULE_DONE_CHECK,
            text: "This old restored act must never replay".into(),
            mode: crate::decide::Mode::Cruise,
            not_before: deadline,
            repo_owed: None,
            resume_goal: false,
            landing: false,
        };
        h.rt.with_state(|state| {
            state.owed = Some(owed.clone());
            state.point_turn_id = Some("old-finished-turn".into());
            state.last_rule = Some(crate::words::RULE_DONE_CHECK);
            state.last_act = Some(at);
            state.typed.push_back(at);
        });
        h.autopilot.schedule_act(&h.rt, deadline, Some(owed));
        assert!(h.shared.owed_act().is_some());
        let records = tempfile::tempdir().expect("scratch records");
        let writer = Arc::new(Writer::new(
            h.shared,
            records.path().into(),
            Duration::ZERO,
            None,
        ));
        writer.write_now().expect("record old restored act");
        let observed = Arc::new(Mutex::new(None));
        let saved = Arc::clone(&observed);
        let recording = Arc::clone(&writer);
        let runtime = Arc::clone(&h.rt);
        let shared = h.shared;
        assert!(shared.set_flush(Box::new(move || {
            let state = runtime.state();
            assert!(state.owed.is_none(), "cleared before the first flush");
            assert!(state.point_turn_id.is_none());
            assert!(!runtime.timer_pending(), "no old timer can republish it");
            assert!(shared.owed_act().is_none());
            recording.write_now().expect("initial busy record");
            *saved.lock().expect("captured record") = record::read(&recording.path());
            true
        })));
        let session = ExtensionData::new("session");
        let turn = ExtensionData::new("accepted-new-work");
        let mode = CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model: "gpt-5".into(),
                reasoning_effort: None,
                developer_instructions: None,
            },
        };
        h.autopilot
            .on_turn_start(TurnStartInput {
                turn_id: turn.level_id(),
                collaboration_mode: &mode,
                token_usage_at_turn_start: None,
                session_store: &session,
                thread_store: &h._store,
                turn_store: &turn,
            })
            .await;
        // No UserMessage attribution hook ran. Its bounded aterm hand read must not
        // be needed to stop an already superseded act reaching the session record.
        let current = observed
            .lock()
            .expect("captured record")
            .clone()
            .expect("the first flush ran");
        assert_eq!(current.status, record::Status::Busy);
        assert!(current.owed.is_none());
        assert!(current.recovery.is_none());
        let state = h.rt.state();
        assert_eq!(state.last_rule, Some(crate::words::RULE_DONE_CHECK));
        assert_eq!(state.last_act, Some(at));
        assert_eq!(state.typed.front(), Some(&at));
        let started = current.updated_at + 1;
        let args = vec!["codex".to_owned(), "resume".into(), h.rt.key().into()];
        let next = Shared::new();
        next.set_launch(Launch {
            args: args.iter().map(OsString::from).collect(),
            started_at_ms: started,
            ..Default::default()
        });
        next.note_predecessor(Predecessor {
            session_id: h.rt.key().into(),
            status: Some("busy".into()),
            resume_argv: Some(args),
            updated_at_ms: current.updated_at,
            owed_act: current.owed.map(Into::into),
            recovery: current.recovery.map(Into::into),
            ..Default::default()
        });
        next.set_launch_witness(Some(LaunchWitness {
            hand: Some("lease:aterm-harness@812".into()),
            human: HumanSeen::Never,
            read_at_ms: started + 1,
            server_pid: Some(812),
            hand_pid: HandPid::Absent,
        }));
        next.set_swept();
        assert_eq!(
            relaunch(&next, h.rt.key(), started + 1),
            Relaunch::CarryOn,
            "the crash belongs to new work, never the obsolete owed words"
        );
    })
    .await
    .expect("accepted-turn hang detector");
}

#[tokio::test]
async fn server_retry_after_survives_a_witnessed_crash_at_rest() {
    let (before, after) = after_recorded_wait(WaitKind::Server, false, None).await;
    assert!(matches!(
        before.act,
        Act::Type {
            rule: crate::words::RULE_API_RETRY,
            ..
        }
    ));
    assert_eq!(
        after.act, before.act,
        "the failed request still needs its retry after relaunch"
    );
    assert!(
        after.not_before >= before.not_before,
        "a relaunch cannot shorten Retry-After"
    );
}

#[tokio::test]
async fn usage_reset_wait_survives_a_witnessed_sighup_at_rest() {
    let (before, after) = after_recorded_wait(WaitKind::Usage, false, Some(libc::SIGHUP)).await;
    assert!(matches!(before.act, Act::ResumeLimit { .. }));
    assert_eq!(
        after.act, before.act,
        "usage recovery must retain native quota and goal checks"
    );
    assert!(
        after.not_before >= before.not_before,
        "the original reset remains a lower bound"
    );
}

#[tokio::test]
async fn a_persons_resume_does_not_restore_the_old_wall_wait() {
    let (_, after) = after_recorded_wait(WaitKind::Server, true, None).await;
    assert_eq!(after.act, Act::Nothing);
}

#[tokio::test]
async fn an_explicit_stop_does_not_restore_the_old_wall_wait() {
    let (_, after) = after_recorded_wait(WaitKind::Usage, false, Some(libc::SIGINT)).await;
    assert_eq!(after.act, Act::Nothing);
}
