// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

// Core runtime types form a recursive graph during auto-trait evaluation.
#![recursion_limit = "256"]

//! The autopilot end to end: a real core thread (`core_test_support`'s `TestCodex`, its
//! model a wiremock Responses server) with the extension installed, the TUI's registration
//! played into a private shared state, and alab-aterm's in-process FAKE aterm as the
//! session's control socket. Never a real aterm.
//!
//! - claim, a turn ends, `continue_text` is submitted; its done claim gets Cruise's
//!   defend-and-find-more prompt, never an end (FOREVER CRUISE, aterm owner direction
//!   2026-10-19);
//! - under Land, a clean defense at a land act's yield over a clean repository lands, and
//!   nothing is typed after it; a switch to Cruise goes on;
//! - a worker's NEEDS YOU goes on the person's screen (`owner=needs kind=ask`) and never
//!   blocks;
//! - a park on the owner's go-ahead for the agent's own reversible work gets `go-ahead@v1`, its
//!   line never on the menu bar, and goes up where the go-ahead is not typed (`go_ahead`);
//! - a person typing holds the act;
//! - aterm's measured fact (a background call that made no progress) rides an act codex-alab
//!   types anyway, whole or not at all, and is told once its turn started (`fact_rider`);
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
use codex_login::CodexAuth;
use codex_protocol::ThreadId;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::turn_input::CyberAccessProgram;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::turn_input::TurnStartOptions;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use core_test_support::wait_for_event_with_timeout;

#[path = "autopilot/api_back.rs"]
mod api_back;

#[path = "autopilot/late_goal.rs"]
mod late_goal;

#[path = "autopilot/goal_recovery.rs"]
mod goal_recovery;

#[path = "autopilot/terminal_recovery.rs"]
mod terminal_recovery;

#[path = "autopilot/wall_acts.rs"]
mod wall_acts;

#[path = "autopilot/policy_continuation.rs"]
mod policy_continuation;

#[path = "../src/login_fixtures.rs"]
mod login_fixtures;

#[path = "autopilot/login_back.rs"]
mod login_back;

#[path = "autopilot/go_ahead.rs"]
mod go_ahead;

#[path = "autopilot/renewal_admission.rs"]
mod renewal_admission;

#[path = "autopilot/fact_rider.rs"]
mod fact_rider;

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
    harness_with(mode, replies, policy, &[]).await
}

/// [`harness`], with these raw `status` fields set before the claim (`autopilot=land`).
async fn harness_with(
    mode: Mode,
    replies: Vec<String>,
    policy: &[(&str, &str)],
    status: &[(&str, &str)],
) -> Harness {
    harness_full(
        mode,
        replies.into_iter().map(responses::sse_response).collect(),
        policy,
        status,
        None,
    )
    .await
}

async fn harness_with_request_retries(
    mode: Mode,
    replies: Vec<wiremock::ResponseTemplate>,
    policy: &[(&str, &str)],
    retries: Option<u64>,
) -> Harness {
    harness_full(mode, replies, policy, &[], retries).await
}

async fn harness_full(
    mode: Mode,
    replies: Vec<wiremock::ResponseTemplate>,
    policy: &[(&str, &str)],
    status: &[(&str, &str)],
    retries: Option<u64>,
) -> Harness {
    harness_with_auth(mode, replies, policy, status, retries, None).await
}

async fn harness_with_auth(
    mode: Mode,
    replies: Vec<wiremock::ResponseTemplate>,
    policy: &[(&str, &str)],
    status: &[(&str, &str)],
    retries: Option<u64>,
    auth: Option<CodexAuth>,
) -> Harness {
    // The mock's replies take milliseconds: with aterm's 2 min of work a person's turn
    // would be short and back the act off (decide_tests pins that); here every turn is work.
    let timing = Timing {
        min_work: Duration::ZERO,
        ..Timing::default()
    };
    harness_timed(mode, replies, policy, status, retries, auth, timing).await
}

/// [`harness_with_auth`] on the autopilot timings `timing`.
async fn harness_timed(
    mode: Mode,
    replies: Vec<wiremock::ResponseTemplate>,
    policy: &[(&str, &str)],
    status: &[(&str, &str)],
    retries: Option<u64>,
    auth: Option<CodexAuth>,
    timing: Timing,
) -> Harness {
    harness_configured(mode, replies, policy, status, retries, auth, timing, |_| {}).await
}

/// [`harness_timed`], with `configure` applied to core's config after the harness's own.
#[expect(
    clippy::too_many_arguments,
    reason = "the harness's one full constructor"
)]
async fn harness_configured(
    mode: Mode,
    replies: Vec<wiremock::ResponseTemplate>,
    policy: &[(&str, &str)],
    status: &[(&str, &str)],
    retries: Option<u64>,
    auth: Option<CodexAuth>,
    timing: Timing,
    configure: impl FnOnce(&mut Config) + Send + 'static,
) -> Harness {
    harness_goals(
        mode, replies, policy, status, retries, auth, timing, configure, None,
    )
    .await
}

/// [`harness_configured`], with the autopilot's `GoalService` (the TUI's install passes one).
#[expect(
    clippy::too_many_arguments,
    reason = "the harness's one full constructor"
)]
async fn harness_goals(
    mode: Mode,
    replies: Vec<wiremock::ResponseTemplate>,
    policy: &[(&str, &str)],
    status: &[(&str, &str)],
    retries: Option<u64>,
    auth: Option<CodexAuth>,
    timing: Timing,
    configure: impl FnOnce(&mut Config) + Send + 'static,
    goals: Option<Arc<codex_goal_extension::GoalService>>,
) -> Harness {
    harness_login(
        mode,
        replies,
        policy,
        status,
        retries,
        auth.map(TestLogin::Cached),
        timing,
        configure,
        goals,
    )
    .await
}

/// The login the harness's codex runs on, where it is not the test default.
enum TestLogin {
    /// This credential, cached (no store behind it).
    Cached(CodexAuth),
    /// This manager over the scratch `home`, which becomes the codex home (the lost login's
    /// tests: a login file codex's own login code writes and the manager reloads).
    Manager {
        manager: Arc<codex_login::AuthManager>,
        home: Arc<tempfile::TempDir>,
    },
}

#[derive(Default)]
struct ErrorHooks {
    before: Option<Arc<dyn codex_extension_api::TurnLifecycleContributor>>,
    after: Option<Arc<dyn codex_extension_api::TurnLifecycleContributor>>,
    after_goal: Option<Arc<dyn codex_extension_api::TurnLifecycleContributor>>,
}

/// [`harness_goals`], on the login `login`.
#[expect(
    clippy::too_many_arguments,
    reason = "the harness's one full constructor"
)]
async fn harness_login(
    mode: Mode,
    replies: Vec<wiremock::ResponseTemplate>,
    policy: &[(&str, &str)],
    status: &[(&str, &str)],
    retries: Option<u64>,
    login: Option<TestLogin>,
    timing: Timing,
    configure: impl FnOnce(&mut Config) + Send + 'static,
    goals: Option<Arc<codex_goal_extension::GoalService>>,
) -> Harness {
    harness_login_with_error_hook(
        mode,
        replies,
        policy,
        status,
        retries,
        login,
        timing,
        configure,
        goals,
        ErrorHooks::default(),
    )
    .await
}

