// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The real Registry must retain request ancestors after the endpoint future returns.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

const CHILD_WAIT: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<BTreeMap<String, String>>>>);

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
        let mut fields = Fields::default();
        event.record(&mut fields);
        if !matches!(
            fields.0.get("message").map(String::as_str),
            Some("Responses request shape" | "Responses request failed")
        ) {
            return;
        }
        if let Some(scope) = context.event_scope(event) {
            for span in scope.from_root() {
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
        self.0.lock().unwrap().push(fields.0);
    }
}

struct NoAuth;

impl codex_api::AuthProvider for NoAuth {
    fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_sse_failure_retains_real_parent_spans_after_caller_context_exits() {
    const CHILD: &str = "ALAB_SSE_CONTEXT_REGRESSION_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // Activation is process-wide; isolate this native endpoint regression.
        let name = format!(
            "{}::native_sse_failure_retains_real_parent_spans_after_caller_context_exits",
            module_path!().split_once("::").unwrap().1
        );
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", &name, "--nocapture"])
            .env(CHILD, "1");
        let report = crate::test_child_process::run(command, CHILD_WAIT)
            .await
            .unwrap_or_else(|failure| panic!("isolated context regression: {failure}"));
        report.assert_test_success(&name);
        return;
    }

    codex_alab_core::activate();
    let mut server = crate::sse_fixture::SseFixture::start().await;

    let transport = codex_api::ReqwestTransport::from_http_client(
        codex_http_client::HttpClientBuilder::new()
            .without_request_logging()
            .build_direct()
            .expect("direct loopback client"),
    );
    let client = codex_api::ResponsesClient::new(
        transport,
        codex_api::Provider {
            name: "context-fixture".into(),
            base_url: format!("{}/v1", server.uri()),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            stream_idle_timeout: Duration::from_secs(5),
        },
        Arc::new(NoAuth),
    );
    let capture = Capture::default();
    let mut stream = {
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        let _subscriber = tracing::subscriber::set_default(subscriber);
        let thread = tracing::info_span!("fixture_thread", thread_id = "original-thread");
        let _thread = thread.enter();
        let turn = tracing::info_span!("fixture_turn", turn_id = "original-turn");
        let _turn = turn.enter();
        tokio::time::timeout(
            Duration::from_secs(5),
            client.stream(
                serde_json::json!({
                    "model":"gpt-context-fixture",
                    "input":[{"type":"message","role":"user","content":"never-log-input"}]
                }),
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            ),
        )
        .await
        .expect("HTTP opening deadline")
        .expect("HTTP 200 opens a stream before the provider fails")
    };
    // Both entered parent spans, their handles, and the caller's subscriber
    // guard have dropped before the spawned SSE task can observe the failure.
    assert_eq!(
        capture.0.lock().unwrap().len(),
        1,
        "shape only before release"
    );
    server.release_failure();
    let error = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match stream.rx_event.recv().await {
                Some(Err(error)) => break error,
                Some(Ok(_)) => {}
                None => panic!("SSE stream closed without the provider error"),
            }
        }
    })
    .await
    .expect("terminal SSE delivery");
    assert!(matches!(error, codex_api::ApiError::InvalidRequest { .. }));
    server
        .finish()
        .await
        .expect("bounded SSE server completion");

    let records = capture.0.lock().unwrap();
    assert_eq!(records.len(), 2, "one shape and one semantic failure");
    let shape = &records[0];
    let failed = &records[1];
    assert_eq!(shape["message"], "Responses request shape");
    assert_eq!(failed["message"], "Responses request failed");
    assert_eq!(shape["local_attempt"], failed["local_attempt"]);
    for event in [shape, failed] {
        assert_eq!(event["thread_id"], "\"original-thread\"");
        assert_eq!(event["turn_id"], "\"original-turn\"");
    }
    assert_eq!(failed["request_id"], "\"req_context-1\"");
    assert_eq!(failed["cf_ray"], "\"ray_context-1\"");
    assert_eq!(failed["failure_kind"], "\"invalid_request\"");
    assert!(
        !failed.contains_key("http_status"),
        "opening 200 is not an error status"
    );
    let text = format!("{records:?}");
    for forbidden in [
        "never-log",
        "invalid identifier",
        "authorization",
        "invalid_access_program",
    ] {
        assert!(!text.contains(forbidden), "{text}");
    }
}

// The request diagnostic must restore only its own event context, rather than
// changing the subscriber seen by the existing websocket telemetry callbacks.
type ScopedRequestSubscriber =
    tracing_subscriber::layer::Layered<Capture, tracing_subscriber::Registry>;

fn request_subscriber_is_current() -> bool {
    tracing::dispatcher::get_default(|dispatch| {
        dispatch.downcast_ref::<ScopedRequestSubscriber>().is_some()
    })
}

#[derive(Clone, Default)]
struct WebsocketCallbackContext(Arc<Mutex<Vec<bool>>>);

impl codex_api::WebsocketTelemetry for WebsocketCallbackContext {
    fn on_ws_request(&self, _: Duration, _: Option<&codex_api::ApiError>, _: bool) {}

