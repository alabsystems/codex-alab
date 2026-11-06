// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native recovery also covers the Responses requests compaction makes: a failed
//! context reset must not look clean, and provider timing survives every request lane.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use codex_alab_aterm::Options as ClientOptions;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_autopilot::Autopilot;
use codex_alab_autopilot::Supervisor;
use codex_alab_autopilot::SupervisorOptions;
use codex_alab_autopilot::decide::Timing;
use codex_alab_autopilot::decide::Wall;
use codex_alab_autopilot::words;
use codex_alab_core::capacity::SamplingRetryAdvice;
use codex_alab_core::session::Frame;
use codex_alab_core::session::ProgramState;
use codex_alab_core::session::Shared;
use codex_alab_core::session::now_ms;
use codex_core::config::Config;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::TurnErrorInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_login::CodexAuth;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::turn_input::CyberAccessProgram;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::turn_input::TurnInputSubmission;
use codex_protocol::turn_input::TurnStartOptions;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

#[path = "support/progress.rs"]
mod test_progress;

const MODEL: &str = "gpt-5.5";
const HANG_DETECTOR: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Errors(Mutex<Vec<(CodexErrorInfo, Option<Instant>)>>);

impl TurnLifecycleContributor for Errors {
    fn on_turn_error<'a>(&'a self, input: TurnErrorInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let retry_at = input
                .turn_store
                .get::<SamplingRetryAdvice>()
                .and_then(|advice| advice.current_deadline());
            self.0
                .lock()
                .unwrap_or_else(|err| panic!("error observations: {err:?}"))
                .push((input.error, retry_at));
        })
    }
}

struct Harness {
    fake: FakeAterm,
    autopilot: Arc<Autopilot>,
    test: TestCodex,
    errors: Arc<Errors>,
    requests: Arc<Mutex<Vec<(Instant, Value)>>>,
    accounts: Arc<Mutex<Vec<Option<String>>>>,
    thread: String,
    _server: wiremock::MockServer,
    _dir: tempfile::TempDir,
}

impl Harness {
    async fn new(v2: bool, retries: u64, replies: Vec<ResponseTemplate>) -> Self {
        Self::with_limit(v2, retries, replies, None).await
    }

    async fn with_limit(
        v2: bool,
        retries: u64,
        replies: Vec<ResponseTemplate>,
        compact_limit: Option<i64>,
    ) -> Self {
        Self::with_account(v2, retries, replies, compact_limit, None).await
    }

    async fn with_account(
        v2: bool,
        retries: u64,
        replies: Vec<ResponseTemplate>,
        compact_limit: Option<i64>,
        auth: Option<CodexAuth>,
    ) -> Self {
        Self::with_compaction(v2, retries, replies, compact_limit, auth, false).await
    }

    async fn with_compaction(
        v2: bool,
        retries: u64,
        replies: Vec<ResponseTemplate>,
        compact_limit: Option<i64>,
        auth: Option<CodexAuth>,
        post_turn: bool,
    ) -> Self {
        Self::with_compaction_and_setup_observer(
            v2,
            retries,
            replies,
            compact_limit,
            auth,
            post_turn,
            None,
        )
        .await
    }