#[expect(
    clippy::too_many_arguments,
    reason = "full harness plus an ordered native-error fixture contributor"
)]
async fn harness_login_with_error_hook(
    mode: Mode,
    replies: Vec<wiremock::ResponseTemplate>,
    policy: &[(&str, &str)],
    status: &[(&str, &str)],
    retries: Option<u64>,
    login: Option<TestLogin>,
    timing: Timing,
    configure: impl FnOnce(&mut Config) + Send + 'static,
    goals: Option<Arc<codex_goal_extension::GoalService>>,
    error_hooks: ErrorHooks,
) -> Harness {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let fake = FakeAterm::start(dir.path(), mode)
        .await
        .unwrap_or_else(|err| panic!("fake aterm: {err}"));
    for (key, value) in policy {
        fake.set_policy_key(key, value);
    }
    for (key, raw) in status {
        fake.set_status(key, raw);
    }
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let endpoint = fake.endpoint();
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: ClientOptions::default(),
    });
    let mut builder = ExtensionRegistryBuilder::<Config>::new();
    if let Some(hook) = error_hooks.before {
        builder.turn_lifecycle_contributor(hook);
    }
    let autopilot = Autopilot::new(sup, None, goals.clone(), timing, |_| true);
    autopilot.register(&mut builder);
    if let Some(hook) = error_hooks.after {
        builder.turn_lifecycle_contributor(hook);
    }
    // With a goal service, the goal extension the binary installs after the autopilot.
    let late_goal = goals.as_ref().map(|_| {
        let late = Arc::new(late_goal::LateGoal::default());
        late.register(&mut builder);
        late
    });
    if let Some(hook) = error_hooks.after_goal {
        builder.turn_lifecycle_contributor(hook);
    }
    let server = responses::start_mock_server().await;
    let mock = responses::mount_response_sequence(&server, replies).await;
    let mut test_builder = test_codex()
        .with_extensions(Arc::new(builder.build()))
        .with_config(move |config| {
            if let Some(retries) = retries {
                config.model_provider.request_max_retries = Some(retries);
                config.model_provider.stream_max_retries = Some(retries);
            }
        })
        .with_config(configure);
    match login {
        Some(TestLogin::Cached(auth)) => test_builder = test_builder.with_auth(auth),
        Some(TestLogin::Manager { manager, home }) => {
            test_builder = test_builder.with_auth_manager(manager).with_home(home);
        }
        None => {}
    }
    let test = test_builder
        .build(&server)
        .await
        .unwrap_or_else(|err| panic!("test codex: {err}"));
    autopilot.bind_thread_manager(Arc::downgrade(&test.thread_manager));
    if let (Some(late), Some(goals)) = (&late_goal, goals) {
        late.bind(&test, goals).await;
    }
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

/// aterm's task face follows the self-claim's recorded acts. Leaving the exchange that opens a
/// Land session types nothing, but still tells that face once. A person's message once the
/// session has worked is no exchange of theirs (gap 32: the owner, 2026-10-27, "the user
/// question exception is only for the first message of a session"): Land carries the work on,
/// its instruction typed and its yield's clean defense landed.
#[test]
fn an_opening_exchange_records_its_point_once_and_a_later_message_is_carried() {
    big_stack(|| async {
        use codex_alab_autopilot::words::RULE_LEFT;
        let h = harness_with(
            Mode::Current,
            vec![
                reply("first", "Here is the answer."),
                reply("second", "Here is another answer."),
                reply(
                    "third",
                    "Nothing left: every change is merged and pushed to origin/main.",
                ),
            ],
            &[("human_grace_s", "0")],
            &[("human_ms", "0"), ("autopilot", "land")],
        )
        .await;
        wait_until("proven claim", || h.autopilot.supervisor().acting()).await;
        h.test
            .submit_turn("a person's own question")
            .await
            .expect("first turn");
        wait_until("the exchange is left", || {
            journal_has(&h, "a person's own exchange: theirs to answer")
        })
        .await;
        assert_eq!(
            h.fake
                .acts()
                .iter()
                .filter(|rule| *rule == RULE_LEFT)
                .count(),
            1
        );
        assert_eq!(
            h.mock.requests().len(),
            1,
            "leaving never types a model turn"
        );

        let decisions = || {
            h.autopilot
                .supervisor()
                .journal()
                .iter()
                .filter(|line| line.contains(": nothing ("))
                .count()
        };
        let before = decisions();
        h.fake.set_effective(false, "suspended");
        wait_until("standby", || !h.autopilot.supervisor().acting()).await;
        h.fake.set_effective(true, "enabled");
        wait_until("the same point was evaluated again", || {
            h.autopilot.supervisor().acting() && decisions() > before
        })
        .await;
        assert_eq!(
            h.fake
                .acts()
                .iter()
                .filter(|rule| *rule == RULE_LEFT)
                .count(),
            1
        );
        assert_eq!(h.mock.requests().len(), 1);

        h.test
            .submit_turn("a new person's question")
            .await
            .expect("new native point");
        wait_until("the later message is carried to its landing", || {
            h.fake.acts().iter().any(|rule| rule == "landed@v1")
        })
        .await;
        assert_eq!(
            h.fake.acts(),
            vec![
                RULE_LEFT.to_string(),
                "land@v1".to_string(),
                "landed@v1".to_string()
            ]
        );
        let requests = h.mock.requests();
        assert_eq!(
            requests.len(),
            3,
            "the two questions, then Land's instruction"
        );
        let typed = requests[2].message_input_texts("user");
        assert_eq!(
            typed.last().map(String::as_str),
            Some(codex_alab_autopilot::words::LAND)
        );

        // The same native submission without a terminal hand is a control write.
        // It continues the job, rather than being reported as the person's exchange.
        let control = harness(
            Mode::Current,
            vec![
                reply("control", "The first part is ready."),
                reply("continued", "Nothing left: the control task is complete."),
            ],
            &[("human_grace_s", "0")],
        )
        .await;
        wait_until("control claim", || control.autopilot.supervisor().acting()).await;
        control
            .test
            .submit_turn("the same question as a control write")
            .await
            .expect("control turn");
        wait_until("the control write continues", || {
            control.fake.acts().iter().any(|rule| rule == "continue@v1")
                && control.mock.requests().len() == 2
        })
        .await;
        assert_eq!(control.mock.requests().len(), 2);
        assert!(!control.fake.acts().iter().any(|rule| rule == RULE_LEFT));
    });
}

#[test]
fn a_refused_exchange_record_is_published_after_a_new_claim_without_typing() {
    big_stack(|| async {
        use codex_alab_autopilot::words::RULE_LEFT;
        // The exchange that opens a Land session: Cruise leaves none to the person (gap 32).
        let h = harness_with(
            Mode::Current,
            vec![reply("answer", "Here is the answer.")],
            &[("human_grace_s", "0")],
            &[("human_ms", "0"), ("autopilot", "land")],
        )
        .await;
        wait_until("proven claim", || h.autopilot.supervisor().acting()).await;
        h.fake.refuse_next_harness_act("ERR denied");
        h.test
            .submit_turn("a person's own question")
            .await
            .expect("person's turn");
        let decisions = || {
            h.autopilot
                .supervisor()
                .journal()
                .iter()
                .filter(|line| line.contains(": nothing ("))
                .count()
        };
        wait_until("the first exchange point was evaluated", || {
            journal_has(&h, "a person's own exchange: theirs to answer")
        })
        .await;
        assert!(h.fake.acts().is_empty(), "the failed write recorded no act");
        assert_eq!(h.mock.requests().len(), 1);

        let before = decisions();
        h.fake.set_effective(false, "autopilot-off");
        wait_until("the old claim was released", || {
            !h.autopilot.supervisor().held()
        })
        .await;
        h.fake.set_effective(true, "enabled");
        wait_until("the same point was read under a new claim", || {
            h.autopilot.supervisor().acting() && decisions() > before
        })
        .await;
        assert!(
            h.fake
                .claim()
                .is_some_and(|claim| claim.proven_pid.is_some())
        );
        assert_eq!(h.fake.acts(), vec![RULE_LEFT.to_string()]);
        assert_eq!(
            h.mock.requests().len(),
            1,
            "publication never types a model turn"
        );
    });
}

