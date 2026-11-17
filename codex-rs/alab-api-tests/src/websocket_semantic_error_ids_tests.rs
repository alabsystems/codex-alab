// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Statusless semantic refusals retain only safe IDs in their original request diagnostic.

use codex_api::ApiError;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::MisalignmentErrorDetails;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

const WAIT: Duration = Duration::from_secs(5);
const CHILD: &str = "ALAB_WS_SEMANTIC_ERROR_IDS_CHILD";
const COMPLETE: &str = "ALAB_WS_SEMANTIC_ERROR_IDS_OK";
const TARGET: &str = "codex_api::endpoint::alab_request_diagnostics";
const PRIVATE_INPUT: &str = "private-semantic-id-model-input";
const PRIVATE_MESSAGE: &str = "private-semantic-id-provider-message";
const PRIVATE_HEADER: &str = "private-semantic-id-arbitrary-header";
const MISALIGNMENT_TYPE: &str = "never-log-private-misalignment-type";
const MISALIGNMENT_EXPLANATION: &str = "never-log-private-misalignment-explanation";
const MISALIGNMENT_STEER: &str = "never-log-private-misalignment-steer";
const MISALIGNMENT_TARGET: &str = " RB/opaque== ";

fn assert_misalignment_details(details: Option<&MisalignmentErrorDetails>, complete: bool) {
    if !complete {
        assert!(details.is_none());
        return;
    }
    let details = details.expect("complete typed misalignment details");
    assert_eq!(details.error_type.as_deref(), Some(MISALIGNMENT_TYPE));
    assert_eq!(
        details.detailed_explanation.as_deref(),
        Some(MISALIGNMENT_EXPLANATION)
    );
    assert_eq!(
        details.steer.as_ref().map(|steer| steer.message.as_str()),
        Some(MISALIGNMENT_STEER)
    );
    assert_eq!(details.review_target.as_deref(), Some(MISALIGNMENT_TARGET));
}

#[derive(Clone, Copy, Debug)]
enum Envelope {
    Error,
    Failed,
}

