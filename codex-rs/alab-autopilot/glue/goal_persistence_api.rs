// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::runtime::GoalMutationBoundary;
use codex_alab_core::goal_persistence::MutationEpoch;
use std::future::Future;

#[derive(Default)]
pub(super) struct UnownedMutations {
    threads: std::collections::HashMap<String, UnownedThread>,
}

impl std::fmt::Debug for UnownedMutations {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UnownedMutations")
            .field("threads", &self.threads.len())
            .finish()
    }
}

#[derive(Default)]
struct UnownedThread {
    pending: Vec<UnownedIntent>,
    carry: codex_alab_core::goal_persistence::PersistenceCarry,
    runtime: Option<Weak<GoalRuntimeHandle>>,
    turn_options_invalidated: bool,
}

struct UnownedIntent {
    identity: Arc<()>,
    writes: Vec<AttachedWrite>,
    dispatched: bool,
    ownerless_invalidation_created: bool,
}

struct AttachedWrite {
    runtime: Weak<GoalRuntimeHandle>,
    write: codex_alab_core::goal_persistence::Write,
}

impl UnownedThread {
    /// Only the oldest ownerless intent may enter SQLite. Attach that exact
    /// intent to a runtime when one exists; later intents remain queued and
    /// can withdraw without manufacturing an ambiguous storage outcome.
    fn attach_front(&mut self) -> bool {
        let Some(intent) = self.pending.first_mut() else {
            return true;
        };
        if !intent.writes.is_empty() {
            return true;
        }
        let Some(runtime) = self.runtime.as_ref().and_then(Weak::upgrade) else {
            return true;
        };
        let Some(mut write) = runtime.alab_begin_goal_mutation() else {
            return false;
        };
        if intent.dispatched {
            if intent.ownerless_invalidation_created && self.turn_options_invalidated {
                self.turn_options_invalidated = false;
                intent.ownerless_invalidation_created = false;
            }
            runtime.alab_invalidate_turn_start_options();
            if !write.mark_dispatched() {
                runtime.alab_release_turn_start_options_invalidation();
                return false;
            }
        }
        intent.writes.push(AttachedWrite {
            runtime: Arc::downgrade(&runtime),
            write,
        });
        true
    }
}

pub(super) struct UnownedMutation {
    owner: Arc<std::sync::Mutex<UnownedMutations>>,
    thread: String,
    identity: Arc<()>,
    finished: bool,
    admitted: bool,
}

enum UnownedSettlement {
    Committed,
    DefinitelyRolledBack,
    SettledUnknown,
    Abandoned,
}

impl UnownedMutation {
    pub(super) fn admitted(&self) -> bool {
        self.admitted
    }

    /// Authorize dispatch against the live process carry and publish it in one
    /// critical section. A concurrent unknown outcome can never be overtaken.
    pub(super) fn mark_dispatched(&mut self) -> bool {
        let mut owner = self
            .owner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(thread) = owner.threads.get_mut(&self.thread) else {
            return false;
        };
        if thread.carry.hold_class() == codex_alab_core::goal_persistence::HoldClass::Permanent {
            if let Some(intent) = thread
                .pending
                .iter_mut()
                .find(|intent| Arc::ptr_eq(&intent.identity, &self.identity))
            {
                for attached in &mut intent.writes {
                    attached.write.hold_permanently();
                }
            }
            return false;
        }
        let Some(position) = thread
            .pending
            .iter()
            .position(|intent| Arc::ptr_eq(&intent.identity, &self.identity))
        else {
            return false;
        };
        if position != 0 {
            return false;
        }
        let authorized = {
            let intent = &mut thread.pending[position];
            let mut authorized = true;
            for attached in &mut intent.writes {
                if let Some(runtime) = attached.runtime.upgrade() {
                    runtime.alab_invalidate_turn_start_options();
                }
                authorized &= attached.write.mark_dispatched();
            }
            authorized
        };
        if !authorized {
            let any_dispatched = thread.pending[position]
                .writes
                .iter()
                .any(|attached| attached.write.was_dispatched());
            if !any_dispatched {
                for attached in &thread.pending[position].writes {
                    if let Some(runtime) = attached.runtime.upgrade() {
                        runtime.alab_release_turn_start_options_invalidation();
                    }
                }
            }
            if thread.pending[position]
                .writes
                .iter()
                .any(|attached| attached.write.permanently_closed())
            {
                for attached in &mut thread.pending[position].writes {
                    attached.write.hold_permanently();
                }
                thread.carry.hold_permanently();
            }
            return false;
        }
        thread.pending[position].dispatched = true;
        if thread.pending[position].writes.is_empty() {
            let created = !thread.turn_options_invalidated;
            thread.turn_options_invalidated = true;
            thread.pending[position].ownerless_invalidation_created = created;
        }
        true
    }