/// AN OFF LEAVES A PERSON'S MESSAGE TO THEM WHILE IT STANDS, AND LAND READ AFTER IT CARRIES IT
/// once the session has worked (gap 32: the turn ends under no word,
/// `ThreadState::ended_under`, as aterm's `Autopilot::Off`; edges-4, the owner 2026-10-27: "the
/// user question exception is only for the first message of a session", so Land takes back an
/// exchange the Off left past the opening, as aterm's loop that the Off's lift starts acts at
/// that idle point). NEGATIVE CONTROL: the session's opening exchange before the Off stays
/// theirs (its `left-to-person@v1`, recorded once).
#[test]
fn an_off_leaves_a_persons_message_to_them_and_land_after_it_carries_it() {
    big_stack(|| async {
        use codex_alab_autopilot::words::RULE_LEFT;
        let h = harness_with(
            Mode::Current,
            vec![
                reply("first", "Here is the answer."),
                reply("second", "Here is where it stands."),
                reply("third", "Committing and pushing it now."),
            ],
            &[("human_grace_s", "0")],
            &[("human_ms", "0"), ("autopilot", "land")],
        )
        .await;
        wait_until("proven claim", || h.autopilot.supervisor().acting()).await;
        h.test
            .submit_turn("a person's own question")
            .await
            .expect("first turn");
        wait_until("the opening exchange is left", || {
            h.fake.acts() == vec![RULE_LEFT.to_string()]
        })
        .await;
        h.fake.set_effective(false, "autopilot-off");
        wait_until("the claim was released", || {
            !h.autopilot.supervisor().held()
        })
        .await;
        h.test
            .submit_turn("a person's question under the Off")
            .await
            .expect("second turn");
        let rt = h.autopilot.thread_rt(&h.thread).expect("rt");
        wait_until("the turn under the Off ended", || {
            rt.state().point.and_then(|point| point.text).as_deref()
                == Some("Here is where it stands.")
        })
        .await;
        assert!(rt.state().left, "left under the Off");
        h.fake.set_effective(true, "enabled");
        wait_until("the point is decided under the new claim", || {
            let acts = h.fake.acts();
            acts.iter().filter(|rule| *rule == RULE_LEFT).count() == 2
                || acts.iter().any(|rule| rule == "land@v1")
        })
        .await;
        let acts = h.fake.acts();
        assert_eq!(
            acts[..2],
            [RULE_LEFT.to_string(), "land@v1".to_string()],
            "{acts:?}"
        );
        wait_until("Land's instruction was typed", || {
            h.mock.requests().len() == 3
        })
        .await;
    });
}

/// CRUISE NEVER PARKS end to end (aterm 126e259836, the owner 2026-10-26; gap 32): the
/// session's opening message, opened by the person's hand and answered at once, is continued
/// as any turn end is, and nothing is recorded as left to them. NEGATIVE CONTROL: the same
/// exchange under Land is left to them (`an_opening_exchange_records_its_point_once_…`).
#[test]
fn cruise_continues_a_persons_own_exchange() {
    big_stack(|| async {
        use codex_alab_autopilot::words::RULE_LEFT;
        let h = harness_with(
            Mode::Current,
            vec![
                reply("first", "Here is the answer."),
                reply("continued", "Nothing left: done."),
            ],
            &[("human_grace_s", "0")],
            &[("human_ms", "0")],
        )
        .await;
        wait_until("proven claim", || h.autopilot.supervisor().acting()).await;
        h.test
            .submit_turn("a person's own question")
            .await
            .expect("first turn");
        wait_until("the exchange is continued", || {
            h.fake.acts().iter().any(|rule| rule == "continue@v1") && h.mock.requests().len() == 2
        })
        .await;
        assert!(!h.fake.acts().iter().any(|rule| rule == RULE_LEFT));
        assert!(!journal_has(
            &h,
            "a person's own exchange: theirs to answer"
        ));
    });
}

/// FOREVER CRUISE end to end: a claimed turn end is continued, and the done token that
/// once ended the task is a done claim, owed the defend-and-find-more prompt after the short
/// streak's back-off (pending, the paced act the live upgrade would carry); nothing records a
/// finish.
#[test]
fn a_claimed_turn_end_is_continued_and_its_done_claim_is_owed_the_defend_prompt() {
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
        let rt = h
            .autopilot
            .thread_rt(&h.thread)
            .expect("the thread is the autopilot's");
        // The wait's words go to aterm between its journal line and its timer.
        wait_until("the done claim's prompt waits out the back-off", || {
            journal_has(&h, "for continue-done-check@v1") && rt.timer_pending()
        })
        .await;
        let requests = h.mock.requests();
        assert_eq!(requests.len(), 2, "one continuation, then the back-off");
        let typed = requests[1].message_input_texts("user");
        assert_eq!(typed.last().map(String::as_str), Some("keep going"));
        assert!(h.fake.acts().iter().any(|a| a == "continue@v1"));
        assert!(!h.fake.acts().iter().any(|a| a == "finished@v1"));
        let owed = rt.owed_timer().expect("the wait is a paced act's");
        assert_eq!(owed.rule, "continue-done-check@v1");
        assert!(
            owed.text
                .starts_with(codex_alab_autopilot::words::DONE_CHECK),
            "the decision's own words wait: {}",
            owed.text
        );
        assert_eq!(
            owed.repo_owed,
            Some(Vec::new()),
            "decided before the repository was read: its suffix is owed, no open work named"
        );
        assert!(!rt.state().landed);
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

/// A PERSON'S ESC HOLDS THE POINT FOR `human_grace`, THEN CRUISE CARRIES ON (owner
/// 2026-10-27: "you need to keep working even if the user interrupted a long running job";
/// aterm's `person_hold`): an interrupt over a capacity wait ends in no idle, and the point it
/// stops is decided at once (held, its look again armed at the grace's end) and then as a turn
/// end the agent did not end, with no person's message. It pinned the opposite before FOREVER
/// CRUISE reached the Esc (the point parked until a person's turn). NEGATIVE CONTROL: within the
/// grace nothing is typed.
#[test]
fn interrupting_capacity_with_an_active_autopilot_holds_for_the_grace_then_carries_on() {
    big_stack(|| async {
        let h = harness_with_request_retries(
            Mode::Current,
            vec![
                // The long server deadline removes any race between observing the wait
                // and submitting the interrupt, even on a loaded host.
                wiremock::ResponseTemplate::new(503)
                    .insert_header("Retry-After", "3600")
                    .set_body_json(serde_json::json!({
                        "error": {
                            "code": "server_is_overloaded",
                            "message": "The selected model is at capacity."
                        }
                    })),
                responses::sse_response(reply("resumed", "The resumed task has one more step.")),
                responses::sse_response(reply(
                    "done",
                    "The original task is complete.\nATERM-TASK-DONE",
                )),
            ],
            &[("human_grace_s", "2")],
            // Observe ALab's capacity wait rather than the HTTP transport's retries.
            Some(0),
        )
        .await;
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        h.test
            .codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Complete the original task.".to_string(),
                text_elements: Vec::new(),
            }]))
            .await
            .expect("start the task");
        wait_for_event_with_timeout(
            &h.test.codex,
            |ev| matches!(ev, EventMsg::StreamError(_)),
            Duration::from_secs(60),
        )
        .await;
        let rt = h.autopilot.thread_rt(&h.thread).expect("root thread");
        let journal_before_interrupt = h.autopilot.supervisor().journal().len();
        let held_marker = format!(
            "{}: nothing ({})",
            h.thread,
            codex_alab_autopilot::decide::ESC_HOLD
        );
        h.test.codex.submit(Op::Interrupt).await.expect("interrupt");
        wait_for_event_with_timeout(
            &h.test.codex,
            |ev| {
                matches!(ev, EventMsg::TurnAborted(event) if event.reason == TurnAbortReason::Interrupted)
            },
            Duration::from_secs(60),
        )
        .await;
        // Explicit interrupts emit no idle callback: the abort itself has the point decided.
        wait_until("the Esc's point is held for the grace", || {
            rt.state().point.is_some_and(|end| end.aborted)
                && h.autopilot
                    .supervisor()
                    .journal()
                    .iter()
                    .skip(journal_before_interrupt)
                    .any(|line| line.contains(&held_marker))
        })
        .await;
        assert!(rt.state().task, "the original task stays available");
        assert!(rt.state().esc_at.is_some());
        assert_eq!(
            h.mock.requests().len(),
            1,
            "NEGATIVE CONTROL: nothing is typed within the grace"
        );
        assert!(!h.fake.acts().iter().any(|a| a == "continue@v1"));

        // Past the grace Cruise carries on by itself; its continuation's done claim is owed the
        // defend prompt after the short streak's back-off; nothing records a finish.
        wait_until("the done claim's prompt waits out the back-off", || {
            journal_has(&h, "for continue-done-check@v1") && rt.timer_pending()
        })
        .await;
        assert_eq!(
            rt.owed_timer().map(|owed| owed.rule),
            Some("continue-done-check@v1")
        );
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
        let requests = h.mock.requests();
        assert_eq!(
            requests.len(),
            3,
            "one stopped attempt, the carry-on past the grace, its continuation"
        );
        for request in &requests[1..] {
            assert_eq!(
                request
                    .message_input_texts("user")
                    .last()
                    .map(String::as_str),
                Some("keep going")
            );
        }
        assert!(h.fake.acts().iter().any(|a| a == "continue@v1"));
        assert!(!h.fake.acts().iter().any(|a| a == "finished@v1"));
        assert!(!rt.state().landed);
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

