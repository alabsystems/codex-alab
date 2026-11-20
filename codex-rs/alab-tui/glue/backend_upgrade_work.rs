// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! One conditional admission attempt over every original backend's job Books.
//! This observes existing workers; it grants no transport, Native or scene authority.
use super::BackendSessions;
use crate::app_server_session::alab_session_models;
use crate::app_server_session::alab_session_work;
use crate::backend_id::BackendId;
use futures::FutureExt;
use std::collections::HashSet;
use std::io;

struct OwnerCut {
    owner: BackendId,
    models: alab_session_models::Capture,
    work: alab_session_work::Capture,
}

struct OwnerReady {
    owner: BackendId,
    models: alab_session_models::Ready,
    work: alab_session_work::Ready,
}

/// Exact Book-lineage witnesses for the ordered, actual backend owners.
/// Keep this guard until the final synchronous admission check; it is not an H1 cut.
pub(in crate::app) struct UpgradeJobsReady {
    owners: Vec<OwnerReady>,
    displayed: BackendId,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn displayed_owner(backends: &BackendSessions) -> io::Result<BackendId> {
    let displayed = backends
        .entries
        .get(backends.displayed.0)
        .ok_or_else(|| invalid("upgrade job cut has no actual displayed backend"))?;
    let mut owners = HashSet::with_capacity(backends.entries.len());
    for entry in &backends.entries {
        if !owners.insert(entry.session.backend_id()) {
            return Err(invalid("upgrade job cut repeats an actual backend owner"));
        }
    }
    Ok(displayed.session.backend_id())
}

// Only owned captures enter this future: no App, BackendSessions or Session borrow.
async fn observe(cuts: Vec<OwnerCut>, displayed: BackendId) -> io::Result<UpgradeJobsReady> {
    let mut owners = Vec::with_capacity(cuts.len());
    for cut in cuts {
        let models = cut.models.observe().await?;
        let work = cut.work.observe().await?;
        owners.push(OwnerReady {
            owner: cut.owner,
            models,
            work,
        });
    }
    Ok(UpgradeJobsReady { owners, displayed })
}

impl BackendSessions {
    /// Poll one owned aggregate once. A pending attempt does not retain a waiter:
    /// dropping it restores the original join leases while their workers continue.
    /// Errors remain errors; the caller refuses that original conditional request.
    pub(in crate::app) fn alab_try_upgrade_jobs(&self) -> io::Result<Option<UpgradeJobsReady>> {
        let displayed = displayed_owner(self)?;
        let mut cuts = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            cuts.push(OwnerCut {
                owner: entry.session.backend_id(),
                models: entry.session.alab_capture_original_model_jobs(),
                work: entry.session.alab_capture_original_session_jobs()?,
            });
        }
        match observe(cuts, displayed).now_or_never() {
            None => Ok(None),
            Some(result) => {
                let ready = result?;
                ready.current(self)?;
                Ok(Some(ready))
            }
        }
    }
}

impl UpgradeJobsReady {
    /// Recheck immediately before dispatch, after the actor's original checks.
    /// No Session collection borrow crosses an await and no cached result is consumed.
    pub(in crate::app) fn current(&self, backends: &BackendSessions) -> io::Result<()> {
        if self.owners.is_empty()
            || self.owners.len() != backends.entries.len()
            || displayed_owner(backends)? != self.displayed
        {
            return Err(invalid(
                "upgrade job cut changed its actual backend inventory",
            ));
        }
        for (ready, entry) in self.owners.iter().zip(&backends.entries) {
            if ready.owner != entry.session.backend_id() {
                return Err(invalid(
                    "upgrade job cut changed actual backend order or owner",
                ));
            }
            // Both APIs reject request-only views and different physical Books.
            ready.work.current(&entry.session)?;
            // DATA is observed only as this policy precondition. This does not take
            // the original native result, consume its cache, or resume its panic.
            let _ = ready.models.current(&entry.session)?;
        }
        // Model snapshot cloning above can overlap real worker/consumer activity.
        // Recheck every captured generation afterwards; this is not an atomic freeze.
        for ready in &self.owners {
            ready.models.check()?;
            ready.work.check()?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "backend_upgrade_work_tests.rs"]
mod tests;
