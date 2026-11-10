// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Process-local constructor and manager census. This is never native source authority.

use crate::assignment::Activity;
use crate::assignment::Domain;
use crate::assignment::Refused;
use crate::assignment_service::Target;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::Notify;
#[path = "manager_reader.rs"]
mod reader;
pub use reader::ReaderBackend;
pub use reader::ReaderScope;
pub use reader::ReaderUse;
pub use reader::Role;
#[path = "manager_destination.rs"]
mod destination;
pub use destination::{DestinationFactory, DestinationRecord};
#[derive(Clone, PartialEq, Eq)]
enum Stamp {
    Manager(crate::assignment::Generations),
    Reader(Box<crate::native_source_lifetime::ReaderLoaded>),
}

pub trait Backend: Send + Sync {
    fn as_any(&self) -> &dyn std::any::Any;
    /// Implementations inspect the actual manager, not a cached account label.
    fn check_target(&self, target: &Target) -> Result<(), Refused>;
    fn source_stamp(&self) -> Result<crate::assignment::Generations, Refused>;
}
pub type ResourceFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Refused>> + Send + 'a>>;
/// Type-erased mailbox only; implementations must not retain a recursive Session graph.
pub trait Participant: Send + Sync {
    fn retire<'a>(
        &'a self,
        domain: &'a Arc<Domain>,
        proof: Arc<dyn crate::assignment::RetirementProof>,
    ) -> ResourceFuture<'a>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Deferred,
    Loading,
    Ready,
    Unknown,
    Retired,
}
#[derive(Clone)]
struct ConstructorParent {
    domain: Weak<Domain>,
    family: crate::assignment::ActivityRef,
}
impl ConstructorParent {
    fn capture(parent: &Activity) -> Self {
        Self { domain: Arc::downgrade(parent.domain()), family: parent.downgrade() }
    }
    fn permits(&self, parent: &Activity) -> bool {
        self.domain.upgrade().is_some_and(|domain| {
            Arc::ptr_eq(&domain, parent.domain()) || self.family.same_family(parent)
        })
    }
}
struct RecordState {
    parent: Option<ConstructorParent>,
    phase: Phase,
    backend: Option<Arc<dyn Backend>>,
    reader: Option<Arc<dyn ReaderBackend>>,
    closed_reader: Option<crate::native_source_lifetime::MemberIncarnation>,
    destination_role: Option<Arc<destination::AdmissionRole>>,
    participants: Vec<Arc<dyn Participant>>,
}
pub struct Record {
    id: u64,
    admitted_epoch: u64,
    registry: Arc<Gate>,
    domain: Weak<Domain>,
    state: Mutex<RecordState>,
}
impl std::fmt::Debug for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ManagerRecord(<local>)")
    }
}
impl Record {
    pub(crate) fn same_record_registry(&self, other: &Record) -> bool {
        Arc::ptr_eq(&self.registry, &other.registry)
    }
    pub(crate) fn same_registry(&self, registry: &Arc<Gate>) -> bool {
        Arc::ptr_eq(&self.registry, registry)
    }
    pub async fn wait_root_open(&self) -> Result<(), Refused> {
        loop {
            let changed = self.registry.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self
                .registry
                .state
                .lock()
                .map_err(|_| Refused::Unavailable)?
                .seal
                .is_none()
            {
                return match self.phase()? {
                    Phase::Ready => Ok(()),
                    Phase::Deferred | Phase::Loading => Err(Refused::Pending),
                    Phase::Unknown | Phase::Retired => Err(Refused::Unavailable),
                };
            }
            changed.await;
        }
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn domain(&self) -> Option<Arc<Domain>> {
        self.domain.upgrade()
    }
    fn known_domain(&self) -> Result<Arc<Domain>, Refused> {
        match self.domain() {
            Some(domain) => Ok(domain),
            None => {
                let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
                if state.phase != Phase::Retired {
                    state.phase = Phase::Unknown;
                }
                Err(Refused::Unavailable)
            }
        }
    }
    pub fn phase(&self) -> Result<Phase, Refused> {
        let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        if state.phase != Phase::Retired && self.domain.upgrade().is_none() {
            state.phase = Phase::Unknown;
        }
        Ok(state.phase)
    }
    pub fn participant(&self, participant: Arc<dyn Participant>) -> Result<(), Refused> {
        self.state
            .lock()
            .map_err(|_| Refused::Unavailable)?
            .participants
            .push(participant);
        Ok(())
    }
    fn source_stamp(&self) -> Result<Stamp, Refused> {
        let (backend, reader) = {
            let state = self.state.lock().map_err(|_| Refused::Unavailable)?;
            (state.backend.clone(), state.reader.clone())
        };
        if let Some(reader) = reader {
            return reader
                .loaded()
                .map(|loaded| Stamp::Reader(Box::new(loaded)));
        }
        backend
            .ok_or(Refused::Unavailable)?
            .source_stamp()
            .map(Stamp::Manager)
    }
    pub fn backend(&self) -> Result<Option<Arc<dyn Backend>>, Refused> {
        Ok(self
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?
            .backend
            .clone())
    }
    pub fn attach(&self, backend: Arc<dyn Backend>) -> Result<(), Refused> {
        let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        if state.backend.is_some() || state.reader.is_some() {
            return Err(Refused::Busy);
        }
        state.backend = Some(backend);
        Ok(())
    }
    pub fn check_target(&self, target: &Target) -> Result<(), Refused> {
        let (backend, reader) = {
            let state = self.state.lock().map_err(|_| Refused::Unavailable)?;
            if state.phase != Phase::Ready {
                return Err(Refused::Pending);
            }
            (state.backend.clone(), state.reader.clone())
        };
        if let Some(reader) = reader {
            reader.check_target(target)
        } else {
            backend.ok_or(Refused::Unavailable)?.check_target(target)
        }
    }
    pub(crate) fn admission<'a>(
        &'a self,
        parent: Option<&Activity>,
        publication: bool,
        resource: bool,
    ) -> Result<Admission<'a>, Refused> {
        let state = self
            .registry
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        let phase = self.phase()?;
        let constructing = CURRENT_FACTORY
            .try_with(|factory| std::ptr::eq(factory.as_ref(), self))
            .unwrap_or(false);
        if phase != Phase::Ready && !(phase == Phase::Loading && constructing) {
            return Err(Refused::Pending);
        }
        if let Some(seal) = &state.seal {
            let destination = resource && self.destination_resource(parent, seal.epoch);
            if !destination && seal.retiring.load(Ordering::Acquire)
                && (!resource || (publication && parent.is_none()))
            {
                return Err(Refused::Pending);
            }
            if !destination && !constructing
                && !parent.is_some_and(|parent| parent.catalog_before(&self.registry, seal.epoch))
            {
                return Err(Refused::Pending);
            }
        }
        let epoch = parent.map_or(
            if constructing {
                self.admitted_epoch
            } else {
                state.epoch
            },
            Activity::catalog_epoch,
        );
        Ok(Admission {
            _state: state,
            epoch,
        })
    }
    pub fn cache_available(&self) -> bool {
        let constructing = CURRENT_FACTORY
            .try_with(|factory| std::ptr::eq(factory.as_ref(), self))
            .unwrap_or(false);
        let phase = self.phase();
        if phase == Ok(Phase::Loading) && constructing {
            return true;
        }
        if phase != Ok(Phase::Ready) {
            return false;
        }
        self.registry.state.lock().is_ok_and(|state| {
            state.seal.as_ref().is_none_or(|seal| {
                self.destination_resource(crate::assignment::current_activity().as_ref(), seal.epoch)
                || crate::assignment::current_activity()
                    .is_some_and(|parent| parent.catalog_before(&self.registry, seal.epoch))
            })
        })
    }
    pub fn ready(&self) -> Result<(), Refused> {
        let registry = &self.registry;
        let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        if state.phase != Phase::Loading {
            return Err(Refused::Pending);
        }
        state.phase = Phase::Ready;
        drop(state);
        registry.changed.notify_waiters();
        Ok(())
    }
}
#[derive(Default)]
struct State {
    next: u64,
    epoch: u64,
    seal: Option<Arc<SealState>>,
}
#[derive(Default)]
pub struct Registry {
    gate: Arc<Gate>,
    records: Mutex<BTreeMap<u64, Arc<Record>>>,
}
/// Admission state has no ownership edge back to its census/records.
#[derive(Default)]
pub struct Gate {
    state: Mutex<State>,
    changed: Notify,
}
impl std::ops::Deref for Registry {
    type Target = Gate;
    fn deref(&self) -> &Gate {
        &self.gate
    }
}
static GLOBAL: OnceLock<Arc<Registry>> = OnceLock::new();
pub fn global() -> Arc<Registry> {
    Arc::clone(GLOBAL.get_or_init(|| Arc::new(Registry::default())))
}

