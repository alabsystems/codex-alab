// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual endpoint diagnostics through the installed SQLite filter and writer.

use codex_http_client::ByteStream;
use codex_http_client::HttpTransport;
use codex_http_client::Request;
use codex_http_client::RequestBody;
use codex_http_client::Response;
use codex_http_client::StreamResponse;
use codex_http_client::TransportError;
use futures::StreamExt;
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::oneshot;
use tracing_subscriber::prelude::*;

use crate::state_log_filter;

const WAIT: Duration = Duration::from_secs(5);
const THREAD: &str = "endpoint-original-thread";
const TURN: &str = "endpoint-original-turn";
const DIAGNOSTIC_TARGET: &str = "codex_api::endpoint::alab_request_diagnostics";
const SENTINEL_TARGET: &str = "alab_state_contracts::sentinel";
const INPUT_FIELDS: [&str; 6] = [
    "input_items",
    "messages",
    "reasoning_items",
    "tool_calls",
    "tool_outputs",
    "other_items",
];

#[derive(Default)]
struct Failures(Mutex<Vec<String>>);

impl codex_state::log_db::LogWriteFailureReporter for Failures {
    fn report_failure(&self, diagnostic: &str) {
        self.0.lock().unwrap().push(diagnostic.to_owned());
    }
}

#[derive(Clone, Copy, Debug)]
enum Case {
    Inactive,
    OpaqueSetup,
    SseInvalid,
    SseCompleted,
    SseOverloaded { advised: bool },
    SseQuota,
}

impl Case {
    fn is_stream(self) -> bool {
        matches!(
            self,
            Self::SseInvalid | Self::SseCompleted | Self::SseOverloaded { .. } | Self::SseQuota
        )
    }

    fn diagnostic_rows(self) -> usize {
        match self {
            Self::Inactive => 0,
            Self::OpaqueSetup | Self::SseInvalid | Self::SseOverloaded { .. } | Self::SseQuota => 2,
            Self::SseCompleted => 1,
        }
    }
}

fn response_headers() -> http::HeaderMap {
    [
        ("x-request-id", "invalid primary identifier"),
        ("x-oai-request-id", "req_sqlite_endpoint-1"),
        ("cf-ray", "ray_sqlite_endpoint-1"),
        ("authorization", "Bearer never-store-response-header"),
        ("x-unrelated", "never-store-unrelated-header"),
    ]
    .into_iter()
    .map(|(name, value)| (name.parse().unwrap(), value.parse().unwrap()))
    .collect()
}

struct EndpointTransport {
    case: Case,
    gate: Mutex<Option<oneshot::Receiver<()>>>,
    encoded_sizes: Arc<Mutex<Vec<usize>>>,
}

impl HttpTransport for EndpointTransport {
    async fn execute(&self, _: Request) -> Result<Response, TransportError> {
        panic!("Responses endpoint must use the streaming transport");
    }

