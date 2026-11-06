// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A provider's in-turn capacity wait must survive a witnessed crash relaunch.

use crate::Autopilot;
use crate::Supervisor;
use crate::SupervisorOptions;
use crate::decide::Act;
use crate::decide::ThreadState;
use crate::decide::Timing;
use crate::decide::decide;
use crate::extension::Relaunch;
use crate::extension::relaunch;
use crate::extension::resume_into;
use codex_alab_aterm::Options as ClientOptions;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_core::session::Frame;
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
use codex_core::config::Config;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_protocol::protocol::EventMsg;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use wiremock::ResponseTemplate;

#[derive(Clone, Copy)]
enum Phase {
    Active,
    BeforeClaim,
    OffThenOn,
    BeforeFrame,
    Signal(i32),
}

#[test]
fn an_in_turn_capacity_deadline_survives_the_busy_crash_record() {
    run(Phase::Active);
}

#[test]
fn an_in_turn_capacity_wait_before_the_first_claim_keeps_its_deadline() {
    run(Phase::BeforeClaim);
}

#[test]
fn off_and_on_during_the_same_native_capacity_wait_keeps_provider_pacing() {
    run(Phase::OffThenOn);
}

#[test]
fn the_initial_capacity_response_can_precede_the_first_native_frame() {
    run(Phase::BeforeFrame);
}

#[test]
fn native_shutdown_for_a_term_remedy_keeps_the_capacity_deadline() {
    run(Phase::Signal(libc::SIGTERM));
}

#[test]
fn native_shutdown_when_the_terminal_goes_away_keeps_the_capacity_deadline() {
    run(Phase::Signal(libc::SIGHUP));
}

