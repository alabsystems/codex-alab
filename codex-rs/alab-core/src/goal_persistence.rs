// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Failed goal persistence is backpressure, never permission for another model turn.
//! Dispatched writes finish their bookkeeping even when their observer disappears.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::oneshot;

/// Persistence-only attribution for host/API goal outcomes. Native turn IDs are
/// generated separately; this sentinel never identifies executable work.
pub const HOST_MUTATION_TURN_ID: &str = "alab-host-goal-mutation-boundary";

/// Persistence-only attribution for idle accounting. Legacy events without a
/// turn ID remain ambiguous with old host/API writes and therefore fail closed.
pub const IDLE_ACCOUNTING_TURN_ID: &str = "alab-idle-goal-accounting-boundary";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admission {
    Ready,
    Writing,
    Held,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchedMutationError {
    Progress,
    Held,
}

impl std::fmt::Display for DispatchedMutationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Progress => "goal accounting changed before mutation dispatch",
            Self::Held => "goal persistence has no clean mutation boundary",
        })
    }
}

#[derive(Debug)]
pub struct State(Mutex<Inner>, tokio::sync::watch::Sender<u64>);

#[cfg(test)]
#[path = "goal_mutation_epoch_tests.rs"]
mod mutation_epoch_tests;

impl Default for State {
    fn default() -> Self {
        Self(Mutex::default(), tokio::sync::watch::channel(0).0)
    }
}

/// One atomic passive view; failed admission can coexist with a live writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub revision: u64,
    pub admission: Admission,
    pub writes_pending: bool,
}

#[derive(Debug, Default)]
struct Inner {
    mutation_sequence: u128,
    mutation_exhausted: bool,
    failure_generation: u128,
    accounting_generation: u128,
    accounting_reconciled_generation: u128,
    accounting_data_generation: u128,
    accounting_data_reconciled_generation: u128,
    accounting_data_settlement_generation: Option<u128>,
    accounting_clock_open: bool,
    permanent_hold: bool,
    retired: bool,
    revision: u64,
    next: u128,
    active: BTreeMap<u128, ActiveWrite>,
    failed: bool,
    failed_additive: bool,
    ambiguous: bool,
    settled_mutation_unknown: bool,
    quarantined: BTreeMap<String, QuarantinedUsage>,
}

#[derive(Clone, Copy, Debug)]
struct ActiveWrite {
    sequence: u128,
    additive: bool,
    mutation: bool,
    data_snapshot: Option<u128>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AccountingWitness {
    writes: u128,
    data: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconciliationCandidate(AccountingWitness);

pub struct AccountingDataChange<'a> {
    state: &'a State,
    inner: std::sync::MutexGuard<'a, Inner>,
    clock_open: Option<bool>,
    fresh_boundary: bool,
    additive_settlement: bool,
    capture_additive_after_change: bool,
    changed: bool,
}

impl Drop for AccountingDataChange<'_> {
    fn drop(&mut self) {
        self.apply_pending();
    }
}

impl AccountingDataChange<'_> {
    fn apply_pending(&mut self) {
        if !self.changed {
            return;
        }
        let previous_generation = self.inner.accounting_data_generation;
        if let Some(open) = self.clock_open {
            self.inner.accounting_clock_open = open;
        }
        match self.inner.accounting_data_generation.checked_add(1) {
            Some(generation) => self.inner.accounting_data_generation = generation,
            None => State::permanently_hold(&mut self.inner),
        }
        if self.capture_additive_after_change
            && let Some(owner) = ACCOUNTING_OWNER.try_with(|owner| *owner).ok().flatten()
        {
            let generation = self.inner.accounting_data_generation;
            if let Some(write) = self.inner.active.get_mut(&owner)
                && write.additive
            {
                write.data_snapshot = Some(generation);
            }
        }
        if self.fresh_boundary
            && previous_generation == self.inner.accounting_data_reconciled_generation
            && !self.inner.permanent_hold
            && !self.inner.ambiguous
            && !self.inner.failed_additive
            && !self.inner.active.values().any(|write| write.additive)
        {
            self.inner.accounting_data_reconciled_generation =
                self.inner.accounting_data_generation;
            self.inner.accounting_data_settlement_generation = None;
        }
        if self.additive_settlement
            && !self.inner.permanent_hold
            && !self.inner.ambiguous
            && !self.inner.failed_additive
            && let Some(owner) = ACCOUNTING_OWNER.try_with(|owner| *owner).ok().flatten()
        {
            let settled_generation = self.inner.accounting_data_generation;
            let settled = if let Some(write) = self.inner.active.get_mut(&owner)
                && write.additive
                && write.data_snapshot == Some(previous_generation)
            {
                true
            } else {
                false
            };
            if settled {
                self.inner.accounting_data_settlement_generation = Some(settled_generation);
            }
        }
        self.state.changed(&mut self.inner);
        self.clock_open = None;
        self.fresh_boundary = false;
        self.additive_settlement = false;
        self.capture_additive_after_change = false;
        self.changed = false;
    }

    /// Check every State-side prerequisite except the elapsed-time latch, which
    /// the caller will close while retaining this same guard.
    pub fn can_prepare_dispatched_mutation(&self) -> bool {
        !self.inner.retired
            && !self.inner.mutation_exhausted
            && !self.inner.permanent_hold
            && !self.inner.failed_additive
            && self.inner.accounting_generation == self.inner.accounting_reconciled_generation
            && self.inner.accounting_data_generation
                == self.inner.accounting_data_reconciled_generation
            && !self
                .inner
                .active
                .values()
                .any(|write| write.mutation || write.additive)
    }

    /// True while a native goal mutation still owns its storage command. A
    /// same-goal callback may retain attribution, but must not consume the
    /// elapsed-time freeze until that command settles.
    pub fn mutation_in_flight(&self) -> bool {
        self.inner.active.values().any(|write| write.mutation)
    }

    /// Apply the final frozen-clock publication and create an already-dispatched
    /// mutation before releasing the State guard. No callback can enter a
    /// freeze/begin/dispatch check-use gap.
    pub fn begin_dispatched_mutation(&mut self, state: &Arc<State>) -> Option<Write> {
        if !std::ptr::eq(self.state, state.as_ref()) {
            return None;
        }
        self.apply_pending();
        if !self.can_prepare_dispatched_mutation() || self.inner.accounting_clock_open {
            return None;
        }

        let before = MutationEpoch {
            owner: Arc::downgrade(state),
            sequence: self.inner.mutation_sequence,
        };
        let accounting_witness = AccountingWitness {
            writes: self.inner.accounting_generation,
            data: self.inner.accounting_data_generation,
        };
        let failure_witness = self.inner.failure_generation;
        let predecessor_authoritative = !self.inner.ambiguous;
        let predecessor_settled_mutation_unknown = self.inner.settled_mutation_unknown;
        let sequence = match self.inner.mutation_sequence.checked_add(1) {
            Some(sequence) => {
                self.inner.mutation_sequence = sequence;
                sequence
            }
            None => {
                self.inner.mutation_exhausted = true;
                self.inner.failed = true;
                self.inner.ambiguous = true;
                self.state.changed(&mut self.inner);
                return None;
            }
        };
        let id = match self.inner.next.checked_add(1) {
            Some(id) => {
                self.inner.next = id;
                id
            }
            None => {
                self.inner.failed = true;
                self.inner.ambiguous = true;
                self.inner.permanent_hold = true;
                self.state.changed(&mut self.inner);
                return None;
            }
        };
        self.inner.active.insert(
            id,
            ActiveWrite {
                sequence,
                additive: false,
                mutation: true,
                data_snapshot: None,
            },
        );
        self.state.changed(&mut self.inner);
        Some(Write {
            state: Arc::clone(state),
            id: Some(id),
            epoch: Some(MutationEpoch {
                owner: Arc::downgrade(state),
                sequence,
            }),
            before: Some(before),
            finished: false,
            mutation: true,
            dispatched: true,
            admitted: true,
            accounting_witness: Some(accounting_witness),
            failure_witness: Some(failure_witness),
            predecessor_authoritative,
            predecessor_settled_mutation_unknown,
        })
    }

    /// Final effect authorization under the same State lock that protects the
    /// accounting mutation. A callback or newer mutation cannot enter the gap.
    pub fn mutation_effect_authorized(&self) -> bool {
        let authorized = MUTATION_EFFECT
            .try_with(|effect| State::mutation_effect_matches(self.state, &self.inner, effect))
            .unwrap_or(true);
        if !authorized {
            let _ = MUTATION_EFFECT_REJECTED.try_with(|rejected| rejected.set(true));
        }
        authorized
    }

    /// Authorize only acknowledgement of a non-destructive same-goal effect
    /// after callbacks added data to its exact clean post-commit boundary.
    pub fn mutation_effect_pending_data_only_authorized(&self) -> bool {
        let authorized = MUTATION_EFFECT
            .try_with(|effect| {
                State::mutation_effect_pending_data_only_matches(self.state, &self.inner, effect)
            })
            .unwrap_or(false);
        if !authorized {
            let _ = MUTATION_EFFECT_REJECTED.try_with(|rejected| rejected.set(true));
        }
        authorized
    }

    /// Destructive unscoped associations require the same exact clean State
    /// boundary as mutation effects. This closes the turn-start check/use gap
    /// without making harmless read-only probes mutation authority.
    pub fn destructive_change_authorized(&self) -> bool {
        let authorized = MUTATION_EFFECT
            .try_with(|effect| State::mutation_effect_matches(self.state, &self.inner, effect))
            .unwrap_or_else(|_| State::unscoped_change_matches(&self.inner));
        if !authorized {
            let _ = MUTATION_EFFECT_REJECTED.try_with(|rejected| rejected.set(true));
        }
        authorized
    }

    /// Bind the unique active additive write to the exact native inputs read
    /// while this guard excludes accounting callbacks and mutation dispatch.
    pub fn capture_additive_snapshot(&mut self) -> bool {
        let Some(owner) = ACCOUNTING_OWNER.try_with(|owner| *owner).ok().flatten() else {
            return false;
        };
        let generation = self.inner.accounting_data_generation;
        if let Some(write) = self.inner.active.get_mut(&owner)
            && write.additive
        {
            write.data_snapshot = Some(generation);
            return true;
        }
        false
    }

    /// Freeze a live elapsed-time clock and bind the additive snapshot to the
    /// generation produced by that freeze, rather than the instant before it.
    pub fn publish_frozen_additive_snapshot(&mut self) -> bool {
        let Some(owner) = ACCOUNTING_OWNER.try_with(|owner| *owner).ok().flatten() else {
            return false;
        };
        if !self
            .inner
            .active
            .get(&owner)
            .is_some_and(|write| write.additive)
        {
            return false;
        }
        self.clock_open = Some(false);
        self.capture_additive_after_change = true;
        self.changed = true;
        true
    }

    pub fn additive_snapshot_is_current(&self) -> bool {
        ACCOUNTING_OWNER
            .try_with(|owner| {
                owner.is_some_and(|owner| {
                    self.inner.active.get(&owner).is_some_and(|write| {
                        write.additive
                            && write.data_snapshot == Some(self.inner.accounting_data_generation)
                    })
                })
            })
            .unwrap_or(false)
    }

    pub fn publish_change(&mut self) {
        self.changed = true;
    }

    pub fn publish_clock_change(&mut self, open: bool) {
        self.clock_open = Some(open);
        self.changed = true;
    }

    /// A new association starts from a fresh baseline, but it may only advance
    /// reconciliation when all earlier snapshot input was already reconciled.
    pub fn publish_fresh_clock(&mut self) {
        self.clock_open = Some(true);
        self.fresh_boundary = true;
        self.changed = true;
    }

    /// Freeze an exactly empty elapsed-time boundary without discarding its goal
    /// association. Token callbacks remain attributable and invalidate dispatch.
    pub fn publish_frozen_clock(&mut self) {
        self.clock_open = Some(false);
        self.fresh_boundary = true;
        self.changed = true;
    }

    pub fn publish_fresh_closed_clock(&mut self) {
        self.clock_open = Some(false);
        self.fresh_boundary = true;
        self.changed = true;
    }

    /// Bind baseline settlement to the exact snapshot version it consumed.
    /// A callback before or after settlement leaves the later generation held.
    pub fn publish_additive_settlement(&mut self, close_clock: bool) {
        self.clock_open = close_clock.then_some(false);
        self.additive_settlement = true;
        self.changed = true;
    }

    pub fn publish_additive_settlement_and_reopen(&mut self) {
        self.clock_open = Some(true);
        self.additive_settlement = true;
        self.changed = true;
    }
}

