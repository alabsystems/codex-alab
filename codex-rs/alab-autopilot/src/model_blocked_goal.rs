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

#[derive(Default)]
pub(super) struct PendingImport {
    /// Resolved or explicitly superseded sources never reopen this runtime.
    staged: bool,
    pub(super) goal: Option<codex_alab_core::session::WallGoal>,
    pub(super) lease: Option<MutationEpoch>,
}

impl Autopilot {
    pub(super) fn model_epoch_current(&self, id: ThreadId, epoch: &MutationEpoch) -> bool {
        self.goals
            .as_ref()
            .is_some_and(|goals| goals.alab_model_goal_epoch_current(id, epoch))
    }

    fn close_model_import(&self, rt: &ThreadRt) {
        *lock(&rt.model_goal_import) = PendingImport {
            staged: true,
            goal: None,
            lease: None,
        };
        self.sup
            .shared()
            .set_model_goal_custody(&rt.key, Custody::Closed);
        self.sup.shared().retire_carried_model_goal(&rt.key);
    }

    /// Preserve the exact native lease before a history await. Known custody
    /// transfers whole across runtime replacement, never through a new epoch.
    pub(super) fn stage_model_goal_stop(&self, rt: &ThreadRt) {
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
        match shared.model_goal_custody(&rt.key) {
            Some(Custody::Known(binding)) => {
                // Replacement keeps the exact native lease, but still reads
                // the complete history before adopting its inherited source.
                pending.goal = Some(binding.goal);
                pending.lease = Some(binding.epoch);
                pending.staged = true;
                return;
            }
            Some(Custody::Import(lease)) => pending.lease = Some(lease),
            Some(Custody::Closed) => {
                pending.staged = true;
                pending.goal = None;
                pending.lease = None;
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
                if !epoch.is_initial() {
                    drop(pending);
                    self.close_model_import(rt);
                    return;
                }
                shared.set_model_goal_custody(&rt.key, Custody::Import(epoch.clone()));
                pending.lease = Some(epoch);
            }
        }
        let goal = shared.carried_model_goal(&rt.key).or_else(|| {
            shared
                .pace(&rt.key)
                .filter(|pace| pace_handed(shared, &rt.key, pace))
                .and_then(|pace| pace.model_blocked_goal)
        });
        if let Some(goal) = goal {
            pending.staged = true;
            pending.goal = Some(goal);
        }
    }

    /// A stale lease remains passive until its exact in-flight duplicate can
    /// settle. It cannot bind or be replaced by whatever epoch is current now.
    pub(super) fn seed_model_goal_stop(
        &self,
        rt: &ThreadRt,
        history: &[codex_history::RolloutItem],
    ) {
        let mut pending = lock(&rt.model_goal_import);
        let (Some(goal), Some(lease)) = (pending.goal.clone(), pending.lease.clone()) else {
            return;
        };
        if !self.model_epoch_current(rt.id, &lease) {
            return;
        }
        pending.goal = None;
        pending.lease = None;
        drop(pending);
        let binding = Binding { goal, epoch: lease };
        if binding.goal.person_stops == self.sup.shared().person_goal_stops(&rt.key)
            && !crate::resume_history::model_goal_stop_revoked_since(history, rt.id, &binding.goal)
        {
            *lock(&rt.model_blocked_goal) = Some(binding.clone());
            self.sup
                .shared()
                .set_model_goal_custody(&rt.key, Custody::Known(binding));
            self.sup.shared().retire_carried_model_goal(&rt.key);
        } else {
            self.sup.shared().model_goal_stops(&rt.key).retire(&binding);
            self.close_model_import(rt);
        }
    }

