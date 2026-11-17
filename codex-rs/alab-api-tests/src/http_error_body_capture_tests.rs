// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real Responses HTTP errors retain metadata when default body capture is bounded.

use crate::http_retry_diagnostic_tests::Capture;
use codex_api::ApiError;
use codex_api::RequestTelemetry;
use codex_api::ResponseEvent;
use codex_http_client::HttpTransport;
use codex_http_client::Request;
use codex_http_client::TransportError;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::Instrument;
use tracing_subscriber::prelude::*;

const WAIT: Duration = Duration::from_secs(5);
const OPENING: Duration = Duration::from_millis(6500);
const INACTIVE_CHECK: Duration = Duration::from_millis(5250);
const BUDGET: usize = 64 * 1024;
const OPAQUE: &str = r#"{"detail":"Bad Request"}"#;
const PRIVATE: &str = "never-log-http-error-content";
const REQUEST_ID: &str = "req_http-capture-1";
const CF_RAY: &str = "ray_http-capture-1";
const THREAD: &str = "http-capture-thread";
const TURN: &str = "http-capture-turn";

fn input() -> Value {
    json!({
        "model":"gpt-http-capture", "stream":true,
        "input":[
            {"type":"message", "role":"user", "content":[{"type":"input_text", "text":PRIVATE}]},
            {"type":"function_call", "call_id":"done", "name":"shell", "arguments":"{}"},
            {"type":"function_call_output", "call_id":"done", "output":PRIVATE}
        ]
    })
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
        self.0
            .lock()
            .unwrap()
            .push((attempt, status.map(|s| s.as_u16()), error.is_some()));
    }
}

fn transport() -> codex_api::ReqwestTransport {
    transport_with_header_receipt(false)
}

fn transport_with_header_receipt(observe_headers: bool) -> codex_api::ReqwestTransport {
    let builder = codex_http_client::HttpClientBuilder::new();
    let builder = if observe_headers {
        builder
    } else {
        builder.without_request_logging()
    };
    codex_api::ReqwestTransport::from_http_client(
        builder
            .build_direct()
            .expect("direct no-proxy HTTP fixture client"),
    )
}

fn client(
    uri: &str,
    telemetry: Arc<Telemetry>,
) -> codex_api::ResponsesClient<codex_api::ReqwestTransport> {
    client_with_header_receipt(uri, telemetry, false)
}

fn client_with_header_receipt(
    uri: &str,
    telemetry: Arc<Telemetry>,
    observe_headers: bool,
) -> codex_api::ResponsesClient<codex_api::ReqwestTransport> {
    codex_api::ResponsesClient::new(
        transport_with_header_receipt(observe_headers),
        codex_api::Provider {
            name: "http-error-capture".into(),
            base_url: format!("{uri}/v1"),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 0,
                base_delay: Duration::ZERO,
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            stream_idle_timeout: Duration::from_millis(50),
        },
        Arc::new(NoAuth),
    )
    .with_telemetry(Some(telemetry), None)
}

enum End {
    Close,
    Hold(Vec<u8>),
}

struct Plan {
    status: u16,
    content_type: Option<&'static str>,
    retry_after: Vec<&'static str>,
    correlation_fields: Vec<(&'static str, Vec<u8>)>,
    framing: String,
    body: Vec<u8>,
    minimum_written: usize,
    end: End,
}
impl Plan {
    fn length(body: Vec<u8>, declared: usize, end: End) -> Self {
        Self {
            status: 400,
            content_type: None,
            retry_after: vec!["7"],
            correlation_fields: default_correlation_fields(),
            framing: format!("content-length: {declared}\r\n"),
            minimum_written: body.len(),
            body,
            end,
        }
    }
    fn chunked(body: Vec<u8>, end: End) -> Self {
        Self {
            status: 400,
            content_type: None,
            retry_after: vec!["7"],
            correlation_fields: default_correlation_fields(),
            framing: "transfer-encoding: chunked\r\n".into(),
            minimum_written: body.len(),
            body,
            end,
        }
    }
}

fn default_correlation_fields() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("x-request-id", REQUEST_ID.as_bytes().to_vec()),
        ("cf-ray", CF_RAY.as_bytes().to_vec()),
    ]
}

#[derive(Debug)]
struct Report {
    written: usize,
    peer_closed: bool,
    released: bool,
}

struct Fixture {
    uri: String,
    ready: oneshot::Receiver<()>,
    release: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Report>>,
}
impl Fixture {
    async fn start(plan: Plan) -> Self {
        let listener = tokio::time::timeout(WAIT, TcpListener::bind("127.0.0.1:0"))
            .await
            .expect("HTTP fixture bind deadline")
            .unwrap();
        let uri = format!("http://{}", listener.local_addr().unwrap());
        let (ready, prefix) = oneshot::channel();
        let (release, mut gate) = oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut socket, _) = tokio::time::timeout(WAIT, listener.accept())
                .await
                .expect("HTTP fixture accept deadline")
                .unwrap();
            read_post(&mut socket).await;
            let content_type = plan.content_type.unwrap_or(if plan.status == 200 {
                "text/event-stream"
            } else {
                "application/json"
            });
            let mut headers = format!(
                "HTTP/1.1 {} Fixture\r\n{}content-type: {content_type}\r\nconnection: keep-alive\r\n{}",
                plan.status,
                plan.framing,
                plan.retry_after
                    .iter()
                    .map(|value| format!("retry-after: {value}\r\n"))
                    .collect::<String>(),
            ).into_bytes();
            for (name, value) in &plan.correlation_fields {
                headers.extend_from_slice(name.as_bytes());
                headers.extend_from_slice(b": ");
                headers.extend_from_slice(value);
                headers.extend_from_slice(b"\r\n");
            }
            headers.extend_from_slice(format!("x-unrelated: {PRIVATE}\r\n\r\n").as_bytes());
            tokio::time::timeout(WAIT, socket.write_all(&headers))
                .await
                .expect("HTTP headers write deadline")
                .unwrap();
            let mut ready = Some(ready);
            // A declared oversize can be rejected from headers alone. Signal
            // that permitted boundary before any body write can yield.
            if plan.minimum_written == 0 {
                let _ = ready.take().unwrap().send(());
            }
            let mut written = 0;
            while written < plan.body.len() {
                match tokio::time::timeout(WAIT, socket.write(&plan.body[written..])).await {
                    Ok(Ok(0)) | Ok(Err(_)) if written >= plan.minimum_written => break,
                    Ok(Ok(count)) => {
                        assert_ne!(count, 0);
                        written += count;
                    }
                    Ok(Err(error)) => panic!("HTTP body write failed before its control: {error}"),
                    Err(_) => panic!("HTTP body write deadline"),
                }
            }
            assert!(
                written >= plan.minimum_written,
                "required real wire prefix was sent"
            );
            if let Some(ready) = ready.take() {
                let _ = ready.send(());
            }
            match plan.end {
                End::Close => {
                    let _ = tokio::time::timeout(WAIT, socket.shutdown()).await;
                    Report {
                        written,
                        peer_closed: false,
                        released: false,
                    }
                }
                End::Hold(tail) => {
                    // This intentional gate has no autonomous close timer. Only
                    // the caller's release, real peer closure, or owned task
                    // cancellation ends it; it cannot manufacture EOF at 5s.
                    let mut byte = [0];
                    let mut released = false;
                    let closed = tokio::select! {
                        read = socket.read(&mut byte) => read,
                        release = &mut gate => {
                            release.expect("held HTTP body explicitly released");
                            released = true;
                            tokio::time::timeout(WAIT, socket.write_all(&tail))
                                .await.expect("released HTTP suffix write deadline").unwrap();
                            socket.read(&mut byte).await
                        }
                    };
                    let peer_closed = match closed {
                        Ok(0) => true,
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::ConnectionReset
                                    | std::io::ErrorKind::ConnectionAborted
                                    | std::io::ErrorKind::BrokenPipe
                            ) =>
                        {
                            true
                        }
                        _ => false,
                    };
                    assert!(peer_closed, "one request, followed by actual TCP closure");
                    Report {
                        written,
                        peer_closed,
                        released,
                    }
                }
            }
        });
        Self {
            uri,
            ready: prefix,
            release: Some(release),
            task: Some(task),
        }
    }

    fn release_body(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }

    async fn finish(mut self) -> Report {
        match tokio::time::timeout(WAIT, self.task.as_mut().unwrap()).await {
            Ok(result) => {
                self.task.take();
                result.expect("HTTP fixture completed")
            }
            Err(_) => {
                let task = self.task.take().unwrap();
                task.abort();
                let _ = task.await;
                panic!("HTTP fixture join deadline");
            }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(task) = self.task.as_ref() {
            task.abort();
        }
    }
}