    async fn stream(&self, request: Request) -> Result<StreamResponse, TransportError> {
        let Some(RequestBody::EncodedJson(body)) = request.body.as_ref() else {
            panic!("public endpoint must supply its encoded request body");
        };
        self.encoded_sizes
            .lock()
            .unwrap()
            .push(body.as_bytes().len());
        if !self.case.is_stream() {
            return Err(TransportError::Http {
                status: http::StatusCode::BAD_REQUEST,
                url: Some("https://never-store-url.invalid/token".into()),
                headers: Some(response_headers()),
                body: Some(r#"{"detail":"Bad Request"}"#.into()),
                retry_after: None,
            });
        }
        let gate = self
            .gate
            .lock()
            .unwrap()
            .take()
            .expect("one streamed response");
        let case = self.case;
        let bytes: ByteStream = futures::stream::once(async move {
            gate.await.expect("caller context has exited");
            let frame: String = match case {
                Case::SseInvalid => concat!(
                    "data: {\"type\":\"response.failed\",\"response\":{\"error\":{",
                    "\"code\":\"invalid_access_program\",\"message\":\"never-store-provider-content\",",
                    "\"headers\":{\"retry-after\":\"7\"}}}}\n\n"
                ).into(),
                Case::SseCompleted => concat!(
                    "data: {\"type\":\"response.failed\",\"response\":{\"error\":{",
                    "\"code\":\"unknown_failure\",\"message\":\"never-store-deferred-content\",",
                    "\"headers\":{\"retry-after\":\"7\"}}}}\n\n",
                    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_sqlite_complete\"}}\n\n"
                ).into(),
                Case::SseOverloaded { advised } => {
                    let mut error = json!({"code":"server_is_overloaded", "message":"never-store-overload-content"});
                    if advised {
                        error["headers"] = json!({"retry-after":"7", "authorization":"Bearer never-store-nested-auth"});
                    }
                    format!("data: {}\n\n", json!({"type":"response.failed", "response":{"error":error}}))
                }
                Case::SseQuota => concat!(
                    "data: {\"type\":\"response.failed\",\"response\":{\"error\":{",
                    "\"code\":\"insufficient_quota\",\"message\":\"never-store-quota-content\"}}}\n\n"
                ).into(),
                _ => unreachable!(),
            };
            Ok(frame.into_bytes().into())
        }).boxed();
        Ok(StreamResponse {
            status: http::StatusCode::OK,
            headers: response_headers(),
            bytes,
        })
    }
}

struct FixtureAuth;

impl codex_api::AuthProvider for FixtureAuth {
    fn add_auth_headers(&self, headers: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        headers.insert(
            "authorization",
            "Bearer never-store-request-header".parse().unwrap(),
        );
        Ok(())
    }
}

fn field<'a>(message: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}=");
    let fields: Vec<_> = message
        .split_whitespace()
        .filter_map(|word| word.strip_prefix(&prefix))
        .collect();
    assert!(fields.len() <= 1, "duplicate field {name}: {message}");
    fields.first().copied()
}

fn numeric(message: &str, name: &str) -> u64 {
    field(message, name)
        .unwrap_or_else(|| panic!("missing numeric {name}: {message}"))
        .parse()
        .unwrap_or_else(|_| panic!("non-numeric {name}: {message}"))
}

fn assert_original_context(row: &codex_state::LogRow) {
    assert_eq!(row.target, DIAGNOSTIC_TARGET);
    assert_persisted_context(row);
}

fn assert_persisted_context(row: &codex_state::LogRow) {
    assert_eq!(row.level, "INFO");
    assert_eq!(row.thread_id.as_deref(), Some(THREAD));
    assert!(row.process_uuid.as_ref().is_some_and(|id| !id.is_empty()));
    let message = row.message.as_deref().unwrap();
    assert!(
        message.contains(&format!("session_loop{{thread_id={THREAD}}}")),
        "{message}"
    );
    assert!(
        message.contains(&format!("turn{{thread.id={THREAD} turn.id={TURN}}}")),
        "{message}"
    );
    for private in [
        "never-store",
        "invalid primary identifier",
        "authorization",
        "x-unrelated",
        "retry-after",
        "invalid_access_program",
        "unknown_failure",
        "server_is_overloaded",
        "insufficient_quota",
    ] {
        assert!(!message.contains(private), "{message}");
    }
}

async fn target_rows(runtime: &codex_state::StateRuntime) -> Vec<codex_state::LogRow> {
    // Deliberately include threadless and other-thread rows. A lost attribution
    // must fail the test, rather than disappear behind the original-thread query.
    runtime
        .query_logs(&codex_state::LogQuery::default())
        .await
        .unwrap()
        .into_iter()
        .filter(|row| row.target == DIAGNOSTIC_TARGET)
        .collect()
}

