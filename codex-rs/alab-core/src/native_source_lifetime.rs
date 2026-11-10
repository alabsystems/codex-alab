// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A source's native membership outlives its manager when credential snapshots or
//! derived transports still retain it. Normal credential refresh does not change scope.

use crate::assignment::Generations;
use crate::managed_auth::unavailable;
use crate::native_source::Borrow;
use crate::native_source::Loaded;
use crate::native_source::NoNativeReader;
use crate::native_source::Preload;
use crate::native_source::Source;
use crate::native_source_launch::Allocation;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

#[path = "native_source_preload.rs"]
mod preload_actor;
pub use preload_actor::ReboundPreparation;
pub use preload_actor::ReboundReady;

#[derive(Clone, PartialEq, Eq)]
struct Scope {
    slot: String,
    path: std::path::PathBuf,
    account: String,
    user: String,
    assignment: u64,
    intent: Option<String>,
}
impl From<&Source> for Scope {
    fn from(source: &Source) -> Self {
        Self {
            slot: source.slot.clone(),
            path: source.path.clone(),
            account: source.account.clone(),
            user: source.user.clone(),
            assignment: source.assignment_generation,
            intent: source.native_intent.clone(),
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
enum Kind {
    Managed(Scope),
    Unrelated(NoNativeReader),
}
#[derive(Default)]
struct State {
    registration: Option<String>,
    preload_operation: Option<String>,
    attempted_preload: Option<crate::native_preload::Body>,
    flight: Option<Arc<preload_actor::Flight>>,
    snapshot: Option<(crate::managed_auth::Revision, Generations)>,
    kind: Option<Kind>,
    reader: bool,
    preloaded: Option<crate::managed_auth::Revision>,
    seal_occurrence: Arc<()>,
}
struct Inner {
    allocation: Allocation,
    state: Mutex<State>,
    ready: AtomicBool,
    ready_work: std::sync::atomic::AtomicU64,
    active: AtomicBool,
    domain: std::sync::OnceLock<Arc<crate::assignment::Domain>>,
    changed: tokio::sync::Notify,
    retiring: AtomicBool,
    retirement: crate::native_reader_retirement::Owner,
    preload_order: crate::native_preload::Order,
    // Serial admission stays owned by the spawned PRELOAD actor through completion.
    preload_creation: tokio::sync::Semaphore,
}
impl Drop for Inner {
    fn drop(&mut self) {
        // Last actual storage/snapshot/body ownership captures immutable cleanup.
        // An observer retains metadata, not this Lifetime or an activity lease.
        let (attempt, registration) = match self.state.get_mut() {
            Ok(state) => (
                state.flight.as_ref().map(|flight| flight.attempt.clone()),
                if state
                    .attempted_preload
                    .as_ref()
                    .map(|attempt| &attempt.operation)
                    == state.preload_operation.as_ref()
                {
                    state.registration.clone()
                } else {
                    None
                },
            ),
            Err(_) => {
                self.retirement.fail();
                return;
            }
        };
        if self.retirement.capture(attempt, registration).is_err() {
            self.retirement.fail();
        }
    }
}

/// This lease contains no token and is redacted. It is only created from an actual
/// native-issued member; source access still requires verified preload and local policy.
#[derive(Clone)]
pub struct Lifetime(Arc<Inner>);
impl std::fmt::Debug for Lifetime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeSourceLifetime(<private>)")
    }
}
/// Non-owning incarnation evidence for immutable ACK history; it owns no use lease.
#[derive(Clone)]
pub struct MemberIncarnation(std::sync::Weak<Inner>);
impl MemberIncarnation {
    /// Liveness metadata only, never a Ready or source-use permission.
    pub fn live(&self) -> bool {
        self.upgrade().is_some_and(|member| member.live())
    }
    pub(crate) fn upgrade(&self) -> Option<Lifetime> {
        self.0.upgrade().map(Lifetime)
    }
    pub fn reader_still_loaded(&self, loaded: &ReaderLoaded) -> bool {
        self.upgrade()
            .is_some_and(|member| member.reader_still_loaded(loaded))
    }
}
impl PartialEq for MemberIncarnation {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Weak::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for MemberIncarnation {}
impl std::fmt::Debug for MemberIncarnation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeMemberIncarnation(<private>)")
    }
}
/// Exact loaded direct-reader observation, not manager AuthChangeState or a use lease.
#[derive(Clone, PartialEq, Eq)]
pub struct ReaderLoaded {
    member: MemberIncarnation,
    registration: String,
    kind: ReaderKind,
}
#[derive(Clone, PartialEq, Eq)]
enum ReaderKind {
    Managed {
        scope: Scope,
        revision: crate::managed_auth::Revision,
        operation: String,
    },
    Unrelated(NoNativeReader),
}
impl ReaderLoaded {
    pub fn no_native_reader(&self) -> Option<NoNativeReader> {
        match &self.kind {
            ReaderKind::Unrelated(kind) => Some(*kind),
            ReaderKind::Managed { .. } => None,
        }
    }
}
impl Lifetime {
    pub fn new(allocation: Allocation) -> io::Result<Self> {
        if !crate::managed_auth::valid_hex(&allocation.operation, 32)
            || !crate::managed_auth::valid_hex(allocation.member.id(), 32)
            || !allocation.member.live()
        {
            if allocation.target.is_some() {
                crate::native_target::retire_unused(allocation);
            }
            return Err(unavailable());
        }
        let retirement = match allocation.target.as_ref() {
            Some(destination) => crate::native_reader_retirement::Owner::with_target(
                allocation.member.clone(),
                destination.clone(),
                allocation.operation.clone(),
            ),
            None => crate::native_reader_retirement::Owner::new(allocation.member.clone()),
        };
        let member = Self(Arc::new(Inner {
            allocation,
            state: Mutex::default(),
            ready: AtomicBool::new(false),
            ready_work: std::sync::atomic::AtomicU64::new(0),
            active: AtomicBool::new(false),
            domain: std::sync::OnceLock::new(),
            changed: tokio::sync::Notify::new(),
            retiring: AtomicBool::new(false),
            retirement,
            preload_order: Default::default(),
            preload_creation: tokio::sync::Semaphore::new(1),
        }));
        crate::native_source_launch::destination_member_created(&member)?;
        Ok(member)
    }
    pub(crate) fn constructor_operation(&self) -> &str {
        &self.0.allocation.operation
    }
    pub(crate) fn target_role(&self) -> Option<Arc<dyn crate::native_target::Destination>> {
        self.0.allocation.target.as_ref().map(Arc::clone)
    }
    pub fn incarnation(&self) -> MemberIncarnation {
        MemberIncarnation(Arc::downgrade(&self.0))
    }
    pub fn reader_loaded(&self) -> io::Result<ReaderLoaded> {
        let state = self.0.state.lock().map_err(|_| unavailable())?;
        if !self.ready() {
            return Err(unavailable());
        }
        let registration = state.registration.clone().ok_or_else(unavailable)?;
        let kind = match (&state.kind, state.reader) {
            (Some(Kind::Managed(scope)), true) => ReaderKind::Managed {
                scope: scope.clone(),
                revision: state.preloaded.clone().ok_or_else(unavailable)?,
                operation: state.preload_operation.clone().ok_or_else(unavailable)?,
            },
            (Some(Kind::Unrelated(kind)), false) => ReaderKind::Unrelated(*kind),
            _ => return Err(unavailable()),
        };
        Ok(ReaderLoaded {
            member: self.incarnation(),
            registration,
            kind,
        })
    }
    pub fn reader_still_loaded(&self, loaded: &ReaderLoaded) -> bool {
        self.reader_loaded().is_ok_and(|current| current == *loaded)
    }
    pub async fn reader_changed(&self, loaded: &ReaderLoaded) {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self.reader_still_loaded(loaded) {
                return;
            }
            tokio::select! { _ = changed => {}, _ = self.0.allocation.member.closed() => return }
        }
    }
    /// Negative debug fault on this exact retained member. It neither retries
    /// a connection nor changes Ready/retirement/native-use observations.
    #[cfg(debug_assertions)]
    pub fn debug_close_work(&self) -> io::Result<()> {
        self.0.allocation.member.debug_close_work()
    }
    pub fn retirement_observer(&self) -> crate::native_reader_retirement::Observer {
        self.0.retirement.observer(self.incarnation())
    }
    pub(crate) fn retire_reader(&self) -> io::Result<crate::native_reader_retirement::Observer> {
        self.0.retiring.store(true, Ordering::Release);
        self.seal();
        let state = self.0.state.lock().map_err(|_| unavailable())?;
        self.0.retirement.capture(
            state.flight.as_ref().map(|flight| flight.attempt.clone()),
            if state
                .attempted_preload
                .as_ref()
                .map(|attempt| &attempt.operation)
                == state.preload_operation.as_ref()
            {
                state.registration.clone()
            } else {
                None
            },
        )?;
        Ok(self.retirement_observer())
    }
    pub fn domain(&self) -> Option<Arc<crate::assignment::Domain>> {
        self.0.domain.get().cloned()
    }
    pub fn member(&self) -> &str {
        self.0.allocation.member.id()
    }
    /// Liveness is NOT a source or readiness proof.
    pub fn live(&self) -> bool {
        self.0.allocation.member.live()
    }
    /// Scope admission only. A new credential-consuming request also needs the exact
    /// current read snapshot through source_use(); this is not a fresh-Ready proof.
    pub fn ready(&self) -> bool {
        self.live()
            && !self.0.retiring.load(Ordering::Acquire)
            && self.0.ready.load(Ordering::Acquire)
            && self.0.ready_work.load(Ordering::Acquire)
                == self.0.allocation.member.work_incarnation()
    }
    pub fn unrelated_kind(&self) -> io::Result<Option<NoNativeReader>> {
        let state = self.0.state.lock().map_err(|_| unavailable())?;
        Ok(match &state.kind {
            Some(Kind::Unrelated(kind)) => Some(*kind),
            _ => None,
        })
    }
    /// Stop new credential extraction and derived admissions without deleting storage.
    pub fn seal(&self) {
        // Serialize revocation with retained-Ready recovery publication. Poison
        // still closes access and prevents any subsequent recovery commit.
        let state = self.0.state.lock();
        if let Ok(mut state) = state {
            state.seal_occurrence = Arc::new(());
            self.0.ready.store(false, Ordering::Release);
            self.0.active.store(false, Ordering::Release);
        } else {
            self.0.ready.store(false, Ordering::Release);
            self.0.active.store(false, Ordering::Release);
        }
        self.0.changed.notify_waiters();
    }
    pub fn attach_domain(&self, domain: Arc<crate::assignment::Domain>) -> io::Result<()> {
        let actual = self.0.domain.get_or_init(|| domain.clone());
        if !Arc::ptr_eq(actual, &domain) {
            return Err(unavailable());
        }
        domain.register_native_member(self.incarnation())?;
        Ok(())
    }
    pub(crate) fn native_use_seed(
        &self,
    ) -> io::Result<Option<(Arc<dyn crate::native_source::Member>, String)>> {
        let state = self.0.state.lock().map_err(|_| unavailable())?;
        match state.kind.as_ref() {
            Some(Kind::Unrelated(_)) => Ok(None),
            Some(Kind::Managed(_))
                if self.ready()
                    && (state.reader
                        || state.snapshot.as_ref().is_some_and(|(revision, _)| {
                            Some(revision) == state.preloaded.as_ref()
                        })) =>
            {
                Ok(Some((
                    Arc::clone(&self.0.allocation.member),
                    state.registration.clone().ok_or_else(unavailable)?,
                )))
            }
            _ => Err(unavailable()),
        }
    }
    pub(crate) fn source_domain(&self) -> Arc<crate::assignment::Domain> {
        self.0
            .domain
            .get_or_init(|| Arc::new(crate::assignment::Domain::default()))
            .clone()
    }
    /// A factory owns this scope, but does not count as an admitted request.
    pub fn network_scope(&self) -> io::Result<NetworkScope> {
        let state = self.0.state.lock().map_err(|_| unavailable())?;
        let identity = match state.kind.clone().ok_or_else(unavailable)? {
            Kind::Managed(scope) => {
                NetworkIdentity::Managed(scope, state.preloaded.clone().ok_or_else(unavailable)?)
            }
            Kind::Unrelated(kind) => NetworkIdentity::Unrelated(kind),
        };
        let domain = self
            .0
            .domain
            .get_or_init(|| Arc::new(crate::assignment::Domain::default()))
            .clone();
        Ok(NetworkScope {
            lifetime: self.clone(),
            identity,
            domain,
        })
    }
    pub(crate) fn derived_from(&self, activity: &crate::assignment::Activity) -> io::Result<bool> {
        match self
            .0
            .allocation
            .parent_use
            .as_ref()
            .and_then(crate::native_source_use::ParentUseRef::upgrade)
        {
            Some(parent) => activity.owns_native_use(&parent),
            None => Ok(false),
        }
    }
    pub(crate) fn member_handle(&self) -> &Arc<dyn crate::native_source::Member> {
        &self.0.allocation.member
    }
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub async fn preload(&self, operation: &str, borrow: Option<&Borrow>) -> io::Result<Preload> {
        preload_actor::observe(self, operation, borrow).await
    }
    /// Explicit constructor-owned source-only rebound; no HTTP use is minted.
    pub async fn rebound_preload(&self, operation: &str) -> io::Result<Preload> {
        preload_actor::observe_rebound(self, operation).await
    }
    pub async fn prepare_loaded_ready(
        &self,
        source: &Source,
        generations: &Generations,
    ) -> io::Result<ReboundPreparation> {
        preload_actor::prepare_loaded_ready(self, source, generations).await
    }
    /// Repair the exact pending observation, retaining its original operation,
    /// sequence and source role. This is not a new scope or a refresh grant.
    pub async fn resample_preload(
        &self,
        previous: crate::native_preload::Predecessor,
    ) -> io::Result<crate::native_preload::Resample> {
        preload_actor::resample(self, previous).await
    }
    /// Storage may read after authenticated preload while the native constructor gate
    /// remains held. A derived request must additionally require ready().
    /// Replay only the retained actual report. A committed hint is not an ACK.
    pub async fn resume_preload_completion(&self) -> io::Result<()> {
        preload_actor::resume_completion(self).await
    }
    pub async fn resume_source_ready(
        &self,
        source: &Source,
        generations: &Generations,
    ) -> io::Result<()> {
        preload_actor::resume_source_completion(self, source, generations).await
    }
    /// Rebind only a previously published exact manager snapshot. Native ACK
    /// precedes the caller's separately guarded synchronous cache validation.
    pub async fn prepare_rebound_ready(
        &self,
        source: &Source,
        generations: &Generations,
    ) -> io::Result<ReboundPreparation> {
        preload_actor::prepare_rebound_ready(self, source, generations).await
    }
    /// Actual frozen completion metadata, not a native Ready acknowledgment.
    pub fn retained_preload_completion(&self) -> io::Result<Option<Loaded>> {
        preload_actor::retained_completion(self)
    }
    pub fn permits_storage(&self, source: &Source) -> io::Result<()> {
        source.validate()?;
        let state = self.0.state.lock().map_err(|_| unavailable())?;
        if self.0.retiring.load(Ordering::Acquire)
            || !self.live()
            || state.kind.as_ref() != Some(&Kind::Managed(Scope::from(source)))
        {
            return Err(unavailable());
        }
        Ok(())
    }
    pub fn permits_revision(&self, revision: &crate::managed_auth::Revision) -> io::Result<()> {
        if self.0.retiring.load(Ordering::Acquire)
            || !self.live()
            || self
                .0
                .state
                .lock()
                .map_err(|_| unavailable())?
                .preloaded
                .as_ref()
                != Some(revision)
        {
            return Err(unavailable());
        }
        Ok(())
    }
    async fn complete_preload(
        &self,
        operation: &str,
        registration: &str,
        loaded: &Loaded,
    ) -> io::Result<()> {
        let flight = self
            .0
            .state
            .lock()
            .map_err(|_| unavailable())?
            .flight
            .clone()
            .ok_or_else(unavailable)?;
        if flight.body().operation != operation {
            return Err(unavailable());
        }
        flight
            .complete(Arc::downgrade(&self.0), registration.into(), loaded.clone())
            .await
    }
    /// Registered refresh always uses actual admitted native work. There is no
    /// legacy refresh fallback when this member exists.
    pub async fn refresh(
        &self,
        slot: &str,
        digest: &str,
    ) -> io::Result<crate::managed_auth::Revision> {
        {
            let state = self.0.state.lock().map_err(|_| unavailable())?;
            if state.reader
                || !matches!(state.kind.as_ref(), Some(Kind::Managed(scope)) if scope.slot == slot)
            {
                return Err(unavailable());
            }
        }
        // Captured PRELOAD completion is not new HTTP/refresh permission.
        let parent = crate::assignment::current_activity()
            .ok_or_else(unavailable)?
            .native_use_for(self)?
            .ok_or_else(unavailable)?;
        let receipt = parent.ready().await?;
        let host = MemberRefresh {
            member: Arc::clone(&self.0.allocation.member),
            receipt,
            _parent: parent,
        };
        crate::managed_auth::refresh_using(slot, digest, &host).await
    }
    pub async fn source_ready(&self, source: &Source, generations: Generations) -> io::Result<()> {
        if self.0.state.lock().map_err(|_| unavailable())?.reader {
            return Err(unavailable());
        }
        self.permits_storage(source)?;
        let (registration, operation) = {
            let state = self.0.state.lock().map_err(|_| unavailable())?;
            if state.preloaded.as_ref() != Some(&source.revision) {
                return Err(unavailable());
            }
            (
                state.registration.clone().ok_or_else(unavailable)?,
                state.preload_operation.clone().ok_or_else(unavailable)?,
            )
        };
        self.complete_preload(
            &operation,
            &registration,
            &Loaded::Source {
                source: Box::new(source.clone()),
                generations,
            },
        )
        .await
    }
    /// Direct-reader completion uses no manager counters.
    pub async fn reader_ready(&self, source: &Source) -> io::Result<()> {
        self.permits_storage(source)?;
        let (registration, operation) = {
            let state = self.0.state.lock().map_err(|_| unavailable())?;
            if state.preloaded.as_ref() != Some(&source.revision) {
                return Err(unavailable());
            }
            (
                state.registration.clone().ok_or_else(unavailable)?,
                state.preload_operation.clone().ok_or_else(unavailable)?,
            )
        };
        self.complete_preload(
            &operation,
            &registration,
            &Loaded::ReaderSource {
                source: Box::new(source.clone()),
            },
        )
        .await
    }

    /// Admit a new derived request using the actual current native/doc/AuthState
    /// snapshot. The resulting use retains immutable scope across same-grant refresh.
    pub fn source_use(&self, source: &Source, generations: &Generations) -> io::Result<SourceUse> {
        self.permits_storage(source)?;
        let state = self.0.state.lock().map_err(|_| unavailable())?;
        if !self.ready()
            || state.snapshot.as_ref() != Some(&(source.revision.clone(), generations.clone()))
        {
            return Err(unavailable());
        }
        Ok(SourceUse(self.clone()))
    }
    /// Describe an actual destination Ready snapshot. This is not a resource
    /// completion or ACK; the native endpoint independently checks this record.
    pub fn target_report(
        &self,
        source: &Source,
        generations: Generations,
        boundary: &str,
    ) -> io::Result<crate::native_target::ResourceReport> {
        self.permits_storage(source)?;
        let target = self.0.allocation.target.as_ref().ok_or_else(unavailable)?;
        let state = self.0.state.lock().map_err(|_| unavailable())?;
        if !self.ready()
            || state.reader
            || state.snapshot.as_ref() != Some(&(source.revision.clone(), generations.clone()))
        {
            return Err(unavailable());
        }
        crate::native_target::ResourceReport::from_loaded(
            target.context(),
            self.member().into(),
            state.registration.clone().ok_or_else(unavailable)?,
            boundary.into(),
            source.clone(),
            generations,
        )
    }
    pub async fn unrelated_ready(&self, kind: NoNativeReader) -> io::Result<()> {
        {
            let state = self.0.state.lock().map_err(|_| unavailable())?;
            if self.0.retiring.load(Ordering::Acquire)
                || !self.live()
                || state
                    .kind
                    .as_ref()
                    .is_some_and(|previous| previous != &Kind::Unrelated(kind))
            {
                return Err(unavailable());
            }
        }
        self.0.allocation.member.reconcile_work().await?;
        let work = self.0.allocation.member.work_incarnation();
        let receipt = self
            .0
            .allocation
            .member
            .ready(
                &self.0.allocation.operation,
                None,
                &Loaded::NoNativeReader(kind),
            )
            .await?;
        receipt.validate(None)?;
        if work != self.0.allocation.member.work_incarnation() {
            return Err(unavailable());
        }
        let mut state = self.0.state.lock().map_err(|_| unavailable())?;
        if self.0.retiring.load(Ordering::Acquire)
            || !self.live()
            || state
                .kind
                .as_ref()
                .is_some_and(|previous| previous != &Kind::Unrelated(kind))
        {
            return Err(unavailable());
        }
        state.kind = Some(Kind::Unrelated(kind));
        state.registration = Some(receipt.registration);
        self.0.active.store(true, Ordering::Release);
        self.0.ready_work.store(work, Ordering::Release);
        self.0.ready.store(true, Ordering::Release);
        Ok(())
    }
    /// Actual policy refusal with no cache. It does not mark a credential source Ready.
    pub async fn unavailable(&self, operation: &str) -> io::Result<()> {
        if self.0.state.lock().map_err(|_| unavailable())?.reader {
            return Err(unavailable());
        }
        self.0.ready.store(false, Ordering::Release);
        if self
            .0
            .state
            .lock()
            .map_err(|_| unavailable())?
            .preload_operation
            .as_deref()
            != Some(operation)
        {
            return Err(unavailable());
        }
        let registration = self
            .0
            .state
            .lock()
            .map_err(|_| unavailable())?
            .registration
            .clone()
            .ok_or_else(unavailable)?;
        self.complete_preload(operation, &registration, &Loaded::PolicyUnavailable)
            .await
    }
}

