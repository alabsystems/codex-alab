// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real rejected Upgrade diagnostics through the installed SQLite filter and writer.

use crate::state_log_filter;
use codex_api::ApiError;
use codex_api::TransportError;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing_subscriber::prelude::*;

const WAIT: Duration = Duration::from_secs(5);
const THREAD: &str = "handshake-original-thread";
const TURN: &str = "handshake-original-turn";
const TARGET: &str = "codex_api::endpoint::alab_request_diagnostics";
const CAPTURE_TARGET: &str = "codex_http_client::response::alab_error_body_capture";
const SENTINEL_TARGET: &str = "alab_state_contracts::handshake_sentinel";
const OPAQUE: &str = r#"{"detail":"Bad Request"}"#;
const FORBIDDEN: &str =
    r#"{"error":{"code":"access_program_not_enabled","message":"never-store-ws-provider-body"}}"#;
const PATH: &str = "/v1/responses?privacy=never-store-websocket-url";

#[derive(Default)]
struct Failures(Mutex<Vec<String>>);

impl codex_state::log_db::LogWriteFailureReporter for Failures {
    fn report_failure(&self, diagnostic: &str) {
        self.0.lock().unwrap().push(diagnostic.to_owned());
    }
}

#[derive(Clone, Copy)]
enum Case {
    Inactive,
    Opaque,
    Truncated,
    Forbidden,
}

impl Case {
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
            FORBIDDEN
        } else {
            OPAQUE
        }
    }

    fn advised(self) -> bool {
        !matches!(self, Self::Forbidden)
    }
}

struct FixtureAuth;

impl codex_api::AuthProvider for FixtureAuth {
    fn add_auth_headers(&self, headers: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        headers.insert(
            "authorization",
            "Bearer never-store-ws-request-header".parse().unwrap(),
        );
        Ok(())
    }
}

struct RejectionFixture {
    uri: String,
    task: Option<JoinHandle<()>>,
}

impl RejectionFixture {
    async fn start(case: Case) -> Self {
        let listener = tokio::time::timeout(WAIT, tokio::net::TcpListener::bind("127.0.0.1:0"))
            .await
            .expect("SQLite Upgrade fixture bind deadline")
            .unwrap();
        let uri = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = tokio::time::timeout(WAIT, listener.accept())
                .await
                .expect("SQLite Upgrade fixture accept deadline")
                .unwrap();
            read_upgrade(&mut socket).await;
            let declared = case.body().len() + usize::from(matches!(case, Case::Truncated));
            let mut response = format!(
                "HTTP/1.1 {} Fixture\r\ncontent-length: {declared}\r\ncontent-type: application/json\r\nconnection: close\r\nx-request-id: invalid primary identifier\r\nx-oai-request-id: req_sqlite_upgrade-1\r\ncf-ray: ray_sqlite_upgrade-1\r\nauthorization: Bearer never-store-ws-response-header\r\nx-unrelated: never-store-ws-unrelated-header\r\n",
                case.status(),
            );
            if case.advised() {
                response.push_str("retry-after: 7\r\n");
            }
            response.push_str("\r\n");
            response.push_str(case.body());
            tokio::time::timeout(WAIT, socket.write_all(response.as_bytes()))
                .await
                .expect("SQLite Upgrade fixture response write deadline")
                .unwrap();
            // Truncated sends complete JSON but declares another byte. Real EOF,
            // rather than a manufactured API error, makes its body unavailable.
            tokio::time::timeout(WAIT, socket.shutdown())
                .await
                .expect("SQLite Upgrade fixture shutdown deadline")
                .unwrap();
        });
        Self {
            uri,
            task: Some(task),
        }
    }

    async fn finish(mut self) {
        match tokio::time::timeout(WAIT, self.task.as_mut().unwrap()).await {
            Ok(result) => {
                self.task.take();
                result.expect("SQLite Upgrade fixture completed");
            }
            Err(_) => {
                self.task.as_ref().unwrap().abort();
                let _ = tokio::time::timeout(WAIT, self.task.as_mut().unwrap())
                    .await
                    .expect("SQLite Upgrade fixture abort/reap deadline");
                self.task.take();
                panic!("SQLite Upgrade fixture join deadline");
            }
        }
    }
}

