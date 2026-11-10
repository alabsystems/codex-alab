// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The real upgrade evaluator, a loaded native core thread and an owned control
//! server. The conditional TUI queue is held while new native facts arrive.

#[path = "upgrade_end_catchup_tests.rs"]
mod end_catchup_tests;

#[path = "upgrade_ask_native_tests.rs"]
mod upgrade_ask_native_tests;

use super::*;
use codex_alab_aterm::Options as ClientOptions;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_core::session::Frame;
use codex_alab_core::session::Launch;
use codex_alab_core::session::ProgramState;
use codex_alab_core::session::Shared;
use codex_alab_core::session::UpgradeRequest;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::AtomicUsize;

#[path = "../tests/autopilot/late_goal.rs"]
mod late_goal;

/// A crash carry-on's provider deadline becomes an owed act during a later live
/// upgrade. Neither fractional waits nor epoch floors can move it earlier, even
/// after several handoffs. The hidden wall-clock fraction is 999 microseconds at
/// each capture; one microsecond later the restore sample advances a millisecond.
#[test]
fn repeated_owed_handoffs_never_shorten_the_provider_deadline() {
    let base = Instant::now();
    let epoch = 1_000_000;
    for fraction in [
        Duration::ZERO,
        Duration::from_nanos(1),
        Duration::from_micros(999),
    ] {
        let mut owed = Owed {
            native_origin: Some(codex_alab_core::session::NativeOrigin {
                turn_id: "capacity-attempt".into(),
                phase: codex_alab_core::session::NativePhase::Interrupted,
            }),
            rule: words::RULE_HOST_TURN,
            text: "Carry on the same task after the provider's wait".into(),
            mode: decide::Mode::Cruise,
            not_before: base + Duration::from_secs(1800) + fraction,
            repo_owed: None,
            resume_goal: false,
            landing: false,
        };
        for handoff in 0..32 {
            let at = base + Duration::from_millis(handoff) + Duration::from_micros(999);
            let carried = owed_act_at("root", Some(&owed), false, None, at, epoch + handoff);
            let restored =
                Autopilot::owed_of_at(&carried, at + Duration::from_micros(1), epoch + handoff + 1)
                    .expect("a future owed act");
            assert!(
                restored.not_before >= owed.not_before,
                "handoff {handoff} shortened the provider's lower bound: {owed:?} -> {restored:?}"
            );
            assert!(restored.not_before <= owed.not_before + Duration::from_millis(2));
            assert_eq!(restored.rule, owed.rule);
            assert_eq!(restored.text, owed.text);
            assert_eq!(restored.mode, owed.mode);
            assert_eq!(restored.native_origin, owed.native_origin);
            owed = restored;
        }
        // A past-due record grants no new waiting period; an empty handoff grants no act.
        let carried = owed_act_at("root", Some(&owed), false, None, base, epoch);
        let overdue = codex_alab_core::session::OwedAct {
            not_before_ms: 0,
            ..carried
        };
        assert_eq!(
            Autopilot::owed_of_at(&overdue, base, epoch)
                .unwrap()
                .not_before,
            base
        );
        let landing_origin = codex_alab_core::session::NativeOrigin {
            turn_id: "landed-attempt".into(),
            phase: codex_alab_core::session::NativePhase::Complete,
        };
        let empty = owed_act_at("root", None, true, Some(&landing_origin), base, epoch);
        assert_eq!(empty.not_before_ms, 0);
        assert_eq!(empty.native_origin.as_ref(), Some(&landing_origin));
        assert!(Autopilot::owed_of_at(&empty, base, epoch).is_none());
    }
}