struct MemberRefresh {
    member: Arc<dyn crate::native_source::Member>,
    receipt: Arc<crate::native_source_use::UseReceipt>,
    _parent: crate::native_source_use::ParentUseLease,
}
impl crate::managed_auth::Host for MemberRefresh {
    fn refresh<'a>(
        &'a self,
        request: &'a crate::managed_auth::Request,
    ) -> crate::managed_auth::RefreshFuture<'a> {
        self.member.refresh_use(request, &self.receipt)
    }
}

/// A request/body/WebSocket lifetime. Credential revision is intentionally not its
/// identity: an admitted old access token can finish during normal same-grant refresh.
#[derive(Clone)]
pub struct SourceUse(Lifetime);
impl SourceUse {
    pub fn live(&self) -> bool {
        self.0.live() && self.0.0.active.load(Ordering::Acquire)
    }
    pub fn same_scope(&self, other: &Self) -> bool {
        self.0.same(&other.0)
    }
}
impl std::fmt::Debug for SourceUse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeSourceUse(<private>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
enum NetworkIdentity {
    Managed(Scope, crate::managed_auth::Revision),
    Unrelated(NoNativeReader),
}
/// A captured credential source; no path/token is exposed through Debug or errors.
#[derive(Clone)]
pub struct NetworkScope {
    lifetime: Lifetime,
    identity: NetworkIdentity,
    domain: Arc<crate::assignment::Domain>,
}
impl std::fmt::Debug for NetworkScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeNetworkScope(<private>)")
    }
}
impl PartialEq for NetworkScope {
    fn eq(&self, other: &Self) -> bool {
        let same_scope = match (&self.identity, &other.identity) {
            (NetworkIdentity::Managed(left, _), NetworkIdentity::Managed(right, _)) => {
                left == right
            }
            (NetworkIdentity::Unrelated(left), NetworkIdentity::Unrelated(right)) => left == right,
            _ => false,
        };
        self.lifetime.same(&other.lifetime)
            && same_scope
            && Arc::ptr_eq(&self.domain, &other.domain)
    }
}
impl Eq for NetworkScope {}
impl NetworkScope {
    pub(crate) fn preserves_grant(&self) -> bool {
        matches!(&self.identity, NetworkIdentity::Managed(..))
    }
    /// Retained exact native-admitted constructor linkage, not metadata equality.
    pub(crate) fn derived_from_scope(&self, parent: &Self) -> bool {
        self.lifetime.0.allocation.derived_parent.as_ref() == Some(&parent.lifetime.incarnation())
            && matches!((&self.identity, &parent.identity),
                (NetworkIdentity::Managed(actual, _), NetworkIdentity::Managed(expected, _)) if actual == expected)
    }
    pub(crate) fn belongs_to(&self, domain: &Arc<crate::assignment::Domain>) -> bool {
        Arc::ptr_eq(&self.domain, domain)
    }
    pub(crate) fn member_is(&self, member: &Lifetime) -> bool {
        self.lifetime.same(member)
    }
    pub fn acquire(&self) -> io::Result<NetworkUse> {
        self.acquire_for(crate::assignment::current_activity())
    }
    pub(crate) fn acquire_for(
        &self,
        current: Option<crate::assignment::Activity>,
    ) -> io::Result<NetworkUse> {
        let resource = self
            .domain
            .admit_resource(current.as_ref())
            .or_else(|error| match current.as_ref() {
                Some(parent) => self
                    .domain
                    .admit_inherited_native_resource(parent, &self.lifetime),
                None => Err(error),
            })
            .map_err(|_| unavailable())?;
        let state = self.lifetime.0.state.lock().map_err(|_| unavailable())?;
        let matches = match (&self.identity, &state.kind) {
            (NetworkIdentity::Managed(scope, revision), Some(Kind::Managed(actual))) => {
                scope == actual
                    && state.preloaded.as_ref().is_some_and(|current| {
                        current.generation >= revision.generation
                            && (current.generation != revision.generation
                                || current.digest == revision.digest)
                            && (state.reader
                                || state
                                    .snapshot
                                    .as_ref()
                                    .is_some_and(|(ready, _)| ready == current))
                    })
            }
            (NetworkIdentity::Unrelated(kind), Some(Kind::Unrelated(actual))) => kind == actual,
            _ => false,
        };
        if !matches || !self.lifetime.ready() {
            return Err(unavailable());
        }
        drop(state);
        let parent_use = match current.as_ref() {
            Some(parent) => parent.native_use_for(&self.lifetime)?,
            None => resource.native_use_for(&self.lifetime)?,
        };
        if let Some(lease) = &parent_use {
            lease.cover_domain(&self.domain)?;
            resource.retain_native_use(&self.lifetime, lease)?;
        }
        Ok(NetworkUse {
            source: SourceUse(self.lifetime.clone()),
            _resource: resource,
            parent_use,
        })
    }
}
/// Actual request/body/WebSocket use, including the existing assignment resource lease.
#[derive(Clone)]
pub struct NetworkUse {
    source: SourceUse,
    _resource: crate::assignment::Activity,
    parent_use: Option<crate::native_source_use::ParentUseLease>,
}
impl std::fmt::Debug for NetworkUse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeNetworkUse(<private>)")
    }
}
impl NetworkUse {
    pub async fn prepare(&self) -> io::Result<()> {
        if let Some(lease) = &self.parent_use {
            lease.ready().await?;
        }
        if !self.live() {
            return Err(unavailable());
        }
        Ok(())
    }
    pub fn live(&self) -> bool {
        self.source.live()
    }
    pub async fn revoked(&self) {
        loop {
            let changed = self.source.0.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self.live() {
                return;
            }
            tokio::select! {
                _ = &mut changed => {},
                _ = self.source.0.0.allocation.member.closed() => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_auth::Revision;
    use crate::native_source::Member;
    use crate::native_source::PreloadFuture;
    use crate::native_source::ReadyFuture;
    use crate::native_source::ReadyReceipt;
    use crate::native_source::UnitFuture;
    use std::sync::atomic::AtomicUsize;

    struct Native {
        live: AtomicBool,
        work: std::sync::atomic::AtomicU64,
        ready: Mutex<Vec<(String, Option<Generations>)>>,
        retired: AtomicUsize,
        retirements: Mutex<Vec<crate::native_source::Retirement>>,
        refreshes: Mutex<Vec<crate::managed_auth::Request>>,
        preload_lost: tokio::sync::Notify,
        changed: tokio::sync::Notify,
        use_serial: Arc<tokio::sync::Semaphore>,
        use_next: std::sync::atomic::AtomicU64,
        use_begins: AtomicUsize,
        close_begin_once: AtomicBool,
        use_ends: AtomicUsize,
        end_blocked: AtomicBool,
        end_started: tokio::sync::Notify,
        end_release: tokio::sync::Notify,
        retire_blocked: AtomicBool,
        retire_started: tokio::sync::Notify,
        retire_release: tokio::sync::Notify,
    }
    fn source() -> Source {
        Source {
            operation: "b".repeat(32),
            slot: "6d656d626572".into(),
            path: "/private/native-source-fixture".into(),
            account: "c".repeat(64),
            user: "d".repeat(64),
            revision: Revision {
                generation: 9,
                digest: "e".repeat(64),
            },
            assignment_generation: 3,
            native_intent: Some("f".repeat(32)),
        }
    }
    impl Member for Native {
        fn rebound_ready_owned<'a>(
            &'a self,
            attempt: &'a crate::native_preload::Body,
            registration: &'a str,
            loaded: &'a Loaded,
        ) -> crate::native_source::ReboundReadyFuture<'a> {
            Box::pin(async move {
                self.ready_owned(attempt, registration, loaded)
                    .await
                    .map(crate::native_source::ReboundReadyOutcome::Ready)
            })
        }
        fn rebound_preload_owned<'a>(
            &'a self,
            attempt: &'a crate::native_preload::Body,
        ) -> PreloadFuture<'a> {
            self.preload_owned(attempt, None)
        }
        fn work_incarnation(&self) -> u64 {
            self.work.load(Ordering::Acquire)
        }
        fn id(&self) -> &str {
            "11111111111111111111111111111111"
        }
        fn live(&self) -> bool {
            self.live.load(Ordering::Acquire)
        }
        fn preload_owned<'a>(
            &'a self,
            attempt: &'a crate::native_preload::Body,
            _: Option<&'a Borrow>,
        ) -> PreloadFuture<'a> {
            Box::pin(async move {
                let operation = attempt.operation.as_str();
                if operation == "7".repeat(32) {
                    self.preload_lost.notify_one();
                    return Err(std::io::ErrorKind::ConnectionReset.into());
                }
                let mut source = source();
                source.operation = operation.into();
                if operation == "8".repeat(32) {
                    source.revision = Revision {
                        generation: 10,
                        digest: "9".repeat(64),
                    };
                }
                Ok(Preload {
                    registration: if operation == "8".repeat(32) {
                        "3".repeat(32)
                    } else {
                        "2".repeat(32)
                    },
                    source,
                    sequence: attempt.sequence.clone(),
                })
            })
        }
        fn preload_owned_use<'a>(
            &'a self,
            attempt: &'a crate::native_preload::Body,
            receipt: &'a crate::native_source_use::UseReceipt,
        ) -> PreloadFuture<'a> {
            assert_eq!(receipt.member(), self.id());
            self.preload_owned(attempt, None)
        }
        fn resample_owned_use<'a>(
            &'a self,
            attempt: &'a crate::native_preload::Body,
            previous: &'a crate::native_preload::Predecessor,
            receipt: &'a crate::native_source_use::UseReceipt,
        ) -> crate::native_source::ResampleFuture<'a> {
            Box::pin(async move {
                assert_eq!(receipt.member(), self.id());
                attempt.validate()?;
                assert_eq!(previous.registration, "2".repeat(32));
                assert_eq!(previous.revision, source().revision);
                let mut fresh = source();
                fresh.operation = attempt.operation.clone();
                fresh.revision = Revision {
                    generation: 10,
                    digest: "9".repeat(64),
                };
                Ok(crate::native_preload::Resample::Preload(Preload {
                    registration: "3".repeat(32),
                    sequence: attempt.sequence.clone(),
                    source: fresh,
                }))
            })
        }
        fn ready_owned<'a>(
            &'a self,
            attempt: &'a crate::native_preload::Body,
            registration: &'a str,
            loaded: &'a Loaded,
        ) -> crate::native_source::OwnedReadyFuture<'a> {
            Box::pin(async move {
                let receipt = self
                    .ready(&attempt.operation, Some(registration), loaded)
                    .await?;
                crate::native_preload::Ready::from_native(attempt.clone(), receipt.registration)
            })
        }
        fn ready_owned_use<'a>(
            &'a self,
            attempt: &'a crate::native_preload::Body,
            registration: &'a str,
            loaded: &'a Loaded,
            receipt: &'a crate::native_source_use::UseReceipt,
        ) -> crate::native_source::OwnedReadyFuture<'a> {
            assert_eq!(receipt.member(), self.id());
            self.ready_owned(attempt, registration, loaded)
        }
        fn ready<'a>(
            &'a self,
            operation: &'a str,
            registration: Option<&'a str>,
            loaded: &'a Loaded,
        ) -> ReadyFuture<'a> {
            Box::pin(async move {
                let generations = match loaded {
                    Loaded::Source { generations, .. } => Some(generations.clone()),
                    _ => None,
                };
                self.ready
                    .lock()
                    .unwrap()
                    .push((operation.into(), generations));
                Ok(ReadyReceipt {
                    registration: registration
                        .map(str::to_owned)
                        .unwrap_or_else(|| "2".repeat(32)),
                })
            })
        }
        fn begin_use<'a>(
            &'a self,
            attempt: &'a crate::native_source_use::UseAttempt,
            registration: &'a str,
        ) -> crate::native_source_use::UseFuture<'a> {
            Box::pin(async move {
                use crate::native_source_use::UseBody;
                use crate::native_source_use::UseOutcome;
                use crate::native_source_use::UseReceipt;
                if !attempt.hold_admission(self.use_serial.clone()).await? {
                    return Ok(UseOutcome::Unsent);
                }
                let body = match attempt.body()? {
                    Some(body) => body,
                    None => {
                        let counter = self.use_next.fetch_add(1, Ordering::Relaxed) + 1;
                        self.use_begins.fetch_add(1, Ordering::Relaxed);
                        attempt.assign(UseBody {
                            member: self.id().into(),
                            operation: format!("0000000000000001{counter:016x}"),
                            registration: registration.into(),
                        })?
                    }
                };
                if self.close_begin_once.swap(false, Ordering::AcqRel) {
                    return Ok(UseOutcome::Closed);
                }
                Ok(UseOutcome::Active(UseReceipt::from_native(
                    body.member,
                    body.operation.clone(),
                    body.operation.repeat(2),
                )?))
            })
        }
        fn close_use<'a>(
            &'a self,
            _: &'a crate::native_source_use::UseBody,
        ) -> crate::native_source_use::UseFuture<'a> {
            Box::pin(async { Ok(crate::native_source_use::UseOutcome::Closed) })
        }
        fn end_use<'a>(&'a self, _: &'a crate::native_source_use::UseReceipt) -> UnitFuture<'a> {
            Box::pin(async move {
                self.use_ends.fetch_add(1, Ordering::Relaxed);
                self.end_started.notify_one();
                if self.end_blocked.load(Ordering::Acquire) {
                    self.end_release.notified().await;
                }
                Ok(())
            })
        }
        fn refresh_use<'a>(
            &'a self,
            request: &'a crate::managed_auth::Request,
            receipt: &'a crate::native_source_use::UseReceipt,
        ) -> crate::managed_auth::RefreshFuture<'a> {
            Box::pin(async move {
                assert_eq!(receipt.member(), self.id());
                self.refreshes.lock().unwrap().push(request.clone());
                Ok(Revision {
                    generation: 10,
                    digest: "9".repeat(64),
                })
            })
        }
        fn retire_captured<'a>(
            &'a self,
            request: &'a crate::native_source::Retirement,
        ) -> UnitFuture<'a> {
            Box::pin(async move {
                request.validate()?;
                self.retirements.lock().unwrap().push(request.clone());
                self.retire(request.registration.as_deref()).await
            })
        }
        fn retire<'a>(&'a self, _: Option<&'a str>) -> UnitFuture<'a> {
            Box::pin(async move {
                self.retire_started.notify_one();
                if self.retire_blocked.load(Ordering::Acquire) {
                    self.retire_release.notified().await;
                }
                self.retired.fetch_add(1, Ordering::Relaxed);
                self.changed.notify_one();
                Ok(())
            })
        }
    }
    fn lifetime() -> (Lifetime, Arc<Native>) {
        lifetime_with(None)
    }
    fn lifetime_with(
        parent_use: Option<crate::native_source_use::ParentUseRef>,
    ) -> (Lifetime, Arc<Native>) {
        let native = Arc::new(Native {
            work: std::sync::atomic::AtomicU64::new(0),
            live: AtomicBool::new(true),
            ready: Mutex::default(),
            retired: AtomicUsize::new(0),
            retirements: Mutex::default(),
            refreshes: Mutex::default(),
            preload_lost: Default::default(),
            changed: Default::default(),
            use_serial: Arc::new(tokio::sync::Semaphore::new(1)),
            use_next: std::sync::atomic::AtomicU64::new(0),
            use_begins: AtomicUsize::new(0),
            close_begin_once: AtomicBool::new(false),
            use_ends: AtomicUsize::new(0),
            end_blocked: AtomicBool::new(false),
            end_started: tokio::sync::Notify::new(),
            end_release: tokio::sync::Notify::new(),
            retire_blocked: AtomicBool::new(false),
            retire_started: tokio::sync::Notify::new(),
            retire_release: tokio::sync::Notify::new(),
        });
        let lifetime = Lifetime::new(Allocation {
            target: None,
            derived_parent: None,
            parent_use,
            operation: "a".repeat(32),
            member: native.clone(),
        })
        .unwrap();
        (lifetime, native)
    }
    // An already loaded member needs real new-work admission before another
    // PRELOAD. Its captured completion remains owned after this activity ends.
    async fn admitted_preload(lifetime: &Lifetime, operation: &str) -> Preload {
        let domain = match lifetime.domain() {
            Some(domain) => domain,
            None => {
                let domain = Arc::new(crate::assignment::Domain::default());
                lifetime.attach_domain(domain.clone()).unwrap();
                domain
            }
        };
        domain
            .admit_work(None)
            .unwrap()
            .scope(lifetime.preload(operation, None))
            .await
            .unwrap()
    }
    #[tokio::test]
    async fn same_scope_refresh_updates_actual_readiness_without_replacing_its_lifetime() {
        let (lifetime, native) = lifetime();
        let preload = lifetime.preload(&"b".repeat(32), None).await.unwrap();
        assert!(!lifetime.ready());
        lifetime.permits_storage(&preload.source).unwrap();
        let generations = Generations {
            auth: 13,
            owner: 2,
            source: 4,
            login: 7,
        };
        lifetime
            .source_ready(&preload.source, generations.clone())
            .await
            .unwrap();
        let old_use = lifetime.source_use(&preload.source, &generations).unwrap();
        let mut refreshed = preload.source;
        refreshed.operation = "8".repeat(32);
        refreshed.revision = Revision {
            generation: 10,
            digest: "9".repeat(64),
        };
        assert!(
            lifetime
                .source_use(
                    &refreshed,
                    &Generations {
                        auth: 14,
                        ..generations.clone()
                    }
                )
                .is_err()
        );
        assert!(old_use.live());
        assert!(
            lifetime
                .source_ready(&refreshed, generations.clone())
                .await
                .is_err()
        );
        assert!(
            lifetime.preload(&refreshed.operation, None).await.is_err(),
            "post-Ready reads require actual admitted work"
        );
        let renewed = admitted_preload(&lifetime, &refreshed.operation).await;
        assert_eq!(renewed.source.revision, refreshed.revision);
        lifetime
            .source_ready(
                &refreshed,
                Generations {
                    auth: 14,
                    ..generations.clone()
                },
            )
            .await
            .unwrap();
        assert!(lifetime.ready());
        let refreshed_use = lifetime
            .source_use(
                &refreshed,
                &Generations {
                    auth: 14,
                    ..generations.clone()
                },
            )
            .unwrap();
        assert!(old_use.same_scope(&refreshed_use));
        assert_eq!(
            native
                .ready
                .lock()
                .unwrap()
                .iter()
                .map(|(operation, _)| operation.clone())
                .collect::<Vec<_>>(),
            vec!["b".repeat(32), "8".repeat(32)]
        );
        assert_eq!(native.ready.lock().unwrap()[0].1, Some(generations));
        refreshed.assignment_generation += 1;
        assert!(lifetime.permits_storage(&refreshed).is_err());
        assert!(
            lifetime
                .source_ready(
                    &refreshed,
                    Generations {
                        auth: 15,
                        owner: 2,
                        source: 4,
                        login: 7
                    }
                )
                .await
                .is_err()
        );
        assert_eq!(native.ready.lock().unwrap().len(), 2);
    }
    #[tokio::test]
    async fn reader_cannot_downgrade_its_held_source_lease() {
        let (lifetime, native) = lifetime();
        let preload = lifetime.preload(&"b".repeat(32), None).await.unwrap();
        lifetime.reader_ready(&preload.source).await.unwrap();
        assert!(lifetime.ready());
        assert!(
            lifetime
                .unavailable(&preload.source.operation)
                .await
                .is_err()
        );
        assert!(
            lifetime
                .source_ready(
                    &preload.source,
                    Generations {
                        auth: 1,
                        owner: 1,
                        source: 1,
                        login: 1
                    }
                )
                .await
                .is_err()
        );
        assert_eq!(native.ready.lock().unwrap().len(), 1);
        assert!(lifetime.ready());
    }

    #[tokio::test]
    async fn last_storage_or_derived_use_retires_the_member_and_eof_fences_every_clone() {
        let (lifetime, native) = lifetime();
        let preload = lifetime.preload(&"b".repeat(32), None).await.unwrap();
        let storage = lifetime.clone();
        let derived = lifetime.clone();
        drop(lifetime);
        drop(storage);
        tokio::task::yield_now().await;
        assert_eq!(native.retired.load(Ordering::Relaxed), 0);
        native.live.store(false, Ordering::Release);
        assert!(!derived.ready());
        assert!(derived.permits_storage(&preload.source).is_err());
        drop(derived);
        native.changed.notified().await;
        assert_eq!(native.retired.load(Ordering::Relaxed), 1);
    }
    #[tokio::test]
    async fn api_scope_has_native_completion_without_a_source_or_fake_policy_refusal() {
        let (lifetime, native) = lifetime();
        lifetime
            .unrelated_ready(NoNativeReader::ApiKey)
            .await
            .unwrap();
        assert!(lifetime.ready());
        assert!(lifetime.permits_storage(&source()).is_err());
        assert_eq!(
            native.ready.lock().unwrap().as_slice(),
            &[("a".repeat(32), None)]
        );
    }
    #[tokio::test]
    async fn native_network_request_keeps_resource_until_last_body_use_not_cached_factory() {
        let (lifetime, _) = lifetime();
        let domain = Arc::new(crate::assignment::Domain::default());
        lifetime.attach_domain(domain.clone()).unwrap();
        let preload = lifetime.preload(&"b".repeat(32), None).await.unwrap();
        lifetime.reader_ready(&preload.source).await.unwrap();
        let factory = lifetime.network_scope().unwrap();
        let request = factory.acquire().unwrap();
        let body = request.clone();
        let operation = "a".repeat(32);
        domain
            .request(crate::assignment::Intent {
                operation: operation.clone(),
                thread: "00000000-0000-0000-0000-000000000001".into(),
                selection: crate::assignment::Selection::Auto {
                    revision: "a".repeat(64),
                },
            })
            .unwrap();
        let retiring = domain.try_retire(&operation).unwrap();
        assert!(factory.acquire().is_err());
        drop(request);
        let retiring = match retiring.freeze() {
            Err(crate::assignment::FreezeError::Pending(retiring)) => retiring,
            _ => panic!("body still owns the source resource"),
        };
        drop(body);
        wait_settled(&domain).await;
        assert!(
            retiring.freeze().is_ok(),
            "cached factory does not count as active request"
        );
        assert!(lifetime.ready());
    }
    #[tokio::test]
    async fn native_network_new_requests_need_fresh_ready_but_old_response_survives_refresh() {
        let (lifetime, native) = lifetime();
        let preload = lifetime.preload(&"b".repeat(32), None).await.unwrap();
        lifetime.reader_ready(&preload.source).await.unwrap();
        let old_factory = lifetime.network_scope().unwrap();
        let old_response = old_factory.acquire().unwrap();
        let fresh = admitted_preload(&lifetime, &"8".repeat(32)).await;
        assert!(old_response.live());
        assert!(old_factory.acquire().is_err());
        let fresh_factory = lifetime.network_scope().unwrap();
        assert!(fresh_factory.acquire().is_err());
        lifetime.reader_ready(&fresh.source).await.unwrap();
        assert!(fresh_factory.acquire().is_ok());
        assert!(
            old_factory == fresh_factory,
            "normal refresh preserves immutable account scope"
        );
        assert!(
            old_factory.identity != fresh_factory.identity,
            "read observations still distinguish C1/C2"
        );
        assert!(
            old_factory.acquire().is_ok(),
            "same-grant cached client remains usable under actual current Ready"
        );
        assert!(old_response.live());
        native.live.store(false, Ordering::Release);
        tokio::time::timeout(std::time::Duration::from_secs(1), old_response.revoked())
            .await
            .unwrap();
        assert!(!old_response.live());
    }
    async fn wait_settled(domain: &Arc<crate::assignment::Domain>) {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !crate::native_source_use::settled_for(domain) {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn native_parent_use_survives_http_and_root_guard_until_tool_approval_compaction_and_end_ack()
     {
        let (lifetime, native) = lifetime();
        let domain = Arc::new(crate::assignment::Domain::default());
        lifetime.attach_domain(domain.clone()).unwrap();
        let preload = lifetime.preload(&"b".repeat(32), None).await.unwrap();
        lifetime.reader_ready(&preload.source).await.unwrap();
        let factory = lifetime.network_scope().unwrap();
        let root = domain.admit_work(None).unwrap();
        // Children created before the first auth action share the root's lazy map.
        let tool = domain.admit_work(Some(&root)).unwrap();
        let approval = domain.admit_work(Some(&root)).unwrap();
        let compaction = domain.admit_work(Some(&root)).unwrap();
        root.clone()
            .scope(async {
                let body = factory.acquire().unwrap();
                body.prepare().await.unwrap();
                drop(body);
            })
            .await;
        assert_eq!(native.use_begins.load(Ordering::Relaxed), 1);
        drop(root);
        drop(tool);
        drop(approval);
        assert_eq!(native.use_ends.load(Ordering::Relaxed), 0);
        compaction
            .clone()
            .scope(async {
                let later = factory.acquire().unwrap();
                later.prepare().await.unwrap();
            })
            .await;
        compaction
            .clone()
            .scope(async {
                let fresh = lifetime.preload(&"8".repeat(32), None).await.unwrap();
                assert_eq!(fresh.source.revision.generation, 10);
                assert!(!lifetime.ready(), "preload resets descriptive readiness");
                lifetime.reader_ready(&fresh.source).await.unwrap();
                factory.acquire().unwrap().prepare().await.unwrap();
            })
            .await;
        assert_eq!(native.use_begins.load(Ordering::Relaxed), 1);
        native.end_blocked.store(true, Ordering::Release);
        domain
            .request(crate::assignment::Intent {
                operation: "a".repeat(32),
                thread: "00000000-0000-0000-0000-000000000001".into(),
                selection: crate::assignment::Selection::Auto {
                    revision: "a".repeat(64),
                },
            })
            .unwrap();
        drop(compaction);
        native.end_started.notified().await;
        let retiring = domain.try_retire(&"a".repeat(32)).unwrap();
        let retiring = match retiring.freeze() {
            Err(crate::assignment::FreezeError::Pending(retiring)) => retiring,
            _ => panic!("local zero-use must wait actual native End ACK"),
        };
        assert!(
            factory.acquire().is_err(),
            "cached factory cannot resurrect the old parent cap"
        );
        assert_eq!(native.use_begins.load(Ordering::Relaxed), 1);
        let mut drained = tokio::spawn(retiring.wait_drained());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), &mut drained)
                .await
                .is_err(),
            "retained reconciliation actor also waits the actual End ACK"
        );
        native.end_release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(2), drained)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(native.use_ends.load(Ordering::Relaxed), 1);
    }
    #[tokio::test]
    async fn derived_first_request_inherits_only_actual_parent_and_its_end_ack() {
        let (parent, _) = lifetime();
        let root_domain = Arc::new(crate::assignment::Domain::default());
        parent.attach_domain(root_domain.clone()).unwrap();
        let initial = parent.preload(&"b".repeat(32), None).await.unwrap();
        parent.reader_ready(&initial.source).await.unwrap();
        let root = root_domain.admit_work(None).unwrap();
        let parent_use = root.native_use_for(&parent).unwrap().unwrap();
        parent_use.ready().await.unwrap();
        let weak_parent = parent_use.downgrade();
        let (child, native) = lifetime_with(Some(weak_parent.clone()));
        let child_domain = Arc::new(crate::assignment::Domain::default());
        child.attach_domain(child_domain.clone()).unwrap();
        let loaded = child.preload(&"c".repeat(32), None).await.unwrap();
        child.reader_ready(&loaded.source).await.unwrap();
        let operation = "a".repeat(32);
        child_domain
            .request(crate::assignment::Intent {
                operation: operation.clone(),
                thread: "00000000-0000-0000-0000-000000000001".into(),
                selection: crate::assignment::Selection::Auto {
                    revision: "a".repeat(64),
                },
            })
            .unwrap();
        let factory = child.network_scope().unwrap();
        assert!(
            factory.acquire().is_err(),
            "cached factory alone cannot bypass pending admission"
        );
        let body = root
            .clone()
            .scope(async {
                let body = factory.acquire().unwrap();
                body.prepare().await.unwrap();
                body
            })
            .await;
        native.end_blocked.store(true, Ordering::Release);
        drop(root);
        drop(parent_use);
        drop(body);
        native.end_started.notified().await;
        assert!(
            !crate::native_source_use::settled_for(&root_domain),
            "derived End ACK is part of original root's drain"
        );
        native.end_release.notify_one();
        wait_settled(&root_domain).await;
        assert!(weak_parent.upgrade().is_none());
        let (orphan, orphan_native) = lifetime_with(Some(weak_parent));
        assert!(
            orphan.preload(&"d".repeat(32), None).await.is_err(),
            "expired derived context cannot fall back to ordinary preload"
        );
        assert!(orphan_native.ready.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn settled_use_replacement_requires_fresh_real_ready_on_the_same_activity() {
        let (lifetime, native) = lifetime();
        let domain = Arc::new(crate::assignment::Domain::default());
        lifetime.attach_domain(domain.clone()).unwrap();
        let initial = lifetime.preload(&"b".repeat(32), None).await.unwrap();
        lifetime.reader_ready(&initial.source).await.unwrap();
        let root = domain.admit_work(None).unwrap();
        native.close_begin_once.store(true, Ordering::Release);
        let old = root.native_use_for(&lifetime).unwrap().unwrap();
        assert!(old.ready().await.is_err());
        let same = root.native_use_for(&lifetime).unwrap().unwrap();
        assert!(
            same.same(&old),
            "no new nonce before an actual Ready repair"
        );
        assert_eq!(native.use_begins.load(Ordering::Relaxed), 1);
        // Separate, actually admitted same-source work supplies a fresh
        // PRELOAD/Ready. The settled original activity cannot mint a repair cap
        // or claim the separate cap-free member-repair frontdoor exists.
        let repaired = admitted_preload(&lifetime, &"8".repeat(32)).await;
        lifetime.reader_ready(&repaired.source).await.unwrap();
        let next = root.native_use_for(&lifetime).unwrap().unwrap();
        assert!(!next.same(&old));
        next.ready().await.unwrap();
        assert_eq!(
            native.use_begins.load(Ordering::Relaxed),
            3,
            "closed original, admitted repair, then repaired original activity"
        );
        drop(old);
        drop(same);
        drop(next);
        drop(root);
        wait_settled(&domain).await;
    }
    #[tokio::test]
    async fn latest_attempt_before_send_retires_lost_ack_without_reusing_old_registration() {
        let (lifetime, native) = lifetime();
        let first = lifetime.preload(&"b".repeat(32), None).await.unwrap();
        lifetime.reader_ready(&first.source).await.unwrap();
        let domain = Arc::new(crate::assignment::Domain::default());
        lifetime.attach_domain(domain.clone()).unwrap();
        let root = domain.admit_work(None).unwrap();
        {
            let operation = "7".repeat(32);
            let pending = root.clone().scope(lifetime.preload(&operation, None));
            tokio::pin!(pending);
            tokio::select! {
                _ = native.preload_lost.notified() => {},
                _ = &mut pending => panic!("lost ACK keeps its owned admission"),
            }
            // Explicit retirement cancels further read admission; it does not infer
            // a registration from the lost reply or erase the accepted attempt.
            native.retire_blocked.store(true, Ordering::Release);
            lifetime.retire_reader().unwrap();
            assert!(pending.await.is_err());
        }
        drop(root);
        wait_settled(&domain).await;
        assert_eq!(
            native.retired.load(Ordering::Acquire),
            0,
            "local retirement is not native Closed"
        );
        native.retire_release.notify_one();
        drop(lifetime);
        native.changed.notified().await;
        let requests = native.retirements.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].preload_operation, Some("7".repeat(32)));
        assert_eq!(
            requests[0].registration, None,
            "the previous acknowledged registration is not a wildcard"
        );
        requests[0].validate().unwrap();
    }
    #[tokio::test]
    async fn rebound_ack_ticket_cannot_publish_after_seal_or_retirement() {
        for retiring in [false, true] {
            let (lifetime, native) = lifetime();
            let initial = lifetime.preload(&"b".repeat(32), None).await.unwrap();
            let generations = Generations {
                auth: 1,
                owner: 2,
                source: 3,
                login: 4,
            };
            lifetime
                .source_ready(&initial.source, generations.clone())
                .await
                .unwrap();
            native.work.fetch_add(1, Ordering::AcqRel);
            assert!(!lifetime.ready());
            let acknowledged = lifetime
                .prepare_rebound_ready(&initial.source, &generations)
                .await
                .unwrap();
            let ReboundPreparation::Ready(acknowledged) = acknowledged else {
                panic!("same source ACK")
            };
            assert!(
                !lifetime.ready(),
                "native ACK alone cannot publish credential readiness"
            );
            if retiring {
                lifetime.retire_reader().unwrap();
            } else {
                lifetime.seal();
            }
            assert!(acknowledged.commit().is_err());
            assert!(!lifetime.ready());
        }
    }
    #[tokio::test]
    async fn actual_loaded_ack_is_read_history_until_guarded_publication_and_cannot_cross_seal() {
        let (lifetime, native) = lifetime();
        let first = lifetime.preload(&"b".repeat(32), None).await.unwrap();
        let old = Generations {
            auth: 1,
            owner: 2,
            source: 3,
            login: 4,
        };
        lifetime
            .source_ready(&first.source, old.clone())
            .await
            .unwrap();
        native.work.fetch_add(1, Ordering::AcqRel);
        let current = lifetime.rebound_preload(&"8".repeat(32)).await.unwrap();
        let loaded = Generations { auth: 2, ..old };
        let ReboundPreparation::Ready(ack) = lifetime
            .prepare_loaded_ready(&current.source, &loaded)
            .await
            .unwrap()
        else {
            panic!("same physical source")
        };
        assert!(!lifetime.ready());
        assert!(
            lifetime.native_use_seed().is_err(),
            "actual ACK history cannot admit HTTP use"
        );
        let ReboundPreparation::Ready(rechecked) = lifetime
            .prepare_rebound_ready(&current.source, &loaded)
            .await
            .unwrap()
        else {
            panic!("same physical source")
        };
        assert!(!lifetime.ready());
        lifetime.seal();
        assert!(ack.commit().is_err());
        assert!(rechecked.commit().is_err());
        assert!(
            lifetime.rebound_preload(&"c".repeat(32)).await.is_err(),
            "a sealed factory cannot turn ACK history into read admission"
        );
    }
    #[tokio::test]
    async fn registered_refresh_requires_actual_activity_and_reloads_new_ready_in_same_use() {
        let (lifetime, native) = lifetime();
        let domain = Arc::new(crate::assignment::Domain::default());
        lifetime.attach_domain(domain.clone()).unwrap();
        let initial = lifetime.preload(&"b".repeat(32), None).await.unwrap();
        let generations = Generations {
            auth: 1,
            owner: 2,
            source: 3,
            login: 4,
        };
        lifetime
            .source_ready(&initial.source, generations.clone())
            .await
            .unwrap();
        assert!(
            lifetime
                .refresh(&initial.source.slot, &initial.source.revision.digest)
                .await
                .is_err()
        );
        assert!(native.refreshes.lock().unwrap().is_empty());
        let root = domain.admit_work(None).unwrap();
        root.clone()
            .scope(async {
                assert!(
                    lifetime
                        .refresh("6f74686572", &initial.source.revision.digest)
                        .await
                        .is_err()
                );
                let registered = lifetime.preload(&"c".repeat(32), None).await.unwrap();
                assert!(!lifetime.ready());
                let revision = lifetime
                    .refresh(&registered.source.slot, &registered.source.revision.digest)
                    .await
                    .unwrap();
                assert_eq!(revision.generation, 10);
                // Neither a foreign registration nor a changed old revision can
                // convert owner publication into fresh credential read authority.
                let predecessor = crate::native_preload::Predecessor {
                    registration: registered.registration.clone(),
                    revision: registered.source.revision.clone(),
                };
                let mut foreign = predecessor.clone();
                foreign.registration = "f".repeat(32);
                assert!(lifetime.resample_preload(foreign).await.is_err());
                let mut changed = predecessor;
                changed.revision.generation += 1;
                assert!(lifetime.resample_preload(changed).await.is_err());
                assert!(!lifetime.ready());
                let fresh = match lifetime
                    .resample_preload(crate::native_preload::Predecessor {
                        registration: registered.registration.clone(),
                        revision: registered.source.revision.clone(),
                    })
                    .await
                    .unwrap()
                {
                    crate::native_preload::Resample::Preload(fresh) => fresh,
                    _ => panic!("only a native resample grants a new read"),
                };
                assert_eq!(fresh.source.operation, registered.source.operation);
                assert_eq!(fresh.sequence, registered.sequence);
                assert_ne!(fresh.registration, registered.registration);
                assert_eq!(fresh.source.revision, revision);
                lifetime
                    .source_ready(
                        &fresh.source,
                        Generations {
                            auth: 2,
                            ..generations.clone()
                        },
                    )
                    .await
                    .unwrap();
                assert!(lifetime.ready());
            })
            .await;
        assert_eq!(
            native.use_begins.load(Ordering::Relaxed),
            1,
            "preload/refresh/Ready preserve the real admitted use"
        );
        let requests = native.refreshes.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].slot, initial.source.slot);
        assert_eq!(requests[0].expected_digest, initial.source.revision.digest);
        drop(root);
        wait_settled(&domain).await;
    }
    #[tokio::test]
    async fn reader_loaded_observation_is_exact_to_preload_and_member_incarnation() {
        let (reader, _) = lifetime();
        let first = reader.preload(&"b".repeat(32), None).await.unwrap();
        assert!(reader.reader_loaded().is_err());
        reader.reader_ready(&first.source).await.unwrap();
        let loaded = reader.reader_loaded().unwrap();
        assert!(reader.reader_still_loaded(&loaded));
        let (other, _) = lifetime();
        let same = other.preload(&"b".repeat(32), None).await.unwrap();
        other.reader_ready(&same.source).await.unwrap();
        assert!(!other.reader_still_loaded(&loaded));
        let fresh = admitted_preload(&reader, &"8".repeat(32)).await;
        assert!(!reader.reader_still_loaded(&loaded));
        reader.reader_ready(&fresh.source).await.unwrap();
        assert!(reader.ready());
        assert!(
            !reader.reader_still_loaded(&loaded),
            "same-grant Ready cannot revive an older direct snapshot"
        );
        assert!(reader.reader_still_loaded(&reader.reader_loaded().unwrap()));
    }
    struct CatalogReader {
        identity: MemberIncarnation,
        loaded: ReaderLoaded,
        observer: crate::native_reader_retirement::Observer,
    }
    impl crate::manager_catalog::ReaderBackend for CatalogReader {
        fn identity(&self) -> &MemberIncarnation {
            &self.identity
        }
        fn loaded(&self) -> Result<ReaderLoaded, crate::assignment::Refused> {
            if !self.identity.reader_still_loaded(&self.loaded) {
                return Err(crate::assignment::Refused::Unavailable);
            }
            Ok(self.loaded.clone())
        }
        fn retirement(&self) -> crate::native_reader_retirement::Observer {
            self.observer.clone()
        }
        fn check_target(
            &self,
            _: &crate::assignment_service::Target,
        ) -> Result<(), crate::assignment::Refused> {
            Err(crate::assignment::Refused::Unavailable)
        }
    }
    async fn catalog_reader() -> (
        Arc<crate::manager_catalog::Registry>,
        Lifetime,
        Arc<Native>,
        Arc<crate::manager_catalog::Record>,
    ) {
        let registry = Arc::new(crate::manager_catalog::Registry::default());
        let factory = registry.factory(None).unwrap();
        let (reader, native) = lifetime();
        reader.attach_domain(factory.domain()).unwrap();
        let preload = reader.preload(&"b".repeat(32), None).await.unwrap();
        reader.reader_ready(&preload.source).await.unwrap();
        let record = factory.record().clone();
        record
            .attach_reader(Arc::new(CatalogReader {
                identity: reader.incarnation(),
                loaded: reader.reader_loaded().unwrap(),
                observer: reader.retirement_observer(),
            }))
            .unwrap();
        factory.complete().unwrap();
        assert!(
            record.backend().unwrap().is_none(),
            "reader has no manager backend/counters"
        );
        assert!(native.ready.lock().unwrap()[0].1.is_none());
        (registry, reader, native, record)
    }
    #[tokio::test]
    async fn reader_catalog_drains_body_and_end_ack_then_revokes_cache_and_waits_native_closed() {
        let (registry, reader, native, record) = catalog_reader().await;
        native.end_blocked.store(true, Ordering::Release);
        native.retire_blocked.store(true, Ordering::Release);
        let cached_factory = reader.network_scope().unwrap();
        let body = cached_factory.acquire().unwrap();
        body.prepare().await.unwrap();
        let clone = body.clone();
        let seal = registry.seal().unwrap();
        let draining = tokio::spawn(async move { seal.drain_resources().await });
        let mut draining = Box::pin(draining);
        drop(body);
        tokio::select! { _ = &mut draining => panic!("last body clone still owns reader activity"), _ = tokio::task::yield_now() => {} }
        drop(clone);
        native.end_started.notified().await;
        assert_eq!(native.retired.load(Ordering::Acquire), 0);
        tokio::select! { _ = &mut draining => panic!("native End is not acknowledged"), _ = tokio::task::yield_now() => {} }
        native.end_release.notify_one();
        native.retire_started.notified().await;
        assert!(!reader.ready());
        assert!(
            cached_factory.acquire().is_err(),
            "cached factory owns no continuing admission"
        );
        assert_eq!(record.role().unwrap(), crate::manager_catalog::Role::Reader);
        assert_eq!(registry.compact_retired_readers().unwrap(), 0);
        tokio::select! { _ = &mut draining => panic!("native retirement is not Closed"), _ = tokio::task::yield_now() => {} }
        native.retire_release.notify_one();
        draining.await.unwrap().unwrap();
        assert_eq!(
            record.phase().unwrap(),
            crate::manager_catalog::Phase::Retired
        );
        assert_eq!(registry.compact_retired_readers().unwrap(), 1);
        drop(cached_factory);
        drop(reader);
        tokio::task::yield_now().await;
        assert_eq!(
            native.retired.load(Ordering::Acquire),
            1,
            "Drop cannot issue another retirement"
        );
    }
    #[tokio::test]
    async fn lost_reader_domain_remains_unknown_until_its_exact_native_closed_proof() {
        let (registry, reader, native, record) = catalog_reader().await;
        native.retire_blocked.store(true, Ordering::Release);
        drop(reader);
        native.retire_started.notified().await;
        assert!(record.domain().is_none());
        let seal = registry.seal().unwrap();
        assert!(seal.preflight_census().is_err());
        assert_eq!(
            record.phase().unwrap(),
            crate::manager_catalog::Phase::Unknown
        );
        assert_eq!(registry.compact_retired_readers().unwrap(), 0);
        native.retire_release.notify_one();
        record.reader_retired().await.unwrap();
        assert_eq!(registry.compact_retired_readers().unwrap(), 1);
        seal.cancel_before_effects().unwrap();
        assert!(registry.factory(None).unwrap().admitted());
    }
    #[tokio::test]
    async fn reader_catalog_rejects_cleanup_observation_from_another_real_incarnation() {
        let registry = Arc::new(crate::manager_catalog::Registry::default());
        let factory = registry.factory(None).unwrap();
        let (other, _) = lifetime();
        let (reader, _) = lifetime();
        reader.attach_domain(factory.domain()).unwrap();
        let preload = reader.preload(&"b".repeat(32), None).await.unwrap();
        reader.reader_ready(&preload.source).await.unwrap();
        assert!(
            factory
                .record()
                .attach_reader(Arc::new(CatalogReader {
                    identity: reader.incarnation(),
                    loaded: reader.reader_loaded().unwrap(),
                    observer: other.retirement_observer(),
                }))
                .is_err()
        );
        assert_eq!(
            factory.record().role().unwrap(),
            crate::manager_catalog::Role::Unresolved
        );
        assert!(factory.record().backend().unwrap().is_none());
        assert!(factory.record().reader_snapshot().unwrap().is_none());
    }
    #[tokio::test]
    async fn replacing_work_channel_cannot_revive_previous_reader_ready() {
        let (member, native) = lifetime();
        let packet = member.preload(&"b".repeat(32), None).await.unwrap();
        member.reader_ready(&packet.source).await.unwrap();
        assert!(member.ready());
        assert!(member.reader_loaded().is_ok());
        native.work.fetch_add(1, Ordering::AcqRel);
        assert!(!member.ready());
        assert!(member.reader_loaded().is_err());
        // Only the exact owned native completion can qualify this channel.
        member.reader_ready(&packet.source).await.unwrap();
        assert!(member.ready());
        assert!(member.reader_loaded().is_ok());
    }
}