fn run(phase: Phase) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_stack_size(16 << 20)
                .enable_all()
                .build()
                .expect("owned test runtime")
                .block_on(async {
                    tokio::time::timeout(Duration::from_secs(60), capacity_crash(phase))
                        .await
                        .expect("capacity crash test hang detector");
                });
        })
        .expect("owned test thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

async fn capacity_crash(phase: Phase) {
    codex_alab_core::activate();
    let scratch = tempfile::tempdir().expect("scratch record and control socket");
    let fake = FakeAterm::start(scratch.path(), Mode::Current)
        .await
        .expect("fake aterm");
    fake.set_policy_key("human_grace_s", "0");
    if matches!(phase, Phase::BeforeClaim) {
        fake.set_effective(false, "autopilot-off");
    }
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let endpoint = fake.endpoint();
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: ClientOptions::default(),
    });
    let mut builder = ExtensionRegistryBuilder::<Config>::new();
    let autopilot = Autopilot::new(sup, None, None, Timing::default(), |_| true);
    autopilot.register(&mut builder);
    let server = responses::start_mock_server().await;
    let mock = responses::mount_response_once(
        &server,
        ResponseTemplate::new(503)
            .insert_header("Retry-After", "3600")
            .set_body_json(serde_json::json!({"error": {
                "code": "server_is_overloaded",
                "message": "The selected model is at capacity."
            }})),
    )
    .await;
    let test = test_codex()
        .with_extensions(Arc::new(builder.build()))
        .with_config(|config| {
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(0);
        })
        .build(&server)
        .await
        .expect("real core fixture");
    autopilot.bind_thread_manager(Arc::downgrade(&test.thread_manager));
    let thread = test.session_configured.thread_id.to_string();
    let rollout = test
        .session_configured
        .rollout_path
        .as_ref()
        .expect("native rollout path");
    let frame = Frame {
        primary_thread: Some(&thread),
        displayed_thread: Some(&thread),
        rollout_path: Some(rollout),
        cwd: test.cwd_path(),
        model: "gpt-5",
        codex_home: test.codex_home_path(),
        embedded: true,
        state: ProgramState::Working,
        draft_empty: true,
        queued_input: false,
        popup_open: false,
        editor_open: false,
        pending_user_turn: false,
    };
    if !matches!(phase, Phase::BeforeFrame) {
        shared.observe(&frame, now_ms());
    }
    if !matches!(phase, Phase::BeforeClaim | Phase::BeforeFrame) {
        assert!(
            fake.wait_for(|f| f.claim().is_some_and(|claim| claim.proven_pid.is_some()))
                .await
        );
    }
    let request_began = Instant::now();
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Keep the original task during the capacity wait.".into(),
            text_elements: Vec::new(),
        }]))
        .await
        .expect("start real turn");
    loop {
        match test.codex.next_event().await.expect("core event").msg {
            EventMsg::StreamError(_) => break,
            EventMsg::Error(error) => panic!("capacity became terminal: {error:?}"),
            EventMsg::TurnComplete(event) => panic!("capacity ended early: {event:?}"),
            EventMsg::TurnAborted(event) => panic!("capacity aborted: {event:?}"),
            _ => {}
        }
    }
    assert!(shared.turn_running());
    let selected = shared
        .in_flight_retry()
        .expect("the real retry hook published its pacing fact");
    assert_eq!(selected.thread, thread);
    if matches!(phase, Phase::BeforeFrame) {
        assert_eq!(shared.view().primary_thread, None);
        shared.observe(&frame, now_ms());
    }
    if matches!(phase, Phase::BeforeClaim) {
        assert_ne!(
            shared.autopilot(),
            codex_alab_core::session::Autopilot::Active
        );
        fake.set_effective(true, "enabled");
    }
    if matches!(phase, Phase::OffThenOn) {
        fake.set_effective(false, "autopilot-off");
        assert!(fake.wait_for(|f| f.claim().is_none()).await);
        assert_eq!(shared.in_flight_retry(), Some(selected.clone()));
        fake.set_effective(true, "enabled");
    }
    assert!(
        fake.wait_for(|f| f.claim().is_some_and(|claim| claim.proven_pid.is_some()))
            .await
    );
    assert_eq!(shared.in_flight_retry(), Some(selected));
    let loaded = test
        .thread_manager
        .get_thread(test.session_configured.thread_id)
        .await
        .expect("live native thread");
    let writer = Writer::new(shared, scratch.path().into(), Duration::ZERO, None);
    writer
        .write_now()
        .expect("record during real capacity wait");
    if let Phase::Signal(signal) = phase {
        shared.set_signal(signal);
        writer.hold_status();
        test.codex
            .shutdown_and_wait()
            .await
            .expect("native signal teardown");
        writer
            .finalize(&Ending::Signalled(signal))
            .expect("held-busy signal record");
    } else {
        loaded
            .flush_rollout()
            .await
            .expect("flush native crash history");
    }
    let mut recorded = record::read(&writer.path()).expect("actual busy record");
    assert_eq!(recorded.status, record::Status::Busy);
    let args = vec!["/abs/bin/codex".into(), "resume".into(), thread.clone()];
    recorded.resume_argv = Some(args.clone());
    record::write_atomic(scratch.path(), &recorded).expect("resume argv fixture");
    let started = now_ms();
    let argv: Vec<_> = args.into_iter().map(OsString::from).collect();
    let next = Shared::new();
    next.set_launch(Launch {
        args: argv.clone(),
        started_at_ms: started,
        ..Default::default()
    });
    record::sweep_with(
        scratch.path(),
        recorded.pid.wrapping_add(1),
        started,
        |_, _| false,
        |candidate| record::judge(candidate, &argv, started, 86_400_000),
        |pid, candidate| {
            next.note_predecessor(Predecessor {
                pid,
                session_id: candidate.session_id.clone().expect("primary thread"),
                exit: candidate.exit.clone(),
                resume_argv: candidate.resume_argv.clone(),
                status: Some("busy".into()),
                updated_at_ms: candidate.updated_at,
                recovery: candidate.recovery.clone().map(Into::into),
                in_flight_retry: candidate.in_flight_retry.clone().map(Into::into),
                ..Predecessor::default()
            })
        },
    );
    next.set_swept();
    next.set_launch_witness(Some(LaunchWitness {
        hand: Some("lease:aterm-harness@812".into()),
        human: HumanSeen::Never,
        read_at_ms: started + 200,
        server_pid: Some(812),
        hand_pid: HandPid::Absent,
    }));
    let inherited = relaunch(&next, &thread, started + 500);
    assert_eq!(inherited, Relaunch::CarryOn);
    // Flush native history without allowing shutdown's synthetic abort to become the
    // crash history. A real crash leaves this same currently unfinished turn.
    let (items, _, _) = codex_core::RolloutRecorder::load_rollout_items(rollout)
        .await
        .expect("native crash rollout reader");
    // The fake shows the claim proven a moment before the supervisor takes it and keeps its
    // policy: under load the read met that window ("proven policy"). Wait for the
    // supervisor's own (a hang detector).
    let deadline = Instant::now() + Duration::from_secs(60);
    let policy = loop {
        if let Some(policy) = autopilot
            .supervisor()
            .policy()
            .filter(|_| autopilot.supervisor().acting())
        {
            break policy;
        }
        assert!(
            Instant::now() < deadline,
            "never: the supervisor's proven policy"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let epoch = now_ms();
    let now = Instant::now();
    let mut state = ThreadState::default();
    let source = if matches!(phase, Phase::Signal(_)) {
        crate::resume_history::ResumeSource::Signal
    } else {
        crate::resume_history::ResumeSource::Crash
    };
    assert!(resume_into(&mut state, &items, source, None, 0, now));
    let retry = next
        .predecessor(&thread)
        .and_then(|old| old.in_flight_retry)
        .expect("the exact crash record carried the selected provider wait");
    assert!(
        crate::in_flight_retry::restore(
            &mut state,
            &items,
            &retry,
            matches!(phase, Phase::Signal(_)),
            now,
            epoch
        ),
        "the native rollout still ends at the waiting turn"
    );
    let decision = decide(&state, &policy, &Timing::default(), now, Duration::ZERO);
    assert!(matches!(decision.act, Act::Type { .. }), "{decision:?}");
    // Stop the owned fixture before the assertion so a failed regression leaves no task.
    if !matches!(phase, Phase::Signal(_)) {
        test.codex
            .shutdown_and_wait()
            .await
            .expect("owned core shutdown");
        assert_eq!(
            shared.in_flight_retry(),
            None,
            "an ordinary shutdown cleared the wait"
        );
    }
    assert_eq!(mock.requests().len(), 1);
    assert!(
        decision.shown_wait.is_some(),
        "a long provider delay is visible"
    );
    assert!(
        decision
            .not_before
            .is_some_and(|at| at >= request_began + Duration::from_secs(3600)),
        "witnessed crash continuation bypasses the in-turn provider deadline: {decision:?}; record recovery={:?}",
        recorded.recovery
    );
}