    pub(super) async fn restore_model_goal_stop(&self, rt: &ThreadRt) -> Result<(), String> {
        self.stage_model_goal_stop(rt);
        let (pending, lease) = {
            let pending = lock(&rt.model_goal_import);
            (pending.goal.clone(), pending.lease.clone())
        };
        let (Some(pending), Some(lease)) = (pending, lease) else {
            return Ok(());
        };
        if pending.person_stops != self.sup.shared().person_goal_stops(&rt.key) {
            let _ = self.with_current_runtime_and_threads(rt, |kept| {
                let current = lock(&rt.model_goal_import);
                if current.goal.as_ref() == Some(&pending) && current.lease.as_ref() == Some(&lease)
                {
                    drop(current);
                    self.sup
                        .shared()
                        .model_goal_stops(&rt.key)
                        .retire(&Binding {
                            goal: pending.clone(),
                            epoch: lease.clone(),
                        });
                    self.close_model_import(rt);
                    self.sync_pace_from(kept, rt);
                }
            });
            return Ok(());
        }
        if !self.model_epoch_current(rt.id, &lease) {
            return Ok(());
        }
        let thread = self
            .thread(rt.id)
            .await
            .ok_or("the model goal's native history is unavailable")?;
        let history = resumed_history(&thread).await;
        #[cfg(test)]
        let history = match lock(&self.history_seam).clone() {
            Some(items) => items,
            None => history,
        };
        let history = history?;
        let _ = self.with_current_runtime_and_threads(rt, |kept| {
            let current = lock(&rt.model_goal_import);
            if current.goal.as_ref() != Some(&pending) || current.lease.as_ref() != Some(&lease) {
                return;
            }
            drop(current);
            self.seed_model_goal_stop(rt, &history);
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
            self.sync_pace_from(kept, rt);
        });
        Ok(())
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
        let accepted = matches!(
            (input.outcome, committed.as_ref()),
            (ToolCallOutcome::Completed { success: true }, _) | (ToolCallOutcome::Aborted, Some(_))
        );
        if !codex_alab_core::active() || !accepted {
            return;
        }
        let Some(track) = input.turn_store.get::<TurnTrack>() else {
            return;
        };
        let Some(rt) = Self::rt(input.thread_store) else {
            return;
        };
        if self.with_current_runtime_and_threads(&rt, |kept| {
            let stops = self.sup.shared().person_goal_stops(&rt.key);
            let stops_at_start = lock(&track.0).person_stops_at_start;
            let shared = self.sup.shared();
            // The first unknown duplicate may carry the untouched initial lease
            // across its own mutation, without granting old source authority.
            if let Some(stop) = &committed
                && stop.previous_status == PreviousStatus::Blocked
                && stops == stops_at_start
                && stop.receipt.before.is_initial()
                && shared.model_goal_custody(&rt.key).is_none()
                && !lock(&rt.model_goal_import).staged
            {
                shared
                    .set_model_goal_custody(&rt.key, Custody::Import(stop.receipt.before.clone()));
            }
            self.stage_model_goal_stop(&rt);
            let custody = shared.model_goal_custody(&rt.key);
            let Some(mut stop) = committed else {
                self.sync_pace_from(kept, &rt);
                return true;
            };
            if stops != stops_at_start {
                if self.model_epoch_current(rt.id, &stop.receipt.after) {
                    *lock(&rt.model_blocked_goal) = None;
                    self.close_model_import(&rt);
                }
                self.sync_pace_from(kept, &rt);
                return true;
            }
            stop.goal.person_stops = stops_at_start;
            let accepted = shared.model_goal_stops(&rt.key);
            // A committed marker is not accepted until this callback. Keep its
            // exact source/edge even while another dispatcher owns a mutation.
            accepted.accept(&stop);
            let Some(target) = self
                .goals
                .as_ref()
                .and_then(|goals| goals.alab_model_goal_epoch(rt.id))
            else {
                self.sync_pace_from(kept, &rt);
                return true;
            };
            let prior_live = lock(&rt.model_blocked_goal).clone();
            let resolved = accepted
                .source(&target)
                .filter(|source| source.origin.goal.person_stops == stops);
            let own_fresh = resolved.as_ref().is_some_and(|source| {
                stop.previous_status == PreviousStatus::Active
                    && source.origin.epoch == stop.receipt.after
                    && source.origin.goal == stop.goal
            });
            let prior_validated = match &custody {
                Some(Custody::Known(prior))
                    if prior_live.as_ref() == Some(prior) && prior.goal.person_stops == stops =>
                {
                    accepted
                        .advance(&prior.goal, &prior.epoch, &target)
                        .is_some()
                }
                _ => false,
            };
            let next = resolved
                .map(|source| {
                    Custody::Known(Binding {
                        goal: source.origin.goal,
                        epoch: source.epoch,
                    })
                })
                .or_else(|| match custody {
                    Some(Custody::Known(mut prior)) if prior.goal.person_stops == stops => {
                        prior.epoch = accepted.advance(&prior.goal, &prior.epoch, &target)?;
                        Some(Custody::Known(prior))
                    }
                    Some(Custody::Import(lease))
                        if stop.previous_status == PreviousStatus::Blocked =>
                    {
                        let pending = lock(&rt.model_goal_import);
                        let goal = pending.goal.as_ref().unwrap_or(&stop.goal);
                        if goal.goal_id != stop.goal.goal_id
                            || goal.objective != stop.goal.objective
                            || goal.token_budget != stop.goal.token_budget
                            || goal.person_stops != stops
                        {
                            return None;
                        }
                        accepted.advance(goal, &lease, &target).map(Custody::Import)
                    }
                    _ => None,
                });
            // Native intent may race this local resolution. Passive facts stay
            // retained, but only the actual registered current epoch can publish.
            if !self.model_epoch_current(rt.id, &target) {
                self.sync_pace_from(kept, &rt);
                return true;
            }
            match next {
                Some(Custody::Known(binding)) => {
                    if own_fresh
                        || (prior_validated
                            && prior_live
                                .as_ref()
                                .is_some_and(|prior| prior.goal == binding.goal))
                    {
                        *lock(&rt.model_blocked_goal) = Some(binding.clone());
                        *lock(&rt.model_goal_import) = PendingImport {
                            staged: true,
                            goal: None,
                            lease: None,
                        };
                        shared.retire_carried_model_goal(&rt.key);
                    } else {
                        // A later callback can resolve an accepted origin, but
                        // cannot skip full history adoption of that source.
                        *lock(&rt.model_blocked_goal) = None;
                        *lock(&rt.model_goal_import) = PendingImport {
                            staged: true,
                            goal: Some(binding.goal.clone()),
                            lease: Some(binding.epoch.clone()),
                        };
                    }
                    shared.set_model_goal_custody(&rt.key, Custody::Known(binding));
                }
                Some(Custody::Import(lease)) => {
                    lock(&rt.model_goal_import).lease = Some(lease.clone());
                    shared.set_model_goal_custody(&rt.key, Custody::Import(lease));
                }
                Some(Custody::Closed) | None => {
                    // A newer duplicate may finish before the source or an
                    // earlier edge. Preserve passive custody until those exact
                    // callbacks arrive; stale epochs grant no action.
                    if stop.previous_status == PreviousStatus::Other && stop.receipt.after == target
                    {
                        *lock(&rt.model_blocked_goal) = None;
                        self.close_model_import(&rt);
                    }
                }
            }
            self.sync_pace_from(kept, &rt);
            true
        }) != Some(true)
        {
            return;
        }
        if !write_through(self.sup.shared()).await {
            self.sup.note(format!(
                "{}: the model goal stop's record did not flush; the live binding remains guarded",
                rt.key
            ));
        }
    }