    fn finish(
        &mut self,
        settlement: UnownedSettlement,
    ) -> Option<(
        Arc<GoalRuntimeHandle>,
        codex_alab_core::goal_persistence::MutationEffect,
    )> {
        if self.finished {
            return None;
        }
        let mut owner = self
            .owner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(thread) = owner.threads.get_mut(&self.thread) else {
            self.finished = true;
            return None;
        };
        let Some(position) = thread
            .pending
            .iter()
            .position(|intent| Arc::ptr_eq(&intent.identity, &self.identity))
        else {
            self.finished = true;
            return None;
        };
        let intent = thread.pending.remove(position);
        let mut committed_effect = None;
        if matches!(settlement, UnownedSettlement::Committed) {
            let mut exact = true;
            for attached in intent.writes {
                match (attached.runtime.upgrade(), attached.write.commit_mutation()) {
                    (Some(runtime), Some(transition)) if committed_effect.is_none() => {
                        committed_effect = Some((runtime, transition.effect));
                    }
                    (Some(_), Some(_)) | (_, None) => exact = false,
                    (None, Some(_)) => {}
                }
            }
            if !exact {
                thread.carry.hold_ambiguous();
                committed_effect = None;
            } else {
                // A later serialized acknowledged mutation reconciles a prior
                // settled failure exactly as a native one does.
                thread.carry.acknowledge_mutation();
            }
        } else if matches!(settlement, UnownedSettlement::DefinitelyRolledBack) {
            if intent.ownerless_invalidation_created {
                thread.turn_options_invalidated = false;
            }
            for mut attached in intent.writes {
                if let Some(runtime) = attached.runtime.upgrade() {
                    runtime.alab_release_turn_start_options_invalidation();
                    if attached.write.predecessor_is_authoritative() {
                        runtime
                            .accounting_state()
                            .alab_reopen_rolled_back_mutation();
                    }
                }
                // Reopen while the write is still active, so a successor cannot
                // dispatch between settlement and thaw.
                attached.write.definitely_rolled_back();
            }
            thread.carry.hold_failed();
        } else if matches!(settlement, UnownedSettlement::SettledUnknown) {
            for mut attached in intent.writes {
                attached.write.settled_unknown();
            }
            thread.carry.hold_settled_mutation_unknown();
        } else if intent.dispatched
            || intent
                .writes
                .iter()
                .any(|attached| attached.write.was_dispatched())
        {
            // Dropping the writes publishes their permanent unknown-settlement
            // fence to attached owners; retain the same fence for every successor.
            drop(intent.writes);
            thread.carry.hold_permanently();
        } else {
            for mut attached in intent.writes {
                attached.write.cancel_before_dispatch();
            }
            // The process chronology remains non-pristine, but a request which
            // reached no storage command creates no persistence failure.
        }
        self.finished = true;
        committed_effect
    }

    fn commit(
        mut self,
    ) -> Option<(
        Arc<GoalRuntimeHandle>,
        codex_alab_core::goal_persistence::MutationEffect,
    )> {
        self.finish(UnownedSettlement::Committed)
    }

    fn definitely_rolled_back(&mut self) {
        let _ = self.finish(UnownedSettlement::DefinitelyRolledBack);
    }

    fn settled_unknown(&mut self) {
        let _ = self.finish(UnownedSettlement::SettledUnknown);
    }
}

impl Drop for UnownedMutation {
    fn drop(&mut self) {
        let _ = self.finish(UnownedSettlement::Abandoned);
    }
}

