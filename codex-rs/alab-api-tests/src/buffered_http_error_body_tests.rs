// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Buffered Images rejections retain their HTTP authority when the entity truncates.

use crate::http_retry_diagnostic_tests::Capture;
use codex_api::ApiError;
use codex_api::ImageGenerationRequest;
use codex_api::RequestTelemetry;
use codex_http_client::HttpTransport;
use codex_http_client::Request;
use codex_http_client::TransportError;
use serde_json::Value;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::Instrument;
use tracing_subscriber::prelude::*;

const WAIT: Duration = Duration::from_secs(5);
const OPENING: Duration = Duration::from_millis(6500);
const THREAD: &str = "buffered-capture-thread";
const TURN: &str = "buffered-capture-turn";
const BODY: &str =
    r#"{"error":{"message":"never-log-buffered-provider-content","type":"rate_limit_error"}}"#;
const REQUEST_ID: &str = "req_buffered-1";
const CF_RAY: &str = "ray_buffered-1";
const IMAGEGEN_ID: &str = "imagegen_buffered-1";
const PRIVATE: &str = "never-log-buffered-private-input";

fn input() -> ImageGenerationRequest {
    ImageGenerationRequest {
        prompt: PRIVATE.into(),
        output: None,
        background: None,
        model: "gpt-image-buffered-fixture".into(),
        n: None,
        quality: None,
        size: None,
    }
}

struct NoAuth;
impl codex_api::AuthProvider for NoAuth {
    fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        Ok(())
    }
}

#[derive(Default)]
struct Telemetry(Mutex<Vec<(u64, Option<u16>, bool)>>);
impl RequestTelemetry for Telemetry {
    fn on_request(
        &self,
        attempt: u64,
        status: Option<http::StatusCode>,
        error: Option<&TransportError>,
        _: Duration,
    ) {
        self.0.lock().unwrap().push((
            attempt,
            status.map(|status| status.as_u16()),
            error.is_some(),
        ));
    }
}

struct Plan {
    status: u16,
    body: Vec<u8>,
    declared: usize,
    content_type: &'static str,
    hold: bool,
}
impl Plan {
    fn rejection(truncated: bool) -> Self {
        Self {
            status: 429,
            body: BODY.as_bytes().to_vec(),
            declared: BODY.len() + usize::from(truncated),
            content_type: "application/json",
            hold: false,
        }
    }
    fn complete(body: Vec<u8>) -> Self {
        let declared = body.len();
        Self {
            body,
            declared,
            ..Self::rejection(false)
        }
    }
}

struct Report {
    posts: Vec<Vec<u8>>,
    peer_closed: bool,
}

struct Fixture {
    uri: String,
    prefix_written: oneshot::Receiver<()>,
    done: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Report>>,
    hold: bool,
}

impl Fixture {
    async fn start(truncated: bool) -> Self {
        Self::start_plan(Plan::rejection(truncated)).await
    }