    async fn with_compaction_and_setup_observer(
        v2: bool,
        retries: u64,
        replies: Vec<ResponseTemplate>,
        compact_limit: Option<i64>,
        auth: Option<CodexAuth>,
        post_turn: bool,
        setup_observer: Option<Arc<dyn Fn(&'static str) + Send + Sync>>,
    ) -> Self {
        let observe = |phase| {
            if let Some(observer) = &setup_observer {
                observer(phase);
            }
        };
        observe("setup: create fake aterm");
        codex_alab_core::activate();
        let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("owned control socket: {err:?}"));
        let fake = FakeAterm::start(dir.path(), Mode::Current)
            .await
            .unwrap_or_else(|err| panic!("fake aterm: {err:?}"));
        fake.set_policy_key("continue", "false");
        fake.set_policy_key("human_grace_s", "0");
        observe("setup: register extensions");
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let endpoint = fake.endpoint();
        let sup = Supervisor::new(SupervisorOptions {
            shared,
            endpoint: Arc::new(move || Some(endpoint.clone())),
            client: ClientOptions::default(),
        });
        let mut builder = ExtensionRegistryBuilder::<Config>::new();
        let autopilot = Autopilot::new(
            sup,
            None,
            None,
            Timing {
                min_work: Duration::ZERO,
                retry_backoff: vec![Duration::from_millis(1)],
                ..Timing::default()
            },
            |_| true,
        );
        autopilot.register(&mut builder);
        let errors = Arc::new(Errors::default());
        builder.turn_lifecycle_contributor(errors.clone());
        observe("setup: start mock server");
        let server = responses::start_mock_server().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let accounts = Arc::new(Mutex::new(Vec::new()));
        let captured_accounts = Arc::clone(&accounts);
        let replies = Mutex::new(VecDeque::from(replies));
        Mock::given(method("POST"))
            .and(path_regex(".*/responses$"))
            .respond_with(move |request: &wiremock::Request| {
                captured_accounts
                    .lock()
                    .unwrap_or_else(|err| panic!("account observations: {err:?}"))
                    .push(
                        request
                            .headers
                            .get("chatgpt-account-id")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_owned),
                    );
                captured
                    .lock()
                    .unwrap_or_else(|err| panic!("request observations: {err:?}"))
                    .push((
                        Instant::now(),
                        request
                            .body_json()
                            .unwrap_or_else(|err| panic!("Responses JSON: {err:?}")),
                    ));
                replies
                    .lock()
                    .unwrap_or_else(|err| panic!("scripted responses: {err:?}"))
                    .pop_front()
                    .unwrap_or_else(|| panic!("no unexpected retry or continuation"))
            })
            .mount(&server)
            .await;
        let mut test_builder = test_codex()
            .with_model(MODEL)
            .with_extensions(Arc::new(builder.build()))
            .with_config(move |config| {
                config
                    .features
                    .disable(codex_features::Feature::EnableRequestCompression)
                    .unwrap_or_else(|err| panic!("uncompressed fixture requests: {err:?}"));
                config.model_auto_compact_token_limit = compact_limit;
                config.update_plan_enabled = true;
                config.model_post_turn_compact_threshold_percent = if post_turn { 100 } else { 0 };
                config.model_provider.request_max_retries = Some(0);
                config.model_provider.stream_max_retries = Some(retries);
                config.model_provider.capabilities = Some(
                    serde_json::from_value(json!({
                        "remote_compaction": if v2 { "v2" } else { "unsupported" }
                    }))
                    .unwrap_or_else(|err| panic!("provider compaction capability: {err:?}")),
                );
            });
        if let Some(auth) = auth {
            test_builder = test_builder.with_auth(auth);
        }
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
            .unwrap_or_else(|err| panic!("native core fixture: {err:?}"));
        observe("setup: bind thread manager");
        autopilot.bind_thread_manager(Arc::downgrade(&test.thread_manager));
        let thread = test.session_configured.thread_id.to_string();
        shared.observe(
            &Frame {
                primary_thread: Some(&thread),
                displayed_thread: Some(&thread),
                rollout_path: None,
                cwd: test.cwd_path(),
                model: MODEL,
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
        assert!(
            fake.wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await,
            "native autopilot owns its proven claim"
        );
        Self {
            fake,
            autopilot,
            test,
            errors,
            requests,
            accounts,
            thread,
            _server: server,
            _dir: dir,
        }
    }

    async fn shutdown(&self) {
        self.test
            .codex
            .shutdown_and_wait()
            .await
            .unwrap_or_else(|err| panic!("shutdown: {err:?}"));
    }
}

fn big_stack<F, Fut>(test: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()>,
{
    big_stack_with_progress(move |_| test());
}

fn big_stack_with_progress<F, Fut>(test: F)
where
    F: FnOnce(test_progress::Progress) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()>,
{
    let result = std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_stack_size(16 << 20)
                .enable_all()
                .build()
                .unwrap_or_else(|err| panic!("owned test runtime: {err:?}"))
                .block_on(async {
                    let progress = test_progress::Progress::default();
                    tokio::time::timeout(HANG_DETECTOR, test(progress.clone()))
                        .await
                        .unwrap_or_else(|err| {
                            panic!(
                                "compaction recovery hang detector: {err:?}; {}",
                                progress.describe()
                            )
                        });
                });
        })
        .unwrap_or_else(|err| panic!("owned test thread: {err:?}"))
        .join();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn reply(id: &str, text: &str) -> ResponseTemplate {
    responses::sse_response(responses::sse(vec![
        responses::ev_response_created(id),
        responses::ev_assistant_message(&format!("{id}-message"), text),
        responses::ev_completed(id),
    ]))
}

async fn wait_until(done: impl Fn() -> bool) {
    tokio::time::timeout(HANG_DETECTOR, async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|err| panic!("expected recovery state: {err:?}"));
}

fn attention_is(fake: &FakeAterm, prefix: &str) -> bool {
    fake.attention()
        .get("codex-alab")
        .is_some_and(|text| text.starts_with(prefix))
}

async fn corrected_turn(h: &Harness, program: Option<CyberAccessProgram>) {
    let accepted = h
        .test
        .codex
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Explain ordinary error handling.".into(),
                text_elements: Vec::new(),
            }])
            .on_start(TurnStartOptions {
                cyber_access_program: program,
                ..Default::default()
            }),
        )
        .await
        .unwrap_or_else(|err| panic!("the person's corrected turn: {err:?}"));
    let TurnInputSubmission::Started { turn_id, .. } = accepted else {
        panic!("the correction must start its own native turn: {accepted:?}");
    };
    // The failed compaction's completion can still be queued. Only this correction's
    // native identity proves sampling finished; clearing old attention proves its start.
    let event = core_test_support::wait_for_event_with_timeout(
        &h.test.codex,
        |event| matches!(event, EventMsg::TurnComplete(done) if done.turn_id == turn_id),
        HANG_DETECTOR,
    )
    .await;
    let EventMsg::TurnComplete(done) = event else {
        unreachable!()
    };
    assert!(
        done.error.is_none(),
        "the corrected native turn must succeed: {done:?}"
    );
    assert_eq!(
        done.last_agent_message.as_deref(),
        Some("The corrected request succeeded.")
    );
}