/// A wake between ready and selection must not turn an expired real-time wait
/// into a fresh awake-time wait, including when the conditional leave is refused.
#[test]
fn a_live_upgrade_counts_sleep_before_carry_and_refused_rollback() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("owned current-thread runtime")
                .block_on(live_upgrade_counts_sleep_before_carry_and_refused_rollback());
        })
        .expect("owned test thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

async fn live_upgrade_counts_sleep_before_carry_and_refused_rollback() {
    use codex_alab_core::wake::Reading;
    use codex_alab_core::wake::Wake;
    use codex_history::RolloutItem;
    use codex_protocol::protocol::EventMsg;
    use serde_json::json;

    struct WallWatch {
        cancel: std::sync::mpsc::Sender<()>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for WallWatch {
        fn drop(&mut self) {
            let _ = self.cancel.send(());
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
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
        let policy_items = [
            json!({"type":"task_started", "turn_id":"refused",
                "turn_attribution":{"turn_id":"refused", "turn_trigger":"user"},
                "model_context_window":258400, "collaboration_mode_kind":"default"}),
            json!({"type":"user_message", "message":"the original inert request",
                "images":null, "local_images":[], "text_elements":[]}),
            json!({"type":"task_complete", "turn_id":"refused", "last_agent_message":null,
                "error":{"message":"the policy refused the action", "codex_error_info":"cyber_policy"}}),
        ].map(|value| RolloutItem::EventMsg(
            serde_json::from_value::<EventMsg>(value).expect("native policy event")
        ));
        let mut failures = Vec::new();
        for policy in [false, true] {
            for sleep in ["none", "historical", "after-ready"] {
                let mut h = Harness::new().await;
                h.retain_artifacts();
                let base = Duration::from_secs(5_000);
                let slept = Duration::from_secs(7_200);
                let armed_asleep = base
                    + if sleep == "historical" {
                        slept
                    } else {
                        Duration::ZERO
                    };
                let reading = Arc::new(std::sync::Mutex::new(Reading {
                    asleep: armed_asleep,
                    last: None,
                }));
                let read = Arc::clone(&reading);
                h.autopilot
                    .set_wake_read(Arc::new(move || Some(*lock(&read))));
                let deadline = clock() + Duration::from_secs(1_200);
                let intended = if policy {
                    let mut state = ThreadState::default();
                    assert!(resume_into(
                        &mut state,
                        &policy_items,
                        false,
                        None,
                        0,
                        clock()
                    ));
                    let decision = decide::decide(
                        &state,
                        &h.autopilot.sup.policy().expect("proven policy"),
                        &h.autopilot.timing,
                        clock(),
                        Duration::ZERO,
                    );
                    assert!(matches!(&decision.act, Act::Type { rule, text }
                        if *rule == words::RULE_POLICY_CONTINUE && text == words::POLICY_CONTINUE));
                    let intended = owes_of(
                        &decision,
                        decide::Mode::Cruise,
                        deadline,
                        false,
                        state.native_origin.clone(),
                    )
                    .expect("the native refusal leaves the bounded policy instruction");
                    h.rt.with_state(|slot| *slot = state);
                    intended
                } else {
                    Owed {
                        not_before: deadline,
                        repo_owed: Some(Vec::new()),
                        ..h.rt.owed_timer().expect("pending paced act")
                    }
                };
                h.rt.with_state(|state| state.asleep_mark = Some(armed_asleep));
                *lock(&h.rt.decided) = Some(h.rt.state().point_at());
                h.autopilot
                    .schedule_act(&h.rt, deadline, Some(intended.clone()));
                let original = h.shared.owed_act().expect("the armed act is published");
                assert!(original.not_before_ms > now_ms());
                assert!(h.fake.acts().is_empty(), "setup submitted no act");
                let (entered, paused) = tokio::sync::oneshot::channel();
                let (resume, release) = tokio::sync::oneshot::channel();
                *lock(&h.autopilot.before_upgrade_take) = Some((entered, release));
                let pilot = Arc::clone(&h.autopilot);
                let mut upgrading = tokio::spawn(async move { pilot.try_upgrade().await });
                tokio::select! {
                    reached = paused => reached.expect("real admission reached act selection"),
                    result = &mut upgrading => panic!("upgrade ended before selection: {result:?}"),
                }
                // Keep paused time from auto-advancing during the fresh async facts scan.
                // The winning wake schedule is an ordering, never a scheduler-speed budget.
                tokio::time::pause();
                let runnable = tokio::spawn(async {
                    loop {
                        tokio::task::yield_now().await;
                    }
                });
                let wake = Wake {
                    at: clock(),
                    slept,
                    seq: 1,
                };
                if sleep == "after-ready" {
                    *lock(&reading) = Reading {
                        asleep: base + slept,
                        last: Some(wake),
                    };
                }
                let synced_before = now_ms();
                h.autopilot.sync_record_owed();
                let synced = h
                    .shared
                    .owed_act()
                    .expect("the pending act remains published");
                let synced_epoch = now_ms();
                let carried_before = now_ms();
                resume.send(()).expect("release admitted upgrade");
                let request = tokio::select! {
                    request = h.requests.recv() => request.expect("conditional upgrade request"),
                    result = &mut upgrading => panic!("upgrade ended before its conditional request: {result:?}"),
                };
                let carried = h.shared.owed_act().expect("the selected act is carried");
                let carried_epoch = now_ms();
                // Queue ahead of an immediately due restored timer's evaluation.
                let capture = h.rt.hold_eval_when_free();
                tokio::pin!(capture);
                std::future::poll_fn(|cx| {
                    assert!(std::future::Future::poll(capture.as_mut(), cx).is_pending());
                    std::task::Poll::Ready(())
                })
                .await;
                let rollback_before = now_ms();
                assert!(!request.decide(false));
                assert!(!upgrading.await.expect("refused upgrade settles"));
                let capture = capture.await;
                let restored = h.rt.owed_timer().expect("refusal restores the exact act");
                let rollback_after = now_ms();
                let mut identity = restored.clone();
                identity.not_before = intended.not_before;
                assert_eq!(identity, intended);
                for record in [&synced, &carried] {
                    assert_eq!(record.rule.as_deref(), Some(intended.rule));
                    assert_eq!(record.text.as_deref(), Some(intended.text.as_str()));
                    assert_eq!(record.native_origin, intended.native_origin);
                    if policy {
                        let history = crate::resume_history::latest(&policy_items);
                        assert!(crate::owed_origin::record(record, &history, None).is_some());
                        let mut ordinary = (*record).clone();
                        ordinary.rule = Some(words::RULE_CONTINUE.into());
                        ordinary.text = Some("continue the original task".into());
                        assert!(crate::owed_origin::record(&ordinary, &history, None).is_none());
                        let mut wrong = (*record).clone();
                        wrong
                            .native_origin
                            .as_mut()
                            .expect("failed native turn")
                            .turn_id = "another-turn".into();
                        assert!(crate::owed_origin::record(&wrong, &history, None).is_none());
                    }
                }
                let expected = if sleep == "after-ready" {
                    codex_alab_core::wake::deadline_after(deadline, &wake)
                } else {
                    deadline
                };
                let remaining = expected.saturating_duration_since(clock());
                let millis =
                    u64::try_from(remaining.as_millis()).expect("fixture wait fits the record");
                // epoch_of rounds the floored epoch and fractional wait separately.
                let rounded =
                    millis + u64::from(!remaining.subsec_nanos().is_multiple_of(1_000_000)) + 1;
                if sleep == "after-ready" {
                    for (name, record, epoch) in [
                        ("sync", &synced, synced_epoch),
                        ("carry", &carried, carried_epoch),
                    ] {
                        if record.not_before_ms > epoch.saturating_add(rounded) {
                            failures.push(format!(
                                "policy={policy} {name}: {} ms remain after the sleep",
                                record.not_before_ms.saturating_sub(epoch)
                            ));
                        }
                    }
                    let restored_upper = clock()
                        + Duration::from_millis(
                            carried_epoch
                                .saturating_add(rounded)
                                .saturating_sub(rollback_before),
                        );
                    if restored.not_before > restored_upper {
                        failures.push(format!(
                            "policy={policy} rollback: {:?} remain after the sleep",
                            restored.not_before.saturating_duration_since(clock())
                        ));
                    }
                } else {
                    for (record, before, after) in [
                        (&synced, synced_before, synced_epoch),
                        (&carried, carried_before, carried_epoch),
                    ] {
                        assert!(record.not_before_ms >= before.saturating_add(millis));
                        assert!(record.not_before_ms <= after.saturating_add(rounded));
                    }
                    assert!(
                        restored.not_before
                            >= clock()
                                + Duration::from_millis(
                                    carried.not_before_ms.saturating_sub(rollback_after)
                                )
                    );
                    assert!(
                        restored.not_before
                            <= clock()
                                + Duration::from_millis(
                                    carried.not_before_ms.saturating_sub(rollback_before)
                                )
                    );
                }
                assert!(!h.shared.exec_pending());
                assert!(!h.shared.turn_running_on(h.rt.key()));
                assert!(!h.fake.acts().iter().any(|rule| rule == intended.rule));
                Autopilot::cancel_timer(&h.rt).await;
                drop(capture);
                runnable.abort();
                let _ = runnable.await;
                tokio::time::resume();
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("native shutdown");
            }
        }
        assert!(
            failures.is_empty(),
            "sleep was re-aged during handoff: {failures:#?}"
        );
    };
    tokio::select! {
        () = exercise => {},
        _ = wall => panic!("sleep handoff real-wall hang detector"),
    }
    drop(watch);
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
#[tokio::test(start_paused = true)]
async fn a_fresh_recheck_does_not_borrow_an_older_intents_sleep_origin() {
    use codex_alab_core::wake::POLL;
    use codex_alab_core::wake::Reading;
    use codex_alab_core::wake::Wake;

    let mut early_recheck = false;
    for (same_deadline, unread_arm) in [(true, true), (false, false), (false, true)] {
        codex_alab_core::activate();
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let sup = Supervisor::new(crate::SupervisorOptions {
            shared,
            endpoint: Arc::new(|| None),
            client: ClientOptions::default(),
        });
        let autopilot = Autopilot::new(sup, None, None, Timing::default(), |_| true);
        let rt = Arc::new(ThreadRt::new(ThreadId::new(), None, false, Duration::ZERO));
        autopilot.keep_thread(&rt);
        let now = clock();
        let sec = Duration::from_secs(1);
        let base = 5_000 * sec;
        let slept = 120 * sec;
        let first = AtomicBool::new(true);
        autopilot.set_wake_read(Arc::new(move || {
            if first.swap(false, Ordering::SeqCst) && unread_arm {
                None
            } else {
                Some(Reading {
                    asleep: base + slept,
                    last: Some(Wake {
                        at: now,
                        slept,
                        seq: 1,
                    }),
                })
            }
        }));
        let recheck = now + 60 * sec;
        let owed = Owed {
            native_origin: None,
            rule: words::RULE_CONTINUE,
            text: "Keep the independent instruction unchanged.".into(),
            mode: decide::Mode::Cruise,
            not_before: if same_deadline { recheck } else { now },
            repo_owed: None,
            resume_goal: false,
            landing: false,
        };
        let permit = rt.hold_eval();
        assert!(autopilot.arm_timer_tracking(&rt, recheck, None, |_| {
            Some(TrackedOwed::from_state(owed, Some(base)))
        }));
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        tokio::time::advance(5 * sec).await;
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        tokio::time::advance(POLL).await;
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        if same_deadline {
            assert_eq!(
                rt.evaluations(),
                1,
                "the original wait counts measured sleep"
            );
        } else if !unread_arm {
            assert_eq!(
                rt.evaluations(),
                0,
                "a fresh readable recheck starts after that sleep"
            );
            tokio::time::advance(60 * sec).await;
            for _ in 0..16 {
                tokio::task::yield_now().await;
            }
            assert_eq!(
                rt.evaluations(),
                1,
                "the recheck eventually reaches its own deadline"
            );
        } else {
            early_recheck = rt.evaluations() != 0;
        }
        Autopilot::cancel_timer(&rt).await;
        drop(permit);
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
    }
    assert!(
        !early_recheck,
        "an unread fresh recheck counted historical intent sleep"
    );
}

fn cli() -> clap::Command {
    clap::Command::new("codex")
        .subcommand(clap::Command::new("resume").arg(clap::Arg::new("session_id")))
}

pub(super) async fn wait_for_supervisor(ready: impl Fn() -> bool) {
    // The outer one-minute hang detector bounds this wait. Supervisor state can move
    // after the fake's last response, so the fake's own change stream cannot wake it.
    while !ready() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Native Core setup and turns use upstream's explicit stack convention; the
/// ordinary libtest stack remains unchanged for ALab's pure tests.
pub(super) fn native_test<F, Fut>(test: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    core_test_support::test_codex::run_test_with_large_stack(
        "codex-alab native core",
        move || async move {
            test().await;
            Ok(())
        },
    )
    .expect("native Core test");
}

pub(super) struct Harness {
    pub(super) autopilot: Arc<Autopilot>,
    pub(super) fake: FakeAterm,
    pub(super) shared: &'static Shared,
    pub(super) rt: Arc<ThreadRt>,
    pub(super) test: TestCodex,
    pub(super) requests: tokio::sync::mpsc::UnboundedReceiver<UpgradeRequest>,
    deadline: Instant,
    _dir: tempfile::TempDir,
    pub(super) _server: wiremock::MockServer,
}

impl Harness {
    /// Keep a new regression fixture's roots, including on an assertion failure.
    pub(super) fn retain_artifacts(&mut self) {
        self._dir.disable_cleanup(true);
        // Core may retain other Arc owners. Keeping one owner of each TempDir
        // prevents its eventual cleanup without changing legacy fixture lifetimes.
        std::mem::forget(Arc::clone(&self.test.home));
        std::mem::forget(Arc::clone(&self.test.cwd));
        eprintln!(
            "kept native fixture roots: {} {} {}",
            self._dir.path().display(),
            self.test.home.path().display(),
            self.test.cwd.path().display()
        );
    }

    pub(super) async fn immediate_point(&self) {
        Autopilot::cancel_timer(&self.rt).await;
        self.autopilot.sync_record_owed();
        self.rt.ask_again();
    }

    pub(super) async fn hold_model_response(&self) {
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_delay(Duration::from_secs(60)))
            .mount(&self._server)
            .await;
    }

    pub(super) async fn new() -> Self {
        Self::new_with_goals(None).await
    }

    pub(super) async fn new_with_goals(goals: Option<Arc<GoalService>>) -> Self {
        Self::new_with_goals_and_observer(goals, None).await
    }

    pub(super) async fn new_with_goals_and_setup_observer(
        goals: Option<Arc<GoalService>>,
        setup_observer: Arc<dyn Fn(&'static str) + Send + Sync>,
    ) -> Self {
        Self::new_with_control_timeout_and_setup_observer(
            goals,
            None,
            ClientOptions::default().timeout,
            Some(setup_observer),
        )
        .await
    }

    pub(super) async fn new_with_goals_and_observer(
        goals: Option<Arc<GoalService>>,
        observer: Option<Arc<dyn TurnLifecycleContributor>>,
    ) -> Self {
        let timeout = if observer.is_some() {
            Duration::from_secs(60)
        } else {
            ClientOptions::default().timeout
        };
        Self::new_with_control_timeout(goals, observer, timeout).await
    }

    pub(super) async fn new_with_control_timeout(
        goals: Option<Arc<GoalService>>,
        observer: Option<Arc<dyn TurnLifecycleContributor>>,
        timeout: Duration,
    ) -> Self {
        Self::new_with_control_timeout_and_setup_observer(goals, observer, timeout, None).await
    }

    async fn new_with_control_timeout_and_setup_observer(
        goals: Option<Arc<GoalService>>,
        observer: Option<Arc<dyn TurnLifecycleContributor>>,
        timeout: Duration,
        setup_observer: Option<Arc<dyn Fn(&'static str) + Send + Sync>>,
    ) -> Self {
        let observe = |phase| {
            if let Some(observer) = &setup_observer {
                observer(phase);
            }
        };
        observe("setup: create fake aterm");
        codex_alab_core::activate();
        let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
        let fake = FakeAterm::start(dir.path(), Mode::Current)
            .await
            .unwrap_or_else(|err| panic!("{err}"));
        fake.set_policy_key("human_grace_s", "0");
        observe("setup: register extensions");
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let endpoint = fake.endpoint();
        let client = ClientOptions {
            timeout,
            ..ClientOptions::default()
        };
        let sup = Supervisor::new(crate::SupervisorOptions {
            shared,
            endpoint: Arc::new(move || Some(endpoint.clone())),
            client,
        });
        let mut builder = codex_extension_api::ExtensionRegistryBuilder::<Config>::new();
        let autopilot = Autopilot::new(sup, None, goals.clone(), Timing::default(), |_| true);
        // This fixture supplies the decided point and timer itself. Do not also launch
        // an unrelated claim-change evaluation that can supersede them after setup.
        autopilot.watching.store(true, Ordering::SeqCst);
        autopilot.register(&mut builder);
        if let Some(observer) = observer {
            builder.turn_lifecycle_contributor(observer);
        }
        let late_goal = goals.as_ref().map(|_| {
            let late = Arc::new(late_goal::LateGoal::default());
            late.register(&mut builder);
            late
        });
        observe("setup: start mock server");
        let server = responses::start_mock_server().await;
        let mut test_builder = test_codex().with_extensions(Arc::new(builder.build()));
        if let Some(observer) = &setup_observer {
            let pre_build = Arc::clone(observer);
            let configured = Arc::clone(observer);
            let workspace = Arc::clone(observer);
            let manager = Arc::clone(observer);
            test_builder = test_builder
                .with_pre_build_hook(move |_| {
                    pre_build("setup: default config and tool discovery");
                })
                .with_config(move |_| {
                    configured("setup: post-config and environment preparation");
                })
                .with_workspace_setup(move |_, _| async move {
                    workspace("setup: state database and managers");
                    Ok(())
                })
                .with_thread_manager(move |thread_manager| {
                    manager("setup: native thread startup");
                    thread_manager
                });
        }
        observe("setup: create native test environment");
        let test = test_builder
            .build(&server)
            .await
            .unwrap_or_else(|err| panic!("{err}"));
        observe("setup: bind goal runtime");
        autopilot.bind_thread_manager(Arc::downgrade(&test.thread_manager));
        if let (Some(late), Some(goals)) = (&late_goal, goals) {
            late.bind(&test, goals).await;
        }
        observe("setup: fixture paths and launch identity");
        let rt = test
            .codex
            .thread_extension_data()
            .get::<ThreadRt>()
            .unwrap_or_else(|| panic!("native root runtime"));
        let old = dir.path().join("old-build");
        let target = dir.path().join("new-build");
        for path in [&old, &target] {
            std::fs::write(path, b"captured image; never executed")
                .unwrap_or_else(|err| panic!("{err}"));
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .unwrap_or_else(|err| panic!("{err}"));
        }
        let id =
            codex_alab_tui::upgrade::build_id(&old).unwrap_or_else(|| panic!("build identity"));
        shared.set_launch(Launch {
            args: vec!["codex".into()],
            argv0: Some(target),
            current_exe: Some(old),
            exe_id: Some((id.dev, id.ino)),
            cwd: Some(dir.path().to_path_buf()),
            ..Default::default()
        });
        codex_alab_tui::resume_argv::set_command(cli);
        let rollout = dir.path().join("rollout.jsonl");
        std::fs::write(&rollout, b"materialized rollout\n").unwrap_or_else(|err| panic!("{err}"));
        shared.observe(
            &Frame {
                primary_thread: Some(rt.key()),
                displayed_thread: Some(rt.key()),
                rollout_path: Some(&rollout),
                cwd: dir.path(),
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
        observe("setup: acquire supervisor claim");
        wait_for_supervisor(|| autopilot.sup.acting()).await;
        observe("setup: seed decision and timer");
        let at = clock();
        rt.with_state(|state| {
            state.task = true;
            state.point = Some(TurnEnd {
                at,
                by_autopilot: true,
                by_person: false,
                text: Some("Continue the current task".into()),
                earlier: None,
                questions: false,
                worked: Duration::from_secs(600),
                wall: None,
                aborted: false,
                ran_tools: false,
                restored: false,
            });
        });
        *lock(&rt.decided) = Some(Some(at));
        let deadline = at + Duration::from_secs(300);
        autopilot.schedule_act(
            &rt,
            deadline,
            Some(Owed {
                native_origin: None,
                rule: crate::words::RULE_DONE_CHECK,
                text: "Preserve this exact pending act".into(),
                mode: crate::decide::Mode::Cruise,
                not_before: deadline,
                repo_owed: None,
                resume_goal: false,
                landing: false,
            }),
        );
        let (queue, requests) = tokio::sync::mpsc::unbounded_channel();
        shared.set_upgrade_leave(Box::new(move |request| {
            let _ = queue.send(request);
        }));
        Self {
            autopilot,
            fake,
            shared,
            rt,
            test,
            requests,
            deadline,
            _dir: dir,
            _server: server,
        }
    }
}

/// A long successful turn has no short-streak timer to carry. Its freshly
/// decided continuation must reach the upgrade queue before it starts a new
/// native turn, rather than relying on a minute poll landing in that gap.
#[test]
fn an_immediate_continuation_after_real_work_yields_to_the_new_build() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let mut h = Harness::new().await;
            h.immediate_point().await;
            let decision = decide::decide(
                &h.rt.state(),
                &h.autopilot.sup.policy().expect("proven policy"),
                &h.autopilot.timing,
                clock(),
                Duration::ZERO,
            );
            assert!(
                matches!(&decision.act, Act::Type { rule, .. } if *rule == words::RULE_CONTINUE)
            );
            assert!(decision.not_before.is_none_or(|at| at <= clock()));
            assert!(h.rt.state().point.as_ref().unwrap().worked > h.autopilot.timing.progress);
            let session = codex_extension_api::ExtensionData::new("session");
            h.autopilot
                .on_thread_idle(ThreadIdleInput {
                    cause: codex_extension_api::ThreadIdleCause::Completed,
                    session_store: &session,
                    thread_store: h.test.codex.thread_extension_data(),
                })
                .await;
            let request = tokio::select! {
                request = h.requests.recv() => request.expect("conditional upgrade"),
                _ = wait_for_supervisor(|| h.shared.turn_running_on(h.rt.key())) => {
                    panic!("the immediate continuation started before considering the new build")
                }
            };
            let owed = h
                .shared
                .owed_act()
                .expect("the newly decided act is carried");
            assert_eq!(owed.rule.as_deref(), Some(words::RULE_CONTINUE));
            assert!(!h.shared.turn_running_on(h.rt.key()));
            assert!(request.decide(true));
            wait_for_supervisor(|| h.shared.exec_pending()).await;
            assert!(!h.fake.acts().iter().any(|act| act == words::RULE_CONTINUE));
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("immediate upgrade hang detector");
    });
}

#[test]
fn an_immediate_continuation_survives_refusal_but_never_supersedes_new_input() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
        for change in ["refused", "draft", "native", "write-failed"] {
            let mut h = Harness::new().await;
            h.immediate_point().await;
            h.hold_model_response().await;
            let decision = decide::decide(
                &h.rt.state(),
                &h.autopilot.sup.policy().expect("proven policy"),
                &h.autopilot.timing,
                clock(),
                Duration::ZERO,
            );
            let Act::Type { rule, text } = decision.act else {
                panic!("the immediate point owes a continuation");
            };
            assert_eq!(rule, words::RULE_CONTINUE);
            let writes = Arc::new(AtomicUsize::new(0));
            let refused_carry = Arc::new(std::sync::Mutex::new(None));
            if change == "write-failed" {
                let writes = Arc::clone(&writes);
                let refused_carry = Arc::clone(&refused_carry);
                let shared = h.shared;
                assert!(shared.set_flush(Box::new(move || {
                    if writes.fetch_add(1, Ordering::SeqCst) == 0 {
                        *lock(&refused_carry) = shared.owed_act();
                        false
                    } else {
                        true
                    }
                })));
            }
            let evaluations = h.rt.evaluations();
            let pilot = Arc::clone(&h.autopilot);
            let rt = Arc::clone(&h.rt);
            let mut evaluating = tokio::spawn(async move { pilot.evaluate(rt).await });
            let request = if change == "write-failed" {
                tokio::select! {
                    request = h.requests.recv() => {
                        request.expect("conditional upgrade").decide(false);
                        panic!("a failed carry write cannot queue teardown");
                    }
                    result = &mut evaluating => result.expect("evaluation completes"),
                }
                let carry = lock(&refused_carry).clone().expect("the unsaved continuation");
                assert_eq!(carry.rule.as_deref(), Some(words::RULE_CONTINUE));
                assert_eq!(carry.text.as_deref(), Some(text.as_str()));
                assert!(writes.load(Ordering::SeqCst) >= 2, "the ordinary act can persist after refusal");
                None
            } else {
                Some(h.requests.recv().await.expect("conditional upgrade"))
            };
            if let Some(request) = request {
            assert_eq!(
                h.shared.owed_act().unwrap().rule.as_deref(),
                Some(words::RULE_CONTINUE)
            );
            match change {
                "refused" => {
                    assert!(!request.decide(false));
                }
                "draft" => {
                    let rollout = h._dir.path().join("rollout.jsonl");
                    h.shared.observe(
                        &Frame {
                            primary_thread: Some(h.rt.key()),
                            displayed_thread: Some(h.rt.key()),
                            rollout_path: Some(&rollout),
                            cwd: h._dir.path(),
                            model: "gpt-5.5",
                            codex_home: h.test.codex_home_path(),
                            embedded: true,
                            state: ProgramState::Idle,
                            draft_empty: false,
                            queued_input: false,
                            popup_open: false,
                            editor_open: false,
                            pending_user_turn: false,
                        },
                        now_ms(),
                    );
                    h.shared.human_input(now_ms());
                    assert!(!request.decide(true));
                }
                "native" => {
                    use codex_protocol::config_types::CollaborationMode;
                    use codex_protocol::config_types::ModeKind;
                    use codex_protocol::config_types::Settings;
                    let session = codex_extension_api::ExtensionData::new("session");
                    let turn = codex_extension_api::ExtensionData::new("new-native-turn");
                    let mode = CollaborationMode {
                        mode: ModeKind::Default,
                        settings: Settings {
                            model: "gpt-5.5".into(),
                            reasoning_effort: None,
                            developer_instructions: None,
                        },
                    };
                    h.autopilot
                        .on_turn_start(TurnStartInput {
                            turn_id: "new-native-turn",
                            collaboration_mode: &mode,
                            token_usage_at_turn_start: None,
                            session_store: &session,
                            thread_store: h.test.codex.thread_extension_data(),
                            turn_store: &turn,
                        })
                        .await;
                    h.autopilot
                        .on_turn_stop(codex_extension_api::TurnStopInput {
                            session_store: &session,
                            thread_store: h.test.codex.thread_extension_data(),
                            turn_store: &turn,
                        })
                        .await;
                    assert!(!request.decide(true));
                }
                _ => unreachable!(),
            }
            evaluating.await.expect("evaluation completes");
            }
            assert!(!h.shared.exec_pending());
            if matches!(change, "refused" | "write-failed") {
                tokio::select! {
                    retry = h.requests.recv() => {
                        retry.expect("another handoff").decide(false);
                        panic!("a refused immediate upgrade was offered again instead of continuing");
                    }
                    _ = wait_for_supervisor(|| {
                    h.fake
                        .acts()
                        .iter()
                        .filter(|act| *act == words::RULE_CONTINUE)
                        .count()
                        == 1
                    }) => {}
                }
                assert!(h.shared.turn_running_on(h.rt.key()));
                assert_eq!(h.rt.evaluations(), evaluations + 1, "the original evaluator alone owns synchronous refusal");
                assert!(
                    h.rt.owed_timer().is_none(),
                    "one submission consumes the restored act"
                );
                let requests = loop {
                    let requests = responses::received_responses_requests(&h._server).await;
                    if !requests.is_empty() {
                        break requests;
                    }
                    tokio::task::yield_now().await;
                };
                assert_eq!(requests.len(), 1);
                assert_eq!(
                    requests[0]
                        .message_input_texts("user")
                        .iter()
                        .filter(|input| input.as_str() == text.as_str())
                        .count(),
                    1
                );
            } else {
                assert!(!h.shared.turn_running_on(h.rt.key()));
                assert!(!h.fake.acts().iter().any(|act| act == words::RULE_CONTINUE));
                if change == "draft" {
                    assert_eq!(h.rt.owed_timer().unwrap().rule, words::RULE_CONTINUE);
                } else {
                    assert!(
                        h.rt.owed_timer().is_none(),
                        "new native acceptance revokes the old act"
                    );
                    assert!(h.shared.owed_act().is_none());
                }
            }
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        }
    })
    .await
    .expect("immediate refusal hang detector");
    });
}