async fn finish_deferred_upgrade_retirement(
    runtime: &Arc<GoalRuntimeHandle>,
    key: &str,
    runtimes: &Arc<std::sync::Mutex<std::collections::HashMap<String, Weak<GoalRuntimeHandle>>>>,
    gaps: &Arc<std::sync::Mutex<UnownedMutations>>,
    retirement: &codex_alab_core::upgrade_barrier::DeferredRetirement,
    capture: Result<(), String>,
) -> Result<(), String> {
    let _retirement = runtime.alab_goal_runtime_retirement_permit().await?;
    let _callbacks = runtime
        .alab_goal_runtime_retirement_callback_permit()
        .await?;
    let _accounting = runtime
        .alab_goal_runtime_retirement_accounting_permit()
        .await?;
    let mut errors = Vec::new();
    if let Err(error) = capture {
        errors.push(format!(
            "final upgrade capture was abandoned before source quiescence: {error}"
        ));
        runtime.alab_permanently_hold_goal_persistence();
        runtime
            .accounting_state()
            .alab_quarantine_pending_boundaries();
    } else if let Err(error) = runtime.alab_finish_deferred_upgrade_retirement() {
        errors.push(error);
        runtime
            .accounting_state()
            .alab_quarantine_pending_boundaries();
    }
    if !runtime
        .accounting_state()
        .alab_persistence
        .quarantined_usage()
        .is_empty()
    {
        errors.push("deferred upgrade retirement retains quarantined goal usage".to_owned());
    }
    runtime.alab_finalize_runtime_retirement();
    let carry = runtime.alab_retired_goal_persistence_carry();
    if carry.hold_class() != codex_alab_core::goal_persistence::HoldClass::Ready {
        errors.push(format!(
            "deferred upgrade retirement retained a {:?} persistence hold",
            carry.hold_class()
        ));
    }
    runtime.alab_retire_runtime_status();
    let prepared = if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    };
    let publication = if retirement.prepare(prepared.clone()) {
        retirement.published().await
    } else {
        Err("deferred upgrade retirement lost its preparation owner".into())
    };
    let publication_scope = retirement.publication_scope();
    if prepared.is_ok()
        && publication.is_ok()
        && let Some(inventory) = publication_scope
    {
        if inventory {
            // The shared coordinator already merged this carry and removed the
            // exact old key. A successor may legitimately have registered
            // before this detached cleanup owner resumes.
            return Ok(());
        }
        // Inventory-wide publication normally merged the Ready carry and
        // removed the exact key above. Preserve the barrier's standalone API
        // for direct single-runtime owners by finishing that one key here.
        let weak = Arc::downgrade(runtime);
        let mut registered = runtimes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if registered
            .get(key)
            .is_some_and(|current| current.ptr_eq(&weak))
        {
            let mut gaps = gaps
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let gap = gaps.threads.entry(key.to_owned()).or_default();
            gap.runtime = None;
            gap.turn_options_invalidated |= runtime.alab_turn_start_options_invalidated();
            gap.carry.merge(carry);
            registered.remove(key);
        } else if registered.contains_key(key) {
            return Err("published upgrade retirement lost exact registry custody".into());
        }
        return Ok(());
    }

    let mut failures = Vec::new();
    if let Err(error) = prepared {
        failures.push(error);
    }
    if let Err(error) = publication {
        failures.push(format!("final upgrade capture was not published: {error}"));
    } else if publication_scope.is_none() {
        failures.push("final upgrade capture publication was revoked".into());
    }
    runtime.alab_permanently_hold_goal_persistence();
    runtime
        .accounting_state()
        .alab_quarantine_pending_boundaries();
    let carry = runtime.alab_retired_goal_persistence_carry();
    let weak = Arc::downgrade(runtime);
    let mut registered = runtimes
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let exact = registered
        .get(key)
        .is_some_and(|current| current.ptr_eq(&weak));
    let mut gaps = gaps
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let gap = gaps.threads.entry(key.to_owned()).or_default();
    gap.runtime = None;
    gap.turn_options_invalidated |= runtime.alab_turn_start_options_invalidated();
    gap.carry.merge(carry);
    gap.carry.hold_permanently();
    if exact {
        registered.remove(key);
    } else {
        failures.push("deferred upgrade retirement lost exact registry custody".into());
    }
    Err(failures.join("; "))
}

impl GoalService {
    /// Keep a live runtime's native goal permit through the host's SQL,
    /// durable chronology and exact runtime effect. The nested service call
    /// observes the task-local owner and does not acquire the permit twice.
    pub async fn alab_scope_host_goal_mutation<T>(
        &self,
        thread_id: codex_protocol::ThreadId,
        future: impl Future<Output = T>,
    ) -> Result<T, String> {
        if !codex_alab_core::is_alab_binary() {
            return Ok(future.await);
        }
        let Some(runtime) = self.runtime_for_thread(thread_id) else {
            return Ok(future.await);
        };
        let _permit = runtime.goal_state_permit().await?;
        Ok(crate::runtime::alab_scope_host_goal_mutation(thread_id, future).await)
    }