fn transient(advice: bool) -> ResponseTemplate {
    let response = ResponseTemplate::new(503).set_body_json(json!({"error": {
        "code": "temporary_failure", "message": "Try again later."
    }}));
    if advice {
        response.insert_header("Retry-After", "2")
    } else {
        response
    }
}

fn compacted(v2: bool) -> ResponseTemplate {
    if v2 {
        responses::sse_response(responses::sse(vec![
            json!({"type": "response.output_item.done", "item": {
                "type": "compaction", "encrypted_content": "corrected-compaction-summary"
            }}),
            responses::ev_completed("corrected-compaction"),
        ]))
    } else {
        reply(
            "corrected-compaction",
            "The original task's context is preserved.",
        )
    }
}

#[test]
fn compaction_capacity_recovers_in_place_without_ordinary_retries_or_autopilot() {
    big_stack(|| async {
        for v2 in [false, true] {
            let completed_work = responses::sse_response(responses::sse(vec![
                responses::ev_response_created("completed-work"),
                responses::ev_function_call(
                    "completed-plan-call",
                    "update_plan",
                    &json!({"plan": [{
                        "step": "Retain completed work through compaction overload",
                        "status": "completed"
                    }]})
                    .to_string(),
                ),
                responses::ev_completed("completed-work"),
            ]));
            let h = Harness::new(
                v2,
                0,
                vec![
                    completed_work,
                    reply("initial", "The original task's work is complete."),
                    responses::sse_response(responses::sse_failed(
                        "overloaded-compaction",
                        "server_is_overloaded",
                        "The selected model is at capacity.",
                    )),
                    ResponseTemplate::new(503)
                        .insert_header("Retry-After", "2")
                        .set_body_json(json!({"error": {
                            "code": "server_is_overloaded",
                            "message": "The selected model is still at capacity."
                        }})),
                    compacted(v2),
                ],
            )
            .await;
            h.fake.set_policy_key("retry_api_errors", "false");
            h.test
                .submit_text_turn("Complete this task before compacting its context.")
                .await
                .unwrap_or_else(|err| panic!("person's task: {err:?}"));
            h.test
                .codex
                .submit(Op::Compact)
                .await
                .unwrap_or_else(|err| panic!("manual compact: {err:?}"));
            let mut turns = Vec::new();
            let mut waits = 0;
            loop {
                match h
                    .test
                    .codex
                    .next_event()
                    .await
                    .unwrap_or_else(|err| panic!("compaction event: {err:?}"))
                    .msg
                {
                    EventMsg::TurnStarted(event) => turns.push(event.turn_id),
                    EventMsg::StreamError(event) => {
                        assert!(event.message.contains("retrying automatically"));
                        waits += 1;
                    }
                    EventMsg::Error(error) => panic!("capacity became terminal: {error:?}"),
                    EventMsg::TurnAborted(event) => panic!("compaction aborted: {event:?}"),
                    EventMsg::TurnComplete(event) => {
                        assert_eq!(turns, vec![event.turn_id]);
                        assert!(event.error.is_none());
                        break;
                    }
                    _ => {}
                }
            }
            assert_eq!(waits, 2, "both overloads stay inside the same compaction");
            h.shutdown().await;
            assert!(h.fake.acts().is_empty(), "no Autopilot recovery is needed");
            assert!(
                h.errors
                    .0
                    .lock()
                    .unwrap_or_else(|err| panic!("errors: {err:?}"))
                    .is_empty()
            );
            let requests = h
                .requests
                .lock()
                .unwrap_or_else(|err| panic!("requests: {err:?}"));
            assert_eq!(requests.len(), 5, "completed work is never executed again");
            assert!(
                requests[2].1["input"]
                    .as_array()
                    .is_some_and(|input| !input.is_empty())
            );
            assert_eq!(requests[2].1["model"], MODEL);
            for retry in &requests[3..] {
                assert_eq!(retry.1["input"], requests[2].1["input"]);
                assert_eq!(retry.1["model"], requests[2].1["model"]);
            }
            assert!(requests[3].0 >= requests[2].0 + Duration::from_millis(500));
            assert!(requests[4].0 >= requests[3].0 + Duration::from_secs(2));
        }
    });
}

