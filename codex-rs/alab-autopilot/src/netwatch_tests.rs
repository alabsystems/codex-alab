// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The API's reach (R4 of the parity review, aterm's `RULE_API_BACK`): the measure's state and
//! schedule (aterm's `netwatch` tests, ported), one probe against LOCAL listeners only (a TLS
//! listener whose self-signed certificate only the test's trust holds, a refused port, a server
//! that answers garbage, a resolver seam), the route a network wall takes, and the task that
//! tells a waiting thread each verdict it did not decide with. Never the internet.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Waker;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use codex_http_client::HttpClientFactory;
use codex_http_client::NetworkPolicyController;
use codex_http_client::OutboundProxyPolicy;
use codex_http_client::RouteFailureClass;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::auth::AuthMode;
use pretty_assertions::assert_eq;
use rcgen::CertifiedKey;
use rustls::ClientConfig;
use rustls::RootCertStore;
use rustls::ServerConfig;
use rustls_pki_types::PrivateKeyDer;
use rustls_pki_types::PrivatePkcs8KeyDer;
use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

use crate::netwatch::Dest;
use crate::netwatch::NET_LEASE;
use crate::netwatch::NetState;
use crate::netwatch::NetTiming;
use crate::netwatch::NetWatch;
use crate::netwatch::Outcome;
use crate::netwatch::PROXY_KEYS;
use crate::netwatch::Reach;
use crate::netwatch::Resolve;
use crate::netwatch::Resolving;
use crate::netwatch::SystemResolve;
use crate::netwatch::Trust;
use crate::netwatch::TrustWork;
use crate::netwatch::Waiter;
use crate::netwatch::base_url_of;
use crate::netwatch::config_refusal;
use crate::netwatch::dest_of;
use crate::netwatch::env_proxy;
use crate::netwatch::probe_once;
use crate::netwatch::proxy_of;

const S: Duration = Duration::from_secs(1);

fn t0() -> (Instant, SystemTime) {
    (
        Instant::now() + Duration::from_secs(3600),
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000),
    )
}

/// One probe, measured at `t` on both clocks.
fn measured(st: &mut NetState, outcome: Outcome, t: Instant, w: SystemTime) {
    assert!(st.begin(t));
    st.finish(outcome, t, w);
}