    pub(super) fn alab_arm_goal_chronology_dispatch(
        &self,
        runtime: Option<&Arc<GoalRuntimeHandle>>,
        mutation: &mut Option<codex_alab_core::goal_persistence::Write>,
        unowned: &mut Option<UnownedMutation>,
    ) -> Result<(), GoalServiceError> {
        if let Err(error) = codex_alab_core::goal_chronology_fence::arm_for_dispatch() {
            self.alab_definitely_rollback_goal_mutation(runtime, mutation, unowned);
            if let Err(clear_error) =
                codex_alab_core::goal_chronology_fence::restore_dispatch_fence()
            {
                tracing::warn!(%clear_error, "failed to withdraw undispatched goal chronology fence");
            }
            return Err(GoalServiceError::Internal(format!(
                "cannot fence goal chronology before SQLite dispatch: {error}"
            )));
        }
        Ok(())
    }

    pub(super) fn alab_settle_goal_mutation_error(
        &self,
        runtime: Option<&Arc<GoalRuntimeHandle>>,
        mutation: &mut Option<codex_alab_core::goal_persistence::Write>,
        unowned: &mut Option<UnownedMutation>,
        error: &anyhow::Error,
    ) {
        if codex_state::alab_goal_read_recovery::mutation_definitely_uncommitted(error) {
            self.alab_definitely_rollback_goal_mutation(runtime, mutation, unowned);
            if let Err(clear_error) =
                codex_alab_core::goal_chronology_fence::restore_dispatch_fence()
            {
                tracing::warn!(%clear_error, "failed to withdraw rolled-back goal chronology fence");
            }
        } else {
            if let Some(mutation) = mutation {
                mutation.settled_unknown();
            }
            if let Some(unowned) = unowned {
                unowned.settled_unknown();
            }
        }
    }

    pub(super) fn alab_settle_goal_mutation_unknown(
        &self,
        mutation: &mut Option<codex_alab_core::goal_persistence::Write>,
        unowned: &mut Option<UnownedMutation>,
    ) {
        if let Some(mutation) = mutation {
            mutation.settled_unknown();
        }
        if let Some(unowned) = unowned {
            unowned.settled_unknown();
        }
    }

    pub(super) fn alab_settle_goal_mutation_noop(
        &self,
        runtime: Option<&Arc<GoalRuntimeHandle>>,
        mutation: &mut Option<codex_alab_core::goal_persistence::Write>,
        unowned: &mut Option<UnownedMutation>,
    ) {
        self.alab_definitely_rollback_goal_mutation(runtime, mutation, unowned);
        if let Err(error) = codex_alab_core::goal_chronology_fence::restore_dispatch_fence() {
            tracing::warn!(%error, "failed to withdraw no-op goal chronology fence");
        }
    }

    pub(super) fn alab_definitely_rollback_goal_mutation(
        &self,
        runtime: Option<&Arc<GoalRuntimeHandle>>,
        mutation: &mut Option<codex_alab_core::goal_persistence::Write>,
        unowned: &mut Option<UnownedMutation>,
    ) {
        if let Some(mutation) = mutation {
            if let Some(runtime) = runtime {
                runtime.alab_release_turn_start_options_invalidation();
                if mutation.predecessor_is_authoritative() {
                    runtime
                        .accounting_state()
                        .alab_reopen_rolled_back_mutation();
                }
            }
            // Keep the active mutation visible until after the clock is open.
            mutation.definitely_rolled_back();
        }
        if let Some(unowned) = unowned {
            unowned.definitely_rolled_back();
        }
    }

