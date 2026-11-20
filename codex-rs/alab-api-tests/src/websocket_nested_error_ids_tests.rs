// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Nested error correlation is diagnostic metadata, not a new retry classifier.

use codex_api::ApiError;
use codex_api::TransportError;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::Instant;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

const WAIT: Duration = Duration::from_secs(5);
const CHILD_WAIT: Duration = Duration::from_secs(60);
const CHILD: &str = "ALAB_WS_NESTED_ERROR_IDS_CHILD";
const TARGET: &str = "codex_api::endpoint::alab_request_diagnostics";
const PRIVATE_INPUT: &str = "private-nested-id-model-input";
const PRIVATE_BODY: &str = "private-nested-id-provider-message";
const PRIVATE_HEADER: &str = "private-nested-id-arbitrary-header";
const PRIVATE_OUTER_HEADER: &str = "private-nested-id-original-outer-header";
const PRIVATE_REQUEST_ID: &str = "private nested request id with spaces";
const PRIVATE_CF_RAY: &str = "private nested ray with spaces";
const PRIVATE_OUTER_REQUEST_ID: &str = "private invalid outer request id";

#[derive(Clone, Copy, Debug)]
enum Case {
    NestedOnly,
    OuterWins,
    InvalidNested,
    OuterAliasWins,
    MixedCaseNestedAlias,
    InvalidNestedTooLong,
    InvalidNestedPattern(&'static str),
}

impl Case {
    fn frame(self) -> Value {
        let (request_id, cf_ray) = match self {
            Self::NestedOnly | Self::OuterWins | Self::OuterAliasWins => {
                ("req_nested-error-1".into(), "ray_nested-error-1".into())
            }
            Self::MixedCaseNestedAlias => {
                ("req_nested-alias-1".into(), "ray_nested-alias-1".into())
            }
            Self::InvalidNested => (PRIVATE_REQUEST_ID.into(), PRIVATE_CF_RAY.into()),
            Self::InvalidNestedTooLong => (
                format!("private-overlong-request-{}", "r".repeat(128)),
                format!("private-overlong-ray-{}", "r".repeat(128)),
            ),
            Self::InvalidNestedPattern(separator) => (
                format!("private{separator}nested{separator}request"),
                format!("private{separator}nested{separator}ray"),
            ),
        };
        let mut frame = json!({
            "type":"error", "status":400,
            "error":{
                "type":"invalid_request_error", "code":"invalid_prompt",
                "message":PRIVATE_BODY,
                "headers":{
                    "x-request-id":request_id, "cf-ray":cf_ray, "retry-after":"7",
                    "authorization":format!("Bearer {PRIVATE_HEADER}"),
                    "x-unrelated":PRIVATE_HEADER
                }
            },
            "private-body-field":"private-nested-id-extra-body"
        });
        if matches!(self, Self::MixedCaseNestedAlias) {
            frame["error"]["headers"] = json!({
                "X-OaI-ReQuEsT-Id":request_id, "Cf-RaY":cf_ray, "ReTrY-AfTeR":"7",
                "AuThOrIzAtIoN":format!("Bearer {PRIVATE_HEADER}"),
                "X-UnReLaTeD":PRIVATE_HEADER
            });
        }
        if matches!(self, Self::OuterWins | Self::OuterAliasWins) {
            frame["headers"] = json!({
                "x-request-id":"req_outer-error-1", "cf-ray":"ray_outer-error-1",
                "retry-after":"11", "x-unrelated":PRIVATE_OUTER_HEADER,
                "authorization":format!("Bearer {PRIVATE_OUTER_HEADER}")
            });
        }
        if matches!(self, Self::OuterAliasWins) {
            frame["headers"]["x-request-id"] = json!(PRIVATE_OUTER_REQUEST_ID);
            frame["headers"]["x-oai-request-id"] = json!("req_outer-alias-1");
        }
        frame
    }

    fn ids(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::NestedOnly => Some(("req_nested-error-1", "ray_nested-error-1")),
            Self::OuterWins => Some(("req_outer-error-1", "ray_outer-error-1")),
            Self::OuterAliasWins => Some(("req_outer-alias-1", "ray_outer-error-1")),
            Self::MixedCaseNestedAlias => Some(("req_nested-alias-1", "ray_nested-alias-1")),
            Self::InvalidNested | Self::InvalidNestedTooLong | Self::InvalidNestedPattern(_) => {
                None
            }
        }
    }
}

#[derive(Default)]
struct Fields(BTreeMap<String, Value>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().into(), format!("{value:?}").into());
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0.insert(field.name().into(), value.into());
    }
}

#[derive(Debug)]
struct Captured {
    level: tracing::Level,
    ancestors: Vec<&'static str>,
    fields: BTreeMap<String, Value>,
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Captured>>>);

