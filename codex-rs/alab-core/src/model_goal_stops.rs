// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact committed native goal stops awaiting their tool completion. This is
//! source evidence, not continuation authority: the autopilot still checks the
//! native completion, person stops, current goal, policy and mutation admission.

use crate::goal_persistence::MutationEpoch;
use crate::goal_persistence::MutationReceipt;
use crate::goal_persistence::State;
use crate::session::WallGoal;
use std::collections::VecDeque;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

const MAX_PENDING: usize = 256;
const MAX_ACCEPTED_EDGES: usize = 256;
const MAX_ACCEPTED_SOURCES: usize = 256;

/// The state read under the native mutation permit before this committed stop.
/// Only Blocked-to-Blocked can preserve an earlier source without new authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviousStatus {
    Active,
    Blocked,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommittedStop {
    pub goal: WallGoal,
    pub previous_status: PreviousStatus,
    pub receipt: MutationReceipt,
}

/// Live authority also names the exact native mutation owner and boundary.
/// Only `goal` is serialized; a copied goal cannot recreate this local fence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub goal: WallGoal,
    pub epoch: MutationEpoch,
}

/// Passive process custody survives an Autopilot runtime replacement. A stale
/// token grants no action, but an exact native duplicate receipt may transfer it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Custody {
    Known(Binding),
    Import(MutationEpoch),
    Closed,
}

#[derive(Debug)]
struct AcceptedDuplicate {
    goal: WallGoal,
    before: MutationEpoch,
    // Conflicting successors permanently close this retained edge. A repeated
    // callback cannot choose whichever successor happens to be current later.
    after: Option<MutationEpoch>,
}

#[derive(Debug)]
struct AcceptedOrigin {
    epoch: MutationEpoch,
    // One exact native epoch cannot have two different original sources.
    goal: Option<WallGoal>,
}

#[derive(Debug, Default)]
struct AcceptedEvidence {
    edges: VecDeque<AcceptedDuplicate>,
    origins: VecDeque<AcceptedOrigin>,
}

/// Keep the accepted origin's exact epoch separate from the resolved endpoint.
/// Equal goal text is not evidence that a later callback owns this origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSource {
    pub origin: Binding,
    pub epoch: MutationEpoch,
}

/// Accepted callback evidence for one thread, owned by its Shared session.
/// Edges are not stop authority: the caller must validate completion and person
/// stops before accepting one, and validate the actual native owner and current
/// epoch again before publishing or admitting a resolved source.
#[derive(Debug, Default)]
pub struct AcceptedStops {
    evidence: Mutex<AcceptedEvidence>,
}

impl AcceptedStops {
    /// Retain an accepted Active origin or Blocked duplicate, never Other.
    /// Fresh sources do not fill duplicate gaps. Repeated identical callbacks
    /// are idempotent; conflicting native epochs fail closed.
    pub fn accept(&self, stop: &CommittedStop) -> bool {
        if stop.previous_status == PreviousStatus::Other
            || stop.receipt.before == stop.receipt.after
        {
            return false;
        }
        let mut evidence = self
            .evidence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if stop.previous_status == PreviousStatus::Active {
            if let Some(origin) = evidence
                .origins
                .iter_mut()
                .find(|origin| origin.epoch == stop.receipt.after)
            {
                if origin.goal.as_ref() == Some(&stop.goal) {
                    return true;
                }
                origin.goal = None;
                return false;
            }
            while evidence.origins.len() >= MAX_ACCEPTED_SOURCES {
                evidence.origins.pop_front();
            }
            evidence.origins.push_back(AcceptedOrigin {
                epoch: stop.receipt.after.clone(),
                goal: Some(stop.goal.clone()),
            });
            return true;
        }
        if let Some(edge) = evidence
            .edges
            .iter_mut()
            .find(|edge| edge.before == stop.receipt.before)
        {
            if edge.after.as_ref() == Some(&stop.receipt.after)
                && same_duplicate_goal(&edge.goal, &stop.goal)
            {
                return true;
            }
            edge.after = None;
            return false;
        }
        while evidence.edges.len() >= MAX_ACCEPTED_EDGES {
            evidence.edges.pop_front();
        }
        evidence.edges.push_back(AcceptedDuplicate {
            goal: stop.goal.clone(),
            before: stop.receipt.before.clone(),
            after: Some(stop.receipt.after.clone()),
        });
        true
    }