/// A HOLD'S LIFT TAKES EFFECT AT ONCE (parity review P1): a continuation due under a person's
/// hold waits on `status hold=`, and aterm's lift (`effective=` moved back on with no policy
/// event, and the `hold 0` event that tells it, as aterm folds a hold) has the thread look
/// again then, not at the wait's own recheck. That recheck is lengthened to an hour here, so
/// only the lift can end the wait within the hang detector (before the fix, the 60 s recheck
/// alone typed it, about 50 s after the lift). NEGATIVE CONTROL: nothing is typed while the
/// hold stands.
#[test]
fn a_hold_lift_has_the_continuation_typed_at_once() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in; stage 2 is next."),
                reply("r2", "Stage 2 is in; stage 3 is next."),
            ],
            &[("human_grace_s", "0")],
        )
        .await;
        h.autopilot
            .supervisor()
            .set_recheck(Duration::from_secs(3600));
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        // A person's hold, as aterm folds one: `status hold=1`, the policy off quietly, and the
        // hold's own event.
        h.fake.set_status("hold", "1");
        h.fake.set_effective_quietly(false, "hold");
        h.fake.emit("hold 1 reason=owner origin=cli");
        wait_until("the hold is read", || {
            journal_has(&h, "why=hold: the claim is kept")
        })
        .await;
        h.test.submit_turn("do the task").await.expect("first turn");
        wait_until("the continuation waits on the hold", || {
            journal_has(&h, "not now (aterm says not now: Hold)")
        })
        .await;
        // NEGATIVE CONTROL: the hold stands, and nothing is typed under it.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(h.mock.requests().len(), 1, "nothing typed under the hold");
        assert!(!h.fake.acts().iter().any(|a| a == "continue@v1"));
        // The lift.
        h.fake.set_status("hold", "0");
        h.fake.set_effective_quietly(true, "enabled");
        h.fake.emit("hold 0 reason=owner origin=cli");
        wait_until("the continuation is typed at the lift", || {
            h.fake.acts().iter().any(|a| a == "continue@v1")
        })
        .await;
        wait_until("its turn runs", || h.mock.requests().len() == 2).await;
        let typed = h.mock.requests()[1].message_input_texts("user");
        assert_eq!(typed.last().map(String::as_str), Some("keep going"));
    });
}

/// AN ACT DUE UNDER A HOLD STAYS OWED IN THE RECORD (parity review finding 4): the paced act a
/// check holds back (a person's hold here; a draft, a popup or an unreadable status alike) is
/// still the act the thread owes, so the record carries it while it waits, through every
/// recheck that fires under the hold, and an exit aterm relaunches (S3) or the live upgrade
/// hands it on. Before, the wait was scheduled as no paced act's, and the record said `idle`
/// with nothing owed: aterm's relaunch of a session killed under the hold typed nothing at its
/// lift. NEGATIVE CONTROLS: nothing is typed under the hold, and once the act goes at the lift
/// the record owes nothing.
#[test]
fn an_act_due_under_a_hold_stays_owed_in_the_record() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in; stage 2 is next."),
                reply("r2", "Stage 2 is in; stage 3 is next."),
            ],
            &[("human_grace_s", "0")],
        )
        .await;
        // The hold's own recheck fires several times while it stands.
        h.autopilot
            .supervisor()
            .set_recheck(Duration::from_millis(200));
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        h.fake.set_status("hold", "1");
        h.fake.set_effective_quietly(false, "hold");
        h.fake.emit("hold 1 reason=owner origin=cli");
        wait_until("the hold is read", || {
            journal_has(&h, "why=hold: the claim is kept")
        })
        .await;
        h.test.submit_turn("do the task").await.expect("first turn");
        let held = || {
            h.autopilot
                .supervisor()
                .journal()
                .iter()
                .filter(|line| line.contains("not now (aterm says not now: Hold)"))
                .count()
        };
        wait_until("the continuation waits on the hold", || held() >= 1).await;
        let owed = h
            .shared
            .owed_act()
            .unwrap_or_else(|| panic!("the record carries the held act"));
        assert_eq!(owed.thread, h.thread);
        assert_eq!(owed.rule.as_deref(), Some("continue@v1"));
        assert_eq!(owed.text.as_deref(), Some("keep going"));
        assert_eq!(owed.mode.as_deref(), Some("cruise"));
        // Through the rechecks that fire under the hold.
        wait_until("the hold's recheck fires again", || held() >= 3).await;
        assert_eq!(
            h.shared.owed_act().and_then(|owed| owed.rule),
            Some("continue@v1".to_string())
        );
        // NEGATIVE CONTROL: nothing is typed under the hold.
        assert_eq!(h.mock.requests().len(), 1, "nothing typed under the hold");
        assert!(!h.fake.acts().iter().any(|a| a == "continue@v1"));
        h.fake.set_status("hold", "0");
        h.fake.set_effective_quietly(true, "enabled");
        h.fake.emit("hold 0 reason=owner origin=cli");
        wait_until("the continuation is typed at the lift", || {
            h.fake.acts().iter().any(|a| a == "continue@v1")
        })
        .await;
        wait_until("its turn runs", || h.mock.requests().len() == 2).await;
        // NEGATIVE CONTROL: the act went; nothing is owed for it.
        wait_until("the record owes nothing once it went", || {
            h.shared
                .owed_act()
                .is_none_or(|owed| owed.text.as_deref() != Some("keep going"))
        })
        .await;
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
        // aterm saw a person's input 60 s ago (a key, a click, IME composition): inside the
        // 120 s grace, and before the turn's PERSON_TURN_SLACK, so the turn is no hand's own
        // exchange (an opening one under Land is left to the person outright:
        // under_land_only_the_sessions_opening_exchange_is_the_persons; Cruise leaves none).
        h.fake.set_status("human_ms", "60000");
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

