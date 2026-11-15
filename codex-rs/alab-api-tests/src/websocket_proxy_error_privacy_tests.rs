// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Public WebSocket proxy parse failures retain their class without credentials.

use codex_api::ApiError;
use codex_api::TransportError;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use std::sync::Arc;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(5);
const CHILD_WAIT: Duration = Duration::from_secs(60);
const CHILD: &str = "ALAB_API_WS_PROXY_ERROR_CHILD";
const USER: &str = "fixture-proxy-user-7c19";
const PASSWORD: &str = "fixture-proxy-password-7c19";
const URL_SENTINEL: &str = "fixture-proxy-host-7c19.invalid";

#[derive(Clone, Copy, Debug)]
enum Case {
    MalformedUri,
    TruncatedPercent,
}

impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::MalformedUri => "malformed-uri",
            Self::TruncatedPercent => "truncated-percent",
        }
    }

    fn proxy(self) -> String {
        match self {
            // The space makes http::Uri parsing fail with the entire value.
            // If parsing ever accepts it, only a loopback proxy can be dialed.
            Self::MalformedUri => {
                format!("http://{USER}:{PASSWORD}@127.0.0.1:9 bad/{URL_SENTINEL}")
            }
            // The URI is valid, but decoding the password lacks a second hex
            // digit and reports the original password component.
            Self::TruncatedPercent => format!("http://{USER}:{PASSWORD}%4@127.0.0.1:9"),
        }
    }
}

struct NoAuth;

impl codex_api::AuthProvider for NoAuth {
    fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        Ok(())
    }
}

async fn public_connect_failure(case: Case) {
    let proxy = case.proxy();
    assert_eq!(std::env::var("HTTP_PROXY").unwrap(), proxy);
    let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
    assert!(!factory.allows_system_proxy_fallback());
    let client = codex_api::ResponsesWebsocketClient::new(
        codex_api::Provider {
            name: "proxy-error-fixture".into(),
            // The endpoint uses the ordinary connector for loopback targets too.
            // An unexpected parsing success must still stay on loopback.
            base_url: "http://127.0.0.1:9/v1".into(),
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
    let error = tokio::time::timeout(
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
    .expect("public proxy configuration failure deadline")
    .expect_err("malformed proxy configuration prevents the public WebSocket connection");
    let ApiError::Transport(TransportError::Network(message)) = &error else {
        panic!("proxy parse error must retain its public Network class: {error:?}");
    };
    assert!(
        message.contains("Invalid proxy configuration"),
        "the proxy parser must reject configuration before a socket error: {error:?}"
    );
    for (view, text) in [
        ("Display", error.to_string()),
        ("Debug", format!("{error:?}")),
    ] {
        for private in [proxy.as_str(), USER, PASSWORD, URL_SENTINEL] {
            assert!(
                !text.contains(private),
                "{case:?}: public API {view} leaked a proxy credential or URL sentinel: {text}"
            );
        }
    }
}

async fn isolated(case: Case) {
    let exact = format!(
        "{}::public_websocket_proxy_parse_errors_hide_credentials",
        module_path!().split_once("::").unwrap().1
    );
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
        .env(CHILD, case.name());
    for name in [
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
        command.env_remove(name);
    }
    command.env("HTTP_PROXY", case.proxy());
    let report = crate::test_child_process::run(command, CHILD_WAIT)
        .await
        .unwrap_or_else(|failure| panic!("isolated proxy privacy child: {failure}"));
    report.assert_test_success(&exact);
}

#[tokio::test(flavor = "current_thread")]
async fn public_websocket_proxy_parse_errors_hide_credentials() {
    if let Ok(case) = std::env::var(CHILD) {
        let case = match case.as_str() {
            "malformed-uri" => Case::MalformedUri,
            "truncated-percent" => Case::TruncatedPercent,
            _ => panic!("unknown isolated proxy privacy case"),
        };
        codex_alab_core::activate();
        public_connect_failure(case).await;
        return;
    }
    assert!(!codex_alab_core::active(), "only the exact child activates");
    for case in [Case::MalformedUri, Case::TruncatedPercent] {
        isolated(case).await;
    }
    assert!(!codex_alab_core::active(), "parent activation is unchanged");
}
