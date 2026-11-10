// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real sampling requests with steering queued after a parsed provider failure.
//! The fixture owns StepContext's actual preemption token, not a timing proxy.

#![cfg(unix)]

use super::run_sampling_request;
use crate::session::TurnInput;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::session::tests::make_session_and_context_with_auth_config_home_and_rx;
use crate::session::turn_context::TurnContext;
use crate::state::TaskKind;
use crate::tasks::SessionTask;
use crate::tasks::SessionTaskResult;
use crate::turn_diff_tracker::TurnDiffTracker;
use codex_alab_core::capacity::CapacityReason;
use codex_alab_core::capacity::InFlightRetryObserver;
use codex_alab_core::capacity::LiveCapacityWait;
use codex_alab_core::capacity::SamplingRetryAdvice;
use codex_extension_api::ExtensionData;
use codex_features::Feature;
use codex_login::CodexAuth;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookExecutionMode;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::turn_input::TurnInputMode;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::turn_input::TurnInputSubmission;
use codex_protocol::user_input::UserInput;
use core_test_support::hooks::trusted_config_layer_stack;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

const INPUT: &str = "Keep the outcome of the provider request.";
const STEERING: &str = "Preserve this clarification for the next explicit turn.";
const HANG_DETECTOR: Duration = Duration::from_secs(60);

// A core lib test uses this crate's Config; core_test_support's convenience
// function takes the dependency copy's Config, so compose its shared-layer seam.
fn trust_discovered_hooks(config: &mut crate::config::Config) {
    config
        .features
        .enable(Feature::CodexHooks)
        .expect("enable the private hook fixture");
    let listed = codex_hooks::list_hooks(codex_hooks::HooksConfig {
        feature_enabled: true,
        config_layer_stack: Some(config.config_layer_stack.clone()),
        ..codex_hooks::HooksConfig::default()
    });
    assert!(
        !listed.hooks.is_empty(),
        "discover the private post-request hook"
    );
    config.config_layer_stack =
        trusted_config_layer_stack(&config.config_layer_stack, &config.codex_home, listed.hooks);
}

/// Registers the real turn's input queue while the fixture drives its sampling function.
struct HoldingTask;

impl SessionTask for HoldingTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }

    fn span_name(&self) -> &'static str {
        "alab.terminal_steering.holding_task"
    }

    async fn run(
        self: Arc<Self>,
        _session: Arc<Session>,
        _turn: Arc<TurnContext>,
        _input: Vec<TurnInput>,
        cancellation: CancellationToken,
    ) -> SessionTaskResult {
        cancellation.cancelled().await;
        Ok(None)
    }
}

/// Release the fixture's private command hook even if an assertion fails.
struct ReleaseHook(PathBuf);

impl Drop for ReleaseHook {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, "released");
    }
}

