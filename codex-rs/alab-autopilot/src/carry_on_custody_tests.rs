// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A second loaded root cannot relinquish the crash continuation's busy record.

use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;
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
use codex_alab_tui::record;
use codex_alab_tui::session::Writer;
use codex_protocol::protocol::EventMsg;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;

fn resume_command() -> clap::Command {
    clap::Command::new("codex")
        .subcommand(clap::Command::new("resume").arg(clap::Arg::new("session_id")))
}

#[test]
fn another_roots_native_completion_preserves_the_crash_carry_on_record() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            codex_alab_core::activate();
            let scratch = tempfile::tempdir().expect("private world");
            let fake = FakeAterm::start(scratch.path(), Mode::Current)
                .await
                .expect("owned aterm");
            fake.set_policy_key("human_grace_s", "0");
            let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
            let b_id = ThreadId::new();
            let started = now_ms();
            let image = scratch.path().join("codex");
            std::fs::write(&image, b"captured image; never executed").expect("owned launch image");
            let identity = codex_alab_tui::upgrade::build_id(&image).expect("launch image identity");
            let line = vec![image.to_string_lossy().into_owned(), "resume".into(), b_id.to_string()];
            shared.set_launch(Launch {
                args: line.iter().map(Into::into).collect(),
                current_exe: Some(image.clone()),
                argv0: Some(image),
                exe_id: Some((identity.dev, identity.ino)),
                pid: std::process::id(),
                started_at_ms: started,
                ..Default::default()
            });
            shared.note_predecessor(Predecessor {
                pid: 41,
                session_id: b_id.to_string(),
                // SIGKILL leaves the busy record without a terminal exit write.
                exit: None,
                resume_argv: Some(line),
                status: Some("busy".into()),
                updated_at_ms: started - 1_000,
                ..Default::default()
            });
            shared.set_swept();
            shared.set_launch_witness(Some(LaunchWitness {
                hand: Some("lease:aterm-harness@812".into()),
                human: HumanSeen::Never,
                read_at_ms: started + 1,
                server_pid: Some(812),
                hand_pid: HandPid::Absent,
            }));
            let endpoint = fake.endpoint();
            let sup = Supervisor::new(crate::SupervisorOptions {
                shared,
                endpoint: Arc::new(move || Some(endpoint.clone())),
                client: ClientOptions::default(),
            });
            let mut builder = codex_extension_api::ExtensionRegistryBuilder::<Config>::new();
            let pilot = Autopilot::new(
                sup,
                None,
                None,
                Timing::default(),
                |_| true,
            );
            pilot.watching.store(true, Ordering::SeqCst);
            pilot.register(&mut builder);
            let server = responses::start_mock_server().await;
            let test = test_codex()
                .with_extensions(Arc::new(builder.build()))
                .build(&server)
                .await
                .expect("root A");
            eprintln!("carry-on setup: native A loaded");
            pilot.bind_thread_manager(Arc::downgrade(&test.thread_manager));
            let a_rt = test.codex.thread_extension_data().get::<ThreadRt>().unwrap();
            let _a_evaluation = a_rt.hold_eval();
            let b = test.thread_manager.start_thread(codex_core::StartThreadOptions {
                environments: Some(Vec::new()),
                reserved_thread_id: Some(b_id),
                ..codex_core::StartThreadOptions::new(test.config.clone())
            }).await.expect("root B");
            eprintln!("carry-on setup: native B loaded");
            let b_rt = b.thread.thread_extension_data().get::<ThreadRt>().unwrap();
            for (rt, rollout) in [
                (&a_rt, test.codex.rollout_path()),
                (&b_rt, b.thread.rollout_path()),
            ] {
                shared.observe(&Frame {
                    primary_thread: Some(rt.key()),
                    displayed_thread: Some(rt.key()),
                    rollout_path: rollout.as_deref(),
                    cwd: test.cwd_path(),
                    model: "gpt-5.5",
                    codex_home: test.codex_home_path(),
                    embedded: true,
                    state: ProgramState::Idle,
                    draft_empty: true,
                    queued_input: false,
                    popup_open: false,
                    editor_open: false,
                    pending_user_turn: false,
                }, now_ms());
            }
            wait_for_supervisor(|| pilot.sup.acting()).await;
            eprintln!("carry-on setup: proven claim");
            // The actual resume scanner reads the crashed native history. The store seam
            // supplies the unfinished person's task, as the paginated rollout reader does.
            let history = scratch.path().join("crash.jsonl");
            std::fs::write(&history, concat!(
                r#"{"timestamp":"2026-10-16T22:06:56.447Z","ordinal":1,"type":"event_msg","payload":{"type":"task_started","turn_attribution":{"turn_id":"crashed-turn","turn_trigger":"user","parent_turn_id":null,"initiating_agent_path":null,"root_turn_id":"crashed-turn"},"turn_id":"crashed-turn","root_turn_id":"crashed-turn","started_at":1792188416,"model_context_window":258400,"collaboration_mode_kind":"default"}}"#,
                "\n"
            )).expect("crashed native history");
            let (items, _, _) = codex_core::RolloutRecorder::load_rollout_items(&history)
                .await.expect("native rollout reader");
            shared.observe(&Frame {
                primary_thread: Some(b_rt.key()),
                displayed_thread: Some(b_rt.key()),
                rollout_path: Some(&history),
                cwd: test.cwd_path(),
                model: "gpt-5.5",
                codex_home: test.codex_home_path(),
                embedded: true,
                state: ProgramState::Idle,
                draft_empty: true,
                queued_input: false,
                popup_open: false,
                editor_open: false,
                pending_user_turn: false,
            }, now_ms());
            pilot.set_history(items);
            b_rt.resumed.store(true, Ordering::SeqCst);
            let (entered, entry) = tokio::sync::oneshot::channel();
            let (release, blocked) = tokio::sync::oneshot::channel();
            *lock(&b_rt.before_ready) = Some((entered, blocked));
            let mut scan = tokio::spawn(Arc::clone(&pilot).resume_scan(Arc::clone(&b_rt)));
            tokio::select! {
                entered = entry => entered.expect("crash continuation reached preparatory readiness"),
                result = &mut scan => {
                    eprintln!("resume ended before readiness: {result:?}, state={:#?}, journal={:#?}", b_rt.state(), pilot.sup.journal());
                    b.thread.shutdown_and_wait().await.expect("stop owned B");
                    test.codex.shutdown_and_wait().await.expect("stop owned A");
                    panic!("crash continuation never reached its readiness checks");
                }
            }
            eprintln!("carry-on setup: B awaits readiness");
            assert!(b_rt.state().carry_on);
            assert!(shared.carry_on_owed());
            assert_eq!(shared.view().primary_thread.as_deref(), Some(b_rt.key()));
            assert!(shared.owed_act().is_none());

            responses::mount_sse_once(&server, responses::sse(vec![
                responses::ev_response_created("a-response"),
                responses::ev_assistant_message("a-message", "A completed its independent task"),
                responses::ev_completed("a-response"),
            ])).await;
            test.codex.start_or_steer_turn(TurnInputRequest::new(TurnInput::UserInput {
                content: vec![UserInput::Text { text: "root A's separate task".into(), text_elements: Vec::new() }],
                client_id: None,
            })).await.expect("actual native A turn");
            wait_for_event(&test.codex, |event| matches!(event, EventMsg::TurnComplete(_))).await;
            eprintln!("carry-on setup: A native complete event");
            wait_for_supervisor(|| {
                pilot.with_current_runtime(&a_rt, || {
                    a_rt.state().native_origin.is_some_and(|origin| {
                        origin.phase == codex_alab_core::session::NativePhase::Complete
                    }) && !shared.turn_running_on(a_rt.key())
                }).unwrap_or(false)
            }).await;
            assert!(b_rt.state().carry_on);
            assert!(!shared.turn_running());
            assert!(!shared.acting());
            assert!(shared.owed_act().is_none());
            let writer = Writer::new(shared, scratch.path().join("records"), Duration::ZERO, Some(resume_command));
            writer.prepare();
            writer.write_now().expect("write actual session record");
            let recorded = record::read(&writer.path()).expect("session record");
            assert_eq!(recorded.session_id.as_deref(), Some(b_rt.key()));
            assert!(recorded.resume_argv.as_ref().is_some_and(|line| line.last().is_some_and(|id| id == b_rt.key())));

            b.thread.shutdown_and_wait().await.expect("stop owned B");
            let _ = release.send(());
            scan.await.expect("resume scan ended");
            test.codex.shutdown_and_wait().await.expect("stop owned A");
            assert_eq!(recorded.status, record::Status::Busy,
                "B still owes the crash continuation: another root's completion cannot make its record idle");
        }).await.expect("carry-on custody hang detector");
    });
}