pub(crate) struct Admission<'a> {
    _state: std::sync::MutexGuard<'a, State>,
    pub(crate) epoch: u64,
}
tokio::task_local! { static CURRENT_FACTORY: Arc<Record>; }
tokio::task_local! { static CURRENT_DESTINATION: Arc<Record>; }
pub async fn factory_scope<F: std::future::Future>(record: Arc<Record>, future: F) -> F::Output {
    CURRENT_FACTORY.scope(record, future).await
}
pub fn current_factory() -> Option<Arc<Record>> {
    CURRENT_FACTORY.try_with(Arc::clone).ok()
}

pub struct Factory {
    record: Arc<Record>,
    domain: Arc<Domain>,
    completed: bool,
    epoch: u64,
}
impl std::fmt::Debug for Factory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ManagerFactory(<local>)")
    }
}
impl Factory {
    pub fn record(&self) -> &Arc<Record> {
        &self.record
    }
    pub fn domain(&self) -> Arc<Domain> {
        Arc::clone(&self.domain)
    }
    pub fn admitted(&self) -> bool {
        self.record.phase() == Ok(Phase::Loading)
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub async fn scope<F: std::future::Future>(&self, future: F) -> Result<F::Output, Refused> {
        if !self.admitted() {
            return Err(Refused::Pending);
        }
        Ok(CURRENT_FACTORY
            .scope(Arc::clone(&self.record), future)
            .await)
    }
    pub fn defer(mut self) -> Result<(), Refused> {
        let mut state = self.record.state.lock().map_err(|_| Refused::Unavailable)?;
        if state.phase != Phase::Loading {
            return Err(Refused::Pending);
        }
        state.phase = Phase::Deferred;
        self.completed = true;
        Ok(())
    }
    pub fn complete(mut self) -> Result<Arc<Record>, Refused> {
        self.record.ready()?;
        self.completed = true;
        Ok(Arc::clone(&self.record))
    }
}
impl Drop for Factory {
    fn drop(&mut self) {
        if !self.completed
            && let Ok(mut state) = self.record.state.lock()
            && state.phase == Phase::Loading
        {
            state.phase = Phase::Unknown;
        }
        self.record.registry.changed.notify_waiters();
    }
}
/// Infallible fail-closed construction for Self-returning upstream constructors.
/// This isolated Unknown record cannot authorize a reader, cache, or root activity.
pub fn refused_factory() -> Factory {
    let registry = Arc::new(Registry::default());
    let domain = Arc::new(Domain::default());
    let record = Arc::new(Record {
        id: 0,
        admitted_epoch: 0,
        registry: Arc::clone(&registry.gate),
        domain: Arc::downgrade(&domain),
        state: Mutex::new(RecordState {
            parent: None,
            phase: Phase::Unknown,
            backend: None,
            reader: None,
            closed_reader: None,
            destination_role: None,
            participants: Vec::new(),
        }),
    });
    // Both values are unpublished; this is the fresh Domain's only attachment.
    let _ = domain.attach_catalog(Arc::clone(&record));
    Factory {
        record,
        domain,
        completed: true,
        epoch: 0,
    }
}
impl Registry {
    pub fn factory(self: &Arc<Self>, parent: Option<&Activity>) -> Result<Factory, Refused> {
        let domain = Arc::new(Domain::default());
        let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        state.next = state.next.checked_add(1).ok_or(Refused::Unavailable)?;
        let admitted = state.seal.as_ref().is_none_or(|seal| {
            !seal.retiring.load(Ordering::Acquire)
                && parent.is_some_and(|parent| parent.catalog_before(&self.gate, seal.epoch))
        });
        let epoch = parent.map_or(state.epoch, Activity::catalog_epoch);
        let record = Arc::new(Record {
            id: state.next,
            admitted_epoch: epoch,
            registry: Arc::clone(&self.gate),
            domain: Arc::downgrade(&domain),
            state: Mutex::new(RecordState {
                parent: admitted.then(|| parent.map(ConstructorParent::capture)).flatten(),
                phase: if admitted {
                    Phase::Loading
                } else {
                    Phase::Deferred
                },
                backend: None,
                reader: None,
                closed_reader: None,
                destination_role: None,
                participants: Vec::new(),
            }),
        });
        domain.attach_catalog(Arc::clone(&record))?;
        self.records
            .lock()
            .map_err(|_| Refused::Unavailable)?
            .insert(record.id, Arc::clone(&record));
        drop(state);
        self.changed.notify_waiters();
        Ok(Factory {
            record,
            domain,
            completed: false,
            epoch,
        })
    }
    pub fn admit_deferred(self: &Arc<Self>, record: &Arc<Record>) -> Result<Factory, Refused> {
        let state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        if state.seal.is_some() {
            return Err(Refused::Pending);
        }
        let mut current = record.state.lock().map_err(|_| Refused::Unavailable)?;
        if current.phase != Phase::Deferred {
            return Err(Refused::Busy);
        }
        current.parent = crate::assignment::current_activity().as_ref().map(ConstructorParent::capture);
        current.phase = Phase::Loading;
        let domain = record.domain().ok_or(Refused::Unavailable)?;
        Ok(Factory {
            record: Arc::clone(record),
            domain,
            completed: false,
            epoch: state.epoch,
        })
    }
    pub fn seal(self: &Arc<Self>) -> Result<Seal, Refused> {
        let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        if state.seal.is_some() {
            return Err(Refused::Busy);
        }
        state.epoch = state.epoch.checked_add(1).ok_or(Refused::Unavailable)?;
        let seal = Arc::new(SealState {
            epoch: state.epoch,
            effects: AtomicBool::new(false),
            retiring: AtomicBool::new(false),
        });
        state.seal = Some(Arc::clone(&seal));
        Ok(Seal {
            registry: Arc::clone(self),
            state: seal,
        })
    }
}
struct SealState {
    epoch: u64,
    effects: AtomicBool,
    retiring: AtomicBool,
}
/// Registry retains closure after Drop/cancellation. Reopening is always explicit.
pub struct Seal {
    registry: Arc<Registry>,
    state: Arc<SealState>,
}
impl std::fmt::Debug for Seal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LocalCatalogSeal(<not-native>)")
    }
}
impl Seal {
    pub fn validate(&self) -> Result<(), Refused> {
        if self
            .registry
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?
            .seal
            .as_ref()
            .is_some_and(|seal| Arc::ptr_eq(seal, &self.state))
        {
            Ok(())
        } else {
            Err(Refused::Busy)
        }
    }
    pub fn records(&self) -> Result<Vec<Arc<Record>>, Refused> {
        self.validate()?;
        Ok(self
            .registry
            .records
            .lock()
            .map_err(|_| Refused::Unavailable)?
            .values()
            .filter(|record| record.admitted_epoch < self.state.epoch)
            .cloned()
            .collect())
    }
    /// Validate the actual retained census before stopping any task or resource.
    /// A missing owner is Unknown, never an implicit NoNativeReader or retirement.
    pub fn preflight_census(&self) -> Result<Vec<Arc<Domain>>, Refused> {
        let mut domains = Vec::new();
        for record in self.records()? {
            if matches!(record.phase()?, Phase::Deferred | Phase::Retired) {
                continue;
            }
            let domain = match record.known_domain() {
                Ok(domain) => domain,
                Err(_) if record.phase()? == Phase::Retired => continue,
                Err(error) => return Err(error),
            };
            if record.phase()? != Phase::Ready {
                return Err(Refused::Pending);
            }
            domains.push(domain);
        }
        Ok(domains)
    }
    pub fn mark_effects(&self) -> Result<(), Refused> {
        self.preflight_census()?;
        self.state.effects.store(true, Ordering::Release);
        Ok(())
    }
    pub fn begin_retirement(&self) -> Result<(), Refused> {
        let state = self
            .registry
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        if !state
            .seal
            .as_ref()
            .is_some_and(|seal| Arc::ptr_eq(seal, &self.state))
        {
            return Err(Refused::Busy);
        }
        let records = self
            .registry
            .records
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        for record in records
            .values()
            .filter(|record| record.admitted_epoch < self.state.epoch)
        {
            if matches!(record.phase()?, Phase::Deferred | Phase::Retired) {
                continue;
            }
            if !record.known_domain()?.catalog_idle(false)? {
                return Err(Refused::Pending);
            }
            if record.domain().is_some()
                && matches!(record.phase()?, Phase::Loading | Phase::Unknown)
            {
                return Err(Refused::Pending);
            }
        }
        self.state.effects.store(true, Ordering::Release);
        self.state.retiring.store(true, Ordering::Release);
        Ok(())
    }
    /// Drain only the actual old constructor epoch. Destination factories need a
    /// separate native-authorized continuation and cannot join this old cohort.
    pub async fn drain_resources(&self) -> Result<(), Refused> {
        let _owned_census = self.preflight_census()?;
        loop {
            self.validate()?;
            let records = self.records()?;
            for record in &records {
                if !matches!(record.phase()?, Phase::Deferred | Phase::Retired) {
                    record.known_domain()?.catalog_wait(false).await?;
                }
            }
            match self.begin_retirement() {
                Ok(()) => break,
                Err(Refused::Pending) => {
                    let changed = self.registry.changed.notified();
                    tokio::pin!(changed);
                    changed.as_mut().enable();
                    let records = self.records()?;
                    if records
                        .iter()
                        .any(|record| record.phase() == Ok(Phase::Unknown))
                    {
                        return Err(Refused::Unavailable);
                    }
                    if records
                        .iter()
                        .any(|record| record.phase() == Ok(Phase::Loading))
                    {
                        changed.await;
                    }
                }
                Err(error) => return Err(error),
            }
        }
        for record in self.records()? {
            if matches!(record.phase()?, Phase::Deferred | Phase::Retired) {
                continue;
            }
            let domain = record.known_domain()?;
            if record.drain_reader(&domain).await? {
                continue;
            }
            let proof = Arc::new(CatalogProof {
                record: Arc::clone(&record),
                domain: Arc::clone(&domain),
                seal: Arc::clone(&self.state),
                stamp: record.source_stamp()?,
            });
            if !crate::assignment::RetirementProof::protects(proof.as_ref(), &domain) {
                return Err(Refused::Pending);
            }
            let participants = record
                .state
                .lock()
                .map_err(|_| Refused::Unavailable)?
                .participants
                .clone();
            for participant in participants {
                participant.retire(&domain, proof.clone()).await?;
            }
            domain.catalog_wait(true).await?;
            if !crate::assignment::RetirementProof::protects(proof.as_ref(), &domain) {
                return Err(Refused::Pending);
            }
        }
        Ok(())
    }
    pub fn cancel_before_effects(self) -> Result<(), Refused> {
        let mut state = self
            .registry
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        if !state
            .seal
            .as_ref()
            .is_some_and(|seal| Arc::ptr_eq(seal, &self.state))
            || self.state.effects.load(Ordering::Acquire)
        {
            return Err(Refused::Busy);
        }
        state.seal = None;
        drop(state);
        self.registry.changed.notify_waiters();
        Ok(())
    }
}