#[test]
fn cancelling_an_immediate_upgrade_before_its_scan_keeps_the_point_undecided() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.immediate_point().await;
            let (entered, entry) = tokio::sync::oneshot::channel();
            let (_release, blocked) = tokio::sync::oneshot::channel();
            *lock(&h.autopilot.upgrade_begin) = Some((entered, blocked));
            let pilot = Arc::clone(&h.autopilot);
            let rt = Arc::clone(&h.rt);
            let evaluating = tokio::spawn(async move { pilot.evaluate(rt).await });
            entry.await.expect("upgrade entered");
            evaluating.abort();
            assert!(evaluating.await.is_err());
            assert!(!h.autopilot.upgrading.load(Ordering::SeqCst));
            assert!(!h.shared.exec_pending());
            assert!(
                h.rt.undecided(),
                "cancellation cannot authorize an empty handoff"
            );
            assert_eq!(
                h.autopilot.upgrade_facts().await.undecided.as_deref(),
                Some(h.rt.key())
            );
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("immediate cancellation hang detector");
    });
}

#[test]
fn cancelling_a_queued_immediate_upgrade_restores_one_continuation_without_reoffering_it() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
        let mut h = Harness::new().await;
        h.immediate_point().await;
        h.hold_model_response().await;
        let pilot = Arc::clone(&h.autopilot);
        let rt = Arc::clone(&h.rt);
        let evaluating = tokio::spawn(async move { pilot.evaluate(rt).await });
        let request = h.requests.recv().await.expect("conditional upgrade");
        evaluating.abort();
        assert!(evaluating.await.is_err());
        assert!(!request.decide(true));
        tokio::select! {
            retry = h.requests.recv() => {
                retry.expect("another handoff").decide(false);
                panic!("a restored immediate act must not re-offer its cancelled upgrade");
            }
            _ = wait_for_supervisor(|| h.fake.acts().iter().any(|act| act == words::RULE_CONTINUE)) => {}
        }
        assert_eq!(h.fake.acts().iter().filter(|act| *act == words::RULE_CONTINUE).count(), 1);
        assert!(h.shared.turn_running_on(h.rt.key()));
        assert!(h.rt.owed_timer().is_none());
        assert!(!h.shared.exec_pending());
        h.test.codex.shutdown_and_wait().await.expect("native shutdown");
    }).await.expect("queued cancellation hang detector");
    });
}