#[derive(Clone, Copy, Debug)]
enum Case {
    Nested,
    MisalignmentComplete,
    MisalignmentMalformed,
    OuterPrimary,
    OuterAlias,
    InvalidOuter,
    MixedHeaders,
    OuterMalformed,
    NestedMalformed,
    RequestOnly,
    RayOnly,
    OuterRequestNestedRay,
    NestedRequestOuterRay,
    InvalidRequestValidRay,
    ValidRequestInvalidRay,
    PositionalError,
    Invalid,
    Overlong,
    Punctuation(&'static str),
    IgnoredThenWithoutIds,
    IgnoredThenCurrent,
    IgnoredThenCompleted,
}

impl Case {
    fn frame(self, code: &str, envelope: Envelope) -> Value {
        let mut nested = json!({
            "x-request-id":"req_semantic-nested-1", "cf-ray":"ray_semantic-nested-1",
            "retry-after":"7", "authorization":PRIVATE_HEADER, "x-unrelated":PRIVATE_HEADER
        });
        let mut outer = json!({
            "retry-after":"11", "authorization":PRIVATE_HEADER, "x-unrelated":PRIVATE_HEADER
        });
        match self {
            Self::Nested | Self::MisalignmentComplete | Self::MisalignmentMalformed => {}
            Self::OuterPrimary | Self::OuterAlias | Self::NestedMalformed => {
                outer["x-request-id"] = json!("req_semantic-outer-1");
                outer["x-oai-request-id"] = json!("req_semantic-outer-alias-1");
                outer["cf-ray"] = json!("ray_semantic-outer-1");
                if matches!(self, Self::OuterAlias) {
                    outer["x-request-id"] = json!("private invalid outer primary");
                }
                if matches!(self, Self::NestedMalformed) {
                    nested = json!(42);
                }
            }
            Self::InvalidOuter => {
                outer["x-request-id"] = json!("private invalid outer primary");
                outer["x-oai-request-id"] = json!("private.invalid.outer.alias");
                outer["cf-ray"] = json!("private invalid outer ray");
            }
            Self::MixedHeaders => {
                outer["X-ReQuEsT-Id"] = json!("private invalid outer primary");
                outer["Cf-RaY"] = json!("private invalid outer ray");
                nested = json!({
                    "X-OaI-ReQuEsT-Id":"req_semantic-mixed-1", "cF-rAy":"ray_semantic-mixed-1",
                    "ReTrY-AfTeR":"7", "AuThOrIzAtIoN":PRIVATE_HEADER, "X-UnReLaTeD":PRIVATE_HEADER
                });
            }
            Self::OuterMalformed => outer = json!(42),
            Self::RequestOnly => {
                nested.as_object_mut().unwrap().remove("cf-ray");
            }
            Self::RayOnly => {
                nested.as_object_mut().unwrap().remove("x-request-id");
            }
            Self::OuterRequestNestedRay => {
                outer["x-request-id"] = json!("req_semantic-outer-1");
                outer["cf-ray"] = json!("private invalid outer ray");
            }
            Self::NestedRequestOuterRay => {
                outer["x-request-id"] = json!("private invalid outer primary");
                outer["x-oai-request-id"] = json!("private.invalid.outer.alias");
                outer["cf-ray"] = json!("ray_semantic-outer-1");
            }
            Self::InvalidRequestValidRay => {
                nested["x-request-id"] = json!("private invalid request");
                nested["x-oai-request-id"] = json!("private.invalid.alias");
            }
            Self::ValidRequestInvalidRay => {
                nested["cf-ray"] = json!("private invalid ray");
            }
            Self::PositionalError => {
                assert!(matches!(envelope, Envelope::Failed));
                outer["x-request-id"] = json!("req_semantic-outer-1");
                outer["cf-ray"] = json!("ray_semantic-outer-1");
            }
            Self::IgnoredThenWithoutIds => {
                nested.as_object_mut().unwrap().remove("x-request-id");
                nested.as_object_mut().unwrap().remove("cf-ray");
            }
            Self::IgnoredThenCurrent => {
                nested["x-request-id"] = json!("req_semantic-current-B");
                nested["cf-ray"] = json!("ray_semantic-current-B");
            }
            Self::IgnoredThenCompleted => {
                return json!({"type":"response.completed", "response":{"id":"semantic-completed"}});
            }
            Self::Invalid | Self::Overlong | Self::Punctuation(_) => {
                let value = match self {
                    Self::Invalid => "private invalid correlation token".to_string(),
                    Self::Overlong => format!("private-overlong-{}", "r".repeat(128)),
                    Self::Punctuation(separator) => {
                        format!("private{separator}invalid{separator}token")
                    }
                    _ => unreachable!(),
                };
                for name in ["x-request-id", "x-oai-request-id", "cf-ray"] {
                    nested[name] = json!(value);
                    outer[name] = json!(value);
                }
            }
        }
        let mut error = if matches!(self, Self::PositionalError) {
            json!([null, code, PRIVATE_MESSAGE, null, null, null])
        } else {
            json!({"code":code, "message":PRIVATE_MESSAGE, "headers":nested})
        };
        if matches!(
            self,
            Self::MisalignmentComplete | Self::MisalignmentMalformed
        ) {
            assert_eq!(code, "misalignment_policy_violation");
            assert!(matches!(envelope, Envelope::Failed));
            let mut details = json!({
                "error_type":MISALIGNMENT_TYPE,
                "detailed_explanation":MISALIGNMENT_EXPLANATION,
                "steer":{"message":MISALIGNMENT_STEER},
                "review_target":MISALIGNMENT_TARGET
            });
            if matches!(self, Self::MisalignmentMalformed) {
                details["steer"]["message"] = json!(42);
            }
            error["misalignment"] = details;
        }
        match envelope {
            Envelope::Error => json!({
                "type":"error", "headers":outer, "error":error,
                "private-body-field":"private-semantic-extra-body"
            }),
            Envelope::Failed => json!({
                "type":"response.failed", "headers":outer, "response":{"error":error},
                "private-body-field":"private-semantic-extra-body"
            }),
        }
    }

    fn frames(self, code: &str, envelope: Envelope) -> Vec<Value> {
        let current = self.frame(code, envelope);
        if matches!(
            self,
            Self::IgnoredThenWithoutIds | Self::IgnoredThenCurrent | Self::IgnoredThenCompleted
        ) {
            vec![
                json!({
                    "type":"error",
                    "headers":{"x-request-id":"req_ignored-A", "cf-ray":"ray_ignored-A"},
                    "error":{"code":"unknown_error", "message":PRIVATE_MESSAGE,
                        "headers":{"x-request-id":"req_ignored-nested-A", "cf-ray":"ray_ignored-nested-A"}}
                }),
                current,
            ]
        } else {
            vec![current]
        }
    }