/// Opaque identity for one exact mutation boundary of one native persistence owner.
///
/// A weak owner keeps replacement states distinct without retaining a retired runtime.
/// The sequence is deliberately private: equality and [`State`] validation are the only
/// authority this value exposes.
#[derive(Clone)]
pub struct MutationEpoch {
    owner: Weak<State>,
    sequence: u128,
}

impl std::fmt::Debug for MutationEpoch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MutationEpoch")
            .finish_non_exhaustive()
    }
}

impl PartialEq for MutationEpoch {
    fn eq(&self, other: &Self) -> bool {
        self.sequence == other.sequence && self.owner.ptr_eq(&other.owner)
    }
}

impl Eq for MutationEpoch {}

impl MutationEpoch {
    /// Whether this exact owner/sequence boundary is still live.
    pub fn is_current(&self) -> bool {
        self.owner
            .upgrade()
            .is_some_and(|owner| owner.mutation_epoch_is_current(self))
    }

    /// Only a never-mutated native owner can originate import custody.
    pub fn is_initial(&self) -> bool {
        self.sequence == 0
    }
}

/// Exact post-commit authority for applying one mutation's in-memory effects.
///
/// Unlike [`MutationEpoch`], this witness is intentionally invalidated by any
/// accounting generation that settles after the SQL mutation. Durable model
/// custody must survive accounting-only work; delayed runtime side effects must
/// not overtake it.
#[derive(Clone)]
pub struct MutationEffect {
    epoch: MutationEpoch,
    accounting: AccountingWitness,
    failure_generation: u128,
}

impl std::fmt::Debug for MutationEffect {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MutationEffect")
            .finish_non_exhaustive()
    }
}

impl PartialEq for MutationEffect {
    fn eq(&self, other: &Self) -> bool {
        self.epoch == other.epoch
            && self.accounting == other.accounting
            && self.failure_generation == other.failure_generation
    }
}

impl Eq for MutationEffect {}

/// The exact pre-write and acknowledged post-write epochs of one committed mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutationTransition {
    pub before: MutationEpoch,
    pub after: MutationEpoch,
    pub effect: MutationEffect,
}

/// An unsupported raw goal replacement crossed an unacknowledged accounting
/// boundary. Keep the original observations; this is never replay authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuarantinedUsage {
    pub original_goal_id: String,
    pub elapsed_seconds: i64,
    pub descendant_tokens: i64,
    pub turn_tokens: i64,
}

/// The strongest persistence restriction a replacement runtime must inherit.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum HoldClass {
    #[default]
    Ready,
    Failed,
    Ambiguous,
    Permanent,
}

/// Process-local custody transferred from one retired persistence owner to the next.
///
/// The epoch itself is never transplanted. `non_pristine` is a monotone chronology
/// tombstone; the hold class and quarantine evidence preserve the retired owner's
/// safety restrictions without retaining that owner.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PersistenceCarry {
    non_pristine: bool,
    hold: HoldClass,
    mutation_exhausted: bool,
    settled_mutation_unknown: bool,
    quarantined: BTreeMap<String, QuarantinedUsage>,
}

impl PersistenceCarry {
    pub fn is_pristine(&self) -> bool {
        !self.non_pristine && self.hold == HoldClass::Ready
    }

    pub fn hold_class(&self) -> HoldClass {
        self.hold
    }

    /// Fence source import as soon as an ownerless mutation intent exists.
    pub fn note_intent(&mut self) {
        self.non_pristine = true;
    }

    /// One exact serialized commit supersedes settled failed/ambiguous outcomes.
    pub fn acknowledge_mutation(&mut self) {
        self.non_pristine = true;
        if self.hold != HoldClass::Permanent {
            self.hold = HoldClass::Ready;
            self.settled_mutation_unknown = false;
        }
    }

    pub fn hold_failed(&mut self) {
        self.non_pristine = true;
        self.hold = self.hold.max(HoldClass::Failed);
    }

    pub fn hold_ambiguous(&mut self) {
        self.non_pristine = true;
        self.hold = self.hold.max(HoldClass::Ambiguous);
        self.settled_mutation_unknown = false;
    }

    pub fn hold_settled_mutation_unknown(&mut self) {
        self.non_pristine = true;
        if self.hold < HoldClass::Ambiguous || self.settled_mutation_unknown {
            self.hold = HoldClass::Ambiguous;
            self.settled_mutation_unknown = true;
        }
    }

    pub fn hold_permanently(&mut self) {
        self.non_pristine = true;
        self.hold = HoldClass::Permanent;
        self.settled_mutation_unknown = false;
    }

    pub fn merge(&mut self, other: Self) {
        self.non_pristine |= other.non_pristine;
        let self_ambiguous = self.hold >= HoldClass::Ambiguous;
        let other_ambiguous = other.hold >= HoldClass::Ambiguous;
        let settled_mutation_unknown = (!self_ambiguous || self.settled_mutation_unknown)
            && (!other_ambiguous || other.settled_mutation_unknown);
        self.hold = self.hold.max(other.hold);
        self.settled_mutation_unknown =
            self.hold == HoldClass::Ambiguous && settled_mutation_unknown;
        self.mutation_exhausted |= other.mutation_exhausted;
        for (goal, usage) in other.quarantined {
            self.quarantined
                .entry(goal)
                .and_modify(|current| {
                    current.elapsed_seconds = current.elapsed_seconds.max(usage.elapsed_seconds);
                    current.descendant_tokens =
                        current.descendant_tokens.max(usage.descendant_tokens);
                    current.turn_tokens = current.turn_tokens.max(usage.turn_tokens);
                })
                .or_insert(usage);
        }
    }
}

/// Opaque equality identity for passive observer lifetimes, never write authority.
pub fn observation_incarnation() -> String {
    format!("{:032x}", rand::random::<u128>())
}

tokio::task_local! {
    static ROLLED_BACK: Arc<AtomicBool>;
    static ACCOUNTING_OWNER: Option<u128>;
    static MUTATION_EFFECT: MutationEffect;
    static MUTATION_EFFECT_REJECTED: std::cell::Cell<bool>;
    static MUTATION_EFFECT_APPLIED: Arc<AtomicBool>;
}

/// Cancellation-visible acknowledgement that the exact native association
/// effect has finished. Custody guards retain a clone outside the scoped future.
#[derive(Clone, Default)]
pub struct MutationEffectAcknowledgement(Arc<AtomicBool>);

