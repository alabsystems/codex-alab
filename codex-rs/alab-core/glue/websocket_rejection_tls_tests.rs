// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The rejection guard must read HTTP entities above actual target TLS.

use super::RejectionBodyOutcome;
use super::RejectionMetadata;
use super::client_async_tls_with_config;
use codex_utils_rustls_provider::ensure_rustls_crypto_provider;
use futures::task::ArcWake;
use futures::task::waker_ref;
use rustls::ClientConfig;
use rustls::RootCertStore;
use rustls::ServerConfig;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::PrivatePkcs8KeyDer;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::Connector;
use tokio_tungstenite::tungstenite::Error as WebSocketError;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const WAIT: Duration = Duration::from_secs(5);
const CHILD_WAIT: Duration = Duration::from_secs(60);
const BODY: &[u8] = br#"{"detail":"Bad Request"}"#;

struct Material {
    certificate: Vec<u8>,
    key: Vec<u8>,
    pem: String,
}

fn material() -> Material {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    Material {
        certificate: cert.der().to_vec(),
        key: signing_key.serialize_der(),
        pem: cert.pem(),
    }
}

fn configurations(material: &Material) -> (Arc<ClientConfig>, TlsAcceptor) {
    ensure_rustls_crypto_provider();
    let certificate = CertificateDer::from(material.certificate.clone());
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
            PrivatePkcs8KeyDer::from(material.key.clone()).into(),
        )
        .unwrap();
    server.alpn_protocols = vec![b"http/1.1".to_vec()];
    (Arc::new(client), TlsAcceptor::from(Arc::new(server)))
}

struct ServerTask(Option<JoinHandle<()>>);

impl ServerTask {
    async fn finish(mut self) {
        match tokio::time::timeout(WAIT, self.0.as_mut().unwrap()).await {
            Ok(result) => {
                drop(self.0.take());
                result.expect("TLS rejection fixture completes");
            }
            Err(_) => {
                let task = self.0.take().unwrap();
                task.abort();
                let _ = task.await;
                panic!("TLS rejection fixture join deadline");
            }
        }
    }
}

impl Drop for ServerTask {
    fn drop(&mut self) {
        if let Some(task) = self.0.as_ref() {
            task.abort();
        }
    }
}

#[derive(Default)]
struct ReadWake {
    revision: AtomicU64,
    notify: Notify,
}

impl ArcWake for ReadWake {
    fn wake_by_ref(this: &Arc<Self>) {
        this.revision.fetch_add(1, Ordering::SeqCst);
        this.notify.notify_one();
    }
}

impl ReadWake {
    async fn after(&self, revision: u64) {
        loop {
            let notified = self.notify.notified();
            if self.revision.load(Ordering::SeqCst) != revision {
                return;
            }
            notified.await;
        }
    }
}

