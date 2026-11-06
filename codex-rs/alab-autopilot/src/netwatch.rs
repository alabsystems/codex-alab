// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE API'S REACH, MEASURED (aterm's `harness::netwatch` and aterm-gui's `harness_netprobe`,
//! ported): what codex-alab may say it measured of a thread's route to its API while the thread
//! waits at a network wall (`decide::Wall::Network`: Codex's `HttpConnectionFailed` or
//! `ResponseStreamConnectionFailed` with no status). The decider holds the wall while the API is
//! measured down and types `api-back@v1` as soon as it is measured up (CONTRACT §5, the API row),
//! so this module says exactly what a measure may claim.
//!
//! * [`Route`]: which destination the thread's requests take. Only a route the probe reproduces
//!   is measured ([`Route::Direct`]): the base URL codex resolves for the thread's provider and
//!   sign-in ([`base_url_of`], `ModelProviderInfo::to_api_provider`), over `https`, with codex's
//!   own proxy decision for it direct ([`proxy_of`]), and a wall whose own failure is a TLS or
//!   proxy refusal ([`config_refusal`]) never measured: the probe's handshake could pass where
//!   the request's was refused. ChatGPT workspace routing may move a ChatGPT account's host to
//!   one only core sees, so that route is [`Route::Custom`] too. A custom route reads
//!   [`Reach::Unknown`], and its wall is tried on the time ladder as before.
//! * [`Outcome`]: `Down` only on a definite failure (the name does not resolve, or every address
//!   is refused or unreachable by the kernel's word), `Up` on a completed handshake that codex's
//!   own trust verified (`codex_http_client::build_rustls_client_config_with_custom_ca`: the
//!   native roots and `CODEX_CA_CERTIFICATE`/`SSL_CERT_FILE`), with no request byte sent, and
//!   `Unknown` for anything else (a timeout, a TLS failure, a probe past its budget), so a slow
//!   network never reads as an outage.
//! * [`NetState`]: one destination's measure and schedule (aterm's verbatim). Each waiting thread
//!   holds a lease; with no fresh measure a probe runs at once, then after a Down at 15 s, 30 s
//!   and every 60 s, otherwise every 60 s, and not at all without a live lease. A measure older
//!   than [`FRESH`] on either clock is no measure, and `since` is the start of the measure's run,
//!   so a thread sees an edge only on a change.
//! * [`NetWatch`]: one task per destination carries the state out. aterm's loop asks at every
//!   wait step; a codex-alab thread waits on a timer instead, so the task renews the lease of
//!   every thread still standing at that destination's wall ([`Waiter`]) and, on a verdict the
//!   thread did not decide with, has it look again (the edge).
//! * aterm's `probe_api` (gap 30): off, no thread stands at a route (its host's `reach` asks
//!   its probe only with the switch on), so no lease is taken and no task starts, and a probe a
//!   lease taken before it went off would still run is held back ([`MayProbe`]): no TLS connect
//!   is made while the switch is off, where aterm's lease lapses on its own.
//! * A usage window's due resume is measured the same way (2026-10-24): its wall's route is read
//!   as a network wall's, and the thread stands at it once the resume comes due and asks
//!   ([`crate::decide::ThreadState::net_asked`]), never through the hours before the reset, so
//!   a resume is never typed into an API measured down.
//!
//! Core's own wait for the network (`unbounded_connection_retries`, on by default) retries a
//! sampling request whose connection failed inside its turn, 5 s doubling to every 60 s, and that
//! turn never ends at the wall: this measure answers the network walls that do end a turn (that
//! feature off, a remote compaction's request).

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::Weak;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use codex_core::config::Config;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_http_client::OutboundProxyRoute;
use codex_http_client::RouteFailureClass;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::auth::AuthMode;
use codex_protocol::error::CodexErrorDetails;
use rustls::ClientConfig;
use rustls_pki_types::ServerName;
use tokio::net::TcpStream;
use tokio::sync::Notify;

use crate::supervisor::clock;

/// One probe's whole budget: resolve, connect and handshake together.
pub const PROBE_BUDGET: Duration = Duration::from_secs(10);
/// How long one thread's ask keeps the probe running.
pub const NET_LEASE: Duration = Duration::from_secs(60);
/// How old a measure may be, on either clock, and still say anything.
pub const FRESH: Duration = Duration::from_secs(120);
/// The pause after the first, second, and every later Down in a row.
pub const DOWN_CADENCE: [Duration; 3] = [
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(60),
];
/// The pause after an Up or an Unknown.
pub const STEADY_CADENCE: Duration = Duration::from_secs(60);
/// A wall clock that steps back this little (a time sync) is not a sleep.
const CLOCK_SLACK: Duration = Duration::from_secs(5);