async fn wait_for_hook_start(path: &Path) {
    loop {
        if path.try_exists().expect("private hook marker metadata") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn failed(code: &str) -> ResponseTemplate {
    responses::sse_response(responses::sse_failed(
        "refused-response",
        code,
        "The provider refused this request.",
    ))
}

async fn parsed_failure_with_queued_steering(
    first: ResponseTemplate,
    expected: CodexErrorInfo,
    interrupt: bool,
) {
    codex_alab_core::activate();
    let server = responses::start_mock_server().await;
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = Arc::clone(&requests);
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(move |request: &wiremock::Request| {
            let mut seen = captured
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            seen.push(request.body_json().expect("Responses JSON"));
            if seen.len() == 1 {
                first.clone()
            } else {
                // Settle an erroneous retry so its actual request count is inspectable.
                responses::sse_response(responses::sse(vec![
                    responses::ev_response_created("unexpected-retry"),
                    responses::ev_completed("unexpected-retry"),
                ]))
            }
        })
        .mount(&server)
        .await;
    let home = tempfile::tempdir().expect("private Codex home");
    let once = home.path().join("post-once");
    let started = home.path().join("post-started");
    let release = ReleaseHook(home.path().join("post-release"));
    let hook_input = home.path().join("post-input.json");
    let command = format!(
        "if mkdir '{}' 2>/dev/null; then cat > '{}'; touch '{}'; while test ! -e '{}'; do sleep 0.02; done; fi",
        once.display(),
        hook_input.display(),
        started.display(),
        release.0.display(),
    );
    fs::write(
        home.path().join("hooks.json"),
        json!({"hooks":{"PostModelRequest":[{"hooks":[{
            "type":"command", "command":command, "async":false
        }]}]}})
        .to_string(),
    )
    .expect("private synchronous hook configuration");
    let base_url = format!("{}/v1", server.uri());
    let (session, turn, _events) = make_session_and_context_with_auth_config_home_and_rx(
        CodexAuth::from_api_key("private-test-key"),
        Vec::new(),
        home.path(),
        move |config| {
            config.model_provider.base_url = Some(base_url);
            config.model_provider.supports_websockets = false;
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(0);
            config
                .features
                .enable(Feature::InstantInterrupt)
                .expect("InstantInterrupt");
            config
                .features
                .disable(Feature::UnboundedConnectionRetries)
                .expect("bounded retries");
            trust_discovered_hooks(config);
        },
    )
    .await;
    #[expect(
        clippy::print_stderr,
        reason = "private child setup failures retain their last completed stage"
    )]
    {
        eprintln!("terminal steering fixture: Session constructed");
    }
    session.refresh_hooks(Arc::clone(&turn.config)).await;
    assert_eq!(
        session
            .hooks()
            .execution_mode_for_event(HookEventName::PostModelRequest),
        Some(HookExecutionMode::Sync),
        "the local Session factory's empty hook registry must be refreshed"
    );
    session
        .spawn_task(Arc::clone(&turn), Vec::new(), HoldingTask)
        .await;
    let previous_deadline = std::time::Instant::now() + Duration::from_secs(3600);
    turn.extension_data
        .insert(SamplingRetryAdvice::new(Some(previous_deadline)));
    let observations = Arc::new(Mutex::new(Vec::new()));
    let captured_observations = Arc::clone(&observations);
    let observer = InFlightRetryObserver::new(move |deadline| {
        captured_observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(deadline);
    });
    observer.observe(Some(previous_deadline));
    turn.extension_data.insert(observer);
    let step = StepContext::for_test(Arc::clone(&turn));
    assert_eq!(
        crate::hook_runtime::hooks_for_model_request(&session, &step)
            .execution_mode_for_event(HookEventName::PostModelRequest),
        Some(HookExecutionMode::Sync),
        "the actual sampling step must retain the private synchronous hook"
    );
    #[expect(
        clippy::print_stderr,
        reason = "private child setup failures retain their last completed stage"
    )]
    {
        eprintln!("terminal steering fixture: active task and sampling hook ready");
    }
    let preempt = step
        .preempt
        .as_ref()
        .expect("the real InstantInterrupt token")
        .clone();
    let cancellation = CancellationToken::new();
    let store = Arc::new(ExtensionData::new(turn.sub_id.clone()));
    let diff = Arc::new(tokio::sync::Mutex::new(TurnDiffTracker::new()));
    let mut client = session.services.model_client.new_session();
    let mut input =
        crate::context_manager::ContextManager::new(Arc::clone(&session.item_id_generator));
    input.replace(vec![ResponseItem::Message {
        id: None,
        status: None,
        role: "user".into(),
        content: vec![ContentItem::InputText { text: INPUT.into() }],
        encrypted_content: None,
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }]);
    let mut sampling = Box::pin(run_sampling_request(
        Arc::clone(&session),
        step,
        store,
        diff,
        &mut client,
        input,
        cancellation.clone(),
    ));
    tokio::select! {
        result = &mut sampling => {
            let outcome = match result {
                Err(error) => format!("{error:?}"),
                Ok((output, _)) => format!("successful needs_follow_up={}", output.needs_follow_up),
            };
            panic!(
                "sampling returned before its held post-request hook: {outcome}; requests={}",
                requests.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len()
            );
        }
        _ = wait_for_hook_start(&started) => {}
    }
    let hook: Value = serde_json::from_slice(&fs::read(hook_input).expect("actual hook input"))
        .expect("hook JSON");
    assert_eq!(hook["status"], "failed", "{hook}");
    #[expect(
        clippy::print_stderr,
        reason = "the private child records its positive parsed-failure hook witness"
    )]
    {
        eprintln!("terminal steering fixture: actual failed PostModelRequest is held");
    }
    let submitted = crate::session::turn_input::handle(
        &session,
        TurnInputRequest::user_input(vec![UserInput::Text {
            text: STEERING.into(),
            text_elements: Vec::new(),
        }]),
        TurnInputMode::Steer {
            expected_turn_id: turn.sub_id.clone(),
        },
        "queued-after-failure".into(),
    )
    .await
    .expect("real steering admission");
    assert!(
        matches!(submitted, TurnInputSubmission::Steered { turn_id, .. } if turn_id == turn.sub_id)
    );
    tokio::select! {
        _ = &mut sampling => panic!("sampling returned before its post-request hook released"),
        _ = preempt.cancelled() => {}
    }
    assert!(
        preempt.is_cancelled(),
        "positively witnessed real input-watch preemption"
    );
    if interrupt {
        cancellation.cancel();
    }
    drop(release);
    let result = sampling.await;
    session.abort_all_tasks(TurnAbortReason::Interrupted).await;
    assert_eq!(
        requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len(),
        1
    );
    let error = match result {
        Err(error) => error,
        Ok((output, _)) => panic!(
            "a parsed failure became successful steering follow-up (needs_follow_up={})",
            output.needs_follow_up
        ),
    };
    if interrupt {
        assert!(matches!(error.details(), CodexErrorDetails::TurnAborted));
    } else {
        assert_eq!(error.to_codex_protocol_error(), expected);
        let deadline = error
            .retry_after()
            .map(|advice| advice.deadline().into_std());
        if matches!(
            expected,
            CodexErrorInfo::HttpConnectionFailed {
                http_status_code: Some(401 | 403)
            }
        ) {
            assert!(
                deadline.is_some(),
                "the fixture supplies provider Retry-After advice"
            );
        } else {
            assert!(
                deadline.is_none(),
                "headerless terminal advice must clear the earlier deadline"
            );
        }
        assert_ne!(deadline, Some(previous_deadline));
        assert_eq!(
            turn.extension_data
                .get::<SamplingRetryAdvice>()
                .expect("the real retry handler remembers the received failure")
                .current_deadline(),
            deadline,
            "terminal handling replaces or clears the earlier response's advice"
        );
        assert_eq!(
            observations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .last()
                .copied(),
            Some(deadline),
            "the real retry observer sees the same original absolute advice"
        );
    }
}