async fn split_rejection(material: &Material, native_default: bool) {
    let (config, acceptor) = configurations(material);
    let listener = tokio::time::timeout(WAIT, TcpListener::bind("127.0.0.1:0"))
        .await
        .expect("TLS fixture bind deadline")
        .unwrap();
    let address = listener.local_addr().unwrap();
    let (observed, request_seen) = oneshot::channel();
    let (send_headers, header_gate) = oneshot::channel();
    let (headers_sent, header_ack) = oneshot::channel();
    let (send_body, body_gate) = oneshot::channel();
    let server = ServerTask(Some(tokio::spawn(async move {
        let (socket, _) = tokio::time::timeout(WAIT, listener.accept())
            .await
            .expect("TLS fixture accept deadline")
            .unwrap();
        let mut socket = tokio::time::timeout(WAIT, acceptor.accept(socket))
            .await
            .expect("server TLS handshake deadline")
            .expect("client trusts the generated localhost certificate");
        assert_eq!(
            socket.get_ref().1.alpn_protocol(),
            if native_default {
                None
            } else {
                Some(&b"http/1.1"[..])
            },
            "explicit TLS configuration preserves its ALPN selection; default mode stays default"
        );
        tokio::time::timeout(WAIT, async {
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = socket.read(&mut buffer).await.unwrap();
                assert_ne!(read, 0, "TLS Upgrade request must have complete headers");
                request.extend_from_slice(&buffer[..read]);
                assert!(request.len() < 64 * 1024, "bounded TLS Upgrade request");
                if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let request = std::str::from_utf8(&request).unwrap();
                    assert!(request.starts_with("GET /v1/responses HTTP/1.1\r\n"));
                    assert!(
                        request
                            .lines()
                            .any(|line| line.eq_ignore_ascii_case("upgrade: websocket"))
                    );
                    break;
                }
            }
        })
        .await
        .expect("TLS Upgrade request deadline");
        observed.send(()).unwrap();
        tokio::time::timeout(WAIT, header_gate)
            .await
            .expect("TLS response header release deadline")
            .unwrap();
        let headers = format!(
            "HTTP/1.1 400 Bad Request\r\ncontent-length: {}\r\nretry-after: 7\r\nx-request-id: req_wss-body-1\r\ncf-ray: ray_wss-body-1\r\nconnection: close\r\n\r\n",
            BODY.len(),
        );
        let sent_at = Instant::now();
        tokio::time::timeout(WAIT, socket.write_all(headers.as_bytes()))
            .await
            .expect("TLS rejection headers write deadline")
            .unwrap();
        headers_sent.send(sent_at).unwrap();
        tokio::time::timeout(WAIT, body_gate)
            .await
            .expect("TLS rejection body release deadline")
            .unwrap();
        // An old empty-body result remains the assertion's evidence even if its
        // client has already closed before this second TLS record is written.
        let _ = tokio::time::timeout(WAIT, socket.write_all(BODY))
            .await
            .expect("TLS rejection body write deadline");
        let _ = tokio::time::timeout(WAIT, socket.shutdown()).await;
    })));

    let socket = tokio::time::timeout(WAIT, TcpStream::connect(address))
        .await
        .expect("client TCP connect deadline")
        .unwrap();
    let request = format!("wss://localhost:{}/v1/responses", address.port())
        .into_client_request()
        .unwrap();
    let connector = (!native_default).then(|| Connector::Rustls(Arc::clone(&config)));
    let mut connect = Box::pin(client_async_tls_with_config(
        request, socket, None, connector,
    ));
    tokio::time::timeout(WAIT, async {
        tokio::select! {
            result = &mut connect => match result {
                Err(error) => panic!("TLS handshake ended before response headers: {error}"),
                Ok(_) => panic!("withheld HTTP response cannot complete the WSS Upgrade"),
            },
            observed = request_seen => observed.expect("server saw real HTTP above TLS"),
        }
    })
    .await
    .expect("actual WSS Upgrade request deadline");

    let wake = Arc::new(ReadWake::default());
    let waker = waker_ref(&wake);
    assert!(
        connect
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending(),
        "response is withheld after TLS completes"
    );
    let checkpoint = wake.revision.load(Ordering::SeqCst);
    send_headers.send(()).unwrap();
    let header_sent_at = tokio::time::timeout(WAIT, header_ack)
        .await
        .expect("TLS header write acknowledgement deadline")
        .unwrap();
    tokio::time::timeout(WAIT, wake.after(checkpoint))
        .await
        .expect("TLS header read becomes runnable");
    let after_headers = connect.as_mut().poll(&mut Context::from_waker(&waker));
    // The body gate is independent of the headers and remains closed throughout
    // this explicit client read poll on the current-thread runtime.
    send_body.send(()).unwrap();
    let result = match after_headers {
        Poll::Ready(result) => result,
        Poll::Pending => tokio::time::timeout(WAIT, &mut connect)
            .await
            .expect("complete WSS rejection body deadline"),
    };
    let finished_at = Instant::now();
    server.finish().await;
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("TLS succeeded and HTTP must reject the Upgrade"),
    };
    let WebSocketError::Http(response) = error else {
        panic!("TLS transport must preserve the original HTTP rejection: {error}");
    };
    assert_eq!(response.status().as_u16(), 400);
    assert_eq!(response.body().as_deref(), Some(BODY));
    assert_eq!(response.headers()["x-request-id"], "req_wss-body-1");
    assert_eq!(response.headers()["cf-ray"], "ray_wss-body-1");
    assert_eq!(response.headers()["retry-after"], "7");
    let metadata = response
        .extensions()
        .get::<RejectionMetadata>()
        .expect("guard metadata above TLS");
    assert_eq!(metadata.outcome, RejectionBodyOutcome::Complete);
    assert_eq!(metadata.decoded_body_bytes, Some(BODY.len()));
    let advice = metadata
        .retry_after
        .expect("TLS rejection retains header advice");
    assert!(advice.deadline() >= header_sent_at + Duration::from_secs(7));
    assert!(advice.deadline() <= finished_at + Duration::from_secs(7));
    // The caller retains its original immutable TLS configuration; the explicit
    // mode's generated trust anchor is what allowed this handshake to succeed.
    assert_eq!(config.alpn_protocols, [b"http/1.1".to_vec()]);
}

#[tokio::test(flavor = "current_thread")]
async fn actual_wss_split_rejection_preserves_explicit_tls_body_and_metadata() {
    split_rejection(&material(), false).await;
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_default_wss_honors_ssl_cert_file_and_ignores_codex_ca() {
    const CHILD: &str = "ALAB_NATIVE_WSS_REJECTION_CHILD";
    const DIRECTORY: &str = "ALAB_NATIVE_WSS_REJECTION_MATERIAL";
    if std::env::var_os(CHILD).is_none() {
        let material = material();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scratch = Scratch(
            std::env::temp_dir()
                .join(".ubd")
                .join(format!("codex-native-wss-{}-{unique}", std::process::id())),
        );
        std::fs::create_dir_all(&scratch.0).unwrap();
        std::fs::write(scratch.0.join("root.pem"), &material.pem).unwrap();
        std::fs::write(scratch.0.join("certificate.der"), &material.certificate).unwrap();
        std::fs::write(scratch.0.join("key.der"), &material.key).unwrap();
        std::fs::write(
            scratch.0.join("invalid-codex-ca.pem"),
            "invalid certificate",
        )
        .unwrap();
        let name = format!(
            "{}::native_default_wss_honors_ssl_cert_file_and_ignores_codex_ca",
            module_path!().split_once("::").unwrap().1,
        );
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", &name, "--nocapture"])
            .env(CHILD, "1")
            .env(DIRECTORY, &scratch.0)
            .env("SSL_CERT_FILE", scratch.0.join("root.pem"))
            .env_remove("SSL_CERT_DIR")
            .env(
                "CODEX_CA_CERTIFICATE",
                scratch.0.join("invalid-codex-ca.pem"),
            );
        super::test_child_process::run(command, CHILD_WAIT)
            .await
            .unwrap_or_else(|failure| panic!("{name}: {failure}"))
            .assert_test_success(&name);
        return;
    }
    let directory = PathBuf::from(std::env::var_os(DIRECTORY).unwrap());
    let material = Material {
        certificate: std::fs::read(directory.join("certificate.der")).unwrap(),
        key: std::fs::read(directory.join("key.der")).unwrap(),
        pem: String::new(),
    };
    split_rejection(&material, true).await;
}
