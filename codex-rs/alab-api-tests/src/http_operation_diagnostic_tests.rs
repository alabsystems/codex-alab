// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Public HTTP-operation evidence without calling a proposed private adapter.

use codex_api::ApiError;
use codex_http_client::HttpTransport;
use codex_http_client::Request;
use codex_http_client::RequestBody;
use codex_http_client::Response;
use codex_http_client::StreamResponse;
use codex_http_client::TransportError;
use futures::StreamExt;
use serde_json::json;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::oneshot;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

const WAIT: Duration = Duration::from_secs(5);
const CHILD_WAIT: Duration = Duration::from_secs(60);
const OLD_TARGET: &str = "codex_api::endpoint::alab_request_diagnostics";
const TARGET: &str = "codex_api::endpoint::alab_request_diagnostics::http_operations";
type Call<'a> = Pin<Box<dyn Future<Output = Result<codex_api::ResponseStream, ApiError>> + 'a>>;
type Client = codex_api::ResponsesClient<ScriptedTransport>;
type EvidenceRow = (bool, BTreeMap<String, String>);

#[derive(Default)]
struct Fields {
    values: BTreeMap<String, String>,
    strict: bool,
}

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let allowed = matches!(
            field.name(),
            "message"
                | "local_attempt"
                | "transport"
                | "compression"
                | "connection_reused"
                | "warmup"
                | "model"
                | "mode"
                | "request_bytes"
                | "input_kind"
                | "input_items"
                | "messages"
                | "reasoning_items"
                | "tool_calls"
                | "tool_outputs"
                | "other_items"
                | "tools"
                | "tool_bytes"
                | "elapsed_ms"
                | "failure_kind"
                | "http_status"
                | "opaque_bad_request"
                | "retry_advised"
                | "request_id"
                | "cf_ray"
                | "thread_id"
                | "turn_id"
                | "http.method"
                | "api.path"
                | "operation_attempt"
        );
        if !allowed {
            assert!(
                !self.strict,
                "unexpected diagnostic field: {}",
                field.name()
            );
            return;
        }
        self.values
            .insert(field.name().into(), format!("{value:?}"));
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<EvidenceRow>>>);

impl<S> Layer<S> for Capture
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        ctx: Context<'_, S>,
    ) {
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        ctx.span(id).unwrap().extensions_mut().insert(fields);
    }
    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        if !matches!(event.metadata().target(), OLD_TARGET | TARGET)
            || *event.metadata().level() != tracing::Level::INFO
        {
            return;
        }
        let mut fields = Fields {
            strict: true,
            ..Fields::default()
        };
        event.record(&mut fields);
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                if let Some(parent) = span.extensions().get::<Fields>() {
                    for (name, value) in &parent.values {
                        fields
                            .values
                            .entry(name.clone())
                            .or_insert_with(|| value.clone());
                    }
                }
            }
        }
        self.0
            .lock()
            .unwrap()
            .push((event.metadata().target() == TARGET, fields.values));
    }
}