async fn bounded(test: impl std::future::Future<Output = ()>) {
    tokio::time::timeout(HANG_DETECTOR, test)
        .await
        .expect("terminal steering hang detector");
}

async fn typed_capacity_sampling(code: &str, steer: bool) {
    codex_alab_core::activate();
    let reason = CapacityReason::from_code(code).expect("exact fixture capacity code");
    let server = responses::start_mock_server().await;
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = Arc::clone(&requests);
    let code = code.to_owned();
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(move |request: &wiremock::Request| {
            let mut seen = captured
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            seen.push(json!({
                "body":request.body_json::<Value>().expect("actual Responses request"),
                "url":request.url.as_str(),
                "auth":request.headers.get("authorization").map(|value| value.as_bytes().to_vec()),
            }));
            if seen.len() == 1 {
                ResponseTemplate::new(503)
                    .insert_header("Retry-After", if steer { "3600" } else { "0" })
                    .set_body_json(json!({"error":{"code":code}}))
            } else {
                responses::sse_response(responses::sse(vec![
                    responses::ev_response_created("capacity-recovered"),
                    responses::ev_completed("capacity-recovered"),
                ]))
            }
        })
        .mount(&server)
        .await;
    let home = tempfile::tempdir().expect("owned Codex home");
    let base_url = format!("{}/v1", server.uri());
    let (session, turn, events) = make_session_and_context_with_auth_config_home_and_rx(
        CodexAuth::from_api_key("private-test-key"),
        Vec::new(),
        home.path(),
        move |config| {
            config.model_provider.base_url = Some(base_url);
            config.model_provider.supports_websockets = false;
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(0);
            config
                .features
                .enable(Feature::InstantInterrupt)
                .expect("steering watch");
        },
    )
    .await;
    session
        .spawn_task(Arc::clone(&turn), Vec::new(), HoldingTask)
        .await;
    let input: Vec<ResponseItem> = serde_json::from_value(json!([
        {"type":"message","role":"user","content":[{"type":"input_text","text":INPUT}]},
        {"type":"function_call","name":"completed_tool","call_id":"completed-call","arguments":"{}"},
        {"type":"function_call_output","call_id":"completed-call","output":"completed-work-witness"},
    ])).expect("retained user input and completed tool output");
    session
        .record_conversation_items(&turn, turn.model_info(), &input)
        .await;
    let step = StepContext::for_test(Arc::clone(&turn));
    let model = step.settings.model_info.slug.clone();
    let input = session.clone_history().await;
    let preempt = step
        .preempt
        .as_ref()
        .expect("real steering watch token")
        .clone();
    let cancellation = CancellationToken::new();
    let mut client = session.services.model_client.new_session();
    let mut sampling = Box::pin(run_sampling_request(
        Arc::clone(&session),
        step,
        Arc::new(ExtensionData::new(turn.sub_id.clone())),
        Arc::new(tokio::sync::Mutex::new(TurnDiffTracker::new())),
        &mut client,
        input,
        cancellation.clone(),
    ));
    if steer {
        loop {
            tokio::select! {
                result = &mut sampling => panic!("capacity ended before its long wait: {}", result.is_ok()),
                event = events.recv() => {
                    if let EventMsg::StreamError(error) = event.expect("capacity notice event").msg {
                        let expected = match reason {
                            CapacityReason::ServerOverloaded => CodexErrorInfo::ServerOverloaded,
                            CapacityReason::PolicyCapacityUnavailable => CodexErrorInfo::PolicyCapacityUnavailable,
                        };
                        assert_eq!(error.codex_error_info, Some(expected));
                        break;
                    }
                }
            }
        }
        let deadline = turn
            .extension_data
            .get::<SamplingRetryAdvice>()
            .expect("provider advice")
            .current_deadline()
            .expect("provider supplied Retry-After");
        let live = turn
            .extension_data
            .get::<LiveCapacityWait>()
            .expect("actual live capacity wait");
        assert_eq!(
            live.publish_reason_if_current(std::time::Instant::now, |reason| reason),
            Some(reason)
        );
        let submission = crate::session::turn_input::handle(
            &session,
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: STEERING.into(),
                text_elements: Vec::new(),
            }]),
            TurnInputMode::Steer {
                expected_turn_id: turn.sub_id.clone(),
            },
            "capacity-steering".into(),
        )
        .await
        .expect("real same-turn steering admission");
        assert!(matches!(submission, TurnInputSubmission::Steered { .. }));
        tokio::select! {
            result = &mut sampling => panic!("steering shortened the capacity wait: {}", result.is_ok()),
            _ = preempt.cancelled() => {}
        }
        assert!(futures::poll!(&mut sampling).is_pending());
        assert_eq!(
            turn.extension_data
                .get::<SamplingRetryAdvice>()
                .unwrap()
                .current_deadline(),
            Some(deadline)
        );
        cancellation.cancel();
        let error = match sampling.await {
            Err(error) => error,
            Ok(_) => panic!("interrupt must cancel the actual sampling wait"),
        };
        assert!(matches!(error.details(), CodexErrorDetails::TurnAborted));
        assert!(
            live.publish_reason_if_current(std::time::Instant::now, |reason| reason)
                .is_none()
        );
    } else {
        let (output, _) = sampling
            .await
            .expect("typed capacity retries despite ordinary budget zero");
        assert!(!output.needs_follow_up);
    }
    session.abort_all_tasks(TurnAbortReason::Interrupted).await;
    let seen = requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(seen.len(), if steer { 1 } else { 2 });
    assert_eq!(seen[0]["body"]["model"].as_str(), Some(model.as_str()));
    assert_eq!(seen[0]["auth"], json!(b"Bearer private-test-key".to_vec()));
    assert!(
        seen[0]["body"]["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["type"] == "message"
                    && item["role"] == "user"
                    && item["content"]
                        .as_array()
                        .is_some_and(|parts| parts.iter().any(|part| part["text"] == INPUT))
            })
    );
    assert!(
        seen[0]["body"]["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["type"] == "function_call_output"
                && item["call_id"] == "completed-call"
                && item["output"] == "completed-work-witness")
    );
    if !steer {
        assert_eq!(seen[0]["body"]["input"], seen[1]["body"]["input"]);
        assert_eq!(seen[0]["body"]["model"], seen[1]["body"]["model"]);
        assert_eq!(seen[0]["url"], seen[1]["url"]);
        assert_eq!(seen[0]["auth"], seen[1]["auth"]);
    }
}