/// Asks expire lapsed leases themselves while a stuck probe keeps the task from pruning, and
/// start no second probe (aterm's).
#[tokio::test(start_paused = true)]
async fn asks_expire_churned_threads_even_while_the_probe_is_stuck() {
    let (t, w) = t0();
    let mut st = NetState::default();
    assert!(st.begin(t));
    for n in 0..128_u32 {
        let elapsed = NET_LEASE * n;
        assert_eq!(st.ask("live", t + elapsed, w + elapsed), Reach::Unknown);
        st.ask(&format!("churn-{n}"), t + elapsed, w + elapsed);
        assert_eq!(
            st.waiting(t + elapsed),
            2,
            "old leases retire at their boundary"
        );
        assert!(!st.begin(t + elapsed), "no second probe is started");
    }

    // A trust reader can be stuck too. Deadlines and cancellation leave that one
    // reader owned, without parking a Tokio worker or creating another reader.
    codex_utils_rustls_provider::ensure_rustls_crypto_provider();
    let config = Arc::new(
        ClientConfig::builder()
            .with_root_certificates(RootCertStore::empty())
            .with_no_client_auth(),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let (entered, started) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let released = Mutex::new(released);
    let work = Arc::new(TrustWork::new(Arc::new(move || {
        if counted.fetch_add(1, Ordering::SeqCst) == 0 {
            entered.send(()).expect("the reader's start is observed");
            released
                .lock()
                .expect("unpoisoned")
                .recv_timeout(Duration::from_secs(60))
                .map_err(io::Error::other)?;
        }
        Ok(Arc::clone(&config))
    })));
    let mut custody = TrustCustody {
        release: Some(release),
        work: Arc::clone(&work),
    };
    let budget = Duration::from_secs(10);
    let mut first = Box::pin(work.load(tokio::time::Instant::now() + budget));
    let mut context = Context::from_waker(Waker::noop());
    assert!(std::future::Future::poll(first.as_mut(), &mut context).is_pending());
    started
        .recv_timeout(Duration::from_secs(60))
        .expect("the owned trust reader began");
    tokio::time::advance(budget).await;
    assert!(matches!(
        std::future::Future::poll(first.as_mut(), &mut context),
        std::task::Poll::Ready(None)
    ));
    drop(first);
    let mut cancelled = Box::pin(work.load(tokio::time::Instant::now() + budget));
    assert!(std::future::Future::poll(cancelled.as_mut(), &mut context).is_pending());
    drop(cancelled);
    let mut second = Box::pin(work.load(tokio::time::Instant::now() + budget));
    assert!(std::future::Future::poll(second.as_mut(), &mut context).is_pending());
    tokio::time::advance(budget).await;
    assert!(matches!(
        std::future::Future::poll(second.as_mut(), &mut context),
        std::task::Poll::Ready(None)
    ));
    drop(second);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the flight stays single");
    custody.finish();
    tokio::time::resume();
    assert!(
        work.load(tokio::time::Instant::now() + Duration::from_secs(60))
            .await
            .is_some(),
        "a completed flight permits a fresh load"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

/// Release and join the injected reader on success and on every assertion's unwind.
struct TrustCustody {
    release: Option<std::sync::mpsc::Sender<()>>,
    work: Arc<TrustWork>,
}

impl TrustCustody {
    fn finish(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Err(why) = self.work.finish_for_test() {
            if std::thread::panicking() {
                eprintln!("{why}; retaining the owned flight");
                std::mem::forget(Arc::clone(&self.work));
            } else {
                panic!("{why}");
            }
        }
    }
}

impl Drop for TrustCustody {
    fn drop(&mut self) {
        self.finish();
    }
}

/// The schedule: parked while nobody waits, a probe at once for the first waiter, then the Down
/// and steady cadences, one in flight at a time. NEGATIVE CONTROL: a waiter that keeps asking
/// keeps it probing.
#[test]
fn the_schedule_probes_at_once_then_15_30_60_while_someone_waits_and_parks_when_nobody_does() {
    let (t, w) = t0();
    let mut st = NetState::default();
    assert_eq!(st.next_probe(t, w), None, "nobody waits: parked");
    assert_eq!(st.ask("thread-1", t, w), Reach::Unknown, "no measure yet");
    assert_eq!(st.next_probe(t, w), Some(t), "at once");
    assert!(st.begin(t));
    assert!(!st.begin(t), "one in flight at a time");
    assert_eq!(st.next_probe(t, w), None, "none while one flies");
    st.finish(Outcome::Down, t, w);
    let mut at = t;
    for pause in [15, 30, 60, 60] {
        let due = st.next_probe(at, w).expect("a waiter");
        assert_eq!(due, at + pause * S, "after a Down: {pause} s");
        at = due;
        st.ask("thread-1", at, w);
        measured(&mut st, Outcome::Down, at, w);
    }
    measured(&mut st, Outcome::Up, at, w);
    assert_eq!(st.next_probe(at, w), Some(at + 60 * S), "after an Up");
    assert_eq!(
        st.next_probe(at + 61 * S, w),
        None,
        "the lease lapsed: parked"
    );
    st.ask("thread-1", at + 50 * S, w);
    assert!(
        st.next_probe(at + 61 * S, w).is_some(),
        "asked within it: live"
    );
}

/// A measure older than `FRESH` on either clock says nothing (the monotonic clock stops while
/// the Mac sleeps, the wall clock does not), nor does one whose wall clock stepped far back.
/// NEGATIVE CONTROL: a time sync's small step back is no sleep.
#[test]
fn a_stale_measure_reads_unknown_on_either_clock() {
    let (t, w) = t0();
    let mut st = NetState::default();
    st.ask("thread-1", t, w);
    measured(&mut st, Outcome::Up, t, w);
    assert_eq!(st.reach(t + 60 * S, w + 60 * S), Reach::Up { since: t });
    assert_eq!(st.reach(t + 121 * S, w + 60 * S), Reach::Unknown);
    assert_eq!(
        st.reach(t + 10 * S, w + 3600 * S),
        Reach::Unknown,
        "a sleep"
    );
    assert_eq!(st.reach(t + 10 * S, w - 600 * S), Reach::Unknown);
    assert_eq!(
        st.reach(t + 10 * S, w - 2 * S),
        Reach::Up { since: t },
        "a time sync's step back is no sleep"
    );
    assert_eq!(
        st.next_probe(t + 10 * S, w + 3600 * S),
        Some(t + 10 * S),
        "stale: at once"
    );
}

/// An Up measured again keeps its `since`, so a thread sees no edge each minute; a probe past
/// its budget reads Unknown, in flight and after.
#[test]
fn since_is_the_runs_start_and_an_overrun_is_unknown() {
    let (t, w) = t0();
    let mut st = NetState::default();
    st.ask("thread-1", t, w);
    measured(&mut st, Outcome::Up, t, w);
    measured(&mut st, Outcome::Up, t + 60 * S, w + 60 * S);
    assert_eq!(st.reach(t + 61 * S, w + 61 * S), Reach::Up { since: t });
    measured(&mut st, Outcome::Down, t + 120 * S, w + 120 * S);
    measured(&mut st, Outcome::Up, t + 135 * S, w + 135 * S);
    assert_eq!(
        st.reach(t + 136 * S, w + 136 * S),
        Reach::Up { since: t + 135 * S }
    );
    let began = t + 200 * S;
    assert!(st.begin(began));
    assert_eq!(
        st.reach(began + 5 * S, w + 205 * S),
        Reach::Up { since: t + 135 * S }
    );
    assert_eq!(st.reach(began + 11 * S, w + 211 * S), Reach::Unknown);
    st.finish(Outcome::Down, began + 30 * S, w + 230 * S);
    assert_eq!(st.reach(began + 30 * S, w + 230 * S), Reach::Unknown);
}

/// A self-signed certificate for `localhost` and `127.0.0.1`: the acceptor that serves it, the
/// trust that holds it (the test's only), and a trust that holds no root.
fn tls() -> (TlsAcceptor, Trust, Trust) {
    codex_utils_rustls_provider::ensure_rustls_crypto_provider();
    let CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .expect("a test certificate");
    let der = cert.der().clone();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
    let server = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![der.clone()], key)
        .expect("a test server config");
    let mut roots = RootCertStore::empty();
    roots.add(der).expect("the test certificate is a root");
    let trusting = Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    );
    let none = Arc::new(
        ClientConfig::builder()
            .with_root_certificates(RootCertStore::empty())
            .with_no_client_auth(),
    );
    (
        TlsAcceptor::from(Arc::new(server)),
        Arc::new(move || Ok(Arc::clone(&trusting))),
        Arc::new(move || Ok(Arc::clone(&none))),
    )
}

/// A loopback TLS listener on `port` (0: any): every connection's handshake served.
async fn serve_tls(acceptor: TlsAcceptor, port: u16) -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("a loopback listener");
    let port = listener.local_addr().expect("its address").port();
    let task = tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let _ = acceptor.accept(tcp).await;
            });
        }
    });
    (port, task)
}

