// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Local source/caller conjunction and counted lifetime controls; no native image proof.
use super::*;
use codex_alab_core::assignment;
use codex_alab_core::manager_catalog as catalog;
use codex_alab_core::manager_source_epoch;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
struct Backend;
impl catalog::Backend for Backend {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn check_target(
        &self,
        _: &codex_alab_core::assignment_service::Target,
    ) -> Result<(), assignment::Refused> {
        Ok(())
    }
    fn source_stamp(&self) -> Result<assignment::Generations, assignment::Refused> {
        Ok(stamp())
    }
}
fn stamp() -> assignment::Generations {
    assignment::Generations {
        auth: 1,
        owner: 1,
        source: 1,
        login: 1,
    }
}
struct Version(manager_source_epoch::Epoch);
impl catalog::Version for Version {
    fn current(&self) -> assignment::Generations {
        stamp()
    }
    fn source_epoch(&self) -> Result<Option<manager_source_epoch::Epoch>, assignment::Refused> {
        Ok(Some(self.0.clone()))
    }
    fn changed<'a>(
        &'a self,
        _: &'a assignment::Generations,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(std::future::pending())
    }
}
fn local() -> (
    Arc<catalog::Registry>,
    Arc<assignment::Domain>,
    NetworkPolicy,
) {
    let registry = Arc::new(catalog::Registry::default());
    let factory = registry.factory(None).unwrap();
    factory.record().attach(Arc::new(Backend)).unwrap();
    let domain = factory.domain();
    factory.complete().unwrap();
    let owner = manager_source_epoch::Owner::new(domain.clone());
    let scope =
        catalog::Scope::capture(domain.clone(), Arc::new(Version(owner.capture().unwrap())))
            .unwrap();
    (
        registry,
        domain,
        NetworkPolicy::unmanaged().with_manager_source(Ok(scope)),
    )
}
struct Auth {
    policy: NetworkPolicy,
    reads: Arc<AtomicUsize>,
}
impl AuthProvider for Auth {
    fn alab_source_policy(&self) -> Result<Option<NetworkPolicy>, AuthError> {
        Ok(Some(self.policy.clone()))
    }
    fn add_auth_headers(&self, headers: &mut http::HeaderMap) -> Result<(), AuthError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer fixture"),
        );
        Ok(())
    }
}
struct Body(Arc<AtomicBool>);