#[test]
fn an_immediate_upgrade_redecides_after_policy_or_off_changes() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            for change in ["mode", "settings", "off-on"] {
                let mut h = Harness::new().await;
                h.immediate_point().await;
                let pilot = Arc::clone(&h.autopilot);
                let rt = Arc::clone(&h.rt);
                let evaluating = tokio::spawn(async move { pilot.evaluate(rt).await });
                let request = h.requests.recv().await.expect("initial upgrade");
                match change {
                    "mode" => {
                        h.fake.set_status("autopilot", "land");
                        h.fake.set_policy_key("autopilot", "land");
                        wait_for_supervisor(|| {
                            h.autopilot
                                .sup
                                .policy()
                                .is_some_and(|p| p.autopilot == decide::Mode::Land)
                        })
                        .await;
                    }
                    "settings" => {
                        h.fake.set_policy_key("questions", "ask");
                        wait_for_supervisor(|| {
                            h.autopilot
                                .sup
                                .policy()
                                .is_some_and(|p| p.questions == decide::Questions::Ask)
                        })
                        .await;
                    }
                    "off-on" => {
                        h.fake.set_effective(false, "autopilot-off");
                        wait_for_supervisor(|| !h.autopilot.sup.held()).await;
                        h.fake.set_effective(true, "enabled");
                        wait_for_supervisor(|| h.autopilot.sup.acting()).await;
                    }
                    _ => unreachable!(),
                }
                assert!(!request.decide(true), "the old policy cannot commit leave");
                evaluating.await.expect("old evaluation returns");
                let next = h
                    .requests
                    .recv()
                    .await
                    .expect("a fresh evaluation owns the new policy");
                let owed = h.shared.owed_act().expect("fresh decided act");
                if change == "mode" {
                    assert!(owed.rule.as_deref().is_some_and(words::is_land_rule));
                    assert_eq!(owed.mode.as_deref(), Some("land"));
                } else {
                    assert_eq!(owed.rule.as_deref(), Some(words::RULE_CONTINUE));
                }
                assert_eq!(h.rt.off_seen.load(Ordering::SeqCst), h.autopilot.sup.offs());
                assert!(!h.fake.acts().iter().any(|act| act == words::RULE_CONTINUE));
                assert!(next.decide(true));
                wait_for_supervisor(|| h.shared.exec_pending()).await;
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("native shutdown");
            }
        })
        .await
        .expect("policy change hang detector");
    });
}