/// A loopback port nothing listens on (bound, then let go): connecting is refused.
async fn refused_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback listener");
    listener.local_addr().expect("its address").port()
}

fn loopback(port: u16) -> Dest {
    Dest {
        host: "127.0.0.1".to_string(),
        port,
    }
}

/// A resolver that answers what `answer` makes.
struct Answer(Arc<dyn Fn() -> io::Result<Vec<SocketAddr>> + Send + Sync>);

impl Resolve for Answer {
    fn resolve(&self, _host: &str, _port: u16) -> Resolving {
        let answer = (self.0)();
        Box::pin(async move { answer })
    }
}

/// A resolver that never answers.
struct Hang;

impl Resolve for Hang {
    fn resolve(&self, _host: &str, _port: u16) -> Resolving {
        Box::pin(std::future::pending())
    }
}

const BUDGET: Duration = Duration::from_secs(10);

/// UP ONLY ON A HANDSHAKE THE TRUST VERIFIED: a local TLS listener whose certificate the test's
/// trust holds reads Up, by address and by a resolved name (IPv6 refused, IPv4 first). NEGATIVE
/// CONTROLS: the same listener under a trust that holds no root reads Unknown (a TLS refusal is
/// never Down, nor Up), and so does a server that answers the hello with garbage.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_probe_reads_up_only_on_a_handshake_its_trust_verifies() {
    let (acceptor, trusting, untrusting) = tls();
    let trusting = TrustWork::new(trusting);
    let untrusting = TrustWork::new(untrusting);
    let (port, _served) = serve_tls(acceptor, 0).await;
    assert_eq!(
        probe_once(&SystemResolve, &trusting, &loopback(port), BUDGET).await,
        Outcome::Up
    );
    let closed = refused_port().await;
    let named = Answer(Arc::new(move || {
        Ok(vec![
            SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], closed)),
            SocketAddr::from(([127, 0, 0, 1], port)),
        ])
    }));
    let localhost = Dest {
        host: "localhost".to_string(),
        port,
    };
    assert_eq!(
        probe_once(&named, &trusting, &localhost, BUDGET).await,
        Outcome::Up
    );
    assert_eq!(
        probe_once(&SystemResolve, &untrusting, &loopback(port), BUDGET).await,
        Outcome::Unknown,
        "a certificate the trust does not hold"
    );
    let garbage = TcpListener::bind("127.0.0.1:0").await.expect("a listener");
    let garbage_port = garbage.local_addr().expect("its address").port();
    let _answering = tokio::spawn(async move {
        while let Ok((mut tcp, _)) = garbage.accept().await {
            let _ = tcp.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
        }
    });
    assert_eq!(
        probe_once(&SystemResolve, &trusting, &loopback(garbage_port), BUDGET).await,
        Outcome::Unknown,
        "a garbage answer to the hello"
    );
}