#[test]
fn interrupt_cancels_compaction_capacity_wait_with_long_server_advice() {
    big_stack(|| async {
        for v2 in [false, true] {
            let h = Harness::new(
                v2,
                0,
                vec![
                    reply("initial", "The task has context to compact."),
                    ResponseTemplate::new(503)
                        .insert_header("Retry-After", "3600")
                        .set_body_json(json!({"error": {
                            "code": "server_is_overloaded",
                            "message": "The selected model is at capacity."
                        }})),
                ],
            )
            .await;
            h.fake.set_policy_key("retry_api_errors", "false");
            h.test
                .submit_text_turn("Keep this task's context.")
                .await
                .unwrap_or_else(|err| panic!("person's task: {err:?}"));
            h.test
                .codex
                .submit(Op::Compact)
                .await
                .unwrap_or_else(|err| panic!("compact: {err:?}"));
            let mut turns = Vec::new();
            loop {
                match h
                    .test
                    .codex
                    .next_event()
                    .await
                    .unwrap_or_else(|err| panic!("capacity wait: {err:?}"))
                    .msg
                {
                    EventMsg::TurnStarted(event) => turns.push(event.turn_id),
                    EventMsg::StreamError(event) => {
                        assert!(event.message.contains("retrying automatically"));
                        break;
                    }
                    EventMsg::Error(error) => panic!("capacity became terminal: {error:?}"),
                    EventMsg::TurnComplete(event) => panic!("compaction ended early: {event:?}"),
                    EventMsg::TurnAborted(event) => panic!("unexpected interruption: {event:?}"),
                    _ => {}
                }
            }
            h.test
                .codex
                .submit(Op::Interrupt)
                .await
                .unwrap_or_else(|err| panic!("interrupt: {err:?}"));
            loop {
                match h
                    .test
                    .codex
                    .next_event()
                    .await
                    .unwrap_or_else(|err| panic!("interruption: {err:?}"))
                    .msg
                {
                    EventMsg::TurnAborted(event) => {
                        assert_eq!(event.reason, TurnAbortReason::Interrupted);
                        assert_eq!(turns.len(), 1);
                        assert_eq!(event.turn_id.as_deref(), Some(turns[0].as_str()));
                        break;
                    }
                    EventMsg::Error(error) => panic!("interruption became an error: {error:?}"),
                    EventMsg::TurnComplete(event) => panic!("interruption completed: {event:?}"),
                    _ => {}
                }
            }
            h.shutdown().await;
            assert_eq!(
                h.requests
                    .lock()
                    .unwrap_or_else(|err| panic!("requests: {err:?}"))
                    .len(),
                2
            );
            assert!(h.fake.acts().is_empty());
            assert!(
                h.errors
                    .0
                    .lock()
                    .unwrap_or_else(|err| panic!("errors: {err:?}"))
                    .is_empty()
            );
        }
    });
}

#[test]
fn compaction_overload_label_does_not_retry_a_permanent_refusal() {
    big_stack(|| async {
        for v2 in [false, true] {
            let h = Harness::new(
                v2,
                2,
                vec![
                    reply("initial", "The task has context to compact."),
                    ResponseTemplate::new(403).set_body_json(json!({"error": {
                        "code": "server_is_overloaded", "message": "Access denied."
                    }})),
                ],
            )
            .await;
            h.test
                .submit_text_turn("Keep this task's context.")
                .await
                .unwrap_or_else(|err| panic!("person's task: {err:?}"));
            h.test
                .codex
                .submit(Op::Compact)
                .await
                .unwrap_or_else(|err| panic!("compact: {err:?}"));
            assert!(h.fake.wait_for(|f| attention_is(f, "stopped:")).await);
            h.shutdown().await;
            assert_eq!(
                h.requests
                    .lock()
                    .unwrap_or_else(|err| panic!("requests: {err:?}"))
                    .len(),
                2
            );
            assert!(h.fake.acts().is_empty());
        }
    });
}