/// Actual manager owner watch; no credential bytes and no native source authority.
pub trait Version: Send + Sync {
    fn source_epoch(&self) -> Result<Option<crate::manager_source_epoch::Epoch>, Refused> {
        Ok(None)
    }
    fn current(&self) -> crate::assignment::Generations;
    fn changed<'a>(
        &'a self,
        observed: &'a crate::assignment::Generations,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;
}
#[derive(Clone)]
pub struct Scope {
    domain: Arc<Domain>,
    version: Arc<dyn Version>,
    owner: u64,
    login: u64,
    source: u64,
    epoch: Option<crate::manager_source_epoch::Epoch>,
    native: Option<crate::native_source_lifetime::NetworkScope>,
}
impl std::fmt::Debug for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ManagerSourceScope(<local>)")
    }
}
impl PartialEq for Scope {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.domain, &other.domain)
            && self.owner == other.owner
            && self.login == other.login
            && self.native == other.native
            && match (&self.epoch, &other.epoch) {
                (Some(left), Some(right)) => left.same(right),
                (None, None) => self.native.is_some() || self.source == other.source,
                _ => false,
            }
    }
}
impl Eq for Scope {}
impl Scope {
    pub fn capture(domain: Arc<Domain>, version: Arc<dyn Version>) -> Result<Self, Refused> {
        let record = domain.catalog().ok_or(Refused::Unavailable)?;
        if !record.cache_available() {
            return Err(Refused::Pending);
        }
        let epoch = version.source_epoch()?;
        if epoch
            .as_ref()
            .is_some_and(|epoch| !Arc::ptr_eq(&epoch.domain(), &domain))
        {
            return Err(Refused::WrongDomain);
        }
        let stamp = version.current();
        Ok(Self {
            domain,
            version,
            owner: stamp.owner,
            login: stamp.login,
            source: stamp.source,
            epoch,
            native: None,
        })
    }
    pub fn matches_native(&self, native: &crate::native_source_lifetime::NetworkScope) -> bool {
        self.native.as_ref() == Some(native) && native.belongs_to(&self.domain)
    }
    pub fn with_native_scope(
        mut self,
        scope: &crate::native_source_lifetime::NetworkScope,
    ) -> Self {
        if scope.preserves_grant() {
            self.native = Some(scope.clone());
        }
        self
    }
    fn matches(&self, stamp: &crate::assignment::Generations) -> bool {
        stamp.owner == self.owner
            && stamp.login == self.login
            && (self.epoch.is_some() || self.native.is_some() || stamp.source == self.source)
            && match (&self.epoch, self.version.source_epoch()) {
                (Some(expected), Ok(Some(current))) => expected.active() && expected.same(&current),
                (None, Ok(None)) => true,
                _ => false,
            }
    }
    pub fn revoked(&self) -> bool {
        let current = self.version.current();
        self.epoch.as_ref().is_some_and(|epoch| !epoch.active())
            || current.owner != self.owner
            || current.login != self.login
            || matches!((&self.epoch, self.version.source_epoch()), (Some(expected), Ok(Some(current))) if !expected.same(&current))
    }
    fn live(&self) -> bool {
        self.matches(&self.version.current())
    }
    pub fn acquire(&self) -> Result<Use, Refused> {
        if !self.live() {
            return Err(Refused::Unavailable);
        }
        let activity = self.domain.admit_current_resource().or_else(|error| {
            match crate::assignment::current_activity().as_ref() {
                Some(parent) => self.domain.admit_catalog_resource(parent),
                None => Err(error),
            }
        })?;
        if !self.live() {
            return Err(Refused::Unavailable);
        }
        Ok(Use {
            scope: self.clone(),
            _activity: activity,
        })
    }
}
#[derive(Clone)]
pub struct Use {
    scope: Scope,
    _activity: Activity,
}
impl std::fmt::Debug for Use {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ManagerSourceUse(<local>)")
    }
}
impl Use {
    pub fn acquire_native(&self, native: &crate::native_source_lifetime::NetworkScope) -> std::io::Result<crate::native_source_lifetime::NetworkUse> {
        if !self.live() || !native.belongs_to(&self.scope.domain) { return Err(crate::managed_auth::unavailable()); }
        native.acquire_for(Some(self._activity.clone()))
    }
    pub fn live(&self) -> bool {
        self.scope.live()
    }
    pub async fn revoked(&self) {
        loop {
            if !self.live() {
                return;
            }
            let observed = self.scope.version.current();
            if !self.scope.matches(&observed) {
                return;
            }
            self.scope.version.changed(&observed).await;
        }
    }
}

