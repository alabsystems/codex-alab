// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! HOW LONG A `codex exec` RUN KEEPS ITS REQUEST AT A WALL ONLY A MOVE GETS IT PAST (gap 4 of
//! `alab/PARALLEL-LOGINS.md`, step S3).
//!
//! An exec run keeps its request across an ordinary wall until the wall's reset, as before. A
//! workspace's wall ([`WallKind::host_kept`]) has no reset this account reaches: only a refill or
//! a person ends it. So the run keeps its request there only while its host says a move may
//! come, and otherwise ends at the wall as it always did:
//!
//! - with no exec handoff runtime subscribed to the run's quota observations, it ends at once;
//! - its first wait is [`UNKNOWN_RESET_WAIT`], whatever reset the refusal names, so the runtime
//!   can ask its host;
//! - each later wait needs the host's [`Verdict::Keep`] for the root turn and that kind of wall
//!   (a `codex_rotation_refusal=1` policy, answered `waiting` or `selected`); an unanswered wall
//!   ends at its second refusal, and a declined one at its next;
//! - the whole keep is bounded ([`HOST_KEPT_BOUND`] from the root turn's first such wait), so a
//!   host that answers `waiting` for ever (no destination, a hold) ends the run, not a person;
//! - while the runtime holds a grant for the root turn ([`ExecWalls::hold`]), the bound is not
//!   applied, and a kept wait does not end on its timer ([`ExecWalls::kept_wait`]). A retry would
//!   end the wait's observation, and the runtime cancels a grant whose observation ended. The wait
//!   ends when the grant is let go (cancelled, or at its deadline) or when the suspension the
//!   grant leads to cancels the turn.
//!
//! The state is the exec tree's ([`ExecWalls`] is shared by every member's turn): the verdict
//! and the first wait are kept per root turn and wall kind, never per observation, since each
//! retry publishes a new observation of the same wall. Only the waiting turn records its first
//! wait ([`ExecWalls::waited`]) and only the runtime records a verdict ([`ExecWalls::decide`]);
//! the judgement before a retry only reads ([`ExecWalls::keep`]).

use crate::wall_kind::WallKind;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::watch;

/// How long an exec run waits at a usage wall whose refusal names no reset, and the length of
/// every wait at a host-kept wall.
pub const UNKNOWN_RESET_WAIT: Duration = Duration::from_secs(30);

/// The longest a root turn keeps its request at host-kept walls, from its first such wait,
/// unless a grant for it is held.
pub const HOST_KEPT_BOUND: Duration = Duration::from_secs(10 * 60);

/// What the run's host said about a host-kept wall of its root turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Verdict {
    /// No answer yet.
    #[default]
    Unknown,
    /// A move may come: the host's policy leaves such a wall and it answered `waiting` or
    /// `selected`.
    Keep,
    /// No move will come: no host reached, a policy that does not leave such a wall, the host's
    /// `not-needed`, or a definite refusal.
    Declined,
}

/// The exec tree's host-kept walls, shared by every member's turn.
pub struct ExecWalls {
    state: Mutex<State>,
    /// The root turns whose grants the runtime holds, one entry per hold.
    grants: watch::Sender<Vec<String>>,
}

impl Default for ExecWalls {
    fn default() -> Self {
        Self {
            state: Mutex::default(),
            grants: watch::channel(Vec::new()).0,
        }
    }
}

impl std::fmt::Debug for ExecWalls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecWalls").finish_non_exhaustive()
    }
}

#[derive(Default)]
struct State {
    /// The tree's current root turn.
    root: Option<String>,
    /// The root turn's first wait at a host-kept wall.
    since: Option<Instant>,
    walls: Vec<(WallKind, Entry)>,
}

#[derive(Clone, Copy, Debug, Default)]
struct Entry {
    verdict: Verdict,
    /// The first wait was taken.
    waited: bool,
}

impl State {
    fn entry(&self, kind: &WallKind) -> Entry {
        self.walls
            .iter()
            .find(|(known, _)| known == kind)
            .map(|(_, entry)| *entry)
            .unwrap_or_default()
    }