#[derive(Debug, PartialEq, Eq)]
struct CallbackContext {
    ambient: bool,
    span: Option<tracing::Id>,
}
fn callback_context() -> CallbackContext {
    CallbackContext {
        ambient: tracing::dispatcher::get_default(|dispatch| {
            dispatch.is::<tracing::subscriber::NoSubscriber>()
        }),
        span: tracing::Span::current().id(),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Signature {
    status: Option<u16>,
    kind: Option<&'static str>,
    private: Option<String>,
    allocation: Option<usize>,
    advice: Option<Option<codex_http_client::RetryAfter>>,
    headers: Option<http::HeaderMap>,
}
fn signature(status: Option<http::StatusCode>, error: Option<&TransportError>) -> Signature {
    let mut value = Signature {
        status: status.map(|status| status.as_u16()),
        kind: None,
        private: None,
        allocation: None,
        advice: None,
        headers: None,
    };
    value.kind = error.map(|error| match error {
        TransportError::Http {
            body,
            headers,
            retry_after,
            ..
        } => {
            value.private = body.clone();
            value.allocation = body.as_ref().map(|body| body.as_ptr() as usize);
            value.advice = Some(*retry_after);
            value.headers = headers.clone();
            "http"
        }
        TransportError::Network(text) | TransportError::Build(text) => {
            value.private = Some(text.clone());
            value.allocation = Some(text.as_ptr() as usize);
            if matches!(error, TransportError::Network(_)) {
                "network"
            } else {
                "request_build"
            }
        }
        TransportError::Policy(_) => "network_policy",
        TransportError::Timeout => "timeout",
        TransportError::Connection(_) => "connection",
        TransportError::RetryLimit => "retry_limit",
        TransportError::ResponseTooLarge { .. } => "response_too_large",
    });
    value
}

struct ExpectedCallback {
    label: String,
    attempt: u64,
    signature: Signature,
    context: CallbackContext,
}
struct Callback {
    label: String,
    attempt: u64,
    duration: Duration,
}
enum Reply {
    Error(TransportError),
    Complete,
}
struct Step {
    attempt: u64,
    gate: Option<oneshot::Receiver<()>>,
    reply: Reply,
}
#[derive(Default)]
struct Scene {
    plans: BTreeMap<String, VecDeque<Step>>,
    sent: BTreeMap<String, Vec<Vec<u8>>>,
    expected: VecDeque<ExpectedCallback>,
    callbacks: Vec<Callback>,
    has_delegate: bool,
}
impl Scene {
    fn produced(
        &mut self,
        label: &str,
        attempt: u64,
        status: Option<http::StatusCode>,
        error: Option<&TransportError>,
    ) {
        if self.has_delegate {
            self.expected.push_back(ExpectedCallback {
                label: label.into(),
                attempt,
                signature: signature(status, error),
                context: callback_context(),
            });
        }
    }
}
struct ScriptedTransport(Arc<Mutex<Scene>>);
impl HttpTransport for ScriptedTransport {
    async fn execute(&self, _: Request) -> Result<Response, TransportError> {
        panic!("streaming endpoint only");
    }
    async fn stream(&self, request: Request) -> Result<StreamResponse, TransportError> {
        assert_eq!(request.method, http::Method::POST);
        assert_eq!(request.headers["authorization"], "Bearer never-log-auth");
        let Some(RequestBody::EncodedJson(body)) = request.body else {
            panic!("prepared encoded body");
        };
        let input: serde_json::Value = serde_json::from_slice(body.as_bytes()).unwrap();
        let label = input["fixture_label"].as_str().unwrap().to_owned();
        let step = {
            let mut scene = self.0.lock().unwrap();
            scene
                .sent
                .entry(label.clone())
                .or_default()
                .push(body.as_bytes().to_vec());
            scene
                .plans
                .get_mut(&label)
                .unwrap()
                .pop_front()
                .expect("no extra operation")
        };
        if let Some(gate) = step.gate {
            tokio::time::timeout(WAIT, gate)
                .await
                .expect("operation gate deadline")
                .expect("gate released");
        }
        match step.reply {
            Reply::Error(error) => {
                let status = match &error {
                    TransportError::Http { status, .. } => Some(*status),
                    _ => None,
                };
                self.0
                    .lock()
                    .unwrap()
                    .produced(&label, step.attempt, status, Some(&error));
                Err(error)
            }
            Reply::Complete => {
                self.0.lock().unwrap().produced(
                    &label,
                    step.attempt,
                    Some(http::StatusCode::OK),
                    None,
                );
                let body = format!(
                    "data: {}\n\ndata: {}\n\n",
                    json!({"type":"response.created","response":{"id":"never-log-response-id"}}),
                    json!({"type":"response.completed","response":{"id":"never-log-response-id"}})
                );
                Ok(StreamResponse {
                    status: http::StatusCode::OK,
                    headers: http::HeaderMap::new(),
                    bytes: futures::stream::once(async move { Ok(body.into_bytes().into()) })
                        .boxed(),
                })
            }
        }
    }
}

struct Delegate(Arc<Mutex<Scene>>);
impl codex_api::RequestTelemetry for Delegate {
    fn on_request(
        &self,
        attempt: u64,
        status: Option<http::StatusCode>,
        error: Option<&TransportError>,
        duration: Duration,
    ) {
        let mut scene = self.0.lock().unwrap();
        let expected = scene
            .expected
            .pop_front()
            .expect("one original producer per callback");
        assert_eq!(attempt, expected.attempt);
        assert_eq!(
            signature(status, error),
            expected.signature,
            "typed values and original private allocations survive forwarding"
        );
        assert_eq!(
            callback_context(),
            expected.context,
            "delegate keeps its ambient subscriber and span"
        );
        scene.callbacks.push(Callback {
            label: expected.label,
            attempt,
            duration,
        });
    }
}
struct Auth;
impl codex_api::AuthProvider for Auth {
    fn add_auth_headers(&self, headers: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        headers.insert("authorization", "Bearer never-log-auth".parse().unwrap());
        Ok(())
    }
}
struct RejectAuth {
    scene: Arc<Mutex<Scene>>,
    label: &'static str,
    source: bool,
    gate: Mutex<Option<oneshot::Receiver<()>>>,
    auth_calls: AtomicUsize,
}
impl RejectAuth {
    fn rejection(&self) -> codex_api::AuthError {
        let error = TransportError::Policy(codex_http_client::NetworkPolicyDenied::Destination);
        self.scene
            .lock()
            .unwrap()
            .produced(self.label, 0, None, Some(&error));
        codex_api::AuthError::Policy(codex_http_client::NetworkPolicyDenied::Destination)
    }
}
impl codex_api::AuthProvider for RejectAuth {
    fn alab_source_policy(
        &self,
    ) -> Result<Option<codex_http_client::NetworkPolicy>, codex_api::AuthError> {
        if self.source {
            Err(self.rejection())
        } else {
            Ok(None)
        }
    }
    fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        panic!("asynchronous auth path only");
    }
    fn apply_auth(&self, _: Request) -> codex_api::AuthProviderFuture<'_> {
        Box::pin(async move {
            self.auth_calls.fetch_add(1, Ordering::Relaxed);
            let gate = self.gate.lock().unwrap().take().expect("one auth gate");
            tokio::time::timeout(WAIT, gate)
                .await
                .expect("auth gate deadline")
                .expect("auth gate released");
            Err(self.rejection())
        })
    }
}