    fn ids(self) -> (Option<&'static str>, Option<&'static str>) {
        match self {
            Self::Nested
            | Self::MisalignmentComplete
            | Self::MisalignmentMalformed
            | Self::InvalidOuter
            | Self::OuterMalformed => {
                (Some("req_semantic-nested-1"), Some("ray_semantic-nested-1"))
            }
            Self::OuterPrimary | Self::NestedMalformed | Self::PositionalError => {
                (Some("req_semantic-outer-1"), Some("ray_semantic-outer-1"))
            }
            Self::OuterAlias => (
                Some("req_semantic-outer-alias-1"),
                Some("ray_semantic-outer-1"),
            ),
            Self::MixedHeaders => (Some("req_semantic-mixed-1"), Some("ray_semantic-mixed-1")),
            Self::RequestOnly | Self::ValidRequestInvalidRay => {
                (Some("req_semantic-nested-1"), None)
            }
            Self::RayOnly | Self::InvalidRequestValidRay => (None, Some("ray_semantic-nested-1")),
            Self::OuterRequestNestedRay => {
                (Some("req_semantic-outer-1"), Some("ray_semantic-nested-1"))
            }
            Self::NestedRequestOuterRay => {
                (Some("req_semantic-nested-1"), Some("ray_semantic-outer-1"))
            }
            Self::IgnoredThenCurrent => (
                Some("req_semantic-current-B"),
                Some("ray_semantic-current-B"),
            ),
            Self::Invalid
            | Self::Overlong
            | Self::Punctuation(_)
            | Self::IgnoredThenWithoutIds
            | Self::IgnoredThenCompleted => (None, None),
        }
    }
}

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
}

#[derive(Debug)]
struct Record {
    fields: BTreeMap<String, String>,
    ancestors: Vec<&'static str>,
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Record>>>);

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        cx: Context<'_, S>,
    ) {
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        cx.span(id).unwrap().extensions_mut().insert(fields);
    }

    fn on_event(&self, event: &tracing::Event<'_>, cx: Context<'_, S>) {
        if event.metadata().target() != TARGET {
            return;
        }
        assert_eq!(*event.metadata().level(), tracing::Level::INFO);
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut ancestors = Vec::new();
        for span in cx.event_scope(event).unwrap().from_root() {
            ancestors.push(span.name());
            let extensions = span.extensions();
            for (name, value) in &extensions.get::<Fields>().unwrap().0 {
                fields
                    .0
                    .entry(name.clone())
                    .or_insert_with(|| value.clone());
            }
        }
        self.0.lock().unwrap().push(Record {
            fields: fields.0,
            ancestors,
        });
    }
}

type RequestSubscriber = tracing_subscriber::layer::Layered<Capture, tracing_subscriber::Registry>;

fn request_subscriber_is_current() -> bool {
    tracing::dispatcher::get_default(|dispatch| {
        dispatch.downcast_ref::<RequestSubscriber>().is_some()
    })
}

struct NoAuth;

impl codex_api::AuthProvider for NoAuth {
    fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        Ok(())
    }
}

fn request() -> codex_api::ResponsesApiRequest {
    codex_api::ResponsesApiRequest {
        model: "gpt-semantic-error-ids".into(),
        stream: true,
        service_tier: None,
        input: vec![
            serde_json::from_value(json!({
                "type":"message", "role":"user",
                "content":[{"type":"input_text", "text":PRIVATE_INPUT}]
            }))
            .unwrap(),
        ],
        tools: None,
        tool_choice: "auto".into(),
        parallel_tool_calls: true,
        reasoning: None,
        store: false,
        stream_options: None,
        include: vec![],
        prompt_cache_key: None,
        prompt_cache_options: None,
        text: None,
        client_metadata: None,
        access_programs: None,
    }
}