async fn run_case(case: Case) {
    let home = tempfile::tempdir().unwrap();
    let runtime = codex_state::StateRuntime::init(
        codex_state::SqliteConfig::new_for_testing(home.path().to_path_buf().try_into().unwrap()),
        "test-provider".into(),
    )
    .await
    .unwrap();
    let failures = Arc::new(Failures::default());
    let layer = codex_state::log_db::start(runtime.clone(), failures.clone());
    let (release, gate) = oneshot::channel();
    let encoded_sizes = Arc::new(Mutex::new(Vec::new()));
    let client = codex_api::ResponsesClient::new(
        EndpointTransport {
            case,
            gate: Mutex::new(Some(gate)),
            encoded_sizes: encoded_sizes.clone(),
        },
        codex_api::Provider {
            name: "sqlite-fixture".into(),
            base_url: "https://example.invalid/v1".into(),
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
        Arc::new(FixtureAuth),
    );
    let request = if matches!(case, Case::SseCompleted) {
        json!({"model":"gpt-sqlite-fixture", "input":"never-store-string-input", "tools":null})
    } else {
        json!({
            "model":"gpt-sqlite-fixture",
            "input":[{"type":"message", "role":"user", "content":[{"type":"input_text", "text":"never-store-array-input"}]}],
            "tools":[{"type":"function", "name":"never-store-tool-name", "parameters":{"type":"object"}}]
        })
    };
    let result = {
        let subscriber = tracing_subscriber::registry().with(layer.clone().with_filter(
            state_log_filter::filter(codex_state::log_db::default_filter()),
        ));
        let _subscriber = tracing::subscriber::set_default(subscriber);
        let session = tracing::info_span!("session_loop", thread_id = %THREAD);
        let _session = session.enter();
        let turn = tracing::info_span!("turn", thread.id = %THREAD, turn.id = %TURN);
        let _turn = turn.enter();
        tracing::info!(target: "alab_state_contracts::sentinel", "SQLite INFO sentinel");
        client
            .stream(
                request,
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            )
            .await
    };
    let sizes = encoded_sizes.lock().unwrap().clone();
    assert_eq!(sizes.len(), 1, "one actual endpoint request");
    if case.is_stream() {
        let mut stream = result.unwrap_or_else(|error| panic!("stream must open: {error}"));
        // The parent handles and scoped subscriber are already gone. Flush and
        // inspect the shape before releasing any semantic response event.
        layer.flush().await;
        let before = target_rows(&runtime).await;
        assert_eq!(
            before.len(),
            1,
            "shape only before response release: {before:?}"
        );
        assert_original_context(&before[0]);
        release.send(()).unwrap();
        let (errors, completed) = tokio::time::timeout(WAIT, async {
            let mut errors = Vec::new();
            let mut completed = 0;
            while let Some(event) = stream.rx_event.recv().await {
                match event {
                    Err(error) => errors.push(error),
                    Ok(codex_api::ResponseEvent::Completed { .. }) => completed += 1,
                    _ => {}
                }
            }
            (errors, completed)
        })
        .await
        .expect("semantic response and stream EOF");
        match case {
            Case::SseInvalid => assert!(matches!(
                errors.as_slice(), [codex_api::ApiError::InvalidRequest { message }]
                    if message == "never-store-provider-content"
            )),
            Case::SseOverloaded { advised } => match errors.as_slice() {
                [codex_api::ApiError::ServerOverloaded { retry_after }] => {
                    assert_eq!(retry_after.is_some(), advised);
                }
                _ => panic!("the original typed overload and advice must survive"),
            },
            Case::SseQuota => assert!(matches!(
                errors.as_slice(),
                [codex_api::ApiError::QuotaExceeded]
            )),
            Case::SseCompleted => assert!(
                errors.is_empty(),
                "deferred advised failure must be superseded by completion"
            ),
            _ => unreachable!(),
        }
        assert_eq!(completed, usize::from(matches!(case, Case::SseCompleted)));
    } else {
        assert!(matches!(
            result,
            Err(codex_api::ApiError::Transport(TransportError::Http { status, retry_after, .. }))
                if status == http::StatusCode::BAD_REQUEST && retry_after.is_none()
        ));
        drop(release);
    }
    layer.flush().await;
    assert!(!layer.has_write_failure());
    assert!(failures.0.lock().unwrap().is_empty());
    let all_rows = runtime
        .query_logs(&codex_state::LogQuery::default())
        .await
        .unwrap();
    assert!(
        all_rows.iter().any(|row| row.target == SENTINEL_TARGET
            && row.level == "INFO"
            && row
                .message
                .as_deref()
                .is_some_and(|message| message.contains("SQLite INFO sentinel"))),
        "live SQLite writer: {all_rows:?}"
    );
    let diagnostic_rows: Vec<_> = all_rows
        .iter()
        .filter(|row| row.target == DIAGNOSTIC_TARGET)
        .collect();
    assert_eq!(
        diagnostic_rows.len(),
        case.diagnostic_rows(),
        "{case:?}: {diagnostic_rows:?}"
    );
    let original_rows = runtime
        .query_logs(&codex_state::LogQuery {
            thread_ids: vec![THREAD.into()],
            search: Some("Responses request ".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let original_ids: Vec<_> = original_rows
        .iter()
        .filter(|row| row.target == DIAGNOSTIC_TARGET)
        .map(|row| row.id)
        .collect();
    assert_eq!(
        diagnostic_rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        original_ids
    );
    for row in &diagnostic_rows {
        assert_original_context(row);
    }
    if let Some(shape) = diagnostic_rows.first() {
        let message = shape.message.as_deref().unwrap();
        assert!(message.contains("Responses request shape"));
        for name in ["failure_kind", "http_status", "retry_advised"] {
            assert!(
                field(message, name).is_none(),
                "shape is not a selected failure: {message}"
            );
        }
        assert!(numeric(message, "local_attempt") > 0);
        assert_eq!(
            numeric(message, "request_bytes"),
            u64::try_from(sizes[0]).unwrap()
        );
        if matches!(case, Case::SseCompleted) {
            assert_eq!(field(message, "input_kind"), Some("\"string\""));
            for field_name in INPUT_FIELDS.into_iter().chain(["tools", "tool_bytes"]) {
                assert!(
                    field(message, field_name).is_none(),
                    "unknown metric {field_name}: {message}"
                );
            }
        } else {
            assert_eq!(field(message, "input_kind"), Some("\"array\""));
            for (name, expected) in INPUT_FIELDS.into_iter().zip([1, 1, 0, 0, 0, 0]) {
                assert_eq!(numeric(message, name), expected);
            }
            assert_eq!(numeric(message, "tools"), 1);
            assert!(field(message, "tool_bytes").is_none());
        }
    }
    if diagnostic_rows.len() == 2 {
        let shape = diagnostic_rows[0];
        let failed = diagnostic_rows[1];
        assert_eq!(shape.process_uuid, failed.process_uuid);
        let shape_message = shape.message.as_deref().unwrap();
        let failed_message = failed.message.as_deref().unwrap();
        assert!(failed_message.contains("Responses request failed"));
        assert_eq!(
            numeric(shape_message, "local_attempt"),
            numeric(failed_message, "local_attempt")
        );
        numeric(failed_message, "elapsed_ms");
        assert_eq!(
            field(failed_message, "request_id"),
            Some("\"req_sqlite_endpoint-1\"")
        );
        assert_eq!(
            field(failed_message, "cf_ray"),
            Some("\"ray_sqlite_endpoint-1\"")
        );
        if matches!(case, Case::OpaqueSetup) {
            assert_eq!(numeric(failed_message, "http_status"), 400);
            assert_eq!(field(failed_message, "opaque_bad_request"), Some("true"));
        } else {
            assert!(
                field(failed_message, "http_status").is_none(),
                "opening 200 is not an error status"
            );
            assert_eq!(field(failed_message, "opaque_bad_request"), Some("false"));
        }
        let (kind, advised) = match case {
            Case::OpaqueSetup => ("\"http\"", Some("false")),
            Case::SseInvalid => ("\"invalid_request\"", None),
            Case::SseOverloaded { advised } => (
                "\"server_overloaded\"",
                Some(if advised { "true" } else { "false" }),
            ),
            Case::SseQuota => ("\"quota_exceeded\"", None),
            _ => unreachable!(),
        };
        assert_eq!(field(failed_message, "failure_kind"), Some(kind));
        // SQLite stores formatted tokens. Bool values must persist as true/false,
        // with omission for a type without an advice slot, never Debug Option text.
        assert_eq!(field(failed_message, "retry_advised"), advised);
    }
    drop(client);
    drop(layer);
    runtime.close().await;
}

#[test]
fn public_responses_diagnostics_reach_sqlite_with_original_context() {
    const CHILD: &str = "ALAB_ENDPOINT_SQLITE_DIAGNOSTIC_CHILD";
    if crate::test_process::parent_handled(
        module_path!(),
        "public_responses_diagnostics_reach_sqlite_with_original_context",
        CHILD,
    ) {
        return;
    }
    assert!(!codex_alab_core::active());
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            run_case(Case::Inactive).await;
            codex_alab_core::activate();
            for case in [
                Case::OpaqueSetup,
                Case::SseInvalid,
                Case::SseCompleted,
                Case::SseOverloaded { advised: true },
                Case::SseOverloaded { advised: false },
                Case::SseQuota,
            ] {
                run_case(case).await;
            }
        });
}

// Real HTTP capture records pass through the installed SQLite filter and writer.

const CAPTURE_TARGET: &str = "codex_http_client::response::alab_error_body_capture";
const OPAQUE_HTTP: &str = r#"{"detail":"Bad Request"}"#;
const PRIVATE_FORBIDDEN: &str =
    r#"{"error":{"code":"access_program_not_enabled","message":"never-store-http-provider-body"}}"#;

#[derive(Clone, Copy, Debug)]
enum HttpCaptureCase {
    Inactive,
    Opaque,
    Truncated,
    Forbidden,
}

impl HttpCaptureCase {
    fn active(self) -> bool {
        !matches!(self, Self::Inactive)
    }

    fn status(self) -> u16 {
        if matches!(self, Self::Forbidden) {
            403
        } else {
            400
        }
    }

    fn body(self) -> &'static str {
        if matches!(self, Self::Forbidden) {
            PRIVATE_FORBIDDEN
        } else {
            OPAQUE_HTTP
        }
    }

    fn advised(self) -> bool {
        !matches!(self, Self::Forbidden)
    }
}

fn capture_input() -> serde_json::Value {
    json!({
        "model":"gpt-sqlite-http-capture",
        "input":[{"type":"message", "role":"user", "content":[{"type":"input_text", "text":"never-store-real-http-input"}]}],
        "tools":[]
    })
}

struct CaptureHttpFixture {
    uri: String,
    task: Option<tokio::task::JoinHandle<usize>>,
}

impl CaptureHttpFixture {
    async fn start(case: HttpCaptureCase) -> Self {
        let listener = tokio::time::timeout(WAIT, tokio::net::TcpListener::bind("127.0.0.1:0"))
            .await
            .expect("SQLite HTTP fixture bind deadline")
            .unwrap();
        let uri = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt as _;

            let (mut socket, _) = tokio::time::timeout(WAIT, listener.accept())
                .await
                .expect("SQLite HTTP fixture accept deadline")
                .unwrap();
            let request_bytes = capture_read_post(&mut socket).await;
            let declared = case.body().len()
                + if matches!(case, HttpCaptureCase::Truncated) {
                    1
                } else {
                    0
                };
            let mut headers = response_headers();
            headers.insert("content-length", declared.to_string().parse().unwrap());
            headers.insert("content-type", "application/json".parse().unwrap());
            headers.insert("connection", "close".parse().unwrap());
            if case.advised() {
                headers.insert("retry-after", "7".parse().unwrap());
            }
            let mut response = format!("HTTP/1.1 {} Fixture\r\n", case.status());
            for (name, value) in &headers {
                response.push_str(&format!("{name}: {}\r\n", value.to_str().unwrap()));
            }
            response.push_str("\r\n");
            response.push_str(case.body());
            tokio::time::timeout(WAIT, socket.write_all(response.as_bytes()))
                .await
                .expect("SQLite HTTP response write deadline")
                .unwrap();
            tokio::time::timeout(WAIT, socket.shutdown())
                .await
                .expect("SQLite HTTP close deadline")
                .unwrap();
            request_bytes
        });
        Self {
            uri,
            task: Some(task),
        }
    }

    async fn finish(mut self) -> usize {
        match tokio::time::timeout(WAIT, self.task.as_mut().unwrap()).await {
            Ok(result) => {
                self.task.take();
                result.expect("SQLite HTTP fixture completed")
            }
            Err(_) => {
                let mut task = self.task.take().unwrap();
                task.abort();
                let _ = tokio::time::timeout(WAIT, &mut task)
                    .await
                    .expect("SQLite HTTP fixture abort/reap deadline");
                panic!("SQLite HTTP fixture join deadline");
            }
        }
    }
}