    /// Resolve an original or existing source through exact accepted edges to
    /// the supplied target. This never samples a latest epoch, consumes an edge,
    /// changes the source turn, or bridges an absent API/mode/owner transition.
    pub fn advance(
        &self,
        goal: &WallGoal,
        from: &MutationEpoch,
        target: &MutationEpoch,
    ) -> Option<MutationEpoch> {
        let evidence = self
            .evidence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        advance_edges(&evidence.edges, Some(goal), from, target)
    }

    /// Find one original accepted source whose complete duplicate path reaches
    /// this exact target. An unresolved origin stays retained while later native
    /// writes are in flight; it is never assigned whichever epoch is latest.
    pub fn source(&self, target: &MutationEpoch) -> Option<ResolvedSource> {
        let evidence = self
            .evidence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut resolved: Option<ResolvedSource> = None;
        for origin in &evidence.origins {
            let Some(goal) = &origin.goal else {
                // A conflicting origin cannot select a different metadata
                // bucket after its duplicate path becomes current.
                if advance_edges(&evidence.edges, None, &origin.epoch, target).is_some() {
                    return None;
                }
                continue;
            };
            let Some(epoch) = advance_edges(&evidence.edges, Some(goal), &origin.epoch, target)
            else {
                continue;
            };
            let candidate = ResolvedSource {
                origin: Binding {
                    goal: goal.clone(),
                    epoch: origin.epoch.clone(),
                },
                epoch,
            };
            if resolved
                .as_ref()
                .is_some_and(|prior| prior.origin != candidate.origin)
            {
                return None;
            }
            resolved = Some(candidate);
        }
        resolved
    }

    /// Retire only origins proven to reach this exact bound source. A newer
    /// independent Active origin, even with equal goal text, must survive an old
    /// history read's cleanup. Accepted duplicate edges remain available.
    pub fn retire(&self, bound: &Binding) -> usize {
        let mut evidence = self
            .evidence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let AcceptedEvidence { edges, origins } = &mut *evidence;
        let before = origins.len();
        origins.retain(|origin| {
            !origin.goal.as_ref().is_some_and(|goal| {
                goal == &bound.goal
                    && advance_edges(edges, Some(goal), &origin.epoch, &bound.epoch).is_some()
            })
        });
        before - origins.len()
    }
}

fn advance_edges(
    edges: &VecDeque<AcceptedDuplicate>,
    goal: Option<&WallGoal>,
    from: &MutationEpoch,
    target: &MutationEpoch,
) -> Option<MutationEpoch> {
    let mut at = from.clone();
    let mut visited = Vec::new();
    for _ in 0..=MAX_ACCEPTED_EDGES {
        if &at == target {
            return Some(at);
        }
        if visited.contains(&at) {
            return None;
        }
        visited.push(at.clone());
        let edge = edges.iter().find(|edge| {
            edge.before == at && goal.is_none_or(|goal| same_duplicate_goal(&edge.goal, goal))
        })?;
        at = edge.after.clone()?;
    }
    None
}

fn same_duplicate_goal(left: &WallGoal, right: &WallGoal) -> bool {
    left.goal_id == right.goal_id
        && left.objective == right.objective
        && left.token_budget == right.token_budget
        && left.person_stops == right.person_stops
}

struct ExpectedResume {
    thread: String,
    epoch: MutationEpoch,
}

tokio::task_local! { static EXPECTED_RESUME: ExpectedResume; }

pub async fn resume_scoped<F: Future>(thread: String, epoch: MutationEpoch, work: F) -> F::Output {
    EXPECTED_RESUME
        .scope(ExpectedResume { thread, epoch }, work)
        .await
}

pub fn resume_unscoped() -> bool {
    EXPECTED_RESUME.try_with(|_| ()).is_err()
}

/// Called against the runtime actually selected by the native goal service,
/// while its mutation permit stands. Normal non-model API calls have no scope.
pub fn resume_allowed(thread: &str, state: &Arc<State>) -> bool {
    EXPECTED_RESUME
        .try_with(|expected| expected.thread == thread && state.epoch_is_current(&expected.epoch))
        .unwrap_or(true)
}

