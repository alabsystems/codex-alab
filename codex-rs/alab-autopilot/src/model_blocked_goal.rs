// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Recover the native model's committed stop while its mutation epoch stands.
//! Saved provenance and process-local mutation custody are separate: importing
//! an old goal can never acquire the epoch of a later explicit mutation.

use super::*;
use codex_alab_core::goal_persistence::MutationEpoch;
use codex_alab_core::model_goal_stops::Binding;
use codex_alab_core::model_goal_stops::Custody;
use codex_alab_core::model_goal_stops::PreviousStatus;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ModelGoalBinding {
    pub(super) goal: codex_alab_core::session::WallGoal,
    pub(super) epoch: MutationEpoch,
    pub(super) mode_epoch: u64,
}

impl std::ops::Deref for ModelGoalBinding {
    type Target = codex_alab_core::session::WallGoal;

    fn deref(&self) -> &Self::Target {
        &self.goal
    }
}

impl PartialEq<codex_alab_core::session::WallGoal> for ModelGoalBinding {
    fn eq(&self, other: &codex_alab_core::session::WallGoal) -> bool {
        self.goal == *other
    }
}

impl PartialEq<ModelGoalBinding> for codex_alab_core::session::WallGoal {
    fn eq(&self, other: &ModelGoalBinding) -> bool {
        *self == other.goal
    }
}

impl ModelGoalBinding {
    pub(super) fn passive(&self) -> Binding {
        Binding {
            goal: self.goal.clone(),
            epoch: self.epoch.clone(),
            mode_epoch: self.mode_epoch,
        }
    }

    fn from_passive(binding: Binding) -> Self {
        Self {
            goal: binding.goal,
            epoch: binding.epoch,
            mode_epoch: binding.mode_epoch,
        }
    }
}

#[derive(Clone, Debug)]
struct ImportBridge {
    goal: codex_alab_core::session::WallGoal,
    epoch: MutationEpoch,
    mode_epoch: u64,
}

#[derive(Default)]
pub(super) struct PendingImport {
    /// Resolved or explicitly superseded sources never reopen this runtime.
    staged: bool,
    pub(super) goal: Option<codex_alab_core::session::WallGoal>,
    bridge: Option<ImportBridge>,
}

