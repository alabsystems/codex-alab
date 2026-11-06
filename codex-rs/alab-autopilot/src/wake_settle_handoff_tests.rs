// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A real accepted upgrade and native resume must not forget a recent wake.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;
use codex_alab_aterm::Options as ClientOptions;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_core::session::Frame;
use codex_alab_core::session::Inherited;
use codex_alab_core::session::Launch;
use codex_alab_core::session::ProgramState;
use codex_alab_core::wake::Reading;
use codex_alab_core::wake::Wake;
use codex_alab_core::wake::WakeState;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;

fn idle_frame(shared: &codex_alab_core::session::Shared, rt: &ThreadRt, test: &TestCodex) {
    shared.observe(
        &Frame {
            primary_thread: Some(rt.key()),
            displayed_thread: Some(rt.key()),
            rollout_path: test.codex.rollout_path().as_deref(),
            cwd: test.cwd.path(),
            model: "gpt-5.5",
            codex_home: test.codex_home_path(),
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

/// The new registry must bind its real manager before its detached resume scans.
#[derive(Default)]
struct ResumeControl {
    permit: Mutex<Option<tokio::sync::OwnedSemaphorePermit>>,
    started: Mutex<Vec<Instant>>,
}

impl ThreadLifecycleContributor<Config> for ResumeControl {
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, Config>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let rt = input
                .thread_store
                .get::<ThreadRt>()
                .expect("Autopilot registered the native runtime first");
            let permit = Arc::clone(&rt.eval)
                .try_acquire_owned()
                .expect("new runtime has no evaluator yet");
            assert!(lock(&self.permit).replace(permit).is_none());
        })
    }
}

impl TurnLifecycleContributor for ResumeControl {
    fn on_turn_start<'a>(&'a self, _: TurnStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            lock(&self.started).push(clock());
        })
    }
}

async fn observed(ready: impl Fn() -> bool) {
    while !ready() {
        tokio::task::yield_now().await;
    }
}