/// The selected timer keeps its own point across refusal and cancellation, even
/// when a different thread was the only candidate in the first live snapshot.
#[test]
fn a_changed_first_thread_cannot_lose_the_second_threads_suspended_act() {
    struct AbortOnDrop(tokio::task::AbortHandle);
    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    struct ReleaseBlocking(Option<std::sync::mpsc::Sender<()>>);
    impl Drop for ReleaseBlocking {
        fn drop(&mut self) {
            if let Some(release) = self.0.take() {
                let _ = release.send(());
            }
        }
    }
    native_test(|| async {
        for (fresh, cancel, change_b) in [
            (false, false, "same"),
            (false, true, "same"),
            (true, false, "same"),
            (true, true, "same"),
            (false, false, "timer"),
            (false, true, "native"),
        ] {
            let mut h = tokio::time::timeout(Duration::from_secs(60), Harness::new())
                .await
                .expect("native fixture hang detector");
            h.retain_artifacts();
            let first_owed = h.rt.owed_timer().expect("first thread's pending act");
            let first_tracked =
                h.rt.tracked_owed_timer()
                    .expect("first thread's tracked act");
            let first_point = h.rt.state().point_at();
            let accepted_turns = h.rt.accepted_turns.load(Ordering::SeqCst);
            let _own_eval = if fresh {
                tokio::time::timeout(Duration::from_secs(60), h.immediate_point())
                    .await
                    .expect("first timer cancellation hang detector");
                *lock(&h.rt.decided) = Some(first_point);
                Some(
                    tokio::time::timeout(
                        Duration::from_secs(60),
                        Arc::clone(&h.rt.eval).acquire_owned(),
                    )
                    .await
                    .expect("first evaluator hang detector")
                    .expect("first evaluator"),
                )
            } else {
                None
            };
            let decided = fresh.then(|| DecidedUpgradeAct {
                rt: Arc::clone(&h.rt),
                owed: first_tracked.clone(),
                point: first_point,
                accepted_turns,
            });
            let (entered, paused) = tokio::sync::oneshot::channel();
            let (resume, release) = tokio::sync::oneshot::channel();
            *lock(&h.autopilot.before_upgrade_take) = Some((entered, release));
            let pilot = Arc::clone(&h.autopilot);
            let mut upgrading =
                tokio::spawn(async move { pilot.try_upgrade_carrying(decided.as_ref()).await });
            let _abort = AbortOnDrop(upgrading.abort_handle());
            // These owners remain outside the timed future. Timeout cancels and drains
            // the evaluator, releases the unabortable blocking stand-in and waits for it.
            let mut blocking = None::<ReleaseBlocking>;
            let mut blocking_finished = None;
            let mut blocking_handle = None;
            let mut second_thread = None::<Arc<CodexThread>>;
            let mut second_eval = None;
            let mut settled = false;
            let checked = tokio::time::timeout(Duration::from_secs(60), async {
                tokio::select! {
                    reached = paused => reached.expect("upgrade reached act selection"),
                    result = &mut upgrading => panic!("upgrade ended before selection: {result:?}"),
                }
                let second = h
                    .test
                    .thread_manager
                    .start_thread(h.test.start_thread_options().await)
                    .await
                    .expect("second native thread")
                    .thread;
                second_thread = Some(Arc::clone(&second));
                let rt = second
                    .thread_extension_data()
                    .get::<ThreadRt>()
                    .expect("second native runtime");
                second_eval = Some(
                    Arc::clone(&rt.eval)
                        .acquire_owned()
                        .await
                        .expect("second evaluator"),
                );
                let mut state = h.rt.state();
                state.point.as_mut().expect("first point").at += Duration::from_secs(1);
                let point = state.point_at();
                rt.with_state(|slot| *slot = state);
                *lock(&rt.decided) = Some(point);
                let mut owed = first_owed;
                owed.text = "Preserve the second thread's exact act".into();
                let deadline = owed.not_before;
                h.autopilot.schedule_act(&rt, deadline, Some(owed.clone()));

                let (joining, joined) = tokio::sync::oneshot::channel();
                let (finished, observed_finished) = tokio::sync::oneshot::channel();
                let (release_join, wait_join) = std::sync::mpsc::channel::<()>();
                blocking = Some(ReleaseBlocking(Some(release_join)));
                blocking_finished = Some(observed_finished);
                let handle = tokio::task::spawn_blocking(move || {
                    let _ = joining.send(());
                    let _ = wait_join.recv();
                    let _ = finished.send(());
                });
                blocking_handle = Some(handle.abort_handle());
                joined.await.expect("owned timer stand-in started");
                let old = {
                    let mut timer = lock(&rt.timer);
                    std::mem::replace(&mut timer.as_mut().expect("second timer").handle, handle)
                };
                old.abort();
                let _ = old.await;

                use codex_protocol::config_types::CollaborationMode;
                use codex_protocol::config_types::ModeKind;
                use codex_protocol::config_types::Settings;
                let session = codex_extension_api::ExtensionData::new("session");
                let turn = codex_extension_api::ExtensionData::new("superseding-first-thread");
                let mode = CollaborationMode {
                    mode: ModeKind::Default,
                    settings: Settings {
                        model: "gpt-5.5".into(),
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                };
                h.autopilot
                    .on_turn_start(TurnStartInput {
                        turn_id: "superseding-first-thread",
                        collaboration_mode: &mode,
                        token_usage_at_turn_start: None,
                        session_store: &session,
                        thread_store: h.test.codex.thread_extension_data(),
                        turn_store: &turn,
                    })
                    .await;
                h.autopilot
                    .on_turn_abort(TurnAbortInput {
                        reason: codex_protocol::protocol::TurnAbortReason::Interrupted,
                        session_store: &session,
                        thread_store: h.test.codex.thread_extension_data(),
                        turn_store: &turn,
                    })
                    .await;
                assert!(
                    h.rt.owed_timer().is_none(),
                    "native input superseded only A"
                );
                resume.send(()).expect("resume selection");
                wait_for_supervisor(|| !rt.timer_pending()).await;
                assert!(
                    !upgrading.is_finished(),
                    "B's timer join is still suspended"
                );

                let newer = if change_b == "timer" {
                    let mut newer = owed.clone();
                    newer.text = "A newer wait for the same native point".into();
                    newer.not_before += Duration::from_secs(60);
                    h.autopilot
                        .schedule_act(&rt, newer.not_before, Some(newer.clone()));
                    Some(newer)
                } else {
                    None
                };
                if change_b == "native" {
                    let turn = codex_extension_api::ExtensionData::new("superseding-second-thread");
                    h.autopilot
                        .on_turn_start(TurnStartInput {
                            turn_id: "superseding-second-thread",
                            collaboration_mode: &mode,
                            token_usage_at_turn_start: None,
                            session_store: &session,
                            thread_store: second.thread_extension_data(),
                            turn_store: &turn,
                        })
                        .await;
                    h.autopilot
                        .on_turn_abort(TurnAbortInput {
                            reason: codex_protocol::protocol::TurnAbortReason::Interrupted,
                            session_store: &session,
                            thread_store: second.thread_extension_data(),
                            turn_store: &turn,
                        })
                        .await;
                    // Equal clock stamps do not undo an accepted native turn.
                    rt.with_state(|state| {
                        if let (Some(end), Some(at)) = (&mut state.point, point) {
                            end.at = at;
                        }
                    });
                }
                if cancel {
                    upgrading.abort();
                    assert!((&mut upgrading).await.is_err(), "cancel during B's join");
                } else {
                    drop(blocking.take());
                    assert!(!(&mut upgrading).await.expect("upgrade refusal"));
                }
                settled = true;
                if let Some(newer) = newer {
                    let kept = rt.owed_timer().expect("newer timer remains");
                    assert_eq!(kept.text, newer.text);
                    assert_eq!(kept.not_before, newer.not_before);
                } else if change_b == "native" {
                    assert!(
                        rt.owed_timer().is_none(),
                        "B's superseded act cannot return"
                    );
                } else {
                    let restored = rt.owed_timer().expect("B's act survives rollback");
                    assert_eq!(restored.text, owed.text);
                    assert_eq!(restored.rule, owed.rule);
                    assert_eq!(restored.mode, owed.mode);
                    assert_eq!(restored.native_origin, owed.native_origin);
                    assert!(
                        restored.not_before >= deadline,
                        "provider wait is not shortened"
                    );
                }
                assert!(h.rt.owed_timer().is_none(), "A stays superseded");
                assert!(!h.shared.exec_pending());
            })
            .await;
            if checked.is_err() {
                upgrading.abort();
            }
            drop(blocking.take());
            if !settled {
                let _ = tokio::time::timeout(Duration::from_secs(60), &mut upgrading)
                    .await
                    .expect("upgrade cancellation drain hang detector");
            }
            if let Some(finished) = blocking_finished {
                tokio::time::timeout(Duration::from_secs(60), finished)
                    .await
                    .expect("blocking timer settlement hang detector")
                    .expect("blocking timer settled");
            }
            if let Some(handle) = blocking_handle {
                tokio::time::timeout(
                    Duration::from_secs(60),
                    wait_for_supervisor(|| handle.is_finished()),
                )
                .await
                .expect("blocking timer task finish hang detector");
            }
            drop(second_eval);
            if let Some(second) = second_thread {
                tokio::time::timeout(Duration::from_secs(60), second.shutdown_and_wait())
                    .await
                    .expect("second shutdown hang detector")
                    .expect("second shutdown");
            }
            tokio::time::timeout(Duration::from_secs(60), h.test.codex.shutdown_and_wait())
                .await
                .expect("first shutdown hang detector")
                .expect("first shutdown");
            checked.expect("selected-thread handoff hang detector");
        }
    });
}

/// A retired instance cannot lend its words to a replacement with the same key,
/// even when that replacement starts with equal point and native-turn counters.
#[test]
fn a_same_key_replacement_keeps_its_own_selected_act() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let mut h = Harness::new().await;
            h.retain_artifacts();
            let old = Arc::clone(&h.rt);
            let old_owed = old.owed_timer().expect("old runtime wait");
            let point = old.state().point_at();
            let accepted_turns = old.accepted_turns.load(Ordering::SeqCst);
            let decided = DecidedUpgradeAct {
                rt: Arc::clone(&old),
                owed: old.tracked_owed_timer().expect("old runtime tracked wait"),
                point,
                accepted_turns,
            };
            // Model registration after try_upgrade's initial checks, using its
            // existing runtime-registry seam. The official callback retires the old
            // instance before registering the replacement in the same way.
            let replacement = Arc::new(ThreadRt::new(
                ThreadId::from_string(&old.key).expect("same thread id"),
                old.config.clone(),
                old.token_budget,
                Duration::ZERO,
            ));
            replacement.with_state(|state| *state = old.state());
            replacement
                .accepted_turns
                .store(accepted_turns, Ordering::SeqCst);
            *lock(&replacement.decided) = Some(point);
            old.stopped.store(true, Ordering::SeqCst);
            h.autopilot.keep_thread(&replacement);
            Autopilot::cancel_timer(&old).await;
            let mut replacement_owed = old_owed;
            replacement_owed.text = "The replacement runtime's own wait".into();
            h.autopilot.schedule_act(
                &replacement,
                replacement_owed.not_before,
                Some(replacement_owed.clone()),
            );
            let mut handoff = UpgradeHandoff {
                autopilot: Arc::clone(&h.autopilot),
                owed: None,
            };
            let (selected, carried) = h
                .autopilot
                .take_owed_act_carrying(Some(&decided), Some(&mut handoff))
                .await
                .expect("replacement has an act");
            assert!(Arc::ptr_eq(&selected, &replacement));
            assert_eq!(
                carried.text.as_deref(),
                Some(replacement_owed.text.as_str())
            );
            assert!(!replacement.timer_pending());
            drop(handoff);
            assert_eq!(
                replacement.owed_timer().expect("replacement restored").text,
                replacement_owed.text
            );
            assert!(old.owed_timer().is_none());
            assert!(!h.shared.exec_pending());
            Autopilot::cancel_timer(&replacement).await;
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("same-key selection hang detector");
    });
}