/// A PERSON'S TURN A WALL ENDED IS NO ANSWER IN THE RESUMED HISTORY (parity review S4): a
/// real turn refused by the usage policy leaves its terminal error on its completion event in
/// the rollout core writes (`TurnCompleteEvent::error`), and the resume reader reads that, so
/// the need the report before it named stands after a relaunch or a re-exec, as the live
/// thread holds it at that wall. NEGATIVE CONTROL: the report's own turn completed with no
/// error.
#[test]
fn a_persons_walled_turn_answers_no_need_in_the_resumed_history() {
    big_stack(|| async {
        let h = harness(
            Mode::Old,
            vec![
                reply("r1", "Stage 1 is in.\nNEEDS YOU: the prod API key"),
                responses::sse_failed(
                    "refused",
                    "cyber_policy",
                    "This request was refused by the cybersecurity access policy.",
                ),
            ],
            &[("human_grace_s", "0")],
        )
        .await;
        h.test.submit_turn("do the task").await.expect("first turn");
        h.test
            .submit_turn("here is the key")
            .await
            .expect("the person's turn");
        h.test.codex.flush_rollout().await.expect("flushed");
        let path = h.test.codex.rollout_path().expect("a rollout");
        let (items, _, _) = codex_core::RolloutRecorder::load_rollout_items(&path)
            .await
            .expect("the rollout reads");
        let errors: Vec<bool> = items
            .iter()
            .filter_map(|item| match item {
                codex_history::RolloutItem::EventMsg(EventMsg::TurnComplete(done)) => {
                    Some(done.error.is_some())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            errors,
            [false, true],
            "the wall is on the second completion"
        );
        let needs = codex_alab_autopilot::extension::history_needs(&items);
        assert_eq!(needs.standing, ["the prod API key"]);
    });
}

/// A COMPACTION'S TURN IS NOBODY'S IN THE RESUMED HISTORY (parity review R1): core starts a
/// compaction (`Op::Compact`: a person's `/compact`, or codex-alab's own `context-compact@v1`)
/// as a turn whose `TurnStarted` carries no `turn_attribution` (only a regular task's does,
/// `Session::emit_turn_started`) and whose rollout holds no user message, by the provider's
/// remote compaction or core's local one. The live thread reads that turn as nobody's (no
/// `UserMessage` item), so the need the report before it named stands; the resume reader
/// must read the rollout core wrote the same way, not as a person's turn that answered it.
/// NEGATIVE CONTROL: the person's own turn after it answers the need, in the history as live.
#[test]
fn a_compactions_turn_answers_no_need_in_the_resumed_history() {
    for v2 in [true, false] {
        big_stack(move || async move {
            let summary = if v2 {
                responses::sse(vec![
                    serde_json::json!({
                        "type": "response.output_item.done",
                        "item": {"type": "compaction", "encrypted_content": "SUMMARY"},
                    }),
                    responses::ev_completed("summary"),
                ])
            } else {
                reply("summary", "The task so far: stage 1 is in.")
            };
            let timing = Timing {
                min_work: Duration::ZERO,
                ..Timing::default()
            };
            let h = harness_configured(
                Mode::Old,
                [
                    reply("r1", "Stage 1 is in.\nNEEDS YOU: the prod API key"),
                    summary,
                    reply("r3", "Stage 2 is in."),
                ]
                .into_iter()
                .map(responses::sse_response)
                .collect(),
                &[("human_grace_s", "0")],
                &[],
                None,
                None,
                timing,
                move |config| {
                    config.model_provider.capabilities = Some(
                        serde_json::from_value(serde_json::json!({
                            "remote_compaction": if v2 { "v2" } else { "unsupported" }
                        }))
                        .unwrap_or_else(|err| panic!("compaction capability: {err:?}")),
                    );
                },
            )
            .await;
            h.test.submit_turn("do the task").await.expect("first turn");
            h.test
                .codex
                .submit(Op::Compact)
                .await
                .expect("the compaction is queued");
            wait_for_event(&h.test.codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
            let rt = h.autopilot.thread_rt(&h.thread).expect("root thread");
            let history = || async {
                h.test.codex.flush_rollout().await.expect("flushed");
                let path = h.test.codex.rollout_path().expect("a rollout");
                codex_core::RolloutRecorder::load_rollout_items(&path)
                    .await
                    .expect("the rollout reads")
                    .0
            };
            let items = history().await;
            let turns: Vec<(bool, bool)> = items
                .iter()
                .filter_map(|item| match item {
                    codex_history::RolloutItem::EventMsg(EventMsg::TurnStarted(started)) => {
                        Some((started.turn_attribution.is_some(), false))
                    }
                    codex_history::RolloutItem::EventMsg(EventMsg::TurnComplete(done)) => {
                        Some((done.error.is_some(), true))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                turns,
                [(true, false), (false, true), (false, false), (false, true)],
                "v2={v2}: the person's turn names its attribution, the compaction's none, \
                 and neither ended at a wall"
            );
            let user_messages = items
                .iter()
                .filter(|item| {
                    matches!(
                        item,
                        codex_history::RolloutItem::EventMsg(EventMsg::UserMessage(_))
                    )
                })
                .count();
            assert_eq!(user_messages, 1, "v2={v2}: only the person's");
            assert_eq!(
                rt.state().needs.standing,
                ["the prod API key"],
                "v2={v2}: the live thread holds the need through the compaction"
            );
            let needs = codex_alab_autopilot::extension::history_needs(&items);
            assert_eq!(needs.standing, ["the prod API key"], "v2={v2}");
            assert!(
                codex_alab_autopilot::extension::scan_history(&items).0,
                "v2={v2}: the person's task"
            );
            // NEGATIVE CONTROL: the person's own turn answers it.
            h.test
                .submit_turn("here is the key")
                .await
                .expect("the person's turn");
            let items = history().await;
            assert_eq!(rt.state().needs.standing, Vec::<String>::new(), "v2={v2}");
            assert_eq!(
                codex_alab_autopilot::extension::history_needs(&items).standing,
                Vec::<String>::new(),
                "v2={v2}"
            );
        });
    }
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

/// A done token on the first reply is a done claim under Cruise: the repository probed, the
/// defend-and-find-more prompt typed at once, never a finish.
#[test]
fn a_done_token_on_the_first_reply_gets_the_defend_prompt() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "All of it is in.\nATERM-TASK-DONE"),
                reply("r2", "Nothing left: it is all merged and pushed."),
            ],
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
                .wait_for(|f| f.acts().iter().any(|a| a == "continue-done-check@v1"))
                .await,
            "{:?}",
            h.autopilot.supervisor().journal()
        );
        wait_until("the prompt's yield is answered with a wait", || {
            h.mock.requests().len() == 2 && journal_has(&h, "for continue-done-check@v1")
        })
        .await;
        let typed = h.mock.requests()[1].message_input_texts("user");
        assert!(
            typed
                .last()
                .is_some_and(|t| t.starts_with(codex_alab_autopilot::words::DONE_CHECK)),
            "{typed:?}"
        );
        assert!(journal_has(&h, "the repository probe"));
        assert!(!h.fake.acts().iter().any(|a| a == "finished@v1"));
    });
}

/// FOREVER LAND end to end (`status autopilot=land`): the land instruction at the first point,
/// and at its yield a clean defense over a repository that holds nothing (the test's folder
/// is no repository: the probe finds nothing) lands: `landed@v1` recorded, nothing typed after
/// it. NEGATIVE CONTROL: a switch to Cruise (`meta field=autopilot`) reads the landing's
/// defense as a done claim, owed the defend prompt.
#[test]
fn land_lands_a_clean_defense_and_types_nothing_after_it() {
    big_stack(|| async {
        let h = harness_with(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in."),
                reply(
                    "r2",
                    "Nothing left: every change is merged and pushed to origin/main.",
                ),
            ],
            &[("human_grace_s", "0")],
            &[("autopilot", "land")],
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
                .wait_for(|f| f.acts().iter().any(|a| a == "landed@v1"))
                .await,
            "{:?} {:?}",
            h.fake.acts(),
            h.autopilot.supervisor().journal()
        );
        let requests = h.mock.requests();
        assert_eq!(requests.len(), 2, "the instruction, then the landing");
        let typed = requests[1].message_input_texts("user");
        assert_eq!(
            typed.last().map(String::as_str),
            Some(codex_alab_autopilot::words::LAND)
        );
        assert!(h.fake.acts().iter().any(|a| a == "land@v1"));
        let rt = h.autopilot.thread_rt(&h.thread).expect("rt");
        // aterm records `landed@v1` before its answer reaches codex-alab, which commits the
        // landing locally after it: wait for that commit (a hang detector, not a budget).
        wait_until("the landing is committed", || rt.state().landed).await;
        assert!(!rt.timer_pending(), "nothing more is owed");
        // NEGATIVE CONTROL: Cruise takes nothing as the end.
        h.fake.set_status("autopilot", "cruise");
        h.fake.emit("meta field=autopilot value=cruise");
        wait_until("Cruise owes the defend prompt", || {
            journal_has(&h, "for continue-done-check@v1")
        })
        .await;
        assert!(!rt.state().landed);
        assert_eq!(h.mock.requests().len(), 2);
    });
}

/// NEEDS YOU end to end (aterm owner direction 2026-10-19: "be LOUDER ABOUT IT ... but NEVER
/// let that block you"): the worker's need goes on the person's screen under `owner=needs
/// kind=ask`, apart from codex-alab's own entry, and the act goes on with the ack; it stands
/// while the worker's reports leave it, and a person's turn clears it. NEGATIVE CONTROL: the
/// act is never held back for it.
#[test]
fn a_workers_need_goes_on_the_persons_screen_and_never_blocks() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in.\nNEEDS YOU: the signing key password"),
                reply("r2", "Stage 2 is in."),
                reply("r3", "Stage 3 is in."),
            ],
            &[("human_grace_s", "0"), ("continue_per_hour", "1")],
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
                .wait_for(|f| f.attention().get("needs").is_some_and(
                    |t| t == "Codex needs you: the signing key password \u{2014} carrying on"
                ))
                .await,
            "{:?} {:?}",
            h.fake.attention(),
            h.autopilot.supervisor().journal()
        );
        assert_eq!(
            h.fake.attention_kinds().get("needs").map(String::as_str),
            Some("ask")
        );
        assert!(!h.fake.attention().contains_key("codex-alab"));
        // The act went on, with the ack.
        wait_until("the continuation's yield waits for the cap", || {
            h.mock.requests().len() == 2 && journal_has(&h, "continue_per_hour=1 reached")
        })
        .await;
        let typed = h.mock.requests()[1].message_input_texts("user");
        assert_eq!(
            typed.last().map(String::as_str),
            Some(format!("keep going {}", codex_alab_autopilot::needs::NEEDS_ACK).as_str())
        );
        // A report silent on needs leaves it standing.
        assert!(h.fake.attention().contains_key("needs"));
        // A passive spell (a suspension) keeps it up, while codex-alab's own attention goes.
        h.fake.set_effective(false, "suspended");
        wait_until("passive", || !h.autopilot.supervisor().acting()).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            h.fake.attention().contains_key("needs"),
            "{:?} {:?}",
            h.fake.attention(),
            h.autopilot.supervisor().journal()
        );
        h.fake.set_effective(true, "enabled");
        wait_until("acting", || h.autopilot.supervisor().acting()).await;
        // A person's turn answers it.
        h.test
            .submit_turn("the credential is in the keychain")
            .await
            .expect("person's turn");
        assert!(
            h.fake
                .wait_for(|f| !f.attention().contains_key("needs"))
                .await,
            "{:?} {:?}",
            h.fake.attention(),
            h.autopilot.supervisor().journal()
        );
        wait_until("the person's turn ran", || h.mock.requests().len() == 3).await;
    });
}