async fn exchange(case: Case, code: &str, envelope: Envelope) {
    let mut peer = tokio::time::timeout(
        WAIT,
        crate::websocket_fixture::WebsocketFixture::start_frames(case.frames(code, envelope)),
    )
    .await
    .expect("semantic ID fixture startup deadline");
    let client = codex_api::ResponsesWebsocketClient::new(
        codex_api::Provider {
            name: "semantic-error-ids".into(),
            base_url: format!("{}/v1", peer.uri()),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 1,
                base_delay: Duration::ZERO,
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            stream_idle_timeout: WAIT,
        },
        Arc::new(NoAuth),
    );
    let factory = codex_http_client::HttpClientFactory::new(
        codex_http_client::OutboundProxyPolicy::ReqwestDefault,
    );
    let connection = tokio::time::timeout(
        WAIT,
        client.connect(
            &factory,
            http::HeaderMap::new(),
            http::HeaderMap::new(),
            None,
            None,
        ),
    )
    .await
    .expect("semantic ID Upgrade deadline")
    .unwrap();
    let capture = Capture::default();
    let mut stream = {
        let _subscriber =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
        assert!(request_subscriber_is_current());
        let thread =
            tracing::info_span!("semantic_fixture_thread", thread_id = "semantic-id-thread");
        let _thread = thread.enter();
        let turn = tracing::info_span!("semantic_fixture_turn", turn_id = "semantic-id-turn");
        let _turn = turn.enter();
        let stream = tokio::time::timeout(
            WAIT,
            connection.stream_request(
                codex_api::ResponsesWsRequest::ResponseCreate(
                    codex_api::ResponseCreateWsRequest::from(&request()),
                ),
                false,
                None,
            ),
        )
        .await
        .expect("semantic ID request opening deadline")
        .unwrap();
        // The fixture observes its first actual text request; it does not count
        // every transmission or establish a complete wire request inventory.
        let observed = peer.observed_request().await;
        assert_eq!(observed["type"], "response.create");
        assert_eq!(observed["model"], "gpt-semantic-error-ids");
        assert_eq!(observed["input"][0]["content"][0]["text"], PRIVATE_INPUT);
        stream
    };
    assert!(
        !request_subscriber_is_current(),
        "caller subscriber and both span handles exited"
    );
    assert_eq!(
        capture.0.lock().unwrap().len(),
        1,
        "only the prepared shape before release"
    );
    peer.release_failure();
    let outcome = tokio::time::timeout(WAIT, stream.rx_event.recv())
        .await
        .expect("semantic outcome delivery deadline");
    assert!(
        tokio::time::timeout(WAIT, stream.rx_event.recv())
            .await
            .expect("semantic error closes channel")
            .is_none()
    );
    peer.finish().await;
    let completed = matches!(case, Case::IgnoredThenCompleted);
    if completed {
        assert!(
            matches!(outcome, Some(Ok(codex_api::ResponseEvent::Completed { response_id, .. })) if response_id == "semantic-completed")
        );
    } else {
        let Some(Err(error)) = outcome else {
            panic!("expected semantic refusal, got {outcome:?}");
        };
        let audio_per_recording = match code {
            "invalid_audio" => Some(true),
            "audio_too_large" => Some(false),
            _ => None,
        };
        if let Some(expected_per_recording) = audio_per_recording {
            let ApiError::InvalidAudioInput {
                message,
                per_recording,
            } = &error
            else {
                panic!("expected original typed audio rejection, got {error:?}");
            };
            assert_eq!(message, PRIVATE_MESSAGE);
            assert_eq!(*per_recording, expected_per_recording);
        } else {
            match (code, &error) {
                ("inference_hook_denied", ApiError::InferenceHookDenied { message })
                | ("access_program_not_enabled", ApiError::AccessProgramNotEnabled { message })
                | ("cyber_policy", ApiError::CyberPolicy { message })
                | ("bio_policy", ApiError::BioPolicy { message }) => {
                    assert_eq!(message, PRIVATE_MESSAGE);
                }
                ("model_not_found", ApiError::ModelNotFound { message, status })
                | (
                    "content_policy_violation",
                    ApiError::ContentPolicyViolation { message, status },
                ) => {
                    assert_eq!(message, PRIVATE_MESSAGE);
                    assert!(
                        status.is_none(),
                        "statusless refusal retains no HTTP status"
                    );
                }
                (
                    "misalignment_policy_violation",
                    ApiError::MisalignmentPolicyViolation {
                        message,
                        misalignment,
                    },
                ) => {
                    assert_eq!(message, PRIVATE_MESSAGE);
                    assert_misalignment_details(
                        misalignment.as_ref(),
                        matches!(case, Case::MisalignmentComplete),
                    );
                }
                ("context_length_exceeded", ApiError::ContextWindowExceeded)
                | ("insufficient_quota", ApiError::QuotaExceeded)
                | ("usage_not_included", ApiError::UsageNotIncluded)
                | ("budget_reached", ApiError::SessionBudgetExceeded)
                | ("flex_unavailable", ApiError::FlexUnavailable) => {}
                _ => {
                    assert!(matches!((code, &error),
                        ("invalid_request", ApiError::InvalidRequest { message })
                        | ("invalid_prompt", ApiError::InvalidPrompt { message }) if message == PRIVATE_MESSAGE));
                }
            }
        }
        let mapped = codex_api::map_api_error(error);
        if code == "misalignment_policy_violation" {
            let CodexErrorDetails::MisalignmentPolicyViolation {
                message,
                misalignment,
            } = mapped.details()
            else {
                panic!("expected mapped typed misalignment refusal");
            };
            assert_eq!(message, PRIVATE_MESSAGE);
            assert_misalignment_details(
                misalignment.as_ref(),
                matches!(case, Case::MisalignmentComplete),
            );
        }
        if let Some(expected_per_recording) = audio_per_recording {
            let CodexErrorDetails::InvalidAudioInput {
                message,
                per_recording,
            } = mapped.details()
            else {
                panic!("expected mapped typed audio rejection");
            };
            assert_eq!(message, PRIVATE_MESSAGE);
            assert_eq!(*per_recording, expected_per_recording);
        }
        let expected_message = match code {
            "context_length_exceeded" => {
                "Codex ran out of room in the model's context window. Start a new thread or clear earlier history before retrying."
            }
            "insufficient_quota" => "Quota exceeded. Check your plan and billing details.",
            "usage_not_included" => {
                "To use Codex with your ChatGPT plan, upgrade to Plus: https://chatgpt.com/explore/plus."
            }
            "budget_reached" => "budget reached",
            "flex_unavailable" => "Flex capacity unavailable.",
            _ => PRIVATE_MESSAGE,
        };
        assert_eq!(mapped.to_string(), expected_message);
        assert!(
            !mapped.requires_http_fallback(),
            "terminal semantic refusal does not switch transports"
        );
        assert!(
            mapped.retry_delay(1).is_none(),
            "semantic validation refusal is never retried"
        );
        assert!(
            mapped.retry_after().is_none(),
            "wire advice is not validation retry authority"
        );
    }

    let records = capture.0.lock().unwrap();
    assert_eq!(
        records.len(),
        if completed { 1 } else { 2 },
        "shape plus only the selected semantic failure"
    );
    let shape = &records[0];
    assert_eq!(shape.fields["message"], "Responses request shape");
    assert!(shape.fields["local_attempt"].parse::<u64>().unwrap() > 0);
    for record in records.iter() {
        assert_eq!(record.fields["transport"], "\"responses_websocket\"");
        assert_eq!(record.fields["thread_id"], "\"semantic-id-thread\"");
        assert_eq!(record.fields["turn_id"], "\"semantic-id-turn\"");
        assert!(record.ancestors.contains(&"semantic_fixture_thread"));
        assert!(record.ancestors.contains(&"semantic_fixture_turn"));
    }
    if let Some(failed) = records.get(1) {
        assert_eq!(failed.fields["message"], "Responses request failed");
        assert_eq!(
            shape.fields["local_attempt"],
            failed.fields["local_attempt"]
        );
        let failure_kind = match code {
            "invalid_audio" | "audio_too_large" => "invalid_audio_input",
            "context_length_exceeded" => "context_window",
            "insufficient_quota" => "quota_exceeded",
            "budget_reached" => "session_budget_exceeded",
            _ => code,
        };
        assert_eq!(failed.fields["failure_kind"], format!("{failure_kind:?}"));
        assert!(!failed.fields.contains_key("http_status"));
        assert!(!failed.fields.contains_key("retry_advised"));
    }
    for record in records.iter() {
        for field in [
            "code",
            "error_code",
            "error_message",
            "body",
            "payload",
            "headers",
            "misalignment",
            "error_type",
            "detailed_explanation",
            "steer",
            "review_target",
        ] {
            assert!(!record.fields.contains_key(field));
        }
    }
    let text = format!("{records:?}");
    for forbidden in [
        "private",
        "authorization",
        "x-unrelated",
        "retry-after",
        "req_ignored-A",
        "ray_ignored-A",
        "req_ignored-nested-A",
        "ray_ignored-nested-A",
        "RB/opaque==",
    ] {
        assert!(
            !text.contains(forbidden),
            "diagnostics exclude input, provider body and arbitrary headers"
        );
    }
    // Keep correlation checks last: RED must first establish unchanged semantic
    // error, cleanup, attempt/context and privacy, then expose missing safe IDs.
    if let Some(failed) = records.get(1) {
        if matches!(case, Case::Nested) {
            match code {
                "invalid_audio" => println!(
                    "ALAB_WS_AUDIO_ERROR_IDS:code=invalid_audio:case=Nested:stage=late_ids"
                ),
                "audio_too_large" => println!(
                    "ALAB_WS_AUDIO_ERROR_IDS:code=audio_too_large:case=Nested:stage=late_ids"
                ),
                "inference_hook_denied" => println!(
                    "ALAB_WS_TERMINAL_ERROR_IDS:code=inference_hook_denied:case=Nested:stage=late_ids"
                ),
                "context_length_exceeded" => println!(
                    "ALAB_WS_TERMINAL_ERROR_IDS:code=context_length_exceeded:case=Nested:stage=late_ids"
                ),
                _ => {}
            }
        }
        let (request_id, cf_ray) = case.ids();
        let request_id = request_id.map(|value| format!("{value:?}"));
        let cf_ray = cf_ray.map(|value| format!("{value:?}"));
        assert_eq!(failed.fields.get("request_id"), request_id.as_ref());
        assert_eq!(failed.fields.get("cf_ray"), cf_ray.as_ref());
    }
}