/// The variables reqwest's own proxy discovery reads (codex's `ReqwestDefault` policy). Any
/// non-empty one, `NO_PROXY` aside, makes the route custom: a route misread as custom costs only
/// the time ladder, one misread as direct a hold.
pub const PROXY_KEYS: &[&str] = &[
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
];

/// The schedule's durations (aterm's constants; a test shortens them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetTiming {
    pub budget: Duration,
    pub lease: Duration,
    pub fresh: Duration,
    pub down_cadence: [Duration; 3],
    pub steady: Duration,
}

impl Default for NetTiming {
    fn default() -> Self {
        NetTiming {
            budget: PROBE_BUDGET,
            lease: NET_LEASE,
            fresh: FRESH,
            down_cadence: DOWN_CADENCE,
            steady: STEADY_CADENCE,
        }
    }
}

/// The API's reach as the decider reads it (aterm's `Reach`): a fresh measure's verdict with the
/// start of its run, else unknown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Reach {
    Up {
        since: Instant,
    },
    Down {
        since: Instant,
    },
    /// No fresh measure, a custom route, or a wall the reach does not answer.
    #[default]
    Unknown,
}

/// What one probe concluded ([`NetState::finish`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A handshake to the destination completed and codex's trust verified it.
    Up,
    /// A definite failure: the name did not resolve, or every address was refused or
    /// unreachable by the kernel's own word.
    Down,
    /// Anything else: a timeout, a TLS or certificate failure, another error.
    Unknown,
}

/// A destination the probe reproduces: a host (a name, or an address literal) and its port.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Dest {
    pub host: String,
    pub port: u16,
}

