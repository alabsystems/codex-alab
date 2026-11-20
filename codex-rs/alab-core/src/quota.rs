// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Causal, account-bound evidence from accepted full usage reads. No credentials or polling.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::watch;

/// Prefixes shared by recovery and the TUI's transcript filter.
pub const EARNED_RESET_NOTICE: &str = "[codex-alab reset] ";
pub const USAGE_CREDIT_NOTICE: &str = "[codex-alab credits] ";

pub fn is_recovery_notice(message: &str) -> bool {
    message.starts_with(EARNED_RESET_NOTICE) || message.starts_with(USAGE_CREDIT_NOTICE)
}

/// Keep a finite provider-reported credit balance at its original precision for display.
pub fn reported_credit_amount(balance: Option<&str>) -> Option<&str> {
    balance
        .map(str::trim)
        .filter(|value| value.len() <= 64 && value.parse::<f64>().is_ok_and(f64::is_finite))
}

/// Volatile admission for account-quota recovery in a root turn executed by an
/// exec host. The host's extension registry supplies it; saved session origin
/// and serialized history cannot grant it.
#[derive(Clone, Copy, Debug)]
pub struct HeadlessExec;

/// This live exec host owns quota waiting for a tree member. It does not make
/// the member a root or authorize replacing any turn or selecting an account.
#[derive(Clone, Copy, Debug)]
pub struct ExecQuotaMember;

/// Which of the account's own usage windows a creditless wall has spent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpentWindow {
    /// The seven-day allowance.
    Week,
    /// The five-hour allowance, on a week that is not spent.
    FiveHour,
}

/// A usage wall no earned credit can lift, as the walled request's own fresh
/// usage read shows it. `resets_at` is the moment that window resets (seconds
/// since the Unix epoch), as the read names it: a host that leaves only walls
/// far enough from their reset is judged against its bound with the time left
/// when it is asked ([`SpentWall::resets_in`]), not when the usage was read.
/// The requests between the two (the credit list, the host's policy) take
/// time, and a relative figure would be that much too long by then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpentWall {
    pub window: SpentWindow,
    pub resets_at: i64,
}

impl SpentWall {
    /// The time left to the reset at `now` (seconds since the Unix epoch);
    /// zero once it has come.
    pub fn resets_in(&self, now: i64) -> Duration {
        Duration::from_secs(u64::try_from(self.resets_at.saturating_sub(now)).unwrap_or(0))
    }
}

type HandoffProbe = dyn Fn(SpentWall) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync;

type LookHeard = dyn Fn() + Send + Sync;

/// Installed in a root turn's store by a host that can move the conversation to
/// another account once that turn ends at a usage wall. Quota recovery tells it
/// which wall and asks only whether ending the walled turn now would be
/// followed by such a move. It grants no execution, spends no credit and
/// selects no account; the host re-checks everything after the turn ends.
///
/// A yes ends the walled request on its 429, so the turn ends at the wall the
/// probe was told: an owner that answers yes keeps that wall as the ended
/// turn's, with the reset the usage read named, whatever reset the 429 named
/// (none, or another moment). Nothing after the turn runs on the 429's.
pub struct WallHandoff {
    probe: Box<HandoffProbe>,
    /// The host changes this when its answer may have turned to yes
    /// ([`Self::waking`]); `None` for a host that says nothing between questions.
    wake: Option<watch::Receiver<u64>>,
    /// Told as each look of the caller's wait begins ([`Self::look_begins`]);
    /// `None` for a host that does not hear of looks.
    looks: Option<Box<LookHeard>>,
}

/// A wait for a host's word that its answer may have changed ([`WallHandoff::watch`]).
pub struct HandoffWatch(Option<watch::Receiver<u64>>);

impl HandoffWatch {
    /// Resolves once the host has signalled since this watch was made or last
    /// resolved. It carries no answer: the caller asks the host again.
    ///
    /// A handoff with no wake, and one whose host is gone, never resolve. A
    /// caller asks again at each wake, so a watch that resolved at once for such
    /// a host would have it ask without pause; its own interval is all that is
    /// left to it there.
    pub async fn changed(&mut self) {
        if let Some(wake) = self.0.as_mut()
            && wake.changed().await.is_ok()
        {
            return;
        }
        self.0 = None;
        std::future::pending::<()>().await;
    }
}

