// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Proxy wire contracts complete a real WebSocket before checking target encoding.

use crate::WebSocketConnector;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_http_client::OutboundProxyRoute;
use futures::SinkExt;
use futures::StreamExt;
use std::net::Ipv6Addr;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

const WAIT: Duration = Duration::from_secs(5);
const CHILD_WAIT: Duration = Duration::from_secs(60);
const CHILD: &str = "ALAB_WS_PROXY_WIRE_CHILD";
const PROXY: &str = "ALAB_WS_PROXY_WIRE_URL";

#[derive(Clone, Copy)]
enum Protocol {
    Http,
    Socks5,
    Socks5h,
}

impl Protocol {
    fn scheme(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Socks5 => "socks5",
            Self::Socks5h => "socks5h",
        }
    }
}

#[derive(Clone, Copy)]
enum Route {
    Default,
    Explicit,
}

#[derive(Clone, Copy)]
enum Target {
    Ipv6,
    Ipv4,
    Dns,
}

impl Target {
    fn authority(self) -> &'static str {
        match self {
            Self::Ipv6 => "[::1]:8080",
            Self::Ipv4 => "127.0.0.1:8080",
            Self::Dns => "proxy-target.invalid:8080",
        }
    }

    fn socks_command(self) -> Vec<u8> {
        let mut command = vec![5, 1, 0];
        match self {
            Self::Ipv6 => {
                command.push(4);
                command.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
            }
            Self::Ipv4 => command.extend_from_slice(&[1, 127, 0, 0, 1]),
            Self::Dns => {
                let name = b"proxy-target.invalid";
                command.extend_from_slice(&[3, u8::try_from(name.len()).unwrap()]);
                command.extend_from_slice(name);
            }
        }
        command.extend_from_slice(&8080_u16.to_be_bytes());
        command
    }
}

#[derive(Debug)]
enum Observation {
    Http(Vec<u8>),
    Socks { greeting: Vec<u8>, command: Vec<u8> },
}

struct ProxyTask(Option<JoinHandle<Vec<Observation>>>);

impl ProxyTask {
    async fn finish(mut self) -> Result<Vec<Observation>, String> {
        // Keep the handle in this owner across both waits, so cancellation of
        // finish() still invokes Drop's abort fallback.
        let completed = match self.0.as_mut() {
            Some(task) => tokio::time::timeout(WAIT, task).await,
            None => return Err("proxy fixture task owner is missing".to_owned()),
        };
        match completed {
            Ok(result) => {
                drop(self.0.take());
                result.map_err(|error| format!("proxy fixture task failed: {error}"))
            }
            Err(_) => {
                let Some(task) = self.0.as_mut() else {
                    return Err("proxy fixture task owner is missing after timeout".to_owned());
                };
                task.abort();
                let cleanup = tokio::time::timeout(WAIT, task).await;
                let detail = match &cleanup {
                    Ok(Ok(_)) => "task completed during cleanup".to_owned(),
                    Ok(Err(error)) if error.is_cancelled() => "task aborted and reaped".to_owned(),
                    Ok(Err(error)) => format!("task cleanup failed: {error}"),
                    Err(_) => "abort/reap deadline; abort remains requested".to_owned(),
                };
                if cleanup.is_ok() {
                    drop(self.0.take());
                }
                // A late completion cannot turn the original join deadline
                // into a successful fixture result.
                Err(format!("proxy fixture join deadline; {detail}"))
            }
        }
    }
}

impl Drop for ProxyTask {
    fn drop(&mut self) {
        if let Some(task) = &self.0 {
            task.abort();
        }
    }
}

async fn http_tunnel(socket: &mut TcpStream) -> Observation {
    let request = tokio::time::timeout(WAIT, async {
        let mut request = Vec::new();
        let mut byte = [0_u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
            assert!(request.len() <= 16 * 1024, "bounded CONNECT request");
        }
        request
    })
    .await
    .expect("HTTP proxy CONNECT read deadline");
    // Accept even an incorrectly formatted authority. The test must fail on
    // the recorded wire bytes after a successful Upgrade, not a proxy EOF.
    tokio::time::timeout(
        WAIT,
        socket.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n"),
    )
    .await
    .expect("HTTP proxy acknowledgement deadline")
    .unwrap();
    Observation::Http(request)
}