/// The route a thread's API requests take, as far as codex-alab can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Direct, over TLS, to this destination: the probe reproduces it.
    Direct(Dest),
    /// Anything else, or what decides it could not be read: the reason, for the journal.
    Custom(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Measure {
    outcome: Outcome,
    /// When it was taken, on both clocks.
    at: Instant,
    at_wall: SystemTime,
    /// When its run began: the first of the measures in a row that say it.
    since: Instant,
}

/// One destination's measure and its schedule (aterm's `NetState`, the module header). Pure:
/// every clock is passed in.
#[derive(Debug)]
pub struct NetState {
    timing: NetTiming,
    /// Each asking thread's last ask, while its lease lives.
    waiters: HashMap<String, Instant>,
    last: Option<Measure>,
    /// When the probe in flight began (one at a time).
    flight: Option<Instant>,
    /// Downs in a row (the cadence after a Down).
    downs: u32,
}

impl Default for NetState {
    fn default() -> Self {
        Self::with_timing(NetTiming::default())
    }
}

impl NetState {
    /// A state on `timing`.
    #[must_use]
    pub fn with_timing(timing: NetTiming) -> Self {
        Self {
            timing,
            waiters: HashMap::new(),
            last: None,
            flight: None,
            downs: 0,
        }
    }

    /// One probe's budget.
    #[must_use]
    pub fn budget(&self) -> Duration {
        self.timing.budget
    }

    /// A thread asks for `who`: its lease renewed, and the verdict now ([`Self::reach`]).
    pub fn ask(&mut self, who: &str, now: Instant, wall: SystemTime) -> Reach {
        // The ask expires lapsed leases too: a stuck probe cannot let them pile up.
        self.waiting(now);
        self.waiters.insert(who.to_string(), now);
        self.reach(now, wall)
    }

    /// The verdict: the last measure (`Up`/`Down` with the start of its run) while it is fresh
    /// on both clocks and no probe has overrun its budget, else [`Reach::Unknown`].
    #[must_use]
    pub fn reach(&self, now: Instant, wall: SystemTime) -> Reach {
        if self
            .flight
            .is_some_and(|began| now.saturating_duration_since(began) > self.timing.budget)
        {
            return Reach::Unknown;
        }
        match self.fresh(now, wall).map(|m| (m.outcome, m.since)) {
            Some((Outcome::Up, since)) => Reach::Up { since },
            Some((Outcome::Down, since)) => Reach::Down { since },
            _ => Reach::Unknown,
        }
    }

    /// The last measure, while it is younger than [`FRESH`] on the monotonic clock and the
    /// wall clock (a wall clock that went back more than a time sync would is no evidence
    /// either way: stale).
    fn fresh(&self, now: Instant, wall: SystemTime) -> Option<&Measure> {
        let m = self.last.as_ref()?;
        let on_wall = match wall.duration_since(m.at_wall) {
            Ok(d) => d,
            Err(back) if back.duration() <= CLOCK_SLACK => Duration::ZERO,
            Err(_) => return None,
        };
        (now.saturating_duration_since(m.at) < self.timing.fresh && on_wall < self.timing.fresh)
            .then_some(m)
    }

    /// The live leases, the lapsed ones dropped.
    pub fn waiting(&mut self, now: Instant) -> usize {
        let lease = self.timing.lease;
        self.waiters
            .retain(|_, asked| now.saturating_duration_since(*asked) < lease);
        self.waiters.len()
    }

    /// When the next probe is due: `None` while no lease lives or one is in flight; else the
    /// instant — at once with no fresh measure, else the cadence after the last. An instant not
    /// after `now` is now.
    pub fn next_probe(&mut self, now: Instant, wall: SystemTime) -> Option<Instant> {
        if self.waiting(now) == 0 || self.flight.is_some() {
            return None;
        }
        let Some(m) = self.fresh(now, wall) else {
            return Some(now);
        };
        let pause = match m.outcome {
            Outcome::Down => {
                let n = usize::try_from(self.downs.max(1) - 1).unwrap_or(usize::MAX);
                let cadence = &self.timing.down_cadence;
                cadence[n.min(cadence.len() - 1)]
            }
            Outcome::Up | Outcome::Unknown => self.timing.steady,
        };
        Some(m.at.checked_add(pause).unwrap_or(now))
    }

    /// A probe begins: `false` when one is in flight already (one at a time).
    pub fn begin(&mut self, now: Instant) -> bool {
        if self.flight.is_some() {
            return false;
        }
        self.flight = Some(now);
        true
    }

    /// A probe ended with `outcome`, read as [`Outcome::Unknown`] when it overran its budget. A
    /// measure that repeats the last fresh one continues its run (`since` kept); any other
    /// starts one.
    pub fn finish(&mut self, outcome: Outcome, now: Instant, wall: SystemTime) {
        let overran = self
            .flight
            .take()
            .is_some_and(|began| now.saturating_duration_since(began) > self.timing.budget);
        let outcome = if overran { Outcome::Unknown } else { outcome };
        let since = match self.fresh(now, wall) {
            Some(prev) if prev.outcome == outcome => prev.since,
            _ => now,
        };
        self.downs = if outcome == Outcome::Down {
            self.downs.saturating_add(1)
        } else {
            0
        };
        self.last = Some(Measure {
            outcome,
            at: now,
            at_wall: wall,
            since,
        });
    }
}

/// A WALL WHOSE OWN FAILURE IS THE ROUTE'S CONFIGURATION (aterm's `ApiCause::Config`): a TLS or
/// proxy refusal codex classified (`RouteAwareRequestError::failure_class`). The probe's own
/// handshake could pass where the request's was refused, so such a wall is never measured.
#[must_use]
pub fn config_refusal(class: RouteFailureClass) -> Option<String> {
    match class {
        RouteFailureClass::TlsError
        | RouteFailureClass::ProxyAuthenticationRequired
        | RouteFailureClass::InvalidProxyConfig
        | RouteFailureClass::UnsupportedProxyScheme
        | RouteFailureClass::ProxyResolutionUnavailable => {
            Some(format!("the request's own failure was a refusal ({class})"))
        }
        RouteFailureClass::ConnectTimeout | RouteFailureClass::ResolverError => None,
    }
}

/// The failure class of a network wall's error, where its details name one.
#[must_use]
pub fn failure_of(details: &CodexErrorDetails) -> Option<RouteFailureClass> {
    match details {
        CodexErrorDetails::ConnectionFailed(failed) => failed.source.failure_class(),
        CodexErrorDetails::ResponseStreamFailed(failed) => failed.source.failure_class(),
        _ => None,
    }
}

/// THE BASE URL CODEX RESOLVES for `provider` under the sign-in `auth`
/// (`ModelProviderInfo::to_api_provider`, the provider's `api_provider`), or why it cannot be
/// told: an Amazon Bedrock provider builds its own; a ChatGPT account on the Codex backend may
/// have its host moved by workspace routing, which only core sees; and with no base URL
/// configured, the default follows a sign-in that could not be read.
pub fn base_url_of(provider: &ModelProviderInfo, auth: Option<AuthMode>) -> Result<String, String> {
    if provider.is_amazon_bedrock() {
        return Err("an Amazon Bedrock provider builds its own URL".to_string());
    }
    if provider.supports_codex_backend_routes() && auth.is_none_or(AuthMode::has_chatgpt_account) {
        return Err("ChatGPT workspace routing may move the host".to_string());
    }
    if provider.base_url.is_none() && auth.is_none() {
        return Err("no base URL is configured and the sign-in could not be read".to_string());
    }
    provider
        .to_api_provider(auth)
        .map(|api| api.base_url)
        .map_err(|e| format!("the provider's URL could not be resolved: {e}"))
}

/// The destination of an `https` (or `wss`) URL: its host and port. A plain-HTTP URL has no
/// handshake to verify, and is no destination this probe measures.
pub fn dest_of(url: &str) -> Result<Dest, String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("the base URL does not parse: {e}"))?;
    if !matches!(parsed.scheme(), "https" | "wss") {
        return Err(format!(
            "a {} base URL has no TLS handshake to verify",
            parsed.scheme()
        ));
    }
    let host = match parsed.host() {
        Some(url::Host::Domain(name)) => name.to_string(),
        Some(url::Host::Ipv4(addr)) => addr.to_string(),
        Some(url::Host::Ipv6(addr)) => addr.to_string(),
        None => return Err("the base URL names no host".to_string()),
    };
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| "the base URL names no port".to_string())?;
    Ok(Dest { host, port })
}