impl WallHandoff {
    /// A host that does not answer in time is not ready: the caller keeps its
    /// own wait rather than hold the original request on the host.
    const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

    pub fn new<F, Fut>(probe: F) -> Self
    where
        F: Fn(SpentWall) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        Self {
            probe: Box::new(move |wall| Box::pin(probe(wall))),
            wake: None,
            looks: None,
        }
    }

    /// The host changes `wake` whenever its answer may have turned to yes. A
    /// caller that waits on a host that was not ready ([`Self::watch`]) then
    /// asks again at once, where it would otherwise ask only at its own
    /// interval; the turn it holds meanwhile keeps the session busy.
    pub fn waking(mut self, wake: watch::Receiver<u64>) -> Self {
        self.wake = Some(wake);
        self
    }

    /// The host hears through `heard` as each look of a caller's wait begins
    /// ([`Self::look_begins`]). A host that wakes its callers ([`Self::waking`])
    /// need then signal only once a look began since its last signal: a caller
    /// whose last look began before that signal looks again after it anyway,
    /// and one whose look began since may have read the host before the change.
    /// Each look is a usage read and a credit list read for every caller that
    /// waits.
    pub fn hearing_looks(mut self, heard: impl Fn() + Send + Sync + 'static) -> Self {
        self.looks = Some(Box::new(heard));
        self
    }

    /// A look of the caller's wait begins. Said at every look, before it reads
    /// anything, whether or not it comes to ask the host ([`Self::ready`]): a
    /// look that ends before its question (a credit list unread, a read that
    /// failed, an attempt that ran out of time) waits on the host's next signal
    /// as much as one the host told no.
    pub fn look_begins(&self) {
        if let Some(heard) = self.looks.as_ref() {
            heard();
        }
    }

    /// A wait for the host's next signal, counted from this call: make it before
    /// the first question, and a signal sent while a question is in flight is
    /// kept for the wait that follows it.
    pub fn watch(&self) -> HandoffWatch {
        HandoffWatch(self.wake.clone().map(|mut wake| {
            wake.mark_unchanged();
            wake
        }))
    }

    /// Advisory and evaluated while the turn still runs. A stale answer costs
    /// either one more poll or an ordinary usage-wall wait, never a credit.
    pub async fn ready(&self, wall: SpentWall) -> bool {
        tokio::time::timeout(Self::PROBE_TIMEOUT, (self.probe)(wall))
            .await
            .unwrap_or(false)
    }
}

/// Persisted identity, bucket and denial history; never live permission.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    pub account: String,
    pub bucket: String,
    pub user: Option<String>,
    pub ordinary_retry: bool,
    pub ordinary_denied: bool,
}

impl Scope {
    pub fn valid(&self) -> bool {
        [&self.account, &self.bucket]
            .into_iter()
            .chain(self.user.iter())
            .all(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
    }
}

#[derive(Clone, Debug)]
pub struct Wall {
    account: String,
    generation: u64,
    at: Instant,
    bucket: String,
    ordinary_retry: bool,
    ordinary_denied: bool,
}

impl Wall {
    pub fn with_ordinary_retry(mut self, eligible: bool, denied: bool) -> Self {
        self.ordinary_retry = eligible;
        self.ordinary_denied |= denied;
        self
    }
}

struct Reading {
    account: String,
    generation: u64,
    started: ReadTime,
    completed: Instant,
    usable: Vec<String>,
    ordinary: Option<bool>,
    denied: Vec<String>,
}

/// The awake clock orders observations against their wall. The continuous
/// clock also counts host sleep toward an observation's freshness.
#[derive(Clone, Copy)]
struct ReadTime {
    at: Instant,
    continuous_ns: Option<u64>,
}

impl ReadTime {
    fn sample(at: Instant) -> Self {
        Self {
            at,
            continuous_ns: crate::wake::continuous_ns(),
        }
    }