#[test]
fn compaction_refusals_stop_the_autopilot_until_a_corrected_persons_turn() {
    big_stack(|| async {
        for v2 in [false, true] {
            for policy in [false, true] {
                let denied = if policy {
                    responses::sse_response(responses::sse_failed(
                        "refused-compaction",
                        "cyber_policy",
                        "Cybersecurity access denied.",
                    ))
                } else {
                    ResponseTemplate::new(403).set_body_json(json!({"error": {
                        "code": "access_denied", "message": "Access denied."
                    }}))
                };
                let h = Harness::new(
                    v2,
                    2,
                    vec![
                        responses::sse_response(responses::sse_failed(
                            "context-full",
                            "context_length_exceeded",
                            "Context is full.",
                        )),
                        denied,
                        compacted(v2),
                        reply("corrected", "The corrected request succeeded."),
                    ],
                )
                .await;
                h.test
                    .submit_text_turn("Complete the original task.")
                    .await
                    .unwrap_or_else(|err| panic!("person's task: {err:?}"));
                assert!(
                    h.fake.wait_for(|f| attention_is(f, "stopped:")).await,
                    "v2={v2} policy={policy}: {:?}",
                    h.autopilot.supervisor().journal()
                );
                let rt = h
                    .autopilot
                    .thread_rt(&h.thread)
                    .unwrap_or_else(|| panic!("thread runtime"));
                assert!(matches!(
                    rt.state().point.and_then(|end| end.wall),
                    Some(Wall::Fatal { .. })
                ));
                assert!(!rt.timer_pending());
                assert_eq!(
                    h.requests
                        .lock()
                        .unwrap_or_else(|err| panic!("requests: {err:?}"))
                        .len(),
                    2,
                    "a compaction refusal is not replayed"
                );
                assert_eq!(h.fake.acts(), vec!["context-compact@v1".to_string()]);
                {
                    let observed = h
                        .errors
                        .0
                        .lock()
                        .unwrap_or_else(|err| panic!("errors: {err:?}"));
                    assert_eq!(observed.len(), 2, "one error per failed turn");
                    assert_eq!(
                        observed[1].0,
                        if policy {
                            CodexErrorInfo::CyberPolicy
                        } else {
                            CodexErrorInfo::HttpConnectionFailed {
                                http_status_code: Some(403),
                            }
                        }
                    );
                }
                corrected_turn(&h, None).await;
                wait_until(|| !attention_is(&h.fake, "stopped:")).await;
                h.shutdown().await;
                assert_eq!(
                    h.requests
                        .lock()
                        .unwrap_or_else(|err| panic!("requests: {err:?}"))
                        .len(),
                    4
                );
                assert_eq!(h.fake.acts(), vec!["context-compact@v1".to_string()]);
            }
        }
    });
}