/// Whether codex's own proxy decision for `url` is direct (`HttpClientFactory`, from the
/// thread's config): never under a managed network policy; under `RespectSystemProxy`, codex's
/// own resolver must route it direct; under `ReqwestDefault`, no proxy variable reqwest reads
/// may be set ([`PROXY_KEYS`], read through `env`) and the platform's settings must leave it
/// direct ([`system_direct`]).
pub async fn proxy_of(
    factory: &HttpClientFactory,
    url: &str,
    env: impl Fn(&str) -> Option<String>,
) -> Result<(), String> {
    if factory.network_policy().is_managed() {
        return Err("a managed network policy governs the route".to_string());
    }
    match factory.outbound_proxy_policy() {
        OutboundProxyPolicy::RespectSystemProxy => {
            match factory.resolve_proxy_route_async(url.to_string()).await {
                Ok(OutboundProxyRoute::Direct) => Ok(()),
                Ok(_) => Err("codex's proxy resolution routes it through a proxy".to_string()),
                Err(e) => Err(format!("codex's proxy resolution failed: {e}")),
            }
        }
        OutboundProxyPolicy::ReqwestDefault => {
            if let Some(key) = env_proxy(&env) {
                return Err(format!("{key} is set"));
            }
            let url = url.to_string();
            tokio::task::spawn_blocking(move || system_direct(&url))
                .await
                .map_err(|e| format!("the system proxy settings could not be read: {e}"))?
        }
    }
}

/// The first proxy variable `env` sets non-empty, if any.
pub fn env_proxy(env: &impl Fn(&str) -> Option<String>) -> Option<&'static str> {
    PROXY_KEYS
        .iter()
        .copied()
        .find(|key| env(key).is_some_and(|value| !value.trim().is_empty()))
}

/// macOS: reqwest's default reads the system proxy settings, and only a direct answer for this
/// destination is direct (codex's `macos_system_proxy_configuration`, which runs no PAC script).
#[cfg(target_os = "macos")]
fn system_direct(url: &str) -> Result<(), String> {
    use codex_http_client::MacosSystemProxyConfiguration;
    match codex_http_client::macos_system_proxy_configuration(url) {
        MacosSystemProxyConfiguration::Direct => Ok(()),
        other => Err(format!("the macOS system proxy settings ({other:?})")),
    }
}

/// Windows: reqwest's default reads the registry's proxy, which codex-alab does not read.
#[cfg(target_os = "windows")]
fn system_direct(_url: &str) -> Result<(), String> {
    Err("Windows' system proxy settings are not read".to_string())
}

/// Elsewhere reqwest's default reads only the environment.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn system_direct(_url: &str) -> Result<(), String> {
    Ok(())
}

