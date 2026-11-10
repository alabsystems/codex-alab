// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Direct readers have native loaded/retirement evidence, never manager counters.
use super::*;
use crate::native_reader_retirement::Closed;
use crate::native_reader_retirement::Observer;
use crate::native_source_lifetime::MemberIncarnation;
use crate::native_source_lifetime::ReaderLoaded;

pub trait ReaderBackend: Send + Sync {
    fn identity(&self) -> &MemberIncarnation;
    fn loaded(&self) -> Result<ReaderLoaded, Refused>;
    fn retirement(&self) -> Observer;
    fn check_target(&self, target: &Target) -> Result<(), Refused>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Manager,
    Reader,
    Retired,
    Unresolved,
}
impl Record {
    pub fn role(&self) -> Result<Role, Refused> {
        let state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        Ok(if state.phase == Phase::Retired {
            Role::Retired
        } else if state.reader.is_some() {
            Role::Reader
        } else if state.backend.is_some() {
            Role::Manager
        } else {
            Role::Unresolved
        })
    }
    pub fn attach_reader(self: &Arc<Self>, backend: Arc<dyn ReaderBackend>) -> Result<(), Refused> {
        let observer = backend.retirement();
        if observer.incarnation() != backend.identity() {
            return Err(Refused::Unavailable);
        }
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| Refused::Unavailable)?;
        {
            let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
            if state.phase != Phase::Loading || state.backend.is_some() || state.reader.is_some() {
                return Err(Refused::Busy);
            }
            state.reader = Some(backend);
        }
        let record = Arc::clone(self);
        runtime.spawn(async move {
            if let Ok(closed) = observer.closed().await {
                let _ = record.finish_reader_closed(&closed);
            }
        });
        Ok(())
    }
    pub fn reader_snapshot(&self) -> Result<Option<ReaderLoaded>, Refused> {
        let backend = self
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?
            .reader
            .clone();
        backend.map(|backend| backend.loaded()).transpose()
    }
    fn finish_reader_closed(&self, closed: &Closed) -> Result<(), Refused> {
        let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        if state.phase == Phase::Retired {
            return if state
                .closed_reader
                .as_ref()
                .is_some_and(|member| closed.matches(member))
            {
                Ok(())
            } else {
                Err(Refused::Unavailable)
            };
        }
        let identity = state
            .reader
            .as_ref()
            .ok_or(Refused::Unavailable)?
            .identity()
            .clone();
        if !closed.matches(&identity) {
            return Err(Refused::Unavailable);
        }
        state.phase = Phase::Retired;
        state.closed_reader = Some(identity);
        state.reader = None;
        state.participants.clear();
        drop(state);
        self.registry.changed.notify_waiters();
        Ok(())
    }
    pub async fn reader_retired(&self) -> Result<(), Refused> {
        loop {
            let changed = self.registry.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.phase()? == Phase::Retired {
                return Ok(());
            }
            changed.await;
        }
    }
    pub(super) async fn drain_reader(&self, domain: &Arc<Domain>) -> Result<bool, Refused> {
        let backend = self
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?
            .reader
            .clone();
        let Some(backend) = backend else {
            return Ok(false);
        };
        // This branch is reached only under the actual catalog retiring gate.
        // Reader extraction is revoked after every old body/use and End ACK drains.
        domain.catalog_wait(true).await?;
        let observer = {
            let gate = self
                .registry
                .state
                .lock()
                .map_err(|_| Refused::Unavailable)?;
            if !gate
                .seal
                .as_ref()
                .is_some_and(|seal| seal.retiring.load(Ordering::Acquire))
                || !Arc::ptr_eq(domain, &self.known_domain()?)
                || !domain.catalog_idle(true)?
                || !crate::native_source_use::settled_for(domain)
            {
                return Err(Refused::Pending);
            }
            match backend.identity().upgrade() {
                Some(member) => member.retire_reader().map_err(|_| Refused::Unavailable)?,
                None => backend.retirement(),
            }
        };
        if observer.incarnation() != backend.identity() {
            return Err(Refused::Unavailable);
        }
        let closed = observer.closed().await.map_err(|_| Refused::Unavailable)?;
        self.finish_reader_closed(&closed)?;
        Ok(true)
    }
}
impl Registry {
    /// Only authenticated terminal reader proofs permit census compaction.
    pub fn compact_retired_readers(&self) -> Result<usize, Refused> {
        let _gate = self.state.lock().map_err(|_| Refused::Unavailable)?;
        let mut records = self.records.lock().map_err(|_| Refused::Unavailable)?;
        let before = records.len();
        records.retain(|_, record| record.phase() != Ok(Phase::Retired));
        Ok(before - records.len())
    }
}
/// An actual direct-reader origin. It has no manager generations or manager
/// Version adapter; the exact native loaded observation and catalogue own it.
#[derive(Clone)]
pub struct ReaderScope {
    record: Arc<Record>,
    domain: Arc<Domain>,
    member: crate::native_source_lifetime::Lifetime,
    loaded: ReaderLoaded,
    parent: Option<ConstructorParent>,
}
impl std::fmt::Debug for ReaderScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReaderSourceScope(<local>)")
    }
}
impl PartialEq for ReaderScope {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.record, &other.record) && self.loaded == other.loaded
    }
}
impl Eq for ReaderScope {}
impl ReaderScope {
    pub fn capture(
        record: Arc<Record>,
        member: crate::native_source_lifetime::Lifetime,
    ) -> Result<Self, Refused> {
        let domain = member.domain().ok_or(Refused::Unavailable)?;
        if !domain
            .catalog()
            .is_some_and(|actual| Arc::ptr_eq(actual, &record))
        {
            return Err(Refused::Unavailable);
        }
        let loaded = member.reader_loaded().map_err(|_| Refused::Unavailable)?;
        if record.reader_snapshot()?.as_ref() != Some(&loaded) {
            return Err(Refused::Unavailable);
        }
        let parent = record
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?
            .parent
            .clone();
        Ok(Self {
            record,
            domain,
            member,
            loaded,
            parent,
        })
    }
    pub fn live(&self) -> bool {
        self.record.phase() == Ok(Phase::Ready)
            && self
                .parent
                .as_ref()
                .is_none_or(|parent| parent.domain.upgrade().is_some())
            && self
                .record
                .reader_snapshot()
                .is_ok_and(|current| current.as_ref() == Some(&self.loaded))
            && self.member.reader_still_loaded(&self.loaded)
    }
    pub fn revoked(&self) -> bool {
        self.record.phase() == Ok(Phase::Retired)
            || self
                .member
                .reader_loaded()
                .is_ok_and(|current| current != self.loaded)
    }
    pub fn compatible_manager(&self, manager: &Scope) -> bool {
        let Some(parent) = &self.parent else {
            return false;
        };
        let Some(domain) = parent.domain.upgrade() else {
            return false;
        };
        if !Arc::ptr_eq(&domain, &manager.domain) {
            return false;
        }
        let Some(native) = &manager.native else {
            return false;
        };
        self.native_scope()
            .is_ok_and(|reader| reader.derived_from_scope(native))
    }
    pub fn native_scope(&self) -> Result<crate::native_source_lifetime::NetworkScope, Refused> {
        if !self.live() {
            return Err(Refused::Unavailable);
        }
        self.member
            .network_scope()
            .map_err(|_| Refused::Unavailable)
    }
    pub fn acquire(&self) -> Result<ReaderUse, Refused> {
        if !self.record.cache_available() {
            return Err(Refused::Pending);
        }
        if !self.live() {
            return Err(Refused::Unavailable);
        }
        let current = crate::assignment::current_activity();
        let activity = match current.as_ref() {
            Some(parent) if parent.same_domain(&self.domain) => {
                self.domain.admit_resource(Some(parent))
            }
            Some(parent)
                if self
                    .parent
                    .as_ref()
                    .is_some_and(|origin| origin.permits(parent)) =>
            {
                self.domain.admit_catalog_resource(parent)
            }
            Some(_) => Err(Refused::WrongDomain),
            None => self.domain.admit_resource(None),
        }?;
        if !self.live() {
            return Err(Refused::Unavailable);
        }
        Ok(ReaderUse {
            scope: self.clone(),
            _activity: activity,
        })
    }
}
#[derive(Clone)]
pub struct ReaderUse {
    scope: ReaderScope,
    _activity: Activity,
}
impl std::fmt::Debug for ReaderUse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReaderSourceUse(<local>)")
    }
}
impl ReaderUse {
    pub fn acquire_native(
        &self,
        native: &crate::native_source_lifetime::NetworkScope,
    ) -> std::io::Result<crate::native_source_lifetime::NetworkUse> {
        if !self.live()
            || !native.belongs_to(&self.scope.domain)
            || !native.member_is(&self.scope.member)
        {
            return Err(crate::managed_auth::unavailable());
        }
        native.acquire_for(Some(self._activity.clone()))
    }
    pub fn live(&self) -> bool {
        self.scope.live()
    }
    pub async fn revoked(&self) {
        loop {
            let changed = self.scope.record.registry.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self.live() {
                return;
            }
            tokio::select! {
                _ = changed => {},
                _ = self.scope.member.reader_changed(&self.scope.loaded) => {},
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_source::Loaded;
    use crate::native_source::Member;
    use crate::native_source::ReadyFuture;
    use crate::native_source::ReadyReceipt;
    use crate::native_source::UnitFuture;
    use crate::native_source_lifetime::Lifetime;
    struct Peer;
    impl Member for Peer {
        fn id(&self) -> &str {
            "11111111111111111111111111111111"
        }
        fn live(&self) -> bool {
            true
        }
        fn ready<'a>(
            &'a self,
            _: &'a str,
            _: Option<&'a str>,
            loaded: &'a Loaded,
        ) -> ReadyFuture<'a> {
            Box::pin(async move {
                assert!(matches!(
                    loaded,
                    Loaded::NoNativeReader(crate::native_source::NoNativeReader::ApiKey)
                ));
                Ok(ReadyReceipt {
                    registration: "2".repeat(32),
                })
            })
        }
        fn retire<'a>(&'a self, _: Option<&'a str>) -> UnitFuture<'a> {
            Box::pin(async { Ok(()) })
        }
    }
    struct Reader {
        identity: MemberIncarnation,
        loaded: ReaderLoaded,
        observer: Observer,
    }
    impl ReaderBackend for Reader {
        fn identity(&self) -> &MemberIncarnation {
            &self.identity
        }
        fn loaded(&self) -> Result<ReaderLoaded, Refused> {
            if self.identity.reader_still_loaded(&self.loaded) {
                Ok(self.loaded.clone())
            } else {
                Err(Refused::Unavailable)
            }
        }
        fn retirement(&self) -> Observer {
            self.observer.clone()
        }
        fn check_target(&self, _: &Target) -> Result<(), Refused> {
            Err(Refused::Unavailable)
        }
    }
    fn admitted(registry: &Arc<Registry>) -> Arc<Domain> {
        let factory = registry.factory(None).unwrap();
        let domain = factory.domain();
        factory.complete().unwrap();
        domain
    }
    #[tokio::test]
    async fn reader_origin_requires_actual_constructor_lineage_and_terminal_retirement() {
        let registry = Arc::new(Registry::default());
        let origin = admitted(&registry);
        let parent = origin.admit_work(None).unwrap();
        let factory = registry.factory(Some(&parent)).unwrap();
        let domain = factory.domain();
        let record = factory.record().clone();
        let member = Lifetime::new(crate::native_source_launch::Allocation {
            operation: "a".repeat(32),
            member: Arc::new(Peer),
            parent_use: None,
            derived_parent: None,
            target: None,
        })
        .unwrap();
        member.attach_domain(domain).unwrap();
        member
            .unrelated_ready(crate::native_source::NoNativeReader::ApiKey)
            .await
            .unwrap();
        record
            .attach_reader(Arc::new(Reader {
                identity: member.incarnation(),
                loaded: member.reader_loaded().unwrap(),
                observer: member.retirement_observer(),
            }))
            .unwrap();
        factory.complete().unwrap();
        let scope = ReaderScope::capture(record.clone(), member.clone()).unwrap();
        let other = admitted(&registry);
        let unrelated = other.admit_work(None).unwrap();
        assert!(
            unrelated.scope(async { scope.acquire() }).await.is_err(),
            "same registry/counters cannot forge parent identity"
        );
        assert!(
            parent
                .clone()
                .scope(async { scope.acquire() })
                .await
                .is_ok()
        );
        let child = admitted(&registry);
        let inherited = child.admit_catalog_resource(&parent).unwrap();
        assert!(
            inherited
                .clone()
                .scope(async { scope.acquire() })
                .await
                .is_ok(),
            "actual inherited use family remains admitted"
        );
        drop(parent);
        drop(origin);
        assert!(
            inherited
                .clone()
                .scope(async { scope.acquire() })
                .await
                .is_err(),
            "lost weak parent Domain is Unknown, never independent"
        );
        drop(inherited);
        member.retire_reader().unwrap().closed().await.unwrap();
        record.reader_retired().await.unwrap();
        assert!(!scope.live());
        assert!(scope.revoked());
        assert!(scope.acquire().is_err());
    }
}