async fn read_post(socket: &mut TcpStream) {
    tokio::time::timeout(WAIT, async {
        let mut bytes = Vec::new();
        let mut buffer = [0; 1024];
        let end = loop {
            let count = socket.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0, "real HTTP request headers");
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                assert!(end + 4 <= BUDGET, "bounded request headers");
                break end + 4;
            }
            assert!(bytes.len() <= BUDGET, "bounded request headers");
        };
        let headers = std::str::from_utf8(&bytes[..end]).unwrap();
        assert!(headers.starts_with("POST /v1/responses HTTP/1.1\r\n"));
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .expect("actual encoded POST Content-Length");
        assert!(length <= BUDGET, "bounded real request body");
        while bytes.len() < end + length {
            let count = socket.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0, "real HTTP request body");
            bytes.extend_from_slice(&buffer[..count]);
            assert!(bytes.len() <= 2 * BUDGET);
        }
        assert_eq!(bytes.len(), end + length, "one complete HTTP request");
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes[end..]).unwrap(),
            input()
        );
    })
    .await
    .expect("complete HTTP POST deadline");
}

enum Gate {
    Wait,
    Deadline,
    ReleaseAfter(Duration),
}
struct ErrorCase {
    error: Option<ApiError>,
    capture: Capture,
    telemetry: Vec<(u64, Option<u16>, bool)>,
    report: Report,
    stayed_pending: bool,
    header_events: usize,
    receipt_window: Option<(tokio::time::Instant, tokio::time::Instant)>,
    release_at: Option<tokio::time::Instant>,
}

async fn error_case(plan: Plan, gate: Gate) -> ErrorCase {
    error_case_with_header_receipt(plan, gate, false).await
}

async fn error_case_with_header_receipt(
    plan: Plan,
    gate: Gate,
    observe_headers: bool,
) -> ErrorCase {
    let mut fixture = Fixture::start(plan).await;
    let telemetry = Arc::new(Telemetry::default());
    let client = client_with_header_receipt(&fixture.uri, telemetry.clone(), observe_headers);
    let capture = Capture::default();
    let headers = HeaderReceipt::default();
    let subscriber = tracing_subscriber::registry()
        .with(capture.clone())
        .with(headers.clone())
        .with(tracing_subscriber::filter::LevelFilter::DEBUG);
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let thread = tracing::info_span!("session_loop", thread_id = THREAD);
    let turn = tracing::info_span!(parent: &thread, "turn", thread.id = THREAD, turn.id = TURN);
    let mut call = Box::pin(
        client
            .stream(
                input(),
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            )
            .instrument(turn),
    );
    tokio::time::timeout(WAIT, async {
        tokio::select! {
            biased;
            ready = &mut fixture.ready => ready.expect("real headers and prefix reached TCP"),
            _ = &mut call => panic!("endpoint returned before the real fixture prefix"),
        }
    })
    .await
    .expect("real HTTP prefix observation deadline");
    let receipt_window = if observe_headers {
        tokio::time::timeout(WAIT, async {
            loop {
                assert!(
                    futures::poll!(&mut call).is_pending(),
                    "the owning API future awaits the held entity"
                );
                if headers.0.load(std::sync::atomic::Ordering::SeqCst) == 1 {
                    break;
                }
                tokio::select! {
                    _ = headers.2.notified() => {}
                    _ = &mut call => panic!("held entity completed before header observation"),
                }
            }
        })
        .await
        .expect("actual client header-receipt deadline");
        assert!(futures::poll!(&mut call).is_pending());
        let upper = tokio::time::Instant::now();
        let receipts = headers.1.lock().unwrap();
        assert_eq!(receipts.len(), 1);
        let lower = receipts[0];
        assert!(lower <= upper);
        Some((lower, upper))
    } else {
        None
    };
    let keep_client = matches!(gate, Gate::Deadline);
    let mut stayed_pending = false;
    let mut release_at = None;
    let outcome = match gate {
        Gate::Wait => tokio::time::timeout(WAIT, &mut call).await,
        Gate::Deadline => {
            assert!(
                futures::poll!(&mut call).is_pending(),
                "incomplete real HTTP entity is pending"
            );
            stayed_pending = true;
            tokio::time::timeout(OPENING, &mut call).await
        }
        Gate::ReleaseAfter(wait) => {
            let first = tokio::time::timeout(wait, &mut call).await;
            stayed_pending = first.is_err();
            release_at = Some(tokio::time::Instant::now());
            fixture.release_body();
            if stayed_pending {
                tokio::time::timeout(WAIT, &mut call).await
            } else {
                first
            }
        }
    };
    // Drop the actual body-owning future, not merely a Pin<&mut> reborrow.
    // In Deadline mode the pooled client stays alive until the server observed
    // TCP closure, proving cancellation/capture did not leave a live response.
    drop(call);
    let report = if keep_client {
        let report = fixture.finish().await;
        drop(client);
        report
    } else {
        drop(client);
        fixture.finish().await
    };
    let error = match outcome {
        Ok(Err(error)) => Some(error),
        Ok(Ok(_)) => panic!("HTTP rejection unexpectedly opened a successful stream"),
        Err(_) => None,
    };
    let observed = std::mem::take(&mut *telemetry.0.lock().unwrap());
    ErrorCase {
        error,
        capture,
        telemetry: observed,
        report,
        stayed_pending,
        header_events: headers.0.load(std::sync::atomic::Ordering::SeqCst),
        receipt_window,
        release_at,
    }
}

fn assert_metadata(case: &ErrorCase, delayed: bool) -> Option<&str> {
    let Some(ApiError::Transport(TransportError::Http {
        status,
        headers: Some(headers),
        body,
        retry_after: Some(advice),
        ..
    })) = case.error.as_ref()
    else {
        panic!("original HTTP status error and advice must survive");
    };
    assert_eq!(*status, http::StatusCode::BAD_REQUEST);
    assert_eq!(headers["x-request-id"], REQUEST_ID);
    assert_eq!(headers["cf-ray"], CF_RAY);
    assert_eq!(headers["retry-after"], "7");
    assert_eq!(headers["x-unrelated"], PRIVATE);
    assert!(
        advice.remaining_delay() <= Duration::from_secs(if delayed { 3 } else { 7 }),
        "advice cannot restart after body capture"
    );
    assert_eq!(case.telemetry, [(0, Some(400), true)]);
    body.as_deref()
}

fn assert_records(case: &ErrorCase, failure: bool, opaque: bool) {
    assert_records_metadata(case, failure, opaque, true);
}