fn request(label: &str) -> serde_json::Value {
    json!({"fixture_label":label, "model":"gpt-operation-fixture", "input":[
        {"type":"message","role":"user","content":"never-log-input"},
        {"type":"function_call","name":"fixture_tool","call_id":"completed-tool","arguments":"never-log-arguments"},
        {"type":"function_call_output","call_id":"completed-tool","output":"never-log-output"}]})
}
fn client(
    scene: Arc<Mutex<Scene>>,
    auth: Arc<dyn codex_api::AuthProvider>,
    delegate: bool,
) -> Client {
    scene.lock().unwrap().has_delegate = delegate;
    codex_api::ResponsesClient::new(
        ScriptedTransport(scene.clone()),
        codex_api::Provider {
            name: "operation-fixture".into(),
            base_url: "https://never-log-url.invalid/v1".into(),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 2,
                base_delay: Duration::ZERO,
                retry_429: false,
                retry_5xx: true,
                retry_transport: true,
            },
            stream_idle_timeout: WAIT,
        },
        auth,
    )
    .with_telemetry(
        delegate.then(|| Arc::new(Delegate(scene)) as Arc<dyn codex_api::RequestTelemetry>),
        None,
    )
}
fn step(attempt: u64, reply: Reply) -> Step {
    Step {
        attempt,
        gate: None,
        reply,
    }
}
fn gated(attempt: u64, reply: Reply) -> (oneshot::Sender<()>, Step) {
    let (release, gate) = oneshot::channel();
    (
        release,
        Step {
            attempt,
            gate: Some(gate),
            reply,
        },
    )
}
fn scene(plans: Vec<(&str, Vec<Step>)>) -> Arc<Mutex<Scene>> {
    Arc::new(Mutex::new(Scene {
        plans: plans
            .into_iter()
            .map(|(label, steps)| (label.into(), steps.into()))
            .collect(),
        ..Scene::default()
    }))
}
fn http_error(status: http::StatusCode, id: &'static str, advised: bool) -> TransportError {
    let mut headers = http::HeaderMap::new();
    headers.append("x-request-id", "never-log invalid id".parse().unwrap());
    headers.append("x-request-id", id.parse().unwrap());
    headers.insert("cf-ray", "ray_operation".parse().unwrap());
    headers.insert("retry-after", "7".parse().unwrap());
    headers.insert("x-unrelated", "never-log-header".parse().unwrap());
    TransportError::Http {
        status,
        url: Some("https://never-log-url.invalid/token".into()),
        headers: Some(headers),
        body: Some("{\"error\":\"never-log-http-body\"}".into()),
        retry_after: advised
            .then(|| codex_http_client::RetryAfter::from_delay(Duration::ZERO).unwrap()),
    }
}
async fn start<'a>(
    client: &'a Client,
    label: &str,
    capture: &Capture,
    context: &'static str,
) -> Call<'a> {
    let _subscriber =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
    let span = tracing::info_span!("captured_request", thread_id = context, turn_id = context);
    let _span = span.enter();
    assert!(!callback_context().ambient);
    assert!(callback_context().span.is_some());
    let mut call = Box::pin(client.stream(
        request(label),
        http::HeaderMap::new(),
        codex_api::Compression::None,
        None,
    ));
    assert!(
        futures::poll!(call.as_mut()).is_pending(),
        "first operation reached its real gate"
    );
    call
}
async fn opened(call: Call<'_>) {
    let mut stream = tokio::time::timeout(WAIT, call)
        .await
        .expect("endpoint deadline")
        .expect("operation recovered");
    tokio::time::timeout(WAIT, async {
        assert!(matches!(stream.rx_event.recv().await, Some(Ok(codex_api::ResponseEvent::RateLimits(_)))));
        assert!(matches!(stream.rx_event.recv().await, Some(Ok(codex_api::ResponseEvent::Created { response_id: Some(id) })) if id == "never-log-response-id"));
        assert!(matches!(stream.rx_event.recv().await, Some(Ok(codex_api::ResponseEvent::Completed { response_id, .. })) if response_id == "never-log-response-id"));
        assert!(stream.rx_event.recv().await.is_none());
    }).await.expect("completion/channel deadline");
}
async fn denied(call: Call<'_>, policy: bool) {
    let result = tokio::time::timeout(WAIT, call)
        .await
        .expect("rejection deadline");
    match result {
        Err(ApiError::Transport(TransportError::Policy(
            codex_http_client::NetworkPolicyDenied::Destination,
        ))) if policy => {}
        Err(ApiError::Transport(TransportError::Http {
            status,
            body,
            headers: Some(headers),
            retry_after,
            ..
        })) if !policy => {
            assert_eq!(status, http::StatusCode::BAD_REQUEST);
            assert_eq!(body.as_deref(), Some("{\"error\":\"never-log-http-body\"}"));
            assert_eq!(headers["retry-after"], "7");
            assert!(retry_after.is_none());
        }
        _ => panic!("original typed operation error must survive"),
    }
}
fn old_preconditions(capture: &Capture, context: &str, selected: bool) -> u64 {
    let records = capture.0.lock().unwrap();
    let old: Vec<_> = records
        .iter()
        .filter(|(operation, _)| !operation)
        .map(|(_, fields)| fields)
        .collect();
    assert_eq!(old.len(), if selected { 2 } else { 1 });
    let shape = old[0];
    assert_eq!(shape["message"], "Responses request shape");
    assert_eq!(shape["input_items"], "3");
    assert_eq!(shape["tool_calls"], "1");
    assert_eq!(shape["tool_outputs"], "1");
    for fields in &old {
        assert_eq!(fields["thread_id"], format!("{context:?}"));
        assert_eq!(fields["turn_id"], format!("{context:?}"));
        assert_eq!(fields["transport"], "\"responses_http\"");
    }
    if selected {
        assert_eq!(old[1]["message"], "Responses request failed");
        assert_eq!(old[1]["local_attempt"], shape["local_attempt"]);
    }
    let logged = format!("{records:?}");
    for private in [
        "never-log",
        "fixture_tool",
        "completed-tool",
        "authorization",
        "x-unrelated",
    ] {
        assert!(
            !logged.contains(private),
            "private content stays outside evidence"
        );
    }
    let attempt = shape["local_attempt"].parse::<u64>().unwrap();
    assert!(attempt > 0);
    attempt
}
fn sent_preconditions(scene: &Arc<Mutex<Scene>>, expected: &[(&str, usize)], callbacks: usize) {
    let scene = scene.lock().unwrap();
    assert!(scene.plans.values().all(VecDeque::is_empty));
    assert!(scene.expected.is_empty());
    assert_eq!(scene.callbacks.len(), callbacks);
    for (label, count) in expected {
        assert_eq!(scene.sent.get(*label).map_or(0, Vec::len), *count);
        if let Some(sent) = scene.sent.get(*label) {
            let expected = serde_json::to_vec(&request(label)).unwrap();
            assert!(
                sent.iter().all(|body| body == &expected),
                "encoded history is unchanged"
            );
        }
    }
}
struct Op {
    index: u64,
    status: Option<u16>,
    kind: Option<&'static str>,
    advice: Option<bool>,
    id: Option<&'static str>,
}
fn operations(
    capture: &Capture,
    context: &str,
    local: u64,
    expected: &[Op],
    delegate: Option<(&Arc<Mutex<Scene>>, &str)>,
) {
    let records = capture.0.lock().unwrap();
    let rows: Vec<_> = records
        .iter()
        .filter(|(operation, _)| *operation)
        .map(|(_, fields)| fields)
        .collect();
    // New evidence is deliberately last, after original callbacks/recovery/input/context/privacy.
    assert_eq!(
        rows.len(),
        expected.len(),
        "missing per-operation INFO evidence"
    );
    for (row, expected) in rows.iter().zip(expected) {
        assert_eq!(row["message"], "Responses HTTP operation completed");
        assert_eq!(row["local_attempt"], local.to_string());
        assert_eq!(row["operation_attempt"], expected.index.to_string());
        assert_eq!(row["transport"], "\"responses_http\"");
        assert_eq!(row["thread_id"], format!("{context:?}"));
        assert_eq!(row["turn_id"], format!("{context:?}"));
        assert_eq!(
            row.get("http_status").cloned(),
            expected.status.map(|status| status.to_string())
        );
        assert_eq!(
            row.get("failure_kind").cloned(),
            expected.kind.map(|kind| format!("{kind:?}"))
        );
        assert_eq!(
            row.get("retry_advised").cloned(),
            expected.advice.map(|advice| advice.to_string())
        );
        assert_eq!(
            row.get("request_id").cloned(),
            expected.id.map(|id| format!("{id:?}"))
        );
        assert_eq!(
            row.get("cf_ray").map(String::as_str),
            expected.id.map(|_| "\"ray_operation\"")
        );
        let elapsed = row["elapsed_ms"].parse::<u64>().unwrap();
        if let Some((scene, label)) = delegate {
            let scene = scene.lock().unwrap();
            let callback = scene
                .callbacks
                .iter()
                .find(|callback| callback.label == label && callback.attempt == expected.index)
                .unwrap();
            assert_eq!(
                elapsed,
                u64::try_from(callback.duration.as_millis()).unwrap_or(u64::MAX)
            );
        }
    }
}
async fn parent(name: &str) -> bool {
    const CHILD: &str = "ALAB_HTTP_OPERATION_CHILD";
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(name));
        assert_eq!(
            std::env::args().skip(1).collect::<Vec<_>>(),
            ["--exact", &exact, "--nocapture", "--test-threads=1"]
        );
        return false;
    }
    assert!(!codex_alab_core::active());
    let marker = format!("ALAB_HTTP_OPERATION_OK:{name}\n");
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
    crate::test_child_process::run_with_stdout_marker(command, CHILD_WAIT, Some(&marker))
        .await
        .unwrap_or_else(|failure| panic!("isolated operation child: {failure}"))
        .assert_test_success(&exact);
    assert!(!codex_alab_core::active());
    true
}
fn complete(name: &str) {
    std::io::stdout()
        .lock()
        .write_all(format!("ALAB_HTTP_OPERATION_OK:{name}\n").as_bytes())
        .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn http_operation_metadata_tracks_recovery_without_changing_delegate_callbacks() {
    const NAME: &str =
        "http_operation_metadata_tracks_recovery_without_changing_delegate_callbacks";
    if parent(NAME).await {
        return;
    }
    codex_alab_core::activate();
    let (release, first) = gated(
        0,
        Reply::Error(TransportError::Network("never-log-network".into())),
    );
    let scene = scene(vec![(
        "recovery",
        vec![
            first,
            step(
                1,
                Reply::Error(http_error(
                    http::StatusCode::SERVICE_UNAVAILABLE,
                    "req_operation",
                    true,
                )),
            ),
            step(2, Reply::Complete),
        ],
    )]);
    let client = client(scene.clone(), Arc::new(Auth), true);
    let capture = Capture::default();
    let call = start(&client, "recovery", &capture, "recovery-context").await;
    {
        let _subscriber =
            tracing::subscriber::set_default(tracing::subscriber::NoSubscriber::new());
        assert!(callback_context().ambient);
        assert!(callback_context().span.is_none());
        release.send(()).unwrap();
        opened(call).await;
    }
    sent_preconditions(&scene, &[("recovery", 3)], 3);
    let local = old_preconditions(&capture, "recovery-context", false);
    println!("ALAB_HTTP_OPERATION:case=recovery:stage=late_rows");
    operations(
        &capture,
        "recovery-context",
        local,
        &[
            Op {
                index: 0,
                status: None,
                kind: Some("network"),
                advice: None,
                id: None,
            },
            Op {
                index: 1,
                status: Some(503),
                kind: Some("http"),
                advice: Some(true),
                id: Some("req_operation"),
            },
            Op {
                index: 2,
                status: Some(200),
                kind: None,
                advice: None,
                id: None,
            },
        ],
        Some((&scene, "recovery")),
    );
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_http_operations_keep_request_contexts_and_attempts_separate() {
    const NAME: &str = "concurrent_http_operations_keep_request_contexts_and_attempts_separate";
    if parent(NAME).await {
        return;
    }
    codex_alab_core::activate();
    let (release_a, first_a) = gated(
        0,
        Reply::Error(TransportError::Network("never-log-a".into())),
    );
    let (release_b, first_b) = gated(0, Reply::Error(TransportError::Timeout));
    let scene = scene(vec![
        ("a", vec![first_a, step(1, Reply::Complete)]),
        (
            "b",
            vec![
                first_b,
                step(
                    1,
                    Reply::Error(http_error(
                        http::StatusCode::BAD_REQUEST,
                        "req_terminal",
                        false,
                    )),
                ),
            ],
        ),
    ]);
    let client = client(scene.clone(), Arc::new(Auth), true);
    let a = Capture::default();
    let b = Capture::default();
    let call_a = start(&client, "a", &a, "context-a").await;
    let call_b = start(&client, "b", &b, "context-b").await;
    {
        let _subscriber =
            tracing::subscriber::set_default(tracing::subscriber::NoSubscriber::new());
        assert!(callback_context().ambient);
        assert!(callback_context().span.is_none());
        release_a.send(()).unwrap();
        opened(call_a).await;
        release_b.send(()).unwrap();
        denied(call_b, false).await;
    }
    sent_preconditions(&scene, &[("a", 2), ("b", 2)], 4);
    let local_a = old_preconditions(&a, "context-a", false);
    let local_b = old_preconditions(&b, "context-b", true);
    assert_ne!(local_a, local_b);
    println!("ALAB_HTTP_OPERATION:case=concurrent:stage=late_rows");
    operations(
        &a,
        "context-a",
        local_a,
        &[
            Op {
                index: 0,
                status: None,
                kind: Some("network"),
                advice: None,
                id: None,
            },
            Op {
                index: 1,
                status: Some(200),
                kind: None,
                advice: None,
                id: None,
            },
        ],
        Some((&scene, "a")),
    );
    operations(
        &b,
        "context-b",
        local_b,
        &[
            Op {
                index: 0,
                status: None,
                kind: Some("timeout"),
                advice: None,
                id: None,
            },
            Op {
                index: 1,
                status: Some(400),
                kind: Some("http"),
                advice: Some(false),
                id: Some("req_terminal"),
            },
        ],
        Some((&scene, "b")),
    );
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn http_operation_metadata_respects_quiet_inactive_and_cancelled_boundaries() {
    const NAME: &str = "http_operation_metadata_respects_quiet_inactive_and_cancelled_boundaries";
    if parent(NAME).await {
        return;
    }
    let (release, first) = gated(
        0,
        Reply::Error(TransportError::Network("never-log-inactive".into())),
    );
    let inactive_scene = scene(vec![("inactive", vec![first, step(1, Reply::Complete)])]);
    let inactive_client = client(inactive_scene.clone(), Arc::new(Auth), true);
    let inactive = Capture::default();
    let call = start(&inactive_client, "inactive", &inactive, "inactive-context").await;
    assert!(inactive.0.lock().unwrap().is_empty());
    codex_alab_core::activate();
    {
        let _subscriber =
            tracing::subscriber::set_default(tracing::subscriber::NoSubscriber::new());
        assert!(callback_context().ambient);
        assert!(callback_context().span.is_none());
        release.send(()).unwrap();
        opened(call).await;
    }
    sent_preconditions(&inactive_scene, &[("inactive", 2)], 2);
    assert!(
        inactive.0.lock().unwrap().is_empty(),
        "captured None stays inactive after activation"
    );
    let (release, first) = gated(0, Reply::Complete);
    let quiet_scene = scene(vec![("quiet", vec![first])]);
    let quiet_client = client(quiet_scene.clone(), Arc::new(Auth), true);
    let quiet = Capture::default();
    let call = start(&quiet_client, "quiet", &quiet, "quiet-context").await;
    {
        let _subscriber =
            tracing::subscriber::set_default(tracing::subscriber::NoSubscriber::new());
        assert!(callback_context().ambient);
        assert!(callback_context().span.is_none());
        release.send(()).unwrap();
        opened(call).await;
    }
    sent_preconditions(&quiet_scene, &[("quiet", 1)], 1);
    let quiet_local = old_preconditions(&quiet, "quiet-context", false);
    let (held, first) = gated(0, Reply::Error(TransportError::Timeout));
    let cancelled_scene = scene(vec![("cancelled", vec![first])]);
    let cancelled_client = client(cancelled_scene.clone(), Arc::new(Auth), true);
    let cancelled = Capture::default();
    let call = start(
        &cancelled_client,
        "cancelled",
        &cancelled,
        "cancelled-context",
    )
    .await;
    drop(call);
    assert!(held.is_closed(), "owning future retired its operation gate");
    sent_preconditions(&cancelled_scene, &[("cancelled", 1)], 0);
    let cancelled_local = old_preconditions(&cancelled, "cancelled-context", false);
    let auth_scene = scene(vec![]);
    let (release, gate) = oneshot::channel();
    let auth = Arc::new(RejectAuth {
        scene: auth_scene.clone(),
        label: "auth",
        source: false,
        gate: Mutex::new(Some(gate)),
        auth_calls: AtomicUsize::new(0),
    });
    let auth_client = client(auth_scene.clone(), auth.clone(), true);
    let auth_capture = Capture::default();
    let call = start(&auth_client, "auth", &auth_capture, "auth-context").await;
    {
        let _subscriber =
            tracing::subscriber::set_default(tracing::subscriber::NoSubscriber::new());
        assert!(callback_context().ambient);
        assert!(callback_context().span.is_none());
        release.send(()).unwrap();
        denied(call, true).await;
    }
    assert_eq!(auth.auth_calls.load(Ordering::Relaxed), 1);
    sent_preconditions(&auth_scene, &[("auth", 0)], 1);
    let auth_local = old_preconditions(&auth_capture, "auth-context", true);
    let source_scene = scene(vec![]);
    let source = Arc::new(RejectAuth {
        scene: source_scene.clone(),
        label: "source",
        source: true,
        gate: Mutex::new(None),
        auth_calls: AtomicUsize::new(0),
    });
    let source_client = client(source_scene.clone(), source.clone(), true);
    let source_capture = Capture::default();
    {
        let _subscriber = tracing::subscriber::set_default(
            tracing_subscriber::registry().with(source_capture.clone()),
        );
        let span = tracing::info_span!(
            "captured_source",
            thread_id = "source-context",
            turn_id = "source-context"
        );
        let _span = span.enter();
        denied(
            Box::pin(source_client.stream(
                request("source"),
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            )),
            true,
        )
        .await;
    }
    assert_eq!(source.auth_calls.load(Ordering::Relaxed), 0);
    sent_preconditions(&source_scene, &[("source", 0)], 1);
    let source_local = old_preconditions(&source_capture, "source-context", true);
    let (release, first) = gated(
        0,
        Reply::Error(TransportError::Network("never-log-no-inner".into())),
    );
    let no_inner_scene = scene(vec![("no-inner", vec![first, step(1, Reply::Complete)])]);
    let no_inner_client = client(no_inner_scene.clone(), Arc::new(Auth), false);
    let no_inner = Capture::default();
    let call = start(&no_inner_client, "no-inner", &no_inner, "no-inner-context").await;
    release.send(()).unwrap();
    opened(call).await;
    sent_preconditions(&no_inner_scene, &[("no-inner", 2)], 0);
    let no_inner_local = old_preconditions(&no_inner, "no-inner-context", false);
    println!("ALAB_HTTP_OPERATION:case=boundaries:stage=late_rows");
    operations(
        &quiet,
        "quiet-context",
        quiet_local,
        &[],
        Some((&quiet_scene, "quiet")),
    );
    operations(
        &cancelled,
        "cancelled-context",
        cancelled_local,
        &[],
        Some((&cancelled_scene, "cancelled")),
    );
    operations(
        &auth_capture,
        "auth-context",
        auth_local,
        &[Op {
            index: 0,
            status: None,
            kind: Some("network_policy"),
            advice: None,
            id: None,
        }],
        Some((&auth_scene, "auth")),
    );
    operations(
        &source_capture,
        "source-context",
        source_local,
        &[Op {
            index: 0,
            status: None,
            kind: Some("network_policy"),
            advice: None,
            id: None,
        }],
        Some((&source_scene, "source")),
    );
    operations(
        &no_inner,
        "no-inner-context",
        no_inner_local,
        &[
            Op {
                index: 0,
                status: None,
                kind: Some("network"),
                advice: None,
                id: None,
            },
            Op {
                index: 1,
                status: Some(200),
                kind: None,
                advice: None,
                id: None,
            },
        ],
        None,
    );
    complete(NAME);
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_http_retry_wait_keeps_only_completed_failure() {
    const NAME: &str = "cancelled_http_retry_wait_keeps_only_completed_failure";
    const CANCEL: &str = "cancel-backoff";
    const LIVE: &str = "live-backoff";
    if parent(NAME).await {
        return;
    }
    codex_alab_core::activate();
    tokio::time::pause();
    let advised = |id: &'static str| {
        // An unavailable receipt reading fixes the virtual-clock premise. This
        // case does not test platform sleep/wake accounting.
        let advice = codex_http_client::RetryAfter::from_delay_with_sleep_reading(
            Duration::from_secs(10),
            None,
        )
        .unwrap();
        let deadline = advice.deadline();
        let mut error = http_error(http::StatusCode::SERVICE_UNAVAILABLE, id, false);
        let TransportError::Http { retry_after, .. } = &mut error else {
            unreachable!();
        };
        *retry_after = Some(advice);
        (advice, deadline, error)
    };
    let (cancel_advice, cancel_deadline, error) = advised("req_backoff-cancel");
    let (release, first) = gated(0, Reply::Error(error));
    let scene = scene(vec![(CANCEL, vec![first, step(1, Reply::Complete)])]);
    let client = client(scene.clone(), Arc::new(Auth), true);
    let cancelled = Capture::default();
    let mut call = start(&client, CANCEL, &cancelled, "cancel-backoff-context").await;
    let assert_cancelled = || {
        let scene = scene.lock().unwrap();
        assert!(scene.expected.is_empty());
        let remaining = &scene.plans[CANCEL];
        assert_eq!(remaining.len(), 1, "cancelled continuation stays unused");
        assert_eq!(remaining[0].attempt, 1);
        assert!(remaining[0].gate.is_none());
        assert!(matches!(&remaining[0].reply, Reply::Complete));
        assert_eq!(
            scene.sent[CANCEL],
            [serde_json::to_vec(&request(CANCEL)).unwrap()]
        );
        let callbacks = scene
            .callbacks
            .iter()
            .filter(|callback| callback.label == CANCEL)
            .map(|callback| callback.attempt)
            .collect::<Vec<_>>();
        assert_eq!(callbacks, [0]);
    };
    let cancelled_local;
    let cancelled_records;
    {
        let _subscriber =
            tracing::subscriber::set_default(tracing::subscriber::NoSubscriber::new());
        assert!(callback_context().ambient);
        assert!(callback_context().span.is_none());
        let before = tokio::time::Instant::now();
        release.send(()).unwrap();
        assert!(futures::poll!(call.as_mut()).is_pending());
        assert_eq!(tokio::time::Instant::now(), before);
        assert!(cancel_deadline > before);
        assert!(cancel_advice.remaining_delay() > Duration::ZERO);
        assert_eq!(scene.lock().unwrap().callbacks.len(), 1);
        assert_cancelled();
        cancelled_local = old_preconditions(&cancelled, "cancel-backoff-context", false);
        operations(
            &cancelled,
            "cancel-backoff-context",
            cancelled_local,
            &[Op {
                index: 0,
                status: Some(503),
                kind: Some("http"),
                advice: Some(true),
                id: Some("req_backoff-cancel"),
            }],
            Some((&scene, CANCEL)),
        );
        cancelled_records = cancelled.0.lock().unwrap().clone();
        // No await separates the completed callback/Pending witness from the
        // owning future's drop under the same ambient subscriber.
        drop(call);
        tokio::time::advance(
            cancel_deadline.saturating_duration_since(tokio::time::Instant::now())
                + Duration::from_millis(10),
        )
        .await;
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(tokio::time::Instant::now() > cancel_deadline);
        assert_cancelled();
        assert_eq!(scene.lock().unwrap().callbacks.len(), 1);
        assert_eq!(*cancelled.0.lock().unwrap(), cancelled_records);
        assert_eq!(
            old_preconditions(&cancelled, "cancel-backoff-context", false),
            cancelled_local
        );
    }

    // A fresh deadline after the cancelled clock advance proves this same
    // retained client can still wait and recover normally.
    let (live_advice, live_deadline, error) = advised("req_backoff-live");
    let (release, first) = gated(0, Reply::Error(error));
    assert!(
        scene
            .lock()
            .unwrap()
            .plans
            .insert(LIVE.into(), vec![first, step(1, Reply::Complete)].into())
            .is_none()
    );
    let live = Capture::default();
    let mut call = start(&client, LIVE, &live, "live-backoff-context").await;
    {
        let _subscriber =
            tracing::subscriber::set_default(tracing::subscriber::NoSubscriber::new());
        assert!(callback_context().ambient);
        assert!(callback_context().span.is_none());
        let before = tokio::time::Instant::now();
        release.send(()).unwrap();
        assert!(futures::poll!(call.as_mut()).is_pending());
        assert_eq!(tokio::time::Instant::now(), before);
        assert!(live_deadline > before);
        assert!(live_advice.remaining_delay() > Duration::ZERO);
        {
            let scene = scene.lock().unwrap();
            assert_eq!(scene.sent[LIVE].len(), 1);
            assert_eq!(scene.callbacks.len(), 2);
            assert_eq!(scene.plans[LIVE].len(), 1);
            assert!(scene.expected.is_empty());
        }
        let live_local = old_preconditions(&live, "live-backoff-context", false);
        assert_ne!(live_local, cancelled_local);
        operations(
            &live,
            "live-backoff-context",
            live_local,
            &[Op {
                index: 0,
                status: Some(503),
                kind: Some("http"),
                advice: Some(true),
                id: Some("req_backoff-live"),
            }],
            Some((&scene, LIVE)),
        );
        tokio::time::advance(
            (live_deadline - Duration::from_millis(1))
                .saturating_duration_since(tokio::time::Instant::now()),
        )
        .await;
        assert!(tokio::time::Instant::now() < live_deadline);
        assert!(futures::poll!(call.as_mut()).is_pending());
        {
            let scene = scene.lock().unwrap();
            assert_eq!(scene.sent[LIVE].len(), 1);
            assert_eq!(scene.callbacks.len(), 2);
            assert_eq!(scene.plans[LIVE].len(), 1);
            assert!(scene.expected.is_empty());
        }
        tokio::time::advance(
            live_deadline.saturating_duration_since(tokio::time::Instant::now())
                + Duration::from_millis(10),
        )
        .await;
        assert!(tokio::time::Instant::now() > live_deadline);
        opened(call).await;
    }
    assert_cancelled();
    {
        let scene = scene.lock().unwrap();
        assert!(scene.plans[LIVE].is_empty());
        assert!(scene.expected.is_empty());
        let expected = serde_json::to_vec(&request(LIVE)).unwrap();
        assert_eq!(scene.sent[LIVE], [expected.clone(), expected]);
        let callbacks = scene
            .callbacks
            .iter()
            .map(|callback| (callback.label.as_str(), callback.attempt))
            .collect::<Vec<_>>();
        assert_eq!(callbacks, [(CANCEL, 0), (LIVE, 0), (LIVE, 1)]);
    }
    let live_local = old_preconditions(&live, "live-backoff-context", false);
    assert_ne!(live_local, cancelled_local);
    operations(
        &live,
        "live-backoff-context",
        live_local,
        &[
            Op {
                index: 0,
                status: Some(503),
                kind: Some("http"),
                advice: Some(true),
                id: Some("req_backoff-live"),
            },
            Op {
                index: 1,
                status: Some(200),
                kind: None,
                advice: None,
                id: None,
            },
        ],
        Some((&scene, LIVE)),
    );
    assert_eq!(*cancelled.0.lock().unwrap(), cancelled_records);
    drop(client);
    complete(NAME);
}