impl<S> Layer<S> for Capture
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attributes: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        context: Context<'_, S>,
    ) {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        context.span(id).unwrap().extensions_mut().insert(fields);
    }

    fn on_event(&self, event: &tracing::Event<'_>, context: Context<'_, S>) {
        if event.metadata().target() != TARGET {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut ancestors = Vec::new();
        if let Some(scope) = context.event_scope(event) {
            for span in scope.from_root() {
                ancestors.push(span.name());
                if let Some(parent) = span.extensions().get::<Fields>() {
                    for (name, value) in &parent.0 {
                        fields
                            .0
                            .entry(name.clone())
                            .or_insert_with(|| value.clone());
                    }
                }
            }
        }
        self.0.lock().unwrap().push(Captured {
            level: *event.metadata().level(),
            ancestors,
            fields: fields.0,
        });
    }
}

struct NoAuth;

impl codex_api::AuthProvider for NoAuth {
    fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        Ok(())
    }
}

async fn regression(case: Case) {
    codex_alab_core::activate();
    let frame = case.frame();
    let body = frame.to_string();
    let outer_headers = frame
        .get("headers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut peer = tokio::time::timeout(
        WAIT,
        crate::websocket_fixture::WebsocketFixture::start(frame),
    )
    .await
    .expect("nested error peer startup deadline");
    let client = codex_api::ResponsesWebsocketClient::new(
        codex_api::Provider {
            name: "nested-error-ids".into(),
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
    .expect("public nested error Upgrade deadline")
    .unwrap();
    let request = crate::streaming_request("gpt-nested-error-ids", PRIVATE_INPUT);
    let capture = Capture::default();
    let mut stream = {
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        let _subscriber = tracing::subscriber::set_default(subscriber);
        let thread =
            tracing::info_span!("fixture_nested_thread", thread_id = "nested-error-thread");
        let _thread = thread.enter();
        let turn = tracing::info_span!("fixture_nested_turn", turn_id = "nested-error-turn");
        let _turn = turn.enter();
        let stream = tokio::time::timeout(
            WAIT,
            connection.stream_request(
                codex_api::ResponsesWsRequest::ResponseCreate(
                    codex_api::ResponseCreateWsRequest::from(&request),
                ),
                false,
                None,
            ),
        )
        .await
        .expect("public nested model request deadline")
        .unwrap();
        let observed = peer.observed_request().await;
        assert_eq!(observed["type"], "response.create");
        assert_eq!(observed["model"], "gpt-nested-error-ids");
        assert_eq!(observed["input"][0]["content"][0]["text"], PRIVATE_INPUT);
        stream
    };
    assert_eq!(
        capture.0.lock().unwrap().len(),
        1,
        "one shape before gated error release"
    );
    let before = Instant::now();
    peer.release_failure();
    let error = match tokio::time::timeout(WAIT, stream.rx_event.recv())
        .await
        .expect("selected nested HTTP error delivery deadline")
    {
        Some(Err(error)) => error,
        event => panic!("expected the peer's structured HTTP error: {event:?}"),
    };
    let after = Instant::now();
    assert!(
        tokio::time::timeout(WAIT, stream.rx_event.recv())
            .await
            .expect("nested error task closes its event channel")
            .is_none()
    );
    peer.finish().await;
    let (advice, returned_headers) = match &error {
        ApiError::Transport(TransportError::Http {
            status,
            headers,
            body: actual_body,
            retry_after,
            ..
        }) => {
            assert_eq!(status.as_u16(), 400);
            assert_eq!(actual_body.as_deref(), Some(body.as_str()));
            let advice = retry_after.expect("nested provider advice remains available");
            assert!(advice.deadline() >= before + Duration::from_secs(7));
            assert!(advice.deadline() <= after + Duration::from_secs(7));
            (advice, headers.clone().unwrap_or_default())
        }
        other => panic!("nested error preserves its HTTP API class: {other:?}"),
    };
    for (name, value) in &outer_headers {
        assert_eq!(
            returned_headers
                .get(name)
                .and_then(|value| value.to_str().ok()),
            value.as_str(),
            "every original outer value, including an invalid primary ID, survives"
        );
    }
    for name in ["authorization", "x-unrelated", "retry-after"] {
        assert_eq!(
            returned_headers
                .get(name)
                .and_then(|value| value.to_str().ok()),
            outer_headers.get(name).and_then(Value::as_str),
            "nested metadata must not add or replace non-correlation headers"
        );
    }
    let mapped = codex_api::map_api_error(error);
    let expected = codex_api::map_api_error(ApiError::InvalidPrompt {
        message: PRIVATE_BODY.into(),
    });
    assert_eq!(
        mapped.to_codex_protocol_error(),
        expected.to_codex_protocol_error()
    );
    assert_eq!(mapped.to_string(), PRIVATE_BODY);
    assert_eq!(
        mapped.retry_after(),
        Some(advice),
        "mapping cannot recapture advice"
    );
    assert!(
        mapped.retry_delay(1).is_none(),
        "a structured invalid prompt remains terminal"
    );

    let records = capture.0.lock().unwrap();
    assert_eq!(
        records.len(),
        2,
        "one real shape and one selected HTTP failure"
    );
    let shape = &records[0];
    let failed = &records[1];
    assert_eq!(shape.fields["message"], "Responses request shape");
    assert_eq!(failed.fields["message"], "Responses request failed");
    assert!(
        shape.fields["local_attempt"]
            .as_u64()
            .is_some_and(|id| id > 0)
    );
    assert_eq!(
        shape.fields["local_attempt"],
        failed.fields["local_attempt"]
    );
    for record in [shape, failed] {
        assert_eq!(record.level, tracing::Level::INFO);
        assert_eq!(record.fields["thread_id"], "nested-error-thread");
        assert_eq!(record.fields["turn_id"], "nested-error-turn");
        assert_eq!(record.fields["transport"], "responses_websocket");
        assert!(record.ancestors.contains(&"fixture_nested_thread"));
        assert!(record.ancestors.contains(&"fixture_nested_turn"));
    }
    assert_eq!(failed.fields["failure_kind"], "http");
    assert_eq!(failed.fields["http_status"], 400);
    assert_eq!(failed.fields["opaque_bad_request"], false);
    let text = format!("{records:?}");
    for private in [
        PRIVATE_INPUT,
        PRIVATE_BODY,
        PRIVATE_HEADER,
        PRIVATE_OUTER_HEADER,
        PRIVATE_REQUEST_ID,
        PRIVATE_CF_RAY,
        PRIVATE_OUTER_REQUEST_ID,
        "private-nested-id-extra-body",
        "private-overlong",
        "private.nested",
        "private/nested",
        "private:nested",
    ] {
        assert!(
            !text.contains(private),
            "selected diagnostic contains private input/body/header metadata"
        );
    }
    // Keep positive ID enrichment checks last: the old mapper must first pass
    // all original error/retry/header/privacy controls, then expose the lost IDs.
    if let Some((request_id, cf_ray)) = case.ids() {
        assert_eq!(failed.fields.get("request_id"), Some(&json!(request_id)));
        assert_eq!(failed.fields.get("cf_ray"), Some(&json!(cf_ray)));
        assert!(
            ["x-request-id", "x-oai-request-id"]
                .into_iter()
                .any(|name| {
                    returned_headers
                        .get(name)
                        .is_some_and(|value| value == request_id)
                }),
            "safe selected request correlation is retained on the HTTP error"
        );
        assert_eq!(returned_headers.get("cf-ray").unwrap(), cf_ray);
    } else {
        assert!(!failed.fields.contains_key("request_id"));
        assert!(!failed.fields.contains_key("cf_ray"));
        for name in ["x-request-id", "x-oai-request-id", "cf-ray"] {
            assert!(!returned_headers.contains_key(name));
        }
    }
}

async fn isolated(name: &str, cases: &[Case]) {
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(name));
        for &case in cases {
            regression(case).await;
        }
        return;
    }
    assert!(!codex_alab_core::active());
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &exact, "--nocapture"])
        .env(CHILD, name);
    for key in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
        "CODEX_CA_CERTIFICATE",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ] {
        command.env_remove(key);
    }
    let report = crate::test_child_process::run(command, CHILD_WAIT)
        .await
        .unwrap_or_else(|failure| panic!("isolated websocket child: {failure}"));
    report.assert_test_success(&exact);
    assert!(!codex_alab_core::active());
}