impl Autopilot {
    /// Lock this runtime's model sources and reconcile its observed mode. The
    /// caller may keep the returned guard through a process-wide publication;
    /// `current` says whether local provenance belongs to the live generation.
    pub(super) fn lock_model_goal_mode<'a>(&self, rt: &'a ThreadRt) -> (MutexGuard<'a, ()>, bool) {
        let sources = lock(&rt.model_goal_sources);
        let current = codex_alab_core::mode_epoch();
        let mut observed = lock(&rt.model_goal_mode_epoch);
        if current.is_some() && *observed == current {
            drop(observed);
            return (sources, true);
        }
        *observed = current;
        drop(observed);
        lock(&rt.model_blocked_goal).take();
        *lock(&rt.model_goal_import) = PendingImport {
            staged: true,
            goal: None,
            bridge: None,
        };
        self.sup
            .shared()
            .set_model_goal_custody(&rt.key, Custody::Closed);
        self.sup.shared().retire_carried_model_goal(&rt.key);
        (sources, false)
    }

    /// A mode boundary is a provenance boundary even when vanilla work happens
    /// to restore the same final row. Clear every older in-memory/carried source.
    pub(super) fn model_goal_mode_current<'a>(
        &self,
        rt: &'a ThreadRt,
    ) -> Option<MutexGuard<'a, ()>> {
        let (sources, current) = self.lock_model_goal_mode(rt);
        current.then_some(sources)
    }

    /// Final non-awaiting check used by the native goal service immediately
    /// before it creates the SQLite mutation owner.
    pub(super) fn model_goal_dispatch_current(
        &self,
        rt: &ThreadRt,
        goals: &GoalService,
        expected: &ModelGoalBinding,
    ) -> bool {
        let Some(_sources) = self.model_goal_mode_current(rt) else {
            return false;
        };
        codex_alab_core::mode_epoch() == Some(expected.mode_epoch)
            && lock(&rt.model_blocked_goal).as_ref() == Some(expected)
            && self.model_goal_chronology_current(rt, expected)
            && goals
                .alab_with_current_goal_mutation_epoch(rt.id, &expected.epoch, || ())
                .is_some()
    }

    fn model_goal_chronology_current(&self, rt: &ThreadRt, expected: &ModelGoalBinding) -> bool {
        rt.config.as_ref().is_some_and(|config| {
            matches!(
                codex_alab_core::goal_chronology_fence::read_current(
                    &config.codex_home,
                    &rt.id.to_string(),
                ),
                Ok(generation) if generation == expected.goal.chronology_fence_generation
            )
        })
    }

    pub(super) fn model_epoch_current(&self, id: ThreadId, epoch: &MutationEpoch) -> bool {
        self.goals
            .as_ref()
            .is_some_and(|goals| goals.alab_model_goal_epoch_current(id, epoch))
    }

    /// Preserve only passive goal provenance from a qualified record. Process
    /// custody may retain an exact epoch across runtime replacement, but history,
    /// mode, chronology and the native owner still have to validate it here.
    pub(super) fn stage_model_goal_stop(&self, rt: &ThreadRt) {
        let Some(_sources) = self.model_goal_mode_current(rt) else {
            return;
        };
        let live = lock(&rt.model_blocked_goal).is_some();
        let mut pending = lock(&rt.model_goal_import);
        if pending.staged {
            return;
        }
        if live {
            pending.staged = true;
            return;
        }
        let shared = self.sup.shared();
        let mode_epoch = codex_alab_core::mode_epoch();
        let lease = match shared.model_goal_custody(&rt.key) {
            Some(Custody::Known(binding)) if Some(binding.mode_epoch) == mode_epoch => {
                pending.goal = Some(binding.goal.clone());
                pending.bridge = Some(ImportBridge {
                    goal: binding.goal,
                    epoch: binding.epoch,
                    mode_epoch: binding.mode_epoch,
                });
                pending.staged = true;
                return;
            }
            Some(Custody::Import {
                epoch,
                mode_epoch: custody_mode,
            }) if Some(custody_mode) == mode_epoch => Some((epoch, custody_mode)),
            Some(Custody::Known(_)) | Some(Custody::Import { .. }) | Some(Custody::Closed) => {
                pending.staged = true;
                pending.goal = None;
                pending.bridge = None;
                shared.set_model_goal_custody(&rt.key, Custody::Closed);
                shared.retire_carried_model_goal(&rt.key);
                return;
            }
            None => {
                let Some(epoch) = self
                    .goals
                    .as_ref()
                    .and_then(|goals| goals.alab_model_goal_epoch(rt.id))
                else {
                    return;
                };
                let Some(mode_epoch) = mode_epoch else {
                    return;
                };
                if !epoch.is_initial() {
                    pending.staged = true;
                    pending.goal = None;
                    pending.bridge = None;
                    shared.set_model_goal_custody(&rt.key, Custody::Closed);
                    shared.retire_carried_model_goal(&rt.key);
                    return;
                }
                shared.set_model_goal_custody(
                    &rt.key,
                    Custody::Import {
                        epoch: epoch.clone(),
                        mode_epoch,
                    },
                );
                Some((epoch, mode_epoch))
            }
        };
        let goal = shared.carried_model_goal(&rt.key).or_else(|| {
            shared
                .pace(&rt.key)
                .filter(|pace| pace_handed(shared, &rt.key, pace))
                .and_then(|pace| pace.model_blocked_goal)
        });
        if let Some(goal) = goal {
            pending.staged = true;
            pending.bridge = lease.map(|(epoch, mode_epoch)| ImportBridge {
                goal: goal.clone(),
                epoch,
                mode_epoch,
            });
            pending.goal = Some(goal);
        }
    }

    /// Called at the native publication boundary. History can reject a carried
    /// source synchronously, but only the async native-permit path may attach a
    /// live epoch and publish continuation authority.
    pub(super) fn seed_model_goal_stop(
        &self,
        rt: &ThreadRt,
        history: &[codex_history::RolloutItem],
    ) {
        let Some(_sources) = self.model_goal_mode_current(rt) else {
            return;
        };
        let mut pending = lock(&rt.model_goal_import);
        let Some(goal) = pending.goal.as_ref() else {
            return;
        };
        if goal.person_stops != self.sup.shared().person_goal_stops(&rt.key)
            || crate::resume_history::model_goal_stop_revoked_since(history, rt.id, goal)
        {
            let retired = pending.bridge.as_ref().map(|bridge| Binding {
                goal: bridge.goal.clone(),
                epoch: bridge.epoch.clone(),
                mode_epoch: bridge.mode_epoch,
            });
            pending.goal.take();
            pending.bridge = None;
            pending.staged = true;
            drop(pending);
            if let Some(retired) = retired {
                self.sup.shared().model_goal_stops(&rt.key).retire(&retired);
            }
            self.sup
                .shared()
                .set_model_goal_custody(&rt.key, Custody::Closed);
            self.sup.shared().retire_carried_model_goal(&rt.key);
        }
    }

    pub(super) async fn restore_model_goal_stop(&self, rt: &Arc<ThreadRt>) -> Result<(), String> {
        self.stage_model_goal_stop(rt);
        let (pending, bridge) = {
            let Some(_sources) = self.model_goal_mode_current(rt) else {
                return Ok(());
            };
            let import = lock(&rt.model_goal_import);
            (import.goal.clone(), import.bridge.clone())
        };
        let Some(pending) = pending else {
            return Ok(());
        };
        let Some(mode_epoch) = codex_alab_core::mode_epoch() else {
            return Ok(());
        };
        let close = || {
            let _ = self.with_current_runtime_and_threads(rt, |kept| {
                let Some(sources) = self.model_goal_mode_current(rt) else {
                    return;
                };
                let mut current = lock(&rt.model_goal_import);
                if current.goal.as_ref() == Some(&pending) {
                    let retired = current.bridge.as_ref().map(|bridge| Binding {
                        goal: bridge.goal.clone(),
                        epoch: bridge.epoch.clone(),
                        mode_epoch: bridge.mode_epoch,
                    });
                    current.goal.take();
                    current.bridge = None;
                    current.staged = true;
                    drop(current);
                    if let Some(retired) = retired {
                        self.sup.shared().model_goal_stops(&rt.key).retire(&retired);
                    }
                    self.sup
                        .shared()
                        .set_model_goal_custody(&rt.key, Custody::Closed);
                    self.sup.shared().retire_carried_model_goal(&rt.key);
                    drop(sources);
                    self.sync_pace_from(kept, rt);
                }
            });
        };
        if pending.person_stops != self.sup.shared().person_goal_stops(&rt.key) {
            close();
            return Ok(());
        }
        let goals = self
            .goals
            .as_ref()
            .ok_or("the model goal's native service is unavailable")?;
        let runtime = goals
            .alab_goal_runtime(rt.id)
            .ok_or("the model goal's native runtime is unavailable")?;
        let _permit = runtime.alab_automatic_input_permit().await?;
        let Some(epoch) = runtime.alab_goal_mutation_epoch() else {
            close();
            return Ok(());
        };
        let bridge_matches = bridge.as_ref().is_some_and(|bridge| {
            bridge.epoch == epoch
                && bridge.mode_epoch == mode_epoch
                && bridge.goal.goal_id == pending.goal_id
                && bridge.goal.objective == pending.objective
                && bridge.goal.token_budget == pending.token_budget
                && bridge.goal.person_stops == pending.person_stops
        });
        if let Some(bridge) = &bridge {
            if bridge.epoch != epoch {
                // A later native mutation may still be awaiting its exact
                // callback. The stale bridge is passive and may be advanced by
                // AcceptedStops, so neither bind it nor retire its origin yet.
                return Ok(());
            }
            if !bridge_matches {
                close();
                return Ok(());
            }
        } else if !runtime.alab_goal_mutation_epoch_is_pristine(&epoch) {
            close();
            return Ok(());
        }
        let thread = self
            .thread(rt.id)
            .await
            .ok_or("the model goal's native history is unavailable")?;
        let codex_home = thread.config().await.codex_home.clone();
        let chronology_allows = matches!(
            codex_alab_core::goal_chronology_fence::read_current(
                &codex_home,
                &rt.id.to_string(),
            ),
            Ok(generation) if generation == pending.chronology_fence_generation
        );
        if !chronology_allows {
            close();
            return Ok(());
        } // An exact later model stop may cross only the host generation it observed under the native mutation permit.
        let history = resumed_history(&thread).await;
        #[cfg(test)]
        let history = match lock(&self.history_seam).clone() {
            Some(items) => items,
            None => history,
        };
        let history = history?;
        let current = thread
            .state_db()
            .ok_or("the model goal's native database is unavailable")?
            .thread_goals()
            .get_thread_goal(rt.id)
            .await
            .map_err(|error| error.to_string())?;
        if crate::resume_history::model_goal_stop_revoked_since(&history, rt.id, &pending)
            || !current.as_ref().is_some_and(|goal| {
                same_wall_goal(&pending, goal, self.sup.shared().person_goal_stops(&rt.key))
            })
        {
            close();
            return Ok(());
        }
        let expected = ModelGoalBinding {
            goal: pending.clone(),
            epoch: epoch.clone(),
            mode_epoch,
        };
        let published = goals.alab_with_current_goal_mutation_epoch(rt.id, &epoch, || {
            self.with_current_runtime_and_threads(rt, |kept| {
                let Some(sources) = self.model_goal_mode_current(rt) else {
                    return false;
                };
                let mut import = lock(&rt.model_goal_import);
                if import.goal.as_ref() != Some(&pending)
                    || import.bridge.as_ref().map(|bridge| &bridge.epoch)
                        != bridge.as_ref().map(|bridge| &bridge.epoch)
                    || pending.person_stops != self.sup.shared().person_goal_stops(&rt.key)
                    || codex_alab_core::mode_epoch() != Some(mode_epoch)
                {
                    return false;
                }
                import.goal.take();
                import.bridge = None;
                import.staged = true;
                drop(import);
                *lock(&rt.model_blocked_goal) = Some(expected.clone());
                self.sup
                    .shared()
                    .set_model_goal_custody(&rt.key, Custody::Known(expected.passive()));
                self.sup.shared().retire_carried_model_goal(&rt.key);
                // Only a still-unstarted runtime can recover the missing baseline.
                // A reply accepted during the read keeps its own point and owed state.
                if rt.resumed.load(Ordering::SeqCst)
                    && rt.accepted_turns.load(Ordering::SeqCst) == 0
                    && !self.sup.shared().turn_running_on(&rt.key)
                {
                    let mut state = lock(&rt.state);
                    if state.point.is_none() {
                        resume_bound_with(&mut state, &history, false, None, 0, clock(), |_| false);
                        if state.point.is_none() {
                            crate::resume_history::restore_end(&mut state, &history, clock());
                        }
                        rt.books_read.store(true, Ordering::SeqCst);
                    }
                }
                drop(sources);
                self.sync_pace_from(kept, rt);
                true
            }) == Some(true)
        });
        if published == Some(false) {
            close();
            return Ok(());
        }
        if published != Some(true) {
            return Err("the model goal's native owner changed during import".into());
        }
        if !self
            .durably_publish_model_goal(
                rt,
                Some(expected),
                mode_epoch,
                Some(codex_home.to_path_buf()),
            )
            .await
        {
            self.sup.note(format!(
                "{}: the imported model goal source did not settle durably; its chronology fence remains",
                rt.key
            ));
        }
        Ok(())
    }

    /// Persist the exact live source or its exact revocation, and retire only a
    /// matching live source's host fence. Absence is an authoritative source
    /// state too: returning before flushing it could resurrect an older record.
    /// The local source lock precedes the process-wide mode/source lock, matching
    /// every source mutation; the record write and compare-clear are one custody span.
    async fn durably_publish_model_goal(
        &self,
        rt: &Arc<ThreadRt>,
        expected: Option<ModelGoalBinding>,
        mode_epoch: u64,
        codex_home: Option<PathBuf>,
    ) -> bool {
        let rt = Arc::clone(rt);
        let shared = self.sup.shared();
        tokio::task::spawn_blocking(move || {
            let _sources = lock(&rt.model_goal_sources);
            if expected
                .as_ref()
                .is_some_and(|expected| expected.mode_epoch != mode_epoch)
            {
                return false;
            }
            let generation = expected
                .as_ref()
                .and_then(|expected| expected.goal.chronology_fence_generation);
            codex_alab_core::with_model_source_fence(mode_epoch, || {
                let source_is_current = || {
                    lock(&rt.model_blocked_goal).as_ref() == expected.as_ref()
                        && shared
                            .pace(&rt.key)
                            .and_then(|pace| pace.model_blocked_goal)
                            .as_ref()
                            == expected.as_ref().map(|expected| &expected.goal)
                };
                if !source_is_current() || !shared.flush_record() || !source_is_current() {
                    return false;
                }
                let Some(generation) = generation else {
                    return true;
                };
                let Some(codex_home) = codex_home else {
                    return false;
                };
                let thread_id = rt.id.to_string();
                match codex_alab_core::goal_chronology_fence::read_active(&codex_home, &thread_id) {
                    Ok(Some(active)) if active == generation => {
                        codex_alab_core::goal_chronology_fence::clear_if_current(
                            &codex_home,
                            &thread_id,
                            generation,
                        )
                        .unwrap_or(false)
                    }
                    Ok(None) => matches!(
                        codex_alab_core::goal_chronology_fence::read_current(
                            &codex_home,
                            &thread_id,
                        ),
                        Ok(current) if current == Some(generation)
                    ),
                    Ok(Some(_)) | Err(_) => false,
                }
            })
            .unwrap_or(false)
        })
        .await
        .unwrap_or(false)
    }

    pub(super) async fn model_goal_tool_finish(&self, input: ToolFinishInput<'_>) {
        if !input.tool_name.is_default_namespace()
            || ![
                codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                codex_goal_extension::CREATE_GOAL_TOOL_NAME,
            ]
            .contains(&input.tool_name.name.as_str())
        {
            return;
        }
        let committed = codex_alab_core::model_goal_stops::take(
            input.thread_store.level_id(),
            input.turn_id,
            input.call_id,
        );
        // Cancellation during PostToolUse can follow a completed database write.
        // Only its exact acknowledged mutation may publish or revoke custody;
        // an absent/colliding marker cannot overwrite a newer completion.
        let accepted_completion = committed.is_some()
            && matches!(
                input.outcome,
                ToolCallOutcome::Completed { success: true } | ToolCallOutcome::Aborted
            );
        if !codex_alab_core::active() || !accepted_completion {
            return;
        }
        let Some(mut committed) = committed else {
            return;
        };
        let Some(track) = input.turn_store.get::<TurnTrack>() else {
            return;
        };
        let Some(rt) = Self::rt(input.thread_store) else {
            return;
        };
        let stops_at_start = lock(&track.0).person_stops_at_start;
        if let Some(stop) = committed.stop.as_mut() {
            stop.goal.person_stops = stops_at_start;
        }
        let shared = self.sup.shared();
        let accepted = shared.model_goal_stops(&rt.key);
        let mut accepted_fact = false;
        // Accept the invocation-qualified callback before sampling a current
        // target. A concurrent later mutation may make this callback stale for
        // action while its exact edge is still needed by another callback.
        if self.with_current_runtime_and_threads(&rt, |_| {
            if shared.person_goal_stops(&rt.key) == stops_at_start {
                accepted_fact = accepted.accept(&committed);
            }
            true
        }) != Some(true)
        {
            return;
        }
        let Some(goals) = &self.goals else {
            return;
        };
        let Some(runtime) = goals.alab_goal_runtime(rt.id) else {
            return;
        };
        let Ok(_permit) = runtime.alab_model_goal_epoch_permit().await else {
            return;
        };
        let Some(target) = runtime.alab_goal_mutation_epoch() else {
            return;
        };

        // This read is an authority check for publication, not for retaining
        // exact callback evidence. A later callback can complete an out-of-order
        // chain, but no chain acts until its current host generation is readable.
        let current_generation = if committed.stop.is_some() {
            #[cfg(test)]
            let injected_failure = lock(&self.model_goal_generation_read_failure).take();
            #[cfg(test)]
            let read = match injected_failure {
                Some(error) => Err(error),
                None => runtime.alab_goal_chronology_generation().await,
            };
            #[cfg(not(test))]
            let read = runtime.alab_goal_chronology_generation().await;
            Some(read)
        } else {
            None
        };

        let mode_epoch = committed.mode_epoch;
        let mut expected_publication: Option<Option<ModelGoalBinding>> = None;
        let mut staged_publication = false;
        let published = goals.alab_with_current_goal_mutation_epoch(rt.id, &target, || {
            if codex_alab_core::mode_epoch() != Some(mode_epoch) {
                return false;
            }
            // Goal identity is not runtime identity: the registry epoch and the
            // current autopilot observer must both still be exact owners.
            self.with_current_runtime_and_threads(&rt, |kept| {
                let callback_current = committed.transition.after == target;
                let duplicate_stop = accepted_fact
                    && committed
                        .stop
                        .as_ref()
                        .is_some_and(|stop| stop.previous_status == PreviousStatus::Blocked);
                let custody_before = shared.model_goal_custody(&rt.key);
                let import_seed = duplicate_stop
                    .then(|| {
                        let stop = committed.stop.as_ref()?;
                        let source = match &custody_before {
                            Some(Custody::Import {
                                epoch,
                                mode_epoch: import_mode,
                            }) if *import_mode == mode_epoch => Binding {
                                goal: stop.goal.clone(),
                                epoch: epoch.clone(),
                                mode_epoch,
                            },
                            None if committed.transition.before.is_initial() => Binding {
                                goal: stop.goal.clone(),
                                epoch: committed.transition.before.clone(),
                                mode_epoch,
                            },
                            Some(Custody::Known(_))
                            | Some(Custody::Import { .. })
                            | Some(Custody::Closed)
                            | None => return None,
                        };
                        accepted.advance(&source, &target)
                    })
                    .flatten();
                if let Some(import) = &import_seed {
                    let seeded = {
                        let Some(sources) = self.model_goal_mode_current(&rt) else {
                            return false;
                        };
                        let open = lock(&rt.model_blocked_goal).is_none()
                            && !lock(&rt.model_goal_import).staged;
                        if open {
                            shared.set_model_goal_custody(
                                &rt.key,
                                Custody::Import {
                                    epoch: import.epoch.clone(),
                                    mode_epoch,
                                },
                            );
                        }
                        drop(sources);
                        open
                    };
                    if !seeded {
                        return false;
                    }
                }
                let live_before = lock(&rt.model_blocked_goal).is_some();
                let defer_unresolved_import = duplicate_stop
                    && !live_before
                    && custody_before.is_none()
                    && import_seed.is_none();
                if !defer_unresolved_import {
                    self.stage_model_goal_stop(&rt);
                }
                let Some(sources) = self.model_goal_mode_current(&rt) else {
                    return false;
                };
                let stops = shared.person_goal_stops(&rt.key);
                let prior_live = lock(&rt.model_blocked_goal).clone();
                let custody = shared.model_goal_custody(&rt.key);
                if !callback_current && !accepted_fact {
                    drop(sources);
                    return true;
                }

                let resolved = (stops == stops_at_start)
                    .then(|| accepted.source(&target, mode_epoch))
                    .flatten()
                    .filter(|source| source.binding.goal.person_stops == stops);
                let own_fresh = resolved.as_ref().is_some_and(|source| {
                    committed.stop.as_ref().is_some_and(|stop| {
                        stop.previous_status == PreviousStatus::Active
                            && source.origin.epoch == committed.transition.after
                            && source.origin.goal == stop.goal
                    })
                });
                let advanced_prior = prior_live
                    .as_ref()
                    .map(ModelGoalBinding::passive)
                    .and_then(|prior| accepted.advance(&prior, &target));
                let advanced_custody = match custody {
                    Some(Custody::Known(prior))
                        if prior.mode_epoch == mode_epoch && prior.goal.person_stops == stops =>
                    {
                        accepted.advance(&prior, &target)
                    }
                    Some(Custody::Import {
                        epoch,
                        mode_epoch: import_mode,
                    }) if import_mode == mode_epoch => {
                        let pending = lock(&rt.model_goal_import);
                        pending.goal.as_ref().and_then(|goal| {
                            accepted.advance(
                                &Binding {
                                    goal: goal.clone(),
                                    epoch,
                                    mode_epoch: import_mode,
                                },
                                &target,
                            )
                        })
                    }
                    Some(Custody::Known(_))
                    | Some(Custody::Import { .. })
                    | Some(Custody::Closed)
                    | None => None,
                };
                let advanced_bridge =
                    lock(&rt.model_goal_import)
                        .bridge
                        .as_ref()
                        .and_then(|bridge| {
                            accepted.advance(
                                &Binding {
                                    goal: bridge.goal.clone(),
                                    epoch: bridge.epoch.clone(),
                                    mode_epoch: bridge.mode_epoch,
                                },
                                &target,
                            )
                        });
                let candidate = resolved
                    .map(|source| source.binding)
                    .or_else(|| advanced_prior.clone())
                    .or(advanced_custody)
                    .or(advanced_bridge)
                    .filter(|binding| {
                        accepted_fact
                            && stops == stops_at_start
                            && binding.mode_epoch == mode_epoch
                            && binding.goal.person_stops == stops
                    });
                let direct = candidate.as_ref().is_some_and(|candidate| {
                    own_fresh || advanced_prior.as_ref() == Some(candidate)
                });
                let chronology_current = candidate.as_ref().is_some_and(|candidate| {
                    current_generation.as_ref().is_some_and(|read| {
                        read.as_ref().is_ok_and(|generation| {
                            generation == &candidate.goal.chronology_fence_generation
                        })
                    })
                });
                let duplicate_generation_current = duplicate_stop
                    && stops == stops_at_start
                    && committed.stop.as_ref().is_some_and(|stop| {
                        current_generation.as_ref().is_some_and(|read| {
                            read.as_ref().is_ok_and(|generation| {
                                generation == &stop.goal.chronology_fence_generation
                            })
                        })
                    });

                *lock(&rt.model_goal_mode_epoch) = Some(mode_epoch);
                match candidate.filter(|_| chronology_current) {
                    Some(candidate) if direct => {
                        let bound = ModelGoalBinding::from_passive(candidate.clone());
                        *lock(&rt.model_blocked_goal) = Some(bound.clone());
                        *lock(&rt.model_goal_import) = PendingImport {
                            staged: true,
                            goal: None,
                            bridge: None,
                        };
                        shared.set_model_goal_custody(&rt.key, Custody::Known(candidate));
                        shared.retire_carried_model_goal(&rt.key);
                        expected_publication = Some(Some(bound));
                    }
                    Some(candidate) => {
                        // A source resolved by another callback cannot skip the
                        // complete-history adoption path. Stage its exact target
                        // and keep it passive until restore validates the record.
                        *lock(&rt.model_blocked_goal) = None;
                        *lock(&rt.model_goal_import) = PendingImport {
                            staged: true,
                            goal: Some(candidate.goal.clone()),
                            bridge: Some(ImportBridge {
                                goal: candidate.goal.clone(),
                                epoch: candidate.epoch.clone(),
                                mode_epoch: candidate.mode_epoch,
                            }),
                        };
                        shared.set_model_goal_custody(&rt.key, Custody::Known(candidate));
                        staged_publication = true;
                    }
                    None if callback_current && duplicate_generation_current => {
                        // An accepted duplicate can remain passive while an
                        // earlier edge or the selected durable source is still
                        // arriving. Exact import custody, when resolved above,
                        // advances without publishing goal authority.
                        drop(sources);
                        return true;
                    }
                    None if callback_current => {
                        // A person stop, mode/chronology failure, non-stop
                        // mutation or incomplete duplicate chain revokes every
                        // actionable source. Accepted facts remain passive so a
                        // later exact callback can resolve the full chain.
                        *lock(&rt.model_blocked_goal) = None;
                        *lock(&rt.model_goal_import) = PendingImport {
                            staged: true,
                            goal: None,
                            bridge: None,
                        };
                        shared.set_model_goal_custody(&rt.key, Custody::Closed);
                        shared.retire_carried_model_goal(&rt.key);
                        expected_publication = Some(None);
                    }
                    None => {
                        // A stale accepted callback supplied evidence but did
                        // not resolve the actual current target. It can neither
                        // publish nor revoke what a newer callback owns.
                        drop(sources);
                        return true;
                    }
                }
                drop(sources);
                self.sync_pace_from(kept, &rt);
                true
            }) == Some(true)
        });
        if published != Some(true) {
            return;
        }

        let settled = if let Some(expected) = expected_publication {
            // Retain the original goal even if this native turn crashes after
            // the tool succeeds and before normal turn-end publication.
            let codex_home = if expected
                .as_ref()
                .and_then(|bound| bound.goal.chronology_fence_generation)
                .is_some()
            {
                match self.thread(rt.id).await {
                    Some(thread) => Some(thread.config().await.codex_home.to_path_buf()),
                    None => None,
                }
            } else {
                None
            };
            self.durably_publish_model_goal(&rt, expected, mode_epoch, codex_home)
                .await
        } else if staged_publication {
            write_through(shared).await
        } else {
            true
        };
        if !settled {
            self.sup.note(format!(
                "{}: the model goal stop's record did not flush; its guarded source remains passive",
                rt.key
            ));
        }
    }

    pub(super) fn retire_model_goal_stop(&self, rt: &ThreadRt, bound: &ModelGoalBinding) {
        let _ = self.with_current_runtime_and_threads(rt, |kept| {
            let Some(sources) = self.model_goal_mode_current(rt) else {
                return;
            };
            let mut current = lock(&rt.model_blocked_goal);
            if current.as_ref() != Some(bound) {
                return;
            }
            current.take();
            drop(current);
            *lock(&rt.model_goal_import) = PendingImport {
                staged: true,
                goal: None,
                bridge: None,
            };
            self.sup
                .shared()
                .model_goal_stops(&rt.key)
                .retire(&bound.passive());
            self.sup
                .shared()
                .set_model_goal_custody(&rt.key, Custody::Closed);
            self.sup.shared().retire_carried_model_goal(&rt.key);
            drop(sources);
            self.sync_pace_from(kept, rt);
        });
    }

    /// Remove stale local action authority without retiring passive callback
    /// evidence. Mutation epochs advance monotonically, so a mismatched binding
    /// cannot become current again; its exact accepted edge may still advance a
    /// later callback to the live epoch.
    fn drop_stale_model_goal_action(&self, rt: &ThreadRt, bound: &ModelGoalBinding) {
        let _ = self.with_current_runtime_and_threads(rt, |kept| {
            let Some(sources) = self.model_goal_mode_current(rt) else {
                return;
            };
            let mut live = lock(&rt.model_blocked_goal);
            if live.as_ref() != Some(bound) {
                return;
            }
            live.take();
            drop(live);
            drop(sources);
            self.sync_pace_from(kept, rt);
        });
    }

    pub(super) async fn current_model_binding(
        &self,
        rt: &Arc<ThreadRt>,
    ) -> Result<Option<ModelGoalBinding>, String> {
        self.restore_model_goal_stop(rt).await?;
        let Some(sources) = self.model_goal_mode_current(rt) else {
            return Ok(None);
        };
        let bound = lock(&rt.model_blocked_goal).clone();
        drop(sources);
        let Some(bound) = bound else {
            return Ok(None);
        };
        if codex_alab_core::mode_epoch() != Some(bound.mode_epoch) {
            self.retire_model_goal_stop(rt, &bound);
            return Ok(None);
        }
        if !self.model_epoch_current(rt.id, &bound.epoch) {
            self.drop_stale_model_goal_action(rt, &bound);
            return Ok(None);
        }
        let thread = self
            .thread(rt.id)
            .await
            .ok_or("the model-blocked goal's native thread is unavailable")?;
        let current = self
            .model_goal_standing(&thread, rt.id, bound.clone())
            .await?;
        if current.is_none() {
            if self.model_epoch_current(rt.id, &bound.epoch) {
                self.retire_model_goal_stop(rt, &bound);
            } else {
                // A later native mutation can finish before its callback. Drop
                // stale action authority without retiring passive custody; the
                // accepted transition ledger may still resolve it exactly.
                self.drop_stale_model_goal_action(rt, &bound);
            }
        }
        Ok(current)
    }

    #[cfg(test)]
    pub(super) async fn current_model_goal(
        &self,
        rt: &Arc<ThreadRt>,
    ) -> Result<Option<ModelGoalBinding>, String> {
        self.current_model_binding(rt).await
    }

    pub(super) async fn model_goal_standing(
        &self,
        thread: &Arc<CodexThread>,
        id: ThreadId,
        bound: ModelGoalBinding,
    ) -> Result<Option<ModelGoalBinding>, String> {
        let goals = self
            .goals
            .as_ref()
            .ok_or("the model-blocked goal's native service is unavailable")?;
        let runtime = goals
            .alab_goal_runtime(id)
            .ok_or("the model-blocked goal's native runtime is unavailable")?;
        let _permit = runtime.alab_automatic_input_permit().await?;
        if codex_alab_core::mode_epoch() != Some(bound.mode_epoch) {
            return Ok(None);
        }
        let codex_home = thread.config().await.codex_home.clone();
        let thread_id = id.to_string();
        let chronology_current = || {
            matches!(
                codex_alab_core::goal_chronology_fence::read_current(&codex_home, &thread_id),
                Ok(generation) if generation == bound.goal.chronology_fence_generation
            )
        };
        if goals
            .alab_with_current_goal_mutation_epoch(id, &bound.epoch, || ())
            .is_none()
            || !chronology_current()
        {
            return Ok(None);
        }
        let Some(db) = thread.state_db() else {
            return Ok(None);
        };
        let current = db
            .thread_goals()
            .get_thread_goal(id)
            .await
            .map_err(|error| error.to_string())?;
        let stops = self.sup.shared().person_goal_stops(&id.to_string());
        if !current
            .as_ref()
            .is_some_and(|goal| same_wall_goal(&bound.goal, goal, stops))
            || goals
                .alab_with_current_goal_mutation_epoch(id, &bound.epoch, || ())
                .is_none()
            || !chronology_current()
            || codex_alab_core::mode_epoch() != Some(bound.mode_epoch)
        {
            return Ok(None);
        }
        Ok(Some(bound))
    }
}