#[test]
fn a_cyber_refusal_during_compaction_does_not_replay_it_as_an_independent_turn() {
    big_stack(|| async {
        for v2 in [false, true] {
            let auth = CodexAuth::create_dummy_chatgpt_auth_for_testing();
            let account = auth
                .get_account_id()
                .unwrap_or_else(|| panic!("selected account"));
            let program = CyberAccessProgram::DaybreakBlue;
            let h = Harness::with_account(
                v2,
                2,
                vec![
                    responses::sse_response(responses::sse_failed(
                        "context-full",
                        "context_length_exceeded",
                        "Context is full.",
                    )),
                    responses::sse_response(responses::sse_failed(
                        "refused-compaction",
                        "cyber_policy",
                        "Cybersecurity access denied.",
                    )),
                    compacted(v2),
                    reply("corrected", "The corrected request succeeded."),
                ],
                None,
                Some(auth),
            )
            .await;
            h.fake.set_policy_key("continue", "true");
            wait_until(|| {
                h.autopilot
                    .supervisor()
                    .policy()
                    .is_some_and(|policy| policy.continue_on)
            })
            .await;
            h.test
                .codex
                .start_or_steer_turn(
                    TurnInputRequest::user_input(vec![UserInput::Text {
                        text: "Complete the originally authorized task.".into(),
                        text_elements: Vec::new(),
                    }])
                    .on_start(TurnStartOptions {
                        cyber_access_program: Some(program),
                        ..Default::default()
                    }),
                )
                .await
                .unwrap_or_else(|err| panic!("the initial native turn: {err:?}"));
            wait_until(|| {
                attention_is(&h.fake, "stopped:")
                    || h.requests
                        .lock()
                        .unwrap_or_else(|err| panic!("requests: {err:?}"))
                        .len()
                        > 2
            })
            .await;
            assert_eq!(
                h.requests
                    .lock()
                    .unwrap_or_else(|err| panic!("requests: {err:?}"))
                    .len(),
                2,
                "a new automatic turn would recompact the denied context before its independent-work message; {:?}",
                h.autopilot.supervisor().journal()
            );
            assert!(attention_is(&h.fake, "stopped:"));
            assert!(
                !h.autopilot
                    .thread_rt(&h.thread)
                    .unwrap_or_else(|| panic!("thread"))
                    .timer_pending()
            );
            assert_eq!(h.fake.acts(), [words::RULE_COMPACT]);
            h.fake.set_policy_key("continue", "false");
            wait_until(|| {
                h.autopilot
                    .supervisor()
                    .policy()
                    .is_some_and(|policy| !policy.continue_on)
            })
            .await;
            corrected_turn(&h, Some(program)).await;
            wait_until(|| !attention_is(&h.fake, "stopped:")).await;
            h.shutdown().await;
            let requests = h
                .requests
                .lock()
                .unwrap_or_else(|err| panic!("requests: {err:?}"));
            assert_eq!(
                requests.len(),
                4,
                "only the person's corrected task permits new requests"
            );
            for (_, request) in requests.iter() {
                assert_eq!(request["model"], MODEL);
            }
            for request in [&requests[0].1, &requests[3].1] {
                assert_eq!(request["access_programs"], json!({"cyber":program}));
            }
            assert!(
                h.accounts
                    .lock()
                    .unwrap_or_else(|err| panic!("accounts: {err:?}"))
                    .iter()
                    .all(|seen| seen.as_deref() == Some(account.as_str()))
            );
            let corrected = &requests
                .last()
                .unwrap_or_else(|| panic!("corrected request"))
                .1;
            let text = corrected["input"]
                .as_array()
                .unwrap_or_else(|| panic!("input"))
                .iter()
                .filter(|item| item["role"] == "user")
                .flat_map(|item| item["content"].as_array().into_iter().flatten())
                .filter_map(|item| item["text"].as_str())
                .next_back();
            assert_eq!(text, Some("Explain ordinary error handling."));
            assert_eq!(h.fake.acts(), [words::RULE_COMPACT]);
        }
    });
}

#[test]
fn a_compaction_deadline_survives_exhaustion_and_delays_native_recovery() {
    big_stack(|| async {
        for v2 in [false, true] {
            let h = Harness::new(
                v2,
                0,
                vec![
                    reply("initial", "The task has more work."),
                    transient(true),
                    reply("recovered", "The original task recovered."),
                ],
            )
            .await;
            h.test
                .submit_text_turn("Complete this task.")
                .await
                .unwrap_or_else(|err| panic!("person's task: {err:?}"));
            h.test
                .codex
                .submit(Op::Compact)
                .await
                .unwrap_or_else(|err| panic!("manual compact: {err:?}"));
            wait_until(|| {
                h.requests
                    .lock()
                    .unwrap_or_else(|err| panic!("requests: {err:?}"))
                    .len()
                    == 3
            })
            .await;
            // Join the recovered turn, so no queued act can hide behind a count assertion.
            loop {
                if let EventMsg::TurnComplete(event) = h
                    .test
                    .codex
                    .next_event()
                    .await
                    .unwrap_or_else(|err| panic!("core event: {err:?}"))
                    .msg
                    && event.last_agent_message.as_deref() == Some("The original task recovered.")
                {
                    break;
                }
            }
            h.shutdown().await;
            let errors = h
                .errors
                .0
                .lock()
                .unwrap_or_else(|err| panic!("errors: {err:?}"));
            assert_eq!(errors.len(), 1, "compaction errors reach lifecycle once");
            let retry_at = errors[0]
                .1
                .unwrap_or_else(|| panic!("compaction retained provider advice"));
            let requests = h
                .requests
                .lock()
                .unwrap_or_else(|err| panic!("requests: {err:?}"));
            assert_eq!(requests.len(), 3);
            assert!(retry_at >= requests[1].0 + Duration::from_secs(2));
            assert!(
                requests[2].0 >= retry_at,
                "v2={v2}: native continuation bypassed compaction's deadline"
            );
            assert!(
                requests
                    .iter()
                    .all(|(_, request)| request["model"] == MODEL)
            );
        }
    });
}