/// DOWN ONLY ON A DEFINITE FAILURE: every address refused, a name with no address, an empty
/// answer. NEGATIVE CONTROLS: a resolver error that is no no-name answer, and a resolver that
/// never answers within the budget, read Unknown.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_probe_reads_down_only_on_definite_failures() {
    let (_, trusting, _) = tls();
    let trusting = TrustWork::new(trusting);
    let closed = refused_port().await;
    assert_eq!(
        probe_once(&SystemResolve, &trusting, &loopback(closed), BUDGET).await,
        Outcome::Down
    );
    let dest = Dest {
        host: "api.example.invalid".to_string(),
        port: 443,
    };
    let no_name = Answer(Arc::new(|| {
        Err(io::Error::new(io::ErrorKind::NotFound, "no such host"))
    }));
    assert_eq!(
        probe_once(&no_name, &trusting, &dest, BUDGET).await,
        Outcome::Down
    );
    let empty = Answer(Arc::new(|| Ok(Vec::new())));
    assert_eq!(
        probe_once(&empty, &trusting, &dest, BUDGET).await,
        Outcome::Down
    );
    let temporary = Answer(Arc::new(|| {
        Err(io::Error::other("temporary failure in name resolution"))
    }));
    assert_eq!(
        probe_once(&temporary, &trusting, &dest, BUDGET).await,
        Outcome::Unknown
    );
    assert_eq!(
        probe_once(&Hang, &trusting, &dest, Duration::from_millis(200)).await,
        Outcome::Unknown,
        "a resolver past the budget"
    );
}

/// A WALL WHOSE OWN FAILURE IS A TLS OR PROXY REFUSAL IS NEVER MEASURED (aterm's
/// `ApiCause::Config`): the probe's handshake could pass where the request's was refused.
/// NEGATIVE CONTROL: a connect timeout and a resolver error are the network's, measured.
#[test]
fn a_refusal_of_the_routes_own_is_never_measured() {
    for class in [
        RouteFailureClass::TlsError,
        RouteFailureClass::ProxyAuthenticationRequired,
        RouteFailureClass::InvalidProxyConfig,
        RouteFailureClass::UnsupportedProxyScheme,
        RouteFailureClass::ProxyResolutionUnavailable,
    ] {
        assert!(config_refusal(class).is_some(), "{class}");
    }
    for class in [
        RouteFailureClass::ConnectTimeout,
        RouteFailureClass::ResolverError,
    ] {
        assert_eq!(config_refusal(class), None, "{class}");
    }
}