impl Drop for CaptureHttpFixture {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

async fn capture_read_post(socket: &mut tokio::net::TcpStream) -> usize {
    use tokio::io::AsyncReadExt as _;

    tokio::time::timeout(WAIT, async {
        let mut bytes = Vec::new();
        let mut buffer = [0; 1024];
        let end = loop {
            let count = socket.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0, "real HTTP request headers");
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                assert!(end + 4 <= 16 * 1024, "bounded request headers");
                break end + 4;
            }
            assert!(bytes.len() <= 16 * 1024, "bounded request headers");
        };
        let headers = std::str::from_utf8(&bytes[..end]).unwrap();
        assert!(
            headers.starts_with("POST /v1/responses?privacy=never-store-http-url HTTP/1.1\r\n")
        );
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .expect("prepared JSON Content-Length");
        assert!(length <= 64 * 1024, "bounded fixture request body");
        while bytes.len() < end + length {
            let count = socket.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0, "real HTTP request body");
            bytes.extend_from_slice(&buffer[..count]);
            assert!(bytes.len() <= 80 * 1024, "bounded fixture request");
        }
        assert_eq!(bytes.len(), end + length, "one complete POST");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes[end..]).unwrap(),
            capture_input()
        );
        length
    })
    .await
    .expect("complete SQLite HTTP POST deadline")
}

