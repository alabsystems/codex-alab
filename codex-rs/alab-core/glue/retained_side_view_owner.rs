// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Backend custody for genuinely installed client Side views. Historical
//! receipts keep no strong lease; only this live slot/provisional Scope does.
//! Task children and ordinary unsubscribed threads acquire no such owner.
use super::super::super::thread_lifecycle::alab_primary_lifetime::PrimaryLease;
use super::*;
#[derive(Default)]
pub(super) struct Views {
    current: HashMap<ThreadId, Owned>,
}
pub(super) struct Owned {
    scope: Weak<Scope>,
    actual: Arc<CodexThread>,
    lease: PrimaryLease,
}
impl InitialSessionOperation {
    pub(super) fn retain_side_view(
        &self,
        scope: &Arc<Scope>,
        id: ThreadId,
        actual: &Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.closed || self.input.is_stopped() {
            return Err(invalid_request("Side view owner is closing"));
        }
        if scope.frozen.retains_candidate()
            || !state
                .clients
                .values()
                .any(|current| Arc::ptr_eq(current, scope))
        {
            return Err(invalid_request(
                "actual client Side scope is no longer owned",
            ));
        }
        let mut progress = scope
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !progress.selection_retired
            || progress.worker_lost
            || !progress.worker_finished
            || progress.selection_unknown
            || actual.has_shutdown_status()
        {
            return Err(invalid_request(
                "actual Side selection/worker completion unobserved",
            ));
        }
        let mut views = self
            .side_views
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if views.current.get(&id).is_some_and(|old| {
            old.scope
                .upgrade()
                .is_some_and(|old| Arc::ptr_eq(&old, scope))
        }) {
            views.check(scope, id, actual)?;
            return Ok(());
        }
        let lease = progress
            .primary
            .as_ref()
            .filter(|lease| lease.is_for(actual))
            .ok_or_else(|| invalid_request("actual Side lease is unavailable"))?;
        let weak = lease.downgrade();
        let old = views.install(scope, id, actual, lease)?;
        progress.primary_weak = Some(weak);
        let provisional = progress.primary.take();
        drop(views);
        drop(progress);
        drop(state);
        drop(provisional);
        drop(old); // Same token moved continuously; no lease destruction under any lock.
        Ok(())
    }
    pub(super) fn release_side_views_after_threads(&self) {
        let retired = {
            let mut views = self
                .side_views
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            std::mem::take(&mut views.current)
        };
        drop(retired);
    }
}

impl InitialSessionOperation {
    pub(super) fn installed_side_core(
        &self,
        scope: &Arc<Scope>,
        thread: &str,
    ) -> Result<Arc<CodexThread>, JSONRPCErrorError> {
        let views = self
            .side_views
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        views
            .current
            .iter()
            .find_map(|(id, view)| {
                view.scope
                    .upgrade()
                    .filter(|current| Arc::ptr_eq(current, scope))
                    .filter(|_| id.to_string() == thread && view.lease.is_for(&view.actual))
                    .map(|_| view.actual.clone())
            })
            .ok_or_else(|| invalid_request("actual committed Side view unavailable"))
    }
    pub(super) fn release_side_view(&self, scope: &Arc<Scope>, actual: &Arc<CodexThread>) {
        let removed = {
            let mut views = self
                .side_views
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let id = views.current.iter().find_map(|(id, view)| {
                (Arc::ptr_eq(&view.actual, actual)
                    && view
                        .scope
                        .upgrade()
                        .is_some_and(|current| Arc::ptr_eq(&current, scope)))
                .then_some(*id)
            });
            id.and_then(|id| views.current.remove(&id))
        };
        drop(removed); // Original no-subscriber timer wakes; never hold Source locks across lease Drop.
    }
}

impl Views {
    /// Pure Source lease custody shared with lower actual-Core controls. The
    /// production caller above checks real committed/finished resources first;
    /// this method supplies no Native or UI witness.
    pub(super) fn install(
        &mut self,
        scope: &Arc<Scope>,
        id: ThreadId,
        actual: &Arc<CodexThread>,
        lease: &PrimaryLease,
    ) -> Result<Option<Owned>, JSONRPCErrorError> {
        if !lease.is_for(actual) || actual.has_shutdown_status() {
            return Err(invalid_request("actual Side lease identity unavailable"));
        }
        if let Some(old) = self.current.get(&id) {
            if old
                .scope
                .upgrade()
                .is_some_and(|current| Arc::ptr_eq(&current, scope))
            {
                self.check(scope, id, actual)?;
                return Ok(None);
            }
            if !Arc::ptr_eq(&old.actual, actual)
                && old.actual.wait_until_terminated().now_or_never().is_none()
            {
                return Err(invalid_request(
                    "another actual Side incarnation is still owned",
                ));
            }
        }
        Ok(self.current.insert(
            id,
            Owned {
                scope: Arc::downgrade(scope),
                actual: actual.clone(),
                lease: lease.clone(),
            },
        ))
    }
    pub(super) fn check(
        &self,
        scope: &Arc<Scope>,
        id: ThreadId,
        actual: &Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        self.current
            .get(&id)
            .filter(|old| {
                old.scope
                    .upgrade()
                    .is_some_and(|current| Arc::ptr_eq(&current, scope))
                    && Arc::ptr_eq(&old.actual, actual)
                    && old.lease.is_for(actual)
            })
            .map(|_| ())
            .ok_or_else(|| invalid_request("actual Side lease resource changed"))
    }
}