/// THE BASE URL IS CODEX'S OWN RESOLUTION (`to_api_provider`): an API key on the default
/// provider reaches `api.openai.com`, a configured base URL is itself, and a codex-backend
/// sign-in with no ChatGPT account reaches the Codex backend. NEGATIVE CONTROLS: a ChatGPT
/// account on the Codex backend (workspace routing may move its host), an unread sign-in with
/// no base URL, and an Amazon Bedrock provider are no base URL this probe may claim.
#[test]
fn the_base_url_is_codexs_own_resolution() {
    let openai = ModelProviderInfo::create_openai_provider(None);
    assert_eq!(
        base_url_of(&openai, Some(AuthMode::ApiKey)).as_deref(),
        Ok("https://api.openai.com/v1")
    );
    assert_eq!(
        base_url_of(&openai, Some(AuthMode::Headers)).as_deref(),
        Ok("https://chatgpt.com/backend-api/codex")
    );
    let gateway = ModelProviderInfo {
        name: "gateway".to_string(),
        base_url: Some("https://gw.example:8443/v1".to_string()),
        ..ModelProviderInfo::create_openai_provider(None)
    };
    for auth in [None, Some(AuthMode::ApiKey), Some(AuthMode::Chatgpt)] {
        assert_eq!(
            base_url_of(&gateway, auth).as_deref(),
            Ok("https://gw.example:8443/v1"),
            "{auth:?}"
        );
    }
    let proxied = ModelProviderInfo::create_openai_provider(Some(
        "https://openai-proxy.example/v1".to_string(),
    ));
    assert_eq!(
        base_url_of(&proxied, Some(AuthMode::Chatgpt)).as_deref(),
        Ok("https://openai-proxy.example/v1"),
        "a base URL off the Codex backend is no workspace route"
    );
    for auth in [
        Some(AuthMode::Chatgpt),
        Some(AuthMode::ChatgptAuthTokens),
        Some(AuthMode::PersonalAccessToken),
        None,
    ] {
        assert!(base_url_of(&openai, auth).is_err(), "{auth:?}");
    }
    let backend = ModelProviderInfo::create_openai_provider(Some(
        "https://chatgpt.com/backend-api/codex".to_string(),
    ));
    assert!(base_url_of(&backend, Some(AuthMode::Chatgpt)).is_err());
    let bedrock = ModelProviderInfo {
        name: "Amazon Bedrock".to_string(),
        ..gateway
    };
    assert!(base_url_of(&bedrock, Some(AuthMode::ApiKey)).is_err());
}

/// A destination is an `https` URL's host and port, its default 443. NEGATIVE CONTROLS: a
/// plain-HTTP URL has no handshake to verify, and text that is no URL is none.
#[test]
fn a_destination_is_an_https_urls_host_and_port() {
    let dest = |host: &str, port| Dest {
        host: host.to_string(),
        port,
    };
    assert_eq!(
        dest_of("https://api.openai.com/v1"),
        Ok(dest("api.openai.com", 443))
    );
    assert_eq!(
        dest_of("https://gw.example:8443/v1"),
        Ok(dest("gw.example", 8443))
    );
    assert_eq!(dest_of("https://127.0.0.1:9/v1"), Ok(dest("127.0.0.1", 9)));
    assert_eq!(dest_of("https://[::1]:9/v1"), Ok(dest("::1", 9)));
    assert_eq!(dest_of("wss://gw.example/v1"), Ok(dest("gw.example", 443)));
    assert!(dest_of("http://localhost:11434/v1").is_err());
    assert!(dest_of("not a url").is_err());
}

/// CODEX'S OWN PROXY DECISION: any proxy variable reqwest reads, set non-empty, makes the route
/// custom, and so does a managed network policy, before the platform's settings are read.
/// NEGATIVE CONTROLS: an empty value, and `NO_PROXY` alone, name no proxy.
#[tokio::test]
async fn a_proxy_the_request_may_take_makes_the_route_custom() {
    for key in PROXY_KEYS {
        let env = |k: &str| (k == *key).then(|| "http://proxy.example:3128".to_string());
        assert_eq!(env_proxy(&env), Some(*key));
        let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
        assert_eq!(
            proxy_of(&factory, "https://api.openai.com/v1", env).await,
            Err(format!("{key} is set"))
        );
    }
    assert_eq!(env_proxy(&|_: &str| Some("  ".to_string())), None);
    assert_eq!(
        env_proxy(&|k: &str| (k == "NO_PROXY").then(|| "*".to_string())),
        None
    );
    let managed = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault)
        .with_network_policy(NetworkPolicyController::default().policy());
    assert!(
        proxy_of(&managed, "https://api.openai.com/v1", |_| None)
            .await
            .is_err()
    );
}