/// A person's or the config's off takes the needs down with the claim (the person drove the
/// session). NEGATIVE CONTROL: up to the off, the need stood.
#[test]
fn an_off_takes_the_needs_down() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in.\nNEEDS YOU: a decision on the schema"),
                reply("r2", "Stage 2 is in."),
            ],
            &[("human_grace_s", "0"), ("continue_per_hour", "1")],
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
                .wait_for(|f| f.attention().contains_key("needs"))
                .await,
            "{:?}",
            h.autopilot.supervisor().journal()
        );
        wait_until("the continuation's yield waits for the cap", || {
            h.mock.requests().len() == 2 && journal_has(&h, "continue_per_hour=1 reached")
        })
        .await;
        h.fake.set_effective(false, "disabled");
        assert!(
            h.fake
                .wait_for(|f| !f.attention().contains_key("needs"))
                .await,
            "{:?} {:?}",
            h.fake.attention(),
            h.autopilot.supervisor().journal()
        );
        wait_until("the off is the reason", || {
            journal_has(&h, "CLEARED needs (a person's or the config's off)")
        })
        .await;
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
            &[("human_grace_s", "0"), ("continue", "false")],
        )
        .await;
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        h.test.submit_turn("do the task").await.expect("first turn");
        wait_until("the turn end was looked at", || {
            journal_has(&h, "nothing (continue is off)")
        })
        .await;
        let sup = h.autopilot.supervisor();
        sup.attention_for(
            &h.thread,
            "login",
            Some("login: Codex's sign-in was refused".to_string()),
        )
        .await;
        assert!(h.fake.wait_for(|f| !f.attention().is_empty()).await);
        async fn stop(h: &Harness, store: &ExtensionData) {
            let session = ExtensionData::new("session");
            let turn = ExtensionData::new("turn-side");
            let mode = h
                .test
                .codex
                .thread_settings_snapshot()
                .await
                .collaboration_mode;
            h.autopilot
                .on_turn_start(codex_extension_api::TurnStartInput {
                    turn_id: turn.level_id(),
                    collaboration_mode: &mode,
                    token_usage_at_turn_start: None,
                    session_store: &session,
                    thread_store: store,
                    turn_store: &turn,
                })
                .await;
            h.autopilot
                .on_turn_stop(TurnStopInput {
                    session_store: &session,
                    thread_store: store,
                    turn_store: &turn,
                })
                .await;
        }
        // A thread of the same source that the TUI never registered (a title, a memory).
        let side = ThreadId::new();
        let store = ExtensionData::new(side.to_string());
        store.insert(codex_alab_autopilot::extension::ThreadRt::new(
            side,
            None,
            false,
            Duration::ZERO,
        ));
        stop(&h, &store).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            sup.standing_attention().is_some(),
            "a side thread's success cleared the login wall: {:?}",
            sup.journal()
        );
        assert!(!h.fake.attention().is_empty());
        // Negative control: the registered main thread's turn that went through clears it.
        h.shared.note_root_thread(&h.thread);
        stop(&h, h.test.codex.thread_extension_data()).await;
        wait_until("the main thread's success clears the wall", || {
            sup.standing_attention().is_none()
        })
        .await;
    });
}

#[test]
fn a_daybreak_policy_refusal_continues_independent_work_without_repeating_the_request() {
    big_stack(|| async {
        policy_continuation::assert_cyber_continuation(None, None, false).await;
    });
}

#[test]
fn a_denied_account_waits_for_the_person_and_their_corrected_turn_succeeds() {
    big_stack(|| async {
        assert_refusal_recovers_after_correction(
            wiremock::ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "error": {
                    "type": "permission_error",
                    "code": "access_denied",
                    "message": "Access to this request was denied."
                }
            })),
            codex_protocol::protocol::CodexErrorInfo::HttpConnectionFailed {
                http_status_code: Some(403),
            },
        )
        .await;
    });
}

#[test]
fn a_daybreak_continuation_preserves_the_authenticated_account_and_explicit_program() {
    big_stack(|| async {
        let auth = CodexAuth::create_dummy_chatgpt_auth_for_testing();
        assert_eq!(auth.get_account_id().as_deref(), Some("account_id"));
        policy_continuation::assert_cyber_continuation(
            Some(auth),
            Some(CyberAccessProgram::DaybreakBlue),
            false,
        )
        .await;
    });
}

async fn assert_refusal_recovers_after_correction(
    refusal: wiremock::ResponseTemplate,
    expected: codex_protocol::protocol::CodexErrorInfo,
) {
    assert_refusal_recovers_with_auth(refusal, expected, None, None).await;
}