/// THE ROUTE OF A THREAD AT A NETWORK WALL: [`Route::Direct`] only where the wall's own failure
/// is no refusal of the route's ([`config_refusal`]), the base URL codex resolves is known
/// ([`base_url_of`]) and `https` ([`dest_of`]), and codex's proxy decision for it is direct
/// ([`proxy_of`]). `direct_proxy` skips the proxy read (a test's own route).
pub async fn route_at_wall(
    details: &CodexErrorDetails,
    config: &Config,
    auth: Option<AuthMode>,
    direct_proxy: bool,
) -> Route {
    if let Some(why) = failure_of(details).and_then(config_refusal) {
        return Route::Custom(why);
    }
    let url = match base_url_of(&config.model_provider, auth) {
        Ok(url) => url,
        Err(why) => return Route::Custom(why),
    };
    let dest = match dest_of(&url) {
        Ok(dest) => dest,
        Err(why) => return Route::Custom(why),
    };
    if !direct_proxy
        && let Err(why) = proxy_of(&config.http_client_factory(), &url, |key| {
            std::env::var(key).ok()
        })
        .await
    {
        return Route::Custom(why);
    }
    Route::Direct(dest)
}

/// A boxed future the probe's resolver returns.
pub type Resolving = Pin<Box<dyn Future<Output = io::Result<Vec<SocketAddr>>> + Send + 'static>>;

/// The probe's resolver: the seam the tests replace (aterm's `Probe::resolve`).
pub trait Resolve: Send + Sync {
    /// The host's addresses.
    fn resolve(&self, host: &str, port: u16) -> Resolving;
}

/// The system resolver (tokio's `lookup_host`, the platform's `getaddrinfo`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemResolve;

impl Resolve for SystemResolve {
    fn resolve(&self, host: &str, port: u16) -> Resolving {
        let host = host.to_string();
        Box::pin(async move { Ok(tokio::net::lookup_host((host, port)).await?.collect()) })
    }
}

/// The factory for current handshake trust. Concurrent probes share its owned flight.
pub type Trust = Arc<dyn Fn() -> io::Result<Arc<ClientConfig>> + Send + Sync>;

#[derive(Clone)]
enum TrustResult {
    Pending,
    Finished(Option<Arc<ClientConfig>>),
}

struct TrustFlight {
    result: tokio::sync::watch::Receiver<TrustResult>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// One owned trust reader, shared by every destination of a [`NetWatch`]. A deadline or
/// cancelled probe leaves its flight in custody: another probe shares it, never starts
/// another blocked reader. A completed flight is replaced for the next fresh trust load.
/// The dedicated worker is not a Tokio blocking task, and dropping it never joins it.
pub struct TrustWork {
    build: Trust,
    flight: Mutex<Option<Arc<TrustFlight>>>,
}

impl TrustWork {
    /// Own the reader and its outstanding flight, if any.
    #[must_use]
    pub fn new(build: Trust) -> Self {
        Self {
            build,
            flight: Mutex::new(None),
        }
    }

    pub(crate) async fn load(&self, deadline: tokio::time::Instant) -> Option<Arc<ClientConfig>> {
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        let flight = {
            let mut current = lock(&self.flight);
            let running = current.as_ref().filter(|flight| {
                lock(&flight.worker)
                    .as_ref()
                    .is_some_and(|worker| !worker.is_finished())
            });
            if let Some(flight) = running {
                Arc::clone(flight)
            } else {
                let (sender, result) = tokio::sync::watch::channel(TrustResult::Pending);
                let flight = Arc::new(TrustFlight {
                    result,
                    worker: Mutex::new(None),
                });
                let owned = Arc::clone(&flight);
                let build = Arc::clone(&self.build);
                let failed = sender.clone();
                match std::thread::Builder::new()
                    .name("alab-net-trust".to_string())
                    .spawn(move || {
                        let result = build().ok();
                        let _ = sender.send(TrustResult::Finished(result));
                        drop(owned);
                    }) {
                    Ok(worker) => *lock(&flight.worker) = Some(worker),
                    Err(_) => {
                        let _ = failed.send(TrustResult::Finished(None));
                    }
                }
                *current = Some(Arc::clone(&flight));
                flight
            }
        };
        let mut result = flight.result.clone();
        loop {
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            let ready = { result.borrow().clone() };
            if let TrustResult::Finished(config) = ready {
                return config;
            }
            if !matches!(
                tokio::time::timeout_at(deadline, result.changed()).await,
                Ok(Ok(()))
            ) {
                return None;
            }
        }
    }