    fn entry_mut(&mut self, kind: &WallKind) -> &mut Entry {
        let index = match self.walls.iter().position(|(known, _)| known == kind) {
            Some(index) => index,
            None => {
                self.walls.push((kind.clone(), Entry::default()));
                self.walls.len() - 1
            }
        };
        &mut self.walls[index].1
    }
}

impl ExecWalls {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The tree's root turn is `turn`. Another root turn starts over: its walls have no verdict,
    /// no first wait and no bound yet.
    pub fn root_turn(&self, turn: &str) {
        let mut state = self.lock();
        if state.root.as_deref() != Some(turn) {
            *state = State {
                root: Some(turn.to_owned()),
                ..State::default()
            };
        }
    }

    /// How long a refusal at host-kept wall `kind` waits before its retry at `now`, or `None` when
    /// it ends the run. `subscribed`: an exec handoff runtime reads this tree's observations.
    /// Reads only.
    pub fn keep(&self, kind: &WallKind, subscribed: bool, now: Instant) -> Option<Duration> {
        let state = self.lock();
        let granted = held(&self.grants.borrow(), state.root.as_deref());
        keep(subscribed, state.entry(kind), state.since, granted, now)
    }

    /// A wait at host-kept wall `kind` begins at `now`: its first wait is taken, and the root
    /// turn's bound starts at its first.
    pub fn waited(&self, kind: &WallKind, now: Instant) {
        let mut state = self.lock();
        state.since.get_or_insert(now);
        state.entry_mut(kind).waited = true;
    }

    /// The runtime's verdict for wall `kind` of root turn `root`. A verdict for another root turn
    /// than the tree's is too late or too early, and is dropped.
    pub fn decide(&self, root: &str, kind: &WallKind, verdict: Verdict) {
        let mut state = self.lock();
        if state.root.as_deref() == Some(root) {
            state.entry_mut(kind).verdict = verdict;
        }
    }

    /// The verdict recorded for wall `kind` of the tree's root turn.
    pub fn verdict(&self, kind: &WallKind) -> Verdict {
        self.lock().entry(kind).verdict
    }

    /// The runtime holds a grant for root turn `root` (armed, or reserving) until the hold drops.
    pub fn hold(self: &Arc<Self>, root: &str) -> GrantHold {
        self.grants.send_modify(|held| held.push(root.to_owned()));
        GrantHold {
            walls: Arc::clone(self),
            root: root.to_owned(),
        }
    }

    /// Whether a grant for the tree's root turn is held.
    pub fn granted(&self) -> bool {
        let root = self.lock().root.clone();
        held(&self.grants.borrow(), root.as_deref())
    }

    /// Wait out a kept wait's `timer`, then for as long as a grant for the tree's root turn is
    /// held.
    pub async fn kept_wait<F: Future>(&self, timer: F) -> F::Output {
        let output = timer.await;
        let root = self.lock().root.clone();
        let mut grants = self.grants.subscribe();
        // The sender lives as long as `self`, so the wait ends only on the condition.
        let _ = grants
            .wait_for(|held_now| !held(held_now, root.as_deref()))
            .await;
        output
    }
}

fn held(grants: &[String], root: Option<&str>) -> bool {
    root.is_some_and(|root| grants.iter().any(|held| held == root))
}

/// The keep rule ([`ExecWalls::keep`]), from the wall's entry, the root turn's first wait and
/// whether a grant for it is held.
fn keep(
    subscribed: bool,
    entry: Entry,
    since: Option<Instant>,
    granted: bool,
    now: Instant,
) -> Option<Duration> {
    if !subscribed || entry.verdict == Verdict::Declined {
        return None;
    }
    if granted {
        return Some(UNKNOWN_RESET_WAIT);
    }
    let left = since.map_or(HOST_KEPT_BOUND, |since| {
        HOST_KEPT_BOUND.saturating_sub(now.saturating_duration_since(since))
    });
    if left.is_zero() || (entry.waited && entry.verdict != Verdict::Keep) {
        return None;
    }
    Some(UNKNOWN_RESET_WAIT.min(left))
}

