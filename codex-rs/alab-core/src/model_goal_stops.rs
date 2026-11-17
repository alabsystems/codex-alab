// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact committed native goal mutations awaiting their tool completion. These
//! are source facts, not continuation authority: the autopilot still checks the
//! native completion, current epoch, person stops, current goal and policy.

use crate::goal_persistence::MutationEpoch;
use crate::goal_persistence::MutationTransition;
use crate::session::WallGoal;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::OnceLock;

const MAX_PENDING: usize = 256;
const MAX_SEEN: usize = 16_384;
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
}

/// One acknowledged native mutation, whether or not it established a model stop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommittedMutation {
    pub transition: MutationTransition,
    pub stop: Option<CommittedStop>,
    pub mode_epoch: u64,
}

/// Live authority names the exact native owner, boundary and ALab mode generation.
/// Only `goal` is serialized; copied metadata cannot recreate either local fence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub goal: WallGoal,
    pub epoch: MutationEpoch,
    pub mode_epoch: u64,
}

/// Passive process custody survives an Autopilot runtime replacement. A stale
/// token grants no action, but an exact native duplicate transition may transfer it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Custody {
    Known(Binding),
    Import {
        epoch: MutationEpoch,
        mode_epoch: u64,
    },
    Closed,
}

#[derive(Debug)]
struct AcceptedDuplicate {
    goal: WallGoal,
    mode_epoch: u64,
    before: MutationEpoch,
    // Conflicting successors permanently close this retained edge. A repeated
    // callback cannot choose whichever successor happens to be current later.
    after: Option<MutationEpoch>,
}

#[derive(Debug)]
struct AcceptedOrigin {
    epoch: MutationEpoch,
    mode_epoch: u64,
    // One exact native epoch cannot have two different original sources.
    goal: Option<WallGoal>,
}

#[derive(Debug, Default)]
struct AcceptedEvidence {
    edges: VecDeque<AcceptedDuplicate>,
    origins: VecDeque<AcceptedOrigin>,
}

/// Keep the accepted origin separate from the binding resolved at the target.
/// The resolved goal preserves the source turn while carrying the latest exact
/// duplicate's durable chronology generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSource {
    pub origin: Binding,
    pub binding: Binding,
}

/// Accepted callback evidence for one thread, owned by its Shared session.
/// Edges are not stop authority: the caller must validate completion before
/// accepting one and validate the native owner, mode and policy before use.
#[derive(Debug, Default)]
pub struct AcceptedStops {
    evidence: Mutex<AcceptedEvidence>,
}