/// A thread standing at a destination's wall, as the test moves it.
struct Standing {
    at: Mutex<Option<Dest>>,
    decided: Mutex<Reach>,
}

impl Waiter for Standing {
    fn waits_for(&self) -> Option<Dest> {
        self.at.lock().expect("unpoisoned").clone()
    }

    fn decided(&self) -> Reach {
        *self.decided.lock().expect("unpoisoned")
    }
}

/// A stale snapshot's waiter, held in its external check while the same key is renewed.
struct Leaving {
    at: Mutex<Option<Dest>>,
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl Waiter for Leaving {
    fn waits_for(&self) -> Option<Dest> {
        let at = self.at.lock().expect("unpoisoned").clone();
        if let Some(entered) = self.entered.lock().expect("unpoisoned").take() {
            let _ = entered.send(());
            self.release
                .lock()
                .expect("unpoisoned")
                .recv_timeout(Duration::from_secs(60))
                .expect("the stale check is released");
        }
        at
    }

    fn decided(&self) -> Reach {
        Reach::Unknown
    }
}

struct ReleaseWaiter(Option<std::sync::mpsc::Sender<()>>);

impl ReleaseWaiter {
    fn release(&mut self) {
        if let Some(release) = self.0.take() {
            let _ = release.send(());
        }
    }
}

impl Drop for ReleaseWaiter {
    fn drop(&mut self) {
        self.release();
    }
}

/// A schedule a test waits for: every pause 10 ms.
fn fast() -> NetTiming {
    NetTiming {
        down_cadence: [Duration::from_millis(10); 3],
        steady: Duration::from_millis(10),
        ..NetTiming::default()
    }
}

/// Wait until `done` holds, at most a minute: a hang detector, never a latency budget.
async fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(tokio::time::Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// THE TASK TELLS EACH NEW VERDICT ONCE: a thread waiting at a local TLS listener's wall hears
/// Up once however many probes repeat it, and a thread waiting at a refused port's wall hears
/// Down. NEGATIVE CONTROLS: a thread whose decision read the verdict hears nothing more of it,
/// and one that stands at no wall is never told.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_waiting_thread_hears_each_new_verdict_once() {
    let (acceptor, trusting, _) = tls();
    let (port, _served) = serve_tls(acceptor, 0).await;
    let dest = loopback(port);
    let refused = loopback(refused_port().await);
    let told: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&told);
    let watch = NetWatch::new(
        Arc::new(SystemResolve),
        Arc::clone(&trusting),
        fast(),
        Arc::new(move |key: &str| heard.lock().expect("unpoisoned").push(key.to_string())),
        Arc::new(|| true),
    );
    let standing = |at: Option<&Dest>| {
        Arc::new(Standing {
            at: Mutex::new(at.cloned()),
            decided: Mutex::new(Reach::Unknown),
        })
    };
    let (waiting, gone) = (standing(Some(&dest)), standing(None));
    let as_waiter = |w: &Arc<Standing>| Arc::downgrade(w) as std::sync::Weak<dyn Waiter>;
    assert_eq!(
        watch.ask("waiting", as_waiter(&waiting), &dest),
        Reach::Unknown
    );
    assert_eq!(watch.ask("gone", as_waiter(&gone), &dest), Reach::Unknown);
    let told_now = || told.lock().expect("unpoisoned").clone();
    wait_until("the Up is told", || !told_now().is_empty()).await;
    assert_eq!(told_now(), vec!["waiting".to_string()]);
    let up = watch.ask("waiting", as_waiter(&waiting), &dest);
    assert!(matches!(up, Reach::Up { .. }), "{up:?}");
    let probes = watch.probes();
    wait_until("three more probes", || watch.probes() >= probes + 3).await;
    assert_eq!(told_now().len(), 1, "an Up repeated is no news");
    // The thread decided with it: three more probes tell it nothing.
    *waiting.decided.lock().expect("unpoisoned") = up;
    let probes = watch.probes();
    wait_until("three more probes", || watch.probes() >= probes + 3).await;
    assert_eq!(told_now().len(), 1, "an Up decided with is no news");
    let down_waiting = standing(Some(&refused));
    assert_eq!(
        watch.ask("down", as_waiter(&down_waiting), &refused),
        Reach::Unknown
    );
    wait_until("the Down is told", || told_now().len() == 2).await;
    assert_eq!(told_now(), vec!["waiting".to_string(), "down".to_string()]);
    let down = watch.ask("down", as_waiter(&down_waiting), &refused);
    assert!(matches!(down, Reach::Down { .. }), "{down:?}");