    fn on_ws_receive_failure(&self, _: Duration) {}

    fn on_ws_text_event(&self, _: &str, _: Option<&str>, _: Duration) {
        self.0.lock().unwrap().push(request_subscriber_is_current());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_websocket_failure_retains_request_dispatch_after_caller_context_exits() {
    const CHILD: &str = "ALAB_WS_CONTEXT_REGRESSION_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let name = format!(
            "{}::native_websocket_failure_retains_request_dispatch_after_caller_context_exits",
            module_path!().split_once("::").unwrap().1
        );
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", &name, "--nocapture"])
            .env(CHILD, "1");
        let report = crate::test_child_process::run(command, CHILD_WAIT)
            .await
            .unwrap_or_else(|failure| panic!("isolated websocket context regression: {failure}"));
        report.assert_test_success(&name);
        return;
    }

    codex_alab_core::activate();
    let failure = serde_json::json!({
        "type":"error",
        "error":{
            "code":"invalid_access_program",
            "message":"never-log-websocket-provider-content"
        },
        "headers":{"authorization":"Bearer never-log-websocket-header"}
    });
    let mut server = crate::websocket_fixture::WebsocketFixture::start(failure).await;
    let callback_context = WebsocketCallbackContext::default();
    let client = codex_api::ResponsesWebsocketClient::new(
        codex_api::Provider {
            name: "context-fixture".into(),
            base_url: format!("{}/v1", server.uri()),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            stream_idle_timeout: Duration::from_secs(5),
        },
        Arc::new(NoAuth),
    );
    let factory = codex_http_client::HttpClientFactory::new(
        codex_http_client::OutboundProxyPolicy::ReqwestDefault,
    );
    let connection = tokio::time::timeout(
        Duration::from_secs(5),
        client.connect(
            &factory,
            http::HeaderMap::new(),
            http::HeaderMap::new(),
            None,
            Some(Arc::new(callback_context.clone())),
        ),
    )
    .await
    .expect("websocket connection deadline")
    .expect("loopback websocket connection");
    let request = codex_api::ResponsesApiRequest {
        model: "gpt-context-fixture".into(),
        stream: true,
        service_tier: None,
        input: vec![
            serde_json::from_value(serde_json::json!({
                "type":"message", "role":"user", "content":[{
                    "type":"input_text", "text":"never-log-websocket-input"
                }]
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
    };
    let capture = Capture::default();
    let mut stream = {
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        let _subscriber = tracing::subscriber::set_default(subscriber);
        assert!(request_subscriber_is_current(), "positive context control");
        let thread = tracing::info_span!("fixture_thread", thread_id = "original-ws-thread");
        let _thread = thread.enter();
        let turn = tracing::info_span!("fixture_turn", turn_id = "original-ws-turn");
        let _turn = turn.enter();
        let stream = tokio::time::timeout(
            Duration::from_secs(5),
            connection.stream_request(
                codex_api::ResponsesWsRequest::ResponseCreate(
                    codex_api::ResponseCreateWsRequest::from(&request),
                ),
                false,
                None,
            ),
        )
        .await
        .expect("websocket request opening deadline")
        .expect("request opens before its gated semantic error");
        let observed = server.observed_request().await;
        assert_eq!(observed["type"], "response.create");
        stream
    };
    // The request reached the server, but its failure remains gated until both
    // parent span handles and the caller's thread-local subscriber have dropped.
    assert!(!request_subscriber_is_current());
    assert_eq!(
        capture.0.lock().unwrap().len(),
        1,
        "shape only before release"
    );
    assert!(callback_context.0.lock().unwrap().is_empty());
    server.release_failure();
    let error = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match stream.rx_event.recv().await {
                Some(Err(error)) => break error,
                Some(Ok(_)) => {}
                None => panic!("websocket closed without its gated error"),
            }
        }
    })
    .await
    .expect("terminal websocket error delivery");
    assert!(matches!(error, codex_api::ApiError::InvalidRequest { .. }));
    server.finish().await;

    let records = capture.0.lock().unwrap();
    assert_eq!(records.len(), 2, "one shape and one correlated failure");
    let shape = &records[0];
    let failed = &records[1];
    assert_eq!(shape["message"], "Responses request shape");
    assert_eq!(failed["message"], "Responses request failed");
    assert_eq!(shape["local_attempt"], failed["local_attempt"]);
    for event in [shape, failed] {
        assert_eq!(event["transport"], "\"responses_websocket\"");
        assert_eq!(event["thread_id"], "\"original-ws-thread\"");
        assert_eq!(event["turn_id"], "\"original-ws-turn\"");
    }
    assert_eq!(failed["failure_kind"], "\"invalid_request\"");
    assert!(!failed.contains_key("http_status"));
    assert_eq!(
        *callback_context.0.lock().unwrap(),
        [false],
        "diagnostic restoration must not change the telemetry callback's subscriber"
    );
    let text = format!("{records:?}");
    for forbidden in ["never-log", "authorization", "invalid_access_program"] {
        assert!(!text.contains(forbidden), "{text}");
    }
}