impl AcceptedStops {
    /// Retain an accepted Active origin or Blocked duplicate, never Other or a
    /// non-stop mutation. Repeated identical callbacks are idempotent; evidence
    /// which assigns two meanings to one native epoch fails closed.
    pub fn accept(&self, mutation: &CommittedMutation) -> bool {
        let Some(stop) = mutation.stop.as_ref() else {
            return false;
        };
        if stop.previous_status == PreviousStatus::Other
            || mutation.transition.before == mutation.transition.after
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
                .find(|origin| origin.epoch == mutation.transition.after)
            {
                if origin.mode_epoch == mutation.mode_epoch
                    && origin.goal.as_ref() == Some(&stop.goal)
                {
                    return true;
                }
                origin.goal = None;
                return false;
            }
            while evidence.origins.len() >= MAX_ACCEPTED_SOURCES {
                evidence.origins.pop_front();
            }
            evidence.origins.push_back(AcceptedOrigin {
                epoch: mutation.transition.after.clone(),
                mode_epoch: mutation.mode_epoch,
                goal: Some(stop.goal.clone()),
            });
            return true;
        }
        if let Some(edge) = evidence
            .edges
            .iter_mut()
            .find(|edge| edge.before == mutation.transition.before)
        {
            if edge.mode_epoch == mutation.mode_epoch
                && edge.after.as_ref() == Some(&mutation.transition.after)
                && edge.goal == stop.goal
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
            mode_epoch: mutation.mode_epoch,
            before: mutation.transition.before.clone(),
            after: Some(mutation.transition.after.clone()),
        });
        true
    }

    /// Advance an existing exact source through a complete accepted duplicate
    /// path. Missing transitions, a mode boundary or conflicting evidence grant
    /// no replacement token.
    pub fn advance(&self, source: &Binding, target: &MutationEpoch) -> Option<Binding> {
        let evidence = self
            .evidence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        advance_binding(&evidence.edges, source, target)
    }

    /// Find one original accepted source whose complete duplicate path reaches
    /// this exact target in this exact mode. Unresolved origins remain passive;
    /// they are never assigned whichever epoch happens to be latest.
    pub fn source(&self, target: &MutationEpoch, mode_epoch: u64) -> Option<ResolvedSource> {
        let evidence = self
            .evidence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut resolved: Option<ResolvedSource> = None;
        for origin in evidence
            .origins
            .iter()
            .filter(|origin| origin.mode_epoch == mode_epoch)
        {
            let Some(goal) = &origin.goal else {
                // A conflicting origin cannot select another metadata bucket
                // after its duplicate path becomes current.
                if advance_edges(&evidence.edges, None, mode_epoch, &origin.epoch, target).is_some()
                {
                    return None;
                }
                continue;
            };
            let Some((_, Some(resolved_goal))) = advance_edges(
                &evidence.edges,
                Some(goal.clone()),
                mode_epoch,
                &origin.epoch,
                target,
            ) else {
                continue;
            };
            let candidate = ResolvedSource {
                origin: Binding {
                    goal: goal.clone(),
                    epoch: origin.epoch.clone(),
                    mode_epoch,
                },
                binding: Binding {
                    goal: resolved_goal,
                    epoch: target.clone(),
                    mode_epoch,
                },
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

    /// Retire only origins proven to resolve to this exact bound source. A newer
    /// independent Active origin, even with equal goal text, survives cleanup.
    pub fn retire(&self, bound: &Binding) -> usize {
        let mut evidence = self
            .evidence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let AcceptedEvidence { edges, origins } = &mut *evidence;
        let before = origins.len();
        origins.retain(|origin| {
            let Some(goal) = origin.goal.clone() else {
                return true;
            };
            if origin.mode_epoch != bound.mode_epoch {
                return true;
            }
            let source = Binding {
                goal,
                epoch: origin.epoch.clone(),
                mode_epoch: origin.mode_epoch,
            };
            !advance_binding(edges, &source, &bound.epoch)
                .is_some_and(|resolved| resolved == *bound)
        });
        before - origins.len()
    }
}

fn advance_binding(
    edges: &VecDeque<AcceptedDuplicate>,
    source: &Binding,
    target: &MutationEpoch,
) -> Option<Binding> {
    let (_, goal) = advance_edges(
        edges,
        Some(source.goal.clone()),
        source.mode_epoch,
        &source.epoch,
        target,
    )?;
    Some(Binding {
        goal: goal?,
        epoch: target.clone(),
        mode_epoch: source.mode_epoch,
    })
}

fn advance_edges(
    edges: &VecDeque<AcceptedDuplicate>,
    mut goal: Option<WallGoal>,
    mode_epoch: u64,
    from: &MutationEpoch,
    target: &MutationEpoch,
) -> Option<(MutationEpoch, Option<WallGoal>)> {
    let mut at = from.clone();
    let mut visited = Vec::new();
    for _ in 0..=MAX_ACCEPTED_EDGES {
        if &at == target {
            return Some((at, goal));
        }
        if visited.contains(&at) {
            return None;
        }
        visited.push(at.clone());
        let edge = edges.iter().find(|edge| {
            edge.before == at
                && edge.mode_epoch == mode_epoch
                && goal
                    .as_ref()
                    .is_none_or(|goal| same_duplicate_goal(&edge.goal, goal))
        })?;
        at = edge.after.clone()?;
        if let Some(goal) = goal.as_mut() {
            goal.chronology_fence_generation = edge.goal.chronology_fence_generation;
        }
    }
    None
}

fn same_duplicate_goal(left: &WallGoal, right: &WallGoal) -> bool {
    left.goal_id == right.goal_id
        && left.objective == right.objective
        && left.token_budget == right.token_budget
        && left.person_stops == right.person_stops
}

#[derive(Clone, PartialEq, Eq)]
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

/// A colliding key invalidates pending evidence. Exact seen keys retain
/// process-lifetime, fail-closed reuse detection after pending eviction while
/// still allowing a distinct native invocation to reuse provider IDs.
enum Pending {
    Ready(Box<CommittedMutation>),
    Tombstone,
}

#[derive(Default)]
struct Registry {
    pending: VecDeque<(Key, Pending)>,
    seen: Vec<Key>,
    closed: bool,
}

impl Registry {
    fn committed(&mut self, key: Key, mutation: CommittedMutation) {
        if let Some((_, existing)) = self.pending.iter_mut().find(|(old, _)| old == &key) {
            *existing = Pending::Tombstone;
            return;
        }
        if self.closed || self.seen.iter().any(|seen| seen == &key) {
            return;
        }
        // Invocation is deliberately opaque and equality-only. Retain bounded
        // exact identities instead of hashing provider IDs and conflating two
        // actual calls. Capacity exhaustion closes registration fail-safe.
        if self.seen.len() >= MAX_SEEN {
            self.closed = true;
            return;
        }
        self.seen.push(key.clone());
        while self.pending.len() >= MAX_PENDING {
            self.pending.pop_front();
        }
        self.pending
            .push_back((key, Pending::Ready(Box::new(mutation))));
    }

    fn take(&mut self, key: &Key) -> Option<CommittedMutation> {
        let position = self.pending.iter().position(|(stored, _)| stored == key)?;
        match self.pending.remove(position)?.1 {
            Pending::Ready(mutation) => Some(*mutation),
            Pending::Tombstone => None,
        }
    }
}

static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(Mutex::default)
}

/// Called only after the native goal tool acknowledged its persisted mutation.
/// Only the actual scoped native dispatch can publish it. A non-stop mutation
/// carries `None` while still fencing an older live source.
pub fn committed(thread: &str, turn: &str, call: &str, mutation: CommittedMutation) {
    let Some(key) = Key::current(thread, turn, call) else {
        return;
    };
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .committed(key, mutation);
}

/// Consume only this actual invocation's host thread, turn and call. The exact
/// seen identity remains, so a colliding late producer cannot mint a second fact.
pub fn take(thread: &str, turn: &str, call: &str) -> Option<CommittedMutation> {
    let key = Key::current(thread, turn, call)?;
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take(&key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goal_persistence::State;
    use std::sync::Arc;

    const MODE: u64 = 7;

    fn goal(turn: &str) -> WallGoal {
        WallGoal {
            goal_id: "goal-1".into(),
            objective: "continue the original objective".into(),
            token_budget: Some(500),
            chronology_fence_generation: None,
            turn: turn.into(),
            person_stops: 0,
        }
    }

    fn acknowledged(
        state: &Arc<State>,
        turn: &str,
        previous_status: PreviousStatus,
    ) -> CommittedMutation {
        CommittedMutation {
            transition: state
                .begin_mutation()
                .commit_mutation()
                .expect("an actual acknowledged State mutation"),
            stop: Some(CommittedStop {
                goal: goal(turn),
                previous_status,
            }),
            mode_epoch: MODE,
        }
    }

    fn source_binding(mutation: &CommittedMutation) -> Binding {
        Binding {
            goal: mutation.stop.as_ref().unwrap().goal.clone(),
            epoch: mutation.transition.after.clone(),
            mode_epoch: mutation.mode_epoch,
        }
    }

    fn generic_mutation() -> CommittedMutation {
        let state = Arc::new(State::default());
        CommittedMutation {
            transition: state.begin_mutation().commit_mutation().unwrap(),
            stop: None,
            mode_epoch: MODE,
        }
    }

    fn key(
        invocation: &crate::model_goal_invocation::Invocation,
        thread: &str,
        turn: &str,
        call: &str,
    ) -> Key {
        Key {
            invocation: invocation.clone(),
            thread: thread.to_owned(),
            turn: turn.to_owned(),
            call: call.to_owned(),
        }
    }

    #[test]
    fn accepted_duplicates_resolve_an_origin_arriving_after_their_callbacks() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&state, "original-turn", PreviousStatus::Active);
        let first = acknowledged(&state, "duplicate-one", PreviousStatus::Blocked);
        let second = acknowledged(&state, "duplicate-two", PreviousStatus::Blocked);
        let target = state.mutation_epoch().unwrap();
        assert!(accepted.accept(&second));
        assert!(accepted.source(&target, MODE).is_none());
        assert!(
            accepted
                .advance(&source_binding(&origin), &target)
                .is_none()
        );
        assert!(accepted.accept(&first));
        assert!(
            accepted.source(&target, MODE).is_none(),
            "edges alone supply no source"
        );
        assert!(accepted.accept(&origin));
        let expected = ResolvedSource {
            origin: source_binding(&origin),
            binding: Binding {
                goal: origin.stop.as_ref().unwrap().goal.clone(),
                epoch: target.clone(),
                mode_epoch: MODE,
            },
        };
        assert_eq!(accepted.source(&target, MODE), Some(expected.clone()));
        assert_eq!(accepted.source(&target, MODE), Some(expected));
        assert!(accepted.accept(&first), "the same edge is idempotent");
        assert_eq!(
            accepted
                .advance(&source_binding(&first), &target)
                .map(|binding| binding.epoch),
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
        assert!(
            accepted
                .source(&state.mutation_epoch().unwrap(), MODE)
                .is_none()
        );
        let completed = CommittedMutation {
            transition: pending
                .commit_mutation()
                .expect("the pending native mutation settles"),
            stop: Some(CommittedStop {
                goal: goal("pending-duplicate"),
                previous_status: PreviousStatus::Blocked,
            }),
            mode_epoch: MODE,
        };
        assert!(accepted.accept(&completed));
        let target = state.mutation_epoch().unwrap();
        let resolved = accepted
            .source(&target, MODE)
            .expect("complete accepted chain");
        assert_eq!(resolved.origin.goal, origin.stop.as_ref().unwrap().goal);
        assert_eq!(resolved.origin.epoch, origin.transition.after);
        assert_eq!(resolved.binding.epoch, completed.transition.after);
    }

    #[test]
    fn fresh_other_and_non_stop_mutations_cannot_fill_a_duplicate_gap() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&state, "fresh-source", PreviousStatus::Active);
        assert!(accepted.accept(&origin));
        assert!(
            accepted
                .advance(
                    &Binding {
                        goal: origin.stop.as_ref().unwrap().goal.clone(),
                        epoch: origin.transition.before.clone(),
                        mode_epoch: MODE,
                    },
                    &origin.transition.after,
                )
                .is_none()
        );
        let other = acknowledged(&state, "paused-to-blocked", PreviousStatus::Other);
        assert!(!accepted.accept(&other));
        let gap = CommittedMutation {
            transition: state.begin_mutation().commit_mutation().unwrap(),
            stop: None,
            mode_epoch: MODE,
        };
        assert!(!accepted.accept(&gap));
        let duplicate = acknowledged(&state, "later-duplicate", PreviousStatus::Blocked);
        assert!(accepted.accept(&duplicate));
        assert!(accepted.source(&duplicate.transition.after, MODE).is_none());
    }

    #[test]
    fn duplicate_matching_keeps_source_turn_and_advances_only_chronology() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let mut origin = acknowledged(&state, "original-turn", PreviousStatus::Active);
        let mut duplicate =
            acknowledged(&state, "different-duplicate-turn", PreviousStatus::Blocked);
        let original_generation =
            crate::goal_chronology_fence::FenceGeneration::from_hex(&"01".repeat(32)).unwrap();
        let duplicate_generation =
            crate::goal_chronology_fence::FenceGeneration::from_hex(&"02".repeat(32)).unwrap();
        origin
            .stop
            .as_mut()
            .unwrap()
            .goal
            .chronology_fence_generation = Some(original_generation);
        duplicate
            .stop
            .as_mut()
            .unwrap()
            .goal
            .chronology_fence_generation = Some(duplicate_generation);
        assert!(accepted.accept(&origin));
        assert!(accepted.accept(&duplicate));
        let target = state.mutation_epoch().unwrap();
        let resolved = accepted.source(&target, MODE).unwrap();
        assert_eq!(resolved.origin.goal.turn, "original-turn");
        assert_eq!(
            resolved.origin.goal.chronology_fence_generation,
            Some(original_generation)
        );
        assert_eq!(resolved.binding.goal.turn, "original-turn");
        assert_eq!(
            resolved.binding.goal.chronology_fence_generation,
            Some(duplicate_generation)
        );
        for change in ["goal", "objective", "budget", "person"] {
            let mut different = source_binding(&origin);
            match change {
                "goal" => different.goal.goal_id = "another-goal".into(),
                "objective" => different.goal.objective = "another objective".into(),
                "budget" => different.goal.token_budget = None,
                "person" => different.goal.person_stops = 1,
                _ => unreachable!(),
            }
            assert!(accepted.advance(&different, &target).is_none(), "{change}");
        }
    }

    #[test]
    fn mode_gaps_and_owner_changes_cannot_be_replaced_by_the_latest_epoch() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&state, "original-turn", PreviousStatus::Active);
        let mut wrong_mode = acknowledged(&state, "wrong-mode", PreviousStatus::Blocked);
        wrong_mode.mode_epoch = MODE + 1;
        assert!(accepted.accept(&origin));
        assert!(accepted.accept(&wrong_mode));
        assert!(
            accepted
                .source(&state.mutation_epoch().unwrap(), MODE + 1)
                .is_none()
        );
        assert!(
            accepted
                .source(&state.mutation_epoch().unwrap(), MODE)
                .is_none()
        );

        let original_owner = Arc::new(State::default());
        let replacement_owner = Arc::new(State::default());
        let separate = AcceptedStops::default();
        let origin = acknowledged(&original_owner, "owner-one", PreviousStatus::Active);
        let duplicate = acknowledged(&replacement_owner, "owner-two", PreviousStatus::Blocked);
        assert!(separate.accept(&origin));
        assert!(separate.accept(&duplicate));
        assert!(
            separate
                .source(&replacement_owner.mutation_epoch().unwrap(), MODE)
                .is_none()
        );
    }

    #[test]
    fn retirement_keeps_a_newer_independent_origin_and_retains_edges() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&state, "same-turn-and-goal", PreviousStatus::Active);
        let duplicate = acknowledged(&state, "duplicate-turn", PreviousStatus::Blocked);
        assert!(accepted.accept(&origin));
        assert!(accepted.accept(&duplicate));
        let retired = accepted
            .source(&duplicate.transition.after, MODE)
            .unwrap()
            .binding;
        let _gap = state.begin_mutation().commit_mutation().unwrap();
        let newer = acknowledged(&state, "same-turn-and-goal", PreviousStatus::Active);
        assert_eq!(
            newer.stop.as_ref().unwrap().goal,
            origin.stop.as_ref().unwrap().goal
        );
        assert!(accepted.accept(&newer));
        assert_eq!(accepted.retire(&retired), 1);
        assert!(accepted.source(&retired.epoch, MODE).is_none());
        assert_eq!(
            accepted
                .advance(&source_binding(&origin), &retired.epoch)
                .map(|binding| binding.epoch),
            Some(retired.epoch.clone())
        );
        assert_eq!(accepted.retire(&retired), 0);
        assert_eq!(
            accepted
                .source(&newer.transition.after, MODE)
                .unwrap()
                .origin
                .epoch,
            newer.transition.after
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
        for index in 1..MAX_ACCEPTED_EDGES {
            let duplicate = acknowledged(
                &state,
                &format!("duplicate-{index}"),
                PreviousStatus::Blocked,
            );
            assert!(accepted.accept(&duplicate));
        }
        assert!(
            accepted
                .source(&state.mutation_epoch().unwrap(), MODE)
                .is_some()
        );
        assert!(accepted.accept(&first));
        let extra = acknowledged(&state, "evicts-the-prefix", PreviousStatus::Blocked);
        assert!(accepted.accept(&extra));
        let target = state.mutation_epoch().unwrap();
        assert!(accepted.source(&target, MODE).is_none());
        assert!(
            accepted
                .advance(&source_binding(&origin), &target)
                .is_none()
        );
        assert_eq!(
            accepted
                .advance(&source_binding(&first), &target)
                .map(|binding| binding.epoch),
            Some(target)
        );
    }

    #[test]
    fn bounded_origins_cannot_rediscover_an_evicted_source() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let first = acknowledged(&state, "first-origin", PreviousStatus::Active);
        assert!(accepted.accept(&first));
        for index in 1..MAX_ACCEPTED_SOURCES {
            let origin = acknowledged(&state, &format!("origin-{index}"), PreviousStatus::Active);
            assert!(accepted.accept(&origin));
        }
        assert!(accepted.source(&first.transition.after, MODE).is_some());
        let extra = acknowledged(&state, "newest-origin", PreviousStatus::Active);
        assert!(accepted.accept(&extra));
        assert!(accepted.source(&first.transition.after, MODE).is_none());
        assert_eq!(
            accepted
                .source(&extra.transition.after, MODE)
                .unwrap()
                .origin
                .goal,
            extra.stop.as_ref().unwrap().goal
        );
    }

    #[test]
    fn conflicting_duplicate_successors_or_metadata_poison_the_exact_epoch() {
        for change in ["successor", "goal", "objective", "budget", "person", "turn"] {
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
                "successor" => conflicting.transition.after = second.transition.after,
                "goal" => conflicting.stop.as_mut().unwrap().goal.goal_id = "another-goal".into(),
                "objective" => {
                    conflicting.stop.as_mut().unwrap().goal.objective = "another objective".into()
                }
                "budget" => conflicting.stop.as_mut().unwrap().goal.token_budget = None,
                "person" => conflicting.stop.as_mut().unwrap().goal.person_stops = 1,
                "turn" => conflicting.stop.as_mut().unwrap().goal.turn = "another-turn".into(),
                _ => unreachable!(),
            }
            assert!(!accepted.accept(&conflicting));
            assert!(!accepted.accept(&first));
            assert!(
                accepted
                    .source(&state.mutation_epoch().unwrap(), MODE)
                    .is_none()
            );
        }
    }

    #[test]
    fn conflicting_origins_and_identical_text_at_two_epochs_are_ambiguous() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&state, "same-goal-and-turn", PreviousStatus::Active);
        assert!(accepted.accept(&origin));
        let mut conflict = origin.clone();
        conflict.stop.as_mut().unwrap().goal.objective = "other".into();
        assert!(!accepted.accept(&conflict));
        assert!(!accepted.accept(&origin));

        let separate = AcceptedStops::default();
        let first = acknowledged(&state, "same-goal-and-turn", PreviousStatus::Active);
        let duplicate = acknowledged(&state, "duplicate", PreviousStatus::Blocked);
        assert!(separate.accept(&first));
        assert!(separate.accept(&duplicate));
        let mut second_origin = duplicate;
        second_origin.stop.as_mut().unwrap().previous_status = PreviousStatus::Active;
        second_origin.stop.as_mut().unwrap().goal = first.stop.as_ref().unwrap().goal.clone();
        assert!(separate.accept(&second_origin));
        assert!(
            separate
                .source(&state.mutation_epoch().unwrap(), MODE)
                .is_none()
        );
    }

    #[test]
    fn ordinary_accounting_preserves_an_accepted_source() {
        let state = Arc::new(State::default());
        let accepted = AcceptedStops::default();
        let origin = acknowledged(&state, "original-turn", PreviousStatus::Active);
        assert!(accepted.accept(&origin));
        state.begin().finish(true);
        let resolved = accepted
            .source(&state.mutation_epoch().unwrap(), MODE)
            .expect("the source is unchanged");
        assert_eq!(resolved.origin.goal, origin.stop.as_ref().unwrap().goal);
        assert_eq!(resolved.origin.epoch, origin.transition.after);
    }

    #[test]
    fn registry_preserves_mutations_and_tombstones_reused_exact_calls() {
        let invocation = crate::model_goal_invocation::Invocation::new();
        let mut registry = Registry::default();
        let mutation = generic_mutation();
        let exact = key(&invocation, "thread", "turn", "call");
        registry.committed(exact.clone(), mutation.clone());
        assert_eq!(registry.take(&exact), Some(mutation.clone()));
        registry.committed(exact.clone(), mutation.clone());
        assert!(registry.take(&exact).is_none());

        let collision = key(&invocation, "collision-thread", "turn", "call");
        registry.committed(collision.clone(), mutation.clone());
        registry.committed(collision.clone(), mutation);
        assert!(registry.take(&collision).is_none());
    }

    #[test]
    fn an_evicted_identity_cannot_reenter_but_a_new_invocation_can_reuse_provider_ids() {
        let invocation = crate::model_goal_invocation::Invocation::new();
        let mut registry = Registry::default();
        let mutation = generic_mutation();
        let evicted = key(
            &invocation,
            "evicted-thread",
            "evicted-turn",
            "evicted-call",
        );
        registry.committed(evicted.clone(), mutation.clone());
        for index in 0..MAX_PENDING {
            registry.committed(
                key(
                    &invocation,
                    "fill-thread",
                    &format!("fill-turn-{index}"),
                    &format!("fill-call-{index}"),
                ),
                generic_mutation(),
            );
        }
        assert!(registry.take(&evicted).is_none());
        registry.committed(evicted.clone(), mutation.clone());
        assert!(registry.take(&evicted).is_none());

        let replacement = crate::model_goal_invocation::Invocation::new();
        let reused = key(
            &replacement,
            "evicted-thread",
            "evicted-turn",
            "evicted-call",
        );
        registry.committed(reused.clone(), mutation.clone());
        assert_eq!(registry.take(&reused), Some(mutation));
    }

    #[tokio::test]
    async fn committed_mutation_is_consumed_only_by_its_exact_scoped_call() {
        let first = crate::model_goal_invocation::Invocation::new();
        let other = crate::model_goal_invocation::Invocation::new();
        let mutation = generic_mutation();
        first
            .scope(async {
                committed(
                    "scoped-marker-thread",
                    "scoped-marker-turn",
                    "scoped-marker-call",
                    mutation.clone(),
                );
            })
            .await;
        other
            .scope(async {
                assert!(
                    take(
                        "scoped-marker-thread",
                        "scoped-marker-turn",
                        "scoped-marker-call"
                    )
                    .is_none()
                );
            })
            .await;
        first
            .scope(async {
                assert!(take("other-thread", "scoped-marker-turn", "scoped-marker-call").is_none());
                assert!(take("scoped-marker-thread", "other-turn", "scoped-marker-call").is_none());
                assert!(take("scoped-marker-thread", "scoped-marker-turn", "other-call").is_none());
                assert_eq!(
                    take(
                        "scoped-marker-thread",
                        "scoped-marker-turn",
                        "scoped-marker-call"
                    ),
                    Some(mutation)
                );
            })
            .await;
        assert!(
            take(
                "scoped-marker-thread",
                "scoped-marker-turn",
                "scoped-marker-call"
            )
            .is_none()
        );
    }
}
