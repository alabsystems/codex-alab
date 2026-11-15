// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Malformed proxy configuration must not expose environment credentials.

use crate::WebSocketConnector;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_http_client::OutboundProxyRoute;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Error as WebSocketError;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::error::UrlError;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

const WAIT: Duration = Duration::from_secs(5);
const CHILD_WAIT: Duration = Duration::from_secs(60);
const CHILD: &str = "ALAB_WS_PROXY_CONFIG_ERROR_CHILD";
const USER: &str = "fixture-proxy-user-7c19";
const PASSWORD: &str = "fixture-proxy-password-7c19";
const HOST: &str = "fixture-proxy-host-7c19.invalid";
const MALFORMED_URI: &str = "http://fixture-proxy-user-7c19:fixture-proxy-password-7c19@127.0.0.1:9 bad/fixture-proxy-host-7c19.invalid";
const TRUNCATED_PERCENT: &str =
    "http://fixture-proxy-user-7c19:fixture-proxy-password-7c19%4@127.0.0.1:9";

#[derive(Clone, Copy)]
enum Route {
    Default,
    Explicit,
}

async fn probe(proxy: &str, route: Route) {
    let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
    assert!(!factory.allows_system_proxy_fallback());
    let connector = WebSocketConnector::new(&factory).unwrap();
    let uri = "ws://127.0.0.1:9/v1/responses";
    assert_eq!(
        factory.resolve_proxy_route_async(uri.into()).await.unwrap(),
        OutboundProxyRoute::TransportDefault,
    );
    let request = uri.into_client_request().unwrap();
    let result = tokio::time::timeout(WAIT, async {
        match route {
            Route::Default => connector.connect(request, WebSocketConfig::default()).await,
            Route::Explicit => {
                connector
                    .connect_with_route(
                        request,
                        WebSocketConfig::default(),
                        std::future::ready(Ok(OutboundProxyRoute::Proxy {
                            url: proxy.into(),
                            no_proxy: None,
                        })),
                        /*loopback_direct*/ false,
                    )
                    .await
            }
        }
    })
    .await
    .expect("malformed proxy configuration failure deadline");
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("malformed proxy configuration cannot create a WebSocket"),
    };
    assert!(
        matches!(&error, WebSocketError::Url(UrlError::InvalidProxyConfig(_))),
        "configuration failure must retain its original error variant: {error:?}"
    );
    for (representation, rendered) in [
        ("Display", error.to_string()),
        ("Debug", format!("{error:?}")),
    ] {
        for marker in [USER, PASSWORD, HOST, proxy] {
            assert!(
                !rendered.contains(marker),
                "{representation} exposes private proxy configuration marker {marker}"
            );
        }
    }
}

async fn isolated(name: &str, proxy: &str, route: Route) {
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(name), "one exact isolated test");
        probe(proxy, route).await;
        return;
    }
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", &exact, "--nocapture"]);
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
    command.env(CHILD, name).env("HTTP_PROXY", proxy);
    super::test_child_process::run(command, CHILD_WAIT)
        .await
        .unwrap_or_else(|failure| panic!("{exact}: {failure}"))
        .assert_test_success(&exact);
}

#[tokio::test]
async fn default_env_proxy_uri_error_does_not_expose_credentials() {
    isolated(
        "default_env_proxy_uri_error_does_not_expose_credentials",
        MALFORMED_URI,
        Route::Default,
    )
    .await;
}

#[tokio::test]
async fn default_env_proxy_password_error_does_not_expose_credentials() {
    isolated(
        "default_env_proxy_password_error_does_not_expose_credentials",
        TRUNCATED_PERCENT,
        Route::Default,
    )
    .await;
}

#[tokio::test]
async fn explicit_proxy_uri_error_remains_redacted() {
    isolated(
        "explicit_proxy_uri_error_remains_redacted",
        MALFORMED_URI,
        Route::Explicit,
    )
    .await;
}