struct CatalogProof {
    record: Arc<Record>,
    domain: Arc<Domain>,
    seal: Arc<SealState>,
    stamp: Stamp,
}
impl crate::assignment::retirement_sealed::Sealed for CatalogProof {}
impl crate::assignment::RetirementProof for CatalogProof {
    fn protects(&self, domain: &Arc<Domain>) -> bool {
        Arc::ptr_eq(domain, &self.domain)
            && self.record.registry.state.lock().is_ok_and(|state| {
                state
                    .seal
                    .as_ref()
                    .is_some_and(|seal| Arc::ptr_eq(seal, &self.seal))
            })
            && self.seal.retiring.load(Ordering::Acquire)
            && self.domain.catalog_idle(false) == Ok(true)
            && self.record.source_stamp() == Ok(self.stamp.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manager(registry: &Arc<Registry>) -> Arc<Domain> {
        let factory = registry.factory(None).unwrap();
        let domain = factory.domain();
        factory.complete().unwrap();
        domain
    }
    #[test]
    fn constructor_and_root_admission_are_closed_atomically_and_explicit_cancel_is_pre_effect_only()
    {
        let registry = Arc::new(Registry::default());
        let domain = manager(&registry);
        let old = domain.admit_work(None).unwrap();
        let seal = registry.seal().unwrap();
        let deferred = registry.factory(None).unwrap();
        assert!(!deferred.admitted());
        assert!(!deferred.record().cache_available());
        assert!(domain.admit_work(None).is_err());
        let child = domain.admit_work(Some(&old)).unwrap();
        let child_factory = registry.factory(Some(&child)).unwrap();
        assert!(child_factory.admitted());
        let _child_domain = child_factory.domain();
        child_factory.complete().unwrap();
        drop(child);
        drop(old);
        seal.cancel_before_effects().unwrap();
        assert!(domain.admit_work(None).is_ok());
        let seal = registry.seal().unwrap();
        seal.mark_effects().unwrap();
        assert!(seal.cancel_before_effects().is_err());
        assert!(domain.admit_work(None).is_err());
    }
    #[test]
    fn dropped_seal_stays_closed_and_dropped_loading_owner_is_not_none_or_no_reader() {
        let registry = Arc::new(Registry::default());
        let loading = registry.factory(None).unwrap();
        let domain = loading.domain();
        let record = loading.record().clone();
        let seal = registry.seal().unwrap();
        drop(loading);
        assert_eq!(record.phase().unwrap(), Phase::Unknown);
        assert_eq!(seal.begin_retirement(), Err(Refused::Pending));
        drop(seal);
        assert!(!registry.factory(None).unwrap().admitted());
        assert!(domain.admit_work(None).is_err());
    }
    #[test]
    fn domain_keeps_exact_record_alive_after_manager_anchor_is_gone_without_self_cycle() {
        let registry = Arc::new(Registry::default());
        let factory = registry.factory(None).unwrap();
        let domain = factory.domain();
        let record = factory.complete().unwrap();
        let weak = Arc::downgrade(&record);
        let body = domain.admit_resource(None).unwrap();
        drop(record);
        drop(domain);
        assert!(
            weak.upgrade().is_some(),
            "live real resource owns its Domain/record"
        );
        drop(body);
        let record = weak.upgrade().unwrap();
        assert!(
            record.domain().is_none(),
            "record must not retain a Domain cycle"
        );
        let seal = registry.seal().unwrap();
        assert!(seal.preflight_census().is_err());
        assert_eq!(record.phase().unwrap(), Phase::Unknown);
        seal.cancel_before_effects().unwrap();
    }
    #[test]
    fn constructor_seal_race_never_grants_post_seal_root_credentials() {
        for _ in 0..32 {
            let registry = Arc::new(Registry::default());
            let start = Arc::new(std::sync::Barrier::new(2));
            let other = registry.clone();
            let ready = start.clone();
            let factory = std::thread::spawn(move || {
                ready.wait();
                other.factory(None).unwrap()
            });
            start.wait();
            let seal = registry.seal().unwrap();
            let factory = factory.join().unwrap();
            if factory.admitted() {
                assert_eq!(
                    seal.begin_retirement(),
                    Err(Refused::Pending),
                    "already admitted load remains in census"
                );
            } else {
                assert_eq!(factory.record().phase().unwrap(), Phase::Deferred);
                assert!(!factory.record().cache_available());
            }
            assert!(!registry.factory(None).unwrap().admitted());
        }
    }
    struct Owner(std::sync::atomic::AtomicU64);
    impl Version for Owner {
        fn current(&self) -> crate::assignment::Generations {
            let owner = self.0.load(Ordering::Acquire);
            crate::assignment::Generations {
                auth: 0,
                owner,
                source: 0,
                login: owner,
            }
        }
        fn changed<'a>(
            &'a self,
            _: &'a crate::assignment::Generations,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
            Box::pin(std::future::pending())
        }
    }
    impl Backend for Owner {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn check_target(&self, _: &Target) -> Result<(), Refused> {
            Ok(())
        }
        fn source_stamp(&self) -> Result<crate::assignment::Generations, Refused> {
            Ok(self.current())
        }
    }
    #[tokio::test]
    async fn cached_factory_has_zero_activity_but_body_clones_keep_catalog_retirement_pending() {
        let registry = Arc::new(Registry::default());
        let factory = registry.factory(None).unwrap();
        let domain = factory.domain();
        let record = factory.record().clone();
        let owner = Arc::new(Owner(std::sync::atomic::AtomicU64::new(1)));
        record.attach(owner.clone()).unwrap();
        factory.complete().unwrap();
        let cached_factory = Scope::capture(domain.clone(), owner.clone()).unwrap();
        assert!(domain.catalog_idle(true).unwrap());
        let body = cached_factory.acquire().unwrap();
        let child = body.clone();
        let seal = registry.seal().unwrap();
        assert!(cached_factory.acquire().is_err());
        drop(body);
        let drain = seal.drain_resources();
        tokio::pin!(drain);
        tokio::select! { _ = &mut drain => panic!("live body clone is not retired"), _ = tokio::task::yield_now() => {} }
        drop(child);
        drain.await.unwrap();
        assert!(domain.catalog_idle(true).unwrap());
        assert!(
            cached_factory.acquire().is_err(),
            "cached factory cannot resurrect work after drain"
        );
        owner.0.store(2, Ordering::Release);
        assert!(
            !cached_factory.live(),
            "old-source factory cannot adopt a new owner"
        );
    }
    #[tokio::test]
    async fn catalog_closed_waiter_does_not_spin_or_open_before_explicit_cancel() {
        let registry = Arc::new(Registry::default());
        let domain = manager(&registry);
        let seal = registry.seal().unwrap();
        let waiting = domain.wait_open();
        tokio::pin!(waiting);
        tokio::select! { _ = &mut waiting => panic!("closed catalog must remain pending"), _ = tokio::task::yield_now() => {} }
        seal.cancel_before_effects().unwrap();
        waiting.await.unwrap();
    }
    struct SourceOnly(std::sync::atomic::AtomicU64);
    impl Version for SourceOnly {
        fn current(&self) -> crate::assignment::Generations {
            crate::assignment::Generations {
                auth: 0,
                owner: 1,
                source: self.0.load(Ordering::Acquire),
                login: 1,
            }
        }
        fn changed<'a>(
            &'a self,
            _: &'a crate::assignment::Generations,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
            Box::pin(std::future::pending())
        }
    }
    #[test]
    fn same_principal_without_verified_native_grant_does_not_hide_a_source_change() {
        let registry = Arc::new(Registry::default());
        let domain = manager(&registry);
        let source = Arc::new(SourceOnly(std::sync::atomic::AtomicU64::new(1)));
        let old = Scope::capture(domain.clone(), source.clone()).unwrap();
        let body = old.acquire().unwrap();
        source.0.store(2, Ordering::Release);
        assert!(!body.live());
        assert!(old.acquire().is_err());
        let current = Scope::capture(domain, source).unwrap();
        assert_ne!(old, current);
        assert!(current.acquire().is_ok());
    }
    #[tokio::test]
    async fn refused_factory_is_unknown_and_never_reads_publishes_or_waits_for_impossible_admission()
     {
        let factory = refused_factory();
        assert_eq!(factory.record().phase().unwrap(), Phase::Unknown);
        assert!(!factory.admitted());
        assert!(!factory.record().cache_available());
        assert!(factory.domain().admit_work(None).is_err());
        assert_eq!(
            factory.domain().wait_open().await,
            Err(Refused::Unavailable)
        );
        assert!(factory.complete().is_err());
    }
    #[test]
    fn lost_admitted_owner_is_retained_unknown_and_refuses_before_effects_without_closing_factories()
     {
        let registry = Arc::new(Registry::default());
        let domain = manager(&registry);
        let record = domain.catalog().unwrap().clone();
        drop(domain);
        let seal = registry.seal().unwrap();
        assert_eq!(seal.mark_effects(), Err(Refused::Unavailable));
        assert_eq!(record.phase().unwrap(), Phase::Unknown);
        assert_eq!(seal.records().unwrap().len(), 1);
        seal.cancel_before_effects().unwrap();
        assert!(registry.factory(None).unwrap().admitted());
    }
    #[test]
    fn census_records_do_not_keep_the_registry_or_admission_gate_alive() {
        let registry = Arc::new(Registry::default());
        let domain = manager(&registry);
        let record = Arc::downgrade(domain.catalog().unwrap());
        let gate = Arc::downgrade(&registry.gate);
        drop(domain);
        assert!(
            record.upgrade().is_some(),
            "census retains unknown admission"
        );
        drop(registry);
        assert!(record.upgrade().is_none(), "no census ownership cycle");
        assert!(
            gate.upgrade().is_none(),
            "no admission gate ownership cycle"
        );
    }
}
