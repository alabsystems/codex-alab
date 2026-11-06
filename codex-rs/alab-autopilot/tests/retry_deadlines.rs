// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Provider retry advice through real core turns and the claimed native autopilot.

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
use codex_protocol::protocol::EventMsg;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

const MODEL: &str = "gpt-5.5";
const INPUT: &str = "Preserve this task across transient provider failures.";
const HANG_DETECTOR: Duration = Duration::from_secs(60);

#[derive(Default)]
struct ErrorAdvice(Mutex<Vec<Option<Instant>>>);

impl TurnLifecycleContributor for ErrorAdvice {
    fn on_turn_error<'a>(&'a self, input: TurnErrorInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let advice = input
                .turn_store
                .get::<SamplingRetryAdvice>()
                .unwrap_or_else(|| panic!("the sampling hook retained retry advice"));
            self.0
                .lock()
                .unwrap_or_else(|err| panic!("advice lock: {err:?}"))
                .push(advice.current_deadline());
        })
    }
}

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
                .unwrap_or_else(|err| panic!("owned test runtime: {err:?}"))
                .block_on(async {
                    tokio::time::timeout(HANG_DETECTOR, test())
                        .await
                        .unwrap_or_else(|err| panic!("retry test hang detector: {err:?}"));
                });
        })
        .unwrap_or_else(|err| panic!("owned test thread: {err:?}"))
        .join();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn recovery(status: u16) {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch aterm socket: {err:?}"));
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .unwrap_or_else(|err| panic!("fake aterm: {err:?}"));
    fake.set_policy_key("human_grace_s", "0");
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
    let advice = Arc::new(ErrorAdvice::default());
    builder.turn_lifecycle_contributor(advice.clone());

    let server = responses::start_mock_server().await;
    let received = Arc::new(Mutex::new(Vec::<(Instant, Value)>::new()));
    let captured = Arc::clone(&received);
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(move |request: &wiremock::Request| {
            let mut requests = captured
                .lock()
                .unwrap_or_else(|err| panic!("request lock: {err:?}"));
            requests.push((
                Instant::now(),
                request
                    .body_json()
                    .unwrap_or_else(|err| panic!("Responses JSON: {err:?}")),
            ));
            match requests.len() {
                1 => responses::sse_response(responses::sse(vec![
                    responses::ev_response_created("plan-response"),
                    responses::ev_function_call(
                        "completed-plan-call",
                        "update_plan",
                        &json!({"plan": [{
                            "step": "Preserve completed work during provider recovery",
                            "status": "completed"
                        }]})
                        .to_string(),
                    ),
                    responses::ev_completed("plan-response"),
                ])),
                2 => ResponseTemplate::new(status)
                    .insert_header("Retry-After", "2")
                    .set_body_json(json!({"error": {
                        "code": "rate_limit_exceeded",
                        "message": "Transient provider failure."
                    }})),
                3 => ResponseTemplate::new(status).set_body_json(json!({"error": {
                    "code": "rate_limit_exceeded",
                    "message": "A second transient failure has no retry advice."
                }})),
                4 => responses::sse_response(responses::sse(vec![
                    responses::ev_response_created("recovered"),
                    responses::ev_assistant_message(
                        "recovered-message",
                        "The original task recovered.\nATERM-TASK-DONE",
                    ),
                    responses::ev_completed("recovered"),
                ])),
                // FOREVER CRUISE: the recovered done claim gets the defend prompt; its
                // defense is a done claim again, owed the prompt after the back-off.
                5 => responses::sse_response(responses::sse(vec![
                    responses::ev_response_created("defended"),
                    responses::ev_assistant_message(
                        "defended-message",
                        "The defense holds: nothing is left.\nATERM-TASK-DONE",
                    ),
                    responses::ev_completed("defended"),
                ])),
                count => panic!("unexpected provider request {count}"),
            }
        })
        .expect(5)
        .mount(&server)
        .await;
    let test = test_codex()
        .with_extensions(Arc::new(builder.build()))
        .with_model(MODEL)
        .with_config(|config| {
            config.update_plan_enabled = true;
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(0);
        })
        .build(&server)
        .await
        .unwrap_or_else(|err| panic!("real core fixture: {err:?}"));
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
    assert!(
        fake.wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await,
        "the native autopilot owns its proven claim"
    );
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: INPUT.to_owned(),
            text_elements: Vec::new(),
        }]))
        .await
        .unwrap_or_else(|err| panic!("original task: {err:?}"));
    let mut completed_tools = 0;
    loop {
        match test
            .codex
            .next_event()
            .await
            .unwrap_or_else(|err| panic!("core event: {err:?}"))
            .msg
        {
            EventMsg::PlanUpdate(_) => completed_tools += 1,
            EventMsg::TurnComplete(event)
                if event
                    .last_agent_message
                    .as_deref()
                    .is_some_and(|text| text.contains("ATERM-TASK-DONE")) =>
            {
                assert!(event.error.is_none(), "{event:?}");
                break;
            }
            _ => {}
        }
    }
    assert_eq!(
        completed_tools, 1,
        "the completed tool is never re-executed"
    );
    let rt = autopilot
        .thread_rt(&thread)
        .unwrap_or_else(|| panic!("native thread state"));
    // The wait's words go to aterm between its journal line and its timer: the defend prompt
    // is owed once both stand.
    let deadline = Instant::now() + HANG_DETECTOR;
    while !(autopilot
        .supervisor()
        .journal()
        .iter()
        .any(|line| line.contains("for continue-done-check@v1"))
        && rt.timer_pending())
    {
        assert!(
            Instant::now() < deadline,
            "the defense's done claim never waited out the back-off: {:?}",
            autopilot.supervisor().journal()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        rt.owed_timer().map(|owed| owed.rule),
        Some("continue-done-check@v1")
    );
    assert!(!fake.acts().iter().any(|act| act == "finished@v1"));
    assert!(!rt.state().landed);
    test.codex
        .shutdown_and_wait()
        .await
        .unwrap_or_else(|err| panic!("core shutdown acknowledgement: {err:?}"));

    let advice = advice
        .0
        .lock()
        .unwrap_or_else(|err| panic!("advice lock: {err:?}"));
    assert_eq!(advice.len(), 2);
    let retry_at = advice[0].unwrap_or_else(|| panic!("positive provider retry deadline"));
    assert_eq!(
        advice[1], None,
        "the next turn cannot reuse old retry advice"
    );
    let requests = received
        .lock()
        .unwrap_or_else(|err| panic!("request lock: {err:?}"));
    assert_eq!(requests.len(), 5);
    let defended = |request: &Value| {
        request["input"]
            .as_array()
            .unwrap_or_else(|| panic!("Responses input array"))
            .iter()
            .filter(|item| item["role"] == "user")
            .filter_map(|item| item["content"].as_array())
            .flatten()
            .filter_map(|part| part["text"].as_str())
            .any(|text| text.starts_with(codex_alab_autopilot::words::DONE_CHECK))
    };
    assert!(
        !defended(&requests[3].1) && defended(&requests[4].1),
        "the recovered done claim is answered with the defend prompt"
    );
    assert!(retry_at >= requests[1].0 + Duration::from_secs(2));
    assert!(
        requests[2].0 >= retry_at,
        "the native continuation must not bypass the original provider deadline"
    );
    for (_, request) in requests.iter() {
        assert_eq!(request["model"], json!(MODEL));
        let original_messages = request["input"]
            .as_array()
            .unwrap_or_else(|| panic!("Responses input array"))
            .iter()
            .filter(|item| {
                item["role"] == "user"
                    && item["content"]
                        .as_array()
                        .is_some_and(|content| content.iter().any(|part| part["text"] == INPUT))
            })
            .count();
        assert_eq!(
            original_messages, 1,
            "the original task stays in context once"
        );
    }
    for (_, request) in requests.iter().skip(1) {
        let outputs = request["input"]
            .as_array()
            .unwrap_or_else(|| panic!("Responses input array"))
            .iter()
            .filter(|item| {
                item["type"] == "function_call_output" && item["call_id"] == "completed-plan-call"
            })
            .count();
        assert_eq!(
            outputs, 1,
            "completed work stays in the recovered context once"
        );
    }
}

#[test]
fn http_429_advice_survives_zero_ordinary_retries_then_the_task_recovers() {
    big_stack(|| recovery(429));
}

#[test]
fn http_503_advice_survives_zero_ordinary_retries_then_the_task_recovers() {
    big_stack(|| recovery(503));
}