impl MutationEffectAcknowledgement {
    pub fn is_applied(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

pub async fn mutation_effect<T>(
    effect: MutationEffect,
    future: impl Future<Output = T>,
) -> (T, bool, bool) {
    let acknowledgement = MutationEffectAcknowledgement::default();
    let (output, rejected) =
        mutation_effect_with_acknowledgement(effect, acknowledgement.clone(), future).await;
    (output, rejected, acknowledgement.is_applied())
}

pub async fn mutation_effect_with_acknowledgement<T>(
    effect: MutationEffect,
    acknowledgement: MutationEffectAcknowledgement,
    future: impl Future<Output = T>,
) -> (T, bool) {
    MUTATION_EFFECT
        .scope(
            effect,
            MUTATION_EFFECT_REJECTED.scope(
                std::cell::Cell::new(false),
                MUTATION_EFFECT_APPLIED.scope(Arc::clone(&acknowledgement.0), async {
                    let output = future.await;
                    let rejected = MUTATION_EFFECT_REJECTED.with(std::cell::Cell::get);
                    (output, rejected)
                }),
            ),
        )
        .await
}

pub fn mutation_effect_rejected() -> bool {
    MUTATION_EFFECT_REJECTED
        .try_with(std::cell::Cell::get)
        .unwrap_or(false)
}

/// A runtime calls this only after its exact accounting transition (including
/// an already-satisfied no-op) is complete under the native mutation permit.
pub fn mutation_effect_applied() {
    let _ = MUTATION_EFFECT_APPLIED.try_with(|applied| applied.store(true, Ordering::SeqCst));
}

/// Called only for a typed error proving that the additive write did not commit.
pub fn definitely_rolled_back() {
    let _ = ROLLED_BACK.try_with(|safe| safe.store(true, Ordering::SeqCst));
}

/// True only inside the owner that will finalize the dispatched accounting write.
pub fn owns_current_write() -> bool {
    ROLLED_BACK.try_with(|_| ()).is_ok()
}

impl State {
    fn classify_admission(inner: &Inner) -> Admission {
        if inner.retired || inner.failed || inner.permanent_hold {
            Admission::Held
        } else if !inner.active.is_empty() {
            // A healthy owner can temporarily advance its captured data
            // generation before SQLite settles. Keep that owner recoverable;
            // any mismatch that remains after settlement is a durable hold.
            Admission::Writing
        } else if inner.accounting_data_generation != inner.accounting_data_reconciled_generation {
            Admission::Held
        } else {
            Admission::Ready
        }
    }

    fn mutation_effect_matches(state: &State, inner: &Inner, effect: &MutationEffect) -> bool {
        std::ptr::eq(effect.epoch.owner.as_ptr(), state)
            && effect.epoch.sequence == inner.mutation_sequence
            && effect.failure_generation == inner.failure_generation
            && effect.accounting.writes == inner.accounting_generation
            && effect.accounting.writes == inner.accounting_reconciled_generation
            && effect.accounting.data == inner.accounting_data_generation
            && effect.accounting.data == inner.accounting_data_reconciled_generation
            && !inner.retired
            && !inner.failed
            && !inner.failed_additive
            && !inner.ambiguous
            && !inner.permanent_hold
            && !inner.accounting_clock_open
            && !inner
                .active
                .values()
                .any(|write| write.mutation || write.additive)
    }

    fn accounting_witness_has_only_pending_data_at_completion(
        inner: &Inner,
        witness: AccountingWitness,
    ) -> bool {
        witness.writes == inner.accounting_generation
            && witness.writes == inner.accounting_reconciled_generation
            && witness.data == inner.accounting_data_reconciled_generation
            && witness.data < inner.accounting_data_generation
            && !inner.retired
            && !inner.failed_additive
            && !inner.permanent_hold
            && !inner.mutation_exhausted
            && !inner.accounting_clock_open
            && !inner
                .active
                .values()
                .any(|write| write.mutation || write.additive)
    }

    fn accounting_witness_has_only_pending_data(inner: &Inner, witness: AccountingWitness) -> bool {
        Self::accounting_witness_has_only_pending_data_at_completion(inner, witness)
            && !inner.failed
            && !inner.ambiguous
    }

    fn mutation_effect_pending_data_only_matches(
        state: &State,
        inner: &Inner,
        effect: &MutationEffect,
    ) -> bool {
        std::ptr::eq(effect.epoch.owner.as_ptr(), state)
            && effect.epoch.sequence == inner.mutation_sequence
            && effect.failure_generation == inner.failure_generation
            && Self::accounting_witness_has_only_pending_data(inner, effect.accounting)
    }

    fn unscoped_change_matches(inner: &Inner) -> bool {
        !inner.retired
            && !inner.failed
            && !inner.failed_additive
            && !inner.ambiguous
            && !inner.permanent_hold
            && !inner.mutation_exhausted
            && !inner.accounting_clock_open
            && inner.accounting_generation == inner.accounting_reconciled_generation
            && inner.accounting_data_generation == inner.accounting_data_reconciled_generation
            && !inner
                .active
                .values()
                .any(|write| write.mutation || write.additive)
    }

    pub fn changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.1.subscribe()
    }

    pub fn snapshot(&self) -> Snapshot {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Snapshot {
            revision: inner.revision,
            admission: Self::classify_admission(&inner),
            writes_pending: !inner.active.is_empty(),
        }
    }

    fn changed(&self, inner: &mut Inner) {
        inner.revision = inner.revision.saturating_add(1);
        self.1.send_replace(inner.revision);
    }

    fn advance_failure(inner: &mut Inner) {
        match inner.failure_generation.checked_add(1) {
            Some(generation) => inner.failure_generation = generation,
            None => {
                inner.permanent_hold = true;
                inner.mutation_exhausted = true;
            }
        }
    }

    fn invalidate_mutation_sequence(inner: &mut Inner) {
        match inner.mutation_sequence.checked_add(1) {
            Some(sequence) if !inner.mutation_exhausted => inner.mutation_sequence = sequence,
            _ => inner.mutation_exhausted = true,
        }
    }

    fn permanently_hold(inner: &mut Inner) {
        Self::invalidate_mutation_sequence(inner);
        Self::advance_failure(inner);
        inner.failed = true;
        inner.ambiguous = true;
        inner.permanent_hold = true;
    }

    /// Close all present and future mutation authority after an unrepresentable
    /// owner transition or dispatch race.
    pub fn hold_permanently(&self) {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::permanently_hold(&mut inner);
        self.changed(&mut inner);
    }

    /// Serialize snapshot-producing accounting callbacks with mutation dispatch.
    pub fn begin_accounting_data_change(&self) -> AccountingDataChange<'_> {
        AccountingDataChange {
            state: self,
            inner: self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            clock_open: None,
            fresh_boundary: false,
            additive_settlement: false,
            capture_additive_after_change: false,
            changed: false,
        }
    }

    /// Destructive accounting resets must drain an open elapsed-time clock even
    /// though a running clock alone does not block ordinary turn admission.
    pub fn accounting_drain_needed(&self) -> bool {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.accounting_clock_open
            || inner.accounting_data_generation != inner.accounting_data_reconciled_generation
            || inner.failed_additive
            || inner.active.values().any(|write| write.additive)
    }

    /// An additive owner has been dispatched; preparation/validation alone is
    /// not proof that a write will outlive its observer.
    pub fn accounting_write_pending(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .values()
            .any(|write| write.additive)
    }

    pub fn accepts_accounting_callbacks(&self) -> bool {
        !self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retired
    }

    pub fn quarantine_original_goal(&self, usage: QuarantinedUsage) {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .quarantined
            .entry(usage.original_goal_id.clone())
            .and_modify(|current| {
                current.elapsed_seconds = current.elapsed_seconds.max(usage.elapsed_seconds);
                current.descendant_tokens = current.descendant_tokens.max(usage.descendant_tokens);
                current.turn_tokens = current.turn_tokens.max(usage.turn_tokens);
            })
            .or_insert(usage);
        inner.failed = true;
        inner.ambiguous = true;
        inner.permanent_hold = true;
        Self::invalidate_mutation_sequence(&mut inner);
        Self::advance_failure(&mut inner);
        self.changed(&mut inner);
    }