async fn run_http_capture_case(case: HttpCaptureCase) {
    assert_eq!(codex_alab_core::active(), case.active());
    let home = tempfile::tempdir().unwrap();
    let runtime = codex_state::StateRuntime::init(
        codex_state::SqliteConfig::new_for_testing(home.path().to_path_buf().try_into().unwrap()),
        "http-capture-fixture".into(),
    )
    .await
    .unwrap();
    let failures = Arc::new(Failures::default());
    let layer = codex_state::log_db::start(runtime.clone(), failures.clone());
    let fixture = CaptureHttpFixture::start(case).await;
    let transport = codex_http_client::ReqwestTransport::from_http_client(
        codex_http_client::HttpClientBuilder::new()
            .without_request_logging()
            .build_direct()
            .expect("real direct no-proxy fixture client"),
    );
    let client = codex_api::ResponsesClient::new(
        transport,
        codex_api::Provider {
            name: "sqlite-http-capture".into(),
            base_url: format!("{}/v1", fixture.uri),
            query_params: Some(
                [("privacy".into(), "never-store-http-url".into())]
                    .into_iter()
                    .collect(),
            ),
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 0,
                base_delay: Duration::ZERO,
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            stream_idle_timeout: WAIT,
        },
        Arc::new(FixtureAuth),
    );
    let result = {
        let subscriber = tracing_subscriber::registry().with(layer.clone().with_filter(
            state_log_filter::filter(codex_state::log_db::default_filter()),
        ));
        let _subscriber = tracing::subscriber::set_default(subscriber);
        let session = tracing::info_span!("session_loop", thread_id = %THREAD);
        let _session = session.enter();
        let turn = tracing::info_span!("turn", thread.id = %THREAD, turn.id = %TURN);
        let _turn = turn.enter();
        tracing::info!(target: "alab_state_contracts::sentinel", "SQLite INFO sentinel");
        tokio::time::timeout(
            WAIT,
            client.stream(
                capture_input(),
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            ),
        )
        .await
    };
    // Reap the actual TCP fixture before any endpoint or persistence assertion.
    drop(client);
    let request_bytes = fixture.finish().await;
    let error = match result.expect("public Responses error-opening deadline") {
        Err(error) => error,
        Ok(_) => panic!("rejected HTTP response cannot open SSE"),
    };
    let advice = match &error {
        codex_api::ApiError::Transport(TransportError::Http {
            status,
            url: Some(url),
            headers: Some(headers),
            body,
            retry_after,
        }) => {
            assert_eq!(status.as_u16(), case.status());
            assert!(url.contains("never-store-http-url"));
            assert_eq!(headers["x-request-id"], "invalid primary identifier");
            assert_eq!(headers["x-oai-request-id"], "req_sqlite_endpoint-1");
            assert_eq!(headers["cf-ray"], "ray_sqlite_endpoint-1");
            assert_eq!(headers["x-unrelated"], "never-store-unrelated-header");
            assert_eq!(retry_after.is_some(), case.advised());
            assert_eq!(headers.contains_key("retry-after"), case.advised());
            if case.advised() {
                assert_eq!(headers["retry-after"], "7");
            }
            assert_eq!(
                body.as_deref(),
                if matches!(case, HttpCaptureCase::Truncated) {
                    None
                } else {
                    Some(case.body())
                }
            );
            *retry_after
        }
        _ => panic!("original HTTP rejection metadata must survive real capture"),
    };
    let mapped = codex_api::map_api_error(error);
    assert_eq!(mapped.retry_after(), advice, "mapping retains typed advice");
    assert_eq!(
        mapped.retry_delay(1).is_some(),
        matches!(case, HttpCaptureCase::Opaque)
    );
    if matches!(case, HttpCaptureCase::Forbidden) {
        let expected = codex_api::map_api_error(codex_api::ApiError::AccessProgramNotEnabled {
            message: "never-store-http-provider-body".into(),
        });
        assert_eq!(
            std::mem::discriminant(mapped.details()),
            std::mem::discriminant(expected.details()),
            "the complete provider refusal keeps its original terminal class"
        );
        assert_eq!(mapped.http_status_code_value(), Some(403));
        assert!(
            mapped
                .to_string()
                .contains("never-store-http-provider-body")
        );
    }

    layer.flush().await;
    assert!(!layer.has_write_failure());
    assert!(failures.0.lock().unwrap().is_empty());
    let all_rows = runtime
        .query_logs(&codex_state::LogQuery::default())
        .await
        .unwrap();
    assert!(all_rows.iter().any(|row| {
        row.target == SENTINEL_TARGET
            && row.level == "INFO"
            && row
                .message
                .as_deref()
                .is_some_and(|message| message.contains("SQLite INFO sentinel"))
    }));
    let metadata: Vec<_> = all_rows
        .iter()
        .filter(|row| matches!(row.target.as_str(), DIAGNOSTIC_TARGET | CAPTURE_TARGET))
        .collect();
    let original = runtime
        .query_logs(&codex_state::LogQuery {
            thread_ids: vec![THREAD.into()],
            ..Default::default()
        })
        .await
        .unwrap();
    let original_ids: Vec<_> = original
        .iter()
        .filter(|row| matches!(row.target.as_str(), DIAGNOSTIC_TARGET | CAPTURE_TARGET))
        .map(|row| row.id)
        .collect();
    assert_eq!(
        metadata.iter().map(|row| row.id).collect::<Vec<_>>(),
        original_ids
    );
    assert_eq!(metadata.len(), if case.active() { 3 } else { 0 });
    for row in &metadata {
        assert_persisted_context(row);
        let message = row.message.as_deref().unwrap();
        for private in ["Bad Request", "access_program_not_enabled"] {
            assert!(!message.contains(private), "{message}");
        }
    }
    if case.active() {
        let [shape, phase, failed] = metadata.as_slice() else {
            panic!("one shape, capture and selected HTTP failure");
        };
        assert_eq!(shape.target, DIAGNOSTIC_TARGET);
        assert_eq!(phase.target, CAPTURE_TARGET);
        assert_eq!(failed.target, DIAGNOSTIC_TARGET);
        assert_eq!(shape.process_uuid, phase.process_uuid);
        assert_eq!(phase.process_uuid, failed.process_uuid);
        let shape = shape.message.as_deref().unwrap();
        let phase = phase.message.as_deref().unwrap();
        let failed = failed.message.as_deref().unwrap();
        assert!(shape.contains("Responses request shape"));
        assert!(phase.contains("HTTP rejection body capture finished"));
        assert!(failed.contains("Responses request failed"));
        let attempt = numeric(shape, "local_attempt");
        assert!(attempt > 0);
        assert_eq!(numeric(failed, "local_attempt"), attempt);
        assert_eq!(
            numeric(shape, "request_bytes"),
            u64::try_from(request_bytes).unwrap()
        );
        assert_eq!(field(shape, "input_kind"), Some("\"array\""));
        for (name, expected) in INPUT_FIELDS.into_iter().zip([1, 1, 0, 0, 0, 0]) {
            assert_eq!(numeric(shape, name), expected);
        }
        assert_eq!(numeric(shape, "tools"), 0);
        assert!(field(shape, "tool_bytes").is_none());
        for absent in ["failure_kind", "http_status", "retry_advised"] {
            assert!(field(shape, absent).is_none());
        }
        assert_eq!(field(phase, "phase"), Some("\"rejection_body\""));
        assert_eq!(
            field(phase, "body_capture"),
            Some(if matches!(case, HttpCaptureCase::Truncated) {
                "\"read_error\""
            } else {
                "\"complete\""
            })
        );
        numeric(phase, "body_wait_ms");
        if matches!(case, HttpCaptureCase::Truncated) {
            assert!(
                field(phase, "rejection_body_bytes").is_none(),
                "unknown is not measured zero"
            );
        } else {
            assert_eq!(
                numeric(phase, "rejection_body_bytes"),
                u64::try_from(case.body().len()).unwrap()
            );
        }
        for absent in INPUT_FIELDS.into_iter().chain([
            "local_attempt",
            "request_bytes",
            "model",
            "tools",
            "tool_bytes",
            "failure_kind",
            "body",
            "url",
        ]) {
            assert!(field(phase, absent).is_none(), "phase invented {absent}");
        }
        for message in [phase, failed] {
            assert_eq!(numeric(message, "http_status"), u64::from(case.status()));
            assert_eq!(
                field(message, "request_id"),
                Some("\"req_sqlite_endpoint-1\"")
            );
            assert_eq!(field(message, "cf_ray"), Some("\"ray_sqlite_endpoint-1\""));
            assert_eq!(
                field(message, "retry_advised"),
                Some(if advice.is_some() { "true" } else { "false" })
            );
        }
        assert_eq!(field(failed, "failure_kind"), Some("\"http\""));
        assert_eq!(
            field(failed, "opaque_bad_request"),
            Some(if matches!(case, HttpCaptureCase::Opaque) {
                "true"
            } else {
                "false"
            })
        );
        numeric(failed, "elapsed_ms");
    }
    drop(layer);
    runtime.close().await;
}

#[test]
fn public_http_capture_phase_reaches_sqlite_with_original_context() {
    if crate::test_process::parent_handled(
        module_path!(),
        "public_http_capture_phase_reaches_sqlite_with_original_context",
        "ALAB_HTTP_CAPTURE_SQLITE_CONTRACT_CHILD",
    ) {
        return;
    }
    assert!(!codex_alab_core::active());
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            run_http_capture_case(HttpCaptureCase::Inactive).await;
            codex_alab_core::activate();
            for case in [
                HttpCaptureCase::Opaque,
                HttpCaptureCase::Truncated,
                HttpCaptureCase::Forbidden,
            ] {
                run_http_capture_case(case).await;
            }
        });
}