async fn socks_tunnel(socket: &mut TcpStream) -> Observation {
    let greeting = tokio::time::timeout(WAIT, async {
        let mut header = [0_u8; 2];
        socket.read_exact(&mut header).await.unwrap();
        let mut greeting = header.to_vec();
        let mut methods = vec![0_u8; usize::from(header[1])];
        socket.read_exact(&mut methods).await.unwrap();
        greeting.extend_from_slice(&methods);
        greeting
    })
    .await
    .expect("SOCKS greeting read deadline");
    tokio::time::timeout(WAIT, socket.write_all(&[5, 0]))
        .await
        .expect("SOCKS method acknowledgement deadline")
        .unwrap();
    let command = tokio::time::timeout(WAIT, async {
        let mut header = [0_u8; 4];
        socket.read_exact(&mut header).await.unwrap();
        let mut command = header.to_vec();
        let address_length = match header[3] {
            1 => 4,
            4 => 16,
            3 => {
                let length = socket.read_u8().await.unwrap();
                command.push(length);
                usize::from(length)
            }
            other => panic!("unexpected SOCKS address type {other}"),
        };
        let mut address_and_port = vec![0_u8; address_length + 2];
        socket.read_exact(&mut address_and_port).await.unwrap();
        command.extend_from_slice(&address_and_port);
        command
    })
    .await
    .expect("SOCKS command read deadline");
    // Both literal and domain encodings are accepted before the final assertion.
    tokio::time::timeout(
        WAIT,
        socket.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0x1f, 0x90]),
    )
    .await
    .expect("SOCKS tunnel acknowledgement deadline")
    .unwrap();
    Observation::Socks { greeting, command }
}

async fn serve(listener: TcpListener, protocol: Protocol, count: usize) -> Vec<Observation> {
    let mut observations = Vec::new();
    for _ in 0..count {
        let (mut socket, _) = tokio::time::timeout(WAIT, listener.accept())
            .await
            .expect("proxy TCP accept deadline")
            .unwrap();
        let observation = match protocol {
            Protocol::Http => http_tunnel(&mut socket).await,
            Protocol::Socks5 | Protocol::Socks5h => socks_tunnel(&mut socket).await,
        };
        let mut websocket = tokio::time::timeout(WAIT, accept_async(socket))
            .await
            .expect("proxied WebSocket Upgrade deadline")
            .expect("standard server accepts the proxied Upgrade");
        let message = tokio::time::timeout(WAIT, websocket.next())
            .await
            .expect("proxied client frame deadline")
            .expect("one client frame")
            .expect("valid client frame");
        assert!(
            message.is_text(),
            "client sends a real WebSocket text frame"
        );
        tokio::time::timeout(WAIT, websocket.send(message))
            .await
            .expect("proxied echo deadline")
            .unwrap();
        observations.push(observation);
    }
    observations
}

async fn client(route: Route, targets: &[Target]) {
    let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
    assert!(!factory.allows_system_proxy_fallback());
    let connector = WebSocketConnector::new(&factory).unwrap();
    let proxy = std::env::var(PROXY).unwrap();
    for (index, target) in targets.iter().enumerate() {
        let uri = format!("ws://{}/v1/responses", target.authority());
        assert_eq!(
            factory
                .resolve_proxy_route_async(uri.clone())
                .await
                .unwrap(),
            OutboundProxyRoute::TransportDefault,
            "default factory must exercise environment-selected transport routing"
        );
        let request = uri.into_client_request().unwrap();
        let (mut websocket, response) = tokio::time::timeout(WAIT, async {
            match route {
                Route::Default => connector.connect(request, WebSocketConfig::default()).await,
                // Inject only the already-resolved route. The same connector's
                // source admission and permit still surround the real dialer.
                Route::Explicit => {
                    connector
                        .connect_with_route(
                            request,
                            WebSocketConfig::default(),
                            std::future::ready(Ok(OutboundProxyRoute::Proxy {
                                url: proxy.clone(),
                                no_proxy: None,
                            })),
                            /*loopback_direct*/ false,
                        )
                        .await
                }
            }
        })
        .await
        .expect("public connector Upgrade deadline")
        .expect("proxy tunnel completes the real WebSocket Upgrade");
        assert_eq!(response.status().as_u16(), 101);
        let expected = Message::text(format!("proxy wire contract {index}"));
        tokio::time::timeout(WAIT, websocket.send(expected.clone()))
            .await
            .expect("proxied client send deadline")
            .unwrap();
        let actual = tokio::time::timeout(WAIT, websocket.next())
            .await
            .expect("proxied client echo deadline")
            .expect("proxy echoed one frame")
            .expect("valid echoed WebSocket frame");
        assert_eq!(actual, expected);
    }
}

fn assert_wire(observation: Observation, target: Target, basic_auth: bool) {
    match observation {
        Observation::Http(request) => {
            let request = std::str::from_utf8(&request).unwrap();
            let expected_line = format!("CONNECT {} HTTP/1.1", target.authority());
            assert_eq!(
                request.lines().next(),
                Some(expected_line.as_str()),
                "HTTP CONNECT authority must preserve IPv6 literal brackets"
            );
            let hosts = request
                .lines()
                .filter(|line| line.starts_with("Host:"))
                .collect::<Vec<_>>();
            let expected_host = format!("Host: {}", target.authority());
            assert_eq!(hosts, [expected_host.as_str()]);
            let authorization = request
                .lines()
                .filter(|line| line.starts_with("Proxy-Authorization:"))
                .collect::<Vec<_>>();
            assert_eq!(
                authorization,
                if basic_auth {
                    vec!["Proxy-Authorization: Basic dXNlcjpwYXNz"]
                } else {
                    vec![]
                },
                "target normalization must retain configured proxy authentication"
            );
        }
        Observation::Socks { greeting, command } => {
            assert_eq!(greeting, [5, 1, 0]);
            assert_eq!(
                command,
                target.socks_command(),
                "SOCKS encodes an IPv6 literal as ATYP 4 and 16 address bytes"
            );
        }
    }
}