async fn assert_refusal_recovers_with_auth(
    refusal: wiremock::ResponseTemplate,
    expected: codex_protocol::protocol::CodexErrorInfo,
    auth: Option<CodexAuth>,
    program: Option<CyberAccessProgram>,
) {
    let expected_account = auth.as_ref().and_then(CodexAuth::get_account_id);
    let corrected = "Explain ordinary error handling in this repository.";
    let h = harness_with_auth(
        Mode::Current,
        vec![
            refusal,
            responses::sse_response(reply(
                "corrected",
                "Explanation delivered.\nATERM-TASK-DONE",
            )),
            responses::sse_response(reply(
                "defended",
                "The defense holds: nothing is left.\nATERM-TASK-DONE",
            )),
        ],
        &[("human_grace_s", "0")],
        &[],
        None,
        auth,
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
            codex_core::TurnInputRequest::user_input(vec![
                codex_protocol::user_input::UserInput::Text {
                    text: "the initial request".into(),
                    text_elements: Vec::new(),
                },
            ])
            .on_start(TurnStartOptions {
                cyber_access_program: program,
                ..Default::default()
            }),
        )
        .await
        .unwrap_or_else(|err| panic!("first turn: {err:?}"));
    let event = core_test_support::wait_for_event_with_timeout(
        &h.test.codex,
        |event| matches!(event, EventMsg::TurnComplete(_)),
        Duration::from_secs(60),
    )
    .await;
    let EventMsg::TurnComplete(ended) = event else {
        unreachable!()
    };
    assert_eq!(
        ended.error.and_then(|error| error.codex_error_info),
        Some(expected),
        "a refusal must end after one request; observed {} requests",
        h.mock.requests().len()
    );
    assert!(h.fake.wait_for(|f| f.attention().len() == 1).await);
    let attention = h.fake.attention();
    assert!(
        attention
            .values()
            .all(|text| text.contains("automatic continuation will not retry this turn")),
        "{attention:?}"
    );
    let rt = h
        .autopilot
        .thread_rt(&h.thread)
        .unwrap_or_else(|| panic!("thread runtime"));
    assert!(
        !rt.timer_pending(),
        "a policy refusal must not schedule an automatic retry"
    );
    assert!(
        h.fake.acts().is_empty(),
        "no continuation or access choice was sent"
    );
    assert_eq!(h.mock.requests().len(), 1);

    h.test
        .codex
        .start_or_steer_turn(
            codex_core::TurnInputRequest::user_input(vec![
                codex_protocol::user_input::UserInput::Text {
                    text: corrected.into(),
                    text_elements: Vec::new(),
                },
            ])
            .on_start(TurnStartOptions {
                cyber_access_program: program,
                ..Default::default()
            }),
        )
        .await
        .unwrap_or_else(|err| panic!("the person's corrected turn: {err:?}"));
    let event = core_test_support::wait_for_event_with_timeout(
        &h.test.codex,
        |event| matches!(event, EventMsg::TurnComplete(_)),
        Duration::from_secs(60),
    )
    .await;
    let EventMsg::TurnComplete(ended) = event else {
        unreachable!()
    };
    assert!(ended.error.is_none());
    assert_eq!(
        ended.last_agent_message.as_deref(),
        Some("Explanation delivered.\nATERM-TASK-DONE")
    );
    assert!(h.fake.wait_for(|f| f.attention().is_empty()).await);
    // FOREVER CRUISE: the corrected turn's done claim gets the defend prompt; its defense is a
    // done claim again, owed the prompt after the back-off; nothing records a finish.
    wait_until("the defense's done claim waits out the back-off", || {
        journal_has(&h, "for continue-done-check@v1") && rt.timer_pending()
    })
    .await;
    assert_eq!(
        rt.owed_timer().map(|owed| owed.rule),
        Some("continue-done-check@v1")
    );
    assert!(!h.fake.acts().iter().any(|act| act == "finished@v1"));
    h.test
        .codex
        .shutdown_and_wait()
        .await
        .unwrap_or_else(|err| panic!("shutdown acknowledgement: {err:?}"));

    let requests = h.mock.requests();
    assert_eq!(requests.len(), 3);
    assert!(
        requests[2]
            .message_input_texts("user")
            .last()
            .is_some_and(|text| text.starts_with(codex_alab_autopilot::words::DONE_CHECK)),
        "the corrected turn's done claim gets the defend prompt"
    );
    assert_eq!(
        requests[0].body_json()["model"],
        requests[1].body_json()["model"]
    );
    let expected_programs = program.map(|cyber| serde_json::json!({ "cyber": cyber }));
    for request in &requests[..2] {
        assert_eq!(
            request.body_json().get("access_programs"),
            expected_programs.as_ref(),
            "the person's explicit access choice is kept without enrollment or enabling"
        );
        if let Some(account) = expected_account.as_deref() {
            assert_eq!(
                request.header("chatgpt-account-id").as_deref(),
                Some(account)
            );
            assert!(
                request
                    .header("authorization")
                    .is_some_and(|auth| !auth.is_empty())
            );
        }
    }
    for header in ["authorization", "chatgpt-account-id"] {
        assert_eq!(requests[0].header(header), requests[1].header(header));
    }
    assert_eq!(
        requests[1]
            .message_input_texts("user")
            .last()
            .map(String::as_str),
        Some(corrected)
    );
    assert!(
        !h.fake
            .acts()
            .iter()
            .any(|act| act == "continue@v1" || act.contains("retry"))
    );
    assert_eq!(
        h.fake
            .requests()
            .iter()
            .filter(|request| request.contains("meta set attention owner=codex-alab "))
            .count(),
        1,
        "one actionable alert for the refusal"
    );
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
                reply("r2", "Stage 1 is in."),
                reply("r3", "Stage 2 is in."),
            ],
            &[("human_grace_s", "0"), ("continue_per_hour", "1")],
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
        // The person answers in the default mode: the attention goes, and the continuation
        // after their turn is typed (Default mode), then waits for the hourly cap, which shows
        // as a wait, never as an ask (amended 2026-10-27 after the parity sweep, gap 2).
        h.test.submit_turn("go ahead").await.expect("person's turn");
        assert!(
            h.fake
                .wait_for(|f| !f.attention().contains_key("codex-alab"))
                .await,
            "{:?} {:?}",
            h.fake.attention(),
            h.autopilot.supervisor().journal()
        );
        wait_until("the continuation after the person's turn", || {
            h.mock.requests().len() == 3 && journal_has(&h, "continue_per_hour=1 reached")
        })
        .await;
        assert!(
            h.fake
                .wait_for(|f| {
                    f.attention()
                        .get("codex-alab-wait")
                        .is_some_and(|w| w.starts_with(codex_alab_autopilot::words::HOUR_CAP))
                })
                .await,
            "{:?}",
            h.fake.attention()
        );
        assert_eq!(
            h.fake
                .attention_kinds()
                .get("codex-alab-wait")
                .map(String::as_str),
            Some("wait")
        );
        let attention = h.fake.attention();
        assert!(
            attention.keys().all(|owner| owner == "codex-alab-wait"),
            "no ask stands: {attention:?}"
        );
    });
}

/// Review of 4cdc96090, finding 3, for a goal: a goal that stopped by itself leaves the turn
/// end to a person with `goal:` attention; a suspension clears it, and acting again raises it
/// again, typing nothing. Blocked, and usage-limited where the point owes no usage resume (this
/// turn ended with no wall, so nothing resumes it at the window's reset: review of 5a513b0797,
/// where the goal box raised it once and a standby lost it for good). Negative control: a goal
/// a person paused instead raises nothing, before or after a passive spell.
#[test]
fn a_stopped_goals_attention_is_raised_again_when_acting_resumes() {
    for (stop, said) in [
        (
            codex_state::ThreadGoalStatus::Blocked,
            "goal: the goal is blocked",
        ),
        (
            codex_state::ThreadGoalStatus::UsageLimited,
            "goal: the goal is usage-limited and nothing resumes it",
        ),
    ] {
        big_stack(move || async move {
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
            goal(stop).await;
            h.test.submit_turn("do the task").await.expect("first turn");
            let stopped = |f: &FakeAterm| {
                f.attention()
                    .get("codex-alab")
                    .is_some_and(|t| t.starts_with(said))
            };
            assert!(
                h.fake.wait_for(stopped).await,
                "{stop:?}: {:?} {:?}",
                h.fake.attention(),
                h.autopilot.supervisor().journal()
            );
            h.fake.set_effective(false, "suspended");
            assert!(
                h.fake.wait_for(|f| f.attention().is_empty()).await,
                "{stop:?}: cleared at the off: {:?} {:?}",
                h.fake.attention(),
                h.autopilot.supervisor().journal()
            );
            h.fake.set_effective(true, "enabled");
            assert!(
                h.fake.wait_for(stopped).await,
                "{stop:?}: raised again: {:?} {:?}",
                h.fake.attention(),
                h.autopilot.supervisor().journal()
            );
            assert_eq!(
                h.mock.requests().len(),
                1,
                "{stop:?}: nothing typed past the goal"
            );
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
                "{stop:?}: {:?} {:?}",
                h.fake.attention(),
                h.autopilot.supervisor().journal()
            );
            assert_eq!(h.mock.requests().len(), 2);
        });
    }
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
            &[("human_grace_s", "0"), ("continue", "false")],
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

/// THE UPGRADE'S EXEC ASKED FOR WHILE AN ACT WRITES ITS RECORD STOPS THE ACT (review of
/// 2a9241bbf, finding 23 O; the guard after the checks went untested until the review of the
/// forever autopilot, 2026-10-20): the checks said go before the record's write-through, and
/// an exec under way by its end types nothing into the leaving process. NEGATIVE CONTROL: the
/// same act with no exec under way is typed.
#[test]
fn an_upgrade_asked_for_while_the_act_writes_its_record_types_nothing() {
    for begun in [true, false] {
        big_stack(move || async move {
            // The mock answers exactly the requests it is given replies for: the first turn's,
            // and, with no exec under way, the continuation's (whose done claim then waits out
            // the short streak's back-off).
            let mut replies = vec![reply("r1", "Stage 1 is in; stage 2 is next.")];
            if !begun {
                replies.push(reply("r2", "Stage 2 is in and pushed.\nATERM-TASK-DONE"));
            }
            let h = harness(Mode::Current, replies, &[("human_grace_s", "0")]).await;
            let autopilot = Arc::clone(&h.autopilot);
            let shared = h.shared;
            // The record's write-through, as the TUI's writer installs it: an act's (codex-alab
            // is acting) is where the exec is asked for, after the checks.
            assert!(shared.set_flush(Box::new(move || {
                if begun && shared.acting() {
                    autopilot.set_upgrade_requested(true);
                }
                true
            })));
            assert!(
                h.fake
                    .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                    .await
            );
            h.test.submit_turn("do the task").await.expect("first turn");
            if begun {
                wait_until("the act stops at the guard", || {
                    journal_has(&h, "the upgrade's exec began since the checks; no act")
                })
                .await;
                assert_eq!(h.mock.requests().len(), 1, "nothing was typed");
                assert!(!h.fake.acts().iter().any(|a| a == "continue@v1"));
            } else {
                wait_until("the continuation is typed and its done claim waits", || {
                    journal_has(&h, "for continue-done-check@v1")
                })
                .await;
                assert_eq!(h.mock.requests().len(), 2);
                assert!(h.fake.acts().iter().any(|a| a == "continue@v1"));
                assert!(!journal_has(&h, "the upgrade's exec"));
            }
        });
    }
}