const CHILD_MARKER: &str = "ALAB_TERMINAL_STEERING_CHILD";
const COMPLETE: &str = "ALAB_TERMINAL_STEERING_OK";

fn child_name() -> String {
    let (_, module) = module_path!().split_once("::").expect("test crate prefix");
    format!("{module}::active_terminal_steering_child")
}

fn isolated(case: &str) -> std::io::Result<()> {
    assert!(!codex_alab_core::active());
    let private = tempfile::Builder::new()
        .prefix("alab-terminal-steering-")
        .tempdir()?;
    let home = private.path().join("home");
    let codex_home = private.path().join("codex");
    let tmp = private.path().join("tmp");
    for directory in [&home, &codex_home, &tmp] {
        fs::create_dir_all(directory)?;
    }
    let path = std::env::var_os("PATH")
        .ok_or_else(|| std::io::Error::other("the contained test must supply PATH"))?;
    let name = child_name();
    let mut child = Command::new(std::env::current_exe()?);
    child
        .args(["--exact", &name, "--nocapture", "--test-threads=1"])
        .env_clear()
        .env(CHILD_MARKER, case)
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("TMPDIR", &tmp)
        .env("PATH", path)
        .env("RUST_MIN_STACK", "16777216")
        .stdin(Stdio::null());
    for key in [
        "DYLD_LIBRARY_PATH",
        "LD_LIBRARY_PATH",
        "CARGO_BIN_EXE_test_stdio_server",
        "CARGO_BIN_EXE_codex-code-mode-host",
        "CARGO_BIN_EXE_codex_code_mode_host",
    ] {
        if let Some(value) = std::env::var_os(key) {
            child.env(key, value);
        }
    }
    let output = child.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "isolated {case}: {stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains(&format!("{COMPLETE}:{case}")), "{stdout}");
    assert!(stdout.contains("1 passed; 0 failed"), "{stdout}");
    assert!(!codex_alab_core::active());
    Ok(())
}