#[test]
fn auxiliary_pool_binding_rejects_another_native_source_without_reading_credentials() {
    use codex_http_client::ClientRouteClass;
    use codex_http_client::OutboundProxyPolicy;
    use codex_http_client::RouteAwareClientPool;

    let (_registry, _domain, source) = local();
    let (_other_registry, _other_domain, other) = local();
    let reads = Arc::new(AtomicUsize::new(0));
    let auth = Auth {
        policy: source.clone(),
        reads: Arc::clone(&reads),
    };
    let pool = RouteAwareClientPool::new(
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        ClientRouteClass::Api,
    );
    let bound = bind_pool(&auth, &pool).expect("bind selected source");
    assert!(bound.with_account_binding(&source).is_some());
    assert!(bound.with_account_binding(&other).is_none());
    let foreign = pool.with_account_binding(&other).expect("foreign pool");
    assert!(bind_pool(&auth, &foreign).is_err());
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

impl futures::Stream for Body {
    type Item = Result<bytes::Bytes, TransportError>;
    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Pending
    }
}
impl Drop for Body {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
struct Transport {
    policy: NetworkPolicy,
    calls: Arc<AtomicUsize>,
    closed: Arc<AtomicBool>,
}
impl HttpTransport for Transport {
    fn alab_source_policy(&self) -> Option<NetworkPolicy> {
        Some(self.policy.clone())
    }
    async fn execute(&self, _: Request) -> Result<Response, TransportError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Response {
            status: http::StatusCode::OK,
            headers: http::HeaderMap::new(),
            body: bytes::Bytes::new(),
        })
    }
    async fn stream(&self, _: Request) -> Result<StreamResponse, TransportError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(StreamResponse {
            status: http::StatusCode::OK,
            headers: http::HeaderMap::new(),
            bytes: Box::pin(Body(self.closed.clone())),
        })
    }
}
fn request() -> Request {
    Request::new(http::Method::GET, "https://example.com/allowed".into())
}
#[tokio::test]
async fn foreign_source_and_caller_restriction_refuse_before_any_credential_read() {
    let (_, _, a) = local();
    let (_, _, b) = local();
    let reads = Arc::new(AtomicUsize::new(0));
    let auth = Auth {
        policy: a.clone(),
        reads: reads.clone(),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let mut transport = Transport {
        policy: b,
        calls: calls.clone(),
        closed: Arc::new(AtomicBool::new(false)),
    };
    assert!(execute(&auth, &transport, request()).await.is_err());
    transport.policy = a.restrict_to_endpoints(std::collections::BTreeSet::from([
        "https://example.com/other".parse().unwrap(),
    ]));
    assert!(execute(&auth, &transport, request()).await.is_err());
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn actual_stream_body_and_poll_cancellation_hold_source_until_body_drop() {
    let (registry, _domain, policy) = local();
    let closed = Arc::new(AtomicBool::new(false));
    let auth = Auth {
        policy: policy.clone(),
        reads: Arc::new(AtomicUsize::new(0)),
    };
    let transport = Transport {
        policy,
        calls: Arc::new(AtomicUsize::new(0)),
        closed: closed.clone(),
    };
    let mut response = stream(&auth, &transport, request()).await.unwrap();
    {
        let pending = response.bytes.next();
        tokio::pin!(pending);
        tokio::select! { _ = &mut pending => panic!("held real body"), _ = tokio::task::yield_now() => {} }
    }
    let seal = registry.seal().unwrap();
    let drain = seal.drain_resources();
    tokio::pin!(drain);
    tokio::select! { _ = &mut drain => panic!("body is still owned"), _ = tokio::task::yield_now() => {} }
    assert!(!closed.load(Ordering::SeqCst));
    drop(response);
    drain.await.unwrap();
    assert!(
        closed.load(Ordering::SeqCst),
        "real body closes before drain completes"
    );
}
#[tokio::test]
async fn cancelled_credential_action_keeps_its_owned_source_until_actual_completion() {
    let (registry, _domain, policy) = local();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let completed = Arc::new(AtomicBool::new(false));
    let task = {
        let entered = entered.clone();
        let release = release.clone();
        let completed = completed.clone();
        tokio::spawn(async move {
            credential_action(policy, async move {
                entered.notify_one();
                release.notified().await;
                completed.store(true, Ordering::SeqCst);
                true
            })
            .await
        })
    };
    entered.notified().await;
    task.abort();
    let _ = task.await;
    let seal = registry.seal().unwrap();
    let drain = seal.drain_resources();
    tokio::pin!(drain);
    tokio::select! { _ = &mut drain => panic!("accepted action still owns its source"), _ = tokio::task::yield_now() => {} }
    release.notify_one();
    drain.await.unwrap();
    assert!(completed.load(Ordering::SeqCst));
}

#[tokio::test]
async fn source_change_after_destination_admission_refuses_before_new_credential_materialization() {
    struct Changing {
        current: std::sync::Mutex<NetworkPolicy>,
        replacement: NetworkPolicy,
        reads: Arc<AtomicUsize>,
    }
    impl AuthProvider for Changing {
        fn alab_source_policy(&self) -> Result<Option<NetworkPolicy>, AuthError> {
            Ok(Some(self.current.lock().unwrap().clone()))
        }
        fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), AuthError> {
            panic!("async selection required")
        }
        fn apply_auth(&self, mut request: Request) -> crate::AuthProviderFuture<'_> {
            let replacement = self.replacement.clone();
            *self.current.lock().unwrap() = replacement.clone();
            let reads = self.reads.clone();
            Box::pin(async move {
                select_request(replacement, async move {
                    reads.fetch_add(1, Ordering::SeqCst);
                    request.headers.insert(
                        http::header::AUTHORIZATION,
                        http::HeaderValue::from_static("Bearer must-not-be-read"),
                    );
                    Ok(request)
                })
                .await
            })
        }
    }
    let (_, _, a) = local();
    let (_, _, b) = local();
    let reads = Arc::new(AtomicUsize::new(0));
    let auth = Changing {
        current: std::sync::Mutex::new(a.clone()),
        replacement: b,
        reads: reads.clone(),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let transport = Transport {
        policy: a,
        calls: calls.clone(),
        closed: Arc::new(AtomicBool::new(false)),
    };
    assert!(execute(&auth, &transport, request()).await.is_err());
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn legacy_raw_export_is_never_a_destination_continuation() {
    let (_, _, policy) = local();
    assert!(legacy_snapshot_export_available(&policy));
    let guard = prepare(
        policy.clone(),
        Some(policy.clone()),
        "https://example.test/",
    )
    .await
    .unwrap();
    CURRENT
        .scope(guard, async {
            assert!(!legacy_snapshot_export_available(&policy));
        })
        .await;
}

#[tokio::test]
async fn known_native_material_requires_destination_but_owned_refresh_action_keeps_custody() {
    let (_, _, policy) = local();
    let policy = policy.with_credential_destination_required();
    let reads = Arc::new(AtomicUsize::new(0));
    let selected = reads.clone();
    assert!(
        select_headers(policy.clone(), async move {
            selected.fetch_add(1, Ordering::SeqCst);
            Ok(http::HeaderMap::new())
        })
        .await
        .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let actions = reads.clone();
    assert!(
        credential_action(policy, async move {
            actions.fetch_add(1, Ordering::SeqCst);
            true
        })
        .await
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn concrete_http_owner_conjoins_known_native_source_and_destination_before_materialization() {
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::header;
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    struct Selected {
        policy: NetworkPolicy,
        reads: Arc<AtomicUsize>,
    }
    impl AuthProvider for Selected {
        fn alab_source_policy(&self) -> Result<Option<NetworkPolicy>, AuthError> {
            Ok(Some(self.policy.clone()))
        }
        fn add_auth_headers(&self, headers: &mut http::HeaderMap) -> Result<(), AuthError> {
            with_policy_sync(self.policy.clone(), || {
                self.reads.fetch_add(1, Ordering::SeqCst);
                headers.insert(
                    http::header::AUTHORIZATION,
                    http::HeaderValue::from_static("Bearer fixture"),
                );
                Ok(())
            })
        }
    }
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/allowed"))
        .and(header("authorization", "Bearer fixture"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let url = format!("{}/allowed", server.uri());
    let (_, _, source) = local();
    let source = source.with_credential_destination_required();
    let reads = Arc::new(AtomicUsize::new(0));
    let auth = Selected {
        policy: source.clone(),
        reads: reads.clone(),
    };
    let caller =
        source.restrict_to_endpoints(std::collections::BTreeSet::from([url.parse().unwrap()]));
    let factory = HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault)
        .with_network_policy(caller);
    let transport = codex_http_client::ReqwestTransport::from_http_client(
        factory
            .build_client(&url, codex_http_client::ClientRouteClass::Api)
            .unwrap(),
    );
    assert!(
        execute(
            &auth,
            &transport,
            Request::new(http::Method::GET, url.clone())
        )
        .await
        .unwrap()
        .status
        .is_success()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert!(
        execute(
            &auth,
            &transport,
            Request::new(http::Method::GET, format!("{}/forbidden", server.uri()))
        )
        .await
        .is_err()
    );
    let (_, _, foreign) = local();
    let factory = HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault)
        .with_network_policy(foreign);
    let transport = codex_http_client::ReqwestTransport::from_http_client(
        factory
            .build_client(&url, codex_http_client::ClientRouteClass::Api)
            .unwrap(),
    );
    assert!(
        execute(&auth, &transport, Request::new(http::Method::GET, url))
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
