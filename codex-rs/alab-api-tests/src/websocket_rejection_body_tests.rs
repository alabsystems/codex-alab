// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A rejected upgrade's body must survive HTTP packet boundaries and API mapping.

use codex_api::ApiError;
use codex_api::ResponsesWebsocketConnection;
use codex_api::TransportError;
use futures::task::ArcWake;
use futures::task::waker_ref;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::Instrument;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context as LayerContext;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

const WAIT: Duration = Duration::from_secs(5);
const CHILD_WAIT: Duration = Duration::from_secs(60);
const DIAGNOSTIC_TARGET: &str = "codex_api::endpoint::alab_request_diagnostics";
const OPAQUE: &str = r#"{"detail":"Bad Request"}"#;
const FORBIDDEN: &str =
    r#"{"error":{"code":"codex_workspace_access_denied","message":"fixture refusal"}}"#;

type ConnectResult = Result<ResponsesWebsocketConnection, ApiError>;
type ConnectFuture<'a> = Pin<Box<dyn Future<Output = ConnectResult> + 'a>>;

#[derive(Default)]
struct Fields(BTreeMap<String, serde_json::Value>);

impl Fields {
    fn assert_allowed(metadata: &tracing::Metadata<'_>) {
        // Reject an unexpected payload-bearing event field before invoking its
        // Debug formatter or retaining its value in a test failure.
        for field in metadata.fields() {
            assert!(
                matches!(
                    field.name(),
                    "message"
                        | "transport"
                        | "phase"
                        | "http_status"
                        | "body_capture"
                        | "body_wait_ms"
                        | "rejection_body_bytes"
                        | "rejection_body_bytes_accepted"
                        | "retry_advised"
                        | "upgrade_request_id"
                        | "upgrade_cf_ray"
                        | "thread_id"
                        | "thread.id"
                        | "turn.id"
                ),
                "unexpected handshake diagnostic field: {}",
                field.name()
            );
        }
    }
}

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

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.0.insert(field.name().into(), value.into());
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0.insert(field.name().into(), value.into());
    }
}

#[derive(Debug)]
struct CapturedEvent {
    level: tracing::Level,
    ancestors: Vec<&'static str>,
    fields: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<CapturedEvent>>>);

impl<S> Layer<S> for Capture
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attributes: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        context: LayerContext<'_, S>,
    ) {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        context.span(id).unwrap().extensions_mut().insert(fields);
    }

    fn on_event(&self, event: &tracing::Event<'_>, context: LayerContext<'_, S>) {
        // Include every event at this target, so a fabricated request-shape
        // event cannot hide behind a message filter. The later opaque bridge
        // has a separate target and is outside this privacy contract.
        if event.metadata().target() != DIAGNOSTIC_TARGET {
            return;
        }
        Fields::assert_allowed(event.metadata());
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
        self.0.lock().unwrap().push(CapturedEvent {
            level: *event.metadata().level(),
            ancestors,
            fields: fields.0,
        });
    }
}