/// A grant the runtime holds for a root turn ([`ExecWalls::hold`]).
pub struct GrantHold {
    walls: Arc<ExecWalls>,
    root: String,
}

impl Drop for GrantHold {
    fn drop(&mut self) {
        self.walls.grants.send_if_modified(|held| {
            match held.iter().position(|turn| *turn == self.root) {
                Some(index) => {
                    held.remove(index);
                    true
                }
                None => false,
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wall_kind::Workspace;

    const ROOT: &str = "root-turn";

    fn credits() -> WallKind {
        WallKind::Workspace(Workspace::MemberCreditsDepleted)
    }

    fn walls() -> Arc<ExecWalls> {
        let walls = Arc::new(ExecWalls::default());
        walls.root_turn(ROOT);
        walls
    }

    /// One refusal at `kind`, judged and, when kept, waited: its wait, or `None` when it ends.
    fn refusal(walls: &ExecWalls, kind: &WallKind, now: Instant) -> Option<Duration> {
        let wait = walls.keep(kind, true, now)?;
        walls.waited(kind, now);
        Some(wait)
    }

    #[test]
    fn a_host_kept_wall_is_kept_only_while_a_runtime_and_its_host_may_move_it() {
        let now = Instant::now();
        for subscribed in [false, true] {
            for waited in [false, true] {
                for verdict in [Verdict::Unknown, Verdict::Keep, Verdict::Declined] {
                    let walls = walls();
                    if waited {
                        walls.waited(&credits(), now);
                    }
                    walls.decide(ROOT, &credits(), verdict);
                    let kept = subscribed
                        && verdict != Verdict::Declined
                        && (!waited || verdict == Verdict::Keep);
                    assert_eq!(
                        walls.keep(&credits(), subscribed, now),
                        kept.then_some(UNKNOWN_RESET_WAIT),
                        "subscribed={subscribed} waited={waited} {verdict:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn three_and_more_refusals_in_a_row_are_kept_while_the_host_says_keep() {
        let walls = walls();
        let start = Instant::now();
        for retry in 0..6u32 {
            let now = start + UNKNOWN_RESET_WAIT * retry;
            assert_eq!(
                refusal(&walls, &credits(), now),
                Some(UNKNOWN_RESET_WAIT),
                "{retry}"
            );
            // The runtime asks during each wait; the host answers `waiting`.
            walls.decide(ROOT, &credits(), Verdict::Keep);
        }
        // `not-needed` ends it at the next refusal.
        walls.decide(ROOT, &credits(), Verdict::Declined);
        assert_eq!(
            refusal(&walls, &credits(), start + HOST_KEPT_BOUND / 2),
            None
        );
    }

    #[test]
    fn a_wall_no_host_answers_ends_at_its_second_refusal() {
        let walls = walls();
        let now = Instant::now();
        // Core judges each refusal twice before its wait (whether it is terminal, then whether
        // to retry): judging records nothing, only the wait does.
        for _ in 0..3 {
            assert_eq!(walls.keep(&credits(), true, now), Some(UNKNOWN_RESET_WAIT));
        }
        assert_eq!(refusal(&walls, &credits(), now), Some(UNKNOWN_RESET_WAIT));
        assert_eq!(refusal(&walls, &credits(), now + UNKNOWN_RESET_WAIT), None);
        // A first wait unanswered for one kind takes nothing from another's.
        let cap = WallKind::Workspace(Workspace::MemberUsageLimitReached);
        assert_eq!(
            refusal(&walls, &cap, now + UNKNOWN_RESET_WAIT),
            Some(UNKNOWN_RESET_WAIT)
        );
        assert_eq!(refusal(&walls, &cap, now + UNKNOWN_RESET_WAIT * 2), None);
    }

    #[test]
    fn the_whole_keep_is_bounded_unless_a_grant_for_the_root_turn_is_held() {
        let walls = walls();
        let start = Instant::now();
        assert_eq!(refusal(&walls, &credits(), start), Some(UNKNOWN_RESET_WAIT));
        walls.decide(ROOT, &credits(), Verdict::Keep);
        let ten_left = start + HOST_KEPT_BOUND - Duration::from_secs(10);
        assert_eq!(
            walls.keep(&credits(), true, ten_left),
            Some(Duration::from_secs(10)),
            "a wait never runs past the bound"
        );
        let at_bound = start + HOST_KEPT_BOUND;
        assert_eq!(walls.keep(&credits(), true, at_bound), None);
        // Another kind's first wait is inside the same bound.
        let cap = WallKind::Workspace(Workspace::OwnerUsageLimitReached);
        assert_eq!(walls.keep(&cap, true, at_bound), None);
        // A grant for the root turn keeps it past the bound; one for another turn does not.
        let other = walls.hold("other-turn");
        assert!(!walls.granted());
        assert_eq!(walls.keep(&credits(), true, at_bound), None);
        let hold = walls.hold(ROOT);
        assert!(walls.granted());
        assert_eq!(
            walls.keep(&credits(), true, at_bound + HOST_KEPT_BOUND),
            Some(UNKNOWN_RESET_WAIT)
        );
        // Not past a decline, and not without a runtime.
        walls.decide(ROOT, &credits(), Verdict::Declined);
        assert_eq!(walls.keep(&credits(), true, at_bound), None);
        walls.decide(ROOT, &credits(), Verdict::Keep);
        assert_eq!(walls.keep(&credits(), false, at_bound), None);
        drop(hold);
        drop(other);
        assert!(!walls.granted());
        assert_eq!(walls.keep(&credits(), true, at_bound), None);
    }

    #[test]
    fn a_new_root_turn_starts_over_and_a_verdict_for_another_root_turn_is_dropped() {
        let walls = walls();
        let now = Instant::now();
        refusal(&walls, &credits(), now);
        walls.decide(ROOT, &credits(), Verdict::Declined);
        assert_eq!(walls.verdict(&credits()), Verdict::Declined);
        // The same root turn named again keeps its walls.
        walls.root_turn(ROOT);
        assert_eq!(walls.verdict(&credits()), Verdict::Declined);
        walls.root_turn("next-turn");
        assert_eq!(walls.verdict(&credits()), Verdict::Unknown);
        let late = now + HOST_KEPT_BOUND * 2;
        assert_eq!(
            refusal(&walls, &credits(), late),
            Some(UNKNOWN_RESET_WAIT),
            "a new root turn has its own first wait and bound"
        );
        walls.decide(ROOT, &credits(), Verdict::Keep);
        assert_eq!(
            walls.verdict(&credits()),
            Verdict::Unknown,
            "a stale verdict"
        );
        assert_eq!(refusal(&walls, &credits(), late), None);
        // No root turn yet: nothing is recorded for one.
        let fresh = ExecWalls::default();
        fresh.decide(ROOT, &credits(), Verdict::Keep);
        assert_eq!(fresh.verdict(&credits()), Verdict::Unknown);
        assert!(!fresh.granted());
    }

    #[tokio::test(start_paused = true)]
    async fn a_held_grant_outlasts_a_kept_waits_timer() {
        let walls = walls();
        let hold = walls.hold(ROOT);
        let waiting = {
            let walls = Arc::clone(&walls);
            tokio::spawn(async move {
                walls
                    .kept_wait(tokio::time::sleep(UNKNOWN_RESET_WAIT))
                    .await
            })
        };
        tokio::time::sleep(UNKNOWN_RESET_WAIT * 4).await;
        assert!(!waiting.is_finished(), "the timer alone does not end it");
        drop(hold);
        tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .expect("letting the grant go ends the wait")
            .expect("the wait");
        // With no grant held, or one for another root turn, the timer ends it.
        let other = walls.hold("other-turn");
        let started = tokio::time::Instant::now();
        walls
            .kept_wait(tokio::time::sleep(UNKNOWN_RESET_WAIT))
            .await;
        assert_eq!(started.elapsed(), UNKNOWN_RESET_WAIT);
        drop(other);
    }
}