#[test]
fn fresh_native_or_policy_changes_reject_queued_upgrade_and_preserve_only_unsuperseded_acts() {
    native_test(|| async {
        for change in [
            "none",
            "draft",
            "turn",
            "turn-hup",
            "turn-ended",
            "turn-ended-hup",
            "thread",
            "policy",
            "cancel",
            "hup",
            "term-stop",
            "int-stop",
            "normal-stop",
            "write-failed",
            "write-input",
            "no-carry",
        ] {
            // Keep the existing hang bound for each independently isolated fixture.
            tokio::time::timeout(Duration::from_secs(60), async {
                let mut h = Harness::new().await;
                if change == "no-carry" {
                    Autopilot::cancel_timer(&h.rt).await;
                    h.autopilot.sync_record_owed();
                }
                let before = h.rt.owed_timer();
                let writes = Arc::new(AtomicUsize::new(0));
                let saved = Arc::new(std::sync::Mutex::new(None));
                {
                    let writes = Arc::clone(&writes);
                    let saved = Arc::clone(&saved);
                    let shared = h.shared;
                    assert!(shared.set_flush(Box::new(move || {
                        writes.fetch_add(1, Ordering::SeqCst);
                        if matches!(change, "write-failed" | "no-carry") {
                            false
                        } else {
                            *lock(&saved) = shared.owed_act();
                            if change == "write-input" {
                                shared.human_input(now_ms());
                            }
                            true
                        }
                    })));
                }
                let pilot = Arc::clone(&h.autopilot);
                let mut upgrading = tokio::spawn(async move { pilot.try_upgrade().await });
                if change == "write-failed" {
                    tokio::select! {
                        request = h.requests.recv() => {
                            request.expect("conditional upgrade").decide(false);
                            panic!("a failed carry write cannot queue teardown");
                        }
                        result = &mut upgrading => assert!(!result.expect("write refusal")),
                    }
                    assert_eq!(writes.load(Ordering::SeqCst), 1);
                    assert!(!h.shared.exec_pending());
                    assert!(!h.autopilot.upgrade_requested.load(Ordering::SeqCst));
                    let before = before.expect("one paced act");
                    let restored = h.rt.owed_timer().expect("the paced act is restored");
                    assert_eq!(restored.rule, before.rule);
                    assert_eq!(restored.text, before.text);
                    assert_eq!(restored.mode, before.mode);
                    assert_eq!(restored.native_origin, before.native_origin);
                    assert!(restored.not_before >= before.not_before);
                    assert!(h.requests.try_recv().is_err());
                    h.test
                        .codex
                        .shutdown_and_wait()
                        .await
                        .expect("native shutdown");
                    return;
                }
                let request = tokio::select! {
                    request = h.requests.recv() => request.unwrap_or_else(|| {
                        panic!("{change}: conditional handoff queue closed")
                    }),
                    result = &mut upgrading => panic!(
                        "{change}: upgrade ended before its conditional event: {result:?}; {:?}",
                        h.autopilot.sup.journal()
                    ),
                };
                assert!(
                    !h.shared.exec_pending(),
                    "enqueueing does not authorize teardown"
                );
                assert!(!h.rt.timer_pending(), "the handoff owns the suspended act");
                let carried = h.shared.owed_act();
                assert_eq!(carried.is_some(), change != "no-carry");
                assert_eq!(
                    writes.load(Ordering::SeqCst),
                    usize::from(change != "no-carry")
                );
                assert_eq!(*lock(&saved), carried, "the queued carry was written first");
                match change {
                    "draft" => h.shared.human_input(now_ms()),
                    "turn" | "turn-hup" => {
                        h.shared.turn_started(h.rt.key(), "superseding-turn");
                    }
                    "turn-ended" | "turn-ended-hup" => {
                        use codex_protocol::config_types::CollaborationMode;
                        use codex_protocol::config_types::ModeKind;
                        use codex_protocol::config_types::Settings;
                        let point = h.rt.state().point_at();
                        let session = codex_extension_api::ExtensionData::new("session");
                        let turn = codex_extension_api::ExtensionData::new("superseding-turn");
                        let mode = CollaborationMode {
                            mode: ModeKind::Default,
                            settings: Settings {
                                model: "gpt-5.5".into(),
                                reasoning_effort: None,
                                developer_instructions: None,
                            },
                        };
                        h.autopilot
                            .on_turn_start(TurnStartInput {
                                turn_id: "superseding-turn",
                                collaboration_mode: &mode,
                                token_usage_at_turn_start: None,
                                session_store: &session,
                                thread_store: h.test.codex.thread_extension_data(),
                                turn_store: &turn,
                            })
                            .await;
                        h.autopilot
                            .on_turn_stop(codex_extension_api::TurnStopInput {
                                session_store: &session,
                                thread_store: h.test.codex.thread_extension_data(),
                                turn_store: &turn,
                            })
                            .await;
                        // Model the equal timestamps a paused/coarse clock can assign. The
                        // accepted native turn still supersedes the old act unambiguously.
                        h.rt.with_state(|state| {
                            if let (Some(end), Some(at)) = (&mut state.point, point) {
                                end.at = at;
                            }
                        });
                    }
                    "thread" => h.shared.note_root_thread("new-loaded-thread"),
                    "policy" => {
                        h.fake.set_policy_key("upgrade", "false");
                        wait_for_supervisor(|| {
                            h.autopilot.sup.policy().is_some_and(|p| !p.upgrade)
                        })
                        .await;
                    }
                    _ => {}
                }
                if matches!(
                    change,
                    "hup" | "term-stop" | "int-stop" | "turn-ended-hup" | "turn-hup"
                ) {
                    assert!(h.shared.set_signal(if change == "int-stop" {
                        2
                    } else if change == "term-stop" {
                        15
                    } else {
                        1
                    }));
                }
                if matches!(change, "term-stop" | "int-stop" | "normal-stop") {
                    let session = codex_extension_api::ExtensionData::new("session");
                    h.autopilot
                        .on_thread_stop(codex_extension_api::ThreadStopInput {
                            session_store: &session,
                            thread_store: h.test.codex.thread_extension_data(),
                        })
                        .await;
                }
                if change == "cancel" {
                    upgrading.abort();
                    assert!(upgrading.await.is_err());
                    assert!(
                        !request.decide(true),
                        "the late event is no longer authorized"
                    );
                } else {
                    let accepted = request.decide(true);
                    assert_eq!(accepted, matches!(change, "none" | "no-carry"), "{change}");
                    assert_eq!(
                        upgrading.await.unwrap_or_else(|err| panic!("{err}")),
                        accepted
                    );
                }
                assert_eq!(
                    h.shared.exec_pending(),
                    matches!(change, "none" | "no-carry"),
                    "{change}"
                );
                if matches!(change, "hup" | "term-stop") {
                    assert_eq!(
                        h.shared.owed_act(),
                        carried,
                        "the exact act survives {change}"
                    );
                    assert!(
                        !h.rt.timer_pending(),
                        "an exiting process schedules no timer"
                    );
                    assert!(!h.autopilot.upgrade_requested.load(Ordering::SeqCst));
                } else if matches!(
                    change,
                    "int-stop" | "normal-stop" | "turn-ended-hup" | "turn-hup"
                ) {
                    assert!(
                        h.shared.owed_act().is_none(),
                        "{change}: no obsolete act survives"
                    );
                    assert!(!h.rt.timer_pending());
                }
                if matches!(
                    change,
                    "draft" | "thread" | "policy" | "cancel" | "write-input"
                ) {
                    let restored =
                        h.rt.owed_timer()
                            .unwrap_or_else(|| panic!("{change}: act restored"));
                    assert_eq!(restored.text, "Preserve this exact pending act");
                    // Epoch serialization rounds milliseconds; it must not restart the wait.
                    assert!(
                        restored
                            .not_before
                            .saturating_duration_since(h.deadline)
                            .max(h.deadline.saturating_duration_since(restored.not_before))
                            < Duration::from_secs(1)
                    );
                    assert!(!h.autopilot.upgrade_requested.load(Ordering::SeqCst));
                } else {
                    assert!(
                        !h.rt.timer_pending(),
                        "committed or superseded acts cannot be restored"
                    );
                }
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .unwrap_or_else(|err| panic!("{err}"));
            })
            .await
            .unwrap_or_else(|err| panic!("handoff hang detector ({change}): {err}"));
        }
    });
}