    async fn start_plan(plan: Plan) -> Self {
        let hold = plan.hold;
        let listener = tokio::time::timeout(WAIT, TcpListener::bind("127.0.0.1:0"))
            .await
            .expect("buffered fixture bind deadline")
            .unwrap();
        let uri = format!("http://{}", listener.local_addr().unwrap());
        let (done, mut finished) = oneshot::channel();
        let (prefix_written, written) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut prefix_written = Some(prefix_written);
            let mut posts = Vec::new();
            let mut peer_closed = false;
            loop {
                let accepted = tokio::time::timeout(WAIT, async {
                    tokio::select! {
                        biased;
                        _ = &mut finished => None,
                        accepted = listener.accept() => Some(accepted.unwrap().0),
                    }
                })
                .await
                .expect("buffered fixture accept deadline");
                let Some(mut socket) = accepted else {
                    break;
                };
                posts.push(read_post(&mut socket).await);
                let first = posts.len() == 1;
                // The second rejection is deliberately complete and benign.
                // It makes an unintended replay observable without an image
                // generation, and gives the old client a terminal response.
                let declared = if first { plan.declared } else { BODY.len() };
                let status = if first { plan.status } else { 429 };
                let content_type = if first {
                    plan.content_type
                } else {
                    "application/json"
                };
                let body = if first {
                    plan.body.as_slice()
                } else {
                    BODY.as_bytes()
                };
                let reason = if status == 429 {
                    "Too Many Requests"
                } else {
                    "Fixture"
                };
                let request_id = if first { REQUEST_ID } else { "req_replayed-2" };
                let headers = format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-length: {declared}\r\ncontent-type: {content_type}\r\nconnection: close\r\nretry-after: 7\r\nx-request-id: {request_id}\r\ncf-ray: {CF_RAY}\r\nx-codex-imagegen-request-id: {IMAGEGEN_ID}\r\nx-private-provider: {PRIVATE}\r\n\r\n"
                );
                tokio::time::timeout(WAIT, async {
                    socket.write_all(headers.as_bytes()).await.unwrap();
                    socket.write_all(body).await.unwrap();
                    if !(first && plan.hold) {
                        socket.shutdown().await.unwrap();
                    }
                })
                .await
                .expect("buffered fixture response write deadline");
                if first {
                    // Existing cases need not observe this acknowledgment.
                    // Policy tests revoke only after both writes completed.
                    let _ = prefix_written.take().unwrap().send(());
                }
                if first && plan.hold {
                    // The peer, not this fixture, must end the incomplete body.
                    // Emergency custody includes the separate header barrier,
                    // the caller's 6.5-second observation and a one-second
                    // margin; it never supplies a missing entity suffix.
                    let mut byte = [0];
                    peer_closed = match tokio::time::timeout(
                        WAIT + OPENING + Duration::from_secs(1),
                        socket.read(&mut byte),
                    )
                    .await
                    {
                        Ok(Ok(0)) => true,
                        Ok(Err(error))
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::ConnectionReset
                                    | std::io::ErrorKind::ConnectionAborted
                                    | std::io::ErrorKind::BrokenPipe
                            ) =>
                        {
                            true
                        }
                        Ok(Ok(_)) => panic!("unexpected client bytes during held response"),
                        Ok(Err(error)) => panic!("held response read failed: {error}"),
                        Err(_) => false,
                    };
                    break;
                }
                drop(socket);
                if posts.len() == 2 {
                    break;
                }
            }
            Report { posts, peer_closed }
        });
        Self {
            uri,
            prefix_written: written,
            done: Some(done),
            task: Some(task),
            hold,
        }
    }

    async fn finish(mut self) -> Report {
        // Held cases retain the client while waiting for actual peer closure.
        // Sending the accept-loop stop must not manufacture that observation.
        if !self.hold {
            let _ = self.done.take().unwrap().send(());
        }
        match tokio::time::timeout(WAIT, self.task.as_mut().unwrap()).await {
            Ok(result) => {
                self.task.take();
                result.expect("buffered HTTP fixture completed")
            }
            Err(_) => {
                let task = self.task.take().unwrap();
                task.abort();
                let _ = tokio::time::timeout(WAIT, task).await;
                panic!("buffered HTTP fixture join deadline");
            }
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

async fn read_post(socket: &mut TcpStream) -> Vec<u8> {
    tokio::time::timeout(WAIT, async {
        let mut bytes = Vec::new();
        let mut buffer = [0; 1024];
        let end = loop {
            let count = socket.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0, "real Images request headers");
            bytes.extend_from_slice(&buffer[..count]);
            assert!(bytes.len() <= 16 * 1024, "bounded fixture request headers");
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                break end + 4;
            }
        };
        let headers = std::str::from_utf8(&bytes[..end]).unwrap();
        assert!(headers.starts_with("POST /v1/images/generations HTTP/1.1\r\n"));
        assert!(headers.contains("x-private-caller: never-log-buffered-private-input\r\n"));
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .expect("real Images request Content-Length");
        assert!(length <= 64 * 1024, "bounded fixture request body");
        while bytes.len() < end + length {
            let count = socket.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0, "real Images request body");
            bytes.extend_from_slice(&buffer[..count]);
            assert!(bytes.len() <= end + length, "one complete Images request");
        }
        assert_eq!(bytes.len(), end + length);
        bytes[end..].to_vec()
    })
    .await
    .expect("buffered fixture complete POST deadline")
}

struct Case {
    error: ApiError,
    imagegen_id: Option<String>,
    posts: Vec<Vec<u8>>,
    telemetry: Vec<(u64, Option<u16>, bool)>,
    before: Instant,
    after: Instant,
}

fn transport(logging: bool) -> codex_api::ReqwestTransport {
    let builder = codex_http_client::HttpClientBuilder::new();
    let builder = if logging {
        builder
    } else {
        builder.without_request_logging()
    };
    codex_api::ReqwestTransport::from_http_client(
        builder
            .build_direct()
            .expect("direct no-proxy Images fixture client"),
    )
}

fn client(
    uri: &str,
    telemetry: Arc<Telemetry>,
    max_attempts: u64,
    logging: bool,
) -> codex_api::ImagesClient<codex_api::ReqwestTransport> {
    images_client(
        uri,
        telemetry,
        transport(logging),
        codex_api::RetryConfig {
            max_attempts,
            base_delay: Duration::from_millis(1),
            retry_429: false,
            retry_5xx: false,
            retry_transport: true,
        },
    )
}