#[derive(PartialEq, Eq)]
struct Key {
    invocation: crate::model_goal_invocation::Invocation,
    thread: String,
    turn: String,
    call: String,
}

impl Key {
    fn current(thread: &str, turn: &str, call: &str) -> Option<Self> {
        Some(Self {
            invocation: crate::model_goal_invocation::current()?,
            thread: thread.to_owned(),
            turn: turn.to_owned(),
            call: call.to_owned(),
        })
    }
}

static PENDING: OnceLock<Mutex<VecDeque<(Key, CommittedStop)>>> = OnceLock::new();

fn pending() -> &'static Mutex<VecDeque<(Key, CommittedStop)>> {
    PENDING.get_or_init(Mutex::default)
}

/// Called only after the native goal tool persisted Blocked under its mutation
/// permit. Only an actual scoped native dispatch can publish this fact. A
/// repeated block records its preexisting stopped state distinctly.
pub fn committed(thread: &str, turn: &str, call: &str, stop: CommittedStop) {
    let Some(key) = Key::current(thread, turn, call) else {
        return;
    };
    let mut pending = pending()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    pending.retain(|(old, _)| old != &key);
    while pending.len() >= MAX_PENDING {
        pending.pop_front();
    }
    pending.push_back((key, stop));
}

/// Consume only this actual invocation's host thread, turn and call, including
/// failed completions. An absent scope, eviction or missing fact grants nothing.
pub fn take(thread: &str, turn: &str, call: &str) -> Option<CommittedStop> {
    let key = Key::current(thread, turn, call)?;
    let mut pending = pending()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let position = pending.iter().position(|(stored, _)| stored == &key)?;
    pending.remove(position).map(|(_, stop)| stop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goal(turn: &str) -> WallGoal {
        WallGoal {
            goal_id: "goal-1".into(),
            objective: "continue the original objective".into(),
            token_budget: Some(500),
            turn: turn.into(),
            person_stops: 0,
        }
    }

    fn acknowledged(
        state: &Arc<State>,
        turn: &str,
        previous_status: PreviousStatus,
    ) -> CommittedStop {
        CommittedStop {
            goal: goal(turn),
            previous_status,
            receipt: state
                .begin_mutation()
                .commit_mutation_with_receipt()
                .expect("an actual acknowledged State mutation"),
        }
    }

    #[test]
    fn accepted_duplicates_resolve_an_origin_arriving_after_their_callbacks() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&state, "original-turn", PreviousStatus::Active);
        let first = acknowledged(&state, "duplicate-one", PreviousStatus::Blocked);
        let second = acknowledged(&state, "duplicate-two", PreviousStatus::Blocked);
        let target = state.mutation_epoch();
        assert!(accepted.accept(&second));
        assert!(accepted.source(&target).is_none());
        assert!(
            accepted
                .advance(&origin.goal, &origin.receipt.after, &target)
                .is_none()
        );
        assert!(accepted.accept(&first));
        assert!(
            accepted.source(&target).is_none(),
            "edges alone supply no source"
        );
        assert!(accepted.accept(&origin));
        let expected = ResolvedSource {
            origin: Binding {
                goal: origin.goal.clone(),
                epoch: origin.receipt.after.clone(),
            },
            epoch: target.clone(),
        };
        assert_eq!(accepted.source(&target), Some(expected.clone()));
        assert_eq!(accepted.source(&target), Some(expected));
        assert!(
            accepted.accept(&first),
            "the same accepted edge is idempotent"
        );
        assert_eq!(
            accepted.advance(&origin.goal, &first.receipt.after, &target),
            Some(target)
        );
    }

    #[test]
    fn an_accepted_origin_survives_until_an_in_flight_duplicate_is_acknowledged() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&state, "original-turn", PreviousStatus::Active);
        let first = acknowledged(&state, "first-duplicate", PreviousStatus::Blocked);
        let pending = state.begin_mutation();
        assert!(accepted.accept(&first));
        assert!(accepted.accept(&origin));
        assert!(accepted.source(&state.mutation_epoch()).is_none());
        let completed = CommittedStop {
            goal: goal("pending-duplicate"),
            previous_status: PreviousStatus::Blocked,
            receipt: pending
                .commit_mutation_with_receipt()
                .expect("the pending native mutation settles"),
        };
        assert!(accepted.accept(&completed));
        let resolved = accepted
            .source(&state.mutation_epoch())
            .expect("complete accepted chain");
        assert_eq!(resolved.origin.goal, origin.goal);
        assert_eq!(resolved.origin.epoch, origin.receipt.after);
        assert_eq!(resolved.epoch, completed.receipt.after);
    }

    #[test]
    fn fresh_and_other_stops_cannot_fill_a_duplicate_gap() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&state, "fresh-source", PreviousStatus::Active);
        assert!(accepted.accept(&origin));
        assert!(
            accepted
                .advance(&origin.goal, &origin.receipt.before, &origin.receipt.after)
                .is_none()
        );
        assert_eq!(
            accepted.source(&origin.receipt.after).unwrap().origin.epoch,
            origin.receipt.after
        );
        let other = acknowledged(&state, "paused-to-blocked", PreviousStatus::Other);
        assert!(!accepted.accept(&other));
        let duplicate = acknowledged(&state, "later-duplicate", PreviousStatus::Blocked);
        assert!(accepted.accept(&duplicate));
        assert!(accepted.source(&duplicate.receipt.after).is_none());
    }

    #[test]
    fn duplicate_matching_keeps_the_source_turn_and_requires_all_goal_metadata() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&state, "original-turn", PreviousStatus::Active);
        let duplicate = acknowledged(&state, "different-duplicate-turn", PreviousStatus::Blocked);
        assert!(accepted.accept(&origin));
        assert!(accepted.accept(&duplicate));
        let target = state.mutation_epoch();
        assert_eq!(accepted.source(&target).unwrap().origin.goal, origin.goal);
        for change in ["goal", "objective", "budget", "person"] {
            let mut different = origin.goal.clone();
            match change {
                "goal" => different.goal_id = "another-goal".into(),
                "objective" => different.objective = "another objective".into(),
                "budget" => different.token_budget = None,
                "person" => different.person_stops = 1,
                _ => unreachable!(),
            }
            assert!(
                accepted
                    .advance(&different, &origin.receipt.after, &target)
                    .is_none(),
                "a duplicate cannot transfer different {change} metadata"
            );
        }
    }

    #[test]
    fn api_mode_and_owner_changes_cannot_be_replaced_by_the_latest_epoch() {
        for passive in [false, true] {
            let state = Arc::new(State::default());
            let accepted = AcceptedStops::default();
            let origin = acknowledged(&state, "original-turn", PreviousStatus::Active);
            assert!(accepted.accept(&origin));
            if passive {
                state.revoke_mutation_epoch();
            } else {
                state.begin_mutation().commit_mutation();
            }
            let duplicate = acknowledged(&state, "after-api-or-mode", PreviousStatus::Blocked);
            assert!(accepted.accept(&duplicate));
            assert!(accepted.source(&state.mutation_epoch()).is_none());
        }
        let original_owner = Arc::new(State::default());
        let replacement_owner = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&original_owner, "original-turn", PreviousStatus::Active);
        let duplicate = acknowledged(&replacement_owner, "other-owner", PreviousStatus::Blocked);
        assert!(accepted.accept(&origin));
        assert!(accepted.accept(&duplicate));
        assert!(
            accepted
                .source(&replacement_owner.mutation_epoch())
                .is_none()
        );
    }

    #[test]
    fn retirement_keeps_a_newer_independent_origin_and_retains_duplicate_edges() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&state, "same-turn-and-goal", PreviousStatus::Active);
        let duplicate = acknowledged(&state, "duplicate-turn", PreviousStatus::Blocked);
        assert!(accepted.accept(&origin));
        assert!(accepted.accept(&duplicate));
        let retired = Binding {
            goal: origin.goal.clone(),
            epoch: duplicate.receipt.after,
        };
        state.revoke_mutation_epoch();
        let newer = acknowledged(&state, "same-turn-and-goal", PreviousStatus::Active);
        assert_eq!(newer.goal, origin.goal);
        assert!(accepted.accept(&newer));
        assert_eq!(accepted.retire(&retired), 1);
        assert!(accepted.source(&retired.epoch).is_none());
        assert_eq!(
            accepted.advance(&origin.goal, &origin.receipt.after, &retired.epoch),
            Some(retired.epoch.clone())
        );
        assert_eq!(accepted.retire(&retired), 0);
        assert_eq!(
            accepted.source(&newer.receipt.after).unwrap().origin.epoch,
            newer.receipt.after
        );
    }

    #[test]
    fn bounded_edges_evict_an_old_prefix_without_retokening_its_origin() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&state, "original-turn", PreviousStatus::Active);
        assert!(accepted.accept(&origin));
        let first = acknowledged(&state, "first-duplicate", PreviousStatus::Blocked);
        assert!(accepted.accept(&first));
        for index in 1..256 {
            let duplicate = acknowledged(
                &state,
                &format!("duplicate-{index}"),
                PreviousStatus::Blocked,
            );
            assert!(accepted.accept(&duplicate));
        }
        assert!(accepted.source(&state.mutation_epoch()).is_some());
        assert!(
            accepted.accept(&first),
            "repeated callbacks do not refresh capacity"
        );
        let extra = acknowledged(&state, "evicts-the-prefix", PreviousStatus::Blocked);
        assert!(accepted.accept(&extra));
        let target = state.mutation_epoch();
        assert!(accepted.source(&target).is_none());
        assert!(
            accepted
                .advance(&origin.goal, &origin.receipt.after, &target)
                .is_none()
        );
        assert_eq!(
            accepted.advance(&origin.goal, &first.receipt.after, &target),
            Some(target)
        );
    }

    #[test]
    fn bounded_origins_cannot_rediscover_an_evicted_source() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let first = acknowledged(&state, "first-origin", PreviousStatus::Active);
        assert!(accepted.accept(&first));
        for index in 1..256 {
            let origin = acknowledged(&state, &format!("origin-{index}"), PreviousStatus::Active);
            assert!(accepted.accept(&origin));
        }
        assert!(accepted.source(&first.receipt.after).is_some());
        let extra = acknowledged(&state, "newest-origin", PreviousStatus::Active);
        assert!(accepted.accept(&extra));
        assert!(accepted.source(&first.receipt.after).is_none());
        assert_eq!(
            accepted.source(&extra.receipt.after).unwrap().origin.goal,
            extra.goal
        );
    }

    #[test]
    fn conflicting_duplicate_successors_or_goal_metadata_poison_the_exact_epoch() {
        for change in ["successor", "goal", "objective", "budget", "person"] {
            let state = Arc::new(State::default());
            let accepted = AcceptedStops::default();
            let origin = acknowledged(&state, "original-turn", PreviousStatus::Active);
            let first = acknowledged(&state, "first-duplicate", PreviousStatus::Blocked);
            let second = acknowledged(&state, "second-duplicate", PreviousStatus::Blocked);
            assert!(accepted.accept(&origin));
            assert!(accepted.accept(&first));
            assert!(accepted.accept(&second));
            let mut conflicting = first.clone();
            match change {
                "successor" => conflicting.receipt.after = second.receipt.after.clone(),
                "goal" => conflicting.goal.goal_id = "another-goal".into(),
                "objective" => conflicting.goal.objective = "another objective".into(),
                "budget" => conflicting.goal.token_budget = None,
                "person" => conflicting.goal.person_stops = 1,
                _ => unreachable!(),
            }
            assert!(!accepted.accept(&conflicting));
            assert!(
                !accepted.accept(&first),
                "replay cannot reopen the poisoned edge"
            );
            assert!(
                accepted.source(&state.mutation_epoch()).is_none(),
                "{change}"
            );
            assert!(
                accepted
                    .advance(
                        &conflicting.goal,
                        &origin.receipt.after,
                        &state.mutation_epoch()
                    )
                    .is_none()
            );
        }
    }

    #[test]
    fn conflicting_origin_metadata_including_turn_cannot_select_a_source() {
        for change in ["goal", "objective", "budget", "person", "turn"] {
            let state = Arc::new(State::default());
            let accepted = AcceptedStops::default();
            let origin = acknowledged(&state, "original-turn", PreviousStatus::Active);
            assert!(accepted.accept(&origin));
            let mut conflicting = origin.clone();
            match change {
                "goal" => conflicting.goal.goal_id = "another-goal".into(),
                "objective" => conflicting.goal.objective = "another objective".into(),
                "budget" => conflicting.goal.token_budget = None,
                "person" => conflicting.goal.person_stops = 1,
                "turn" => conflicting.goal.turn = "another-origin-turn".into(),
                _ => unreachable!(),
            }
            assert!(!accepted.accept(&conflicting));
            assert!(!accepted.accept(&origin));
            let duplicate = acknowledged(&state, "duplicate-turn", PreviousStatus::Blocked);
            assert!(accepted.accept(&duplicate));
            assert!(
                accepted.source(&state.mutation_epoch()).is_none(),
                "{change}"
            );
        }
    }

    #[test]
    fn identical_goal_text_cannot_disambiguate_distinct_original_epochs() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&state, "same-goal-and-turn", PreviousStatus::Active);
        let duplicate = acknowledged(&state, "duplicate", PreviousStatus::Blocked);
        assert!(accepted.accept(&origin));
        assert!(accepted.accept(&duplicate));
        // A native writer cannot report both original states for one mutation.
        // Even inconsistent callback evidence must not pick an origin by text.
        let mut another_origin = duplicate;
        another_origin.previous_status = PreviousStatus::Active;
        another_origin.goal = origin.goal;
        assert!(accepted.accept(&another_origin));
        assert!(accepted.source(&state.mutation_epoch()).is_none());
    }

    #[test]
    fn defensive_cycles_and_separate_instances_supply_no_missing_path() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let separate = AcceptedStops::default();
        let origin = acknowledged(&state, "original-turn", PreviousStatus::Active);
        let first = acknowledged(&state, "first-duplicate", PreviousStatus::Blocked);
        assert!(accepted.accept(&origin));
        assert!(accepted.accept(&first));
        assert!(
            separate
                .advance(&origin.goal, &origin.receipt.after, &first.receipt.after)
                .is_none()
        );
        let target = acknowledged(&state, "unlinked-target", PreviousStatus::Blocked);
        let cycle = CommittedStop {
            goal: goal("malformed-cycle"),
            previous_status: PreviousStatus::Blocked,
            receipt: MutationReceipt {
                before: first.receipt.after,
                after: origin.receipt.after,
            },
        };
        assert!(accepted.accept(&cycle));
        assert!(accepted.source(&target.receipt.after).is_none());
    }

    #[test]
    fn ordinary_accounting_preserves_an_accepted_source() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&state, "original-turn", PreviousStatus::Active);
        assert!(accepted.accept(&origin));
        state.begin().finish(true);
        let resolved = accepted
            .source(&state.mutation_epoch())
            .expect("the source is unchanged");
        assert_eq!(resolved.origin.goal, origin.goal);
        assert_eq!(resolved.origin.epoch, origin.receipt.after);
        assert_eq!(resolved.epoch, state.mutation_epoch());
    }

    #[tokio::test]
    async fn a_committed_stop_is_consumed_only_by_its_exact_native_call() {
        crate::model_goal_invocation::Invocation::new()
            .scope(async {
                let state = Arc::new(State::default());
                let stop = CommittedStop {
                    goal: WallGoal {
                        goal_id: "goal-1".into(),
                        objective: "continue the original objective".into(),
                        token_budget: Some(500),
                        turn: "marker-turn".into(),
                        person_stops: 0,
                    },
                    previous_status: PreviousStatus::Active,
                    receipt: state
                        .begin_mutation()
                        .commit_mutation_with_receipt()
                        .expect("exact native mutation receipt"),
                };
                committed("marker-thread", "marker-turn", "marker-call", stop.clone());
                assert!(take("other-thread", "marker-turn", "marker-call").is_none());
                assert!(take("marker-thread", "other-turn", "marker-call").is_none());
                assert!(take("marker-thread", "marker-turn", "other-call").is_none());
                assert_eq!(
                    take("marker-thread", "marker-turn", "marker-call"),
                    Some(stop)
                );
                assert!(take("marker-thread", "marker-turn", "marker-call").is_none());
            })
            .await;
    }
}
