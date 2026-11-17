// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! SAME canonical weak UI slot for genuine configured/NoPrimary ownership.
//! Configuration completion never releases startup submissions or Goal Hold.
use super::*;
use crate::startup_draft::alab_configured::Pending as Configured;
impl FrontendTenure {
    pub(crate) fn start_configured_begin(
        &self,
        begin: &Arc<crate::owned_local_entered_readers::constructor::configured_begin::Begin>,
    ) -> io::Result<()> {
        // SAME actual delivery close gate: registration+spawn+stored handle are
        // atomic with closing admission, without a strong delivery/task cycle.
        let close = self
            .delivery
            .close
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if close.is_some() {
            return Err(unavailable());
        }
        self.delivery
            .configured_begins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Arc::downgrade(begin));
        begin.start_registered()?;
        drop(close);
        Ok(())
    }

    pub(crate) fn install_configured_frontier(&self, pending: &Arc<Configured>) -> io::Result<()> {
        if !pending.belongs_to(self) || self.delivery.closing() {
            return Err(unavailable());
        }
        self.retain_runtime_frontier(pending.native_frontier())?;
        let status = pending.ui_status();
        if !status.is_active() {
            return Err(unavailable());
        }
        let retired = {
            let mut slot = self
                .delivery
                .ui_frontier
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if slot
                .as_ref()
                .is_some_and(|prior| !prior.status.is_complete())
            {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "prior configured/projected UI remains unobserved",
                ));
            }
            slot.replace(UiSlot {
                owner: UiOwner::Configured(Arc::downgrade(pending)),
                status,
            })
        };
        drop(retired);
        Ok(())
    }
    pub(crate) fn is_configured_frontier(&self, pending: &Configured) -> bool {
        if !pending.belongs_to(self) || self.delivery.closing() {
            return false;
        }
        let status = pending.ui_status();
        self.delivery.ui_frontier.lock().unwrap_or_else(PoisonError::into_inner).as_ref().is_some_and(|slot|
            Arc::ptr_eq(&slot.status,&status) && matches!(&slot.owner,UiOwner::Configured(actual) if actual.upgrade().is_some_and(|actual|std::ptr::eq(Arc::as_ref(&actual),pending))))
    }
    /// Called only after actual ConfigurationCommit and local state validation.
    /// Prior/pending actual SDK owners survive every earlier uncertainty.
    pub(crate) fn finish_configured_frontier(&self, pending: &Arc<Configured>) -> io::Result<()> {
        if !pending.belongs_to(self) || self.delivery.closing() {
            return Err(unavailable());
        }
        let status = pending.ui_status();
        let retired = {
            let slot = self
                .delivery
                .ui_frontier
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let slot = slot.as_ref().ok_or_else(unavailable)?;
            if !Arc::ptr_eq(&slot.status, &status)
                || !status.is_active()
                || !matches!(&slot.owner,UiOwner::Configured(actual) if actual.upgrade().is_some_and(|actual|Arc::ptr_eq(&actual,pending)))
            {
                return Err(unavailable());
            }
            let mut owners = self
                .delivery
                .app_owners
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if pending.native_frontier().go_data().is_some() {
                if !owners
                    .pending
                    .as_ref()
                    .is_some_and(|owner| Arc::ptr_eq(&owner.status, &status))
                {
                    return Err(unavailable());
                }
                let promoted = owners.pending.take().ok_or_else(unavailable)?;
                owners._committed.replace(promoted.owner)
            } else {
                if owners.pending.is_some() {
                    return Err(unavailable());
                }
                None
            }
        };
        drop(retired); // Never drop Native ownership under UI/App maps.
        Ok(())
    }
}