impl Capture {
    fn assert_rejection(
        &self,
        status: u16,
        url: &str,
        body_capture: &str,
        body_bytes: Option<usize>,
        accepted_bytes: usize,
    ) {
        let records = self.0.lock().unwrap();
        assert_eq!(records.len(), 1, "one rejection and no request-shape event");
        let event = &records[0];
        assert_eq!(event.level, tracing::Level::INFO);
        assert!(
            event
                .ancestors
                .windows(2)
                .any(|names| names == ["session_loop", "turn"]),
            "production ancestor spans: {event:?}"
        );
        let fields = &event.fields;
        for (name, expected) in [
            ("message", "Responses WebSocket handshake rejected"),
            ("transport", "responses_websocket"),
            ("phase", "handshake"),
            ("body_capture", body_capture),
            ("upgrade_request_id", "req_body-1"),
            ("upgrade_cf_ray", "ray_body-1"),
            ("thread_id", "original-handshake-thread"),
            ("thread.id", "original-handshake-thread"),
            ("turn.id", "original-handshake-turn"),
        ] {
            assert_eq!(
                fields.get(name).and_then(serde_json::Value::as_str),
                Some(expected)
            );
        }
        assert_eq!(
            fields
                .get("http_status")
                .and_then(serde_json::Value::as_u64),
            Some(u64::from(status))
        );
        assert_eq!(
            fields
                .get("retry_advised")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        fields
            .get("body_wait_ms")
            .and_then(serde_json::Value::as_u64)
            .expect("numeric rejection-body wait");
        if let Some(bytes) = body_bytes {
            assert_eq!(
                fields
                    .get("rejection_body_bytes")
                    .and_then(serde_json::Value::as_u64),
                Some(u64::try_from(bytes).unwrap())
            );
        } else {
            assert!(
                !fields.contains_key("rejection_body_bytes"),
                "an unavailable body size must be omitted, not recorded as zero"
            );
        }
        for name in fields.keys() {
            assert!(
                !matches!(
                    name.as_str(),
                    "local_attempt" | "request_id" | "request_bytes"
                ) && !name.starts_with("input_")
                    && !name.starts_with("tool"),
                "handshake has no prepared Responses request metric: {name}"
            );
        }
        let text = format!("{records:?}");
        for forbidden in [
            OPAQUE,
            FORBIDDEN,
            "Bad Request",
            "fixture refusal",
            url,
            "127.0.0.1",
            "private-response-header",
            "x-unrelated",
            "authorization",
        ] {
            assert!(!text.contains(forbidden), "{text}");
        }
        // Keep this regression after the existing status, complete-body,
        // correlation, retry and privacy controls. For truncated delivery the
        // actual parser consumes the whole prefix before observing EOF; this
        // does not infer client progress from a successful server write.
        assert_eq!(
            fields
                .get("rejection_body_bytes_accepted")
                .and_then(serde_json::Value::as_u64),
            Some(u64::try_from(accepted_bytes).unwrap()),
            "accepted entity progress survives incomplete-body suppression"
        );
    }
}

#[derive(Default)]
struct ConnectWake {
    revision: AtomicU64,
    notify: Notify,
}

impl ArcWake for ConnectWake {
    fn wake_by_ref(this: &Arc<Self>) {
        this.revision.fetch_add(1, Ordering::SeqCst);
        this.notify.notify_one();
    }
}

impl ConnectWake {
    fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    async fn after(&self, revision: u64) {
        loop {
            let notified = self.notify.notified();
            if self.revision() != revision {
                return;
            }
            notified.await;
        }
    }
}

fn poll_connect(future: &mut ConnectFuture<'_>, wake: &Arc<ConnectWake>) -> Poll<ConnectResult> {
    let waker = waker_ref(wake);
    future.as_mut().poll(&mut Context::from_waker(&waker))
}

async fn finish_connect(future: &mut ConnectFuture<'_>, wake: &Arc<ConnectWake>) -> ConnectResult {
    tokio::time::timeout(WAIT, async {
        loop {
            let revision = wake.revision();
            match poll_connect(future, wake) {
                Poll::Ready(result) => return result,
                Poll::Pending => wake.after(revision).await,
            }
        }
    })
    .await
    .expect("rejection body and public connect complete")
}

#[derive(Clone, Copy, Debug)]
enum Delivery {
    Complete,
    Split,
    SplitDelayedAdvice,
    // Send complete JSON, declare one extra byte, then close the write half.
    Truncated,
}

struct RejectionFixture {
    uri: String,
    observed: Option<oneshot::Receiver<()>>,
    send_headers: Option<oneshot::Sender<()>>,
    headers_written: Option<oneshot::Receiver<Instant>>,
    send_body: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl RejectionFixture {
    async fn start(status: u16, body: &'static str, delivery: Delivery) -> Self {
        let listener = tokio::time::timeout(WAIT, TcpListener::bind("127.0.0.1:0"))
            .await
            .expect("listener bind deadline")
            .unwrap();
        let uri = format!("http://{}", listener.local_addr().unwrap());
        let (observed, request) = oneshot::channel();
        let (send_headers, header_gate) = oneshot::channel();
        let (headers_written, header_ack) = oneshot::channel();
        let (send_body, body_gate) = oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut socket, _) = tokio::time::timeout(WAIT, listener.accept())
                .await
                .expect("accept deadline")
                .unwrap();
            let request = tokio::time::timeout(WAIT, async {
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                loop {
                    let read = socket.read(&mut buffer).await.unwrap();
                    assert_ne!(read, 0, "complete Upgrade request");
                    request.extend_from_slice(&buffer[..read]);
                    assert!(request.len() < 64 * 1024, "bounded Upgrade request");
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        assert_eq!(end + 4, request.len(), "handshake sends no model payload");
                        break String::from_utf8(request).unwrap();
                    }
                }
            })
            .await
            .expect("Upgrade request deadline");
            assert!(request.starts_with("GET /v1/responses HTTP/1.1\r\n"));
            assert!(
                request
                    .lines()
                    .any(|line| line.eq_ignore_ascii_case("upgrade: websocket"))
            );
            assert!(observed.send(()).is_ok());
            tokio::time::timeout(WAIT, header_gate)
                .await
                .expect("header release deadline")
                .unwrap();
            let reason = if status == 400 {
                "Bad Request"
            } else {
                "Forbidden"
            };
            let headers = format!(
                "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: close\r\nretry-after: 7\r\nx-request-id: req_body-1\r\ncf-ray: ray_body-1\r\nx-unrelated: private-response-header\r\n\r\n",
                body.len() + usize::from(matches!(delivery, Delivery::Truncated)),
            );
            let header_send_at = Instant::now();
            match delivery {
                Delivery::Complete | Delivery::Truncated => {
                    let response = format!("{headers}{body}");
                    tokio::time::timeout(WAIT, socket.write_all(response.as_bytes()))
                        .await
                        .expect("complete rejection write deadline")
                        .unwrap();
                    let _ = headers_written.send(header_send_at);
                }
                Delivery::Split | Delivery::SplitDelayedAdvice => {
                    tokio::time::timeout(WAIT, socket.write_all(headers.as_bytes()))
                        .await
                        .expect("rejection headers write deadline")
                        .unwrap();
                    let _ = headers_written.send(header_send_at);
                    tokio::time::timeout(WAIT, body_gate)
                        .await
                        .expect("body release deadline")
                        .unwrap();
                    // A broken client may already have returned an empty-body
                    // error and closed. Preserve that result for the regression,
                    // rather than replacing it with a fixture write panic.
                    let _ = tokio::time::timeout(WAIT, socket.write_all(body.as_bytes()))
                        .await
                        .expect("split body write deadline");
                }
            }
            let _ = tokio::time::timeout(WAIT, socket.shutdown()).await;
        });
        Self {
            uri,
            observed: Some(request),
            send_headers: Some(send_headers),
            headers_written: Some(header_ack),
            send_body: Some(send_body),
            task: Some(task),
        }
    }

    async fn finish(mut self) {
        match tokio::time::timeout(WAIT, self.task.as_mut().expect("one fixture task")).await {
            Ok(result) => {
                drop(self.task.take());
                result.expect("rejection fixture completion");
            }
            Err(_) => {
                let task = self.task.take().unwrap();
                task.abort();
                let _ = task.await;
                panic!("rejection fixture cleanup deadline");
            }
        }
    }
}