fn assert_records_metadata(case: &ErrorCase, failure: bool, opaque: bool, check_ids: bool) {
    let all_records = case.capture.0.lock().unwrap();
    // Capture also observes the separate HTTP body phase. Keep the original
    // logical endpoint shape/failure contract independent of that event.
    let records: Vec<_> = all_records
        .iter()
        .filter(|record| {
            matches!(
                record.get("message").map(String::as_str),
                Some("Responses request shape" | "Responses request failed")
            )
        })
        .collect();
    assert_eq!(records.len(), if failure { 2 } else { 1 });
    assert_eq!(records[0]["message"], "Responses request shape");
    assert_eq!(records[0]["input_items"], "3");
    assert_eq!(records[0]["tool_calls"], "1");
    assert_eq!(records[0]["tool_outputs"], "1");
    if failure {
        assert_eq!(records[1]["message"], "Responses request failed");
        assert_eq!(records[0]["local_attempt"], records[1]["local_attempt"]);
        if check_ids {
            assert!(records[1]["request_id"].contains(REQUEST_ID));
            assert!(records[1]["cf_ray"].contains(CF_RAY));
        }
        let Some(ApiError::Transport(TransportError::Http { status, .. })) = case.error.as_ref()
        else {
            panic!("a selected HTTP failure retains its original status");
        };
        assert_eq!(records[1]["http_status"], status.as_u16().to_string());
        assert_eq!(records[1]["failure_kind"], "\"http\"");
        assert_eq!(records[1]["opaque_bad_request"], opaque.to_string());
    }
    for record in records.iter() {
        assert_eq!(record["transport"], "\"responses_http\"");
        assert_eq!(record["thread_id"], format!("{THREAD:?}"));
        assert_eq!(record["thread.id"], format!("{THREAD:?}"));
        assert_eq!(record["turn.id"], format!("{TURN:?}"));
    }
    let content_free = format!("{records:?}");
    assert!(!content_free.contains(PRIVATE));
    assert!(!content_free.contains("x-unrelated"));
}

async fn isolated(name: &str) -> bool {
    const CHILD: &str = "ALAB_HTTP_ERROR_BODY_CAPTURE_CHILD";
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(exact.as_str()));
        return false;
    }
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
        .env(CHILD, &exact);
    let report = crate::test_child_process::run(command, Duration::from_secs(30))
        .await
        .unwrap_or_else(|failure| panic!("isolated HTTP capture child: {failure}"));
    report.assert_test_success(&exact);
    true
}

#[tokio::test(flavor = "current_thread")]
async fn default_http_error_body_times_out_without_selecting_an_opaque_prefix() {
    if isolated("default_http_error_body_times_out_without_selecting_an_opaque_prefix").await {
        return;
    }
    codex_alab_core::activate();
    // Old semantic positive control: a complete entity finishes even while its
    // HTTP keep-alive socket remains open. No new capture-phase event required.
    let complete = error_case(
        Plan::length(
            OPAQUE.as_bytes().to_vec(),
            OPAQUE.len(),
            End::Hold(Vec::new()),
        ),
        Gate::Wait,
    )
    .await;
    assert!(complete.report.peer_closed && !complete.report.released);
    assert_eq!(assert_metadata(&complete, false), Some(OPAQUE));
    assert_records(&complete, true, true);

    let held = error_case(
        Plan::length(
            OPAQUE.as_bytes().to_vec(),
            OPAQUE.len() + 1,
            End::Hold(vec![b' ']),
        ),
        Gate::Deadline,
    )
    .await;
    assert_eq!(held.report.written, OPAQUE.len());
    assert!(held.report.peer_closed && !held.report.released);
    assert!(held.stayed_pending);
    if held.error.is_some() {
        assert_metadata(&held, true);
        assert_records(&held, true, false);
    } else {
        assert!(
            held.telemetry.is_empty(),
            "the old request never selected a transport result"
        );
        assert_records(&held, false, false);
    }
    // Expected RED occurs only after real wire/pending/cleanup and old semantic
    // controls: the source without default capture bounds times out at 6.5s.
    assert!(
        held.error.is_some(),
        "default HTTP error body did not return within the capture budget"
    );
    assert!(
        assert_metadata(&held, true).is_none(),
        "an incomplete opaque JSON prefix is not a complete entity"
    );
    assert_phase(&complete.capture, 400, "complete", Some(OPAQUE.len()));
    assert_phase(&held.capture, 400, "timed_out", None);
}

