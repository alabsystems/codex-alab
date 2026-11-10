// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Immutable local source epochs. These are activity/census identities, never native grants.
use crate::assignment::Domain;
use crate::assignment::Refused;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::Notify;

#[derive(Clone)]
pub struct Epoch(Arc<State>);
struct State {
    domain: Arc<Domain>,
    predecessor: Option<Weak<Domain>>,
    active: Arc<AtomicBool>,
    retired: Arc<OnceLock<crate::assignment::Generations>>,
    changed: Arc<Notify>,
}
impl Epoch {
    pub fn predecessor_domain(&self) -> Option<Arc<Domain>> {
        self.0.predecessor.as_ref().and_then(Weak::upgrade)
    }
    pub fn domain(&self) -> Arc<Domain> {
        Arc::clone(&self.0.domain)
    }
    pub fn retired_stamp(&self) -> Option<&crate::assignment::Generations> {
        self.0.retired.get()
    }
    pub fn active(&self) -> bool {
        self.0.active.load(Ordering::Acquire)
    }
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
/// Descriptive observation only. It cannot keep a domain/manager alive or
/// construct an Epoch, and never proves native retirement or readiness.
#[derive(Clone)]
pub struct Observation {
    active: Arc<AtomicBool>,
    retired: Arc<OnceLock<crate::assignment::Generations>>,
}
impl Observation {
    pub fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    pub fn retired_stamp(&self) -> Option<&crate::assignment::Generations> {
        self.retired.get()
    }
}
impl Epoch {
    pub fn observation(&self) -> Observation {
        Observation {
            active: self.0.active.clone(),
            retired: self.0.retired.clone(),
        }
    }
}
impl std::fmt::Debug for Epoch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ManagerSourceEpoch(<local>)")
    }
}
struct Current {
    epoch: Epoch,
    publishing: bool,
}
pub struct Owner(Mutex<Current>, Notify);
impl Owner {
    pub fn new(domain: Arc<Domain>) -> Self {
        Self(
            Mutex::new(Current {
                epoch: Epoch(Arc::new(State {
                    domain,
                    predecessor: None,
                    active: Arc::new(AtomicBool::new(true)),
                    retired: Arc::new(OnceLock::new()),
                    changed: Arc::new(Notify::new()),
                })),
                publishing: false,
            }),
            Notify::new(),
        )
    }
    /// Metadata remains retained during publication; callers must use capture for admission.
    pub fn domain(&self) -> Arc<Domain> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .epoch
            .domain()
    }
    pub fn capture(&self) -> Result<Epoch, Refused> {
        let state = self.0.lock().map_err(|_| Refused::Unavailable)?;
        if state.publishing {
            return Err(Refused::Pending);
        }
        Ok(state.epoch.clone())
    }
    /// A pointer-change wake hint only. The caller must reselect and admit the
    /// actual current domain; this notification carries no source permission.
    pub async fn domain_changed(&self, previous: &Arc<Domain>) -> Result<(), Refused> {
        loop {
            let changed = self.1.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !Arc::ptr_eq(
                &self
                    .0
                    .lock()
                    .map_err(|_| Refused::Unavailable)?
                    .epoch
                    .domain(),
                previous,
            ) {
                return Ok(());
            }
            changed.await;
        }
    }
    pub fn is_current(&self, epoch: &Epoch) -> bool {
        self.capture().is_ok_and(|current| current.same(epoch))
    }
    /// The auth-cache writer holds its write lock through this local publication.
    /// No native or constructor permission is created by beginning an update.
    pub fn begin(&self) -> Result<Publication<'_>, Refused> {
        let mut state = self.0.lock().map_err(|_| Refused::Unavailable)?;
        if state.publishing {
            return Err(Refused::Busy);
        }
        state.publishing = true;
        Ok(Publication {
            owner: self,
            finished: false,
        })
    }
}
pub struct Publication<'a> {
    owner: &'a Owner,
    finished: bool,
}
impl Publication<'_> {
    /// The destination domain must already be admitted by its actual constructor/target role.
    pub fn replace(
        &mut self,
        domain: Arc<Domain>,
        previous: crate::assignment::Generations,
    ) -> Result<Epoch, Refused> {
        let record = domain.catalog().ok_or(Refused::Unavailable)?;
        if !record.cache_available() {
            return Err(Refused::Pending);
        }
        let predecessor = Arc::downgrade(&self.owner.domain());
        let epoch = Epoch(Arc::new(State {
            domain,
            predecessor: Some(predecessor),
            active: Arc::new(AtomicBool::new(false)),
            retired: Arc::new(OnceLock::new()),
            changed: Arc::new(Notify::new()),
        }));
        let mut state = self.owner.0.lock().map_err(|_| Refused::Unavailable)?;
        state
            .epoch
            .0
            .retired
            .set(previous)
            .map_err(|_| Refused::Unavailable)?;
        state.epoch.0.active.store(false, Ordering::Release);
        state.epoch.0.changed.notify_waiters();
        state.epoch = epoch.clone();
        self.owner.1.notify_waiters();
        Ok(epoch)
    }
    pub fn finish(mut self) -> Result<(), Refused> {
        let mut state = self.owner.0.lock().map_err(|_| Refused::Unavailable)?;
        state.epoch.0.active.store(true, Ordering::Release);
        state.epoch.0.changed.notify_waiters();
        state.publishing = false;
        self.finished = true;
        Ok(())
    }
}
// Uncertain publication closes old snapshots too; cancellation cannot reopen it.
impl Drop for Publication<'_> {
    fn drop(&mut self) {
        if !self.finished {
            let state = self
                .owner
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.epoch.0.active.store(false, Ordering::Release);
            state.epoch.0.changed.notify_waiters();
        }
    }
}