#[test]
fn cancellation_before_the_first_upgrade_scan_releases_only_its_own_attempt() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let mut h = Harness::new().await;
            let (entered, entry) = tokio::sync::oneshot::channel();
            let (_release, blocked) = tokio::sync::oneshot::channel();
            *lock(&h.autopilot.upgrade_begin) = Some((entered, blocked));
            let pilot = Arc::clone(&h.autopilot);
            let first = tokio::spawn(pilot.consider_upgrade());
            entry.await.unwrap_or_else(|err| panic!("{err}"));
            assert!(h.autopilot.upgrading.load(Ordering::SeqCst));
            // An overlapping observer never owns the slot and cannot release it.
            Arc::clone(&h.autopilot).consider_upgrade().await;
            assert!(h.autopilot.upgrading.load(Ordering::SeqCst));
            first.abort();
            assert!(first.await.is_err());
            assert!(!h.autopilot.upgrading.load(Ordering::SeqCst));
            assert!(
                h.rt.timer_pending(),
                "the early cancellation never took the paced act"
            );
            let pilot = Arc::clone(&h.autopilot);
            let next = tokio::spawn(pilot.consider_upgrade());
            let request = h
                .requests
                .recv()
                .await
                .unwrap_or_else(|| panic!("a later consideration can still queue its handoff"));
            assert!(request.decide(true));
            next.await.unwrap_or_else(|err| panic!("{err}"));
            assert!(h.shared.exec_pending());
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .unwrap_or_else(|err| panic!("{err}"));
        })
        .await
        .unwrap_or_else(|err| panic!("handoff hang detector: {err}"));
    });
}