    /// Join this test's released injected reader, with a one-minute hang detector.
    #[cfg(test)]
    pub(crate) fn finish_for_test(&self) -> Result<(), String> {
        let Some(flight) = lock(&self.flight).clone() else {
            return Ok(());
        };
        let Some(worker) = lock(&flight.worker).take() else {
            return Ok(());
        };
        let until = Instant::now() + Duration::from_secs(60);
        while !worker.is_finished() {
            if Instant::now() >= until {
                *lock(&flight.worker) = Some(worker);
                return Err("the injected trust reader never ended".to_string());
            }
            std::thread::park_timeout(Duration::from_millis(1));
        }
        worker
            .join()
            .map_err(|_| "the injected trust reader panicked".to_string())
    }
}

/// Codex's own trust: the native roots and its custom CA bundle.
#[must_use]
pub fn codex_trust() -> Trust {
    Arc::new(|| {
        codex_http_client::build_rustls_client_config_with_custom_ca().map_err(io::Error::other)
    })
}

/// A failure that says the address cannot be reached from here: the kernel's own word, never a
/// timeout.
fn definite_failure(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::NetworkUnreachable
            | io::ErrorKind::HostUnreachable
            | io::ErrorKind::NetworkDown
    ) || raw_unreachable(e)
}

/// A resolver answer that specifically says the name has no address. On macOS std reports
/// `getaddrinfo` errors as `Uncategorized` with no raw OS code, so only the known no-name
/// wording tells that answer from a temporary DNS failure. An unmatched or changed message stays
/// Unknown: it must never suppress the retry ladder.
fn definite_name_failure(e: &io::Error) -> bool {
    if e.kind() == io::ErrorKind::NotFound {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        e.to_string()
            == "failed to lookup address information: nodename nor servname provided, or not known"
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// The same four answers by raw errno, for a Unix error std has not mapped to its kind.
#[cfg(unix)]
fn raw_unreachable(e: &io::Error) -> bool {
    e.raw_os_error().is_some_and(|code| {
        [
            libc::ECONNREFUSED,
            libc::ENETUNREACH,
            libc::EHOSTUNREACH,
            libc::ENETDOWN,
        ]
        .contains(&code)
    })
}

#[cfg(not(unix))]
fn raw_unreachable(_: &io::Error) -> bool {
    false
}

/// `addrs`, IPv4 and IPv6 interleaved, IPv4 first.
fn interleaved(addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let (v4, v6): (Vec<SocketAddr>, Vec<SocketAddr>) =
        addrs.into_iter().partition(SocketAddr::is_ipv4);
    let mut out = Vec::with_capacity(v4.len() + v6.len());
    let (mut a, mut b) = (v4.into_iter(), v6.into_iter());
    loop {
        match (a.next(), b.next()) {
            (None, None) => break,
            (x, y) => {
                out.extend(x);
                out.extend(y);
            }
        }
    }
    out
}

/// A TLS handshake to `host` over `tcp`, verified against `trust`, with no request sent. The
/// session is dropped at once.
async fn verified_handshake(
    tcp: TcpStream,
    host: &str,
    trust: Arc<ClientConfig>,
) -> io::Result<()> {
    let name = ServerName::try_from(host.to_string())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    tokio_rustls::TlsConnector::from(trust)
        .connect(name, tcp)
        .await
        .map(drop)
}

/// ONE PROBE of `dest` under `budget` (aterm's `probe_once`): resolve, connect to the addresses
/// IPv4 and IPv6 interleaved, each attempt given an equal share of the budget left, and complete
/// a handshake verified against `trust` on the first connection. Trust reads share its owned
/// flight and are bounded by the same deadline, including after a cancelled probe. Down only on a definite
/// failure, Up only on a verified handshake, else Unknown.
pub async fn probe_once(
    resolve: &dyn Resolve,
    trust: &TrustWork,
    dest: &Dest,
    budget: Duration,
) -> Outcome {
    let deadline = tokio::time::Instant::now() + budget;
    let in_time = |definite: Outcome| {
        if tokio::time::Instant::now() < deadline {
            definite
        } else {
            Outcome::Unknown
        }
    };
    let addrs =
        match tokio::time::timeout_at(deadline, resolve.resolve(&dest.host, dest.port)).await {
            Ok(Ok(addrs)) if !addrs.is_empty() => interleaved(addrs),
            // No address, or a resolver answer that specifically says no name: the request fails
            // the same way. A temporary or unrecognized resolver error, or one past the budget, is
            // no definite word.
            Ok(Ok(_)) => return in_time(Outcome::Down),
            Ok(Err(e)) if definite_name_failure(&e) => return in_time(Outcome::Down),
            Ok(Err(_)) | Err(_) => return Outcome::Unknown,
        };
    let n = addrs.len();
    let mut definite = 0usize;
    for (i, addr) in addrs.into_iter().enumerate() {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        let share = left / u32::try_from(n - i).unwrap_or(u32::MAX);
        if share.is_zero() {
            return Outcome::Unknown;
        }
        match tokio::time::timeout(share, TcpStream::connect(addr)).await {
            // The first connection decides: a verified handshake, or nothing this probe can say
            // (a captive portal, an intercepting proxy, a clock off after sleep, a handshake that
            // timed out, a trust that could not be built).
            Ok(Ok(tcp)) => {
                let Some(config) = trust.load(deadline).await else {
                    return Outcome::Unknown;
                };
                return match tokio::time::timeout_at(
                    deadline,
                    verified_handshake(tcp, &dest.host, config),
                )
                .await
                {
                    Ok(Ok(())) => in_time(Outcome::Up),
                    Ok(Err(_)) | Err(_) => Outcome::Unknown,
                };
            }
            Ok(Err(e)) if definite_failure(&e) => definite += 1,
            Ok(Err(_)) | Err(_) => {}
        }
    }
    if definite == n {
        in_time(Outcome::Down)
    } else {
        Outcome::Unknown
    }
}

/// A thread a destination's probe runs for.
pub trait Waiter: Send + Sync {
    /// The destination it stands at a network wall for, while it does.
    fn waits_for(&self) -> Option<Dest>;
    /// The reach its last decision read.
    fn decided(&self) -> Reach;
}

/// What a verdict a thread did not decide with does: it looks again (its key).
pub type OnEdge = Arc<dyn Fn(&str) + Send + Sync>;

/// WHETHER A DUE PROBE MAY RUN NOW: `false` while aterm's `probe_api` is off
/// ([`crate::supervisor::Supervisor::probe_off`]). A probe it holds back waits, parked, for the
/// next ask, which only a thread the switch lets stand at a route makes.
pub type MayProbe = Arc<dyn Fn() -> bool + Send + Sync>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One destination's state, its waiters, and the wake of its task.
struct DestWatch {
    dest: Dest,
    state: Mutex<NetState>,
    waiters: Mutex<HashMap<String, Arc<Registration>>>,
    wake: Notify,
}

/// A particular ask owns its weak waiter and last told verdict. Replacing the same key
/// replaces this identity too, so an older sweep cannot remove or silence the renewed ask.
struct Registration {
    waiter: Weak<dyn Waiter>,
    told: Mutex<Reach>,
}

/// THE PROBES (aterm-gui's `NetProbe`, on tokio): one task per destination, started on its first
/// ask, parked while no lease lives or [`MayProbe`] holds a due probe back, one probe at a time.
/// The state's lock is never held across I/O or a waiter's call.
pub struct NetWatch {
    timing: NetTiming,
    resolve: Arc<dyn Resolve>,
    trust: TrustWork,
    on_edge: OnEdge,
    may_probe: MayProbe,
    dests: Mutex<HashMap<Dest, Arc<DestWatch>>>,
    /// Probes run (the tests' witness).
    probes: std::sync::atomic::AtomicU64,
}

impl NetWatch {
    /// Probes through `resolve` and `trust` on `timing`, each only while `may_probe` lets it;
    /// `on_edge` has a thread look again.
    pub fn new(
        resolve: Arc<dyn Resolve>,
        trust: Trust,
        timing: NetTiming,
        on_edge: OnEdge,
        may_probe: MayProbe,
    ) -> Arc<NetWatch> {
        Arc::new(NetWatch {
            timing,
            resolve,
            trust: TrustWork::new(trust),
            on_edge,
            may_probe,
            dests: Mutex::new(HashMap::new()),
            probes: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// How many probes ran.
    pub fn probes(&self) -> u64 {
        self.probes.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The leases living at `dest` now (tests).
    #[cfg(test)]
    pub(crate) fn leases(&self, dest: &Dest) -> usize {
        let watch = lock(&self.dests).get(dest).cloned();
        watch.map_or(0, |watch| lock(&watch.state).waiting(clock()))
    }

    /// The destinations whose task started (tests).
    #[cfg(test)]
    pub(crate) fn watched(&self) -> usize {
        lock(&self.dests).len()
    }

    /// THREAD `who` ASKS while it stands at `dest`'s wall: its lease renewed, the verdict now,
    /// and the destination's task woken (started on the first ask) — never blocked on a probe.
    pub fn ask(self: &Arc<Self>, who: &str, waiter: Weak<dyn Waiter>, dest: &Dest) -> Reach {
        let watch = {
            let mut dests = lock(&self.dests);
            match dests.get(dest) {
                Some(watch) => Arc::clone(watch),
                None => {
                    let watch = Arc::new(DestWatch {
                        dest: dest.clone(),
                        state: Mutex::new(NetState::with_timing(self.timing.clone())),
                        waiters: Mutex::new(HashMap::new()),
                        wake: Notify::new(),
                    });
                    dests.insert(dest.clone(), Arc::clone(&watch));
                    tokio::spawn(Arc::clone(self).run(Arc::clone(&watch)));
                    watch
                }
            }
        };
        let reach = {
            let mut waiters = lock(&watch.waiters);
            let reach = lock(&watch.state).ask(who, clock(), SystemTime::now());
            waiters.insert(
                who.to_string(),
                Arc::new(Registration {
                    waiter,
                    told: Mutex::new(reach),
                }),
            );
            reach
        };
        watch.wake.notify_one();
        reach
    }

    /// The waiters still standing at `watch`'s wall; the rest are forgotten.
    fn standing(watch: &DestWatch) -> Vec<(String, Arc<Registration>, Arc<dyn Waiter>)> {
        let all: Vec<(String, Arc<Registration>)> = lock(&watch.waiters)
            .iter()
            .map(|(key, w)| (key.clone(), w.clone()))
            .collect();
        let mut live = Vec::new();
        let mut gone = Vec::new();
        for (key, registration) in all {
            match registration.waiter.upgrade() {
                Some(waiter) if waiter.waits_for().as_ref() == Some(&watch.dest) => {
                    live.push((key, registration, waiter));
                }
                _ => gone.push((key, registration)),
            }
        }
        if !gone.is_empty() {
            let mut waiters = lock(&watch.waiters);
            for (key, registration) in gone {
                if waiters
                    .get(&key)
                    .is_some_and(|current| Arc::ptr_eq(current, &registration))
                {
                    waiters.remove(&key);
                }
            }
        }
        live
    }

    /// The task: each standing waiter's lease renewed at every look, a probe when one is due and
    /// [`MayProbe`] lets it, parked while no lease lives or it does not; after each probe, every
    /// waiter whose decision read another verdict looks again, once per verdict.
    async fn run(self: Arc<Self>, watch: Arc<DestWatch>) {
        loop {
            let live = Self::standing(&watch);
            let now = clock();
            let wall = SystemTime::now();
            let due = {
                let mut st = lock(&watch.state);
                for (key, _, _) in &live {
                    st.ask(key, now, wall);
                }
                st.next_probe(now, wall)
            };
            match due {
                None => {
                    watch.wake.notified().await;
                    continue;
                }
                Some(at) if at > now => {
                    tokio::select! {
                        () = tokio::time::sleep_until(tokio::time::Instant::from_std(at)) => {}
                        () = watch.wake.notified() => {}
                    }
                    continue;
                }
                Some(_) => {}
            }
            // aterm's `probe_api` off: the due probe waits for the next ask.
            if !(self.may_probe)() {
                watch.wake.notified().await;
                continue;
            }
            if !lock(&watch.state).begin(now) {
                continue;
            }
            self.probes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let budget = lock(&watch.state).budget();
            let outcome = probe_once(&*self.resolve, &self.trust, &watch.dest, budget).await;
            let (now, wall) = (clock(), SystemTime::now());
            let reach = {
                let mut st = lock(&watch.state);
                st.finish(outcome, now, wall);
                st.reach(now, wall)
            };
            for (key, registration, waiter) in Self::standing(&watch) {
                let fresh_news = waiter.decided() != reach && {
                    let mut told = lock(&registration.told);
                    let changed = *told != reach;
                    *told = reach;
                    changed
                };
                if fresh_news {
                    (self.on_edge)(&key);
                }
            }
        }
    }
}