    fn fresh_at(self, now: Self) -> bool {
        const FRESH: Duration = Duration::from_secs(30);
        let awake = now.at.checked_duration_since(self.at);
        awake.is_some_and(|age| age <= FRESH)
            && match (self.continuous_ns, now.continuous_ns) {
                (Some(start), Some(end)) => end
                    .checked_sub(start)
                    .is_some_and(|ns| Duration::from_nanos(ns) <= FRESH),
                // On a host that counts suspend, an unread clock cannot establish freshness.
                _ => !crate::wake::supported(),
            }
    }
}

#[derive(Default)]
struct State {
    generation: u64,
    read_epoch: u64,
    owner: Option<(String, Option<String>, u64)>,
    requests: BTreeMap<u64, (u64, ReadTime)>,
    reading: Option<Reading>,
    ordinary: Option<bool>,
    denied_at: Option<Instant>,
    recovered_from: Option<Instant>,
}

#[derive(Default)]
pub struct Quota {
    state: Mutex<State>,
    wake: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

impl Quota {
    /// Native owner changes revoke evidence; a TUI read invalidation does not change ownership.
    pub fn select_owner(&self, account: &str, user: Option<&str>, revision: u64) -> u64 {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let owner = (account.to_string(), user.map(str::to_string), revision);
        if state.owner.as_ref() != Some(&owner) {
            state.owner = Some(owner);
            state.generation = state.generation.wrapping_add(1);
            state.read_epoch = state.read_epoch.wrapping_add(1);
            state.requests.clear();
            state.reading = None;
            state.ordinary = None;
            state.denied_at = None;
            state.recovered_from = None;
        }
        state.generation
    }

    pub fn generation(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .generation
    }

    pub fn wall_in(
        &self,
        account: String,
        generation: u64,
        bucket: String,
        now: Instant,
    ) -> Option<Wall> {
        self.wall_in_at(account, generation, bucket, ReadTime::sample(now))
    }

