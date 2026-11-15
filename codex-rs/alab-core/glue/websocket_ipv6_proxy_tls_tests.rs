// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A real trusted HTTPS proxy reached by IPv6 literal must keep IP TLS identity.

use crate::WebSocketConnector;
use crate::WebSocketTlsMode;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_http_client::OutboundProxyRoute;
use codex_utils_rustls_provider::ensure_rustls_crypto_provider;
use futures::SinkExt;
use futures::StreamExt;
use rustls::ClientConfig;
use rustls::RootCertStore;
use rustls::ServerConfig;
use rustls::pki_types::PrivatePkcs8KeyDer;
use rustls::pki_types::ServerName;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Error as WebSocketError;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

const WAIT: Duration = Duration::from_secs(5);

fn configurations(ip_san: &str) -> (Arc<ClientConfig>, TlsAcceptor) {
    ensure_rustls_crypto_provider();
    // rcgen parses this literal as an IP SAN, not a DNS SAN. The ordinary
    // Rustls verifier must validate the proxy's IP identity against this SAN.
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec![ip_san.into()]).unwrap();
    let certificate = cert.der().clone();
    let mut roots = RootCertStore::empty();
    roots.add(certificate.clone()).unwrap();
    let mut client = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    client.alpn_protocols = vec![b"http/1.1".to_vec()];
    let mut server = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate],
            PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
        )
        .unwrap();
    server.alpn_protocols = vec![b"http/1.1".to_vec()];
    (Arc::new(client), TlsAcceptor::from(Arc::new(server)))
}

struct ProxyTask(Option<JoinHandle<io::Result<Vec<u8>>>>);

impl ProxyTask {
    async fn joined(mut self) -> io::Result<Vec<u8>> {
        match tokio::time::timeout(WAIT, self.0.as_mut().unwrap()).await {
            Ok(result) => {
                drop(self.0.take());
                result.expect("IPv6 HTTPS proxy fixture completes")
            }
            Err(_) => {
                let task = self.0.take().unwrap();
                task.abort();
                let _ = task.await;
                panic!("IPv6 HTTPS proxy fixture join deadline");
            }
        }
    }

    async fn finish(self) -> Vec<u8> {
        self.joined()
            .await
            .expect("client completes trusted proxy TLS")
    }
}

impl Drop for ProxyTask {
    fn drop(&mut self) {
        if let Some(task) = &self.0 {
            task.abort();
        }
    }
}

#[tokio::test]
async fn https_proxy_accepts_ipv6_ip_identity_and_tunnels_websocket() {
    let (config, acceptor) = configurations("::1");
    let listener = tokio::time::timeout(WAIT, TcpListener::bind("[::1]:0"))
        .await
        .expect("IPv6 proxy bind deadline")
        .expect("IPv6 loopback is required; a bind failure leaves this fixture unexercised");
    let address = listener.local_addr().unwrap();
    let server = ProxyTask(Some(tokio::spawn(async move {
        let (socket, _) = tokio::time::timeout(WAIT, listener.accept())
            .await
            .expect("IPv6 proxy TCP accept deadline")
            .unwrap();
        // An old client drops TCP before ClientHello because it rejects the
        // bracketed ServerName. Return that fixture EOF quietly; the client
        // assertion below must report its original InvalidDnsName error.
        let mut socket = tokio::time::timeout(WAIT, acceptor.accept(socket))
            .await
            .expect("IPv6 proxy TLS handshake deadline")?;
        assert_eq!(
            socket.get_ref().1.server_name(),
            None,
            "IP identity has no DNS SNI"
        );
        assert_eq!(socket.get_ref().1.alpn_protocol(), Some(&b"http/1.1"[..]));
        let connect = tokio::time::timeout(WAIT, async {
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
                assert!(request.len() <= 16 * 1024, "bounded proxy CONNECT request");
            }
            request
        })
        .await
        .expect("IPv6 HTTPS proxy CONNECT read deadline");
        tokio::time::timeout(
            WAIT,
            socket.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n"),
        )
        .await
        .expect("IPv6 HTTPS proxy CONNECT acknowledgement deadline")
        .unwrap();
        // The proxy itself terminates this fixture's destination protocol.
        // No external destination DNS lookup or destination socket is used.
        let mut websocket = tokio::time::timeout(WAIT, accept_async(socket))
            .await
            .expect("WebSocket Upgrade through IPv6 HTTPS proxy deadline")
            .expect("standard WebSocket Upgrade after trusted proxy TLS");
        let message = tokio::time::timeout(WAIT, websocket.next())
            .await
            .expect("proxied client frame deadline")
            .expect("one client frame")
            .expect("valid WebSocket client frame");
        assert_eq!(message, Message::text("IPv6 HTTPS proxy TLS identity"));
        tokio::time::timeout(WAIT, websocket.send(message))
            .await
            .expect("proxied echo deadline")
            .unwrap();
        Ok(connect)
    })));

    let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
    let connector = WebSocketConnector::new_with_tls_mode(
        &factory,
        WebSocketTlsMode::Rustls(Arc::clone(&config)),
    )
    .unwrap();
    let request = "ws://proxy-target.invalid:8080/v1/responses"
        .into_client_request()
        .unwrap();
    let result = tokio::time::timeout(
        WAIT,
        connector.connect_with_route(
            request,
            WebSocketConfig::default(),
            std::future::ready(Ok(OutboundProxyRoute::Proxy {
                url: format!("https://{address}"),
                no_proxy: None,
            })),
            /*loopback_direct*/ false,
        ),
    )
    .await
    .expect("IPv6 HTTPS proxy connection deadline");
    // Check the client result before joining the server, so the original
    // name-conversion failure cannot be replaced by the resulting server EOF.
    let (mut websocket, response) = result
        .expect("trusted IPv6 HTTPS proxy must accept a bare IP ServerName, not URI brackets");
    assert_eq!(response.status().as_u16(), 101);
    let expected = Message::text("IPv6 HTTPS proxy TLS identity");
    tokio::time::timeout(WAIT, websocket.send(expected.clone()))
        .await
        .expect("proxied client send deadline")
        .unwrap();
    let actual = tokio::time::timeout(WAIT, websocket.next())
        .await
        .expect("proxied client echo deadline")
        .expect("one echoed frame")
        .expect("valid echoed WebSocket frame");
    assert_eq!(actual, expected);
    let connect = server.finish().await;
    let connect = std::str::from_utf8(&connect).unwrap();
    assert_eq!(
        connect.lines().next(),
        Some("CONNECT proxy-target.invalid:8080 HTTP/1.1")
    );
    let hosts = connect
        .lines()
        .filter(|line| line.starts_with("Host:"))
        .collect::<Vec<_>>();
    assert_eq!(hosts, ["Host: proxy-target.invalid:8080"]);
    assert_eq!(config.alpn_protocols, [b"http/1.1".to_vec()]);
}