#[tokio::test(flavor = "current_thread")]
async fn nested_error_headers_keep_selected_failure_correlation() {
    isolated(
        "nested_error_headers_keep_selected_failure_correlation",
        &[Case::NestedOnly],
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn outer_error_headers_keep_existing_correlation_precedence() {
    isolated(
        "outer_error_headers_keep_existing_correlation_precedence",
        &[Case::OuterWins],
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_nested_error_ids_remain_private() {
    isolated(
        "invalid_nested_error_ids_remain_private",
        &[Case::InvalidNested],
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn outer_error_alias_beats_nested_ids_with_invalid_outer_primary() {
    isolated(
        "outer_error_alias_beats_nested_ids_with_invalid_outer_primary",
        &[Case::OuterAliasWins],
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn mixed_case_nested_error_alias_keeps_selected_correlation() {
    isolated(
        "mixed_case_nested_error_alias_keeps_selected_correlation",
        &[Case::MixedCaseNestedAlias],
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn nested_error_ids_reject_overlong_and_content_like_values() {
    isolated(
        "nested_error_ids_reject_overlong_and_content_like_values",
        &[
            Case::InvalidNestedTooLong,
            Case::InvalidNestedPattern("."),
            Case::InvalidNestedPattern("/"),
            Case::InvalidNestedPattern(":"),
        ],
    )
    .await;
}