impl Drop for RejectionFixture {
    fn drop(&mut self) {
        if let Some(task) = self.task.as_ref() {
            task.abort();
        }
    }
}

struct PendingRejectionFixture {
    uri: String,
    request: oneshot::Receiver<()>,
    release: Option<oneshot::Sender<()>>,
    delivered: oneshot::Receiver<()>,
    task: Option<JoinHandle<()>>,
}

impl PendingRejectionFixture {
    async fn start(prefix: &'static str) -> Self {
        let listener = tokio::time::timeout(WAIT, TcpListener::bind("127.0.0.1:0"))
            .await
            .expect("pending rejection bind deadline")
            .unwrap();
        let uri = format!("http://{}", listener.local_addr().unwrap());
        let (observed, request) = oneshot::channel();
        let (release, gate) = oneshot::channel();
        let (written, delivered) = oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut socket, _) = tokio::time::timeout(WAIT, listener.accept())
                .await
                .expect("pending rejection accept deadline")
                .unwrap();
            let request = tokio::time::timeout(WAIT, async {
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                loop {
                    let count = socket.read(&mut buffer).await.unwrap();
                    assert_ne!(count, 0, "complete Upgrade request before cancellation");
                    request.extend_from_slice(&buffer[..count]);
                    assert!(request.len() < 64 * 1024, "bounded Upgrade request");
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        assert_eq!(end + 4, request.len(), "no model payload in handshake");
                        break String::from_utf8(request).unwrap();
                    }
                }
            })
            .await
            .expect("pending rejection Upgrade request deadline");
            assert!(request.starts_with("GET /v1/responses HTTP/1.1\r\n"));
            assert!(
                request
                    .lines()
                    .any(|line| line.eq_ignore_ascii_case("upgrade: websocket"))
            );
            observed.send(()).unwrap();
            tokio::time::timeout(WAIT, gate)
                .await
                .expect("pending rejection release deadline")
                .unwrap();
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\ncontent-length: {}\r\nretry-after: 7\r\nx-request-id: req_cancel-1\r\n\r\n{prefix}",
                prefix.len() + 1,
            );
            tokio::time::timeout(WAIT, socket.write_all(response.as_bytes()))
                .await
                .expect("pending rejection delivery deadline")
                .unwrap();
            written.send(()).unwrap();
            // Keep the final declared byte and write-side EOF withheld. Only
            // dropping the public connect future should close this connection.
            let mut unexpected = [0_u8; 1];
            let result = tokio::time::timeout(WAIT, socket.read(&mut unexpected))
                .await
                .expect("cancelled public handshake releases its connection");
            // Closing with unread or newly arriving data may reset TCP
            // (RFC 9293 section 3.6.1). Both outcomes retire this peer.
            match result {
                Ok(0) => {}
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
                Ok(count) => panic!("cancelled handshake sent {count} unexpected bytes"),
                Err(error) => panic!("unexpected cancellation read error: {error:?}"),
            }
        });
        Self {
            uri,
            request,
            release: Some(release),
            delivered,
            task: Some(task),
        }
    }

    async fn finish(mut self) {
        match tokio::time::timeout(WAIT, self.task.as_mut().unwrap()).await {
            Ok(result) => {
                drop(self.task.take());
                result.expect("cancelled rejection fixture joins after peer retirement");
            }
            Err(_) => {
                let task = self.task.take().unwrap();
                task.abort();
                let _ = task.await;
                panic!("cancelled rejection fixture cleanup deadline");
            }
        }
    }
}