/// A WALL'S WAIT IS CARRIED IN THE RECORD (FOREVER CRUISE: the wait is rest, so the record
/// says `idle` there, and a crash in it left aterm's relaunch nothing to type until a
/// person's turn): a server's error past core's retries waits its rung with the retry owed in
/// the record, its words and mode as decided, as a paced act's wait is, so aterm's relaunch of
/// a session that died in the wait types the retry at its time (`Relaunch::Owed`), and the
/// live upgrade carries it rather than waiting on it. NEGATIVE CONTROL: with
/// `retry_api_errors` off the wall is raised, and nothing is owed.
#[test]
fn a_walls_wait_is_carried_in_the_record() {
    for retry in [true, false] {
        big_stack(move || async move {
            let h = harness_with_request_retries(
                Mode::Current,
                vec![
                    wiremock::ResponseTemplate::new(500).set_body_json(serde_json::json!({
                        "error": {"message": "Internal server error."}
                    })),
                ],
                &[
                    ("human_grace_s", "0"),
                    ("retry_api_errors", if retry { "true" } else { "false" }),
                ],
                Some(0),
            )
            .await;
            assert!(
                h.fake
                    .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                    .await
            );
            h.test
                .codex
                .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "Do the task.".to_string(),
                    text_elements: Vec::new(),
                }]))
                .await
                .expect("start the task");
            let rt = h.autopilot.thread_rt(&h.thread).expect("root thread");
            if !retry {
                wait_until("the wall is raised", || {
                    journal_has(&h, "retry_api_errors is off")
                })
                .await;
                assert!(!rt.timer_pending());
                assert_eq!(h.shared.owed_act(), None);
                return;
            }
            wait_until("the retry waits its rung", || {
                journal_has(&h, "s for api-retry@v1 (") && rt.timer_pending()
            })
            .await;
            wait_until("the record carries the retry", || {
                h.shared.owed_act().is_some()
            })
            .await;
            let owed = h.shared.owed_act().expect("owed");
            assert_eq!(owed.thread, h.thread);
            assert_eq!(owed.rule.as_deref(), Some("api-retry@v1"));
            assert!(
                owed.text
                    .as_deref()
                    .is_some_and(|text| text.starts_with("Codex reported")),
                "{owed:?}"
            );
            assert_eq!(owed.mode.as_deref(), Some("cruise"));
            assert!(!owed.resume_goal);
            assert!(owed.not_before_ms > now_ms(), "at its rung: {owed:?}");
            assert_eq!(rt.owed_timer().map(|owed| owed.rule), Some("api-retry@v1"));
            assert_eq!(h.mock.requests().len(), 1, "nothing typed before the rung");
        });
    }
}

/// RULING F END TO END (aterm's typing stop): a worker that answers codex-alab's act with
/// the same reply twice and no tool run is answered once more, then nothing is typed, and the
/// stop is told quietly once (an `appnotice` in aterm's words, never an act word nor
/// attention; the once-per-arm rule and the TUI's warning are passive_tests'). NEGATIVE
/// CONTROL: the first same answer is still continued.
#[test]
fn repeated_replies_schedule_recovery_without_attention() {
    big_stack(|| async {
        const SAME: &str = "Still on stage 1; looking into it.";
        let h = harness(
            Mode::Current,
            vec![reply("r1", SAME), reply("r2", SAME), reply("r3", SAME)],
            &[("human_grace_s", "0")],
        )
        .await;
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        h.test.submit_turn("do the task").await.expect("first turn");
        wait_until("recovery is scheduled", || {
            h.autopilot
                .supervisor()
                .journal()
                .iter()
                .any(|line| line.contains("repeated report: change approach after pacing"))
        })
        .await;
        assert_eq!(
            h.mock.requests().len(),
            3,
            "the person's, then two continuations"
        );
        // The two continuations' words and nothing else: no act word for the stop.
        assert_eq!(h.fake.acts(), ["continue@v1", "continue@v1"]);
        assert!(
            h.fake.attention().is_empty(),
            "never attention: {:?}",
            h.fake.attention()
        );
        let rt = h
            .autopilot
            .thread_rt(&h.thread)
            .expect("the thread is the autopilot's");
        assert_eq!(
            rt.state().held_by(),
            Some(codex_alab_autopilot::replies::HeldBy::SameReply)
        );
        assert!(rt.timer_pending(), "paced recovery remains scheduled");
    });
}

/// THE TWO REQUESTS AFTER A USAGE WAIT ARE TWO ACTS (WF5 of the codex-alab proof, 2026-10-20:
/// two `usage:continue` requests 19-30 ms apart): the usage window's resume, typed once at
/// the reset, and the continuation at the point its short yield leaves, typed at once
/// because a resume's yield is no continuation's short turn (aterm's `is_continue_rule`
/// leaves `RULE_LIMIT_RESUME` out); the continuation's own short yield then backs the next
/// act off. The resume types its one cleared line (`words::LIMIT_CLEARED`, aterm's
/// `cleared_text`), which the lab's model tags `usage:cleared`; the continuation types
/// `continue_text` (`usage:continue`). NEGATIVE CONTROL: with `continue` off only the resume is
/// typed.
#[test]
fn a_usage_waits_resume_and_the_continuation_after_it_are_two_acts() {
    for continue_on in [true, false] {
        big_stack(move || async move {
            let resets_at = now_ms() / 1000 - 1;
            let limit = wiremock::ResponseTemplate::new(429).set_body_json(serde_json::json!({
                "error": {
                    "type": "usage_limit_reached",
                    "message": "limit reached",
                    "resets_at": resets_at,
                    "plan_type": "pro"
                }
            }));
            let resumed = |said: &str| responses::sse_response(reply("resumed", said));
            // The mock answers exactly the requests it is given replies for. The
            // continuation's yield in its own words: the resume's reply said again with no
            // work would be ruling F's stop, which holds the act this test waits for.
            let mut replies = vec![limit, resumed("Resumed after the reset.")];
            if continue_on {
                replies.push(resumed("Carried on with the task."));
            }
            let h = harness_timed(
                Mode::Current,
                replies,
                &[
                    ("human_grace_s", "0"),
                    ("resume_limits", "true"),
                    ("limit_wait", "true"),
                    ("continue", if continue_on { "true" } else { "false" }),
                ],
                &[],
                Some(0),
                None,
                Timing {
                    reset_grace: Duration::ZERO,
                    reset_jitter: Duration::ZERO,
                    ..Timing::default()
                },
            )
            .await;
            assert!(
                h.fake
                    .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                    .await
            );
            h.test.submit_turn("do the task").await.expect("first turn");
            let typed = |act: &str| h.fake.acts().iter().filter(|a| *a == act).count();
            if !continue_on {
                wait_until("the resume's yield is left", || {
                    journal_has(&h, "nothing (continue is off)")
                })
                .await;
                assert_eq!(typed("usage-resume@v1"), 1);
                assert_eq!(typed("continue@v1"), 0);
                assert_eq!(h.mock.requests().len(), 2, "the person's, the resume's");
                return;
            }
            wait_until(
                "the continuation's short yield backs the next act off",
                || journal_has(&h, "s for continue@v1 (continue@v1 after 1 short turn(s)"),
            )
            .await;
            let requests = h.mock.requests();
            assert_eq!(
                requests.len(),
                3,
                "the person's, the resume's, the continuation's"
            );
            let last = |i: usize| requests[i].message_input_texts("user").last().cloned();
            assert_eq!(
                last(1).as_deref(),
                Some(codex_alab_autopilot::words::LIMIT_CLEARED)
            );
            assert_eq!(last(2).as_deref(), Some("keep going"));
            assert_eq!(typed("usage-resume@v1"), 1, "{:?}", h.fake.acts());
            assert_eq!(typed("continue@v1"), 1, "{:?}", h.fake.acts());
            let acts = h.fake.acts();
            let at = |act: &str| acts.iter().position(|a| a == act);
            assert!(at("usage-resume@v1") < at("continue@v1"), "{acts:?}");
        });
    }
}