    pub(super) fn alab_begin_dispatched_goal_mutation(
        &self,
        runtime: Option<&Arc<GoalRuntimeHandle>>,
    ) -> Result<
        (
            Option<GoalMutationBoundary>,
            Option<codex_alab_core::goal_persistence::Write>,
        ),
        GoalServiceError,
    > {
        if !codex_alab_core::is_alab_binary() {
            return Ok((None, None));
        }
        runtime.map_or(Ok((None, None)), |runtime| {
            runtime
                .alab_freeze_goal_mutation_boundary()
                .map(|(boundary, write)| (Some(boundary), Some(write)))
                .map_err(|error| GoalServiceError::InvalidRequest(error.to_string()))
        })
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn alab_debug_permanently_hold_unowned_goal_mutation(
        &self,
        thread_id: codex_protocol::ThreadId,
    ) {
        self.alab_unowned_mutations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .threads
            .entry(thread_id.to_string())
            .or_default()
            .carry
            .hold_permanently();
    }

    pub(super) fn alab_note_dead_goal_runtime(&self, key: &str) {
        if !codex_alab_core::is_alab_binary() {
            return;
        }
        self.alab_unowned_mutations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .threads
            .entry(key.to_owned())
            .or_default()
            .carry
            .hold_permanently();
    }

    pub(super) fn alab_commit_goal_mutation(
        &self,
        thread_id: codex_protocol::ThreadId,
        runtime: Option<&Arc<GoalRuntimeHandle>>,
        boundary: Option<GoalMutationBoundary>,
        mutation: Option<codex_alab_core::goal_persistence::Write>,
        unowned: Option<UnownedMutation>,
    ) -> Option<GoalMutationBoundary> {
        let transition =
            mutation.and_then(codex_alab_core::goal_persistence::Write::commit_mutation);
        if transition.is_some() {
            self.alab_reconcile_unowned_goal_mutation(thread_id);
        }
        let effect = transition
            .and_then(|transition| runtime.cloned().map(|runtime| (runtime, transition.effect)))
            .or_else(|| unowned.and_then(UnownedMutation::commit));
        match (boundary, effect) {
            (Some(boundary), effect) => {
                boundary.bind(effect.map(|(_, effect)| effect));
                Some(boundary)
            }
            (None, Some((runtime, effect))) => Some(GoalMutationBoundary::bound(runtime, effect)),
            (None, None) => None,
        }
    }

    fn alab_reconcile_unowned_goal_mutation(&self, thread_id: codex_protocol::ThreadId) {
        if let Some(thread) = self
            .alab_unowned_mutations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .threads
            .get_mut(&thread_id.to_string())
            && thread.pending.is_empty()
        {
            thread.carry.acknowledge_mutation();
        }
    }

    /// Snapshot the exact runtime owner and establish an absent-owner fence in
    /// one registry critical section. A successor can therefore never publish
    /// a pristine epoch between intent creation and SQL settlement.
    pub(super) fn alab_runtime_for_goal_mutation(
        &self,
        thread_id: codex_protocol::ThreadId,
    ) -> (Option<Arc<GoalRuntimeHandle>>, Option<UnownedMutation>) {
        if !codex_alab_core::is_alab_binary() {
            return (self.runtime_for_thread(thread_id), None);
        }
        let key = thread_id.to_string();
        let mut runtimes = self.runtimes();
        let published = runtimes.contains_key(&key);
        let runtime = runtimes.get(&key).and_then(Weak::upgrade);
        if runtime.is_some() {
            // Registration can attach the oldest ownerless intent to this live
            // runtime. Reject a newer request while registry custody still
            // orders the gap lookup, before runtime preparation can account or
            // otherwise revoke the older intent's exact pre-SQL witness.
            let pending = self
                .alab_unowned_mutations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .threads
                .get(&key)
                .is_some_and(|thread| !thread.pending.is_empty());
            if pending {
                return (
                    runtime,
                    Some(UnownedMutation {
                        owner: Arc::clone(&self.alab_unowned_mutations),
                        thread: key,
                        identity: Arc::new(()),
                        finished: true,
                        admitted: false,
                    }),
                );
            }
            return (runtime, None);
        }
        if published {
            self.alab_note_dead_goal_runtime(&key);
        }
        runtimes.remove(&key);
        let identity = Arc::new(());
        let mut gaps = self
            .alab_unowned_mutations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let gap = gaps.threads.entry(key.clone()).or_default();
        if !gap.pending.is_empty() {
            drop(gaps);
            return (
                None,
                Some(UnownedMutation {
                    owner: Arc::clone(&self.alab_unowned_mutations),
                    thread: key,
                    identity,
                    finished: true,
                    admitted: false,
                }),
            );
        }
        gap.carry.note_intent();
        gap.pending.push(UnownedIntent {
            identity: Arc::clone(&identity),
            writes: Vec::new(),
            dispatched: false,
            ownerless_invalidation_created: false,
        });
        if !gap.attach_front() {
            gap.carry.hold_permanently();
        }
        drop(gaps);
        (
            None,
            Some(UnownedMutation {
                owner: Arc::clone(&self.alab_unowned_mutations),
                thread: key,
                identity,
                finished: false,
                admitted: true,
            }),
        )
    }

    pub fn alab_goal_runtime(
        &self,
        thread_id: codex_protocol::ThreadId,
    ) -> Option<Arc<GoalRuntimeHandle>> {
        self.runtime_for_thread(thread_id)
    }

    /// Validate the exact registered epoch. The caller holds that runtime's
    /// native mutation permit, so permit-draining retirement cannot replace it
    /// between this check and the caller's synchronous publication.
    pub fn alab_with_current_goal_mutation_epoch<T>(
        &self,
        thread_id: codex_protocol::ThreadId,
        epoch: &codex_alab_core::goal_persistence::MutationEpoch,
        publish: impl FnOnce() -> T,
    ) -> Option<T> {
        let current = {
            let runtimes = self.runtimes();
            runtimes
                .get(&thread_id.to_string())
                .and_then(Weak::upgrade)
                .is_some_and(|runtime| runtime.alab_goal_mutation_epoch_is_current(epoch))
        };
        current.then(publish)
    }

    fn alab_upgrade_retirement_coordinator(
        &self,
    ) -> Arc<crate::runtime::UpgradeRetirementCoordinator> {
        Arc::clone(self.alab_upgrade_retirement.get_or_init(|| {
            let runtimes = Arc::downgrade(&self.runtimes);
            let gaps = Arc::downgrade(&self.alab_unowned_mutations);
            Arc::new(crate::runtime::UpgradeRetirementCoordinator::new(
                move |members, commit| {
                    let runtimes = runtimes
                        .upgrade()
                        .ok_or("goal runtime registry ended before upgrade publication")?;
                    let gaps = gaps
                        .upgrade()
                        .ok_or("goal persistence-gap registry ended before upgrade publication")?;
                    let mut registered = runtimes
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let mut seen = std::collections::HashSet::new();
                    let mut prepared = Vec::with_capacity(members.len());
                    for runtime in members {
                        let key = runtime.thread_id().to_string();
                        if !seen.insert(key.clone()) {
                            return Err(
                                "upgrade publication repeats a goal runtime registry key".into()
                            );
                        }
                        let weak = Arc::downgrade(runtime);
                        if !registered
                            .get(&key)
                            .is_some_and(|current| current.ptr_eq(&weak))
                        {
                            return Err(format!(
                                "upgrade publication lost exact registry custody for {key}"
                            ));
                        }
                        if !runtime.alab_runtime_status_retired() {
                            return Err(format!(
                                "upgrade publication reached an active runtime for {key}"
                            ));
                        }
                        if !runtime
                            .accounting_state()
                            .alab_persistence
                            .quarantined_usage()
                            .is_empty()
                        {
                            return Err(format!(
                                "upgrade publication retains quarantined usage for {key}"
                            ));
                        }
                        let carry = runtime.alab_retired_goal_persistence_carry();
                        if carry.hold_class() != codex_alab_core::goal_persistence::HoldClass::Ready
                        {
                            return Err(format!(
                                "upgrade publication retains a {:?} persistence hold for {key}",
                                carry.hold_class()
                            ));
                        }
                        prepared.push((key, runtime, carry));
                    }

                    let mut gaps = gaps
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let mut published_keys = Vec::with_capacity(prepared.len());
                    for (key, runtime, carry) in prepared {
                        let gap = gaps.threads.entry(key.clone()).or_default();
                        gap.runtime = None;
                        gap.turn_options_invalidated |=
                            runtime.alab_turn_start_options_invalidated();
                        gap.carry.merge(carry);
                        registered.remove(&key);
                        published_keys.push(key);
                    }
                    // Registry/carry publication precedes the final barriers and
                    // watch delivery, all under the same locks and without an
                    // await. If an internal barrier invariant is violated, turn
                    // every already-unregistered key into a permanent hold
                    // before any successor can acquire the registry.
                    if let Err(error) = commit() {
                        for key in published_keys {
                            gaps.threads
                                .entry(key)
                                .or_default()
                                .carry
                                .hold_permanently();
                        }
                        return Err(error);
                    }
                    Ok(())
                },
            ))
        }))
    }

    pub(super) fn alab_register_goal_runtime(&self, runtime: &Arc<GoalRuntimeHandle>) {
        if !codex_alab_core::is_alab_binary() {
            self.runtimes()
                .insert(runtime.thread_id().to_string(), Arc::downgrade(runtime));
            return;
        }
        let key = runtime.thread_id().to_string();
        let upgrade_retirement = self.alab_upgrade_retirement_coordinator();
        let registry = Arc::downgrade(&self.runtimes);
        let mut runtimes = self.runtimes();
        let (accepted, fresh) = if runtime.alab_runtime_status_retired() {
            (false, false)
        } else {
            match runtimes.get(&key).map(Weak::upgrade) {
                Some(Some(current)) => (Arc::ptr_eq(&current, runtime), false),
                Some(None) => {
                    self.alab_note_dead_goal_runtime(&key);
                    runtimes.insert(key.clone(), Arc::downgrade(runtime));
                    (true, true)
                }
                None => {
                    runtimes.insert(key.clone(), Arc::downgrade(runtime));
                    (true, true)
                }
            }
        };
        if accepted {
            if fresh
                && let Some(gap) = self
                    .alab_unowned_mutations
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .threads
                    .get_mut(&key)
            {
                runtime.alab_seed_goal_persistence_carry(&gap.carry);
                gap.runtime = Some(Arc::downgrade(runtime));
                let attachment_failed = !gap.attach_front();
                if gap.turn_options_invalidated {
                    runtime.alab_invalidate_turn_start_options();
                    gap.turn_options_invalidated = false;
                }
                if attachment_failed {
                    runtime.alab_permanently_hold_goal_persistence();
                    gap.carry.hold_permanently();
                }
            }
            // Bind before exposing the key without registry custody. A matching
            // retirement can then make this handle permanently ineligible.
            let gaps = Arc::downgrade(&self.alab_unowned_mutations);
            let reconcile_key = key;
            let reconcile: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                if let Some(gaps) = gaps.upgrade()
                    && let Some(thread) = gaps
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .threads
                        .get_mut(&reconcile_key)
                    && thread.pending.is_empty()
                {
                    thread.carry.acknowledge_mutation();
                }
            });
            runtime.alab_watch_runtime_status(registry, reconcile, upgrade_retirement);
        } else {
            // Refusal is permanent: a stale late startup cannot claim the key
            // after the owner it raced eventually retires.
            runtime.alab_reject_runtime_status();
            tracing::warn!(
                thread_id = %runtime.thread_id(),
                "refused to replace a live native goal runtime"
            );
        }
        drop(runtimes);
    }

    pub(super) async fn alab_unregister_goal_runtime(&self, runtime: &Arc<GoalRuntimeHandle>) {
        let key = runtime.thread_id().to_string();
        let weak = Arc::downgrade(runtime);
        if !codex_alab_core::is_alab_binary() {
            let mut runtimes = self.runtimes();
            if runtimes
                .get(&key)
                .is_some_and(|registered| registered.ptr_eq(&weak))
            {
                runtimes.remove(&key);
            }
            return;
        }
        if !self
            .runtimes()
            .get(&key)
            .is_some_and(|registered| registered.ptr_eq(&weak))
        {
            return;
        }
        let _ = runtime.alab_begin_runtime_retirement();
        let deferred = match runtime.alab_defer_committed_goal_runtime_retirement() {
            Some(codex_alab_core::upgrade_barrier::RetirementRegistration::Owner(owner)) => {
                Some(owner)
            }
            Some(codex_alab_core::upgrade_barrier::RetirementRegistration::Registered) => {
                return;
            }
            None => None,
        };
        let wait_for_cleanup = deferred.is_none();
        let runtimes = Arc::clone(&self.runtimes);
        let gaps = Arc::clone(&self.alab_unowned_mutations);
        let runtime = Arc::clone(runtime);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        // Once retirement is visible, its owner outlives cancellation of the
        // lifecycle observer. A committed upgrade keeps this registry key
        // reserved and returns from source stop; its exact transfer releases
        // cleanup only after final accounting. Ordinary retries may join with a
        // second harmless owner; exact registry custody makes removal idempotent.
        tokio::spawn(async move {
            if let Some(retirement) = deferred {
                let capture = retirement.released().await;
                let result = finish_deferred_upgrade_retirement(
                    &runtime,
                    &key,
                    &runtimes,
                    &gaps,
                    &retirement,
                    capture,
                )
                .await;
                retirement.complete(result);
                let _ = sender.send(());
                return;
            }
            if let Ok(_retirement) = runtime.alab_goal_runtime_retirement_permit().await {
                let _callbacks = runtime
                    .alab_goal_runtime_retirement_callback_permit()
                    .await
                    .ok(); // ALAB(storage): the final drain and carry snapshot exclude every token callback admitted by this owner.
                if let Err(error) = runtime.alab_drain_retained_progress().await {
                    tracing::warn!(%error, "retiring native goal runtime retained an unreconciled accounting boundary");
                }
                let _accounting = runtime
                    .alab_goal_runtime_retirement_accounting_permit()
                    .await
                    .ok();
                if !runtime
                    .accounting_state()
                    .alab_persistence
                    .quarantined_usage()
                    .is_empty()
                {
                    runtime
                        .accounting_state()
                        .alab_quarantine_pending_boundaries();
                }
                runtime.alab_finalize_runtime_retirement();
                let weak = Arc::downgrade(&runtime);
                let mut registered = runtimes
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if registered
                    .get(&key)
                    .is_some_and(|current| current.ptr_eq(&weak))
                {
                    let carry = runtime.alab_retired_goal_persistence_carry();
                    let mut gaps = gaps
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let gap = gaps.threads.entry(key.clone()).or_default();
                    gap.runtime = None;
                    gap.turn_options_invalidated |= runtime.alab_turn_start_options_invalidated();
                    gap.carry.merge(carry);
                    drop(gaps);
                    // Publish absence before exposing the vacant key.
                    runtime.alab_retire_runtime_status();
                    registered.remove(&key);
                }
            }
            let _ = sender.send(());
        });
        if wait_for_cleanup {
            let _ = receiver.await;
        }
    }

    /// Read the registered native owner's epoch; this is not committed stop evidence.
    pub fn alab_model_goal_epoch(&self, thread_id: ThreadId) -> Option<MutationEpoch> {
        self.runtime_for_thread(thread_id)
            .and_then(|runtime| runtime.alab_model_goal_epoch())
    }

    pub fn alab_model_goal_epoch_current(
        &self,
        thread_id: ThreadId,
        epoch: &MutationEpoch,
    ) -> bool {
        self.runtime_for_thread(thread_id)
            .is_some_and(|runtime| runtime.alab_model_goal_epoch_current(epoch))
    }

    pub(super) async fn alab_record_goal_instruction(
        &self,
        runtime: Option<&Arc<GoalRuntimeHandle>>,
        instruction: impl Future<Output = Result<(), GoalServiceError>> + Send,
    ) -> Result<(), GoalServiceError> {
        let record = runtime
            .filter(|_| codex_alab_core::is_alab_binary())
            .map(|runtime| runtime.accounting_state().alab_persistence.begin());
        let result = instruction.await;
        if let Some(record) = record {
            // Stale automatic decisions and healthy validation refusals have
            // reached no goal SQL. A failed/cancelled recording remains held.
            record.finish(
                result.is_ok() || matches!(&result, Err(GoalServiceError::InvalidRequest(_))),
            );
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownerless_intent_with_no_storage_command_does_not_create_a_failure_hold() {
        codex_alab_core::activate();
        let service = GoalService::new();
        let thread = codex_protocol::ThreadId::new();
        let (runtime, intent) = service.alab_runtime_for_goal_mutation(thread);
        assert!(runtime.is_none());
        drop(intent);

        let gaps = service
            .alab_unowned_mutations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let carry = &gaps.threads[&thread.to_string()].carry;
        assert_eq!(
            carry.hold_class(),
            codex_alab_core::goal_persistence::HoldClass::Ready
        );
        assert!(!carry.is_pristine());
    }

    #[test]
    fn overlapping_ownerless_intents_that_never_dispatch_withdraw_cleanly() {
        codex_alab_core::activate();
        let service = GoalService::new();
        let thread = codex_protocol::ThreadId::new();
        let (_, first) = service.alab_runtime_for_goal_mutation(thread);
        let (_, second) = service.alab_runtime_for_goal_mutation(thread);
        assert!(!second.as_ref().expect("rejected overlap").admitted());

        drop(first);
        drop(second);

        let gaps = service
            .alab_unowned_mutations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let gap = &gaps.threads[&thread.to_string()];
        assert!(gap.pending.is_empty());
        assert_eq!(
            gap.carry.hold_class(),
            codex_alab_core::goal_persistence::HoldClass::Ready
        );
        assert!(!gap.carry.is_pristine());
    }

    #[test]
    fn rejected_overlap_does_not_prevent_front_commit_reconciliation() {
        codex_alab_core::activate();
        let service = GoalService::new();
        let thread = codex_protocol::ThreadId::new();
        service
            .alab_unowned_mutations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .threads
            .entry(thread.to_string())
            .or_default()
            .carry
            .hold_failed();
        let (_, mut first) = service.alab_runtime_for_goal_mutation(thread);
        let (_, second) = service.alab_runtime_for_goal_mutation(thread);
        assert!(!second.as_ref().expect("rejected overlap").admitted());
        let mut first = first.take().expect("front intent");
        assert!(first.mark_dispatched());
        let _ = first.commit();

        drop(second);

        let gaps = service
            .alab_unowned_mutations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let gap = &gaps.threads[&thread.to_string()];
        assert!(gap.pending.is_empty());
        assert_eq!(
            gap.carry.hold_class(),
            codex_alab_core::goal_persistence::HoldClass::Ready
        );
    }
}