async fn isolated(
    name: &str,
    protocol: Protocol,
    route: Route,
    targets: &[Target],
    basic_auth: bool,
) {
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(name), "one exact isolated test");
        client(route, targets).await;
        return;
    }
    let listener = tokio::time::timeout(WAIT, TcpListener::bind("127.0.0.1:0"))
        .await
        .expect("proxy bind deadline")
        .unwrap();
    let authority = listener.local_addr().unwrap();
    let proxy = format!(
        "{}://{}{authority}",
        protocol.scheme(),
        if basic_auth { "user:pass@" } else { "" }
    );
    let count = targets.len();
    let server = ProxyTask(Some(tokio::spawn(serve(listener, protocol, count))));
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", &exact, "--nocapture"]);
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
    command.env(CHILD, name).env(PROXY, &proxy);
    command.env(
        "HTTP_PROXY",
        match route {
            Route::Default => proxy.as_str(),
            Route::Explicit => "http://127.0.0.1:9",
        },
    );
    let child_result = super::test_child_process::run(command, CHILD_WAIT).await;
    // Collect the parent-owned proxy before asserting either result. Keep the
    // child's primary failure and any secondary fixture failure together.
    let server_result = server.finish().await;
    let context = match &server_result {
        Ok(_) => exact.clone(),
        Err(error) => format!("{exact}; {error}"),
    };
    let report = child_result.unwrap_or_else(|failure| panic!("{context}: {failure}"));
    report.assert_test_success(&context);
    let observations = server_result.unwrap_or_else(|error| panic!("{exact}: {error}"));
    assert_eq!(observations.len(), targets.len());
    for (observation, target) in observations.into_iter().zip(targets) {
        assert_wire(observation, *target, basic_auth);
    }
}

#[tokio::test]
async fn default_http_proxy_brackets_ipv6_target() {
    isolated(
        "default_http_proxy_brackets_ipv6_target",
        Protocol::Http,
        Route::Default,
        &[Target::Ipv6],
        false,
    )
    .await;
}

#[tokio::test]
async fn default_http_proxy_preserves_ipv4_dns_and_basic_auth() {
    isolated(
        "default_http_proxy_preserves_ipv4_dns_and_basic_auth",
        Protocol::Http,
        Route::Default,
        &[Target::Ipv4, Target::Dns],
        true,
    )
    .await;
}

#[tokio::test]
async fn explicit_socks5_proxy_encodes_ipv6_literal() {
    isolated(
        "explicit_socks5_proxy_encodes_ipv6_literal",
        Protocol::Socks5,
        Route::Explicit,
        &[Target::Ipv6],
        false,
    )
    .await;
}

#[tokio::test]
async fn explicit_socks5h_proxy_encodes_ipv6_literal() {
    isolated(
        "explicit_socks5h_proxy_encodes_ipv6_literal",
        Protocol::Socks5h,
        Route::Explicit,
        &[Target::Ipv6],
        false,
    )
    .await;
}

#[tokio::test]
async fn default_socks5_proxy_preserves_literal_and_dns_targets() {
    isolated(
        "default_socks5_proxy_preserves_literal_and_dns_targets",
        Protocol::Socks5,
        Route::Default,
        &[Target::Ipv6, Target::Ipv4, Target::Dns],
        false,
    )
    .await;
}

#[tokio::test]
async fn default_socks5h_proxy_preserves_literal_and_dns_targets() {
    isolated(
        "default_socks5h_proxy_preserves_literal_and_dns_targets",
        Protocol::Socks5h,
        Route::Default,
        &[Target::Ipv6, Target::Ipv4, Target::Dns],
        false,
    )
    .await;
}

#[tokio::test]
async fn explicit_socks5_proxy_preserves_ipv4_and_dns_targets() {
    isolated(
        "explicit_socks5_proxy_preserves_ipv4_and_dns_targets",
        Protocol::Socks5,
        Route::Explicit,
        &[Target::Ipv4, Target::Dns],
        false,
    )
    .await;
}

#[tokio::test]
async fn explicit_socks5h_proxy_preserves_ipv4_and_dns_targets() {
    isolated(
        "explicit_socks5h_proxy_preserves_ipv4_and_dns_targets",
        Protocol::Socks5h,
        Route::Explicit,
        &[Target::Ipv4, Target::Dns],
        false,
    )
    .await;
}
