// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The autopilot end to end: a real core thread (`core_test_support`'s `TestCodex`, its
//! model a wiremock Responses server) with the extension installed, the TUI's registration
//! played into a private shared state, and alab-aterm's in-process FAKE aterm as the
//! session's control socket. Never a real aterm.
//!
//! - claim, a turn ends, `continue_text` is submitted; its `ATERM-TASK-DONE` reply ends it;
//! - a person typing holds the act;
//! - an old aterm (no `harness policy`) is never claimed, and nothing is typed.

use std::sync::Arc;
use std::time::Duration;

use codex_alab_aterm::Options as ClientOptions;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_autopilot::Autopilot;
use codex_alab_autopilot::Supervisor;
use codex_alab_autopilot::SupervisorOptions;
use codex_alab_autopilot::decide::Timing;
use codex_alab_core::session::Frame;
use codex_alab_core::session::ProgramState;
use codex_alab_core::session::Shared;
use codex_alab_core::session::now_ms;
use codex_core::config::Config;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStopInput;
use codex_protocol::ThreadId;
use codex_protocol::protocol::EventMsg;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;

struct Harness {
    fake: FakeAterm,
    shared: &'static Shared,
    autopilot: Arc<Autopilot>,
    test: TestCodex,
    mock: responses::ResponseMock,
    _server: wiremock::MockServer,
    thread: String,
    _dir: tempfile::TempDir,
}

/// A failing test says what the supervisor did (its journal) and what the fake heard.
impl Drop for Harness {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "supervisor journal: {:#?}",
                self.autopilot.supervisor().journal()
            );
            eprintln!("fake requests: {:#?}", self.fake.requests());
        }
    }
}

fn reply(id: &str, text: &str) -> String {
    responses::sse(vec![
        responses::ev_response_created(id),
        responses::ev_assistant_message(&format!("{id}-msg"), text),
        responses::ev_completed(id),
    ])
}

async fn harness(mode: Mode, replies: Vec<String>, policy: &[(&str, &str)]) -> Harness {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let fake = FakeAterm::start(dir.path(), mode)
        .await
        .unwrap_or_else(|err| panic!("fake aterm: {err}"));
    for (key, value) in policy {
        fake.set_policy_key(key, value);
    }
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let endpoint = fake.endpoint();
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: ClientOptions::default(),
    });
    let mut builder = ExtensionRegistryBuilder::<Config>::new();
    // The mock's replies take milliseconds: with aterm's 2 min of work a person's turn
    // would be short and back the act off (decide_tests pins that); here every turn is work.
    let autopilot = Autopilot::new(
        sup,
        builder.event_sink(),
        None,
        None,
        Timing {
            min_work: Duration::ZERO,
            ..Timing::default()
        },
        |_| true,
    );
    autopilot.register(&mut builder);
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(&server, replies).await;
    let test = test_codex()
        .with_extensions(Arc::new(builder.build()))
        .build(&server)
        .await
        .unwrap_or_else(|err| panic!("test codex: {err}"));
    autopilot.bind_thread_manager(Arc::downgrade(&test.thread_manager));
    let thread = test.session_configured.thread_id.to_string();
    // The TUI registers its primary thread (the first frame that shows it).
    shared.observe(
        &Frame {
            primary_thread: Some(&thread),
            displayed_thread: Some(&thread),
            rollout_path: None,
            cwd: test.cwd_path(),
            model: "gpt-5",
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
    Harness {
        fake,
        shared,
        autopilot,
        test,
        mock,
        _server: server,
        thread,
        _dir: dir,
    }
}

/// Run a test on its own runtime with 16 MiB stacks: a codex-core turn overflows libtest's
/// 2 MiB default (upstream runs its tests with `RUST_MIN_STACK=8388608`; check.sh runs the
/// ALab crates without it).
fn big_stack<F, Fut>(test: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()>,
{
    let outcome = std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_stack_size(16 << 20)
                .enable_all()
                .build()
                .unwrap_or_else(|err| panic!("runtime: {err}"))
                .block_on(test());
        })
        .unwrap_or_else(|err| panic!("test thread: {err}"))
        .join();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

/// Wait until `done` holds, at most a minute: a hang detector, never a latency budget.
async fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "never happened: {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn journal_has(h: &Harness, needle: &str) -> bool {
    h.autopilot
        .supervisor()
        .journal()
        .iter()
        .any(|line| line.contains(needle))
}

