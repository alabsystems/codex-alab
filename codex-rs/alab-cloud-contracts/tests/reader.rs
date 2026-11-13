// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use base64::Engine as _;
use bytes::Bytes;
use codex_alab_core::native_source as native;
use codex_cloud_client::Client;
use codex_cloud_client::Credentials;
use codex_cloud_client::ThreadListQuery;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_login::AuthConfig;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthRouteConfig;
use http_body_util::BodyExt as _;
use http_body_util::StreamBody;
use hyper::body::Frame;
use hyper::service::service_fn;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use sha2::Digest as _;
use sha2::Sha256;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

const CHILD: &str = "ALAB_CLOUD_READER_CHILD";

#[test]
fn retained_cloud_transport_owns_its_actual_reader_and_destination_policy() {
    let home = tempfile::tempdir().unwrap();
    let issuer = rcgen::CertifiedIssuer::self_signed(
        {
            let mut params = rcgen::CertificateParams::default();
            params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
            params
        },
        rcgen::KeyPair::generate().unwrap(),
    )
    .unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["localhost".into()])
        .unwrap()
        .signed_by(&key, &issuer)
        .unwrap();
    std::fs::write(home.path().join("ca.pem"), issuer.pem()).unwrap();
    std::fs::write(home.path().join("server.der"), cert.der()).unwrap();
    std::fs::write(home.path().join("key.der"), key.serialize_der()).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "actual_cloud_native_reader_child", "--nocapture"])
        .env(CHILD, home.path())
        .env("CODEX_HOME", home.path())
        .env("CODEX_CA_CERTIFICATE", home.path().join("ca.pem"))
        .env(
            codex_alab_core::home::AUTH_PATH_ENV,
            home.path().join("unused.json"),
        )
        .env(native::LAUNCH_ENV, "fixture-cloud-reader")
        .env_remove(codex_alab_core::managed_auth::SLOT_ENV)
        .env_remove("CODEX_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("HTTPS_PROXY")
        .env_remove("https_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

struct Issued {
    id: String,
    source: native::Source,
    live: AtomicBool,
    ready: AtomicUsize,
    retired: AtomicUsize,
    use_serial: Arc<tokio::sync::Semaphore>,
    use_next: AtomicUsize,
    preload: Mutex<Option<codex_alab_core::native_preload::Body>>,
}

impl native::Member for Issued {
    fn begin_use<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_source_use::UseAttempt,
        registration: &'a str,
    ) -> codex_alab_core::native_source_use::UseFuture<'a> {
        Box::pin(async move {
            use codex_alab_core::native_source_use::UseBody;
            use codex_alab_core::native_source_use::UseOutcome;
            use codex_alab_core::native_source_use::UseReceipt;
            if !attempt.hold_admission(self.use_serial.clone()).await? {
                return Ok(UseOutcome::Unsent);
            }
            let body = match attempt.body()? {
                Some(body) => body,
                None => attempt.assign(UseBody {
                    member: self.id.clone(),
                    operation: format!("{:032x}", self.use_next.fetch_add(1, Ordering::AcqRel) + 1),
                    registration: registration.into(),
                })?,
            };
            assert_eq!(body.member, self.id);
            assert_eq!(body.registration, self.id);
            Ok(UseOutcome::Active(UseReceipt::from_native(
                body.member,
                body.operation.clone(),
                body.operation.repeat(2),
            )?))
        })
    }
    fn close_use<'a>(
        &'a self,
        body: &'a codex_alab_core::native_source_use::UseBody,
    ) -> codex_alab_core::native_source_use::UseFuture<'a> {
        Box::pin(async move {
            assert_eq!(body.member, self.id);
            Ok(codex_alab_core::native_source_use::UseOutcome::Closed)
        })
    }
    fn end_use<'a>(
        &'a self,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> native::UnitFuture<'a> {
        Box::pin(async move {
            assert_eq!(receipt.member(), self.id);
            Ok(())
        })
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }
    fn preload_owned<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        _: Option<&'a native::Borrow>,
    ) -> native::PreloadFuture<'a> {
        Box::pin(async move {
            {
                let mut previous = self
                    .preload
                    .lock()
                    .map_err(|_| std::io::Error::other("native fixture preload is not poisoned"))?;
                assert!(previous.is_none(), "one direct-reader preload per member");
                *previous = Some(attempt.clone());
            }
            let mut source = self.source.clone();
            source.operation = attempt.operation.clone();
            Ok(native::Preload {
                source,
                sequence: attempt.sequence.clone(),
                registration: self.id.clone(),
            })
        })
    }
    fn ready_owned<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        registration: &'a str,
        loaded: &'a native::Loaded,
    ) -> native::OwnedReadyFuture<'a> {
        Box::pin(async move {
            self.ready(&attempt.operation, Some(registration), loaded)
                .await?;
            codex_alab_core::native_preload::Ready::from_native(
                attempt.clone(),
                registration.into(),
            )
        })
    }
    fn ready<'a>(
        &'a self,
        _: &'a str,
        registration: Option<&'a str>,
        loaded: &'a native::Loaded,
    ) -> native::ReadyFuture<'a> {
        Box::pin(async move {
            let native::Loaded::ReaderSource { source } = loaded else {
                panic!("actual direct reader expected")
            };
            assert_eq!(source.revision, self.source.revision);
            self.ready.fetch_add(1, Ordering::Release);
            Ok(native::ReadyReceipt {
                registration: registration
                    .ok_or_else(|| {
                        std::io::Error::other("native Ready follows a registered preload")
                    })?
                    .into(),
            })
        })
    }
    fn retire_captured<'a>(&'a self, request: &'a native::Retirement) -> native::UnitFuture<'a> {
        Box::pin(async move {
            request.validate()?;
            {
                let previous = self
                    .preload
                    .lock()
                    .map_err(|_| std::io::Error::other("native fixture preload is not poisoned"))?;
                let previous = previous.as_ref().ok_or_else(|| {
                    std::io::Error::other("retirement follows the actual direct-reader preload")
                })?;
                assert_eq!(
                    request.preload_operation.as_deref(),
                    Some(previous.operation.as_str())
                );
                assert!(request.preload_sequence.as_ref() == Some(&previous.sequence));
            }
            self.retire(request.registration.as_deref()).await
        })
    }
    fn retire<'a>(&'a self, registration: Option<&'a str>) -> native::UnitFuture<'a> {
        Box::pin(async move {
            assert_eq!(registration, Some(self.id.as_str()));
            self.retired.fetch_add(1, Ordering::Release);
            Ok(())
        })
    }
}

