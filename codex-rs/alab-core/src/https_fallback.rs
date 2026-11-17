// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! How long a session that fell back from the Responses WebSocket stays on HTTPS.
//!
//! Upstream keeps the fallback for the rest of the session, which it sizes for sessions of
//! hours. A session here runs for days: one outage of minutes would leave it re-sending its
//! whole history on every request from then on. So a fallback that followed failed requests
//! ends once requests have gone an interval without failing, at the start of a request
//! that is not a retry.
//!
//! The rules are the ones upstream's own recovery had before it was withdrawn (openai/codex
//! #58950, withdrawn by #58989), without its background probe:
//!
//! - a fallback the server asked for (HTTP 426) stays;
//! - a retry stays on the transport its request failed on;
//! - the request that returns is a trial: if the socket does not carry it, the session
//!   is back on HTTPS at once, and that request is replayed there;
//! - returns are spaced out, and a network that keeps failing them keeps HTTPS.
//!
//! Every interval is measured on a clock that stops while the host sleeps.

use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

/// How long requests must go without failing before a return, and how long a returned
/// WebSocket must serve requests to count as working.
pub const BASE: Duration = Duration::from_secs(10 * 60);

/// Each failed return doubles the interval; after this many in a row the session keeps
/// HTTPS (10, 20, 40 and 80 minutes, then none).
pub const MAX_FAILED_RETURNS: u32 = 4;

static BASE_MS: AtomicU64 = AtomicU64::new(BASE.as_millis() as u64);

/// For a test that cannot wait out [`BASE`]: the interval sessions created from now on use.
#[doc(hidden)]
pub fn set_base(base: Duration) {
    BASE_MS.store(
        u64::try_from(base.as_millis()).unwrap_or(u64::MAX),
        Ordering::SeqCst,
    );
}

/// What a fallback's log line says about the way back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Return {
    /// A request that starts after this long tries the WebSocket again.
    After(Duration),
    /// No failed request preceded the fallback: the server refused the WebSocket.
    ServerRefused,
    /// [`MAX_FAILED_RETURNS`] returns in a row did not last.
    Exhausted,
}

/// One session's fallback state. Every method takes the current time, so the policy is
/// tested without a clock.
#[derive(Debug)]
pub struct HttpsFallback {
    base: Duration,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// When the session moved to HTTPS; `None` while it uses the WebSocket.
    since: Option<Instant>,
    /// When it last returned to the WebSocket.
    returned: Option<Instant>,
    /// When the last request that was not a retry began.
    began: Option<Instant>,
    /// When a request last failed for a reason that says the network or backend is unwell.
    unwell: Option<Instant>,
    /// The request that returned to the WebSocket has not been followed by another.
    trial: bool,
    /// Returns in a row that did not serve requests for `base`.
    failed_returns: u32,
    /// The fallback is final.
    kept: bool,
    /// A request failed, and no request has started since: the next one is its retry.
    after_failure: bool,
}

impl Default for HttpsFallback {
    fn default() -> Self {
        Self::with_base(Duration::from_millis(BASE_MS.load(Ordering::SeqCst)))
    }
}