#[test]
fn steering_cannot_erase_a_received_cyber_policy_refusal() -> std::io::Result<()> {
    isolated("cyber")
}

#[test]
fn steering_cannot_erase_a_received_invalid_prompt() -> std::io::Result<()> {
    isolated("invalid")
}

#[test]
fn steering_cannot_erase_a_received_quota_refusal() -> std::io::Result<()> {
    isolated("quota")
}

#[test]
fn steering_cannot_replay_an_overload_looking_permanent_refusal() -> std::io::Result<()> {
    isolated("permanent")
}

#[test]
fn steering_cannot_replay_a_policy_capacity_body_at_unauthorized_or_forbidden_status()
-> std::io::Result<()> {
    isolated("policy-401")?;
    isolated("policy-403")
}

#[test]
fn steering_cannot_hide_an_exhausted_ordinary_retry_budget() -> std::io::Result<()> {
    isolated("exhausted")
}

#[test]
fn an_actual_interrupt_keeps_priority_over_a_received_refusal() -> std::io::Result<()> {
    isolated("interrupt")
}

#[test]
fn capacity_retry_preserves_input_model_and_provider_for_both_typed_causes() -> std::io::Result<()>
{
    isolated("capacity-model-retry")?;
    isolated("capacity-policy-retry")
}