    /// Read-only evidence of the original boundary, never a reconciliation API.
    pub fn quarantined_usage(&self) -> Vec<QuarantinedUsage> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .quarantined
            .values()
            .cloned()
            .collect()
    }

    /// Publish before releasing the accounting semaphore, so a queued observer
    /// cannot replay a snapshot whose first commit has an unknown outcome.
    pub fn block_ambiguous(&self) {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.failed = true;
        inner.ambiguous = true;
        Self::advance_failure(&mut inner);
        self.changed(&mut inner);
    }

    /// Capture immediately before the final empty-snapshot proof scan. A later
    /// additive dispatch advances the generation and invalidates this candidate.
    pub fn reconciliation_candidate(&self) -> Option<ReconciliationCandidate> {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (!inner.retired
            && !inner.permanent_hold
            && !inner.ambiguous
            && !inner.active.values().any(|write| write.additive))
        .then_some(ReconciliationCandidate(AccountingWitness {
            writes: inner.accounting_generation,
            data: inner.accounting_data_generation,
        }))
    }

    /// Publish a proven drain of every retained additive snapshot. Unknown
    /// settlement is not a drain and can never be downgraded by a later scan.
    pub fn mark_accounting_reconciled(&self, candidate: ReconciliationCandidate) -> bool {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.retired
            || inner.permanent_hold
            || inner.ambiguous
            || inner.accounting_generation != candidate.0.writes
            || inner.accounting_data_generation != candidate.0.data
            || inner.active.values().any(|write| write.additive)
        {
            return false;
        }
        inner.failed_additive = false;
        inner.accounting_reconciled_generation = inner.accounting_generation;
        inner.accounting_data_reconciled_generation = inner.accounting_data_generation;
        inner.accounting_data_settlement_generation = None;
        self.changed(&mut inner);
        true
    }

    /// The current mutation epoch, or none after the checked sequence is exhausted.
    pub fn mutation_epoch(self: &Arc<Self>) -> Option<MutationEpoch> {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (!inner.retired && !inner.mutation_exhausted && !inner.permanent_hold).then(|| {
            MutationEpoch {
                owner: Arc::downgrade(self),
                sequence: inner.mutation_sequence,
            }
        })
    }

    /// Capture a queued mutation's baseline only when no older mutation intent
    /// already owns this State. A later intent advances the sequence and revokes it.
    pub fn mutation_epoch_without_active_mutation(self: &Arc<Self>) -> Option<MutationEpoch> {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (!inner.retired
            && !inner.mutation_exhausted
            && !inner.permanent_hold
            && !inner.active.values().any(|write| write.mutation))
        .then(|| MutationEpoch {
            owner: Arc::downgrade(self),
            sequence: inner.mutation_sequence,
        })
    }

    /// Whether `epoch` is the exact current boundary of this owner.
    pub fn mutation_epoch_is_current(self: &Arc<Self>, epoch: &MutationEpoch) -> bool {
        let owner = Arc::downgrade(self);
        if !owner.ptr_eq(&epoch.owner) {
            return false;
        }
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        !inner.retired
            && !inner.mutation_exhausted
            && !inner.permanent_hold
            && inner.mutation_sequence == epoch.sequence
    }

    /// Whether an acknowledged mutation still owns the exact clean accounting
    /// boundary at which it committed.
    pub fn mutation_effect_is_current(self: &Arc<Self>, effect: &MutationEffect) -> bool {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::mutation_effect_matches(self, &inner, effect)
    }

    /// Whether only later, still-unreconciled callback data invalidated this
    /// effect. This grants no destructive mutation authority.
    pub fn mutation_effect_has_only_pending_data(
        self: &Arc<Self>,
        effect: &MutationEffect,
    ) -> bool {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::mutation_effect_pending_data_only_matches(self, &inner, effect)
    }

    /// Revalidate only the mutation owner/sequence while a caller holds the
    /// native goal permit. Accounting changes performed by this same effect may
    /// legitimately advance its exact data witness before continuation launch.
    pub fn mutation_effect_epoch_authorized(self: &Arc<Self>) -> bool {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let authorized = MUTATION_EFFECT
            .try_with(|effect| {
                std::ptr::eq(effect.epoch.owner.as_ptr(), self.as_ref())
                    && effect.epoch.sequence == inner.mutation_sequence
                    && !inner.retired
                    && !inner.mutation_exhausted
                    && !inner.permanent_hold
            })
            .unwrap_or(true);
        if !authorized {
            let _ = MUTATION_EFFECT_REJECTED.try_with(|rejected| rejected.set(true));
        }
        authorized
    }

    /// Cancellation of a committed but unapplied effect is fail-closed only if
    /// no newer mutation has superseded its durable row.
    pub fn hold_if_mutation_effect_epoch_current(
        self: &Arc<Self>,
        effect: &MutationEffect,
    ) -> bool {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !std::ptr::eq(effect.epoch.owner.as_ptr(), self.as_ref())
            || effect.epoch.sequence != inner.mutation_sequence
            || inner.retired
            || inner.mutation_exhausted
            || inner.permanent_hold
        {
            return false;
        }
        Self::permanently_hold(&mut inner);
        self.changed(&mut inner);
        true
    }

    /// Whether `epoch` is this owner's never-mutated initial boundary.
    pub fn mutation_epoch_is_pristine(self: &Arc<Self>, epoch: &MutationEpoch) -> bool {
        let owner = Arc::downgrade(self);
        if !owner.ptr_eq(&epoch.owner) || epoch.sequence != 0 {
            return false;
        }
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        !inner.retired
            && !inner.mutation_exhausted
            && !inner.permanent_hold
            && inner.mutation_sequence == 0
    }

    pub fn admission(&self) -> Admission {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::classify_admission(&inner)
    }

    pub fn permits_admission(&self) -> bool {
        self.admission() == Admission::Ready
    }

    pub fn permits_destructive_accounting_change(&self) -> bool {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::unscoped_change_matches(&inner)
    }

    /// A committed mutation retained only callback data that arrived after its
    /// dispatch. Final turn accounting may persist that exact same-goal input;
    /// failed, ambiguous, owned, or clock-bearing boundaries remain excluded.
    pub fn has_only_pending_accounting_data(&self) -> bool {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::accounting_witness_has_only_pending_data(
            &inner,
            AccountingWitness {
                writes: inner.accounting_reconciled_generation,
                data: inner.accounting_data_reconciled_generation,
            },
        )
    }

    /// An ambiguous commit cannot be replayed as another additive usage update.
    pub fn permits_write(&self) -> bool {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        !inner.ambiguous && !inner.permanent_hold && !inner.retired
    }

    /// A committed process handoff may reconcile healthy callback data which
    /// arrived after preparation, but it must not cross a failed, ambiguous or
    /// still-owned persistence operation. Ordinary admission remains stricter:
    /// this exception belongs only to the exact committed final-capture token.
    pub fn permits_final_reconciliation(&self) -> bool {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        !inner.retired
            && !inner.failed
            && !inner.failed_additive
            && !inner.ambiguous
            && !inner.permanent_hold
            && !inner.mutation_exhausted
            && !inner.settled_mutation_unknown
            && inner.active.is_empty()
    }

    /// A storage future returned without proving commit or rollback. Additive
    /// replay and automatic admission stay held, but one exact serialized
    /// explicit mutation may supersede the now-settled command.
    pub fn settled_mutation_unknown(&self) -> bool {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.settled_mutation_unknown
            && !inner.permanent_hold
            && !inner.retired
            && !inner.accounting_clock_open
            && inner.accounting_generation == inner.accounting_reconciled_generation
            && inner.accounting_data_generation == inner.accounting_data_reconciled_generation
            && inner.active.is_empty()
    }

    /// Permanently close this persistence owner before runtime retirement waits
    /// for its mutation and accounting lanes. Existing writes may settle; no
    /// queued or later write can regain admission or a mutation epoch.
    pub fn retire(&self) {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.retired {
            return;
        }
        inner.retired = true;
        self.changed(&mut inner);
    }

    /// Read only after both native persistence lanes drain. The replacement
    /// receives restrictions and evidence, never this owner's epoch identity.
    pub fn retirement_carry(&self) -> PersistenceCarry {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let permanent = inner.permanent_hold
            || inner.mutation_exhausted
            || inner.failed_additive
            || inner.accounting_clock_open
            || inner.accounting_data_generation != inner.accounting_data_reconciled_generation
            || inner
                .active
                .values()
                .any(|write| write.additive || write.mutation)
            || !inner.quarantined.is_empty();
        let hold = if permanent {
            HoldClass::Permanent
        } else if inner.ambiguous {
            HoldClass::Ambiguous
        } else if inner.failed {
            HoldClass::Failed
        } else {
            HoldClass::Ready
        };
        PersistenceCarry {
            non_pristine: inner.mutation_sequence != 0 || hold != HoldClass::Ready,
            hold,
            mutation_exhausted: inner.mutation_exhausted,
            settled_mutation_unknown: inner.settled_mutation_unknown
                && hold == HoldClass::Ambiguous,
            quarantined: inner.quarantined.clone(),
        }
    }

    /// Seed a fresh replacement before it is visible in the runtime registry.
    pub fn seed_carry(&self, carry: &PersistenceCarry) {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if carry.non_pristine && inner.mutation_sequence == 0 {
            inner.mutation_sequence = 1;
        }
        match carry.hold {
            HoldClass::Ready => {}
            HoldClass::Failed => inner.failed = true,
            HoldClass::Ambiguous => {
                inner.failed = true;
                inner.ambiguous = true;
                inner.settled_mutation_unknown = carry.settled_mutation_unknown;
            }
            HoldClass::Permanent => {
                inner.failed = true;
                inner.ambiguous = true;
                inner.permanent_hold = true;
            }
        }
        inner.mutation_exhausted |= carry.mutation_exhausted;
        for (goal, usage) in &carry.quarantined {
            inner
                .quarantined
                .entry(goal.clone())
                .and_modify(|current| {
                    current.elapsed_seconds = current.elapsed_seconds.max(usage.elapsed_seconds);
                    current.descendant_tokens =
                        current.descendant_tokens.max(usage.descendant_tokens);
                    current.turn_tokens = current.turn_tokens.max(usage.turn_tokens);
                })
                .or_insert_with(|| usage.clone());
        }
        self.changed(&mut inner);
    }

    pub fn begin(self: &Arc<Self>) -> Write {
        self.begin_kind(false)
    }

    /// Revoke old stop custody before the external write can reach SQLite.
    pub fn begin_mutation(self: &Arc<Self>) -> Write {
        self.begin_kind(true)
    }

    fn begin_kind(self: &Arc<Self>, mutation: bool) -> Write {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let predecessor_settled_mutation_unknown = inner.settled_mutation_unknown
            && !inner.permanent_hold
            && !inner.retired
            && !inner.accounting_clock_open
            && inner.accounting_generation == inner.accounting_reconciled_generation
            && inner.accounting_data_generation == inner.accounting_data_reconciled_generation
            && inner.active.is_empty();
        let overlaps_mutation = mutation && inner.active.values().any(|write| write.mutation);
        let before = mutation
            .then(|| {
                (!overlaps_mutation
                    && !inner.retired
                    && !inner.mutation_exhausted
                    && !inner.permanent_hold)
                    .then(|| MutationEpoch {
                        owner: Arc::downgrade(self),
                        sequence: inner.mutation_sequence,
                    })
            })
            .flatten();
        let accounting_witness = mutation
            .then_some(AccountingWitness {
                writes: inner.accounting_generation,
                data: inner.accounting_data_generation,
            })
            .filter(|_| !overlaps_mutation)
            .filter(|witness| {
                witness.writes == inner.accounting_reconciled_generation
                    && witness.data == inner.accounting_data_reconciled_generation
                    && !inner.failed_additive
                    && !inner.accounting_clock_open
                    && !inner.active.values().any(|write| write.additive)
            });
        let failure_witness = (mutation && !overlaps_mutation)
            .then_some(inner.failure_generation)
            .filter(|_| !inner.retired && !inner.mutation_exhausted && !inner.permanent_hold);
        let predecessor_authoritative = !inner.ambiguous;
        let sequence = if mutation {
            if overlaps_mutation
                || inner.retired
                || inner.mutation_exhausted
                || inner.permanent_hold
            {
                None
            } else {
                match inner.mutation_sequence.checked_add(1) {
                    Some(sequence) => {
                        inner.mutation_sequence = sequence;
                        Some(sequence)
                    }
                    None => {
                        // No sequence may repeat. Fail closed permanently rather than
                        // manufacture an ABA-equivalent epoch for a write that may commit.
                        inner.mutation_exhausted = true;
                        inner.failed = true;
                        inner.ambiguous = true;
                        None
                    }
                }
            }
        } else if inner.retired || inner.mutation_exhausted || inner.permanent_hold {
            None
        } else {
            Some(inner.mutation_sequence)
        };
        let id = sequence.and_then(|sequence| match inner.next.checked_add(1) {
            Some(id) => {
                inner.next = id;
                inner.active.insert(
                    id,
                    ActiveWrite {
                        sequence,
                        additive: false,
                        mutation,
                        data_snapshot: None,
                    },
                );
                Some(id)
            }
            None => {
                inner.failed = true;
                inner.ambiguous = true;
                inner.permanent_hold = true;
                None
            }
        });
        let admitted = id.is_some() && sequence.is_some();
        self.changed(&mut inner);
        Write {
            state: Arc::clone(self),
            id,
            epoch: id.zip(sequence).map(|(_, sequence)| MutationEpoch {
                owner: Arc::downgrade(self),
                sequence,
            }),
            before,
            finished: false,
            mutation,
            dispatched: false,
            admitted,
            accounting_witness,
            failure_witness,
            predecessor_authoritative,
            predecessor_settled_mutation_unknown,
        }
    }
}