fn images_client(
    uri: &str,
    telemetry: Arc<Telemetry>,
    transport: codex_api::ReqwestTransport,
    retry: codex_api::RetryConfig,
) -> codex_api::ImagesClient<codex_api::ReqwestTransport> {
    codex_api::ImagesClient::new(
        transport,
        codex_api::Provider {
            name: "buffered-http-error-fixture".into(),
            base_url: format!("{uri}/v1"),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry,
            stream_idle_timeout: Duration::from_millis(50),
        },
        Arc::new(NoAuth),
    )
    .with_telemetry(Some(telemetry))
}

async fn run_case(truncated: bool, max_attempts: u64) -> Case {
    let fixture = Fixture::start(truncated).await;
    let telemetry = Arc::new(Telemetry::default());
    let client = client(&fixture.uri, telemetry.clone(), max_attempts, false);
    let headers = [("x-private-caller", PRIVATE.parse().unwrap())]
        .into_iter()
        .map(|(name, value)| (name.parse().unwrap(), value))
        .collect();
    let request = input();
    let before = Instant::now();
    let result = tokio::time::timeout(WAIT, client.generate(&request, headers)).await;
    let after = Instant::now();
    drop(client);
    // Finish/reap the real server before any semantic assertion. In particular,
    // a RED replay or missing metadata must not leave its accept task running.
    let posts = fixture.finish().await.posts;
    let (error, imagegen_id) = result
        .expect("public Images call deadline")
        .expect_err("fixture rejects every image generation")
        .into_parts();
    let telemetry = std::mem::take(&mut *telemetry.0.lock().unwrap());
    Case {
        error,
        imagegen_id,
        posts,
        telemetry,
        before,
        after,
    }
}

fn assert_wire(case: &Case) {
    assert!(
        !case.posts.is_empty(),
        "at least one real POST reached the fixture"
    );
    for post in &case.posts {
        assert_eq!(
            serde_json::from_slice::<Value>(post).unwrap(),
            serde_json::to_value(input()).unwrap()
        );
        assert_eq!(
            post, &case.posts[0],
            "retries reuse identical request bytes"
        );
    }
    assert_eq!(
        case.telemetry.len(),
        case.posts.len(),
        "telemetry observes every real POST"
    );
    for (index, (attempt, _, error)) in case.telemetry.iter().enumerate() {
        assert_eq!(*attempt, index as u64);
        assert!(*error);
    }
}

fn assert_http(case: &Case, body: Option<&str>) {
    let ApiError::Transport(TransportError::Http {
        status,
        headers,
        body: actual_body,
        retry_after,
        ..
    }) = &case.error
    else {
        panic!(
            "original HTTP rejection must survive body capture: {:?}",
            case.error
        );
    };
    assert_eq!(*status, http::StatusCode::TOO_MANY_REQUESTS);
    let headers = headers.as_ref().expect("original rejection headers");
    assert_eq!(headers["retry-after"], "7");
    assert_eq!(headers["x-request-id"], REQUEST_ID);
    assert_eq!(headers["cf-ray"], CF_RAY);
    assert_eq!(headers["x-codex-imagegen-request-id"], IMAGEGEN_ID);
    assert_eq!(headers["x-private-provider"], PRIVATE);
    assert_eq!(
        actual_body.as_deref(),
        body,
        "only a complete entity is available"
    );
    assert_eq!(case.imagegen_id.as_deref(), Some(IMAGEGEN_ID));
    let deadline = retry_after
        .expect("header-time typed retry advice")
        .deadline();
    assert!(deadline >= case.before + Duration::from_secs(7));
    assert!(deadline <= case.after + Duration::from_secs(7));
    assert_eq!(case.telemetry, [(0, Some(429), true)]);
}

async fn isolated(name: &str) -> bool {
    const CHILD: &str = "ALAB_BUFFERED_HTTP_ERROR_BODY_CHILD";
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(exact.as_str()));
        return false;
    }
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
        .env(CHILD, &exact);
    crate::test_child_process::run(command, Duration::from_secs(30))
        .await
        .unwrap_or_else(|error| panic!("{exact}: {error}"))
        .assert_test_success(&exact);
    true
}

#[tokio::test(flavor = "current_thread")]
async fn truncated_buffered_rejection_retains_http_advice_and_imagegen_id() {
    if isolated("truncated_buffered_rejection_retains_http_advice_and_imagegen_id").await {
        return;
    }
    codex_alab_core::activate();
    let complete = run_case(false, 0).await;
    assert_wire(&complete);
    assert_eq!(complete.posts.len(), 1);
    assert_http(&complete, Some(BODY));

    let truncated = run_case(true, 0).await;
    assert_wire(&truncated);
    assert_eq!(truncated.posts.len(), 1, "no retries configured");
    assert_http(&truncated, None);
}