#[tokio::test(flavor = "current_thread")]
async fn incomplete_http_error_entities_preserve_metadata_without_opaque_recovery() {
    if isolated("incomplete_http_error_entities_preserve_metadata_without_opaque_recovery").await {
        return;
    }
    codex_alab_core::activate();
    let chunk = format!("{:x}\r\n{OPAQUE}\r\n", OPAQUE.len()).into_bytes();
    for plan in [
        Plan::length(OPAQUE.as_bytes().to_vec(), OPAQUE.len() + 1, End::Close),
        Plan::chunked(chunk, End::Close),
    ] {
        let case = error_case(plan, Gate::Wait).await;
        assert!(assert_metadata(&case, false).is_none());
        assert_records(&case, true, false);
        assert_phase(&case.capture, 400, "read_error", None);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_default_http_errors_retain_status_ids_and_original_advice() {
    if isolated("oversized_default_http_errors_retain_status_ids_and_original_advice").await {
        return;
    }
    codex_alab_core::activate();
    let mut body = format!("{OPAQUE}{PRIVATE}").into_bytes();
    body.resize(BUDGET + 1, b'x');
    let mut declared = Plan::length(body.clone(), body.len(), End::Close);
    declared.minimum_written = 0; // Declared overflow can close before body receipt.
    let prefix = format!("{:x}\r\n", body.len());
    let mut chunked = prefix.as_bytes().to_vec();
    chunked.extend_from_slice(&body);
    chunked.extend_from_slice(b"\r\n0\r\n\r\n");
    let mut observed = Plan::chunked(chunked, End::Close);
    observed.minimum_written = prefix.len() + BUDGET + 1;
    let mut retained = Vec::new();
    let mut phase_cases = Vec::new();
    for plan in [declared, observed] {
        let case = error_case(plan, Gate::Wait).await;
        let body_present = assert_metadata(&case, false).is_some();
        assert_records(&case, true, false);
        retained.push(body_present);
        phase_cases.push(case);
    }
    // Do not print the oversized body into the bounded child pipes on RED.
    assert!(
        retained.iter().all(|present| !present),
        "over-budget default error entities must be omitted"
    );
    for case in phase_cases {
        assert_phase(&case.capture, 400, "too_large", None);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn inactive_explicit_limit_and_success_stream_boundaries_stay_unchanged() {
    if isolated("inactive_explicit_limit_and_success_stream_boundaries_stay_unchanged").await {
        return;
    }
    assert!(!codex_alab_core::active());
    let prefix = &OPAQUE.as_bytes()[..OPAQUE.len() - 1];
    let inactive = error_case(
        Plan::length(prefix.to_vec(), OPAQUE.len(), End::Hold(vec![b'}'])),
        Gate::ReleaseAfter(INACTIVE_CHECK),
    )
    .await;
    assert!(
        inactive.stayed_pending,
        "inactive capture must not acquire the new default deadline"
    );
    assert!(inactive.report.peer_closed && inactive.report.released);
    assert_eq!(assert_metadata(&inactive, true), Some(OPAQUE));
    assert!(inactive.capture.0.lock().unwrap().is_empty());

    codex_alab_core::activate();
    let mut explicit_plan = Plan::length(b"denied".to_vec(), 6, End::Close);
    // The existing Some(4) policy can also reject from Content-Length alone.
    explicit_plan.minimum_written = 0;
    let fixture = Fixture::start(explicit_plan).await;
    let direct = transport();
    let mut request = Request::new(http::Method::POST, format!("{}/v1/responses", fixture.uri))
        .with_json(&input());
    request.response_body_limit_bytes = Some(4);
    let explicit = tokio::time::timeout(WAIT, direct.stream(request)).await;
    drop(direct);
    fixture.finish().await;
    assert!(matches!(
        explicit,
        Ok(Err(TransportError::ResponseTooLarge { max_bytes: 4 }))
    ));

    let body = format!(": {}\n\ndata: {{\"type\":\"response.created\",\"response\":{{\"id\":\"resp_http_capture\"}}}}\n\ndata: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"resp_http_capture\"}}}}\n\n", "x".repeat(BUDGET + 1)).into_bytes();
    let mut plan = Plan::length(body.clone(), body.len(), End::Close);
    plan.status = 200;
    let fixture = Fixture::start(plan).await;
    let client = client(&fixture.uri, Arc::new(Telemetry::default()));
    let result = tokio::time::timeout(WAIT, async {
        let mut stream = match client
            .stream(
                input(),
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            )
            .await
        {
            Ok(stream) => stream,
            Err(_) => panic!("successful large SSE opens normally"),
        };
        let mut created = 0;
        let mut completed = 0;
        while let Some(event) = stream.next().await {
            match event.expect("successful large SSE event") {
                ResponseEvent::Created { .. } => created += 1,
                ResponseEvent::Completed { response_id, .. } => {
                    assert_eq!(response_id, "resp_http_capture");
                    completed += 1;
                }
                _ => {}
            }
        }
        (created, completed)
    })
    .await;
    drop(client);
    fixture.finish().await;
    assert_eq!(result.expect("successful large SSE deadline"), (1, 1));
}

fn assert_phase(capture: &Capture, status: u16, outcome: &str, bytes: Option<usize>) {
    assert_phase_metadata(capture, status, outcome, bytes, true);
}

fn body_bytes_read(capture: &Capture) -> Option<usize> {
    let records = capture.0.lock().unwrap();
    let phases = records
        .iter()
        .filter(|row| {
            row.get("message").map(String::as_str) == Some("HTTP rejection body capture finished")
        })
        .collect::<Vec<_>>();
    assert_eq!(phases.len(), 1);
    let phase = phases[0];
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
                    | "transport"
                    | "http.method"
                    | "api.path"
                    | "turn.has_state"
            ),
            "unexpected capture field {name}"
        );
    }
    assert_eq!(phase["transport"], "\"responses_http\"");
    assert_eq!(phase["http.method"], "\"POST\"");
    assert_eq!(phase["api.path"], "\"/responses\"");
    assert_eq!(phase["turn.has_state"], "false");
    phase
        .get("rejection_body_bytes_read")
        .map(|value| value.parse().expect("numeric decoded-entity byte count"))
}

#[tokio::test(flavor = "current_thread")]
async fn default_http_capture_counts_entity_bytes_including_the_rejected_chunk() {
    if isolated("default_http_capture_counts_entity_bytes_including_the_rejected_chunk").await {
        return;
    }
    codex_alab_core::activate();
    let complete = error_case(
        Plan::length(
            OPAQUE.as_bytes().to_vec(),
            OPAQUE.len(),
            End::Hold(Vec::new()),
        ),
        Gate::Wait,
    )
    .await;
    assert!(complete.report.peer_closed && !complete.report.released);
    assert_eq!(assert_metadata(&complete, false), Some(OPAQUE));
    assert_records(&complete, true, true);
    assert_phase(&complete.capture, 400, "complete", Some(OPAQUE.len()));

    // Unknown entity length forces actual chunk reads. Its total is exactly one
    // byte above the retained limit, independently of TCP/chunk partitioning.
    let mut body = PRIVATE.as_bytes().to_vec();
    body.resize(BUDGET + 1, b'x');
    let prefix = format!("{:x}\r\n", body.len());
    let mut wire = prefix.as_bytes().to_vec();
    wire.extend_from_slice(&body);
    wire.extend_from_slice(b"\r\n0\r\n\r\n");
    let mut plan = Plan::chunked(wire, End::Close);
    plan.minimum_written = prefix.len() + body.len();
    let mut oversized = error_case(plan, Gate::Wait).await;
    assert!(oversized.report.written >= prefix.len() + body.len());
    assert!(assert_metadata(&oversized, false).is_none());
    assert_records(&oversized, true, false);
    assert_phase(&oversized.capture, 400, "too_large", None);
    let advice = match oversized.error.as_ref().unwrap() {
        ApiError::Transport(TransportError::Http { retry_after, .. }) => *retry_after,
        _ => unreachable!(),
    };
    let mapped = codex_api::map_api_error(oversized.error.take().unwrap());
    assert_eq!(mapped.retry_after(), advice);
    assert!(
        mapped.retry_delay(1).is_none(),
        "unavailable body supplies no opaque recovery authority"
    );
    eprintln!("ALAB_HTTP_BODY_READ_PROGRESS:case=chunked_oversize:stage=late_metric");
    assert_eq!(body_bytes_read(&oversized.capture), Some(BUDGET + 1));
    eprintln!("ALAB_HTTP_BODY_READ_PROGRESS:case=chunked_oversize:stage=complete");

    assert_eq!(body_bytes_read(&complete.capture), Some(OPAQUE.len()));
    let empty = error_case(
        Plan::length(Vec::new(), 0, End::Hold(Vec::new())),
        Gate::Wait,
    )
    .await;
    assert!(empty.report.peer_closed && !empty.report.released);
    assert_eq!(assert_metadata(&empty, false), Some(""));
    assert_records(&empty, true, false);
    assert_phase(&empty.capture, 400, "complete", Some(0));
    assert_eq!(body_bytes_read(&empty.capture), Some(0));

    let mut declared = Plan::length(Vec::new(), BUDGET + 1, End::Close);
    declared.minimum_written = 0;
    let declared = error_case(declared, Gate::Wait).await;
    assert!(assert_metadata(&declared, false).is_none());
    assert_records(&declared, true, false);
    assert_phase(&declared.capture, 400, "too_large", None);
    assert_eq!(body_bytes_read(&declared.capture), None);
    for prefix in [Vec::new(), OPAQUE.as_bytes().to_vec()] {
        let length = prefix.len();
        let truncated = error_case(Plan::length(prefix, length + 1, End::Close), Gate::Wait).await;
        assert!(assert_metadata(&truncated, false).is_none());
        assert_records(&truncated, true, false);
        assert_phase(&truncated.capture, 400, "read_error", None);
        let read = body_bytes_read(&truncated.capture)
            .expect("chunk phase began, even if no data chunk succeeded");
        assert!(
            read <= length,
            "only successful entity chunks count, not all server writes"
        );
    }
}

fn assert_phase_metadata(
    capture: &Capture,
    status: u16,
    outcome: &str,
    bytes: Option<usize>,
    check_ids: bool,
) {
    let all_records = capture.0.lock().unwrap();
    let phases: Vec<_> = all_records
        .iter()
        .filter(|record| {
            record.get("message").map(String::as_str)
                == Some("HTTP rejection body capture finished")
        })
        .collect();
    assert_eq!(phases.len(), 1, "one completed capture observation");
    let phase = phases[0];
    assert_eq!(phase["phase"], "\"rejection_body\"");
    assert_eq!(phase["http_status"], status.to_string());
    assert_eq!(phase["body_capture"], format!("{outcome:?}"));
    assert_eq!(
        phase["retry_advised"], "true",
        "typed known advice, not an Option dump"
    );
    let wait_ms = phase["body_wait_ms"]
        .parse::<u64>()
        .expect("numeric elapsed capture time");
    if outcome == "timed_out" {
        assert!(
            (5_000..=6_500).contains(&wait_ms),
            "the absolute five-second body budget"
        );
    }
    match bytes {
        Some(bytes) => assert_eq!(phase["rejection_body_bytes"], bytes.to_string()),
        None => assert!(
            !phase.contains_key("rejection_body_bytes"),
            "unknown is not measured zero"
        ),
    }
    if check_ids {
        assert!(phase["request_id"].contains(REQUEST_ID));
        assert!(phase["cf_ray"].contains(CF_RAY));
    }
    assert_eq!(phase["thread_id"], format!("{THREAD:?}"));
    assert_eq!(phase["thread.id"], format!("{THREAD:?}"));
    assert_eq!(phase["turn.id"], format!("{TURN:?}"));
    // The phase can inherit its real parent context. It must not invent a
    // logical endpoint attempt or copy request/body/arbitrary-header fields.
    for absent in [
        "local_attempt",
        "model",
        "input_items",
        "tool_calls",
        "tool_outputs",
        "body",
        "url",
        "authorization",
        "x-unrelated",
    ] {
        assert!(
            !phase.contains_key(absent),
            "phase fabricated or copied {absent}"
        );
    }
    let content_free = format!("{all_records:?}");
    assert!(!content_free.contains(PRIVATE));
    assert!(!content_free.contains("x-unrelated"));
}

fn assert_status_metadata(case: &ErrorCase, expected_status: u16) -> Option<&str> {
    let Some(ApiError::Transport(TransportError::Http {
        status,
        headers: Some(headers),
        body,
        retry_after: Some(advice),
        ..
    })) = case.error.as_ref()
    else {
        panic!("original HTTP body and metadata survive");
    };
    assert_eq!(status.as_u16(), expected_status);
    assert_eq!(headers["x-request-id"], REQUEST_ID);
    assert_eq!(headers["cf-ray"], CF_RAY);
    assert_eq!(headers["retry-after"], "7");
    assert_eq!(headers["x-unrelated"], PRIVATE);
    assert!(advice.remaining_delay() <= Duration::from_secs(7));
    assert_eq!(case.telemetry, [(0, Some(expected_status), true)]);
    body.as_deref()
}

#[tokio::test(flavor = "current_thread")]
async fn complete_terminal_http_bodies_and_exact_capture_budget_keep_their_meaning() {
    if isolated("complete_terminal_http_bodies_and_exact_capture_budget_keep_their_meaning").await {
        return;
    }
    codex_alab_core::activate();
    for (status, code) in [
        (403, "access_program_not_enabled"),
        (429, "insufficient_quota"),
    ] {
        let body = json!({"error":{"type":code, "code":code, "message":PRIVATE}}).to_string();
        let mut plan = Plan::length(body.as_bytes().to_vec(), body.len(), End::Close);
        plan.status = status;
        let mut case = error_case(plan, Gate::Wait).await;
        assert_eq!(assert_status_metadata(&case, status), Some(body.as_str()));
        assert_records(&case, true, false);
        assert_phase(&case.capture, status, "complete", Some(body.len()));
        let advice = case.error.as_ref().and_then(|error| match error {
            ApiError::Transport(transport) => transport.retry_after(),
            _ => None,
        });
        let mapped = codex_api::map_api_error(case.error.take().unwrap());
        let expected = codex_api::map_api_error(if status == 403 {
            ApiError::AccessProgramNotEnabled {
                message: PRIVATE.into(),
            }
        } else {
            ApiError::QuotaExceeded
        });
        // Comparing enum discriminants avoids a new direct protocol dependency
        // while still requiring the original specific terminal error class.
        assert_eq!(
            std::mem::discriminant(mapped.details()),
            std::mem::discriminant(expected.details())
        );
        assert_eq!(
            mapped.retry_after(),
            advice,
            "mapping does not restart provider advice"
        );
        assert!(
            mapped.retry_delay(1).is_none(),
            "complete refusal/quota remains terminal"
        );
        if status == 403 {
            assert_eq!(mapped.http_status_code_value(), Some(403));
            assert!(
                mapped.to_string().contains(PRIVATE),
                "provider refusal text survives mapping"
            );
        }
    }
    let body = vec![b'x'; BUDGET];
    let exact = error_case(Plan::length(body, BUDGET, End::Close), Gate::Wait).await;
    let retained = assert_status_metadata(&exact, 400).expect("exact budget is retained");
    assert_eq!(retained.len(), BUDGET);
    assert!(retained.bytes().all(|byte| byte == b'x'));
    assert_records(&exact, true, false);
    assert_phase(&exact.capture, 400, "complete", Some(BUDGET));
}

async fn text_case(body: Vec<u8>, content_type: &'static str) -> (TransportError, Capture) {
    let mut plan = Plan::length(body.clone(), body.len(), End::Close);
    plan.content_type = Some(content_type);
    let fixture = Fixture::start(plan).await;
    let direct = transport();
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let thread = tracing::info_span!("session_loop", thread_id = THREAD);
    let turn = tracing::info_span!(parent: &thread, "turn", thread.id = THREAD, turn.id = TURN);
    let request = Request::new(http::Method::POST, format!("{}/v1/responses", fixture.uri))
        .with_json(&input());
    // Still the actual Reqwest transport; only error entity bytes/charset differ.
    let result = tokio::time::timeout(WAIT, direct.stream(request).instrument(turn)).await;
    drop(direct);
    fixture.finish().await;
    let error = match result {
        Ok(Err(error)) => error,
        _ => panic!("real charset response must remain an HTTP error"),
    };
    (error, capture)
}

#[tokio::test(flavor = "current_thread")]
async fn complete_default_http_capture_preserves_charset_bom_and_replacement_text() {
    if isolated("complete_default_http_capture_preserves_charset_bom_and_replacement_text").await {
        return;
    }
    codex_alab_core::activate();
    // These are the existing Reqwest text contracts, now on the active default
    // status-error capture path. Expected bytes count is before text decoding.
    for (content_type, body, expected) in [
        (
            "text/plain; charset=windows-1252",
            b"caf\xe9".as_slice(),
            "café",
        ),
        (
            "text/plain; charset=utf-8",
            b"error: \xff".as_slice(),
            "error: �",
        ),
        (
            "text/plain; charset=unknown",
            b"error: \xff".as_slice(),
            "error: �",
        ),
        ("text/plain", b"\xef\xbb\xbfdenied".as_slice(), "denied"),
    ] {
        let (error, capture) = text_case(body.to_vec(), content_type).await;
        let TransportError::Http {
            status,
            headers: Some(headers),
            body: Some(text),
            retry_after: Some(advice),
            ..
        } = error
        else {
            panic!("complete text capture retains HTTP body/status/headers/advice");
        };
        assert_eq!(status, http::StatusCode::BAD_REQUEST);
        assert_eq!(text, expected);
        assert_eq!(headers[http::header::CONTENT_TYPE], content_type);
        assert_eq!(headers["x-request-id"], REQUEST_ID);
        assert_eq!(headers["cf-ray"], CF_RAY);
        assert_eq!(headers["retry-after"], "7");
        assert!(advice.remaining_delay() <= Duration::from_secs(7));
        assert_phase(&capture, 400, "complete", Some(body.len()));
        assert_eq!(
            capture.0.lock().unwrap().len(),
            1,
            "direct transport has no fabricated API request events"
        );
    }
}

#[derive(Clone, Default)]
struct HeaderReceipt(
    Arc<std::sync::atomic::AtomicUsize>,
    Arc<Mutex<Vec<tokio::time::Instant>>>,
    Arc<tokio::sync::Notify>,
);

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
                // The existing source message is static. Do not format/store
                // URL, headers, status, or any other field of this DEBUG event.
                if field.name() == "message" {
                    self.0 = format!("{value:?}") == "Request completed";
                }
            }
        }
        let mut message = Message(false);
        event.record(&mut message);
        if message.0 {
            self.1.lock().unwrap().push(tokio::time::Instant::now());
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.2.notify_one();
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn repeated_http_retry_after_is_captured_before_delayed_opaque_body() {
    if isolated("repeated_http_retry_after_is_captured_before_delayed_opaque_body").await {
        return;
    }
    codex_alab_core::activate();
    let held = Duration::from_millis(100);
    // Real TCP and an unpaused clock: header receipt precedes the final JSON
    // byte. This tests API/bridge pacing, not an executed Core retry or host sleep.
    for hints in [
        vec!["7"],
        vec!["3", "7"],
        vec!["invalid", "7"],
        vec!["7", "3"],
    ] {
        let mut plan = Plan::length(
            OPAQUE.as_bytes()[..OPAQUE.len() - 1].to_vec(),
            OPAQUE.len(),
            End::Hold(vec![b'}']),
        );
        plan.retry_after = hints.clone();
        let mut case = error_case_with_header_receipt(plan, Gate::ReleaseAfter(held), true).await;
        assert!(case.stayed_pending);
        assert_eq!(case.header_events, 1);
        assert_eq!(case.report.written, OPAQUE.len() - 1);
        assert!(case.report.released && case.report.peer_closed);
        let (received_lower, captured_upper) = case.receipt_window.unwrap();
        assert!(case.release_at.unwrap() >= captured_upper + held);
        let lower = received_lower + Duration::from_secs(7);
        let upper = captured_upper + Duration::from_secs(7);
        let Some(ApiError::Transport(TransportError::Http {
            status,
            headers: Some(headers),
            body,
            retry_after,
            ..
        })) = case.error.as_ref()
        else {
            panic!("complete real rejection retains its raw HTTP identity");
        };
        assert_eq!(*status, http::StatusCode::BAD_REQUEST);
        assert_eq!(body.as_deref(), Some(OPAQUE));
        assert_eq!(headers["x-request-id"], REQUEST_ID);
        assert_eq!(headers["cf-ray"], CF_RAY);
        assert_eq!(headers["x-unrelated"], PRIVATE);
        assert_eq!(
            headers
                .get_all(http::header::RETRY_AFTER)
                .iter()
                .map(|value| value.to_str().unwrap())
                .collect::<Vec<_>>(),
            hints,
            "the actual response retains each ordered wire header"
        );
        assert_eq!(case.telemetry, [(0, Some(400), true)]);
        let advice = *retry_after;
        let deadline = advice.map(codex_http_client::RetryAfter::deadline);
        assert_records(&case, true, true);
        assert_phase(&case.capture, 400, "complete", Some(OPAQUE.len()));
        let mapped = codex_api::map_api_error(case.error.take().unwrap());
        let stream_control = codex_api::map_api_error(ApiError::Stream("ordinary control".into()));
        assert_eq!(
            std::mem::discriminant(mapped.details()),
            std::mem::discriminant(stream_control.details())
        );
        let display = mapped.to_string();
        let message = display
            .strip_prefix("stream disconnected before completion: ")
            .expect("actual selected Stream display");
        assert_eq!(
            message,
            format!(
                "{}{OPAQUE}",
                codex_alab_core::request_error::OPAQUE_BAD_REQUEST_PREFIX
            )
        );
        assert!(codex_alab_core::request_error::opaque_bad_request_stream(
            message
        ));
        assert_eq!(mapped.http_status_code_value(), None);
        assert_eq!(mapped.retry_after(), advice, "mapping retains exact advice");
        assert!(mapped.retry_delay(1).is_some());
        assert_eq!(
            serde_json::to_value(mapped.to_codex_protocol_error()).unwrap(),
            json!("other")
        );
        eprintln!("ALAB_HTTP_WIRE_RETRY_ADVICE:case={hints:?}:stage=late_bounds");
        let deadline = deadline.expect("valid repeated wire advice is retained");
        assert!(
            (lower..=upper).contains(&deadline),
            "the longest valid wait starts at headers, before the delayed body"
        );
        eprintln!("ALAB_HTTP_WIRE_RETRY_ADVICE:case={hints:?}:stage=complete");
    }
}

const NORMALIZATION_MESSAGE: &str = "Opaque backend HTTP 400 classified as recoverable";

#[derive(Default)]
struct NormalizationFields(BTreeMap<String, String>);

impl tracing::field::Visit for NormalizationFields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if matches!(
            field.name(),
            "message" | "request_id" | "cf_ray" | "thread_id" | "thread.id" | "turn.id"
        ) {
            self.0.insert(field.name().into(), format!("{value:?}"));
        } else {
            self.0
                .insert(field.name().into(), "<unexpected field>".into());
        }
    }
}