#[test]
fn both_capacity_causes_keep_steering_waits_interruptible() -> std::io::Result<()> {
    isolated("capacity-model-steer")?;
    isolated("capacity-policy-steer")
}

#[test]
fn active_terminal_steering_child() {
    let Ok(case) = std::env::var(CHILD_MARKER) else {
        return;
    };
    let name = child_name();
    assert_eq!(
        std::env::args().skip(1).collect::<Vec<_>>(),
        ["--exact", &name, "--nocapture", "--test-threads=1"]
    );
    if matches!(
        case.as_str(),
        "capacity-model-retry"
            | "capacity-policy-retry"
            | "capacity-model-steer"
            | "capacity-policy-steer"
    ) {
        let code = if case.contains("-policy-") {
            "policy_capacity_unavailable"
        } else {
            "server_is_overloaded"
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("owned capacity sampling runtime")
            .block_on(bounded(typed_capacity_sampling(
                code,
                case.ends_with("-steer"),
            )));
        #[expect(
            clippy::print_stdout,
            reason = "parent requires the private child completion protocol"
        )]
        {
            println!("{COMPLETE}:{case}");
        }
        return;
    }
    let (response, expected, interrupt) = match case.as_str() {
        "cyber" => (failed("cyber_policy"), CodexErrorInfo::CyberPolicy, false),
        "invalid" => (
            failed("invalid_prompt"),
            CodexErrorInfo::InvalidPrompt,
            false,
        ),
        "quota" => (
            failed("insufficient_quota"),
            CodexErrorInfo::UsageLimitExceeded,
            false,
        ),
        "permanent" => (
            ResponseTemplate::new(403)
                .insert_header("Retry-After", "120")
                .set_body_json(json!({"error":{
                    "code":"server_is_overloaded", "message":"Model is at capacity."
                }})),
            CodexErrorInfo::HttpConnectionFailed {
                http_status_code: Some(403),
            },
            false,
        ),
        "policy-401" | "policy-403" => {
            let status = if case == "policy-401" { 401 } else { 403 };
            (
                ResponseTemplate::new(status)
                    .insert_header("Retry-After", "120")
                    .set_body_json(json!({"error":{"code":"policy_capacity_unavailable"}})),
                CodexErrorInfo::HttpConnectionFailed {
                    http_status_code: Some(status),
                },
                false,
            )
        }
        "exhausted" => (
            failed("unknown_server_failure"),
            CodexErrorInfo::Other,
            false,
        ),
        "interrupt" => (failed("cyber_policy"), CodexErrorInfo::CyberPolicy, true),
        other => panic!("unknown child case {other}"),
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("owned fixture runtime")
        .block_on(bounded(parsed_failure_with_queued_steering(
            response, expected, interrupt,
        )));
    #[expect(
        clippy::print_stdout,
        reason = "the isolated parent requires this exact child completion protocol marker"
    )]
    {
        println!("{COMPLETE}:{case}");
    }
}