struct Host {
    source: native::Source,
    issued: Mutex<Vec<Arc<Issued>>>,
}
impl native::Host for Host {
    fn source<'a>(&'a self, operation: &'a str) -> native::SourceFuture<'a> {
        Box::pin(async move {
            let mut source = self.source.clone();
            source.operation = operation.into();
            Ok(source)
        })
    }
    fn open_member<'a>(&'a self, _: &'a str) -> native::MemberFuture<'a> {
        Box::pin(async move {
            let mut issued = self.issued.lock().map_err(|_| {
                std::io::Error::other("native fixture member census is not poisoned")
            })?;
            let member = Arc::new(Issued {
                id: format!("{:032x}", issued.len() + 1),
                source: self.source.clone(),
                live: AtomicBool::new(true),
                ready: AtomicUsize::new(0),
                retired: AtomicUsize::new(0),
                use_serial: Arc::new(tokio::sync::Semaphore::new(1)),
                use_next: AtomicUsize::new(0),
                preload: Mutex::new(None),
            });
            issued.push(member.clone());
            Ok(member as Arc<dyn native::Member>)
        })
    }
}

fn principal(domain: &[u8], value: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(value);
    format!("{:x}", hash.finalize())
}

#[tokio::test]
async fn actual_cloud_native_reader_child() {
    let Some(home) = std::env::var_os(CHILD).map(std::path::PathBuf::from) else {
        return;
    };
    codex_alab_core::activate();
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let encode = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
    let jwt = format!("{}.{}.fixture", encode(br#"{"alg":"none"}"#), encode(br#"{"sub":"user-b","https://api.openai.com/auth":{"chatgpt_user_id":"user-b","chatgpt_account_id":"account-b"}}"#));
    let bytes = serde_json::to_vec(&serde_json::json!({"auth_mode":"chatgpt", "last_refresh":chrono::Utc::now(), "tokens":{"id_token":jwt,"access_token":"fixture-access","refresh_token":"fixture-refresh","account_id":"account-b","login_owner_id":"fixture-login","oauth_provenance":{"issuer":"https://auth.openai.com","client_id":"fixture-client"}}})).unwrap();
    let path = home.join("native.json");
    codex_alab_core::home::save_auth_bytes(&path, &bytes).unwrap();
    codex_alab_core::home::save_auth_bytes(
        &codex_alab_core::managed_auth::marker_path(&path),
        b"ATERM-CODEX-AUTH-SLOT 1 736c6f74\n",
    )
    .unwrap();
    let host = Arc::new(Host {
        source: native::Source {
            operation: "b".repeat(32),
            slot: "736c6f74".into(),
            path,
            account: principal(b"aterm/codex-renewal/account/v1\0", "account-b"),
            user: principal(b"aterm/codex-renewal/user/v1\0", "user-b"),
            revision: codex_alab_core::managed_auth::Revision {
                generation: 42,
                digest: format!("{:x}", Sha256::digest(&bytes)),
            },
            assignment_generation: 3,
            native_intent: Some("f".repeat(32)),
        },
        issued: Mutex::new(Vec::new()),
    });
    native::install(host.clone());
    let base = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
    let config = AuthConfig {
        codex_home: home.clone(),
        auth_credentials_store_mode: AuthCredentialsStoreMode::File,
        keyring_backend_kind: Default::default(),
        forced_login_method: None,
        chatgpt_base_url: None,
        forced_chatgpt_workspace_id: None,
        managed_auth_policy: Default::default(),
        auth_route_config: AuthRouteConfig::from_http_client_factory(base.clone()),
    };
    let auth = config.load_auth(false).await.unwrap().unwrap();
    let factory = auth.alab_bind_http_client_factory(base.clone()).unwrap();
    let other = config.load_auth(false).await.unwrap().unwrap();
    let other_factory = other.alab_bind_http_client_factory(base).unwrap();
    assert!(
        auth.alab_bind_http_client_factory(other_factory).is_err(),
        "independent reader factories cannot relabel the source"
    );
    let members = host.issued.lock().unwrap().clone();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].ready.load(Ordering::Acquire), 1);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "https://localhost:{}",
        listener.local_addr().unwrap().port()
    );
    let mut tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![std::fs::read(home.join("server.der")).unwrap().into()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(
                std::fs::read(home.join("key.der")).unwrap(),
            )
            .into(),
        )
        .unwrap();
    tls.alpn_protocols = vec![b"h2".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    let (shutdown, mut stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                _ = &mut stopped => break,
                accepted = listener.accept() => {
                    let (socket, _) = accepted.unwrap();
                    let acceptor = acceptor.clone();
                    let count = count.clone();
                    connections.spawn(async move {
                        let tls = acceptor.accept(socket).await.unwrap();
                        let service = service_fn(move |request: http::Request<hyper::body::Incoming>| {
                            let count = count.clone();
                            async move {
                                assert_eq!(request.uri().path(), "/oaiproto.codex_app_server_backend.ThreadService/ListThreads");
                                assert_eq!(request.headers()["authorization"], "Bearer fixture-access");
                                assert_eq!(request.headers()["chatgpt-account-id"], "account-b");
                                assert_eq!(request.version(), http::Version::HTTP_2);
                                request.into_body().collect().await.unwrap();
                                count.fetch_add(1, Ordering::Release);
                                let frames = vec![Ok::<_, Infallible>(Frame::data(Bytes::from_static(b"\0\0\0\0\x02\x0a\x00"))), Ok(Frame::trailers(http::HeaderMap::from_iter([(http::header::HeaderName::from_static("grpc-status"), http::HeaderValue::from_static("0"))])))];
                                Ok::<_, Infallible>(http::Response::builder().header("content-type", "application/grpc").body(StreamBody::new(futures::stream::iter(frames))).unwrap())
                            }
                        });
                        hyper::server::conn::http2::Builder::new(TokioExecutor::new()).serve_connection(TokioIo::new(tls), service).await.unwrap();
                    });
                }
            }
        }
        connections.abort_all();
        while let Some(result) = connections.join_next().await {
            if let Err(error) = result {
                assert!(error.is_cancelled(), "{error}");
            }
        }
    });
    let credentials =
        || Credentials::new(&auth.get_token().unwrap(), &auth.get_account_id().unwrap()).unwrap();
    let client = Client::new(&factory, &endpoint, credentials()).unwrap();
    let query = ThreadListQuery::default();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), client.list_threads(&query))
            .await
            .unwrap()
            .unwrap()
            .data
            .is_empty()
    );
    assert_eq!(requests.load(Ordering::Acquire), 1);
    let denied_factory = factory.clone().with_network_policy(
        factory
            .network_policy()
            .clone()
            .restrict_to_endpoints(Default::default()),
    );
    let denied = Client::new(&denied_factory, &endpoint, credentials()).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), denied.list_threads(&query))
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(
        requests.load(Ordering::Acquire),
        1,
        "destination denial sends no request"
    );
    assert!(client.list_threads(&query).await.unwrap().data.is_empty());
    assert_eq!(requests.load(Ordering::Acquire), 2);
    members[0].live.store(false, Ordering::Release);
    assert!(auth.get_token().is_err());
    assert!(other.get_token().is_ok(), "a different reader remains live");
    assert!(
        tokio::time::timeout(Duration::from_secs(5), client.list_threads(&query))
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(
        requests.load(Ordering::Acquire),
        2,
        "retained Cloud transport cannot send after its actual reader EOF"
    );
    drop(client);
    drop(denied);
    drop(denied_factory);
    let _ = shutdown.send(());
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    drop(factory);
    drop(auth);
    drop(other);
    tokio::time::timeout(Duration::from_secs(5), async {
        while members
            .iter()
            .any(|member| member.retired.load(Ordering::Acquire) == 0)
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("actual native reader retirement completes before the runtime exits");
}
