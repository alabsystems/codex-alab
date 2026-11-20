// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::outgoing_message::alab_initial_claim::SavedResumeOutcome;

impl ThreadRequestProcessor {
    pub(crate) async fn alab_read_saved_resume(
        &self,
        context: &RequestContext,
        original: &Prepare,
        offer: &Offer,
    ) -> Result<Completion, JSONRPCErrorError> {
        let owner = self.alab_initial_session.actual()?;
        owner.open()?;
        let origin = actual_origin(context)?;
        owner.observe_bound_side_origin(&origin).await?;
        owner.open()?;
        let child = original
            .parameters
            .get("threadId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| invalid_request("saved resume child missing"))?;
        Ok(Completion::SavedResume {
            offer: offer.clone(),
            thread_id: child.into(),
            outcome: owner.saved_resume_outcome(original, offer, child),
        })
    }
}

impl InitialSessionOperation {
    fn saved_resume_outcome(
        &self,
        original: &Prepare,
        offer: &Offer,
        child: &str,
    ) -> SavedResumeOutcome {
        if original
            .parameters
            .get("threadId")
            .and_then(serde_json::Value::as_str)
            != Some(child)
        {
            return SavedResumeOutcome::Unavailable {};
        }
        let live_outcome = |state: &State| {
            if state.closed || self.open().is_err() {
                return Some(SavedResumeOutcome::Unavailable {});
            }
            let scope = state.clients.get(&offer.claim)?;
            Some(scope.live_saved_resume(original, offer, child))
        };
        {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(outcome) = live_outcome(&state) {
                return outcome;
            }
            if state.retired_saved_resumes.is_empty() {
                return SavedResumeOutcome::Unavailable {};
            }
        }
        // Hash outside admission's mutex and only after a live miss. Recheck
        // admission and live ownership under the same lock that retires a scope.
        let Some(key) = saved_resume_receipts::key(original, offer) else {
            return SavedResumeOutcome::Unavailable {};
        };
        let receipt = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(outcome) = live_outcome(&state) {
                return outcome;
            }
            state.retired_saved_resumes.get(&key)
        };
        receipt.map_or(SavedResumeOutcome::Unavailable {}, |receipt| {
            receipt.outcome()
        })
    }
}

impl Scope {
    fn live_saved_resume(
        &self,
        original: &Prepare,
        offer: &Offer,
        child: &str,
    ) -> SavedResumeOutcome {
        if self.prepared_offer.as_ref() != Some(offer)
            || !self.frozen.matches_saved_resume(original, offer)
        {
            return SavedResumeOutcome::Unavailable {};
        }
        let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if progress.worker_lost || progress.selection_unknown {
            return SavedResumeOutcome::Unavailable {};
        }
        match &progress.complete {
            None => SavedResumeOutcome::Pending {},
            Some(Reply::Resume(response, _)) if response.thread.id == child => {
                SavedResumeOutcome::Resumed {}
            }
            Some(Reply::Error(error)) => SavedResumeOutcome::Rejected {
                code: error.code,
                message: error.message.clone(),
                had_data: error.data.is_some(),
                value: error.data.clone().unwrap_or(serde_json::Value::Null),
            },
            Some(_) => SavedResumeOutcome::Unavailable {},
        }
    }

    /// The registry calls this only after original policy completion and any
    /// captured Core's termination. The receipt holds the policy observation.
    pub(in super::super) fn retired_saved_resume(
        &self,
        offer: &Offer,
    ) -> Option<(saved_resume_receipts::Key, saved_resume_receipts::Receipt)> {
        let (key, child) = self.frozen.saved_resume_key(offer)?;
        let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if progress.worker_lost || progress.selection_unknown {
            return None;
        }
        let receipt = match progress.complete.as_ref()? {
            Reply::Resume(response, _) if response.thread.id == child => {
                saved_resume_receipts::Receipt::resumed()
            }
            Reply::Error(error) => saved_resume_receipts::Receipt::rejected(error)?,
            _ => return None,
        };
        Some((key, receipt))
    }
}

#[cfg(all(test, unix))]
#[path = "saved_resume_lookup_tests.rs"]
mod tests;