    // A sweep saw this key leave, but its external check has not returned. Renewing the
    // same key must replace both that registration and its told state, and really hear Up.
    let renewed_told = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&renewed_told);
    let renewed_watch = NetWatch::new(
        Arc::new(SystemResolve),
        trusting,
        fast(),
        Arc::new(move |key| heard.lock().expect("unpoisoned").push(key.to_string())),
        Arc::new(|| true),
    );
    let (entered, checked) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let mut custody = ReleaseWaiter(Some(release));
    let leaving = Arc::new(Leaving {
        at: Mutex::new(None),
        entered: Mutex::new(Some(entered)),
        release: Mutex::new(released),
    });
    let stale = Arc::downgrade(&leaving) as std::sync::Weak<dyn Waiter>;
    assert_eq!(
        renewed_watch.ask("renewed", stale.clone(), &dest),
        Reach::Unknown
    );
    tokio::time::timeout(Duration::from_secs(60), checked)
        .await
        .expect("the stale external check began")
        .expect("its witness");
    *leaving.at.lock().expect("unpoisoned") = Some(dest.clone());
    assert_eq!(
        renewed_watch.ask("renewed", stale.clone(), &dest),
        Reach::Unknown
    );
    custody.release();
    wait_until("the renewed registration hears Up", || {
        !renewed_told.lock().expect("unpoisoned").is_empty()
    })
    .await;
    assert_eq!(
        *renewed_told.lock().expect("unpoisoned"),
        vec!["renewed".to_string()]
    );
    assert!(matches!(
        renewed_watch.ask("renewed", stale, &dest),
        Reach::Up { .. }
    ));
}

/// A PROBE aterm'S `probe_api` HOLDS BACK NEVER RUNS (gap 30): a thread that took its lease
/// while the switch was on stands on at a destination whose name has no address (no socket is
/// opened), and its probes run at the cadence; once [`crate::netwatch::MayProbe`] says no, no
/// probe runs and no name is looked up however long it stands (the cadence's pause a hundred
/// times over, its lease living all the while), and the next ask with the switch on again
/// probes at once. Paused time: the task's sleeps pass on the test's clock alone.
#[tokio::test(start_paused = true)]
async fn a_probe_the_switch_holds_back_never_runs() {
    let looked_up = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&looked_up);
    let switch = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let read = Arc::clone(&switch);
    let watch = NetWatch::new(
        Arc::new(Answer(Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }))),
        Arc::new(|| Err(io::Error::other("no handshake: the name has no address"))),
        fast(),
        Arc::new(|_: &str| {}),
        Arc::new(move || read.load(Ordering::SeqCst)),
    );
    let dest = Dest {
        host: "api.invalid".to_string(),
        port: 443,
    };
    let waiting = Arc::new(Standing {
        at: Mutex::new(Some(dest.clone())),
        decided: Mutex::new(Reach::Unknown),
    });
    let as_waiter = || Arc::downgrade(&waiting) as std::sync::Weak<dyn Waiter>;
    assert_eq!(watch.ask("waiting", as_waiter(), &dest), Reach::Unknown);
    wait_until("probes run at the cadence", || watch.probes() >= 3).await;
    switch.store(false, Ordering::SeqCst);
    let probes = watch.probes();
    tokio::time::sleep(fast().steady * 100).await;
    assert_eq!(watch.probes(), probes, "no probe while the switch is off");
    assert_eq!(
        u64::try_from(looked_up.load(Ordering::SeqCst)).ok(),
        Some(probes),
        "and no name looked up"
    );
    assert_eq!(watch.leases(&dest), 1, "its lease lives all the while");
    switch.store(true, Ordering::SeqCst);
    assert!(matches!(
        watch.ask("waiting", as_waiter(), &dest),
        Reach::Down { .. }
    ));
    wait_until("the ask probes again", || watch.probes() > probes).await;
}