impl Drop for PendingRejectionFixture {
    fn drop(&mut self) {
        if let Some(task) = self.task.as_ref() {
            task.abort();
        }
    }
}

struct NoAuth;
impl codex_api::AuthProvider for NoAuth {
    fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        Ok(())
    }
}

async fn rejection(status: u16, body: &'static str, delivery: Delivery, retryable: bool) {
    let truncated = matches!(delivery, Delivery::Truncated);
    let expected_body = (!truncated).then_some(body);
    let mut server = RejectionFixture::start(status, body, delivery).await;
    let client = codex_api::ResponsesWebsocketClient::new(
        codex_api::Provider {
            name: "body-fixture".into(),
            base_url: format!("{}/v1", server.uri),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
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
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let thread_id = "original-handshake-thread";
    let session = tracing::info_span!("session_loop", thread_id = %thread_id);
    let turn = tracing::info_span!(
        parent: &session,
        "turn",
        thread.id = %thread_id,
        turn.id = %"original-handshake-turn"
    );
    let before = Instant::now();
    let mut connect: ConnectFuture<'_> = Box::pin(
        client
            .connect(
                &factory,
                http::HeaderMap::new(),
                http::HeaderMap::new(),
                None,
                None,
            )
            .instrument(turn),
    );
    let wake = Arc::new(ConnectWake::default());
    let mut observed = server.observed.take().unwrap();
    tokio::time::timeout(WAIT, async {
        loop {
            let revision = wake.revision();
            assert!(
                poll_connect(&mut connect, &wake).is_pending(),
                "connect cannot finish while headers are withheld"
            );
            tokio::select! {
                result = &mut observed => { result.unwrap(); break; }
                () = wake.after(revision) => {}
            }
        }
    })
    .await
    .expect("public client sends the actual Upgrade request");
    // With the Upgrade GET observed and the response still withheld, this poll
    // places the public future at its response read. Discard old wake generations.
    assert!(poll_connect(&mut connect, &wake).is_pending());
    let read_checkpoint = wake.revision();
    server.send_headers.take().unwrap().send(()).unwrap();
    let header_send_at = tokio::time::timeout(WAIT, server.headers_written.take().unwrap())
        .await
        .expect("server writes the complete HTTP header block")
        .unwrap();
    tokio::time::timeout(WAIT, wake.after(read_checkpoint))
        .await
        .expect("response read becomes runnable after headers");
    // On this current-thread runtime the body writer cannot run during this
    // explicit header-read poll. An early Err is retained, not discarded/retried.
    // This is a read-poll scheduling barrier, not a TCP application-level ACK.
    let after_headers = poll_connect(&mut connect, &wake);
    let header_poll_at = Instant::now();
    let delayed_advice = matches!(delivery, Delivery::SplitDelayedAdvice);
    if delayed_advice {
        assert!(
            after_headers.is_pending(),
            "connect must await the withheld rejection body"
        );
        // Only the clock advances while the connect future is not being polled
        // and the body gate is closed. Resume before any further socket work:
        // waiting for real I/O while paused could auto-advance its deadlines.
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(3)).await;
        tokio::time::resume();
    }
    if let Some(release) = server.send_body.take() {
        let _ = release.send(());
    }
    let result = match after_headers {
        Poll::Ready(result) => result,
        Poll::Pending => finish_connect(&mut connect, &wake).await,
    };
    let after = Instant::now();
    let expected_url = format!(
        "{}/v1/responses",
        server.uri.replacen("http://", "ws://", 1)
    );
    server.finish().await;
    let error = result.expect_err("server rejects the websocket handshake");
    let advice = match &error {
        ApiError::Transport(TransportError::Http {
            status: actual,
            body: actual_body,
            headers: Some(headers),
            retry_after,
            url,
        }) => {
            assert_eq!(actual.as_u16(), status);
            assert_eq!(
                actual_body.as_deref(),
                expected_body,
                "{delivery:?}: expose only a complete rejected upgrade body"
            );
            assert_eq!(url.as_deref(), Some(expected_url.as_str()));
            assert_eq!(headers["x-request-id"], "req_body-1");
            assert_eq!(headers["cf-ray"], "ray_body-1");
            assert_eq!(headers["x-unrelated"], "private-response-header");
            assert_eq!(headers["retry-after"], "7");
            assert_eq!(
                headers["content-length"],
                (body.len() + usize::from(truncated)).to_string()
            );
            let advice = retry_after.expect("handshake retains provider advice");
            assert!(advice.deadline() >= before + Duration::from_secs(7));
            assert!(advice.deadline() <= after + Duration::from_secs(7));
            if delayed_advice {
                assert!(advice.deadline() >= header_send_at + Duration::from_secs(7));
                assert!(
                    advice.deadline() <= header_poll_at + Duration::from_secs(7),
                    "provider deadline must start at the headers, before the delayed body"
                );
            }
            advice
        }
        error => panic!("handshake rejection lost HTTP identity: {error:?}"),
    };
    let mapped = codex_api::map_api_error(error);
    assert_eq!(
        mapped.retry_after(),
        Some(advice),
        "mapping cannot restart the provider deadline"
    );
    assert_eq!(mapped.retry_delay(1).is_some(), retryable);
    if status == 400 {
        assert_eq!(
            mapped
                .to_string()
                .contains(codex_alab_core::request_error::OPAQUE_BAD_REQUEST_PREFIX),
            !truncated,
            "an incomplete opaque prefix cannot select opaque recovery"
        );
    } else {
        assert_eq!(mapped.http_status_code_value(), Some(403));
    }
    capture.assert_rejection(
        status,
        &expected_url,
        if truncated { "truncated" } else { "complete" },
        expected_body.map(str::len),
        body.len(),
    );
}