impl Drop for RejectionFixture {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

async fn read_upgrade(socket: &mut TcpStream) {
    let request = tokio::time::timeout(WAIT, async {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let read = socket.read(&mut buffer).await.unwrap();
            assert_ne!(read, 0, "complete Upgrade GET must arrive");
            request.extend_from_slice(&buffer[..read]);
            assert!(request.len() <= 16 * 1024, "bounded Upgrade headers");
            if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                assert_eq!(end + 4, request.len(), "connect sends no model payload");
                break String::from_utf8(request).unwrap();
            }
        }
    })
    .await
    .expect("SQLite Upgrade fixture request read deadline");
    assert!(request.starts_with(&format!("GET {PATH} HTTP/1.1\r\n")));
    for expected in [
        "upgrade: websocket",
        "connection: upgrade",
        "authorization: Bearer never-store-ws-request-header",
    ] {
        assert!(
            request
                .lines()
                .any(|line| line.eq_ignore_ascii_case(expected)),
            "actual Upgrade request must contain {expected}"
        );
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

fn assert_context(row: &codex_state::LogRow) {
    assert_eq!(row.level, "INFO");
    assert_eq!(row.thread_id.as_deref(), Some(THREAD));
    assert!(row.process_uuid.as_ref().is_some_and(|id| !id.is_empty()));
    let message = row.message.as_deref().unwrap();
    assert!(message.contains(&format!("session_loop{{thread_id={THREAD}}}")));
    assert!(message.contains(&format!("turn{{thread.id={THREAD} turn.id={TURN}}}")));
}

async fn run_case(case: Case) {
    assert_eq!(codex_alab_core::active(), case.active());
    let home = tempfile::tempdir().unwrap();
    let runtime = codex_state::StateRuntime::init(
        codex_state::SqliteConfig::new_for_testing(home.path().to_path_buf().try_into().unwrap()),
        "websocket-handshake-fixture".into(),
    )
    .await
    .unwrap();
    let failures = Arc::new(Failures::default());
    let layer = codex_state::log_db::start(runtime.clone(), failures.clone());
    let fixture = RejectionFixture::start(case).await;
    let expected_url = format!("{}{PATH}", fixture.uri.replacen("http://", "ws://", 1));
    let client = codex_api::ResponsesWebsocketClient::new(
        codex_api::Provider {
            name: "sqlite-websocket-handshake".into(),
            base_url: format!("{}/v1", fixture.uri),
            query_params: Some(
                [("privacy".into(), "never-store-websocket-url".into())]
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
    let factory = codex_http_client::HttpClientFactory::new(
        codex_http_client::OutboundProxyPolicy::ReqwestDefault,
    );
    assert!(!factory.allows_system_proxy_fallback());
    let before = Instant::now();
    let result = {
        let subscriber = tracing_subscriber::registry().with(layer.clone().with_filter(
            state_log_filter::filter(codex_state::log_db::default_filter()),
        ));
        let _subscriber = tracing::subscriber::set_default(subscriber);
        let session = tracing::info_span!("session_loop", thread_id = %THREAD);
        let _session = session.enter();
        let turn = tracing::info_span!("turn", thread.id = %THREAD, turn.id = %TURN);
        let _turn = turn.enter();
        tracing::info!(target: SENTINEL_TARGET, "SQLite handshake INFO sentinel");
        tokio::time::timeout(
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
    };
    let after = Instant::now();
    // Reap the real loopback task before examining the public result or rows.
    drop(client);
    fixture.finish().await;
    let error = match result.expect("public WebSocket connect deadline") {
        Err(error) => error,
        Ok(_) => panic!("rejected Upgrade cannot establish a WebSocket"),
    };
    let advice = match &error {
        ApiError::Transport(TransportError::Http {
            status,
            url,
            headers: Some(headers),
            body,
            retry_after,
        }) => {
            assert_eq!(status.as_u16(), case.status());
            assert_eq!(url.as_deref(), Some(expected_url.as_str()));
            assert_eq!(headers["x-request-id"], "invalid primary identifier");
            assert_eq!(headers["x-oai-request-id"], "req_sqlite_upgrade-1");
            assert_eq!(headers["cf-ray"], "ray_sqlite_upgrade-1");
            assert_eq!(headers["x-unrelated"], "never-store-ws-unrelated-header");
            assert_eq!(
                headers["authorization"],
                "Bearer never-store-ws-response-header"
            );
            assert_eq!(
                headers["content-length"],
                (case.body().len() + usize::from(matches!(case, Case::Truncated))).to_string()
            );
            assert_eq!(
                body.as_deref(),
                if matches!(case, Case::Truncated) {
                    None
                } else {
                    Some(case.body())
                }
            );
            assert_eq!(retry_after.is_some(), case.advised());
            assert_eq!(headers.contains_key("retry-after"), case.advised());
            if let Some(advice) = retry_after {
                assert_eq!(headers["retry-after"], "7");
                assert!(advice.deadline() >= before + Duration::from_secs(7));
                assert!(advice.deadline() <= after + Duration::from_secs(7));
            }
            *retry_after
        }
        _ => panic!("real rejected Upgrade must retain original HTTP metadata"),
    };
    let mapped = codex_api::map_api_error(error);
    assert_eq!(mapped.retry_after(), advice, "mapping retains typed advice");
    assert_eq!(
        mapped.retry_delay(1).is_some(),
        matches!(case, Case::Opaque)
    );
    if matches!(case, Case::Forbidden) {
        let expected = codex_api::map_api_error(ApiError::AccessProgramNotEnabled {
            message: "never-store-ws-provider-body".into(),
        });
        assert_eq!(
            std::mem::discriminant(mapped.details()),
            std::mem::discriminant(expected.details())
        );
        assert_eq!(mapped.http_status_code_value(), Some(403));
        assert!(mapped.to_string().contains("never-store-ws-provider-body"));
    }

    layer.flush().await;
    assert!(!layer.has_write_failure());
    assert!(failures.0.lock().unwrap().is_empty());
    let all_rows = runtime
        .query_logs(&codex_state::LogQuery::default())
        .await
        .unwrap();
    let sentinels: Vec<_> = all_rows
        .iter()
        .filter(|row| row.target == SENTINEL_TARGET)
        .collect();
    let [sentinel] = sentinels.as_slice() else {
        panic!("one persisted INFO control even when handshake emitters are inactive");
    };
    assert_context(sentinel);
    assert!(
        sentinel
            .message
            .as_deref()
            .unwrap()
            .contains("SQLite handshake INFO sentinel")
    );
    // Query every diagnostic row first, including threadless/wrong-thread rows.
    let metadata: Vec<_> = all_rows.iter().filter(|row| row.target == TARGET).collect();
    assert!(!all_rows.iter().any(|row| row.target == CAPTURE_TARGET));
    let original = runtime
        .query_logs(&codex_state::LogQuery {
            thread_ids: vec![THREAD.into()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        metadata.iter().map(|row| row.id).collect::<Vec<_>>(),
        original
            .iter()
            .filter(|row| row.target == TARGET)
            .map(|row| row.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(metadata.len(), usize::from(case.active()));
    for row in metadata {
        assert_context(row);
        assert_eq!(row.process_uuid, sentinel.process_uuid);
        let message = row.message.as_deref().unwrap();
        assert!(message.contains("Responses WebSocket handshake rejected"));
        assert_eq!(field(message, "phase"), Some("\"handshake\""));
        assert_eq!(field(message, "transport"), Some("\"responses_websocket\""));
        assert_eq!(numeric(message, "http_status"), u64::from(case.status()));
        assert_eq!(
            field(message, "body_capture"),
            Some(if matches!(case, Case::Truncated) {
                "\"truncated\""
            } else {
                "\"complete\""
            })
        );
        numeric(message, "body_wait_ms");
        if matches!(case, Case::Truncated) {
            assert!(field(message, "rejection_body_bytes").is_none());
        } else {
            assert_eq!(
                numeric(message, "rejection_body_bytes"),
                u64::try_from(case.body().len()).unwrap()
            );
        }
        assert_eq!(
            field(message, "retry_advised"),
            Some(if advice.is_some() { "true" } else { "false" })
        );
        assert_eq!(
            field(message, "upgrade_request_id"),
            Some("\"req_sqlite_upgrade-1\"")
        );
        assert_eq!(
            field(message, "upgrade_cf_ray"),
            Some("\"ray_sqlite_upgrade-1\"")
        );
        for absent in [
            "local_attempt",
            "request_id",
            "cf_ray",
            "model",
            "request_bytes",
            "input_kind",
            "input_items",
            "messages",
            "reasoning_items",
            "tool_calls",
            "tool_outputs",
            "other_items",
            "tools",
            "tool_bytes",
            "failure_kind",
            "opaque_bad_request",
            "body",
            "url",
        ] {
            assert!(
                field(message, absent).is_none(),
                "handshake invented {absent}"
            );
        }
        for private in [
            "never-store",
            "invalid primary identifier",
            "authorization",
            "x-unrelated",
            "retry-after",
            "Bad Request",
            "access_program_not_enabled",
            "127.0.0.1",
            expected_url.as_str(),
            "Responses request shape",
            "Responses request failed",
            "HTTP rejection body capture finished",
        ] {
            assert!(!message.contains(private), "{message}");
        }
    }
    drop(layer);
    runtime.close().await;
}

#[test]
fn public_websocket_handshake_diagnostics_reach_sqlite_with_original_context() {
    const CHILD: &str = "ALAB_WEBSOCKET_HANDSHAKE_SQLITE_CHILD";
    const TEST: &str = "public_websocket_handshake_diagnostics_reach_sqlite_with_original_context";
    let module = module_path!().split_once("::").unwrap().1;
    let exact = format!("{module}::{TEST}");
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(exact.as_str()));
    } else {
        // The shared custody runner owns cleanup; this local adapter controls
        // only this child's proxy environment before the public connect path.
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap());
        child
            .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
            .env(CHILD, &exact)
            .env("NO_PROXY", "*")
            .env("no_proxy", "*");
        for key in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            child.env_remove(key);
        }
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(crate::test_child_process::run(
                child,
                Duration::from_secs(30),
            ))
            .unwrap_or_else(|failure| panic!("{exact}: {failure}"))
            .assert_test_success(&exact);
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
            for case in [Case::Opaque, Case::Truncated, Case::Forbidden] {
                run_case(case).await;
            }
        });
}

const PROXY_CONNECT_TARGET: &str = "ALAB_PROXY_CONNECT_SQLITE_TARGET";
const PROXY_REQUEST_ID: &str = "req_proxy_connect-only-1";
const PROXY_FALLBACK_ID: &str = "req_proxy_connect-fallback-1";
const PROXY_CF_RAY: &str = "ray_proxy_connect-only-1";
const PROXY_PRIVATE_HEADER: &str = "never-store-proxy-connect-header";
const PROXY_PRIVATE_AUTH: &str = "never-store-proxy-connect-auth";

struct ConnectProxyFixture {
    url: String,
    authority: String,
    target: std::net::TcpListener,
    task: Option<JoinHandle<()>>,
}

impl ConnectProxyFixture {
    async fn start() -> Self {
        let listener = tokio::time::timeout(WAIT, tokio::net::TcpListener::bind("127.0.0.1:0"))
            .await
            .expect("SQLite CONNECT proxy bind deadline")
            .unwrap();
        let target = tokio::time::timeout(WAIT, tokio::net::TcpListener::bind("127.0.0.1:0"))
            .await
            .expect("SQLite CONNECT target bind deadline")
            .unwrap();
        let authority = target.local_addr().unwrap().to_string();
        let expected_authority = authority.clone();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = tokio::time::timeout(WAIT, listener.accept())
                .await
                .expect("SQLite CONNECT proxy accept deadline")
                .unwrap();
            let request = tokio::time::timeout(WAIT, async {
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                loop {
                    let read = socket.read(&mut buffer).await.unwrap();
                    assert_ne!(read, 0, "complete proxy CONNECT must arrive");
                    request.extend_from_slice(&buffer[..read]);
                    assert!(request.len() <= 16 * 1024, "bounded CONNECT headers");
                    if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                        assert_eq!(end + 4, request.len(), "no Upgrade GET after CONNECT");
                        break String::from_utf8(request).unwrap();
                    }
                }
            })
            .await
            .expect("SQLite CONNECT proxy request read deadline");
            assert_eq!(
                request.lines().next(),
                Some(format!("CONNECT {expected_authority} HTTP/1.1").as_str())
            );
            assert!(
                request.lines().any(|line| {
                    line.eq_ignore_ascii_case(&format!("Host: {expected_authority}"))
                })
            );
            assert!(!request.contains("/v1/responses"));
            assert!(!request.lines().any(|line| {
                line.to_ascii_lowercase().starts_with("upgrade:")
                    || line.to_ascii_lowercase().starts_with("authorization:")
            }));
            let response = format!(
                "HTTP/1.1 400 Proxy Rejection\r\ncontent-length: {}\r\ncontent-type: application/json\r\nconnection: close\r\nretry-after: 7\r\nx-request-id: {PROXY_REQUEST_ID}\r\nx-oai-request-id: {PROXY_FALLBACK_ID}\r\ncf-ray: {PROXY_CF_RAY}\r\nauthorization: Bearer {PROXY_PRIVATE_AUTH}\r\nx-unrelated: {PROXY_PRIVATE_HEADER}\r\n\r\n{OPAQUE}",
                OPAQUE.len(),
            );
            tokio::time::timeout(WAIT, socket.write_all(response.as_bytes()))
                .await
                .expect("SQLite CONNECT proxy response write deadline")
                .unwrap();
            tokio::time::timeout(WAIT, socket.shutdown())
                .await
                .expect("SQLite CONNECT proxy shutdown deadline")
                .unwrap();
            let mut trailing = [0_u8; 1024];
            match tokio::time::timeout(WAIT, socket.read(&mut trailing))
                .await
                .expect("SQLite CONNECT proxy client close deadline")
            {
                Ok(0) => {}
                // CONNECT only consumes headers. Closing with an unread proxy
                // entity may reset the socket instead of sending a clean EOF.
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                    ) => {}
                other => panic!("failed CONNECT must not send a target Upgrade GET: {other:?}"),
            }
        });
        Self {
            url,
            authority,
            target: target.into_std().unwrap(),
            task: Some(task),
        }
    }

    async fn finish(&mut self) {
        match tokio::time::timeout(WAIT, self.task.as_mut().unwrap()).await {
            Ok(result) => {
                self.task.take();
                result.expect("SQLite CONNECT proxy completed");
            }
            Err(_) => {
                self.task.as_ref().unwrap().abort();
                let _ = tokio::time::timeout(WAIT, self.task.as_mut().unwrap())
                    .await
                    .expect("SQLite CONNECT proxy abort/reap deadline");
                self.task.take();
                panic!("SQLite CONNECT proxy join deadline");
            }
        }
        assert!(
            matches!(self.target.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "the target must never be dialed after the proxy refuses CONNECT"
        );
    }
}

impl Drop for ConnectProxyFixture {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

async fn run_proxy_connect_case() {
    assert!(codex_alab_core::active());
    let authority = std::env::var(PROXY_CONNECT_TARGET).unwrap();
    let target: std::net::SocketAddr = authority.parse().unwrap();
    assert!(target.ip().is_loopback());
    assert_ne!(target.port(), 0);
    let proxy = std::env::var("HTTP_PROXY").unwrap();
    assert_eq!(std::env::var("http_proxy").unwrap(), proxy);
    let proxy: std::net::SocketAddr = proxy.strip_prefix("http://").unwrap().parse().unwrap();
    assert!(proxy.ip().is_loopback());
    assert_ne!(proxy.port(), 0);
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
    ] {
        assert!(
            std::env::var_os(key).is_none(),
            "unexpected inherited {key}"
        );
    }
    let home = tempfile::tempdir().unwrap();
    let runtime = codex_state::StateRuntime::init(
        codex_state::SqliteConfig::new_for_testing(home.path().to_path_buf().try_into().unwrap()),
        "websocket-proxy-connect-fixture".into(),
    )
    .await
    .unwrap();
    let failures = Arc::new(Failures::default());
    let layer = codex_state::log_db::start(runtime.clone(), failures.clone());
    let client = codex_api::ResponsesWebsocketClient::new(
        codex_api::Provider {
            name: "sqlite-proxy-connect".into(),
            base_url: format!("http://{authority}/v1"),
            query_params: None,
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
    let factory = codex_http_client::HttpClientFactory::new(
        codex_http_client::OutboundProxyPolicy::ReqwestDefault,
    );
    assert!(!factory.allows_system_proxy_fallback());
    assert_eq!(
        factory
            .resolve_proxy_route_async(format!("ws://{authority}/v1/responses"))
            .await
            .unwrap(),
        codex_http_client::OutboundProxyRoute::TransportDefault,
        "the public factory must preserve environment-selected transport routing"
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
        tracing::info!(target: SENTINEL_TARGET, "SQLite CONNECT INFO sentinel");
        tokio::time::timeout(
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
    };
    drop(client);
    let error = match result.expect("public proxy CONNECT deadline") {
        Err(error) => error,
        Ok(_) => panic!("a refused CONNECT cannot establish a WebSocket"),
    };
    let ApiError::Transport(TransportError::Network(message)) = &error else {
        panic!("proxy rejection must retain its public Network class: {error:?}");
    };
    assert!(message.contains("HTTP CONNECT failed with status 400"));
    for private in [
        PROXY_REQUEST_ID,
        PROXY_FALLBACK_ID,
        PROXY_CF_RAY,
        PROXY_PRIVATE_HEADER,
        PROXY_PRIVATE_AUTH,
        OPAQUE,
        "Bad Request",
    ] {
        assert!(
            !message.contains(private),
            "proxy Network error leaked {private}"
        );
    }
    let mapped = codex_api::map_api_error(error);
    assert!(mapped.retry_after().is_none());
    assert!(mapped.http_status_code_value().is_none());
    assert!(
        !mapped
            .to_string()
            .contains(codex_alab_core::request_error::OPAQUE_BAD_REQUEST_PREFIX)
    );

    layer.flush().await;
    assert!(!layer.has_write_failure());
    assert!(failures.0.lock().unwrap().is_empty());
    let all_rows = runtime
        .query_logs(&codex_state::LogQuery::default())
        .await
        .unwrap();
    let sentinels: Vec<_> = all_rows
        .iter()
        .filter(|row| row.target == SENTINEL_TARGET)
        .collect();
    let [sentinel] = sentinels.as_slice() else {
        panic!("one persisted INFO control must prove the CONNECT sink is active");
    };
    assert_context(sentinel);
    assert!(
        sentinel
            .message
            .as_deref()
            .unwrap()
            .contains("SQLite CONNECT INFO sentinel")
    );
    // Inspect every diagnostic row, including threadless/wrong-thread rows.
    assert!(
        !all_rows
            .iter()
            .any(|row| row.target == TARGET || row.target == CAPTURE_TARGET)
    );
    for row in &all_rows {
        let message = row.message.as_deref().unwrap_or_default();
        for private in [
            PROXY_REQUEST_ID,
            PROXY_FALLBACK_ID,
            PROXY_CF_RAY,
            PROXY_PRIVATE_HEADER,
            PROXY_PRIVATE_AUTH,
            OPAQUE,
            "Bad Request",
            "never-store-ws-request-header",
            "upgrade_request_id=",
            "upgrade_cf_ray=",
            "retry_advised=",
            "Responses request shape",
            "Responses request failed",
            "Responses WebSocket handshake rejected",
            "Opaque backend HTTP 400 classified as recoverable",
        ] {
            assert!(
                !message.contains(private),
                "proxy rejection persisted {private}: {message}"
            );
        }
    }
    drop(layer);
    runtime.close().await;
}

#[test]
fn proxy_connect_failure_does_not_persist_upgrade_diagnostics() {
    const CHILD: &str = "ALAB_PROXY_CONNECT_SQLITE_CHILD";
    const TEST: &str = "proxy_connect_failure_does_not_persist_upgrade_diagnostics";
    let module = module_path!().split_once("::").unwrap().1;
    let exact = format!("{module}::{TEST}");
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(exact.as_str()));
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let mut fixture = ConnectProxyFixture::start().await;
                let mut child = tokio::process::Command::new(std::env::current_exe().unwrap());
                child
                    .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
                    .env(CHILD, &exact)
                    .env(PROXY_CONNECT_TARGET, &fixture.authority);
                for key in [
                    "HTTP_PROXY",
                    "http_proxy",
                    "HTTPS_PROXY",
                    "https_proxy",
                    "ALL_PROXY",
                    "all_proxy",
                    "NO_PROXY",
                    "no_proxy",
                ] {
                    child.env_remove(key);
                }
                child
                    .env("HTTP_PROXY", &fixture.url)
                    .env("http_proxy", &fixture.url);
                let report = crate::test_child_process::run(child, Duration::from_secs(30)).await;
                // Reap the actual CONNECT server before checking the child result.
                fixture.finish().await;
                report
                    .unwrap_or_else(|failure| panic!("{exact}: {failure}"))
                    .assert_test_success(&exact);
            });
        return;
    }
    assert!(!codex_alab_core::active());
    codex_alab_core::activate();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run_proxy_connect_case());
}
