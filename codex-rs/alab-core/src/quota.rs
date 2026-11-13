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

/// Installed in a root turn's store by a host that can move the conversation to
/// another account once that turn ends at a usage wall. Quota recovery tells it
/// which wall and asks only whether ending the walled turn now would be
/// followed by such a move. It grants no execution, spends no credit and
/// selects no account; the host re-checks everything after the turn ends.
pub struct WallHandoff {
    probe: Box<HandoffProbe>,
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
        }
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
    started: Instant,
    completed: Instant,
    usable: Vec<String>,
    ordinary: Option<bool>,
    denied: Vec<String>,
}

#[derive(Default)]
struct State {
    generation: u64,
    read_epoch: u64,
    owner: Option<(String, Option<String>, u64)>,
    requests: BTreeMap<u64, (u64, Instant)>,
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
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let ordinary_denied = state.reading.as_ref().is_some_and(|read| {
            read.account == account
                && read.generation == generation
                && read.ordinary == Some(false)
                && read.completed <= now
                && now.duration_since(read.started) <= Duration::from_secs(30)
        });
        (generation == state.generation).then_some(Wall {
            account,
            generation,
            at: now,
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
            || started > now
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
                state.denied_at = Some(started);
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
        self.restored_with(wall, now, false)
    }

    /// Subscription recovery requires a denied-to-allowed transition, not merely
    /// a usable ordinary bucket beside a still-unavailable premium entitlement.
    pub fn ordinary_restored(&self, wall: &Wall, now: Instant) -> bool {
        self.restored_with(wall, now, true)
    }

    fn restored_with(&self, wall: &Wall, now: Instant, ordinary_only: bool) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.reading.as_ref().is_some_and(|read| {
            read.account == wall.account
                && read.generation == wall.generation
                && read.started > wall.at
                && read.completed <= now
                && now.duration_since(read.started) <= Duration::from_secs(30)
                && now.duration_since(read.completed) <= Duration::from_secs(30)
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