#[test]
fn a_claimed_turn_end_is_continued_and_its_done_token_ends_it() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in; stage 2 is next."),
                reply("r2", "Stage 2 is in and pushed.\nATERM-TASK-DONE"),
            ],
            &[("human_grace_s", "0")],
        )
        .await;
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await,
            "the claim is taken once the primary thread is registered"
        );
        h.test.submit_turn("do the task").await.expect("first turn");
        // codex-alab's continuation runs as the next turn.
        wait_for_event(&h.test.codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
        assert!(
            h.fake
                .wait_for(|f| f.acts().iter().any(|a| a == "finished@v1"))
                .await,
            "the done token is recorded: {:?}",
            h.autopilot.supervisor().journal()
        );
        let requests = h.mock.requests();
        assert_eq!(requests.len(), 2, "one continuation, then nothing");
        let typed = requests[1].message_input_texts("user");
        assert_eq!(typed.last().map(String::as_str), Some("keep going"));
        assert!(h.fake.acts().iter().any(|a| a == "continue@v1"));
        let rt = h
            .autopilot
            .thread_rt(&h.thread)
            .expect("the thread is the autopilot's");
        assert!(rt.state().done);
        assert!(!rt.timer_pending());
        assert!(
            h.fake
                .notices()
                .iter()
                .any(|n| n.contains("codex-alab @") && n.contains("keep going")),
            "{:?}",
            h.fake.notices()
        );
        assert_eq!(
            h.shared.claim().as_deref(),
            Some(codex_alab_aterm::holder_for(std::process::id()).as_str())
        );
    });
}

#[test]
fn a_person_typing_holds_the_continuation() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![reply("r1", "Stage 1 is in; stage 2 is next.")],
            &[("human_grace_s", "3600")],
        )
        .await;
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        // A person's key reached the TUI just now.
        h.shared.human_input(now_ms());
        h.test.submit_turn("do the task").await.expect("first turn");
        wait_until("the act waits for the person", || {
            journal_has(&h, "not now (a person typed in the TUI)")
        })
        .await;
        let rt = h.autopilot.thread_rt(&h.thread).expect("rt");
        assert!(rt.timer_pending(), "it looks again at the grace's end");
        assert_eq!(h.mock.requests().len(), 1, "nothing was typed");
        assert!(!h.fake.acts().iter().any(|a| a == "continue@v1"));
    });
}

#[test]
fn a_person_at_the_terminal_holds_the_continuation() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![reply("r1", "Stage 1 is in; stage 2 is next.")],
            &[("human_grace_s", "120")],
        )
        .await;
        // aterm saw a person's input 5 s ago (a key, a click, IME composition).
        h.fake.set_status("human_ms", "5000");
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        h.test.submit_turn("do the task").await.expect("first turn");
        wait_until("the act waits for the person", || {
            journal_has(&h, "not now (a person at the terminal)")
        })
        .await;
        assert_eq!(h.mock.requests().len(), 1, "nothing was typed");
    });
}

#[test]
fn an_old_aterm_is_never_claimed_and_nothing_is_typed() {
    big_stack(|| async {
        let h = harness(
            Mode::Old,
            vec![reply("r1", "Stage 1 is in; stage 2 is next.")],
            &[("human_grace_s", "0")],
        )
        .await;
        wait_until("the supervisor read the old aterm", || {
            journal_has(&h, "never claims")
        })
        .await;
        h.test.submit_turn("do the task").await.expect("first turn");
        wait_until("the turn end was looked at", || {
            journal_has(&h, "passive (no proven claim)")
        })
        .await;
        assert!(h.fake.claim().is_none());
        assert!(
            !h.fake
                .requests()
                .iter()
                .any(|r| r.contains("meta set supervisor")),
            "{:?}",
            h.fake.requests()
        );
        assert_eq!(h.mock.requests().len(), 1);
        assert!(h.shared.claim().is_none());
    });
}

#[test]
fn a_done_token_on_the_first_reply_types_nothing() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![reply("r1", "All of it is in.\nATERM-TASK-DONE")],
            &[("human_grace_s", "0")],
        )
        .await;
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        h.test.submit_turn("do the task").await.expect("first turn");
        assert!(
            h.fake
                .wait_for(|f| f.acts().iter().any(|a| a == "finished@v1"))
                .await
        );
        assert_eq!(h.mock.requests().len(), 1);
        assert!(!h.fake.acts().iter().any(|a| a == "continue@v1"));
    });
}