pub struct Write {
    state: Arc<State>,
    id: Option<u128>,
    epoch: Option<MutationEpoch>,
    before: Option<MutationEpoch>,
    finished: bool,
    mutation: bool,
    dispatched: bool,
    admitted: bool,
    accounting_witness: Option<AccountingWitness>,
    failure_witness: Option<u128>,
    predecessor_authoritative: bool,
    predecessor_settled_mutation_unknown: bool,
}

impl Write {
    pub fn predecessor_is_authoritative(&self) -> bool {
        self.predecessor_authoritative
    }

    /// Whether this write was created from the exact recoverable unknown
    /// settlement boundary, before this write itself entered the active set.
    pub fn predecessor_was_settled_mutation_unknown(&self) -> bool {
        self.predecessor_settled_mutation_unknown
    }

    pub fn was_dispatched(&self) -> bool {
        self.dispatched
    }

    /// Withdraw a mutation intent before any storage command owns it. Beginning
    /// the intent already advanced chronology; withdrawal changes no failure or
    /// reconciliation state.
    pub fn cancel_before_dispatch(&mut self) {
        if self.finished {
            return;
        }
        assert!(self.mutation && !self.dispatched);
        let mut inner = self
            .state
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(id) = self.id {
            inner.active.remove(&id);
        }
        self.state.changed(&mut inner);
        self.finished = true;
    }

    fn mark_accounting(&mut self) {
        self.dispatched = true;
        let mut inner = self
            .state
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.admitted {
            return;
        }
        if let Some(write) = self.id.and_then(|id| inner.active.get_mut(&id)) {
            write.additive = true;
            write.data_snapshot = None;
        }
        match inner.accounting_generation.checked_add(1) {
            Some(generation) if self.admitted => inner.accounting_generation = generation,
            _ => {
                State::advance_failure(&mut inner);
                inner.failed = true;
                inner.ambiguous = true;
                inner.permanent_hold = true;
            }
        }
        self.state.changed(&mut inner);
    }

    /// The epoch observed by this write, if the owner's sequence remains representable.
    pub fn mutation_epoch(&self) -> Option<MutationEpoch> {
        self.epoch.clone()
    }

    /// Propagate an unrepresentable external owner fence into this live owner.
    pub fn hold_permanently(&mut self) {
        self.state.hold_permanently();
    }

    pub fn permanently_closed(&self) -> bool {
        let inner = self
            .state
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.retired || inner.permanent_hold || inner.mutation_exhausted
    }

    /// Atomically authorize SQLite dispatch against the exact live owner.
    /// Rejection before SQL is safe and leaves permanent classification to the
    /// already-published owner state.
    pub fn mark_dispatched(&mut self) -> bool {
        assert!(
            self.mutation,
            "only mutation writes dispatch through this path"
        );
        let inner = self
            .state
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = self.admitted
            && self.id.is_some_and(|id| {
                inner.active.get(&id).is_some_and(|write| {
                    write.mutation && write.sequence == inner.mutation_sequence
                })
            })
            && !inner
                .active
                .iter()
                .any(|(id, write)| Some(*id) != self.id && (write.mutation || write.additive))
            && !inner.retired
            && !inner.mutation_exhausted
            && !inner.permanent_hold
            && self.failure_witness == Some(inner.failure_generation)
            && self.accounting_witness.is_some_and(|witness| {
                witness.writes == inner.accounting_generation
                    && witness.writes == inner.accounting_reconciled_generation
                    && witness.data == inner.accounting_data_generation
                    && witness.data == inner.accounting_data_reconciled_generation
                    && !inner.failed_additive
                    && !inner.accounting_clock_open
            });
        if !current {
            return false;
        }
        self.dispatched = true;
        true
    }

    fn complete(
        &mut self,
        success: bool,
        safe: bool,
        abandoned: bool,
    ) -> Option<MutationTransition> {
        let mut inner = self
            .state
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut transition = None;
        let settled = self.id.and_then(|id| inner.active.remove(&id));
        let additive = settled.as_ref().is_some_and(|write| write.additive);
        if !self.admitted {
            self.state.changed(&mut inner);
            self.finished = true;
            return None;
        }
        let accounting_current = self.accounting_witness.is_some_and(|witness| {
            witness.writes == inner.accounting_generation
                && witness.writes == inner.accounting_reconciled_generation
                && witness.data == inner.accounting_data_generation
                && witness.data == inner.accounting_data_reconciled_generation
                && !inner.failed_additive
                && !inner.accounting_clock_open
                && !inner.active.values().any(|write| write.additive)
        });
        let accounting_pending_data_only = self.dispatched
            && self.accounting_witness.is_some_and(|witness| {
                State::accounting_witness_has_only_pending_data_at_completion(&inner, witness)
            });
        let effect_accounting = (accounting_current || accounting_pending_data_only)
            .then_some(self.accounting_witness)
            .flatten();
        let failure_current = self.failure_witness == Some(inner.failure_generation);
        let owns_sequence = settled.as_ref().is_some_and(|write| {
            write.sequence == inner.mutation_sequence
                && !inner.mutation_exhausted
                && !inner.permanent_hold
                && (!self.mutation
                    || !inner
                        .active
                        .values()
                        .any(|active| active.mutation || active.additive))
        });
        if abandoned {
            // The Rust future ended while SQLite may still own the dispatched
            // command. No later commit can prove that command's settlement order.
            if self.mutation {
                State::invalidate_mutation_sequence(&mut inner);
            }
            State::advance_failure(&mut inner);
            inner.failed = true;
            inner.failed_additive |= additive;
            inner.ambiguous = true;
            inner.permanent_hold = true;
        } else if owns_sequence {
            if self.mutation {
                // Also revoke stops queued while this intent held the native
                // permit. Cancellation after COMMIT must fence them as well.
                match inner.mutation_sequence.checked_add(1) {
                    Some(sequence) => {
                        inner.mutation_sequence = sequence;
                        if success
                            && failure_current
                            && !inner.retired
                            && let Some(accounting) = effect_accounting
                        {
                            transition = self.before.clone().map(|before| {
                                let after = MutationEpoch {
                                    owner: Arc::downgrade(&self.state),
                                    sequence,
                                };
                                MutationTransition {
                                    before,
                                    after: after.clone(),
                                    effect: MutationEffect {
                                        epoch: after,
                                        accounting,
                                        failure_generation: inner.failure_generation,
                                    },
                                }
                            });
                        }
                    }
                    None => {
                        // The SQL outcome can no longer receive a unique witness.
                        // Permanently hold admission and invalidate every old token.
                        inner.mutation_exhausted = true;
                        State::advance_failure(&mut inner);
                        inner.failed = true;
                        inner.ambiguous = true;
                    }
                }
            }
            if !success {
                State::advance_failure(&mut inner);
                inner.failed = true;
                inner.failed_additive |= additive;
                inner.ambiguous |= !safe;
                if self.mutation && !safe {
                    inner.settled_mutation_unknown = accounting_current && failure_current;
                }
            } else if self.mutation
                && !inner.mutation_exhausted
                && !inner.permanent_hold
                && inner.quarantined.is_empty()
                && (accounting_current || accounting_pending_data_only)
                && failure_current
            {
                inner.failed = false;
                inner.failed_additive = false;
                inner.ambiguous = false;
                inner.settled_mutation_unknown = false;
            } else if success
                && self.mutation
                && (!(accounting_current || accounting_pending_data_only) || !failure_current)
            {
                State::advance_failure(&mut inner);
                inner.failed = true;
                inner.ambiguous = true;
            }
        } else if self.mutation {
            // Two mutation intents must be serialized by the native permit. If
            // they overlap anyway, their SQL settlement order is unknowable:
            // invalidate every transition either could have exposed and hold
            // until a later, properly serialized explicit mutation reconciles it.
            State::invalidate_mutation_sequence(&mut inner);
            State::advance_failure(&mut inner);
            inner.failed = true;
            inner.ambiguous = true;
        } else if settled.is_some() && !success {
            // Retirement and a newer mutation may make an accounting write
            // stale, but they cannot erase its known failed settlement.
            State::advance_failure(&mut inner);
            inner.failed = true;
            inner.failed_additive |= additive;
            inner.ambiguous |= !safe;
        }
        if additive
            && success
            && !inner.failed_additive
            && !inner.active.values().any(|write| write.additive)
        {
            inner.accounting_reconciled_generation = inner.accounting_generation;
            if inner.accounting_data_settlement_generation == Some(inner.accounting_data_generation)
            {
                inner.accounting_data_reconciled_generation = inner.accounting_data_generation;
                inner.accounting_data_settlement_generation = None;
            }
        }
        self.state.changed(&mut inner);
        self.finished = true;
        transition
    }

    pub fn commit_mutation(mut self) -> Option<MutationTransition> {
        assert!(
            self.mutation,
            "only a native mutation intent can acknowledge its commit"
        );
        self.complete(true, true, false)
    }

    /// A failed stop remains a hold even if a later accounting write succeeds.
    pub fn finish(mut self, success: bool) {
        let _ = self.complete(success, true, false);
    }