#[derive(Clone, Default)]
struct NormalizationCapture(Arc<Mutex<Vec<BTreeMap<String, String>>>>);

impl<S> tracing_subscriber::Layer<S> for NormalizationCapture
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attributes: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        context: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut fields = NormalizationFields::default();
        attributes.record(&mut fields);
        context.span(id).unwrap().extensions_mut().insert(fields);
    }

    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        context: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if event.metadata().target() != "codex_api::api_bridge::alab_request_recovery"
            || *event.metadata().level() != tracing::Level::INFO
        {
            return;
        }
        let mut fields = NormalizationFields::default();
        // Format only the static message, emitted IDs and safe parent identities.
        // Unexpected fields retain names and a fixed sentinel, never their values.
        event.record(&mut fields);
        if fields.0.get("message").map(String::as_str) != Some(NORMALIZATION_MESSAGE) {
            return;
        }
        if let Some(scope) = context.event_scope(event) {
            for span in scope.from_root() {
                if let Some(parent) = span.extensions().get::<NormalizationFields>() {
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

fn logged_ids(row: &BTreeMap<String, String>) -> (Option<String>, Option<String>) {
    let read = |name| {
        row.get(name)
            .map(|value| serde_json::from_str::<String>(value).expect("emitted string ID"))
    };
    (read("request_id"), read("cf_ray"))
}

async fn correlation_exchange(
    label: &str,
    fields: Vec<(&'static str, Vec<u8>)>,
    expected: (Option<&str>, Option<&str>),
) {
    // Exercise real HTTP capture and API normalization, without executing Core
    // recovery, qualifying an application, or counting unrelated connections.
    let mut plan = Plan::length(
        OPAQUE.as_bytes().to_vec(),
        OPAQUE.len(),
        End::Hold(Vec::new()),
    );
    plan.correlation_fields = fields.clone();
    let mut case = error_case(plan, Gate::Wait).await;
    assert_eq!(case.report.written, OPAQUE.len());
    assert!(case.report.peer_closed && !case.report.released);
    assert_eq!(case.telemetry, [(0, Some(400), true)]);
    let Some(ApiError::Transport(TransportError::Http {
        status,
        headers: Some(headers),
        body,
        retry_after: Some(advice),
        ..
    })) = case.error.as_ref()
    else {
        panic!("real complete opaque rejection retains status/body/headers/advice");
    };
    assert_eq!(*status, http::StatusCode::BAD_REQUEST);
    assert_eq!(body.as_deref(), Some(OPAQUE));
    assert_eq!(headers["retry-after"], "7");
    assert_eq!(headers["x-unrelated"], PRIVATE);
    for name in ["x-request-id", "x-oai-request-id", "cf-ray"] {
        assert_eq!(
            headers
                .get_all(name)
                .iter()
                .map(http::HeaderValue::as_bytes)
                .collect::<Vec<_>>(),
            fields
                .iter()
                .filter(|(field, _)| *field == name)
                .map(|(_, value)| value.as_slice())
                .collect::<Vec<_>>(),
            "raw returned correlation headers retain exact ordered bytes"
        );
    }
    let advice = *advice;
    assert!(advice.remaining_delay() <= Duration::from_secs(7));
    assert_records_metadata(&case, true, true, false);
    assert_phase_metadata(&case.capture, 400, "complete", Some(OPAQUE.len()), false);
    let normalization = NormalizationCapture::default();
    let mapped = {
        let subscriber = tracing_subscriber::registry().with(normalization.clone());
        let _subscriber = tracing::subscriber::set_default(subscriber);
        let thread = tracing::info_span!("session_loop", thread_id = THREAD);
        let turn = tracing::info_span!(parent: &thread, "turn", thread.id = THREAD, turn.id = TURN);
        let _entered = turn.enter();
        codex_api::map_api_error(case.error.take().unwrap())
    };
    let stream_control = codex_api::map_api_error(ApiError::Stream("ordinary control".into()));
    assert_eq!(
        std::mem::discriminant(mapped.details()),
        std::mem::discriminant(stream_control.details())
    );
    let display = mapped.to_string();
    let message = display
        .strip_prefix("stream disconnected before completion: ")
        .expect("actual Stream display");
    assert_eq!(
        message,
        format!(
            "{}{OPAQUE}",
            codex_alab_core::request_error::OPAQUE_BAD_REQUEST_PREFIX
        )
    );
    assert!(codex_alab_core::request_error::opaque_bad_request_stream(
        message
    ));
    assert_eq!(mapped.http_status_code_value(), None);
    assert_eq!(mapped.retry_after(), Some(advice));
    assert!(mapped.retry_delay(1).is_some());
    assert_eq!(
        serde_json::to_value(mapped.to_codex_protocol_error()).unwrap(),
        json!("other")
    );

    let records = case.capture.0.lock().unwrap();
    let normalized = normalization.0.lock().unwrap();
    assert_eq!(normalized.len(), 1, "one actual opaque normalization event");
    for row in &*normalized {
        assert_eq!(row["message"], NORMALIZATION_MESSAGE);
        for name in row.keys() {
            assert!(
                matches!(
                    name.as_str(),
                    "message" | "request_id" | "cf_ray" | "thread_id" | "thread.id" | "turn.id"
                ),
                "unexpected normalization field {name}"
            );
        }
        assert_eq!(row["thread_id"], format!("{THREAD:?}"));
        assert_eq!(row["thread.id"], format!("{THREAD:?}"));
        assert_eq!(row["turn.id"], format!("{TURN:?}"));
    }
    for row in records.iter().chain(normalized.iter()) {
        for key in ["body", "url", "authorization", "x-unrelated", "headers"] {
            assert!(!row.contains_key(key), "no raw field {key}");
        }
        let rendered = format!("{row:?}");
        for private in [
            PRIVATE,
            OPAQUE,
            "x-unrelated",
            "invalid id",
            "id_pair,other",
            "req_é",
            "�",
            "\\xff",
        ] {
            assert!(
                !rendered.contains(private),
                "no private or invalid ID content"
            );
        }
        assert!(
            !rendered.contains(&"z".repeat(128)),
            "no truncated overlong ID"
        );
    }
    let mut observed = records
        .iter()
        .filter(|row| {
            matches!(
                row.get("message").map(String::as_str),
                Some("Responses request failed" | "HTTP rejection body capture finished")
            )
        })
        .map(logged_ids)
        .collect::<Vec<_>>();
    assert_eq!(observed.len(), 2);
    observed.extend(normalized.iter().map(logged_ids));
    let expected = (expected.0.map(str::to_owned), expected.1.map(str::to_owned));
    eprintln!("ALAB_HTTP_CORRELATION_IDS:case={label}:stage=late_ids");
    assert_eq!(
        observed,
        vec![expected; 3],
        "all three real emitters retain safe IDs"
    );
    eprintln!("ALAB_HTTP_CORRELATION_IDS:case={label}:stage=complete");
}

#[tokio::test(flavor = "current_thread")]
async fn repeated_http_correlation_fields_preserve_later_safe_ids() {
    if isolated("repeated_http_correlation_fields_preserve_later_safe_ids").await {
        return;
    }
    codex_alab_core::activate();
    correlation_exchange(
        "healthy",
        default_correlation_fields(),
        (Some(REQUEST_ID), Some(CF_RAY)),
    )
    .await;
    for (label, invalid) in [
        ("internal_whitespace", b"invalid id".to_vec()),
        ("overlong", vec![b'z'; 129]),
        ("non_utf8", vec![0xff]),
        ("non_ascii", "req_é".as_bytes().to_vec()),
    ] {
        correlation_exchange(
            label,
            vec![
                ("x-request-id", invalid.clone()),
                ("x-request-id", REQUEST_ID.as_bytes().to_vec()),
                ("x-oai-request-id", b"req_alias".to_vec()),
                ("cf-ray", invalid),
                ("cf-ray", CF_RAY.as_bytes().to_vec()),
            ],
            (Some(REQUEST_ID), Some(CF_RAY)),
        )
        .await;
    }
    correlation_exchange(
        "later_alias",
        vec![
            ("x-oai-request-id", b"invalid id".to_vec()),
            ("x-oai-request-id", REQUEST_ID.as_bytes().to_vec()),
            ("cf-ray", CF_RAY.as_bytes().to_vec()),
        ],
        (Some(REQUEST_ID), Some(CF_RAY)),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn http_correlation_field_controls_preserve_safe_priority_and_omission() {
    if isolated("http_correlation_field_controls_preserve_safe_priority_and_omission").await {
        return;
    }
    codex_alab_core::activate();
    correlation_exchange(
        "first_valid",
        vec![
            ("x-request-id", REQUEST_ID.as_bytes().to_vec()),
            ("x-request-id", b"req_second".to_vec()),
            ("x-oai-request-id", b"req_alias".to_vec()),
            ("cf-ray", CF_RAY.as_bytes().to_vec()),
            ("cf-ray", b"ray_second".to_vec()),
        ],
        (Some(REQUEST_ID), Some(CF_RAY)),
    )
    .await;
    correlation_exchange(
        "alias_fallback",
        vec![
            ("x-request-id", b"invalid id".to_vec()),
            ("x-oai-request-id", REQUEST_ID.as_bytes().to_vec()),
            ("cf-ray", CF_RAY.as_bytes().to_vec()),
        ],
        (Some(REQUEST_ID), Some(CF_RAY)),
    )
    .await;
    let mut invalid = Vec::new();
    for name in ["x-request-id", "x-oai-request-id", "cf-ray"] {
        for value in [
            b"invalid id".to_vec(),
            vec![b'z'; 129],
            vec![0xff],
            "req_é".as_bytes().to_vec(),
            b"id_pair,other".to_vec(),
        ] {
            invalid.push((name, value));
        }
    }
    correlation_exchange("all_invalid_whole_values", invalid, (None, None)).await;
    correlation_exchange("absent", Vec::new(), (None, None)).await;
    let boundary = "r".repeat(128);
    correlation_exchange(
        "valid_128_bytes",
        vec![
            ("x-request-id", boundary.as_bytes().to_vec()),
            ("cf-ray", boundary.as_bytes().to_vec()),
        ],
        (Some(&boundary), Some(&boundary)),
    )
    .await;
}

fn managed_client(
    uri: &str,
    policy: codex_http_client::NetworkPolicy,
    telemetry: Arc<Telemetry>,
) -> codex_api::ResponsesClient<codex_api::ReqwestTransport> {
    let factory = codex_http_client::HttpClientFactory::new(
        codex_http_client::OutboundProxyPolicy::ReqwestDefault,
    )
    .with_network_policy(policy.for_current_account());
    let pool = codex_http_client::RouteAwareClientPool::with_builder(
        factory,
        codex_http_client::ClientRouteClass::Api,
        // Enable only the existing DEBUG header-receipt barrier. There is no
        // formatting layer or raw-event collector in the test subscriber.
        codex_http_client::HttpClientBuilder::new(),
    )
    .with_direct_proxy();
    codex_api::ResponsesClient::new(
        codex_api::ReqwestTransport::from_route_aware_client_pool(pool),
        codex_api::Provider {
            name: "http-error-capture-policy".into(),
            base_url: format!("{uri}/v1"),
            query_params: None,
            headers: http::HeaderMap::new(),
            // A Policy error must terminate even when other transport failures
            // are configured to retry. NoAuth still supplies no credentials.
            retry: codex_api::RetryConfig {
                max_attempts: 2,
                base_delay: Duration::ZERO,
                retry_429: true,
                retry_5xx: true,
                retry_transport: true,
            },
            stream_idle_timeout: Duration::from_millis(50),
        },
        Arc::new(NoAuth),
    )
    .with_telemetry(Some(telemetry), None)
}

fn assert_interruption_records(capture: &Capture, revoked: bool) {
    let records = capture.0.lock().unwrap();
    // The current fixture's Capture may also collect the new transport-phase
    // target. Count endpoint selections independently from that phase event.
    let endpoint = records
        .iter()
        .filter(|record| {
            matches!(
                record.get("message").map(String::as_str),
                Some("Responses request shape" | "Responses request failed")
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(endpoint.len(), if revoked { 2 } else { 1 });
    assert_eq!(endpoint[0]["message"], "Responses request shape");
    assert_eq!(endpoint[0]["input_items"], "3");
    assert_eq!(endpoint[0]["tool_calls"], "1");
    assert_eq!(endpoint[0]["tool_outputs"], "1");
    if revoked {
        assert_eq!(endpoint[1]["message"], "Responses request failed");
        assert_eq!(endpoint[0]["local_attempt"], endpoint[1]["local_attempt"]);
        assert_eq!(endpoint[1]["failure_kind"], "\"network_policy\"");
        assert!(!endpoint[1].contains_key("http_status"));
        assert!(!endpoint[1].contains_key("retry_advised"));
    }
    for record in endpoint {
        assert_eq!(record["transport"], "\"responses_http\"");
        assert_eq!(record["thread_id"], format!("{THREAD:?}"));
        assert_eq!(record["thread.id"], format!("{THREAD:?}"));
        assert_eq!(record["turn.id"], format!("{TURN:?}"));
    }
    let content_free = format!("{records:?}");
    assert!(!content_free.contains(PRIVATE));
    assert!(!content_free.contains("x-unrelated"));
    assert!(!content_free.contains("Bad Request"));
}

async fn interrupt_held_error_body(revoke: bool) {
    let mut fixture = Fixture::start(Plan::length(
        OPAQUE.as_bytes().to_vec(),
        OPAQUE.len() + 1,
        End::Hold(vec![b' ']),
    ))
    .await;
    let controller = codex_http_client::NetworkPolicyController::default();
    let policy = controller.policy();
    assert!(controller.publish(
        policy.revision(),
        codex_http_client::DestinationPolicy::Unrestricted,
    ));
    let telemetry = Arc::new(Telemetry::default());
    let client = managed_client(&fixture.uri, policy.clone(), telemetry.clone());
    let capture = Capture::default();
    let headers = HeaderReceipt::default();
    let subscriber = tracing_subscriber::registry()
        .with(capture.clone())
        .with(headers.clone())
        .with(tracing_subscriber::filter::LevelFilter::DEBUG);
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let thread = tracing::info_span!("session_loop", thread_id = THREAD);
    let turn = tracing::info_span!(parent: &thread, "turn", thread.id = THREAD, turn.id = TURN);
    let mut call = Box::pin(
        client
            .stream(
                input(),
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            )
            .instrument(turn),
    );
    tokio::time::timeout(WAIT, async {
        tokio::select! {
            biased;
            ready = &mut fixture.ready => ready.expect("real headers and opaque prefix sent"),
            _ = &mut call => panic!("held entity returned before the wire prefix"),
        }
        futures::future::poll_fn(|context| {
            assert!(
                std::future::Future::poll(call.as_mut(), context).is_pending(),
                "the body-owning call remains pending after header receipt"
            );
            if headers.0.load(std::sync::atomic::Ordering::SeqCst) == 1 {
                std::task::Poll::Ready(())
            } else {
                std::task::Poll::Pending
            }
        })
        .await;
    })
    .await
    .expect("wire prefix plus actual client header-receipt deadline");
    assert!(futures::poll!(&mut call).is_pending());
    assert_interruption_records(&capture, false);
    assert!(telemetry.0.lock().unwrap().is_empty());

    let selected = if revoke {
        policy.invalidate();
        match tokio::time::timeout(Duration::from_secs(2), &mut call)
            .await
            .expect("managed revocation wins before the five-second body timeout")
        {
            Err(error) => Some(error),
            Ok(_) => panic!("revoked request cannot open a response stream"),
        }
    } else {
        None
    };
    // Drop the owning Box, not a Pin reborrow. Keep the pooled client alive
    // through the real server's EOF/reset observation in both controls.
    drop(call);
    let report = fixture.finish().await;
    assert_eq!(report.written, OPAQUE.len());
    assert!(report.peer_closed && !report.released);
    assert_eq!(headers.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_interruption_records(&capture, revoke);

    if revoke {
        let error = selected.expect("one selected policy failure");
        assert!(matches!(
            error,
            ApiError::Transport(TransportError::Policy(
                codex_http_client::NetworkPolicyDenied::Revoked
            ))
        ));
        assert_eq!(*telemetry.0.lock().unwrap(), [(0, None, true)]);
        assert!(codex_api::map_api_error(error).retry_delay(1).is_none());
        assert_phase(&capture, 400, "policy_denied", None);
    } else {
        assert!(selected.is_none());
        assert!(telemetry.0.lock().unwrap().is_empty());
        assert!(
            capture.0.lock().unwrap().iter().all(|record| {
                record.get("message").map(String::as_str)
                    != Some("HTTP rejection body capture finished")
            }),
            "cancellation does not manufacture a completed capture observation"
        );
    }
    drop(client);
}

#[tokio::test(flavor = "current_thread")]
async fn managed_policy_revocation_ends_held_http_rejection_before_body_deadline() {
    if isolated("managed_policy_revocation_ends_held_http_rejection_before_body_deadline").await {
        return;
    }
    codex_alab_core::activate();
    interrupt_held_error_body(true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_http_rejection_future_closes_socket_while_client_remains_alive() {
    if isolated("dropping_http_rejection_future_closes_socket_while_client_remains_alive").await {
        return;
    }
    codex_alab_core::activate();
    interrupt_held_error_body(false).await;
}