struct WallWatch {
    cancel: std::sync::mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for WallWatch {
    fn drop(&mut self) {
        let _ = self.cancel.send(());
        if let Some(thread) = self.thread.take() {
            thread.join().expect("wall hang detector settled");
        }
    }
}

#[test]
fn an_accepted_upgrade_preserves_wake_settle_into_a_fresh_native_resume() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("owned current-thread runtime")
                .block_on(accepted_upgrade_preserves_wake_settle());
        })
        .expect("owned test thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

async fn accepted_upgrade_preserves_wake_settle() {
    let (cancel, cancelled) = std::sync::mpsc::channel();
    let (expired, wall) = tokio::sync::oneshot::channel();
    let watch = WallWatch {
        cancel,
        thread: Some(std::thread::spawn(move || {
            if cancelled.recv_timeout(Duration::from_secs(60)).is_err() {
                let _ = expired.send(());
            }
        })),
    };
    let exercise = async {
        let mut early = Vec::new();
        // The first case proves that a known wake really holds a legitimate native
        // continuation. The second models the new image's first kernel sample.
        for known_wake in [true, false] {
            let mut old = Harness::new().await;
            old.retain_artifacts();
            old.fake.set_policy_key("continue", "false");
            old.fake.set_policy_key("upgrade", "false");
            wait_for_supervisor(|| {
                old.autopilot
                    .sup
                    .policy()
                    .is_some_and(|policy| !policy.continue_on && !policy.upgrade)
            })
            .await;
            old.immediate_point().await;
            let base = Duration::from_secs(5_000);
            let old_reading = Arc::new(Mutex::new(Reading {
                asleep: base,
                last: None,
            }));
            let read = Arc::clone(&old_reading);
            old.autopilot
                .set_wake_read(Arc::new(move || Some(*lock(&read))));
            let bootstrap = responses::mount_sse_once(
                &old._server,
                responses::sse(vec![
                    responses::ev_response_created("bootstrap"),
                    responses::ev_assistant_message("bootstrap-message", "Work is in progress."),
                    responses::ev_completed("bootstrap"),
                ]),
            )
            .await;
            old.test
                .codex
                .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "Materialize a local fixture checkpoint, then keep working.".into(),
                    text_elements: Vec::new(),
                }]))
                .await
                .expect("real native bootstrap accepted");
            core_test_support::wait_for_event_with_timeout(
                &old.test.codex,
                |event| matches!(event, codex_protocol::protocol::EventMsg::TurnComplete(_)),
                Duration::from_secs(60),
            )
            .await;
            wait_for_supervisor(|| {
                old.rt.state().native_origin.as_ref().is_some_and(|origin| {
                    origin.phase == codex_alab_core::session::NativePhase::Complete
                })
            })
            .await;
            let _evaluated = old.rt.hold_eval_when_free().await;
            idle_frame(old.shared, &old.rt, &old.test);
            let native = old
                .rt
                .state()
                .native_origin
                .expect("real completed turn identity");
            assert_eq!(
                native.phase,
                codex_alab_core::session::NativePhase::Complete
            );
            assert_eq!(bootstrap.requests().len(), 1);
            old.fake.set_policy_key("continue", "true");
            wait_for_supervisor(|| {
                old.autopilot
                    .sup
                    .policy()
                    .is_some_and(|policy| policy.continue_on && !policy.upgrade)
            })
            .await;
            drop(_evaluated);
            Arc::clone(&old.autopilot)
                .evaluate(Arc::clone(&old.rt))
                .await;
            let original = old
                .rt
                .owed_timer()
                .expect("actual evaluator armed its paced act");
            assert_eq!(original.rule, words::RULE_CONTINUE);
            assert_eq!(original.native_origin.as_ref(), Some(&native));
            assert!(original.not_before > clock());
            assert_eq!(old.rt.state().asleep_mark, Some(base));

            let (entered, selection) = tokio::sync::oneshot::channel();
            let (release, paused) = tokio::sync::oneshot::channel();
            *lock(&old.autopilot.before_upgrade_take) = Some((entered, paused));
            old.fake.set_policy_key("upgrade", "true");
            wait_for_supervisor(|| old.autopilot.sup.policy().is_some_and(|p| p.upgrade)).await;
            let pilot = Arc::clone(&old.autopilot);
            let upgrading = tokio::spawn(async move { pilot.try_upgrade().await });
            selection
                .await
                .expect("real ready and upgrade admission reached selection");
            tokio::time::pause();
            let runnable = tokio::spawn(async {
                loop {
                    tokio::task::yield_now().await;
                }
            });
            let wake = Wake {
                at: clock(),
                slept: Duration::from_secs(7_200),
                seq: 1,
            };
            *lock(&old_reading) = Reading {
                asleep: base + wake.slept,
                last: Some(wake),
            };
            release
                .send(())
                .expect("release only this admitted upgrade");
            let request = old
                .requests
                .recv()
                .await
                .expect("conditional upgrade queued");
            let carried = old
                .shared
                .owed_act()
                .expect("real handoff captured the act");
            assert_eq!(carried.native_origin.as_ref(), Some(&native));
            assert_eq!(carried.rule.as_deref(), Some(original.rule));
            assert_eq!(carried.text.as_deref(), Some(original.text.as_str()));
            assert!(carried.not_before_ms <= now_ms());
            assert!(request.decide(true), "actual conditional leave accepted");
            assert!(upgrading.await.expect("upgrade evaluator completed"));
            assert!(old.shared.exec_pending());
            assert_eq!(
                bootstrap.requests().len(),
                1,
                "old image submitted no continuation"
            );
            let rollout = old
                .test
                .codex
                .rollout_path()
                .expect("real persisted rollout");
            old.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("old native stopped");

            let mut dir = tempfile::tempdir().expect("fresh private supervisor world");
            dir.disable_cleanup(true);
            let fake = FakeAterm::start(dir.path(), Mode::Current)
                .await
                .expect("private control");
            fake.set_policy_key("human_grace_s", "0");
            let shared: &'static codex_alab_core::session::Shared =
                Box::leak(Box::new(codex_alab_core::session::Shared::new()));
            assert!(shared.set_inherited(Inherited {
                session_id: Some(old.rt.key.clone()),
                owed_act: Some(carried.clone()),
                ..Default::default()
            }));
            let image = old.shared.launch().unwrap().argv0.clone().unwrap();
            let image_id = codex_alab_tui::upgrade::build_id(&image).expect("captured new image");
            assert!(shared.set_launch(Launch {
                args: vec!["codex".into(), "resume".into(), old.rt.key.clone().into()],
                argv0: Some(image.clone()),
                current_exe: Some(image),
                exe_id: Some((image_id.dev, image_id.ino)),
                cwd: Some(old.test.cwd.path().to_path_buf()),
                started_at_ms: now_ms(),
                ..Default::default()
            }));
            let endpoint = fake.endpoint();
            let supervisor = Supervisor::new(crate::SupervisorOptions {
                shared,
                endpoint: Arc::new(move || Some(endpoint.clone())),
                client: ClientOptions::default(),
            });
            let mut builder = ExtensionRegistryBuilder::<Config>::new();
            let pilot = Autopilot::new(supervisor, None, None, Timing::default(), |_| true);
            pilot.watching.store(true, Ordering::SeqCst);
            let mut kernel = WakeState::default();
            if known_wake {
                kernel.observe(wake.at, base);
            }
            kernel.observe(wake.at, base + wake.slept);
            assert_eq!(kernel.last().is_some(), known_wake);
            let reading = Reading {
                asleep: base + wake.slept,
                last: kernel.last(),
            };
            pilot.set_wake_read(Arc::new(move || Some(reading)));
            pilot.register(&mut builder);
            let control = Arc::new(ResumeControl::default());
            builder.thread_lifecycle_contributor(Arc::clone(&control) as _);
            builder.turn_lifecycle_contributor(Arc::clone(&control) as _);
            let server = responses::start_mock_server().await;
            let continuation = responses::mount_sse_once(
                &server,
                responses::sse(vec![
                    responses::ev_response_created("continued"),
                    responses::ev_assistant_message(
                        "continued-message",
                        "Work remains in progress.",
                    ),
                    responses::ev_completed("continued"),
                ]),
            )
            .await;
            let fresh = test_codex()
                .with_extensions(Arc::new(builder.build()))
                .resume(&server, Arc::clone(&old.test.home), rollout)
                .await
                .expect("actual fresh registry resumes the untouched native rollout");
            std::mem::forget(Arc::clone(&fresh.cwd));
            pilot.bind_thread_manager(Arc::downgrade(&fresh.thread_manager));
            let rt = fresh
                .codex
                .thread_extension_data()
                .get::<ThreadRt>()
                .unwrap();
            assert_eq!(rt.key, old.rt.key);
            assert_eq!(rt.accepted_turns.load(Ordering::SeqCst), 0);
            idle_frame(shared, &rt, &fresh);
            observed(|| pilot.sup.acting() && shared.launch_probed()).await;
            assert_eq!(pilot.sup.policy().unwrap().human_grace, Duration::ZERO);
            assert!(shared.swept());
            assert!(matches!(pilot.sup.ready().await, Ready::Go));
            assert!(continuation.requests().is_empty());
            drop(
                lock(&control.permit)
                    .take()
                    .expect("resume admission held until manager bound"),
            );
            observed(|| {
                rt.scanned.load(Ordering::SeqCst)
                    && (rt.timer_pending() || !lock(&control.started).is_empty())
            })
            .await;
            let minimum = wake.at + pilot.timing.wake_settle;
            let settled = minimum + rt.jitter;
            let early_post = if lock(&control.started).is_empty() {
                let owed = rt
                    .owed_timer()
                    .expect("a native start still held before the floor has an owed timer");
                assert!(
                    owed.not_before >= minimum,
                    "a readiness recheck cannot stand in for the wake-settle wait"
                );
                None
            } else {
                // Keep the awake clock fixed until the local provider actually
                // captured the request; native acceptance alone is not a POST.
                observed(|| !continuation.requests().is_empty()).await;
                Some(clock())
            };
            if known_wake {
                assert!(
                    lock(&control.started).is_empty(),
                    "known wake holds native start"
                );
                assert_eq!(rt.owed_timer().unwrap().not_before, settled);
                assert!(continuation.requests().is_empty());
            }
            tokio::time::advance(settled.saturating_duration_since(clock())).await;
            observed(|| !lock(&control.started).is_empty() && !continuation.requests().is_empty())
                .await;
            let started = lock(&control.started).clone();
            assert_eq!(started.len(), 1, "one actual automatic native turn");
            assert_eq!(
                continuation.requests().len(),
                1,
                "one actual loopback model request"
            );
            assert!(
                continuation
                    .single_request()
                    .message_input_texts("user")
                    .iter()
                    .any(|text| text == &original.text),
                "native request contains the exact carried continuation"
            );
            eprintln!(
                "known_wake={known_wake} actual native start after {:?}, local POST observed {:?}, floor {:?}, origin {:?}, carried text {:?}",
                started[0].saturating_duration_since(wake.at),
                early_post.map(|at| at.saturating_duration_since(wake.at)),
                minimum.saturating_duration_since(wake.at),
                native,
                original.text,
            );
            if started[0] < minimum && early_post.is_some_and(|at| at < minimum) {
                early.push((known_wake, started[0].saturating_duration_since(wake.at)));
            }
            fresh
                .codex
                .shutdown_and_wait()
                .await
                .expect("fresh native stopped");
            Autopilot::cancel_timer(&rt).await;
            runnable.abort();
            assert!(runnable.await.unwrap_err().is_cancelled());
            tokio::time::resume();
        }
        assert!(
            early.is_empty(),
            "accepted live handoff lost the recent wake: {early:?}"
        );
    };
    tokio::select! {
        () = exercise => {},
        _ = wall => panic!("native handoff/resume exceeded the one-minute wall hang detector"),
    }
    drop(watch);
}