    pub(super) fn retire_model_goal_stop(&self, rt: &ThreadRt, bound: &Binding) {
        let _ = self.with_current_runtime_and_threads(rt, |kept| {
            let mut current = lock(&rt.model_blocked_goal);
            if current.as_ref() != Some(bound) {
                return;
            }
            current.take();
            drop(current);
            self.sup.shared().model_goal_stops(&rt.key).retire(bound);
            self.close_model_import(rt);
            self.sync_pace_from(kept, rt);
        });
    }

    pub(super) async fn current_model_binding(
        &self,
        rt: &ThreadRt,
    ) -> Result<Option<Binding>, String> {
        self.restore_model_goal_stop(rt).await?;
        let Some(bound) = lock(&rt.model_blocked_goal).clone() else {
            return Ok(None);
        };
        if !self.model_epoch_current(rt.id, &bound.epoch) {
            return Ok(None);
        }
        let thread = self
            .thread(rt.id)
            .await
            .ok_or("the model-blocked goal's native thread is unavailable")?;
        let current = self
            .model_goal_standing(&thread, rt.id, bound.clone())
            .await?;
        if current.is_none() && self.model_epoch_current(rt.id, &bound.epoch) {
            self.retire_model_goal_stop(rt, &bound);
        }
        Ok(current)
    }

    #[cfg(test)]
    pub(super) async fn current_model_goal(
        &self,
        rt: &ThreadRt,
    ) -> Result<Option<codex_alab_core::session::WallGoal>, String> {
        Ok(self
            .current_model_binding(rt)
            .await?
            .map(|binding| binding.goal))
    }

    pub(super) async fn model_goal_standing(
        &self,
        thread: &Arc<CodexThread>,
        id: ThreadId,
        bound: Binding,
    ) -> Result<Option<Binding>, String> {
        let Some(db) = thread.state_db() else {
            return Ok(None);
        };
        let current = db
            .thread_goals()
            .get_thread_goal(id)
            .await
            .map_err(|error| error.to_string())?;
        let stops = self.sup.shared().person_goal_stops(&id.to_string());
        Ok(current
            .filter(|goal| {
                self.model_epoch_current(id, &bound.epoch)
                    && same_wall_goal(&bound.goal, goal, stops)
            })
            .map(|_| bound))
    }
}