#[tokio::test]
async fn https_ipv6_proxy_rejects_a_trusted_certificate_for_another_ip() {
    // The exact presented certificate is trusted, so rejection must concern
    // its ::2 SAN and the requested ::1 identity, rather than an unknown root.
    let (config, acceptor) = configurations("::2");
    let listener = tokio::time::timeout(WAIT, TcpListener::bind("[::1]:0"))
        .await
        .expect("IPv6 proxy bind deadline")
        .expect("IPv6 loopback is required; a bind failure leaves this fixture unexercised");
    let address = listener.local_addr().unwrap();
    let server = ProxyTask(Some(tokio::spawn(async move {
        let (socket, _) = tokio::time::timeout(WAIT, listener.accept())
            .await
            .expect("IPv6 wrong-IP proxy TCP accept deadline")
            .unwrap();
        // Certificate rejection and the old pre-ClientHello name error both
        // close TLS quietly. The typed client assertion distinguishes them.
        tokio::time::timeout(WAIT, acceptor.accept(socket))
            .await
            .expect("IPv6 wrong-IP proxy TLS handshake deadline")
            .map(|_| Vec::new())
    })));
    let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
    let connector =
        WebSocketConnector::new_with_tls_mode(&factory, WebSocketTlsMode::Rustls(config)).unwrap();
    let request = "ws://proxy-target.invalid:8080/v1/responses"
        .into_client_request()
        .unwrap();
    let result = tokio::time::timeout(
        WAIT,
        connector.connect_with_route(
            request,
            WebSocketConfig::default(),
            std::future::ready(Ok(OutboundProxyRoute::Proxy {
                url: format!("https://{address}"),
                no_proxy: None,
            })),
            /*loopback_direct*/ false,
        ),
    )
    .await;
    // Join and reap the expected server-side TLS failure before evaluating the
    // saved client error. A pre-fix InvalidDnsName must fail this control too.
    let server_result = server.joined().await;
    let error = match result.expect("IPv6 wrong-IP proxy connection deadline") {
        Err(error) => error,
        Ok(_) => panic!("a trusted certificate for ::2 cannot authenticate proxy ::1"),
    };
    let WebSocketError::Io(error) = error else {
        panic!("expected a certificate name rejection after IP ServerName parsing, got {error:?}");
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    let tls_error = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<rustls::Error>());
    assert!(
        matches!(
            tls_error,
            Some(rustls::Error::InvalidCertificate(
                rustls::CertificateError::NotValidForName
                    | rustls::CertificateError::NotValidForNameContext { .. }
            ))
        ),
        "expected the ordinary verifier's typed name rejection, got {error:?}"
    );
    if let Some(rustls::Error::InvalidCertificate(
        rustls::CertificateError::NotValidForNameContext { expected, .. },
    )) = tls_error
    {
        assert_eq!(expected, &ServerName::try_from("::1").unwrap());
    }
    assert!(server_result.is_err(), "wrong-IP proxy TLS cannot complete");
}