impl HttpsFallback {
    #[must_use]
    pub fn with_base(base: Duration) -> Self {
        Self {
            base,
            state: Mutex::default(),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn interval(&self, failed_returns: u32) -> Duration {
        self.base
            .saturating_mul(1_u32.checked_shl(failed_returns).unwrap_or(u32::MAX))
    }

    /// A request failed and its caller is deciding what follows: a fallback it orders is
    /// the client's own, and the request that starts next is this one's retry. `unwell` is
    /// false for a failure that is the server's considered answer, such as a capacity
    /// limit, which does not postpone a return.
    pub fn request_failed(&self, now: Instant, unwell: bool) {
        let mut state = self.state();
        state.after_failure = true;
        if unwell {
            state.unwell = Some(now);
        }
    }

    /// Whether the request that returned to the WebSocket is still the last to have begun.
    /// Its failure sends the session back to HTTPS without spending a retry budget on the
    /// socket.
    #[must_use]
    pub fn on_trial(&self) -> bool {
        self.state().trial
    }

    /// The session moved from the WebSocket to HTTPS.
    pub fn fell_back(&self, now: Instant) -> Return {
        let mut state = self.state();
        let trial = std::mem::take(&mut state.trial);
        if let Some(returned) = state.returned.take() {
            // A return worked if a request began on it at least `base` after it. When the
            // fallback is recorded says nothing: a dead socket can take that long to fail.
            let served = state.began.map_or(Duration::ZERO, |began| {
                began.saturating_duration_since(returned)
            });
            if !trial && served >= self.base {
                state.failed_returns = 0;
            } else {
                state.failed_returns = state.failed_returns.saturating_add(1);
            }
        }
        state.since = Some(now);
        if !state.after_failure {
            state.kept = true;
            Return::ServerRefused
        } else if state.failed_returns >= MAX_FAILED_RETURNS {
            state.kept = true;
            Return::Exhausted
        } else {
            Return::After(self.interval(state.failed_returns))
        }
    }

    /// A request is about to choose its transport. Returns how long the session spent on
    /// HTTPS when this request is the one that ends the fallback.
    pub fn begin_request(&self, now: Instant) -> Option<Duration> {
        let mut state = self.state();
        if std::mem::take(&mut state.after_failure) {
            return None;
        }
        state.began = Some(now);
        state.trial = false;
        if state.kept {
            return None;
        }
        let since = state.since?;
        let well_since = state.unwell.map_or(since, |unwell| unwell.max(since));
        if now.saturating_duration_since(well_since) < self.interval(state.failed_returns) {
            return None;
        }
        state.since = None;
        state.returned = Some(now);
        state.trial = true;
        Some(now.saturating_duration_since(since))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: Duration = Duration::from_secs(600);

    fn fell_back_after_failure(fallback: &HttpsFallback, now: Instant) -> Return {
        fallback.request_failed(now, /*unwell*/ true);
        let scheduled = fallback.fell_back(now);
        // The failed request's retry starts next and stays on HTTPS.
        assert_eq!(fallback.begin_request(now), None);
        scheduled
    }

    #[test]
    fn a_session_that_never_fell_back_has_nothing_to_end() {
        let fallback = HttpsFallback::with_base(BASE);
        let start = Instant::now();
        assert_eq!(fallback.begin_request(start), None);
        fallback.request_failed(start, true);
        assert_eq!(fallback.begin_request(start + BASE * 100), None);
        assert!(!fallback.on_trial());
    }

    #[test]
    fn a_fallback_after_failed_requests_ends_at_a_request_once_the_interval_passed() {
        let fallback = HttpsFallback::with_base(BASE);
        let start = Instant::now();
        assert_eq!(
            fell_back_after_failure(&fallback, start),
            Return::After(BASE)
        );
        assert_eq!(
            fallback.begin_request(start + BASE - Duration::from_millis(1)),
            None
        );
        assert_eq!(fallback.begin_request(start + BASE), Some(BASE));
        // It ended: later requests have nothing to end.
        assert_eq!(fallback.begin_request(start + BASE * 2), None);
    }

    #[test]
    fn a_retry_stays_on_https_and_the_next_request_returns() {
        let fallback = HttpsFallback::with_base(BASE);
        let start = Instant::now();
        fell_back_after_failure(&fallback, start);
        let due = start + BASE;
        // A capacity answer: the request is retried, and the return is not postponed.
        fallback.request_failed(due, /*unwell*/ false);
        assert_eq!(fallback.begin_request(due), None, "the HTTPS retry");
        assert_eq!(fallback.begin_request(due), Some(BASE));
    }

    #[test]
    fn failures_on_https_postpone_the_return() {
        let fallback = HttpsFallback::with_base(BASE);
        let start = Instant::now();
        fell_back_after_failure(&fallback, start);
        // The outage outlasts the fallback: HTTPS requests fail as well.
        let failing = start + BASE - Duration::from_secs(1);
        fallback.request_failed(failing, true);
        assert_eq!(fallback.begin_request(failing), None, "the retry");
        assert_eq!(fallback.begin_request(start + BASE), None);
        assert_eq!(
            fallback.begin_request(failing + BASE - Duration::from_millis(1)),
            None
        );
        assert_eq!(
            fallback.begin_request(failing + BASE),
            Some(BASE * 2 - Duration::from_secs(1))
        );
    }

    #[test]
    fn a_fallback_no_failed_request_preceded_is_the_servers_and_stays() {
        let fallback = HttpsFallback::with_base(BASE);
        let start = Instant::now();
        // A request began, then its handshake was answered 426: no handler ran between.
        assert_eq!(fallback.begin_request(start), None);
        assert_eq!(fallback.fell_back(start), Return::ServerRefused);
        assert_eq!(fallback.begin_request(start + BASE * 1000), None);
        // Later failures on HTTPS do not reopen it.
        fallback.request_failed(start + BASE * 1500, true);
        assert_eq!(fallback.begin_request(start + BASE * 2000), None);
        assert_eq!(fallback.begin_request(start + BASE * 3000), None);
    }

    #[test]
    fn the_returning_request_is_a_trial_until_another_request_begins() {
        let fallback = HttpsFallback::with_base(BASE);
        let start = Instant::now();
        fell_back_after_failure(&fallback, start);
        assert!(!fallback.on_trial());
        let returned = start + BASE;
        assert!(fallback.begin_request(returned).is_some());
        assert!(fallback.on_trial());
        // Its own retry (after a capacity wait, say) is still the trial.
        fallback.request_failed(returned, false);
        assert_eq!(fallback.begin_request(returned), None);
        assert!(fallback.on_trial());
        // It was answered: the next request is an ordinary one.
        assert_eq!(
            fallback.begin_request(returned + Duration::from_secs(5)),
            None
        );
        assert!(!fallback.on_trial());
    }

    #[test]
    fn a_trial_that_fails_is_a_failed_return_however_long_it_took() {
        let fallback = HttpsFallback::with_base(BASE);
        let start = Instant::now();
        fell_back_after_failure(&fallback, start);
        let returned = start + BASE;
        assert!(fallback.begin_request(returned).is_some());
        // The socket accepted the request and never answered: the idle timeout is long.
        assert_eq!(
            fell_back_after_failure(&fallback, returned + BASE * 3),
            Return::After(BASE * 2)
        );
        assert!(!fallback.on_trial());
    }

    #[test]
    fn a_return_is_judged_by_the_requests_it_served_not_by_when_it_was_given_up() {
        let fallback = HttpsFallback::with_base(BASE);
        let start = Instant::now();
        fell_back_after_failure(&fallback, start);
        let returned = start + BASE;
        assert!(fallback.begin_request(returned).is_some());
        // The trial is answered, and the next request spends an hour failing.
        let second = returned + Duration::from_secs(30);
        assert_eq!(fallback.begin_request(second), None);
        assert_eq!(
            fell_back_after_failure(&fallback, second + BASE * 6),
            Return::After(BASE * 2)
        );
    }

    #[test]
    fn failed_returns_double_the_interval_then_keep_https() {
        let fallback = HttpsFallback::with_base(BASE);
        let mut now = Instant::now();
        assert_eq!(fell_back_after_failure(&fallback, now), Return::After(BASE));
        for failed in 1..MAX_FAILED_RETURNS {
            let interval = BASE * 2_u32.pow(failed - 1);
            now += interval;
            assert_eq!(fallback.begin_request(now), Some(interval));
            // The trial is answered; a request a minute later fails.
            now += Duration::from_secs(60);
            assert_eq!(fallback.begin_request(now), None);
            assert_eq!(
                fell_back_after_failure(&fallback, now),
                Return::After(BASE * 2_u32.pow(failed)),
                "failed return {failed}"
            );
        }
        now += BASE * 2_u32.pow(MAX_FAILED_RETURNS - 1);
        assert!(fallback.begin_request(now).is_some());
        assert_eq!(fell_back_after_failure(&fallback, now), Return::Exhausted);
        assert_eq!(fallback.begin_request(now + BASE * 1000), None);
    }

    #[test]
    fn a_return_that_served_requests_starts_the_count_again() {
        let fallback = HttpsFallback::with_base(BASE);
        let mut now = Instant::now();
        fell_back_after_failure(&fallback, now);
        now += BASE;
        assert!(fallback.begin_request(now).is_some());
        assert_eq!(
            fell_back_after_failure(&fallback, now + Duration::from_secs(1)),
            Return::After(BASE * 2)
        );
        now += Duration::from_secs(1) + BASE * 2;
        assert!(fallback.begin_request(now).is_some());
        // This time the WebSocket serves requests for the base interval before an outage.
        now += BASE;
        assert_eq!(fallback.begin_request(now), None);
        assert_eq!(fell_back_after_failure(&fallback, now), Return::After(BASE));
    }

    #[test]
    fn a_huge_base_saturates() {
        let fallback = HttpsFallback::with_base(Duration::MAX);
        let start = Instant::now();
        assert_eq!(
            fell_back_after_failure(&fallback, start),
            Return::After(Duration::MAX)
        );
        assert_eq!(fallback.begin_request(start + BASE), None);
    }
}
