// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Existing actual configured resources only; no Core or scheduling authority.
use super::*;
use crate::outgoing_message::alab_callback_claim::metadata as callback;
pub(in super::super) struct CallbackConfigured(Arc<ConfiguredProjection>);
impl Binding {
    pub(in super::super) async fn callback_configured_projection(
        &self,
        selected: &callback::ConfiguredProjection,
        origin: &AcceptedOrigin,
    ) -> Result<CallbackConfigured, JSONRPCErrorError> {
        let work = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .configured
            .projections
            .get(&selected.snapshot)
            .cloned()
            .ok_or_else(|| invalid_request("actual configured callback projection unavailable"))?;
        if work.receipt.frontier != selected.frontier
            || work.receipt.runtime != selected.runtime
            || work.receipt.version != selected.version
            || !work.frontier.same_receipt(origin)
        {
            return Err(invalid_request(
                "configured callback receipt changed actual runtime/A",
            ));
        }
        work.wait_ready(&self.input).await?;
        let current = self
            .backend
            .observe_reader_origin(origin)
            .await
            .map_err(native_error)?;
        if !current.same_receipt(origin) {
            return Err(invalid_request(
                "configured callback A is no longer current",
            ));
        }
        self.current_configured(&work)?;
        Ok(CallbackConfigured(work))
    }
    pub(in super::super) fn callback_configured_committed(
        &self,
        actual: &CallbackConfigured,
    ) -> Result<(), JSONRPCErrorError> {
        self.current_configured(&actual.0)?;
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !state
            .configured
            .commits
            .get(&actual.0.receipt.snapshot)
            .is_some_and(|action| {
                Arc::ptr_eq(&action.projection, &actual.0)
                    && !action.completion.uncertain.load(Ordering::Acquire)
                    && matches!(action.completion.result.borrow().as_ref(), Some(Ok(())))
            })
        {
            return Err(invalid_request(
                "configured callback ACK needs actual ConfigurationCommit",
            ));
        }
        Ok(())
    }
}