#[test]
fn a_side_threads_turn_leaves_the_login_wall_standing() {
    // End-to-end run 2026-10-16: the main thread's refresh failure raised the login
    // attention, and a side thread's turn that went through cleared it 39 ms later.
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![reply("r1", "All of it is in.\nATERM-TASK-DONE")],
            &[("human_grace_s", "0")],
        )
        .await;
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        h.test.submit_turn("do the task").await.expect("first turn");
        assert!(
            h.fake
                .wait_for(|f| f.acts().iter().any(|a| a == "finished@v1"))
                .await
        );
        let sup = h.autopilot.supervisor();
        sup.attention(
            "login",
            Some("login: Codex's sign-in was refused".to_string()),
        )
        .await;
        assert!(h.fake.wait_for(|f| !f.attention().is_empty()).await);
        let stop = |store: ExtensionData| {
            let autopilot = Arc::clone(&h.autopilot);
            async move {
                let session = ExtensionData::new("session");
                let turn = ExtensionData::new("turn-side");
                autopilot
                    .on_turn_stop(TurnStopInput {
                        session_store: &session,
                        thread_store: &store,
                        turn_store: &turn,
                    })
                    .await;
            }
        };
        // A thread of the same source that the TUI never registered (a title, a memory).
        let side = ThreadId::new();
        let store = ExtensionData::new(side.to_string());
        store.insert(codex_alab_autopilot::extension::ThreadRt::new(
            side,
            None,
            false,
            Duration::ZERO,
        ));
        stop(store).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            sup.standing_attention().is_some(),
            "a side thread's success cleared the login wall: {:?}",
            sup.journal()
        );
        assert!(!h.fake.attention().is_empty());
        // Negative control: the registered main thread's turn that went through clears it.
        let main = ThreadId::from_string(&h.thread).expect("thread id");
        h.shared.note_root_thread(&h.thread);
        let store = ExtensionData::new(h.thread.clone());
        store.insert(codex_alab_autopilot::extension::ThreadRt::new(
            main,
            None,
            false,
            Duration::ZERO,
        ));
        stop(store).await;
        wait_until("the main thread's success clears the wall", || {
            sup.standing_attention().is_none()
        })
        .await;
    });
}

/// The attention audit: in Plan mode codex-alab types nothing, and the claim keeps aterm's own
/// rows quiet, so the turn end it leaves raises keyed attention; a person's next turn clears
/// it.
#[test]
fn a_plan_mode_turn_end_left_to_a_person_raises_attention() {
    use codex_core::TurnInputRequest;
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;
    use codex_protocol::protocol::ThreadSettingsOverrides;
    use codex_protocol::user_input::UserInput;
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "Here is the plan: read, then write."),
                reply("r2", "Done as asked.\nATERM-TASK-DONE"),
            ],
            &[("human_grace_s", "0")],
        )
        .await;
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        h.test
            .codex
            .start_or_steer_turn(
                TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "plan the task".into(),
                    text_elements: Vec::new(),
                }])
                .with_thread_settings(ThreadSettingsOverrides {
                    collaboration_mode: Some(CollaborationMode {
                        mode: ModeKind::Plan,
                        settings: Settings {
                            model: h.test.session_configured.model.clone(),
                            reasoning_effort: None,
                            developer_instructions: None,
                        },
                    }),
                    ..Default::default()
                }),
            )
            .await
            .expect("plan turn");
        wait_for_event(&h.test.codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
        assert!(
            h.fake
                .wait_for(|f| f
                    .attention()
                    .get("codex-alab")
                    .is_some_and(|t| t.starts_with("plan: Plan mode leaves")))
                .await,
            "{:?} {:?}",
            h.fake.attention(),
            h.autopilot.supervisor().journal()
        );
        assert_eq!(
            h.mock.requests().len(),
            1,
            "nothing was typed into Plan mode"
        );
        // Review of 4cdc96090, finding 3: a suspension clears it; acting again, the point
        // still stands (no turn since) and raises it again, and still types nothing.
        h.fake.set_effective(false, "suspended");
        assert!(
            h.fake.wait_for(|f| f.attention().is_empty()).await,
            "cleared at the off: {:?} {:?}",
            h.fake.attention(),
            h.autopilot.supervisor().journal()
        );
        h.fake.set_effective(true, "enabled");
        assert!(
            h.fake
                .wait_for(|f| f
                    .attention()
                    .get("codex-alab")
                    .is_some_and(|t| t.starts_with("plan: Plan mode leaves")))
                .await,
            "raised again: {:?} {:?}",
            h.fake.attention(),
            h.autopilot.supervisor().journal()
        );
        assert_eq!(h.mock.requests().len(), 1, "still nothing typed");
        // The person answers in the default mode: the attention goes.
        h.test.submit_turn("go ahead").await.expect("person's turn");
        assert!(
            h.fake.wait_for(|f| f.attention().is_empty()).await,
            "{:?} {:?}",
            h.fake.attention(),
            h.autopilot.supervisor().journal()
        );
    });
}