async fn isolated(name: &str, cases: &[Case]) {
    isolated_for(name, cases, "invalid_request", Envelope::Error).await;
}

async fn isolated_for(name: &str, cases: &[Case], code: &str, envelope: Envelope) {
    isolated_for_codes(name, cases, &[code], envelope).await;
}

async fn isolated_for_codes(name: &str, cases: &[Case], codes: &[&str], envelope: Envelope) {
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(name));
        assert_eq!(
            std::env::args().skip(1).collect::<Vec<_>>(),
            ["--exact", &exact, "--nocapture", "--test-threads=1"]
        );
        codex_alab_core::activate();
        for &code in codes {
            for &case in cases {
                exchange(case, code, envelope).await;
            }
        }
        std::io::stdout()
            .lock()
            .write_all(format!("{COMPLETE}:{name}\n").as_bytes())
            .unwrap();
        return;
    }
    assert!(!codex_alab_core::active());
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
        .env_clear()
        .env(CHILD, name);
    for key in ["DYLD_LIBRARY_PATH", "LD_LIBRARY_PATH"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let marker = format!("{COMPLETE}:{name}\n");
    crate::test_child_process::run_with_stdout_marker(
        command,
        Duration::from_secs(60),
        Some(&marker),
    )
    .await
    .unwrap_or_else(|failure| panic!("isolated semantic ID child: {failure}"))
    .assert_test_success(&exact);
    assert!(!codex_alab_core::active());
}