#[tokio::test(flavor = "current_thread")]
async fn truncated_429_does_not_replay_images_when_only_transport_retries_are_enabled() {
    if isolated("truncated_429_does_not_replay_images_when_only_transport_retries_are_enabled")
        .await
    {
        return;
    }
    codex_alab_core::activate();
    let complete = run_case(false, 1).await;
    assert_wire(&complete);
    assert_eq!(
        complete.posts.len(),
        1,
        "complete 429 obeys retry_429=false"
    );
    assert_http(&complete, Some(BODY));

    let truncated = run_case(true, 1).await;
    assert_wire(&truncated);
    assert_eq!(
        truncated.posts.len(),
        1,
        "truncated 429 must obey retry_429=false instead of becoming a replayable transport failure"
    );
    assert_http(&truncated, None);
}

struct Captured {
    result:
        Option<Result<(codex_api::ImageResponse, Option<String>), codex_api::ImageRequestError>>,
    report: Report,
    telemetry: Vec<(u64, Option<u16>, bool)>,
    capture: Capture,
    before: Instant,
    after: Instant,
    header_pending_at: Option<Instant>,
}

#[derive(Clone, Default)]
struct HeaderReceipt(Arc<std::sync::atomic::AtomicUsize>);
impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for HeaderReceipt {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        if event.metadata().target() != "codex_http_client::client"
            || *event.metadata().level() != tracing::Level::DEBUG
        {
            return;
        }
        struct Message(bool);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                // Observe only the existing static receipt message, never raw
                // URL/header fields from the DEBUG event.
                if field.name() == "message" {
                    self.0 = format!("{value:?}") == "Request completed";
                }
            }
        }
        let mut message = Message(false);
        event.record(&mut message);
        if message.0 {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

async fn buffered_body_ready<F: std::future::Future>(
    call: &mut std::pin::Pin<Box<F>>,
    written: &mut oneshot::Receiver<()>,
    headers: &HeaderReceipt,
) -> (bool, Option<F::Output>) {
    let mut selected = None;
    let ready = tokio::time::timeout(WAIT, async {
        let written = tokio::select! {
            biased;
            written = written => written.is_ok(),
            result = &mut *call => {
                selected = Some(result);
                false
            }
        };
        if !written {
            return false;
        }
        futures::future::poll_fn(|context| {
            match std::future::Future::poll(call.as_mut(), context) {
                std::task::Poll::Ready(result) => {
                    selected = Some(result);
                    std::task::Poll::Ready(false)
                }
                std::task::Poll::Pending
                    if headers.0.load(std::sync::atomic::Ordering::SeqCst) == 1 =>
                {
                    std::task::Poll::Ready(true)
                }
                std::task::Poll::Pending => std::task::Poll::Pending,
            }
        })
        .await
    })
    .await
    .unwrap_or(false);
    (ready, selected)
}

fn private_headers() -> http::HeaderMap {
    [("x-private-caller", PRIVATE.parse().unwrap())]
        .into_iter()
        .map(|(name, value)| (name.parse().unwrap(), value))
        .collect()
}

async fn captured_images(plan: Plan) -> Captured {
    use std::future::Future;
    let held = plan.hold;
    let fixture = Fixture::start_plan(plan).await;
    let telemetry = Arc::new(Telemetry::default());
    let client = client(&fixture.uri, telemetry.clone(), 0, held);
    let capture = Capture::default();
    let headers = HeaderReceipt::default();
    let subscriber = tracing_subscriber::registry()
        .with(capture.clone())
        .with(headers.clone())
        .with(tracing_subscriber::filter::LevelFilter::DEBUG);
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let thread = tracing::info_span!("session_loop", thread_id = THREAD);
    let turn = tracing::info_span!(parent: &thread, "turn", thread.id = THREAD, turn.id = TURN);
    let request = input();
    let before = Instant::now();
    let mut call = Box::pin(
        client
            .generate(&request, private_headers())
            .instrument(turn),
    );
    let mut early = None;
    let mut header_pending_at = None;
    if held {
        let _ = tokio::time::timeout(
            WAIT,
            futures::future::poll_fn(|cx| match call.as_mut().poll(cx) {
                std::task::Poll::Ready(result) => {
                    early = Some(result);
                    std::task::Poll::Ready(false)
                }
                std::task::Poll::Pending
                    if headers.0.load(std::sync::atomic::Ordering::SeqCst) == 1 =>
                {
                    header_pending_at = Some(Instant::now());
                    std::task::Poll::Ready(true)
                }
                std::task::Poll::Pending => std::task::Poll::Pending,
            }),
        )
        .await;
    }
    let result = match early {
        Some(result) => Some(result),
        None => tokio::time::timeout(if held { OPENING } else { WAIT }, &mut call)
            .await
            .ok(),
    };
    let after = Instant::now();
    drop(call);
    // In the held case, real peer EOF/reset is observed while the pooled client
    // remains alive. The fixture cannot satisfy the body with a late suffix.
    let report = fixture.finish().await;
    drop(client);
    let telemetry = std::mem::take(&mut *telemetry.0.lock().unwrap());
    Captured {
        result,
        report,
        telemetry,
        capture,
        before,
        after,
        header_pending_at,
    }
}

fn rejection(captured: Captured) -> (Case, Capture) {
    let (error, imagegen_id) = captured
        .result
        .expect("public Images opening deadline")
        .expect_err("fixture rejection")
        .into_parts();
    (
        Case {
            error,
            imagegen_id,
            posts: captured.report.posts,
            telemetry: captured.telemetry,
            before: captured.before,
            after: captured.after,
        },
        captured.capture,
    )
}

fn assert_phase(capture: &Capture, outcome: &str, bytes: Option<usize>) {
    let records = capture.0.lock().unwrap();
    assert_eq!(
        records.len(),
        1,
        "one real HTTP capture, no fabricated Responses request"
    );
    let phase = &records[0];
    assert_eq!(phase["message"], "HTTP rejection body capture finished");
    assert_eq!(phase["phase"], "\"rejection_body\"");
    assert_eq!(phase["http_status"], "429");
    assert_eq!(phase["body_capture"], format!("{outcome:?}"));
    assert_eq!(phase["retry_advised"], "true");
    let wait = phase["body_wait_ms"]
        .parse::<u64>()
        .expect("numeric body wait");
    if outcome == "timed_out" {
        assert!(
            (5_000..=6_500).contains(&wait),
            "five-second absolute body budget"
        );
    }
    match bytes {
        Some(bytes) => assert_eq!(phase["rejection_body_bytes"], bytes.to_string()),
        None => assert!(
            !phase.contains_key("rejection_body_bytes"),
            "unknown is not zero"
        ),
    }
    assert_eq!(phase["request_id"], format!("{REQUEST_ID:?}"));
    assert_eq!(phase["cf_ray"], format!("{CF_RAY:?}"));
    assert_eq!(phase["thread_id"], format!("{THREAD:?}"));
    assert_eq!(phase["thread.id"], format!("{THREAD:?}"));
    assert_eq!(phase["turn.id"], format!("{TURN:?}"));
    for absent in [
        "local_attempt",
        "request_bytes",
        "input_items",
        "body",
        "url",
        "x-private-provider",
        "x-private-caller",
    ] {
        assert!(
            !phase.contains_key(absent),
            "capture fabricated/copied {absent}"
        );
    }
    let fields = format!("{records:?}");
    assert!(!fields.contains(PRIVATE));
    assert!(!fields.contains("never-log-buffered-provider-content"));
}

fn body_bytes_read(capture: &Capture) -> Option<usize> {
    let records = capture.0.lock().unwrap();
    assert_eq!(records.len(), 1);
    let phase = &records[0];
    for name in phase.keys() {
        assert!(
            matches!(
                name.as_str(),
                "message"
                    | "phase"
                    | "http_status"
                    | "body_capture"
                    | "body_wait_ms"
                    | "rejection_body_bytes"
                    | "rejection_body_bytes_read"
                    | "retry_advised"
                    | "request_id"
                    | "cf_ray"
                    | "thread_id"
                    | "thread.id"
                    | "turn.id"
                    | "http.method"
                    | "api.path"
            ),
            "unexpected capture field {name}"
        );
    }
    assert_eq!(phase["http.method"], "POST");
    assert_eq!(phase["api.path"], "\"images/generations\"");
    phase
        .get("rejection_body_bytes_read")
        .map(|value| value.parse().expect("numeric decoded-entity byte count"))
}

#[tokio::test(flavor = "current_thread")]
async fn buffered_http_capture_counts_entity_bytes_before_strict_utf8_conversion() {
    if isolated("buffered_http_capture_counts_entity_bytes_before_strict_utf8_conversion").await {
        return;
    }
    codex_alab_core::activate();
    let (complete, capture) = rejection(captured_images(Plan::rejection(false)).await);
    assert_wire(&complete);
    assert_eq!(complete.posts.len(), 1);
    assert_http(&complete, Some(BODY));
    assert_phase(&capture, "complete", Some(BODY.len()));
    let advice = match &complete.error {
        ApiError::Transport(TransportError::Http { retry_after, .. }) => *retry_after,
        _ => unreachable!(),
    };
    let mapped = codex_api::map_api_error(complete.error);
    assert_eq!(mapped.retry_after(), advice);
    assert_eq!(mapped.http_status_code_value(), Some(429));
    assert!(mapped.retry_delay(1).is_some());
    eprintln!("ALAB_BUFFERED_BODY_READ_PROGRESS:case=complete:stage=late_metric");
    assert_eq!(body_bytes_read(&capture), Some(BODY.len()));
    eprintln!("ALAB_BUFFERED_BODY_READ_PROGRESS:case=complete:stage=complete");

    for (bytes, expected) in [(Vec::new(), Some("")), (b"invalid \xff".to_vec(), None)] {
        let length = bytes.len();
        let (case, capture) = rejection(captured_images(Plan::complete(bytes)).await);
        assert_wire(&case);
        assert_eq!(case.posts.len(), 1);
        assert_http(&case, expected);
        assert_phase(&capture, "complete", Some(length));
        assert_eq!(
            body_bytes_read(&capture),
            Some(length),
            "count entity bytes before strict text decoding"
        );
    }
    let (truncated, capture) = rejection(captured_images(Plan::rejection(true)).await);
    assert_wire(&truncated);
    assert_eq!(truncated.posts.len(), 1);
    assert_http(&truncated, None);
    assert_phase(&capture, "read_error", None);
    assert!(
        body_bytes_read(&capture).expect("chunk phase began") <= BODY.len(),
        "successful chunks may be fewer than bytes written to TCP"
    );

    let (declared, capture) = rejection(
        captured_images(Plan {
            declared: 64 * 1024 + 1,
            ..Plan::complete(Vec::new())
        })
        .await,
    );
    assert_wire(&declared);
    assert_eq!(declared.posts.len(), 1);
    assert_http(&declared, None);
    assert_phase(&capture, "too_large", None);
    assert_eq!(
        body_bytes_read(&capture),
        None,
        "declared oversize did not begin body collection"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn complete_buffered_rejections_keep_strict_utf8_decoding_and_measured_bytes() {
    if isolated("complete_buffered_rejections_keep_strict_utf8_decoding_and_measured_bytes").await {
        return;
    }
    codex_alab_core::activate();
    for (bytes, content_type, expected) in [
        (
            b"plain refusal".to_vec(),
            "application/json",
            Some("plain refusal"),
        ),
        (
            b"\xef\xbb\xbfrefusal".to_vec(),
            "application/json",
            Some("\u{feff}refusal"),
        ),
        (b"invalid \xff".to_vec(), "application/json", None),
        (
            b"caf\xe9".to_vec(),
            "text/plain; charset=windows-1252",
            None,
        ),
    ] {
        let length = bytes.len();
        let plan = Plan {
            content_type,
            ..Plan::complete(bytes)
        };
        let (case, capture) = rejection(captured_images(plan).await);
        assert_wire(&case);
        assert_eq!(case.posts.len(), 1);
        assert_http(&case, expected);
        // Invalid UTF-8 does not make a complete byte entity unavailable.
        assert_phase(&capture, "complete", Some(length));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn buffered_success_inactive_errors_and_explicit_limits_keep_their_boundaries() {
    if isolated("buffered_success_inactive_errors_and_explicit_limits_keep_their_boundaries").await
    {
        return;
    }
    assert!(!codex_alab_core::active());
    let (inactive, capture) = rejection(captured_images(Plan::rejection(false)).await);
    assert_wire(&inactive);
    assert_http(&inactive, Some(BODY));
    assert!(capture.0.lock().unwrap().is_empty());
    let (inactive, capture) = rejection(captured_images(Plan::rejection(true)).await);
    assert_wire(&inactive);
    assert_eq!(inactive.posts.len(), 1);
    assert!(
        matches!(
            inactive.error,
            ApiError::Transport(TransportError::Network(_))
        ),
        "inactive truncated error retains original transport classification"
    );
    assert!(inactive.imagegen_id.is_none());
    assert_eq!(inactive.telemetry, [(0, None, true)]);
    assert!(capture.0.lock().unwrap().is_empty());

    codex_alab_core::activate();
    let payload = "x".repeat(64 * 1024 + 1);
    let body = serde_json::to_vec(&serde_json::json!({"created":1,"data":[{"b64_json":payload}]}))
        .unwrap();
    let success = captured_images(Plan {
        status: 200,
        ..Plan::complete(body)
    })
    .await;
    let (image, id) = success
        .result
        .expect("large successful Images deadline")
        .expect("large successful image remains parsed");
    assert_eq!(id.as_deref(), Some(IMAGEGEN_ID));
    assert_eq!(image.created, 1);
    assert_eq!(image.data.len(), 1);
    assert_eq!(image.data[0].b64_json.len(), 64 * 1024 + 1);
    assert!(image.data[0].b64_json.bytes().all(|byte| byte == b'x'));
    assert_eq!(success.report.posts.len(), 1);
    assert_eq!(
        serde_json::from_slice::<Value>(&success.report.posts[0]).unwrap(),
        serde_json::to_value(input()).unwrap()
    );
    assert_eq!(success.telemetry, [(0, Some(200), false)]);
    assert!(
        success.capture.0.lock().unwrap().is_empty(),
        "success has no rejection phase"
    );

    for (truncated, limit) in [(false, 4), (true, BODY.len() + 1)] {
        let plan = if truncated {
            Plan::rejection(true)
        } else {
            Plan {
                declared: 5,
                ..Plan::complete(Vec::new())
            }
        };
        let fixture = Fixture::start_plan(plan).await;
        let transport = transport(false);
        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        let _subscriber = tracing::subscriber::set_default(subscriber);
        let mut request = Request::new(
            http::Method::POST,
            format!("{}/v1/images/generations", fixture.uri),
        )
        .with_json(&input());
        request.headers = private_headers();
        request.response_body_limit_bytes = Some(limit);
        let result = tokio::time::timeout(WAIT, transport.execute(request)).await;
        drop(transport);
        let report = fixture.finish().await;
        assert_eq!(report.posts.len(), 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&report.posts[0]).unwrap(),
            serde_json::to_value(input()).unwrap()
        );
        let error = result
            .expect("explicit buffered limit deadline")
            .expect_err("explicit rejection");
        if truncated {
            let TransportError::Http {
                status,
                headers: Some(headers),
                body,
                retry_after: Some(_),
                ..
            } = error
            else {
                panic!("explicit under-limit read error retains HTTP metadata")
            };
            assert_eq!(status, http::StatusCode::TOO_MANY_REQUESTS);
            assert_eq!(headers["retry-after"], "7");
            assert_eq!(headers["x-request-id"], REQUEST_ID);
            assert!(body.is_none());
        } else {
            assert!(matches!(
                error,
                TransportError::ResponseTooLarge { max_bytes: 4 }
            ));
        }
        assert!(
            capture.0.lock().unwrap().is_empty(),
            "explicit limit does not use new default capture"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn held_buffered_rejection_ends_at_body_budget_without_restarting_advice() {
    if isolated("held_buffered_rejection_ends_at_body_budget_without_restarting_advice").await {
        return;
    }
    codex_alab_core::activate();
    let held = captured_images(Plan {
        hold: true,
        ..Plan::rejection(true)
    })
    .await;
    // Capture all lifecycle facts only after the server has been reaped.
    let header_pending_at = held.header_pending_at;
    let peer_closed = held.report.peer_closed;
    let (case, capture) = rejection(held);
    let header_pending_at = header_pending_at
        .expect("actual headers were received while the incomplete entity remained pending");
    assert!(
        peer_closed,
        "body capture closes real TCP while the client remains alive"
    );
    assert_wire(&case);
    assert_eq!(case.posts.len(), 1);
    assert_http(&case, None);
    let ApiError::Transport(TransportError::Http {
        retry_after: Some(advice),
        ..
    }) = &case.error
    else {
        unreachable!()
    };
    assert!(
        advice.deadline() <= header_pending_at + Duration::from_secs(7),
        "header-time advice did not restart during capture or cleanup"
    );
    assert_phase(&capture, "timed_out", None);
}

#[tokio::test(flavor = "current_thread")]
async fn managed_policy_revocation_ends_buffered_rejection_without_retrying() {
    if isolated("managed_policy_revocation_ends_buffered_rejection_without_retrying").await {
        return;
    }
    codex_alab_core::activate();
    let controller = codex_http_client::NetworkPolicyController::default();
    let policy = controller.policy();
    assert!(controller.publish(
        policy.revision(),
        codex_http_client::DestinationPolicy::Unrestricted,
    ));
    let mut fixture = Fixture::start_plan(Plan {
        hold: true,
        ..Plan::rejection(true)
    })
    .await;
    let factory = codex_http_client::HttpClientFactory::new(
        codex_http_client::OutboundProxyPolicy::ReqwestDefault,
    )
    .with_network_policy(policy.clone().for_current_account());
    let pool = codex_http_client::RouteAwareClientPool::with_builder(
        factory,
        codex_http_client::ClientRouteClass::Api,
        codex_http_client::HttpClientBuilder::new(),
    )
    .with_direct_proxy();
    let telemetry = Arc::new(Telemetry::default());
    let client = images_client(
        &fixture.uri,
        telemetry.clone(),
        codex_api::ReqwestTransport::from_route_aware_client_pool(pool),
        codex_api::RetryConfig {
            max_attempts: 2,
            base_delay: Duration::ZERO,
            retry_429: true,
            retry_5xx: true,
            retry_transport: true,
        },
    );
    let capture = Capture::default();
    let headers = HeaderReceipt::default();
    let subscriber = tracing_subscriber::registry()
        .with(capture.clone())
        .with(headers.clone())
        .with(tracing_subscriber::filter::LevelFilter::DEBUG);
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let thread = tracing::info_span!("session_loop", thread_id = THREAD);
    let turn = tracing::info_span!(parent: &thread, "turn", thread.id = THREAD, turn.id = TURN);
    let request = input();
    let mut call = Box::pin(
        client
            .generate(&request, private_headers())
            .instrument(turn),
    );
    let (ready, mut selected) =
        buffered_body_ready(&mut call, &mut fixture.prefix_written, &headers).await;
    let telemetry_before = telemetry.0.lock().unwrap().clone();
    let capture_before = capture.0.lock().unwrap().len();
    if ready {
        policy.invalidate();
        selected = tokio::time::timeout(Duration::from_secs(2), &mut call)
            .await
            .ok();
    }
    // Readiness failure, early completion and timeout all retain task custody.
    // Drop the actual body owner and reap TCP before any contract assertion;
    // the pooled client stays alive through the peer's EOF/reset observation.
    drop(call);
    let report = fixture.finish().await;
    drop(client);

    assert!(
        ready,
        "both written prefix and received headers precede revocation"
    );
    assert!(telemetry_before.is_empty());
    assert_eq!(capture_before, 0, "held body has no completed capture");
    assert!(report.peer_closed);
    assert_eq!(report.posts.len(), 1);
    assert_eq!(
        serde_json::from_slice::<Value>(&report.posts[0]).unwrap(),
        serde_json::to_value(input()).unwrap()
    );
    assert_eq!(*telemetry.0.lock().unwrap(), [(0, None, true)]);
    assert_eq!(headers.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    let (error, imagegen_id) = selected
        .expect("managed revocation completes within two seconds")
        .expect_err("revoked request cannot generate an image")
        .into_parts();
    assert!(matches!(
        &error,
        ApiError::Transport(TransportError::Policy(
            codex_http_client::NetworkPolicyDenied::Revoked
        ))
    ));
    assert!(imagegen_id.is_none());
    let mapped = codex_api::map_api_error(error);
    assert!(mapped.retry_delay(1).is_none());
    assert!(mapped.retry_after().is_none());
    assert_phase(&capture, "policy_denied", None);
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_buffered_rejection_future_closes_socket_without_selecting_failure() {
    if isolated("dropping_buffered_rejection_future_closes_socket_without_selecting_failure").await
    {
        return;
    }
    codex_alab_core::activate();
    let mut fixture = Fixture::start_plan(Plan {
        hold: true,
        ..Plan::rejection(true)
    })
    .await;
    let telemetry = Arc::new(Telemetry::default());
    let client = client(&fixture.uri, telemetry.clone(), 2, true);
    let capture = Capture::default();
    let headers = HeaderReceipt::default();
    let subscriber = tracing_subscriber::registry()
        .with(capture.clone())
        .with(headers.clone())
        .with(tracing_subscriber::filter::LevelFilter::DEBUG);
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let thread = tracing::info_span!("session_loop", thread_id = THREAD);
    let turn = tracing::info_span!(parent: &thread, "turn", thread.id = THREAD, turn.id = TURN);
    let request = input();
    let mut call = Box::pin(
        client
            .generate(&request, private_headers())
            .instrument(turn),
    );
    let (ready, selected) =
        buffered_body_ready(&mut call, &mut fixture.prefix_written, &headers).await;
    let telemetry_before = telemetry.0.lock().unwrap().clone();
    let capture_before = capture.0.lock().unwrap().len();
    let headers_before = headers.0.load(std::sync::atomic::Ordering::SeqCst);
    let server_pending = !fixture.task.as_ref().unwrap().is_finished();
    // No await follows the Pending observation before the actual owner drops.
    // Cancellation cannot borrow a terminal result from the body timer.
    drop(call);
    let report = fixture.finish().await;
    drop(client);

    assert!(
        ready,
        "written prefix and received headers precede cancellation"
    );
    assert!(
        selected.is_none(),
        "cancellation precedes terminal selection"
    );
    assert!(telemetry_before.is_empty());
    assert_eq!(capture_before, 0);
    assert_eq!(headers_before, 1);
    assert!(server_pending, "the held server had not already completed");
    assert!(
        report.peer_closed,
        "the body owner closes TCP while the client lives"
    );
    assert_eq!(report.posts.len(), 1);
    assert_eq!(
        serde_json::from_slice::<Value>(&report.posts[0]).unwrap(),
        serde_json::to_value(input()).unwrap()
    );
    assert_eq!(headers.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(telemetry.0.lock().unwrap().is_empty());
    assert!(
        capture.0.lock().unwrap().is_empty(),
        "cancellation invents no selected failure or completed body phase"
    );
}