/// Review of 4cdc96090, finding 3, for a goal: a goal that stopped by itself (blocked) leaves
/// the turn end to a person with `goal:` attention; a suspension clears it, and acting again
/// raises it again, typing nothing. Negative control: a goal a person paused instead raises
/// nothing, before or after a passive spell.
#[test]
fn a_blocked_goals_attention_is_raised_again_when_acting_resumes() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in; stage 2 is next."),
                reply("r2", "Paused as asked."),
            ],
            &[("human_grace_s", "0")],
        )
        .await;
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        let db = h
            .test
            .codex
            .state_db()
            .unwrap_or_else(|| panic!("a state database"));
        let id = h.test.session_configured.thread_id;
        let goal = |status| {
            let db = db.clone();
            async move {
                db.thread_goals()
                    .replace_thread_goal(id, "ship the inbox", status, None)
                    .await
                    .unwrap_or_else(|err| panic!("goal: {err}"))
            }
        };
        goal(codex_state::ThreadGoalStatus::Blocked).await;
        h.test.submit_turn("do the task").await.expect("first turn");
        let blocked = |f: &FakeAterm| {
            f.attention()
                .get("codex-alab")
                .is_some_and(|t| t.starts_with("goal: the goal is blocked"))
        };
        assert!(
            h.fake.wait_for(blocked).await,
            "{:?} {:?}",
            h.fake.attention(),
            h.autopilot.supervisor().journal()
        );
        h.fake.set_effective(false, "suspended");
        assert!(
            h.fake.wait_for(|f| f.attention().is_empty()).await,
            "cleared at the off: {:?} {:?}",
            h.fake.attention(),
            h.autopilot.supervisor().journal()
        );
        h.fake.set_effective(true, "enabled");
        assert!(
            h.fake.wait_for(blocked).await,
            "raised again: {:?} {:?}",
            h.fake.attention(),
            h.autopilot.supervisor().journal()
        );
        assert_eq!(h.mock.requests().len(), 1, "nothing typed past the goal");
        // Negative control: the person pauses it and takes a turn; its end leaves nothing.
        goal(codex_state::ThreadGoalStatus::Paused).await;
        h.test.submit_turn("pause it").await.expect("person's turn");
        assert!(h.fake.wait_for(|f| f.attention().is_empty()).await);
        h.fake.set_effective(false, "suspended");
        wait_until("passive", || !h.autopilot.supervisor().acting()).await;
        h.fake.set_effective(true, "enabled");
        wait_until("acting", || h.autopilot.supervisor().acting()).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            h.fake.attention().is_empty(),
            "{:?} {:?}",
            h.fake.attention(),
            h.autopilot.supervisor().journal()
        );
        assert_eq!(h.mock.requests().len(), 2);
    });
}

/// The native inbox end to end: the turn the TUI submitted for a typed `Inbox: task @<off>`
/// (its text registered as a mail turn) runs on a real core thread; at its end the row is
/// marked handled and the last agent message goes back to the sender, stamped.
#[test]
fn a_mail_turns_end_is_receipted_and_reported_to_the_sender() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![reply("r1", "Tests pass.\nATERM-TASK-DONE")],
            &[("human_grace_s", "0")],
        )
        .await;
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        let (id, off) = h
            .fake
            .deliver("s-lead@n-lab", "task", "agent", "Run the tests.");
        let text = format!(
            "# Mail from aterm (task @{off})\n…\n## My request for Codex:\nInbox: task @{off}"
        );
        h.shared.expect_mail_turn(codex_alab_core::mail::MailTurn {
            id,
            off,
            from: "s-lead@n-lab".to_string(),
            kind: "task".to_string(),
            text: text.clone(),
        });
        h.test.submit_turn(&text).await.expect("mail turn");
        assert!(
            h.fake.wait_for(|f| !f.posts().is_empty()).await,
            "the report goes: {:?}",
            h.autopilot.supervisor().journal()
        );
        assert_eq!(h.fake.receipts(), vec![(id, "handled".to_string())]);
        let post = &h.fake.posts()[0];
        // A session sender is answered as `@s-…`: the only form aterm's bridge routes.
        assert!(post.options.contains(&"to=@s-lead@n-lab".to_string()));
        assert!(post.routed, "{:?}", post.options);
        assert!(post.options.contains(&format!("re={off}")));
        assert!(
            post.body.ends_with("Tests pass.\nATERM-TASK-DONE"),
            "{}",
            post.body
        );
        // A turn whose text was never registered answers no mail (negative control).
        assert!(!h.shared.is_mail_turn(&text), "the expectation was spent");
    });
}