#[tokio::test(flavor = "current_thread")]
async fn statusless_semantic_error_ids_survive_caller_context_exit() {
    isolated(
        "statusless_semantic_error_ids_survive_caller_context_exit",
        &[Case::Nested],
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn statusless_semantic_error_ids_keep_outer_precedence_and_aliases() {
    isolated(
        "statusless_semantic_error_ids_keep_outer_precedence_and_aliases",
        &[
            Case::OuterPrimary,
            Case::OuterAlias,
            Case::InvalidOuter,
            Case::MixedHeaders,
            Case::OuterMalformed,
            Case::NestedMalformed,
            Case::RequestOnly,
            Case::RayOnly,
            Case::OuterRequestNestedRay,
            Case::NestedRequestOuterRay,
            Case::InvalidRequestValidRay,
            Case::ValidRequestInvalidRay,
        ],
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn statusless_semantic_error_ids_omit_invalid_metadata() {
    isolated(
        "statusless_semantic_error_ids_omit_invalid_metadata",
        &[
            Case::Invalid,
            Case::Overlong,
            Case::Punctuation("."),
            Case::Punctuation("/"),
            Case::Punctuation(":"),
        ],
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn ignored_error_ids_cannot_attach_to_later_semantic_outcomes() {
    isolated(
        "ignored_error_ids_cannot_attach_to_later_semantic_outcomes",
        &[
            Case::IgnoredThenWithoutIds,
            Case::IgnoredThenCompleted,
            Case::IgnoredThenCurrent,
        ],
    )
    .await;
}

const PROMPT_CASES: &[Case] = &[
    Case::Nested,
    Case::OuterPrimary,
    Case::OuterAlias,
    Case::InvalidOuter,
    Case::MixedHeaders,
    Case::OuterMalformed,
    Case::NestedMalformed,
    Case::RequestOnly,
    Case::RayOnly,
    Case::OuterRequestNestedRay,
    Case::NestedRequestOuterRay,
    Case::InvalidRequestValidRay,
    Case::ValidRequestInvalidRay,
    Case::Invalid,
    Case::Overlong,
    Case::Punctuation("."),
    Case::Punctuation("/"),
    Case::Punctuation(":"),
    Case::IgnoredThenWithoutIds,
    Case::IgnoredThenCompleted,
    Case::IgnoredThenCurrent,
];

#[tokio::test(flavor = "current_thread")]
async fn invalid_prompt_error_ids_survive_selected_semantic_failure() {
    isolated_for(
        "invalid_prompt_error_ids_survive_selected_semantic_failure",
        PROMPT_CASES,
        "invalid_prompt",
        Envelope::Error,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_prompt_failed_response_ids_survive_selected_semantic_failure() {
    isolated_for(
        "invalid_prompt_failed_response_ids_survive_selected_semantic_failure",
        PROMPT_CASES,
        "invalid_prompt",
        Envelope::Failed,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn positional_invalid_request_keeps_outer_error_ids() {
    isolated_for(
        "positional_invalid_request_keeps_outer_error_ids",
        &[Case::PositionalError],
        "invalid_request",
        Envelope::Failed,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn positional_invalid_prompt_keeps_outer_error_ids() {
    isolated_for(
        "positional_invalid_prompt_keeps_outer_error_ids",
        &[Case::PositionalError],
        "invalid_prompt",
        Envelope::Failed,
    )
    .await;
}

// Audio failures reuse only the object-frame matrix; positional controls remain separate.
#[tokio::test(flavor = "current_thread")]
async fn invalid_audio_failed_response_ids_survive_selected_semantic_failure() {
    isolated_for(
        "invalid_audio_failed_response_ids_survive_selected_semantic_failure",
        PROMPT_CASES,
        "invalid_audio",
        Envelope::Failed,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn audio_too_large_failed_response_ids_survive_selected_semantic_failure() {
    isolated_for(
        "audio_too_large_failed_response_ids_survive_selected_semantic_failure",
        PROMPT_CASES,
        "audio_too_large",
        Envelope::Failed,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn misalignment_details_survive_selected_websocket_failure() {
    isolated_for(
        "misalignment_details_survive_selected_websocket_failure",
        &[Case::MisalignmentComplete, Case::MisalignmentMalformed],
        "misalignment_policy_violation",
        Envelope::Failed,
    )
    .await;
}

// Reuse representative frame ownership and independent-token controls for each
// typed terminal category without repeating every malformed-container case.
const TERMINAL_CASES: &[Case] = &[
    Case::Nested,
    Case::IgnoredThenWithoutIds,
    Case::IgnoredThenCurrent,
    Case::IgnoredThenCompleted,
    Case::RequestOnly,
    Case::RayOnly,
    Case::OuterRequestNestedRay,
    Case::NestedRequestOuterRay,
    Case::InvalidRequestValidRay,
    Case::ValidRequestInvalidRay,
];

#[tokio::test(flavor = "current_thread")]
async fn terminal_access_refusals_keep_selected_event_ids() {
    isolated_for_codes(
        "terminal_access_refusals_keep_selected_event_ids",
        TERMINAL_CASES,
        &[
            "inference_hook_denied",
            "access_program_not_enabled",
            "model_not_found",
            "content_policy_violation",
            "cyber_policy",
            "bio_policy",
            "misalignment_policy_violation",
        ],
        Envelope::Failed,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn terminal_limit_refusals_keep_selected_event_ids() {
    isolated_for_codes(
        "terminal_limit_refusals_keep_selected_event_ids",
        TERMINAL_CASES,
        &[
            "context_length_exceeded",
            "insufficient_quota",
            "usage_not_included",
            "budget_reached",
            "flex_unavailable",
        ],
        Envelope::Failed,
    )
    .await;
}