async fn run_isolated(name: &str) -> bool {
    const CHILD: &str = "ALAB_WS_REJECTION_BODY_CHILD";
    if std::env::var(CHILD).ok().as_deref() == Some(name) {
        return true;
    }
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &exact, "--nocapture"])
        .env(CHILD, name);
    let report = crate::test_child_process::run(command, CHILD_WAIT)
        .await
        .unwrap_or_else(|failure| panic!("isolated handshake rejection child: {failure}"));
    report.assert_test_success(&exact);
    false
}

#[tokio::test(flavor = "current_thread")]
async fn complete_handshake_rejections_preserve_body_and_retry_policy() {
    if !run_isolated("complete_handshake_rejections_preserve_body_and_retry_policy").await {
        return;
    }
    codex_alab_core::activate();
    rejection(400, OPAQUE, Delivery::Complete, true).await;
    rejection(403, FORBIDDEN, Delivery::Complete, false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn split_opaque_handshake_rejection_preserves_body_and_retry_policy() {
    if !run_isolated("split_opaque_handshake_rejection_preserves_body_and_retry_policy").await {
        return;
    }
    codex_alab_core::activate();
    rejection(400, OPAQUE, Delivery::Split, true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn split_forbidden_handshake_rejection_preserves_body_and_terminal_policy() {
    if !run_isolated("split_forbidden_handshake_rejection_preserves_body_and_terminal_policy").await
    {
        return;
    }
    codex_alab_core::activate();
    rejection(403, FORBIDDEN, Delivery::Split, false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn delayed_handshake_body_preserves_header_receipt_retry_deadline() {
    if !run_isolated("delayed_handshake_body_preserves_header_receipt_retry_deadline").await {
        return;
    }
    codex_alab_core::activate();
    rejection(400, OPAQUE, Delivery::SplitDelayedAdvice, true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn truncated_opaque_handshake_body_cannot_trigger_opaque_recovery() {
    if !run_isolated("truncated_opaque_handshake_body_cannot_trigger_opaque_recovery").await {
        return;
    }
    codex_alab_core::activate();
    rejection(400, OPAQUE, Delivery::Truncated, false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn empty_handshake_rejections_measure_zero_accepted_entity_bytes() {
    if !run_isolated("empty_handshake_rejections_measure_zero_accepted_entity_bytes").await {
        return;
    }
    codex_alab_core::activate();
    rejection(403, "", Delivery::Complete, true).await;
    rejection(403, "", Delivery::Truncated, true).await;
}

async fn cancel_pending_rejection(prefix: &'static str) {
    let mut fixture = PendingRejectionFixture::start(prefix).await;
    let client = codex_api::ResponsesWebsocketClient::new(
        codex_api::Provider {
            name: "cancel-fixture".into(),
            base_url: format!("{}/v1", fixture.uri),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
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
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let session = tracing::info_span!("session_loop", thread_id = "cancel-handshake-thread");
    let turn = tracing::info_span!(
        parent: &session,
        "turn",
        thread.id = "cancel-handshake-thread",
        turn.id = "cancel-handshake-turn"
    );
    let mut connect: ConnectFuture<'_> = Box::pin(
        client
            .connect(
                &factory,
                http::HeaderMap::new(),
                http::HeaderMap::new(),
                None,
                None,
            )
            .instrument(turn),
    );
    let wake = Arc::new(ConnectWake::default());
    tokio::time::timeout(WAIT, async {
        loop {
            let revision = wake.revision();
            assert!(poll_connect(&mut connect, &wake).is_pending());
            tokio::select! {
                result = &mut fixture.request => { result.unwrap(); break; }
                () = wake.after(revision) => {}
            }
        }
    })
    .await
    .expect("public cancellation has an observed Upgrade request");
    assert!(poll_connect(&mut connect, &wake).is_pending());
    let read_checkpoint = wake.revision();
    fixture.release.take().unwrap().send(()).unwrap();
    tokio::time::timeout(WAIT, &mut fixture.delivered)
        .await
        .expect("rejection headers and prefix delivered before cancellation")
        .unwrap();
    tokio::time::timeout(WAIT, wake.after(read_checkpoint))
        .await
        .expect("public response read becomes runnable after delivery");
    assert!(
        poll_connect(&mut connect, &wake).is_pending(),
        "an incomplete rejection cannot complete the public connect call"
    );
    assert!(capture.0.lock().unwrap().is_empty());
    // Server delivery and this read-poll barrier do not measure decoder progress.
    // The scripted adapter tests establish that separate internal boundary.
    drop(connect);
    fixture.finish().await;
    assert!(capture.0.lock().unwrap().is_empty());

    // Finish all real TCP I/O before pausing time, so automatic time advancement
    // cannot race socket readiness. Keep the subscriber alive past the old body
    // capture deadline to catch any completed diagnostic detached from its owner.
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(6)).await;
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    tokio::time::resume();
    assert!(
        capture.0.lock().unwrap().is_empty(),
        "cancellation must not emit a completed or late handshake diagnostic"
    );
    // Only the future was cancelled; these callers outlive the peer retirement observation.
    drop(client);
    drop(factory);
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_public_handshake_retires_connection_without_late_diagnostics() {
    if !run_isolated("cancelled_public_handshake_retires_connection_without_late_diagnostics").await
    {
        return;
    }
    codex_alab_core::activate();
    // A real completed rejection first verifies the same capture layer and
    // active diagnostic path. Cancellation cases each use fresh empty records.
    rejection(400, OPAQUE, Delivery::Complete, true).await;
    for prefix in ["", OPAQUE] {
        cancel_pending_rejection(prefix).await;
    }
}
