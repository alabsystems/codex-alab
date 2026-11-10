// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Local destination census ownership. Native destination/source-seal authority
//! is independently retained and checked by the authenticated native endpoint.
use super::*;

pub struct DestinationFactory {
    factory: Factory,
    retained: DestinationRecord,
    begun: AtomicBool,
}
pub struct DestinationRecord {
    registry: Arc<Registry>,
    seal: Arc<SealState>,
    census: Vec<CensusEntry>,
    record: Arc<Record>,
    domain: Arc<Domain>,
    destination: Arc<dyn crate::native_target::Destination>,
    constructor: OnceLock<String>,
    role: Arc<AdmissionRole>,
}
pub(super) struct AdmissionRole(AtomicBool);
impl AdmissionRole {
    fn active(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
struct CensusEntry {
    domain: Arc<Domain>,
    record: Arc<Record>,
    stamp: Option<Stamp>,
}
impl std::fmt::Debug for DestinationRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DestinationCensus(<local-not-native>)")
    }
}
impl DestinationRecord {
    pub fn close_resources(&self) {
        self.role.0.store(false, Ordering::Release);
        self.domain.native_use_changed();
        self.registry.changed.notify_waiters();
    }
    pub fn domain(&self) -> &Arc<Domain> {
        &self.domain
    }
    pub fn record(&self) -> &Arc<Record> {
        &self.record
    }
    pub fn destination(&self) -> &Arc<dyn crate::native_target::Destination> {
        &self.destination
    }
    pub fn protects_old_domain(&self, domain: &Arc<Domain>) -> bool {
        self.census
            .iter()
            .any(|entry| Arc::ptr_eq(&entry.domain, domain))
    }
    pub async fn abandon(self: Arc<Self>, receipt: crate::native_target::ConstructionReceipt) {
        self.close_resources();
        loop {
            if self
                .constructor
                .get()
                .is_some_and(|operation| operation == receipt.operation())
                && self.domain.catalog_wait(true).await.is_ok()
                && receipt.retire_quiescent(&self.domain).is_ok()
                && let Ok(closed) = receipt.closed().await
                && receipt.matches_closed(self.destination.context(), &closed)
                && self.domain.catalog_wait(true).await.is_ok()
                && let Ok(mut state) = self.record.state.lock()
            {
                state.phase = Phase::Retired;
                state.participants.clear();
                self.registry.changed.notify_waiters();
                return;
            }
            // Keep the exact original member/attempt/old census through unknown
            // cleanup. No new provider/source operation is created by an observer.
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }
    pub fn resource_scope<T>(&self, action: impl FnOnce() -> T) -> Result<T, Refused> {
        self.validate()?;
        Ok(CURRENT_DESTINATION.sync_scope(self.record.clone(), action))
    }
    pub async fn scope_resources<F: std::future::Future>(
        &self,
        future: F,
    ) -> Result<F::Output, Refused> {
        self.validate()?;
        Ok(CURRENT_DESTINATION.scope(self.record.clone(), future).await)
    }
    pub fn validate(&self) -> Result<(), Refused> {
        if !self.role.active() {
            return Err(Refused::Busy);
        }
        let state = self
            .registry
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        if !state
            .seal
            .as_ref()
            .is_some_and(|seal| Arc::ptr_eq(seal, &self.seal))
            || !self.seal.retiring.load(Ordering::Acquire)
        {
            return Err(Refused::Busy);
        }
        for entry in &self.census {
            if !entry.domain.catalog_idle(true)?
                || !crate::native_source_use::settled_for(&entry.domain)
            {
                return Err(Refused::Pending);
            }
            if entry.record.phase()? != Phase::Retired
                && Some(entry.record.source_stamp()?) != entry.stamp
            {
                return Err(Refused::Pending);
            }
        }
        Ok(())
    }
}
impl Record {
    pub(super) fn destination_resource(&self, parent: Option<&Activity>, epoch: u64) -> bool {
        if self.admitted_epoch != epoch {
            return false;
        }
        let role = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.destination_role.clone());
        let Some(role) = role else {
            return false;
        };
        (role.active()
            && CURRENT_DESTINATION
                .try_with(|record| std::ptr::eq(record.as_ref(), self))
                .unwrap_or(false))
            || parent.is_some_and(|parent| {
                self.domain()
                    .is_some_and(|domain| Arc::ptr_eq(&domain, parent.domain()))
                    && parent.catalog_epoch() == epoch
            })
    }
}
impl DestinationFactory {
    pub fn protects_old_domain(&self, domain: &Arc<Domain>) -> bool {
        self.retained.protects_old_domain(domain)
    }
    pub fn bind_constructor(&self, operation: &str) -> Result<(), Refused> {
        if !crate::managed_auth::valid_hex(operation, 32) {
            return Err(Refused::Unavailable);
        }
        self.retained
            .constructor
            .set(operation.into())
            .map_err(|_| Refused::Busy)
    }
    pub async fn scope<F: std::future::Future>(&self, future: F) -> Result<F::Output, Refused> {
        self.retained.validate()?;
        if !self.factory.admitted() {
            return Err(Refused::Pending);
        }
        if self.begun.swap(true, Ordering::AcqRel) {
            return Err(Refused::Busy);
        }
        Ok(CURRENT_FACTORY
            .scope(self.factory.record.clone(), future)
            .await)
    }
    pub fn complete(self) -> Result<DestinationRecord, Refused> {
        self.retained.validate()?;
        self.factory.complete()?;
        Ok(self.retained)
    }
    pub fn complete_owned(self) -> (DestinationRecord, Result<(), Refused>) {
        let status = self
            .retained
            .validate()
            .and_then(|_| self.factory.complete().map(|_| ()));
        (self.retained, status)
    }
    pub fn failed_owned(self) -> DestinationRecord {
        // Dropping the started Factory records Unknown, never NoReader/retired.
        let Self {
            factory, retained, ..
        } = self;
        drop(factory);
        retained
    }
    pub fn retire_unstarted(self) -> Result<(), Refused> {
        if self.begun.load(Ordering::Acquire) {
            return Err(Refused::Busy);
        }
        let mut state = self
            .factory
            .record
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        if state.phase != Phase::Loading {
            return Err(Refused::Busy);
        }
        state.phase = Phase::Retired;
        self.factory.record.registry.changed.notify_waiters();
        Ok(())
    }
}
impl Seal {
    /// Preserve the exact drained A census while admitting a separate B record.
    /// No ordinary constructor or A activity receives this continuation. A native
    /// Installed destination is still required for every positive B source call.
    pub fn destination_factory(
        &self,
        source: &Arc<Domain>,
        destination: Arc<dyn crate::native_target::Destination>,
    ) -> Result<DestinationFactory, Refused> {
        let domain = Arc::new(Domain::default());
        let mut state = self
            .registry
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        if !state
            .seal
            .as_ref()
            .is_some_and(|seal| Arc::ptr_eq(seal, &self.state))
            || !self.state.retiring.load(Ordering::Acquire)
            || !self.state.effects.load(Ordering::Acquire)
        {
            return Err(Refused::Busy);
        }
        let mut records = self
            .registry
            .records
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        let mut census = Vec::new();
        for record in records
            .values()
            .filter(|record| record.admitted_epoch < self.state.epoch)
        {
            if record.phase()? == Phase::Deferred {
                continue;
            }
            if record.phase()? == Phase::Retired {
                if let Some(domain) = record.domain() {
                    census.push(CensusEntry {
                        domain,
                        record: record.clone(),
                        stamp: None,
                    });
                }
                continue;
            }
            let old = record.known_domain()?;
            if !old.catalog_idle(true)? || !crate::native_source_use::settled_for(&old) {
                return Err(Refused::Pending);
            }
            census.push(CensusEntry {
                domain: old,
                record: record.clone(),
                stamp: Some(record.source_stamp()?),
            });
        }
        if !census
            .iter()
            .any(|entry| Arc::ptr_eq(&entry.domain, source))
        {
            return Err(Refused::Unavailable);
        }
        state.next = state.next.checked_add(1).ok_or(Refused::Unavailable)?;
        let role = Arc::new(AdmissionRole(AtomicBool::new(true)));
        let record = Arc::new(Record {
            id: state.next,
            admitted_epoch: self.state.epoch,
            registry: self.registry.gate.clone(),
            domain: Arc::downgrade(&domain),
            state: Mutex::new(RecordState {
                parent: None,
                phase: Phase::Loading,
                backend: None,
                reader: None,
                closed_reader: None,
                destination_role: Some(role.clone()),
                participants: Vec::new(),
            }),
        });
        domain.attach_catalog(record.clone())?;
        records.insert(record.id, record.clone());
        let factory = Factory {
            record: record.clone(),
            domain: domain.clone(),
            completed: false,
            epoch: self.state.epoch,
        };
        let retained = DestinationRecord {
            registry: self.registry.clone(),
            seal: self.state.clone(),
            census,
            record,
            domain,
            destination,
            constructor: OnceLock::new(),
            role,
        };
        drop(records);
        drop(state);
        self.registry.changed.notify_waiters();
        Ok(DestinationFactory {
            factory,
            retained,
            begun: AtomicBool::new(false),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Owner;
    impl Backend for Owner {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn check_target(&self, _: &Target) -> Result<(), Refused> {
            Ok(())
        }
        fn source_stamp(&self) -> Result<crate::assignment::Generations, Refused> {
            Ok(crate::assignment::Generations {
                auth: 1,
                owner: 1,
                source: 1,
                login: 1,
            })
        }
    }
    struct TargetOwner(crate::native_target::Context);
    impl crate::native_target::Destination for TargetOwner {
        fn context(&self) -> &crate::native_target::Context {
            &self.0
        }
        fn open_member<'a>(&'a self, _: &'a str) -> crate::native_source::MemberFuture<'a> {
            Box::pin(async { Err(crate::managed_auth::unavailable()) })
        }
    }
    fn destination() -> Arc<dyn crate::native_target::Destination> {
        Arc::new(TargetOwner(
            crate::native_target::Context::from_native(
                "a".repeat(32),
                "b".repeat(64),
                1,
                "c".repeat(64),
            )
            .unwrap(),
        ))
    }
    #[tokio::test]
    async fn destination_resources_keep_separate_domain_and_retained_old_census() {
        let registry = Arc::new(Registry::default());
        let old = registry.factory(None).unwrap();
        let old_domain = old.domain();
        let old_weak = Arc::downgrade(&old_domain);
        old.record().attach(Arc::new(Owner)).unwrap();
        old.complete().unwrap();
        let resource = old_domain.admit_resource(None).unwrap();
        let seal = registry.seal().unwrap();
        assert!(
            seal.destination_factory(&old_domain, destination())
                .is_err()
        );
        seal.begin_retirement().unwrap();
        assert!(
            matches!(
                seal.destination_factory(&old_domain, destination()),
                Err(Refused::Pending)
            ),
            "retirement may begin, but B cannot be constructed while an A resource remains"
        );
        drop(resource);
        let new = seal
            .destination_factory(&old_domain, destination())
            .unwrap();
        let new_domain = new.retained.domain.clone();
        new.factory.record().attach(Arc::new(Owner)).unwrap();
        let ready = new.complete().unwrap();
        drop(old_domain);
        assert!(
            old_weak.upgrade().is_some(),
            "exact old domain stays strongly retained"
        );
        assert!(!ready.record().cache_available());
        assert!(new_domain.admit_resource(None).is_err());
        let b_resource = ready
            .resource_scope(|| {
                assert!(ready.record().cache_available());
                assert!(new_domain.admit_start(None).is_err());
                assert!(new_domain.admit_work(None).is_err());
                new_domain.admit_resource(None).unwrap()
            })
            .unwrap();
        assert!(
            ready.validate().is_ok(),
            "B resources are not part of A drain"
        );
        assert!(new_domain.admit_resource(Some(&b_resource)).is_ok());
        assert!(new_domain.admit_work(Some(&b_resource)).is_err());
        ready.close_resources();
        assert!(
            ready
                .resource_scope(|| new_domain.admit_resource(None))
                .is_err()
        );
        assert!(
            CURRENT_DESTINATION
                .sync_scope(ready.record().clone(), || new_domain.admit_resource(None))
                .is_err(),
            "a previously captured role cannot reopen a queued callback"
        );
        assert!(
            new_domain.admit_resource(Some(&b_resource)).is_ok(),
            "actual admitted resource can finish its counted continuation"
        );
        let old = old_weak.upgrade().unwrap();
        assert!(old.admit_resource(Some(&b_resource)).is_err());
    }
}
