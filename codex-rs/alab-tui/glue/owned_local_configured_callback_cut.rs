// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Resource-only pre-App callback ownership on SAME configured Pending. The
//! owner references Pending weakly; no Pending→transfer→Pending strong cycle.
use super::*;
pub(super) enum Owner {
    Unprepared,
    Original,
    Entered(Arc<crate::app::RetainedStartupCallbacks>),
}
impl Pending {
    fn callback_owner(
        self: &Arc<Self>,
        session: &AppServerSession,
        original: &crate::owned_local_entered_readers::constructor::Constructor,
    ) -> io::Result<Option<Arc<crate::app::RetainedStartupCallbacks>>> {
        if !self.current_on(session) {
            return Err(unavailable());
        }
        {
            let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
            match &progress._callbacks {
                Owner::Original => return Ok(None),
                Owner::Entered(actual) => return Ok(Some(actual.clone())),
                Owner::Unprepared => {}
            }
        }
        let actual = original.prepare_startup_callbacks(session, self)?;
        let mut progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if !matches!(progress._callbacks, Owner::Unprepared) {
            return Err(unavailable());
        }
        progress._callbacks = match &actual {
            Some(owner) => Owner::Entered(owner.clone()),
            None => Owner::Original,
        };
        Ok(actual)
    }
    pub(super) async fn observe_callback_cut(
        self: &Arc<Self>,
        session: &AppServerSession,
        original: &crate::owned_local_entered_readers::constructor::Constructor,
    ) -> io::Result<()> {
        // Actual constructor/operation owner is retained before the first await.
        if let Some(actual) = self.callback_owner(session, original)? {
            actual.observe(session, self).await?;
            actual.reconciled(session, self)?;
        }
        Ok(())
    }
    pub(super) async fn acknowledge_callback_cut(
        self: &Arc<Self>,
        session: &AppServerSession,
    ) -> io::Result<()> {
        let owner = {
            let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
            match &progress._callbacks {
                Owner::Original => None,
                Owner::Entered(actual) => Some(actual.clone()),
                Owner::Unprepared => return Err(unavailable()),
            }
        };
        if let Some(actual) = owner {
            actual.acknowledge(session, self).await?;
        }
        Ok(())
    }
}