#[test]
fn native_pre_mid_and_post_turn_policy_refusals_preserve_the_compaction_boundary() {
    big_stack_with_progress(|progress| async move {
        for v2 in [false, true] {
            for phase in ["pre", "mid", "post"] {
                progress.case(format!("v2={v2}, phase={phase}"));
                let first = if phase == "mid" {
                    responses::sse_response(responses::sse(vec![
                        responses::ev_response_created("work-before-compaction"),
                        responses::ev_function_call("plan", "update_plan", &json!({"plan":[{"step":"Review authorized error handling", "status":"in_progress"}]}).to_string()),
                        responses::ev_completed_with_tokens("work-before-compaction",500),
                    ]))
                } else {
                    responses::sse_response(responses::sse(vec![
                        responses::ev_assistant_message(
                            "answer-before-compaction",
                            "The authorized review made progress.",
                        ),
                        responses::ev_completed_with_tokens("work-before-compaction", 500),
                    ]))
                };
                progress.phase("create native compaction harness");
                let setup_progress = progress.clone();
                let h = Harness::with_compaction_and_setup_observer(
                    v2,
                    2,
                    vec![
                        first,
                        responses::sse_response(responses::sse_failed(
                            "denied-compaction",
                            "cyber_policy",
                            "Cybersecurity access denied.",
                        )),
                    ],
                    Some(200),
                    None,
                    phase == "post",
                    Some(Arc::new(move |phase| setup_progress.phase(phase))),
                )
                .await;
                if phase == "pre" {
                    progress.phase("submit initial task");
                    h.test
                        .submit_text_turn("Review the authorized error handling.")
                        .await
                        .unwrap_or_else(|err| panic!("initial task: {err:?}"));
                }
                progress.phase("start regular native turn");
                let accepted = h
                    .test
                    .codex
                    .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                        text: "Continue reviewing ordinary error handling.".into(),
                        text_elements: Vec::new(),
                    }]))
                    .await
                    .unwrap_or_else(|err| panic!("regular native turn: {err:?}"));
                let TurnInputSubmission::Started { turn_id, .. } = accepted else {
                    panic!("the regular turn starts: {accepted:?}");
                };
                // Continue goes on once the person's turn runs: turned on at the initial
                // task's idle point, the threads look again (any moved setting wakes them,
                // 9df044002) and continue that point themselves, and this turn steers theirs.
                h.fake.set_policy_key("continue", "true");
                progress.phase("observe continue policy");
                wait_until(|| {
                    h.autopilot
                        .supervisor()
                        .policy()
                        .is_some_and(|p| p.continue_on)
                })
                .await;
                progress.phase("wait for native TurnComplete");
                let done = core_test_support::wait_for_event_with_timeout(
                    &h.test.codex,
                    |event| matches!(event,EventMsg::TurnComplete(done) if done.turn_id == turn_id),
                    HANG_DETECTOR,
                )
                .await;
                let EventMsg::TurnComplete(done) = done else {
                    unreachable!()
                };
                assert_eq!(
                    done.error
                        .as_ref()
                        .and_then(|error| error.codex_error_info.as_ref()),
                    Some(&CodexErrorInfo::CyberPolicy),
                    "v2={v2} phase={phase}: terminal refusal must survive restart"
                );
                if phase == "post" {
                    assert_eq!(
                        done.last_agent_message.as_deref(),
                        Some("The authorized review made progress.")
                    );
                }
                progress.phase("observe stopped attention");
                assert!(
                    h.fake.wait_for(|f| attention_is(f, "stopped:")).await,
                    "v2={v2} phase={phase}: {:?}",
                    h.autopilot.supervisor().journal()
                );
                let rt = h
                    .autopilot
                    .thread_rt(&h.thread)
                    .unwrap_or_else(|| panic!("native runtime"));
                assert!(
                    matches!(
                        rt.state().point.and_then(|point| point.wall),
                        Some(Wall::Fatal { .. })
                    ),
                    "v2={v2} phase={phase}"
                );
                assert!(!rt.timer_pending());
                progress.phase("read durable native history");
                let history = h
                    .test
                    .codex
                    .load_history(false)
                    .await
                    .unwrap_or_else(|err| panic!("durable native history: {err:?}"));
                assert!(history.items.iter().any(|item| matches!(item,codex_history::RolloutItem::EventMsg(EventMsg::ItemStarted(started)) if started.turn_id == turn_id && matches!(started.item,codex_protocol::items::TurnItem::ContextCompaction(_)))) ,"v2={v2} phase={phase}: typed compaction provenance must persist");
                assert!(history.items.iter().any(|item| matches!(item,codex_history::RolloutItem::EventMsg(EventMsg::TurnComplete(done)) if done.turn_id == turn_id && done.error.as_ref().and_then(|error| error.codex_error_info.as_ref()) == Some(&CodexErrorInfo::CyberPolicy))));
                progress.phase("shutdown native Core");
                h.shutdown().await;
                progress.phase("check final request and action invariants");
                assert_eq!(
                    h.requests
                        .lock()
                        .unwrap_or_else(|err| panic!("requests: {err:?}"))
                        .len(),
                    2,
                    "v2={v2} phase={phase}: no denied history replay or automatic sampling follows"
                );
                assert!(
                    h.fake.acts().is_empty(),
                    "v2={v2} phase={phase}: no harness action follows a denied compaction"
                );
                progress.phase("drop native compaction fixture");
            }
        }
    });
}