    fn wall_in_at(
        &self,
        account: String,
        generation: u64,
        bucket: String,
        now: ReadTime,
    ) -> Option<Wall> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let ordinary_denied = state.reading.as_ref().is_some_and(|read| {
            read.account == account
                && read.generation == generation
                && read.ordinary == Some(false)
                && read.completed <= now.at
                && read.started.fresh_at(now)
        });
        (generation == state.generation).then_some(Wall {
            account,
            generation,
            at: now.at,
            bucket,
            ordinary_retry: false,
            ordinary_denied,
        })
    }
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                generation: 0,
                read_epoch: 0,
                owner: None,
                requests: BTreeMap::new(),
                reading: None,
                ordinary: None,
                denied_at: None,
                recovered_from: None,
            }),
            wake: OnceLock::new(),
        }
    }
    pub fn set_waker(&self, wake: impl Fn() + Send + Sync + 'static) {
        let _ = self.wake.set(Box::new(wake));
    }

    pub fn invalidate(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.read_epoch = state.read_epoch.wrapping_add(1);
        state.requests.clear();
        state.reading = None;
    }

    pub fn start_read(&self, id: u64, now: Instant) {
        self.start_read_at(id, ReadTime::sample(now));
    }

    fn start_read_at(&self, id: u64, now: ReadTime) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation = state.read_epoch;
        state.requests.insert(id, (generation, now));
        while state.requests.len() > 64 {
            state.requests.pop_first();
        }
    }

    /// Called only after the native response passed account and request ordering checks.
    pub fn accept(&self, id: u64, account: &str, usable: Vec<String>, now: Instant) {
        self.accept_observation(id, account, usable, None, Vec::new(), now);
    }

    pub fn accept_observation(
        &self,
        id: u64,
        account: &str,
        usable: Vec<String>,
        ordinary: Option<bool>,
        denied: Vec<String>,
        now: Instant,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some((generation, started)) = state.requests.remove(&id) else {
            return;
        };
        if account.is_empty()
            || generation != state.read_epoch
            || started.at > now
            || state
                .owner
                .as_ref()
                .is_some_and(|(owner, _, _)| owner != account)
        {
            return;
        }
        if state
            .reading
            .as_ref()
            .is_some_and(|old| old.account != account)
        {
            state.generation = state.generation.wrapping_add(1);
            state.ordinary = None;
            state.denied_at = None;
            state.recovered_from = None;
        }
        if let Some(allowed) = ordinary {
            if allowed && state.ordinary == Some(false) {
                state.recovered_from = state.denied_at;
            } else if !allowed {
                state.denied_at = Some(started.at);
                state.recovered_from = None;
            }
            state.ordinary = Some(allowed);
        }
        state.reading = Some(Reading {
            account: account.into(),
            generation: state.generation,
            started,
            completed: now,
            usable,
            ordinary,
            denied,
        });
        drop(state);
        if let Some(wake) = self.wake.get() {
            wake();
        }
    }

    pub fn wall_for(&self, account: String, bucket: String, now: Instant) -> Wall {
        Wall {
            account,
            generation: self.generation(),
            at: now,
            bucket,
            ordinary_retry: false,
            ordinary_denied: false,
        }
    }

    pub fn scope(&self, wall: &Wall, user: Option<String>) -> Scope {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = state.generation == wall.generation;
        Scope {
            account: wall.account.clone(),
            bucket: wall.bucket.clone(),
            user,
            ordinary_retry: wall.ordinary_retry,
            ordinary_denied: wall.ordinary_denied
                || (current
                    && (state.denied_at.is_some_and(|at| at > wall.at)
                        || state.recovered_from.is_some_and(|at| at > wall.at))),
        }
    }

    pub fn restored(&self, wall: &Wall, now: Instant) -> bool {
        self.restored_with(wall, ReadTime::sample(now), false)
    }

    /// Subscription recovery requires a denied-to-allowed transition, not merely
    /// a usable ordinary bucket beside a still-unavailable premium entitlement.
    pub fn ordinary_restored(&self, wall: &Wall, now: Instant) -> bool {
        self.restored_with(wall, ReadTime::sample(now), true)
    }

    fn restored_with(&self, wall: &Wall, now: ReadTime, ordinary_only: bool) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.reading.as_ref().is_some_and(|read| {
            read.account == wall.account
                && read.generation == wall.generation
                && read.started.at > wall.at
                && read.completed <= now.at
                && read.started.fresh_at(now)
                && !read.denied.contains(&wall.bucket)
                && ((!ordinary_only && read.usable.contains(&wall.bucket))
                    || ((ordinary_only || wall.ordinary_retry)
                        && read.ordinary == Some(true)
                        && (wall.ordinary_denied
                            || state.recovered_from.is_some_and(|at| at > wall.at))))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pre_sleep_denial_cannot_grant_an_ordinary_retry_after_relaunch() {
        let base = Instant::now();
        let time = |awake, elapsed: u64| ReadTime {
            at: base + Duration::from_secs(awake),
            continuous_ns: Some(elapsed * 1_000_000_000),
        };
        let quota = Quota::new();
        let generation = quota.select_owner("a", None, 0);
        quota.start_read_at(1, time(0, 0));
        quota.accept_observation(1, "a", vec![], Some(false), vec![], time(1, 1).at);
        let wall = quota
            .wall_in_at("a".into(), generation, "premium".into(), time(2, 3602))
            .unwrap()
            .with_ordinary_retry(true, false);
        let saved = quota.scope(&wall, None);
        assert!(!saved.ordinary_denied);

        let resumed = Quota::new();
        let generation = resumed.select_owner("a", None, 1);
        let wall = resumed
            .wall_in_at(saved.account, generation, saved.bucket, time(3, 3603))
            .unwrap()
            .with_ordinary_retry(saved.ordinary_retry, saved.ordinary_denied);
        resumed.start_read_at(2, time(4, 3604));
        resumed.accept_observation(2, "a", vec![], Some(true), vec![], time(5, 3605).at);
        assert!(!resumed.restored_with(&wall, time(5, 3605), true));

        resumed.start_read_at(3, time(6, 3606));
        resumed.accept_observation(3, "a", vec![], Some(false), vec![], time(7, 3607).at);
        resumed.start_read_at(4, time(8, 3608));
        resumed.accept_observation(4, "a", vec![], Some(true), vec![], time(9, 3609).at);
        assert!(resumed.restored_with(&wall, time(9, 3609), true));
        assert!(!resumed.restored_with(&wall, time(10, 7210), true));
    }

    #[tokio::test(start_paused = true)]
    async fn wall_handoff_probe_is_bounded() {
        let week = SpentWall {
            window: SpentWindow::Week,
            resets_at: 1_800_000_000,
        };
        let started = tokio::time::Instant::now();
        let silent = WallHandoff::new(|_| std::future::pending::<bool>());
        assert!(
            !silent.ready(week).await,
            "a host that never answers is not ready"
        );
        assert_eq!(started.elapsed(), WallHandoff::PROBE_TIMEOUT);
        let slow = WallHandoff::new(|_| async {
            tokio::time::sleep(WallHandoff::PROBE_TIMEOUT - Duration::from_millis(1)).await;
            true
        });
        assert!(
            slow.ready(week).await,
            "an answer inside the bound is taken"
        );
        assert!(!WallHandoff::new(|_| async { false }).ready(week).await);
    }

    /// A WATCH RESOLVES FOR A SIGNAL SENT AFTER IT WAS MADE, ONCE, AND NEVER WITHOUT ONE. A
    /// caller asks its host again at each resolve, so a watch that resolved with nothing
    /// sent (no wake installed, the host gone, a signal older than the watch) would have it
    /// ask without pause.
    #[tokio::test(start_paused = true)]
    async fn a_handoff_watch_resolves_once_per_signal_and_never_without_one() {
        const LONG: Duration = Duration::from_secs(3600);
        async fn resolves(watch: &mut HandoffWatch) -> bool {
            tokio::time::timeout(LONG, watch.changed()).await.is_ok()
        }
        // No wake installed.
        let mut silent = WallHandoff::new(|_| async { false }).watch();
        assert!(!resolves(&mut silent).await, "a host with no wake woke");

        let (wake, signals) = watch::channel(0_u64);
        let handoff = WallHandoff::new(|_| async { false }).waking(signals);
        // A signal older than the watch is not waited for.
        wake.send_modify(|n| *n += 1);
        let mut watch = handoff.watch();
        assert!(!resolves(&mut watch).await, "an old signal woke");
        // One sent since resolves it once, however long ago it was sent.
        wake.send_modify(|n| *n += 1);
        tokio::time::sleep(LONG).await;
        assert!(resolves(&mut watch).await, "a signal did not wake");
        assert!(!resolves(&mut watch).await, "one signal woke twice");
        // A signal sent while nobody waits is kept for the next wait.
        wake.send_modify(|n| *n += 1);
        wake.send_modify(|n| *n += 1);
        assert!(
            resolves(&mut watch).await,
            "a signal between waits was lost"
        );
        assert!(!resolves(&mut watch).await);
        // The host is gone: nothing more will come, and the watch never resolves.
        drop(wake);
        assert!(!resolves(&mut watch).await, "a host that is gone woke");
        assert!(
            !resolves(&mut watch).await,
            "a host that is gone woke later"
        );
    }

    /// The host is told the wall the request stands at, unchanged: its answer
    /// may depend on the window and on when the reset is.
    #[tokio::test]
    async fn wall_handoff_probe_is_told_the_wall() {
        let five_hour = SpentWall {
            window: SpentWindow::FiveHour,
            resets_at: 1_800_007_200,
        };
        let only_that = WallHandoff::new(move |wall| async move { wall == five_hour });
        assert!(only_that.ready(five_hour).await);
        assert!(
            !only_that
                .ready(SpentWall {
                    window: SpentWindow::Week,
                    ..five_hour
                })
                .await
        );
        assert!(
            !only_that
                .ready(SpentWall {
                    resets_at: 1_800_007_199,
                    ..five_hour
                })
                .await
        );
    }

    /// The time left to a wall's reset is measured when it is asked for, so a
    /// wall read a while ago is that much nearer its reset.
    #[test]
    fn a_spent_walls_time_to_reset_is_measured_when_asked() {
        let wall = SpentWall {
            window: SpentWindow::FiveHour,
            resets_at: 1_800_000_965,
        };
        assert_eq!(wall.resets_in(1_800_000_000), Duration::from_secs(965));
        assert_eq!(wall.resets_in(1_800_000_040), Duration::from_secs(925));
        assert_eq!(wall.resets_in(1_800_000_964), Duration::from_secs(1));
        // At and past the reset nothing is left, whatever the clocks say.
        for now in [1_800_000_965, 1_800_000_966, i64::MAX] {
            assert_eq!(wall.resets_in(now), Duration::ZERO, "{now}");
        }
        let never = SpentWall {
            resets_at: i64::MAX,
            ..wall
        };
        assert_eq!(
            never.resets_in(i64::MIN),
            Duration::from_secs(i64::MAX.unsigned_abs())
        );
    }

    #[test]
    fn subscription_recovery_requires_a_fresh_same_account_transition() {
        let q = Quota::new();
        let base = Instant::now();
        q.select_owner("a", Some("user"), 0);
        let wall = q.wall_for("a".into(), "codex".into(), base);
        q.start_read(1, base + Duration::from_secs(1));
        q.accept_observation(
            1,
            "a",
            vec!["codex".into()],
            Some(true),
            vec![],
            base + Duration::from_secs(2),
        );
        assert!(!q.ordinary_restored(&wall, base + Duration::from_secs(2)));
        q.start_read(2, base + Duration::from_secs(3));
        q.accept_observation(
            2,
            "a",
            vec![],
            Some(false),
            vec![],
            base + Duration::from_secs(4),
        );
        q.start_read(3, base + Duration::from_secs(5));
        q.accept_observation(
            3,
            "a",
            vec!["codex".into()],
            Some(true),
            vec![],
            base + Duration::from_secs(6),
        );
        assert!(q.ordinary_restored(&wall, base + Duration::from_secs(6)));
        assert!(!q.ordinary_restored(&wall, base + Duration::from_secs(40)));
        q.select_owner("other", Some("user"), 1);
        assert!(!q.ordinary_restored(&wall, base + Duration::from_secs(6)));
    }

    #[test]
    fn ordinary_recovery_is_a_transition_not_an_alias_for_missing_premium() {
        let quota = Quota::new();
        let now = Instant::now();
        let at = |seconds| now + Duration::from_secs(seconds);
        quota.start_read(1, at(0));
        quota.accept_observation(1, "a", vec!["codex".into()], Some(true), vec![], at(0));
        let wall = quota
            .wall_for("a".into(), "premium".into(), at(1))
            .with_ordinary_retry(true, false);
        quota.start_read(2, at(2));
        quota.accept_observation(2, "a", vec!["codex".into()], Some(true), vec![], at(2));
        assert!(!quota.restored(&wall, at(2)));
        quota.start_read(3, at(3));
        quota.accept_observation(3, "a", vec![], Some(false), vec![], at(3));
        quota.start_read(4, at(4));
        quota.accept_observation(4, "a", vec!["codex".into()], Some(true), vec![], at(4));
        assert!(quota.restored(&wall, at(4)));
        let scope = quota.scope(&wall, Some("u".into()));
        assert!(scope.ordinary_denied);
        let next = quota
            .wall_for("a".into(), "premium".into(), at(5))
            .with_ordinary_retry(true, false);
        quota.start_read(5, at(6));
        quota.accept_observation(5, "a", vec!["codex".into()], Some(true), vec![], at(6));
        assert!(!quota.restored(&next, at(6)));
        quota.start_read(6, at(7));
        quota.accept_observation(
            6,
            "a",
            vec!["codex".into()],
            Some(true),
            vec!["premium".into()],
            at(7),
        );
        assert!(!quota.restored(&wall, at(7)));
    }

    #[test]
    fn an_account_change_during_a_failed_request_cannot_bind_its_wall() {
        let quota = Quota::new();
        let admitted = quota.generation();
        quota.select_owner("other", Some("other-user"), 1);
        assert!(
            quota
                .wall_in(
                    "old-account".into(),
                    admitted,
                    "codex".into(),
                    Instant::now()
                )
                .is_none()
        );
    }

    #[test]
    fn native_account_binding_does_not_require_an_earlier_usage_read() {
        let quota = Quota::new();
        let now = Instant::now();
        let wall = quota.wall_for("native-account".into(), "codex".into(), now);
        assert!(!quota.restored(&wall, now));
        quota.start_read(1, now + Duration::from_secs(1));
        quota.accept(
            1,
            "another-account",
            vec!["codex".into()],
            now + Duration::from_secs(2),
        );
        assert!(!quota.restored(&wall, now + Duration::from_secs(2)));
        quota.invalidate();
        let wall = quota.wall_for("native-account".into(), "codex".into(), now);
        quota.start_read(2, now + Duration::from_secs(3));
        quota.accept(
            2,
            "native-account",
            vec!["codex".into()],
            now + Duration::from_secs(4),
        );
        assert!(quota.restored(&wall, now + Duration::from_secs(4)));
    }

    #[test]
    fn restoration_requires_same_account_generation_and_a_post_wall_request() {
        let quota = Quota::default();
        let now = Instant::now();
        quota.start_read(1, now);
        quota.accept(1, "a", vec![], now);
        quota.start_read(2, now);
        let wall = quota.wall_for("a".into(), "codex".into(), now);
        quota.accept(2, "a", vec!["codex".into()], now + Duration::from_secs(1));
        assert!(!quota.restored(&wall, now + Duration::from_secs(1)));
        quota.start_read(3, now + Duration::from_secs(2));
        quota.accept(3, "a", vec!["other".into()], now + Duration::from_secs(3));
        assert!(!quota.restored(&wall, now + Duration::from_secs(3)));
        quota.start_read(4, now + Duration::from_secs(4));
        quota.accept(4, "a", vec!["codex".into()], now + Duration::from_secs(5));
        assert!(quota.restored(&wall, now + Duration::from_secs(5)));
        assert!(!quota.restored(&wall, now + Duration::from_secs(36)));
        quota.select_owner("a", Some("new-user"), 1);
        quota.start_read(5, now + Duration::from_secs(6));
        quota.accept(5, "a", vec!["codex".into()], now + Duration::from_secs(7));
        assert!(!quota.restored(&wall, now + Duration::from_secs(7)));
    }

    #[test]
    fn native_owner_changes_clear_denials_but_delayed_read_invalidation_does_not_strand_a_wall() {
        let quota = Quota::new();
        let now = Instant::now();
        let at = |seconds| now + Duration::from_secs(seconds);
        let read = |id, seconds, allowed| {
            quota.start_read(id, at(seconds));
            quota.accept_observation(
                id,
                "workspace",
                vec!["codex".into()],
                Some(allowed),
                vec![],
                at(seconds),
            );
        };
        quota.select_owner("workspace", Some("old-user"), 1);
        read(1, 0, false);
        quota.start_read(2, at(1));
        let owner = quota.select_owner("workspace", Some("new-user"), 2);
        let wall = quota
            .wall_in("workspace".into(), owner, "premium".into(), at(2))
            .unwrap()
            .with_ordinary_retry(true, false);
        quota.accept_observation(2, "workspace", vec![], Some(false), vec![], at(3));
        read(3, 4, true);
        assert!(
            !quota.restored(&wall, at(4)),
            "old owner's denial cannot establish a transition"
        );
        read(4, 5, false);
        read(5, 6, true);
        assert!(quota.restored(&wall, at(6)));
        quota.start_read(6, at(7));
        quota.invalidate();
        assert!(
            quota.scope(&wall, Some("new-user".into())).ordinary_denied,
            "read invalidation must not erase a same-owner denial before a restart"
        );
        quota.accept_observation(6, "workspace", vec![], Some(false), vec![], at(8));
        read(7, 9, true);
        assert!(
            quota.restored(&wall, at(9)),
            "same native owner survives a delayed AccountUpdated"
        );
    }
}