#[test]
fn explicit_off_persists_the_end_of_recovery_before_a_remote_release_answers() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.shared
                .set_recovery(Some(codex_alab_core::session::Recovery {
                    thread: h.rt.key.clone(),
                    turn_id: "failed-turn".into(),
                    point_at_ms: now_ms(),
                    not_before_ms: now_ms().saturating_add(300_000),
                    by_autopilot: true,
                    by_person: false,
                    wall: codex_alab_core::session::RecoveryWall::Usage {
                        quota: None,
                        reset_at_ms: None,
                        reset_label: None,
                    },
                }));
            let flushed_clear = Arc::new(AtomicBool::new(false));
            let seen = Arc::clone(&flushed_clear);
            let shared = h.shared;
            assert!(shared.set_flush(Box::new(move || {
                seen.store(shared.recovery().is_none(), Ordering::SeqCst);
                true
            })));
            h.fake.set_effective_quietly(false, "autopilot-off");
            // A new claim may still be reconciling its attention on this connection.
            // Hold only the release whose acknowledgement this test must precede.
            h.fake.gate("meta unset supervisor");
            let client = h
                .autopilot
                .sup
                .client()
                .unwrap_or_else(|| panic!("held claim"));
            let sup = Arc::clone(&h.autopilot.sup);
            let off = tokio::spawn(async move {
                sup.release_for_off(
                    &client,
                    "effective=off why=autopilot-off".into(),
                    codex_alab_core::session::HarnessWord::Off,
                )
                .await;
            });
            assert!(
                h.fake
                    .wait_for(|f| {
                        f.gated_requests()
                            .iter()
                            .any(|request| request.contains("meta unset supervisor"))
                    })
                    .await
            );
            assert!(h.shared.recovery().is_none());
            assert!(flushed_clear.load(Ordering::SeqCst));
            h.fake.open_gate("meta unset supervisor");
            off.await.unwrap_or_else(|err| panic!("{err}"));
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .unwrap_or_else(|err| panic!("{err}"));
        })
        .await
        .unwrap_or_else(|err| panic!("off boundary hang detector: {err}"));
    });
}

/// THE PARK AT AN OFF WRITES ITS RECORD WITH THE THREAD LIST LET GO (finding hangs-3): the
/// park ran `Shared::flush_record` (blocking file I/O: a sweep, then `sync_all`) on the runtime
/// with `threads` held, from `evaluate`, the turn's start and its end alike, so a slow disk held
/// every callback that takes the list, and the turn's start that core awaits. Now the park is
/// made under the list and its record goes once the list is let go, off the runtime within
/// `WRITE_THROUGH_BUDGET`: at a decision (`park_if_off`) and at a turn's end (`finish`, which
/// hands the write to a task; review of the forever-cruise lane, finding 4: a mutant that
/// dropped that write passed every test). The flush here takes the list from another thread
/// before it answers: held by the flush's own caller, that never comes, which the hang detector
/// names. NEGATIVE CONTROL: the park itself still happens (its epoch seen, its record written).
#[test]
fn the_park_at_an_off_writes_its_record_with_the_thread_list_let_go() {
    native_test(|| async {
        for at_turn_end in [false, true] {
            tokio::time::timeout(Duration::from_secs(60), async {
                let h = Harness::new().await;
                let flushes = Arc::new(AtomicUsize::new(0));
                let under_threads = Arc::new(AtomicBool::new(false));
                let (count, held, autopilot) = (
                    Arc::clone(&flushes),
                    Arc::clone(&under_threads),
                    Arc::clone(&h.autopilot),
                );
                assert!(h.shared.set_flush(Box::new(move || {
                    let (took, taken) = std::sync::mpsc::channel();
                    let autopilot = Arc::clone(&autopilot);
                    std::thread::spawn(move || {
                        drop(lock(&autopilot.threads));
                        let _ = took.send(());
                    });
                    if taken.recv_timeout(Duration::from_secs(60)).is_err() {
                        held.store(true, Ordering::SeqCst);
                    }
                    count.fetch_add(1, Ordering::SeqCst);
                    true
                })));
                h.fake.set_effective_quietly(false, "autopilot-off");
                let client = h
                    .autopilot
                    .sup
                    .client()
                    .unwrap_or_else(|| panic!("held claim"));
                h.autopilot
                    .sup
                    .release_for_off(
                        &client,
                        "effective=off why=autopilot-off".into(),
                        codex_alab_core::session::HarnessWord::Off,
                    )
                    .await;
                let offs = h.autopilot.sup.offs();
                assert_ne!(h.rt.off_seen.load(Ordering::SeqCst), offs, "an Off stands");
                let before = flushes.load(Ordering::SeqCst);
                if at_turn_end {
                    // The turn in hand ends.
                    let turn = ExtensionData::new("turn-at-the-off");
                    *h.rt.native_turn.lock().await = Some(turn.level_id().to_string());
                    h.autopilot.finish(&h.rt, &turn, None).await;
                } else {
                    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
                }
                assert_eq!(
                    h.rt.off_seen.load(Ordering::SeqCst),
                    offs,
                    "at_turn_end={at_turn_end}: it parked"
                );
                // The turn's end hands the write to a task: it comes, or the hang detector
                // names it.
                wait_for_supervisor(|| flushes.load(Ordering::SeqCst) > before).await;
                assert!(
                    !under_threads.load(Ordering::SeqCst),
                    "at_turn_end={at_turn_end}: the park's record was written with `threads` held"
                );
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .unwrap_or_else(|err| panic!("{err}"));
            })
            .await
            .unwrap_or_else(|err| {
                panic!("at_turn_end={at_turn_end}: park flush hang detector: {err}")
            });
        }
    });
}