/// Runtime-only snapshot metadata, captured with the actual auth document under
/// the cache lock. It retains its original epoch, never a mutable Owner/manager.
#[derive(Clone, Default)]
pub struct Snapshot(Option<Arc<Captured>>);
struct Captured {
    epoch: Epoch,
    actual: crate::assignment::Generations,
    native: Result<Option<crate::native_source_lifetime::Lifetime>, ()>,
}
impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CredentialEpoch(<local>)")
    }
}
// Runtime provenance does not change upstream credential-value equality.
impl PartialEq for Snapshot {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for Snapshot {}
impl Snapshot {
    pub fn captured(
        epoch: Epoch,
        actual: crate::assignment::Generations,
        native: Result<Option<crate::native_source_lifetime::Lifetime>, ()>,
    ) -> Self {
        Self(Some(Arc::new(Captured {
            epoch,
            actual,
            native,
        })))
    }
    pub fn native_scope(
        &self,
    ) -> Result<Option<crate::native_source_lifetime::NetworkScope>, Refused> {
        match self.0.as_ref().map(|captured| &captured.native) {
            Some(Ok(Some(member))) => member
                .network_scope()
                .map(Some)
                .map_err(|_| Refused::Unavailable),
            Some(Err(())) => Err(Refused::Unavailable),
            // Absence is no native coverage proof, never NoNativeReader evidence.
            Some(Ok(None)) | None => Ok(None),
        }
    }
    /// Only the explicit nonactivated legacy export contract. This is not
    /// native coverage, an independent grant, or permission to send a request.
    pub fn legacy_snapshot_export(&self) -> bool {
        matches!(
            self.0.as_ref().map(|captured| &captured.native),
            Some(Ok(None))
        )
    }
    pub fn has_native_member(&self) -> bool {
        matches!(
            self.0.as_ref().map(|captured| &captured.native),
            Some(Ok(Some(_)) | Err(()))
        )
    }
    /// Compare the captured native occurrence without granting readiness.
    pub fn same_native_member(&self, actual: &crate::native_source_lifetime::Lifetime) -> bool {
        matches!(self.0.as_ref().map(|captured| &captured.native),
            Some(Ok(Some(expected))) if expected.same(actual))
    }
    pub fn native_ready(&self) -> bool {
        match self.0.as_ref().map(|captured| &captured.native) {
            Some(Ok(Some(member))) => {
                self.0
                    .as_ref()
                    .is_some_and(|captured| captured.epoch.active())
                    && member.ready()
            }
            Some(Err(())) => false,
            Some(Ok(None)) | None => true,
        }
    }
    pub fn scope(&self) -> Result<Option<crate::manager_catalog::Scope>, Refused> {
        let Some(captured) = &self.0 else {
            return Ok(None);
        };
        crate::manager_catalog::Scope::capture(
            captured.epoch.domain(),
            Arc::new(SnapshotVersion {
                epoch: captured.epoch.clone(),
                actual: captured.actual.clone(),
            }),
        )
        .map(Some)
    }
    pub fn epoch(&self) -> Option<&Epoch> {
        self.0.as_ref().map(|captured| &captured.epoch)
    }
}
struct SnapshotVersion {
    epoch: Epoch,
    actual: crate::assignment::Generations,
}
impl crate::manager_catalog::Version for SnapshotVersion {
    fn current(&self) -> crate::assignment::Generations {
        self.epoch
            .domain()
            .catalog()
            .and_then(|record| record.backend().ok().flatten())
            .and_then(|backend| backend.source_stamp().ok())
            .unwrap_or_else(|| self.actual.clone())
    }
    fn source_epoch(&self) -> Result<Option<Epoch>, Refused> {
        if !self.epoch.active() {
            return Ok(Some(self.epoch.clone()));
        }
        let domain = self.epoch.domain();
        let record = domain.catalog().ok_or(Refused::Unavailable)?;
        if !record.cache_available() {
            return Err(Refused::Unavailable);
        }
        record
            .backend()?
            .ok_or(Refused::Unavailable)?
            .source_stamp()?;
        Ok(Some(self.epoch.clone()))
    }
    fn changed<'a>(
        &'a self,
        _: &'a crate::assignment::Generations,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let changed = self.epoch.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self.epoch.active() {
                return;
            }
            changed.await;
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn admitted(registry: &Arc<crate::manager_catalog::Registry>) -> Arc<Domain> {
        let factory = registry.factory(None).unwrap();
        let domain = factory.domain();
        factory.complete().unwrap();
        domain
    }
    #[test]
    fn legacy_export_requires_an_explicit_captured_nonactivated_contract() {
        let registry = Arc::new(crate::manager_catalog::Registry::default());
        let epoch = Owner::new(admitted(&registry)).capture().unwrap();
        let actual = crate::assignment::Generations {
            auth: 1,
            owner: 1,
            source: 1,
            login: 1,
        };
        assert!(!Snapshot::default().legacy_snapshot_export());
        assert!(
            !Snapshot::captured(epoch.clone(), actual.clone(), Err(())).legacy_snapshot_export()
        );
        assert!(Snapshot::captured(epoch, actual, Ok(None)).legacy_snapshot_export());
    }
    #[test]
    fn publication_retains_old_epoch_and_never_reopens_when_uncertain() {
        let registry = Arc::new(crate::manager_catalog::Registry::default());
        let a = admitted(&registry);
        let owner = Owner::new(a.clone());
        let old = owner.capture().unwrap();
        let b = admitted(&registry);
        let mut publication = owner.begin().unwrap();
        assert!(owner.capture().is_err());
        let destination = publication
            .replace(
                b.clone(),
                crate::assignment::Generations {
                    auth: 7,
                    owner: 2,
                    source: 3,
                    login: 2,
                },
            )
            .unwrap();
        assert!(Arc::ptr_eq(&old.domain(), &a));
        assert!(Arc::ptr_eq(&destination.domain(), &b));
        publication.finish().unwrap();
        assert!(!owner.is_current(&old));
        assert!(owner.is_current(&destination));
        drop(owner.begin().unwrap());
        assert!(owner.capture().is_err());
        assert!(Arc::ptr_eq(&old.domain(), &a));
    }
    #[test]
    fn credential_refresh_preserves_epoch_and_unadmitted_destination_is_rejected() {
        let registry = Arc::new(crate::manager_catalog::Registry::default());
        let owner = Owner::new(admitted(&registry));
        let old = owner.capture().unwrap();
        owner.begin().unwrap().finish().unwrap();
        assert!(owner.is_current(&old));
        let seal = registry.seal().unwrap();
        let refused = registry.factory(None).unwrap();
        let mut publication = owner.begin().unwrap();
        assert!(
            publication
                .replace(
                    refused.domain(),
                    crate::assignment::Generations {
                        auth: 7,
                        owner: 2,
                        source: 3,
                        login: 2
                    }
                )
                .is_err()
        );
        // A failed destination is not a native grant or a new source epoch.
        assert!(Arc::ptr_eq(&owner.domain(), &old.domain()));
        drop(publication);
        assert!(owner.capture().is_err());
        seal.cancel_before_effects().unwrap();
    }
}