    /// Settle a dispatched mutation whose typed storage error proves that no
    /// commit occurred. Cancellation and unknown errors must still use Drop.
    pub fn definitely_rolled_back(&mut self) {
        if !self.finished {
            let _ = self.complete(false, true, false);
        }
    }

    /// Settle a dispatched mutation after its storage future returned an error
    /// that does not prove either commit or rollback. The command is no longer
    /// running, so a later serialized mutation may reconcile this ambiguity;
    /// only cancellation while storage still owns the command is permanent.
    pub fn settled_unknown(&mut self) {
        if !self.finished {
            let _ = self.complete(false, false, false);
        }
    }
}

impl Drop for Write {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.complete(false, !self.dispatched, self.dispatched);
        }
    }
}

/// Start ownership before dispatch; dropping the receiver never cancels the write.
pub async fn owned<T, E, F>(
    state: Arc<State>,
    work: F,
) -> Result<Result<T, E>, oneshot::error::RecvError>
where
    T: Send + 'static,
    E: Send + 'static,
    F: Future<Output = Result<T, E>> + Send + 'static,
{
    let mut write = state.begin();
    write.mark_accounting();
    let owner = write.id;
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let safe = Arc::new(AtomicBool::new(false));
        let result = ACCOUNTING_OWNER
            .scope(owner, ROLLED_BACK.scope(Arc::clone(&safe), work))
            .await;
        let _ = write.complete(result.is_ok(), safe.load(Ordering::SeqCst), false);
        let _ = sender.send(result);
    });
    receiver.await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn goal_runtime_status_wakes_for_settlement_and_distinguishes_sticky_failure_from_live_work()
     {
        let state = Arc::new(State::default());
        let mut changes = state.changes();
        let failed = state.begin();
        let in_flight = state.begin();
        failed.finish(false);
        changes.changed().await.unwrap();
        let active = state.snapshot();
        assert_eq!(active.admission, Admission::Held);
        assert!(active.writes_pending);
        changes.borrow_and_update();
        in_flight.finish(true);
        changes.changed().await.unwrap();
        let held = state.snapshot();
        assert_eq!(held.admission, Admission::Held);
        assert!(!held.writes_pending);
        assert!(held.revision > active.revision);
        changes.borrow_and_update();
        let _ = state.begin_mutation().commit_mutation();
        changes.changed().await.unwrap();
        let ready = state.snapshot();
        assert_eq!(ready.admission, Admission::Ready);
        assert!(!ready.writes_pending);
        assert!(ready.revision > held.revision);
    }

    #[tokio::test]
    async fn observer_cancellation_cannot_cancel_commit_bookkeeping() {
        let state = Arc::new(State::default());
        let (release, wait) = oneshot::channel();
        let (committed, commit) = oneshot::channel();
        let observer = tokio::spawn(owned(Arc::clone(&state), async move {
            wait.await.unwrap();
            committed.send(()).unwrap();
            Ok::<_, ()>(())
        }));
        tokio::task::yield_now().await;
        assert!(!state.permits_admission());
        assert!(state.accounting_write_pending());
        observer.abort();
        let _ = observer.await;
        release.send(()).unwrap();
        commit.await.unwrap();
        tokio::task::yield_now().await;
        assert!(state.permits_admission());
        assert!(!state.accounting_write_pending());
    }

    #[tokio::test]
    async fn captured_healthy_write_stays_recoverable_until_its_settlement() {
        let state = Arc::new(State::default());
        let mut pending = state.begin();
        pending.mark_accounting();
        let owner = pending.id;

        ACCOUNTING_OWNER
            .scope(owner, async {
                let mut captured = state.begin_accounting_data_change();
                assert!(captured.publish_frozen_additive_snapshot());
                drop(captured);
                assert_eq!(state.admission(), Admission::Writing);
                assert_eq!(state.snapshot().admission, Admission::Writing);

                let mut settlement = state.begin_accounting_data_change();
                settlement.publish_additive_settlement_and_reopen();
            })
            .await;
        pending.finish(true);

        assert_eq!(state.admission(), Admission::Ready);
        assert_eq!(state.snapshot().admission, Admission::Ready);
    }

    #[test]
    fn final_reconciliation_accepts_only_healthy_ownerless_callback_data() {
        let state = Arc::new(State::default());
        assert!(state.permits_final_reconciliation());

        let mut callback = state.begin_accounting_data_change();
        callback.publish_change();
        drop(callback);
        assert_eq!(state.admission(), Admission::Held);
        assert!(state.permits_final_reconciliation());

        let write = state.begin();
        assert!(!state.permits_final_reconciliation());
        write.finish(true);
        assert!(state.permits_final_reconciliation());

        state.begin().finish(false);
        assert!(!state.permits_final_reconciliation());
    }

    #[tokio::test]
    async fn failed_write_with_successful_reads_never_admits_again() {
        let state = Arc::new(State::default());
        assert!(
            owned(Arc::clone(&state), async {
                definitely_rolled_back();
                Err::<(), _>("injected SQLITE_FULL")
            })
            .await
            .unwrap()
            .is_err()
        );
        for _ in 0..100 {
            let durable_status = "active"; // Reads succeeding cannot acknowledge a write.
            assert_eq!(durable_status, "active");
            assert!(!state.permits_admission());
        }
        assert!(state.permits_write());
        assert!(
            owned(Arc::clone(&state), async { Ok::<_, ()>(()) })
                .await
                .unwrap()
                .is_ok()
        );
        assert!(!state.permits_admission());
        let generation = state.reconciliation_candidate().unwrap();
        assert!(state.mark_accounting_reconciled(generation));
        let _ = state.begin_mutation().commit_mutation();
        assert!(state.permits_admission());
    }

    #[tokio::test]
    async fn ambiguous_commit_is_held_without_replay() {
        let state = Arc::new(State::default());
        let _ = owned(Arc::clone(&state), async {
            Err::<(), _>("injected commit I/O")
        })
        .await;
        assert!(!state.permits_write());
        assert!(!state.permits_admission());
    }

    #[test]
    fn successful_accounting_does_not_erase_failed_stop() {
        let state = Arc::new(State::default());
        state.begin().finish(false);
        state.begin().finish(true);
        assert!(!state.permits_admission());
    }

    #[test]
    fn old_completion_cannot_overwrite_new_mutation_or_stop() {
        let state = Arc::new(State::default());
        let old = state.begin();
        let _ = state.begin_mutation().commit_mutation(); // A fresh explicit mutation also supersedes same-ID resumes.
        let new = state.begin();
        old.finish(false);
        assert!(!state.permits_admission()); // New operation still owns admission.
        new.finish(false);
        assert!(!state.permits_admission());
    }
    #[test]
    fn cancelled_dispatched_mutation_permanently_fences_later_work() {
        let state = Arc::new(State::default());
        let mut mutation = state.begin_mutation();
        mutation.mark_dispatched();
        let queued_stop = state.begin();
        let queued_epoch = queued_stop.mutation_epoch().unwrap();
        assert!(state.mutation_epoch_is_current(&queued_epoch));
        drop(mutation); // Includes lost acknowledgement after a native COMMIT.
        assert!(!state.mutation_epoch_is_current(&queued_epoch));
        queued_stop.finish(true);
        assert!(!state.permits_admission());
        assert!(state.begin_mutation().commit_mutation().is_none());
        assert_eq!(state.retirement_carry().hold_class(), HoldClass::Permanent);
    }

    #[test]
    fn committed_mutation_returns_its_exact_owner_bound_transition() {
        let state = Arc::new(State::default());
        let other = Arc::new(State::default());
        let before = state.mutation_epoch().unwrap();
        assert!(state.mutation_epoch_is_pristine(&before));
        assert!(!other.mutation_epoch_is_current(&before));

        let mutation = state.begin_mutation();
        let intent = mutation.mutation_epoch().unwrap();
        assert!(!state.mutation_epoch_is_current(&before));
        assert!(state.mutation_epoch_is_current(&intent));
        let transition = mutation.commit_mutation().unwrap();

        assert_eq!(transition.before, before);
        assert!(state.mutation_epoch_is_current(&transition.after));
        assert_ne!(transition.after, intent);
    }

    #[test]
    fn frozen_clock_publication_and_dispatched_mutation_share_one_state_guard() {
        let state = Arc::new(State::default());
        let mut opened = state.begin_accounting_data_change();
        opened.publish_fresh_clock();
        drop(opened);

        let mut boundary = state.begin_accounting_data_change();
        assert!(boundary.can_prepare_dispatched_mutation());
        boundary.publish_frozen_clock();
        let write = boundary
            .begin_dispatched_mutation(&state)
            .expect("the exact frozen boundary should dispatch");
        drop(boundary);
        assert!(write.commit_mutation().is_some());

        let raced = Arc::new(State::default());
        let mut opened = raced.begin_accounting_data_change();
        opened.publish_fresh_clock();
        drop(opened);
        let mut boundary = raced.begin_accounting_data_change();
        boundary.publish_frozen_clock();
        let write = boundary
            .begin_dispatched_mutation(&raced)
            .expect("the callback starts only after dispatch is visible");
        drop(boundary);
        let mut callback = raced.begin_accounting_data_change();
        callback.publish_change();
        drop(callback);
        let transition = write
            .commit_mutation()
            .expect("a callback after dispatch preserves the committed mutation");
        assert!(!raced.mutation_effect_is_current(&transition.effect));
        assert!(raced.mutation_effect_has_only_pending_data(&transition.effect));
        assert_eq!(raced.admission(), Admission::Held);
        assert!(raced.permits_write());
    }

    #[test]
    fn dirty_pre_dispatch_boundary_is_rejected_without_advancing_its_epoch() {
        let state = Arc::new(State::default());
        let epoch = state.mutation_epoch().unwrap();
        let mut callback = state.begin_accounting_data_change();
        callback.publish_change();
        drop(callback);

        let mut boundary = state.begin_accounting_data_change();
        assert!(!boundary.can_prepare_dispatched_mutation());
        assert!(boundary.begin_dispatched_mutation(&state).is_none());
        drop(boundary);
        assert!(state.mutation_epoch_is_current(&epoch));
    }

    #[test]
    fn accounting_settlement_leaves_the_mutation_epoch_stable() {
        let state = Arc::new(State::default());
        let before = state.mutation_epoch().unwrap();
        state.begin().finish(true);
        assert_eq!(state.mutation_epoch(), Some(before.clone()));
        state.begin().finish(false);
        assert_eq!(state.mutation_epoch(), Some(before));
    }

    #[test]
    fn failed_and_overlapping_mutations_produce_no_false_commit_witness() {
        let state = Arc::new(State::default());
        let failed = state.begin_mutation();
        let failed_intent = failed.mutation_epoch().unwrap();
        failed.finish(false);
        assert!(!state.mutation_epoch_is_current(&failed_intent));
        assert_eq!(state.admission(), Admission::Held);

        let serialized = state.begin_mutation();
        let rejected = state.begin_mutation();
        assert!(rejected.mutation_epoch().is_none());
        assert!(rejected.commit_mutation().is_none());
        assert!(serialized.commit_mutation().is_some());
        assert_eq!(state.admission(), Admission::Ready);
    }

    #[test]
    fn later_non_additive_failure_invalidates_an_older_mutation() {
        let state = Arc::new(State::default());
        let mutation = state.begin_mutation();
        state.begin().finish(false);
        assert!(mutation.commit_mutation().is_none());
        assert_eq!(state.admission(), Admission::Held);
    }

    #[test]
    fn harmless_validation_can_settle_before_or_after_a_real_mutation() {
        for validation_first in [true, false] {
            let state = Arc::new(State::default());
            let mut mutation = state.begin_mutation();
            let validation = state.begin();
            assert!(mutation.mark_dispatched());
            if validation_first {
                validation.finish(true);
                assert!(mutation.commit_mutation().is_some());
            } else {
                assert!(mutation.commit_mutation().is_some());
                validation.finish(true);
            }
            assert_eq!(state.admission(), Admission::Ready);
        }
    }

    #[test]
    fn reconciliation_rejects_unknown_or_newer_accounting_settlement() {
        let unknown = Arc::new(State::default());
        let candidate = unknown.reconciliation_candidate().unwrap();
        let mut write = unknown.begin();
        write.mark_accounting();
        unknown.block_ambiguous();
        let _ = write.complete(false, false, false);
        assert!(!unknown.mark_accounting_reconciled(candidate));
        assert!(unknown.begin_mutation().commit_mutation().is_none());

        let raced = Arc::new(State::default());
        let candidate = raced.reconciliation_candidate().unwrap();
        let mut late = raced.begin();
        late.mark_accounting();
        let _ = late.complete(false, true, false);
        assert!(!raced.mark_accounting_reconciled(candidate));
        assert!(raced.begin_mutation().commit_mutation().is_none());
    }

    #[test]
    fn snapshot_data_change_revokes_reconciliation_and_prepared_mutation() {
        let state = Arc::new(State::default());
        let candidate = state.reconciliation_candidate().unwrap();
        let mut change = state.begin_accounting_data_change();
        change.publish_change();
        drop(change);
        assert!(!state.mark_accounting_reconciled(candidate));
        assert!(state.begin_mutation().commit_mutation().is_none());

        let state = Arc::new(State::default());
        let mutation = state.begin_mutation();
        let mut change = state.begin_accounting_data_change();
        change.publish_change();
        drop(change);
        assert!(mutation.commit_mutation().is_none());
        assert_eq!(state.admission(), Admission::Held);
    }

    #[tokio::test]
    async fn additive_settlement_reconciles_only_its_exact_snapshot_generation() {
        for callback_before_settlement in [false, true] {
            let state = Arc::new(State::default());
            let mut write = state.begin();
            write.mark_accounting();
            ACCOUNTING_OWNER
                .scope(write.id, async {
                    let mut snapshot = state.begin_accounting_data_change();
                    snapshot.capture_additive_snapshot();
                    drop(snapshot);

                    if callback_before_settlement {
                        let mut callback = state.begin_accounting_data_change();
                        callback.publish_change();
                        drop(callback);
                        ACCOUNTING_OWNER
                            .scope(None, async {
                                let mut unrelated_probe = state.begin_accounting_data_change();
                                unrelated_probe.capture_additive_snapshot();
                            })
                            .await;
                    }
                    let mut settlement = state.begin_accounting_data_change();
                    settlement.publish_additive_settlement(false);
                    drop(settlement);
                    if !callback_before_settlement {
                        let mut callback = state.begin_accounting_data_change();
                        callback.publish_change();
                    }
                })
                .await;

            let _ = write.complete(true, true, false);
            assert_eq!(state.admission(), Admission::Held);
        }

        let state = Arc::new(State::default());
        let candidate = state.reconciliation_candidate().unwrap();
        drop(state.begin_accounting_data_change());
        assert!(state.mark_accounting_reconciled(candidate));
        assert_eq!(state.admission(), Admission::Ready);
    }

    #[tokio::test]
    async fn last_queued_noop_additive_inherits_an_exact_earlier_settlement() {
        for callback_after_settlement in [false, true] {
            let state = Arc::new(State::default());
            let mut first = state.begin();
            first.mark_accounting();
            let mut queued = state.begin();
            queued.mark_accounting();

            ACCOUNTING_OWNER
                .scope(first.id, async {
                    let mut snapshot = state.begin_accounting_data_change();
                    assert!(snapshot.capture_additive_snapshot());
                    drop(snapshot);
                    let mut settlement = state.begin_accounting_data_change();
                    settlement.publish_additive_settlement(false);
                })
                .await;
            let _ = first.complete(true, true, false);
            // The queued additive owner still has a recoverable settlement;
            // only an ownerless unreconciled snapshot is Held.
            assert_eq!(state.admission(), Admission::Writing);

            if callback_after_settlement {
                let mut callback = state.begin_accounting_data_change();
                callback.publish_change();
            }
            let _ = queued.complete(true, true, false);
            assert_eq!(
                state.admission(),
                if callback_after_settlement {
                    Admission::Held
                } else {
                    Admission::Ready
                }
            );
        }
    }

    #[tokio::test]
    async fn delayed_runtime_effect_requires_its_exact_clean_post_commit_epoch() {
        let state = Arc::new(State::default());
        let transition = state.begin_mutation().commit_mutation().unwrap();
        let (authorized, rejected, applied) = mutation_effect(transition.effect.clone(), async {
            let authorized = state
                .begin_accounting_data_change()
                .mutation_effect_authorized();
            if authorized {
                mutation_effect_applied();
            }
            authorized
        })
        .await;
        assert!(authorized);
        assert!(!rejected);
        assert!(applied);

        let mut callback = state.begin_accounting_data_change();
        callback.publish_change();
        drop(callback);
        assert!(!state.mutation_effect_is_current(&transition.effect));
        assert!(state.mutation_effect_has_only_pending_data(&transition.effect));
        let (authorized, rejected, _) = mutation_effect(transition.effect, async {
            state
                .begin_accounting_data_change()
                .mutation_effect_authorized()
        })
        .await;
        assert!(!authorized);
        assert!(rejected);
    }

    #[tokio::test]
    async fn applied_runtime_effect_acknowledgement_survives_future_cancellation() {
        let state = Arc::new(State::default());
        let effect = state.begin_mutation().commit_mutation().unwrap().effect;
        let acknowledgement = MutationEffectAcknowledgement::default();
        let task_acknowledgement = acknowledgement.clone();
        let (applied, observed) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            mutation_effect_with_acknowledgement(effect, task_acknowledgement, async move {
                mutation_effect_applied();
                let _ = applied.send(());
                std::future::pending::<()>().await;
            })
            .await
        });
        observed.await.unwrap();
        task.abort();
        let _ = task.await;
        assert!(acknowledgement.is_applied());
        assert!(state.permits_admission());
    }

    #[tokio::test]
    async fn clean_or_active_later_accounting_invalidates_an_older_runtime_effect() {
        let clean = Arc::new(State::default());
        let effect = clean.begin_mutation().commit_mutation().unwrap().effect;
        let mut boundary = clean.begin_accounting_data_change();
        boundary.publish_fresh_closed_clock();
        drop(boundary);
        assert_eq!(clean.admission(), Admission::Ready);
        let (authorized, rejected, _) = mutation_effect(effect, async {
            clean
                .begin_accounting_data_change()
                .mutation_effect_authorized()
        })
        .await;
        assert!(!authorized);
        assert!(rejected);

        let settled = Arc::new(State::default());
        let effect = settled.begin_mutation().commit_mutation().unwrap().effect;
        let mut write = settled.begin();
        write.mark_accounting();
        let owner = write.id;
        ACCOUNTING_OWNER
            .scope(owner, async {
                let mut snapshot = settled.begin_accounting_data_change();
                snapshot.capture_additive_snapshot();
                drop(snapshot);
                let mut settlement = settled.begin_accounting_data_change();
                settlement.publish_additive_settlement(false);
            })
            .await;
        let _ = write.complete(true, true, false);
        assert_eq!(settled.admission(), Admission::Ready);
        let (authorized, rejected, _) = mutation_effect(effect, async {
            settled
                .begin_accounting_data_change()
                .mutation_effect_authorized()
        })
        .await;
        assert!(!authorized);
        assert!(rejected);

        let active = Arc::new(State::default());
        let effect = active.begin_mutation().commit_mutation().unwrap().effect;
        let mut write = active.begin();
        write.mark_accounting();
        let (authorized, rejected, _) = mutation_effect(effect, async {
            active
                .begin_accounting_data_change()
                .mutation_effect_authorized()
        })
        .await;
        assert!(!authorized);
        assert!(rejected);
        write.finish(true);
    }

    #[test]
    fn sequence_exhaustion_invalidates_every_epoch_and_holds_admission() {
        let state = Arc::new(State::default());
        {
            let mut inner = state
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner.mutation_sequence = u128::MAX - 1;
        }
        let before = state.mutation_epoch().unwrap();
        let mutation = state.begin_mutation();
        let final_intent = mutation.mutation_epoch().unwrap();
        assert!(mutation.commit_mutation().is_none());
        assert!(state.mutation_epoch().is_none());
        assert!(!state.mutation_epoch_is_current(&before));
        assert!(!state.mutation_epoch_is_current(&final_intent));
        assert_eq!(state.admission(), Admission::Held);
        assert!(!state.permits_write());
    }

    #[test]
    fn retirement_permanently_closes_epochs_writes_and_admission() {
        let state = Arc::new(State::default());
        let epoch = state.mutation_epoch().unwrap();
        let in_flight = state.begin();
        state.retire();

        assert_eq!(state.mutation_epoch(), None);
        assert!(!state.mutation_epoch_is_current(&epoch));
        assert!(!state.mutation_epoch_is_pristine(&epoch));
        assert!(!state.permits_write());
        assert!(!state.permits_admission());
        assert_eq!(state.retirement_carry().hold_class(), HoldClass::Ready);

        in_flight.finish(true);
        assert_eq!(state.retirement_carry().hold_class(), HoldClass::Ready);
        assert!(!state.permits_write());
        assert!(!state.permits_admission());
        assert!(state.begin_mutation().commit_mutation().is_none());

        let failed = Arc::new(State::default());
        failed.begin().finish(false);
        failed.retire();
        assert_eq!(failed.retirement_carry().hold_class(), HoldClass::Failed);
    }

    #[tokio::test]
    async fn failed_additive_retirement_is_permanent_without_its_usage_snapshot() {
        let state = Arc::new(State::default());
        assert!(
            owned(Arc::clone(&state), async {
                definitely_rolled_back();
                Err::<(), _>("injected write failure")
            })
            .await
            .unwrap()
            .is_err()
        );
        state.retire();
        let carry = state.retirement_carry();
        assert_eq!(carry.hold_class(), HoldClass::Permanent);
        let successor = Arc::new(State::default());
        successor.seed_carry(&carry);
        assert_eq!(successor.admission(), Admission::Held);
        assert!(successor.begin_mutation().commit_mutation().is_none());
    }

    #[test]
    fn successful_mutation_retirement_keeps_successors_non_pristine() {
        let state = Arc::new(State::default());
        assert!(state.begin_mutation().commit_mutation().is_some());
        state.retire();
        let carry = state.retirement_carry();
        assert!(!carry.is_pristine());
        assert_eq!(carry.hold_class(), HoldClass::Ready);

        let successor = Arc::new(State::default());
        successor.seed_carry(&carry);
        let epoch = successor.mutation_epoch().unwrap();
        assert!(!successor.mutation_epoch_is_pristine(&epoch));
        assert!(successor.permits_admission());
    }

    #[test]
    fn additive_failure_after_mutation_preparation_withholds_the_transition() {
        for safe in [true, false] {
            let state = Arc::new(State::default());
            let mutation = state.begin_mutation();
            let mut accounting = state.begin();
            accounting.mark_accounting();
            let _ = accounting.complete(false, safe, false);
            assert!(mutation.commit_mutation().is_none());
            assert_eq!(state.admission(), Admission::Held);
        }
    }

    #[test]
    fn quarantine_immediately_closes_epochs_and_in_flight_transitions() {
        let state = Arc::new(State::default());
        let before = state.mutation_epoch().unwrap();
        let mutation = state.begin_mutation();
        state.quarantine_original_goal(QuarantinedUsage {
            original_goal_id: "original".into(),
            elapsed_seconds: 7,
            descendant_tokens: 11,
            turn_tokens: 13,
        });
        state.quarantine_original_goal(QuarantinedUsage {
            original_goal_id: "original".into(),
            elapsed_seconds: 9,
            descendant_tokens: 10,
            turn_tokens: 17,
        });
        assert_eq!(state.mutation_epoch(), None);
        assert!(!state.mutation_epoch_is_current(&before));
        assert!(mutation.commit_mutation().is_none());
        assert_eq!(state.retirement_carry().hold_class(), HoldClass::Permanent);
        assert_eq!(
            state.quarantined_usage(),
            vec![QuarantinedUsage {
                original_goal_id: "original".into(),
                elapsed_seconds: 9,
                descendant_tokens: 11,
                turn_tokens: 17,
            }]
        );
    }

    #[test]
    fn closed_state_rejection_and_write_id_exhaustion_fail_without_reuse() {
        let retired = Arc::new(State::default());
        retired.retire();
        assert!(retired.begin_mutation().commit_mutation().is_none());
        assert_eq!(retired.retirement_carry().hold_class(), HoldClass::Ready);

        let exhausted = Arc::new(State::default());
        exhausted
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .next = u128::MAX;
        let mut mutation = exhausted.begin_mutation();
        assert!(mutation.mutation_epoch().is_none());
        assert!(!mutation.mark_dispatched());
        assert_eq!(
            exhausted.retirement_carry().hold_class(),
            HoldClass::Permanent
        );
    }

    #[test]
    fn settled_unknown_mutation_blocks_replay_but_one_exact_mutation_supersedes_it() {
        let state = Arc::new(State::default());
        let mut first = state
            .begin_accounting_data_change()
            .begin_dispatched_mutation(&state)
            .expect("first exact dispatch");
        assert!(first.predecessor_is_authoritative());
        first.settled_unknown();
        assert!(state.settled_mutation_unknown());
        assert!(!state.permits_write());
        assert!(!state.permits_admission());

        let second = state
            .begin_accounting_data_change()
            .begin_dispatched_mutation(&state)
            .expect("settled command can be superseded exactly");
        assert!(!second.predecessor_is_authoritative());
        assert!(second.commit_mutation().is_some());
        assert!(!state.settled_mutation_unknown());
        assert!(state.permits_write());
        assert!(state.permits_admission());
    }

    #[test]
    fn settled_unknown_recovery_survives_accounting_data_after_dispatch() {
        let state = Arc::new(State::default());
        let mut first = state
            .begin_accounting_data_change()
            .begin_dispatched_mutation(&state)
            .expect("first exact dispatch");
        first.settled_unknown();

        let second = state
            .begin_accounting_data_change()
            .begin_dispatched_mutation(&state)
            .expect("exact recovery dispatch");
        let mut callback = state.begin_accounting_data_change();
        callback.publish_change();
        drop(callback);

        let transition = second
            .commit_mutation()
            .expect("the committed recovery keeps its original accounting boundary");
        assert!(!state.settled_mutation_unknown());
        assert!(!state.mutation_effect_is_current(&transition.effect));
        assert!(state.mutation_effect_has_only_pending_data(&transition.effect));
        assert_eq!(state.admission(), Admission::Held);
        assert!(state.permits_write());
    }

    #[test]
    fn validation_write_remembers_a_settled_unknown_predecessor_atomically() {
        let state = Arc::new(State::default());
        let mut mutation = state
            .begin_accounting_data_change()
            .begin_dispatched_mutation(&state)
            .expect("exact dispatch");
        mutation.settled_unknown();

        let validation = state.begin();
        assert!(validation.predecessor_was_settled_mutation_unknown());
        assert!(!state.settled_mutation_unknown());
        validation.finish(true);
        assert!(state.settled_mutation_unknown());
    }

    #[test]
    fn withdrawn_pre_dispatch_intent_changes_chronology_without_failure() {
        let state = Arc::new(State::default());
        let before = state.mutation_epoch().unwrap();
        let mut mutation = state.begin_mutation();
        assert!(!mutation.was_dispatched());
        mutation.cancel_before_dispatch();
        assert!(!state.mutation_epoch_is_current(&before));
        assert!(state.permits_write());
        assert!(state.permits_admission());
    }

    #[test]
    fn settled_unknown_mutation_recovery_survives_owner_transfer() {
        let original = Arc::new(State::default());
        let mut mutation = original
            .begin_accounting_data_change()
            .begin_dispatched_mutation(&original)
            .expect("exact dispatch");
        mutation.settled_unknown();
        original.retire();

        let successor = Arc::new(State::default());
        successor.seed_carry(&original.retirement_carry());
        assert!(successor.settled_mutation_unknown());
        let mutation = successor
            .begin_accounting_data_change()
            .begin_dispatched_mutation(&successor)
            .expect("successor exact dispatch");
        assert!(mutation.commit_mutation().is_some());
        assert!(successor.permits_admission());
    }

    #[tokio::test]
    async fn queued_writer_rechecks_ambiguity_before_replaying_a_delta() {
        let state = Arc::new(State::default());
        let accounting = Arc::new(tokio::sync::Semaphore::new(1));
        let held = accounting.acquire().await.unwrap();
        let second = tokio::spawn({
            let accounting = Arc::clone(&accounting);
            let state = Arc::clone(&state);
            async move {
                let _permit = accounting.acquire().await.unwrap();
                state.permits_write()
            }
        });
        tokio::task::yield_now().await;
        // The first writer may already have committed but lost its response.
        // Publish while its accounting permit still excludes the queued writer.
        state.block_ambiguous();
        drop(held);
        assert!(!second.await.unwrap());
        assert!(!state.permits_admission());
    }
    #[test]
    fn healthy_pending_write_is_distinct_from_a_failed_write_hold() {
        let state = Arc::new(State::default());
        assert_eq!(state.admission(), Admission::Ready);
        let pending = state.begin();
        assert_eq!(state.admission(), Admission::Writing);
        pending.finish(true);
        assert_eq!(state.admission(), Admission::Ready);
        state.begin().finish(false);
        let later = state.begin();
        assert_eq!(state.admission(), Admission::Held);
        later.finish(true);
        assert_eq!(state.admission(), Admission::Held);

        let permanently_held = Arc::new(State::default());
        let pending = permanently_held.begin();
        permanently_held.hold_permanently();
        assert_eq!(permanently_held.admission(), Admission::Held);
        pending.finish(true);
        assert_eq!(permanently_held.admission(), Admission::Held);
    }
}