#[test]
fn compaction_retries_honor_advice_then_clear_it_on_the_next_failure() {
    big_stack(|| async {
        for v2 in [false, true] {
            let h = Harness::new(
                v2,
                1,
                vec![
                    reply("initial", "The task has more work."),
                    transient(true),
                    transient(false),
                ],
            )
            .await;
            h.fake.set_policy_key("retry_api_errors", "false");
            h.test
                .submit_text_turn("Complete this task.")
                .await
                .unwrap_or_else(|err| panic!("person's task: {err:?}"));
            h.test
                .codex
                .submit(Op::Compact)
                .await
                .unwrap_or_else(|err| panic!("manual compact: {err:?}"));
            assert!(
                h.fake.wait_for(|f| attention_is(f, "api:")).await,
                "v2={v2}: {:?}",
                h.autopilot.supervisor().journal()
            );
            let rt = h
                .autopilot
                .thread_rt(&h.thread)
                .unwrap_or_else(|| panic!("thread runtime"));
            h.shutdown().await;
            let errors = h
                .errors
                .0
                .lock()
                .unwrap_or_else(|err| panic!("errors: {err:?}"));
            assert_eq!(errors.len(), 1);
            assert_eq!(errors[0].1, None, "the newest failure replaces old advice");
            let requests = h
                .requests
                .lock()
                .unwrap_or_else(|err| panic!("requests: {err:?}"));
            assert_eq!(requests.len(), 3);
            assert!(
                requests[2].0 >= requests[1].0 + Duration::from_secs(2),
                "v2={v2}: core compaction retry ignored Retry-After"
            );
            assert!(!rt.timer_pending());
            assert!(h.autopilot.thread_rt(&h.thread).is_none());
            assert!(
                h.fake.acts().is_empty(),
                "retry_api_errors=false suppresses native recovery"
            );
        }
    });
}

#[test]
fn pre_turn_compaction_retains_provider_advice_before_any_sampling_error() {
    big_stack(|| async {
        for v2 in [false, true] {
            let initial = responses::sse_response(responses::sse(vec![
                responses::ev_assistant_message("initial-message", "The task has more work."),
                responses::ev_completed_with_tokens("initial", 500),
            ]));
            let h = Harness::with_limit(v2, 0, vec![initial, transient(true)], Some(200)).await;
            h.fake.set_policy_key("retry_api_errors", "false");
            h.test
                .submit_text_turn("Keep this task in context.")
                .await
                .unwrap_or_else(|err| panic!("first person's turn: {err:?}"));
            h.test
                .submit_text_turn("Continue the original task.")
                .await
                .unwrap_or_else(|err| panic!("second person's turn: {err:?}"));
            assert!(
                h.fake.wait_for(|f| attention_is(f, "api:")).await,
                "v2={v2}: {:?}",
                h.autopilot.supervisor().journal()
            );
            h.shutdown().await;
            let errors = h
                .errors
                .0
                .lock()
                .unwrap_or_else(|err| panic!("errors: {err:?}"));
            assert_eq!(errors.len(), 1);
            let retry_at = errors[0]
                .1
                .unwrap_or_else(|| panic!("pre-turn compaction retained advice"));
            let requests = h
                .requests
                .lock()
                .unwrap_or_else(|err| panic!("requests: {err:?}"));
            assert_eq!(
                requests.len(),
                2,
                "no sampling request follows failed compaction"
            );
            assert!(retry_at >= requests[1].0 + Duration::from_secs(2));
            assert!(h.fake.acts().is_empty());
        }
    });
}
