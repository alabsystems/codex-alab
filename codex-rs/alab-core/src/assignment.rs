// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Coordination for one auth manager. Activity leases carry no account authority.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use tokio::sync::Notify;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Manual { account: String, revision: String },
    Auto { revision: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Intent {
    pub operation: String,
    pub thread: String,
    pub selection: Selection,
}

impl Intent {
    pub fn valid(&self) -> bool {
        let revision = match &self.selection {
            Selection::Manual { account, revision } => {
                if !crate::managed_auth::valid_hex(account, 64) {
                    return false;
                }
                revision
            }
            Selection::Auto { revision } => revision,
        };
        crate::managed_auth::valid_hex(&self.operation, 32)
            && crate::managed_auth::valid_hex(revision, 64)
            && self.thread.len() == 36
            && self.thread.bytes().enumerate().all(|(index, byte)| {
                if [8, 13, 18, 23].contains(&index) {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                }
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Open,
    Pending,
    /// Serialized recovery intent without live native succession authority.
    Uncertain,
    Retiring,
    Frozen,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    Invalid,
    Pending,
    Busy,
    WrongDomain,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generations {
    pub auth: u64,
    pub owner: u64,
    pub source: u64,
    pub login: u64,
}
#[derive(Clone, PartialEq, Eq)]
pub struct Binding {
    pub account: String,
    pub user: String,
    pub digest: String,
    pub credential_generation: u64,
    pub assignment_generation: u64,
    pub generations: Generations,
}
impl std::fmt::Debug for Binding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AssignmentBinding(<private>)")
    }
}
pub struct AbortCompletion {
    pub operation: String,
    pub boundary: String,
    pub binding: Binding,
}

pub struct Completion {
    pub operation: String,
    pub native_intent: String,
    pub boundary: String,
    pub binding: Binding,
}

struct State {
    phase: Phase,
    intent: Option<Intent>,
    starts: usize,
    work: usize,
    resources: usize,
    permit: Weak<PermitIdentity>,
    boundary: Option<String>,
    native_intent: Option<String>,
    bound: Option<Binding>,
    resources_ready: bool,
    restored: bool,
}
pub struct Domain {
    state: Mutex<State>,
    changed: Notify,
    native_member: Mutex<Option<crate::native_source_lifetime::MemberIncarnation>>,
}
impl Default for Domain {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                phase: Phase::Open,
                intent: None,
                starts: 0,
                work: 0,
                resources: 0,
                permit: Weak::new(),
                boundary: None,
                native_intent: None,
                bound: None,
                resources_ready: false,
                restored: false,
            }),
            changed: Notify::new(),
            native_member: Mutex::new(None),
        }
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Start,
    Work,
    Resource,
}
type NativeUses = Arc<
    Mutex<
        Vec<(
            crate::native_source_lifetime::MemberIncarnation,
            crate::native_source_use::ParentUseLease,
        )>,
    >,
>;
struct ActivityLease {
    domain: Arc<Domain>,
    kind: Mutex<Kind>,
    native_uses: NativeUses,
}
#[derive(Clone)]
pub struct Activity(Arc<ActivityLease>);
#[derive(Clone)]
pub struct ActivityRef(Weak<ActivityLease>);
tokio::task_local! { static CURRENT: Activity; }
struct Destination {
    domain: Arc<Domain>,
    permit: Weak<PermitIdentity>,
    binding: Binding,
}
tokio::task_local! { static DESTINATION: Destination; }
impl ActivityRef {
    pub fn upgrade(&self) -> Option<Activity> {
        self.0.upgrade().map(Activity)
    }
}
impl Activity {
    pub(crate) fn owns_native_use(
        &self,
        lease: &crate::native_source_use::ParentUseLease,
    ) -> std::io::Result<bool> {
        self.0
            .native_uses
            .lock()
            .map(|uses| uses.iter().any(|(_, actual)| actual.same(lease)))
            .map_err(|_| crate::managed_auth::unavailable())
    }

    pub(crate) fn domain(&self) -> &Arc<Domain> {
        &self.0.domain
    }

    pub(crate) fn native_use_for(
        &self,
        lifetime: &crate::native_source_lifetime::Lifetime,
    ) -> std::io::Result<Option<crate::native_source_use::ParentUseLease>> {
        let key = lifetime.incarnation();
        let mut uses = self
            .0
            .native_uses
            .lock()
            .map_err(|_| crate::managed_auth::unavailable())?;
        if let Some(index) = uses.iter().position(|(member, _)| member == &key) {
            let lease = &uses[index].1;
            if !lease.settled() {
                return Ok(Some(lease.clone()));
            }
            let Some((_, registration)) = lifetime.native_use_seed()? else {
                return Err(crate::managed_auth::unavailable());
            };
            if !lease.can_replace(&registration) {
                return Ok(Some(lease.clone()));
            }
            // Only real current Ready plus definitive old release permits a new semantic use.
            uses.remove(index);
        }
        let Some((member, registration)) = lifetime.native_use_seed()? else {
            return Ok(None);
        };
        let lease = crate::native_source_use::ParentUseLease::start_for(
            member,
            registration,
            lifetime.source_domain(),
            lifetime.clone(),
        )?;
        lease.cover_domain(&self.0.domain)?;
        uses.push((key, lease.clone()));
        Ok(Some(lease))
    }
    pub(crate) fn existing_native_use(
        &self,
        lifetime: &crate::native_source_lifetime::Lifetime,
    ) -> std::io::Result<Option<crate::native_source_use::ParentUseLease>> {
        let key = lifetime.incarnation();
        self.0
            .native_uses
            .lock()
            .map(|uses| {
                uses.iter()
                    .find(|(member, _)| member == &key)
                    .map(|(_, lease)| lease.clone())
            })
            .map_err(|_| crate::managed_auth::unavailable())
    }
    pub(crate) fn retain_native_use(
        &self,
        lifetime: &crate::native_source_lifetime::Lifetime,
        lease: &crate::native_source_use::ParentUseLease,
    ) -> std::io::Result<()> {
        let key = lifetime.incarnation();
        let mut uses = self
            .0
            .native_uses
            .lock()
            .map_err(|_| crate::managed_auth::unavailable())?;
        if !uses.iter().any(|(member, _)| member == &key) {
            uses.push((key, lease.clone()));
        }
        Ok(())
    }
    pub fn is_start(&self) -> bool {
        self.0
            .kind
            .lock()
            .is_ok_and(|kind| matches!(*kind, Kind::Start))
    }
    pub fn downgrade(&self) -> ActivityRef {
        ActivityRef(Arc::downgrade(&self.0))
    }
    pub fn same_domain(&self, domain: &Arc<Domain>) -> bool {
        Arc::ptr_eq(&self.0.domain, domain)
    }
    pub fn into_work(self) -> Result<Self, Refused> {
        {
            let mut state = self
                .0
                .domain
                .state
                .lock()
                .map_err(|_| Refused::Unavailable)?;
            let mut kind = self.0.kind.lock().map_err(|_| Refused::Unavailable)?;
            if state.phase == Phase::Uncertain {
                return Err(Refused::Pending);
            }
            match *kind {
                Kind::Start => {
                    state.starts -= 1;
                    state.work += 1;
                    *kind = Kind::Work;
                }
                Kind::Work => {}
                Kind::Resource => return Err(Refused::Invalid),
            }
        }
        Ok(self)
    }
    pub async fn scope<F: std::future::Future>(self, future: F) -> F::Output {
        CURRENT.scope(self, future).await
    }
}
impl Drop for ActivityLease {
    fn drop(&mut self) {
        let Ok(mut state) = self.domain.state.lock() else {
            return;
        };
        let Ok(kind) = self.kind.lock() else {
            return;
        };
        match *kind {
            Kind::Start => state.starts -= 1,
            Kind::Work => state.work -= 1,
            Kind::Resource => state.resources -= 1,
        }
        drop(state);
        self.domain.changed.notify_waiters();
    }
}

struct PermitIdentity;
/// Dropping retirement never reopens admission: cleanup may already have effects.
pub struct RetirementPermit {
    domain: Arc<Domain>,
    identity: Arc<PermitIdentity>,
    operation: String,
    boundary: String,
}
/// A fully drained permit belonging to this exact manager and native operation.
pub struct BoundaryPermit {
    retirement: RetirementPermit,
    native_intent: Option<String>,
}
pub enum FreezeError {
    Pending(RetirementPermit),
    Refused(Refused),
}

pub(crate) fn current_activity() -> Option<Activity> {
    CURRENT.try_with(Clone::clone).ok()
}

impl Domain {
    pub(crate) fn register_native_member(
        &self,
        member: crate::native_source_lifetime::MemberIncarnation,
    ) -> std::io::Result<()> {
        *self
            .native_member
            .lock()
            .map_err(|_| crate::managed_auth::unavailable())? = Some(member);
        Ok(())
    }
    pub(crate) fn native_member(
        &self,
    ) -> std::io::Result<Option<crate::native_source_lifetime::Lifetime>> {
        self.native_member
            .lock()
            .map(|member| {
                member
                    .as_ref()
                    .and_then(crate::native_source_lifetime::MemberIncarnation::upgrade)
            })
            .map_err(|_| crate::managed_auth::unavailable())
    }
    pub(crate) fn native_use_changed(&self) {
        self.changed.notify_waiters();
    }
    pub(crate) fn admit_inherited_native_resource(
        self: &Arc<Self>,
        parent: &Activity,
        lifetime: &crate::native_source_lifetime::Lifetime,
    ) -> Result<Activity, Refused> {
        if parent
            .existing_native_use(lifetime)
            .map_err(|_| Refused::Unavailable)?
            .is_none()
            && !lifetime
                .derived_from(parent)
                .map_err(|_| Refused::Unavailable)?
        {
            return Err(Refused::Pending);
        }
        let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        if matches!(state.phase, Phase::Frozen | Phase::Uncertain) {
            return Err(Refused::Pending);
        }
        state.resources += 1;
        Ok(Activity(Arc::new(ActivityLease {
            domain: Arc::clone(self),
            kind: Mutex::new(Kind::Resource),
            native_uses: Arc::clone(&parent.0.native_uses),
        })))
    }
    pub fn current(self: &Arc<Self>) -> Option<Activity> {
        CURRENT
            .try_with(Clone::clone)
            .ok()
            .filter(|activity| activity.same_domain(self))
    }
    /// Background work cancels its own pending preparation before retirement joins it.
    pub async fn wait_retiring(&self) -> Result<(), Refused> {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self.state.lock() {
                Ok(state)
                    if matches!(
                        state.phase,
                        Phase::Uncertain | Phase::Retiring | Phase::Frozen
                    ) =>
                {
                    return Ok(());
                }
                Ok(_) => {}
                Err(_) => return Err(Refused::Unavailable),
            }
            notified.await;
        }
    }
    pub fn admit_current_resource(self: &Arc<Self>) -> Result<Activity, Refused> {
        self.admit_resource(self.current().as_ref())
    }
    pub fn phase(&self) -> Phase {
        self.state
            .lock()
            .map(|state| state.phase)
            .unwrap_or(Phase::Frozen)
    }
    /// Waits only for root admission; callers must keep approval dispatch independent.
    pub async fn wait_open(&self) -> Result<(), Refused> {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self.state.lock() {
                Ok(state) if state.phase == Phase::Open => return Ok(()),
                Ok(_) => {}
                Err(_) => return Err(Refused::Unavailable),
            }
            notified.await;
        }
    }

    pub fn intent(&self) -> Option<Intent> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.intent.clone())
    }
    pub fn request(&self, intent: Intent) -> Result<(), Refused> {
        if !intent.valid() {
            return Err(Refused::Invalid);
        }
        let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        if let Some(existing) = &state.intent {
            return if existing == &intent {
                Ok(())
            } else {
                Err(Refused::Busy)
            };
        }
        state.intent = Some(intent);
        state.phase = Phase::Pending;
        drop(state);
        self.changed.notify_waiters();
        Ok(())
    }
    /// Negative startup fence only. It mints neither retirement nor boundary
    /// authority and cannot be cleared by ordinary pending cancellation.
    pub fn hold_unresolved(&self, intent: Intent) -> Result<(), Refused> {
        if !intent.valid() {
            return Err(Refused::Invalid);
        }
        let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        if state
            .intent
            .as_ref()
            .is_some_and(|existing| existing != &intent)
            || !matches!(state.phase, Phase::Open | Phase::Pending | Phase::Uncertain)
            || state.permit.upgrade().is_some()
        {
            return Err(Refused::Busy);
        }
        state.intent = Some(intent);
        state.phase = Phase::Uncertain;
        drop(state);
        self.changed.notify_waiters();
        Ok(())
    }
    pub fn cancel_pending(&self, operation: &str) -> Result<(), Refused> {
        let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        if state.phase != Phase::Pending
            || state
                .intent
                .as_ref()
                .is_none_or(|intent| intent.operation != operation)
        {
            return Err(Refused::Busy);
        }
        state.intent = None;
        state.phase = Phase::Open;
        drop(state);
        self.changed.notify_waiters();
        Ok(())
    }
    fn admit(
        self: &Arc<Self>,
        kind: Kind,
        parent: Option<&Activity>,
        publication: bool,
    ) -> Result<Activity, Refused> {
        if parent.is_some_and(|parent| !parent.same_domain(self)) {
            return Err(Refused::WrongDomain);
        }
        let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        let inherited = parent.is_some_and(|parent| {
            parent.0.kind.lock().is_ok_and(|parent_kind| {
                matches!(kind, Kind::Resource) || !matches!(*parent_kind, Kind::Resource)
            })
        });
        let retiring_resource = !publication
            && state.phase == Phase::Retiring
            && matches!(kind, Kind::Resource)
            && parent.is_some_and(|parent| {
                parent
                    .0
                    .kind
                    .lock()
                    .is_ok_and(|kind| matches!(*kind, Kind::Resource))
            });
        let destination = matches!(kind, Kind::Resource)
            && state.phase == Phase::Frozen
            && DESTINATION
                .try_with(|destination| {
                    Arc::ptr_eq(&destination.domain, self)
                        && state.bound.as_ref() == Some(&destination.binding)
                        && destination
                            .permit
                            .upgrade()
                            .zip(state.permit.upgrade())
                            .is_some_and(|(left, right)| Arc::ptr_eq(&left, &right))
                })
                .unwrap_or(false);
        if state.phase != Phase::Open
            && !(state.phase == Phase::Pending && inherited)
            && !retiring_resource
            && !destination
        {
            return Err(Refused::Pending);
        }
        match kind {
            Kind::Start => state.starts += 1,
            Kind::Work => state.work += 1,
            Kind::Resource => state.resources += 1,
        }
        Ok(Activity(Arc::new(ActivityLease {
            domain: Arc::clone(self),
            kind: Mutex::new(kind),
            native_uses: parent
                .map(|parent| Arc::clone(&parent.0.native_uses))
                .unwrap_or_default(),
        })))
    }
    pub fn admit_mutation(self: &Arc<Self>) -> Result<Activity, Refused> {
        let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        if state.phase != Phase::Open {
            return Err(Refused::Pending);
        }
        state.resources += 1;
        Ok(Activity(Arc::new(ActivityLease {
            domain: Arc::clone(self),
            kind: Mutex::new(Kind::Resource),
            native_uses: Arc::default(),
        })))
    }
    pub fn admit_start(self: &Arc<Self>, parent: Option<&Activity>) -> Result<Activity, Refused> {
        self.admit(Kind::Start, parent, false)
    }
    pub fn admit_work(self: &Arc<Self>, parent: Option<&Activity>) -> Result<Activity, Refused> {
        self.admit(Kind::Work, parent, false)
    }
    /// Background admission is nonblocking; shutdown never joins a task waiting on this gate.
    pub fn admit_resource(
        self: &Arc<Self>,
        parent: Option<&Activity>,
    ) -> Result<Activity, Refused> {
        self.admit(Kind::Resource, parent, false)
    }
    /// New cache/connection publications stop at retirement, including old background resources.
    pub fn admit_publication(self: &Arc<Self>) -> Result<Activity, Refused> {
        self.admit(Kind::Resource, self.current().as_ref(), true)
    }
    pub fn try_retire(self: &Arc<Self>, operation: &str) -> Result<RetirementPermit, Refused> {
        let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        if state.phase != Phase::Pending
            || state
                .intent
                .as_ref()
                .is_none_or(|intent| intent.operation != operation)
            || state.work != 0
            || state.starts != 0
        {
            return Err(Refused::Busy);
        }
        let identity = Arc::new(PermitIdentity);
        let boundary = format!("{:032x}", rand::random::<u128>());
        state.permit = Arc::downgrade(&identity);
        state.phase = Phase::Retiring;
        state.boundary = Some(boundary.clone());
        self.changed.notify_waiters();
        Ok(RetirementPermit {
            domain: Arc::clone(self),
            identity,
            operation: operation.into(),
            boundary,
        })
    }
    pub async fn retire(self: &Arc<Self>, operation: &str) -> Result<RetirementPermit, Refused> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self
                .intent()
                .as_ref()
                .is_none_or(|intent| intent.operation != operation)
                || self.phase() != Phase::Pending
            {
                return Err(Refused::Busy);
            }
            match self.try_retire(operation) {
                Ok(permit) => return Ok(permit),
                Err(Refused::Busy) => changed.await,
                Err(error) => return Err(error),
            }
        }
    }
    pub fn recover_retirement(
        self: &Arc<Self>,
        operation: &str,
    ) -> Result<RetirementPermit, Refused> {
        let mut state = self.state.lock().map_err(|_| Refused::Unavailable)?;
        if !matches!(state.phase, Phase::Retiring | Phase::Frozen)
            || state.permit.upgrade().is_some()
            || state
                .intent
                .as_ref()
                .is_none_or(|intent| intent.operation != operation)
        {
            return Err(Refused::Busy);
        }
        let boundary = state.boundary.clone().ok_or(Refused::Unavailable)?;
        let identity = Arc::new(PermitIdentity);
        state.permit = Arc::downgrade(&identity);
        state.phase = Phase::Retiring;
        self.changed.notify_waiters();
        Ok(RetirementPermit {
            domain: Arc::clone(self),
            identity,
            operation: operation.into(),
            boundary,
        })
    }
}

impl RetirementPermit {
    pub fn protects(&self, domain: &Arc<Domain>) -> bool {
        Arc::ptr_eq(&self.domain, domain)
            && self.domain.state.lock().is_ok_and(|state| {
                self.matches(&state)
                    && state.phase == Phase::Retiring
                    && state.work == 0
                    && state.starts == 0
            })
    }
    fn matches(&self, state: &State) -> bool {
        state
            .intent
            .as_ref()
            .is_some_and(|intent| intent.operation == self.operation)
            && state
                .permit
                .upgrade()
                .is_some_and(|identity| Arc::ptr_eq(&identity, &self.identity))
    }
    fn fully_drained(&self, state: &State) -> bool {
        state.work == 0
            && state.starts == 0
            && state.resources == 0
            && crate::native_source_use::settled_for(&self.domain)
    }
    pub fn freeze(self) -> Result<BoundaryPermit, FreezeError> {
        let ready = match self.domain.state.lock() {
            Ok(mut state) => {
                if !self.matches(&state)
                    || state.phase != Phase::Retiring
                    || state.work != 0
                    || state.starts != 0
                {
                    return Err(FreezeError::Refused(Refused::Busy));
                }
                if !self.fully_drained(&state) {
                    None
                } else {
                    state.phase = Phase::Frozen;
                    Some(state.native_intent.clone())
                }
            }
            Err(_) => return Err(FreezeError::Refused(Refused::Unavailable)),
        };
        if let Some(native_intent) = ready {
            Ok(BoundaryPermit {
                retirement: self,
                native_intent,
            })
        } else {
            Err(FreezeError::Pending(self))
        }
    }
    /// Retained operation actor variant: failure never drops retirement custody.
    pub async fn wait_drained(self) -> BoundaryPermit {
        let domain = Arc::clone(&self.domain);
        loop {
            let changed = domain.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let ready = self.domain.state.lock().ok().and_then(|mut state| {
                if self.matches(&state)
                    && state.phase == Phase::Retiring
                    && self.fully_drained(&state)
                {
                    state.phase = Phase::Frozen;
                    Some(state.native_intent.clone())
                } else {
                    None
                }
            });
            if let Some(native_intent) = ready {
                return BoundaryPermit {
                    retirement: self,
                    native_intent,
                };
            }
            changed.await;
        }
    }
    pub async fn drained(mut self) -> Result<BoundaryPermit, Refused> {
        let domain = Arc::clone(&self.domain);
        loop {
            let changed = domain.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            match self.freeze() {
                Ok(boundary) => return Ok(boundary),
                Err(FreezeError::Pending(retirement)) => self = retirement,
                Err(FreezeError::Refused(error)) => return Err(error),
            }
            changed.await;
        }
    }
}

impl BoundaryPermit {
    /// Only a source already published under this permit may prepare destination resources.
    pub async fn destination<F: std::future::Future>(
        &self,
        binding: &Binding,
        future: F,
    ) -> Result<F::Output, Refused> {
        {
            let state = self
                .retirement
                .domain
                .state
                .lock()
                .map_err(|_| Refused::Unavailable)?;
            if !self.retirement.matches(&state)
                || state.phase != Phase::Frozen
                || state.bound.as_ref() != Some(binding)
            {
                return Err(Refused::Busy);
            }
        }
        Ok(DESTINATION
            .scope(
                Destination {
                    domain: Arc::clone(&self.retirement.domain),
                    permit: Arc::downgrade(&self.retirement.identity),
                    binding: binding.clone(),
                },
                future,
            )
            .await)
    }
    pub fn bound(&self) -> Result<Binding, Refused> {
        let state = self
            .retirement
            .domain
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        if !self.retirement.matches(&state) || state.phase != Phase::Frozen {
            return Err(Refused::Busy);
        }
        state.bound.clone().ok_or(Refused::Busy)
    }
    pub fn operation(&self) -> &str {
        &self.retirement.operation
    }
    pub fn boundary(&self) -> &str {
        &self.retirement.boundary
    }
    pub fn native_intent(&self) -> Option<&str> {
        self.native_intent.as_deref()
    }
    pub fn set_native_intent(&mut self, intent: String) -> Result<(), Refused> {
        if !crate::managed_auth::valid_hex(&intent, 32) {
            return Err(Refused::Invalid);
        }
        let mut state = self
            .retirement
            .domain
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        if !self.retirement.matches(&state)
            || state.phase != Phase::Frozen
            || state
                .native_intent
                .as_ref()
                .is_some_and(|existing| existing != &intent)
        {
            return Err(Refused::Busy);
        }
        state.native_intent = Some(intent.clone());
        self.native_intent = Some(intent);
        Ok(())
    }
    pub fn protects(
        &self,
        domain: &Arc<Domain>,
        operation: &str,
        native_intent: Option<&str>,
    ) -> bool {
        Arc::ptr_eq(&self.retirement.domain, domain)
            && self.operation() == operation
            && self.native_intent.as_deref() == native_intent
            && native_intent.is_some()
            && domain
                .state
                .lock()
                .is_ok_and(|state| state.phase == Phase::Frozen && self.retirement.matches(&state))
    }
    pub fn record_bound(&self, binding: Binding) -> Result<(), Refused> {
        if !crate::managed_auth::valid_hex(&binding.account, 64)
            || !crate::managed_auth::valid_hex(&binding.user, 64)
            || !crate::managed_auth::valid_hex(&binding.digest, 64)
            || binding.credential_generation == 0
            || binding.assignment_generation == 0
        {
            return Err(Refused::Invalid);
        }
        let mut state = self
            .retirement
            .domain
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        if !self.retirement.matches(&state)
            || state.phase != Phase::Frozen
            || self.native_intent.is_none()
        {
            return Err(Refused::Busy);
        }
        if state.bound.as_ref() != Some(&binding) {
            state.resources_ready = false;
        }
        state.bound = Some(binding);
        state.restored = false;
        Ok(())
    }
    /// The source adapter calls this only after native no-install proof and a fresh unchanged source read.
    pub fn restoration_allowed(&self) -> bool {
        self.retirement.domain.state.lock().is_ok_and(|state| {
            self.retirement.matches(&state)
                && state.phase == Phase::Frozen
                && (state.bound.is_none() || state.restored)
        })
    }
    pub fn record_restored(&self, binding: Binding) -> Result<(), Refused> {
        if !crate::managed_auth::valid_hex(&binding.account, 64)
            || !crate::managed_auth::valid_hex(&binding.user, 64)
            || !crate::managed_auth::valid_hex(&binding.digest, 64)
            || binding.credential_generation == 0
        {
            return Err(Refused::Invalid);
        }
        let mut state = self
            .retirement
            .domain
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        if !self.retirement.matches(&state)
            || state.phase != Phase::Frozen
            || (state.bound.is_some() && !state.restored)
        {
            return Err(Refused::Busy);
        }
        state.bound = Some(binding);
        state.resources_ready = false;
        state.restored = true;
        Ok(())
    }
    pub fn abort(self, completion: AbortCompletion) -> Result<(), Refused> {
        self.try_abort(&completion)
    }
    /// Failed finalization retains the same live retirement authority for reconciliation.
    pub fn try_abort(&self, completion: &AbortCompletion) -> Result<(), Refused> {
        let mut state = self
            .retirement
            .domain
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        if !self.retirement.matches(&state)
            || state.phase != Phase::Frozen
            || !state.restored
            || !state.resources_ready
            || self.operation() != completion.operation
            || self.boundary() != completion.boundary
            || state.bound.as_ref() != Some(&completion.binding)
        {
            return Err(Refused::Busy);
        }
        state.phase = Phase::Open;
        state.intent = None;
        state.permit = Weak::new();
        state.boundary = None;
        state.native_intent = None;
        state.bound = None;
        state.restored = false;
        state.resources_ready = false;
        drop(state);
        self.retirement.domain.changed.notify_waiters();
        Ok(())
    }
    pub fn record_resources_ready(&self, binding: &Binding) -> Result<(), Refused> {
        let mut state = self
            .retirement
            .domain
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        if !self.retirement.matches(&state)
            || state.phase != Phase::Frozen
            || state.bound.as_ref() != Some(binding)
        {
            return Err(Refused::Busy);
        }
        state.resources_ready = true;
        Ok(())
    }
    /// Called only after authoritative reconciliation and owner-bound resource readiness.
    pub fn finish(self, completion: Completion) -> Result<(), Refused> {
        self.try_finish(&completion)
    }
    /// A transient failure cannot consume the operation's owned freeze.
    pub fn try_finish(&self, completion: &Completion) -> Result<(), Refused> {
        let mut state = self
            .retirement
            .domain
            .state
            .lock()
            .map_err(|_| Refused::Unavailable)?;
        if !self.retirement.matches(&state)
            || state.phase != Phase::Frozen
            || !state.resources_ready
            || state.restored
            || self.native_intent.as_deref() != Some(completion.native_intent.as_str())
            || self.operation() != completion.operation
            || self.boundary() != completion.boundary
            || state.bound.as_ref() != Some(&completion.binding)
        {
            return Err(Refused::Busy);
        }
        state.phase = Phase::Open;
        state.intent = None;
        state.permit = Weak::new();
        state.boundary = None;
        state.native_intent = None;
        state.bound = None;
        state.restored = false;
        state.resources_ready = false;
        drop(state);
        self.retirement.domain.changed.notify_waiters();
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptStamp {
    pub credential_generation: u64,
    pub assignment_generation: u64,
    pub generations: Generations,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub operation: String,
    pub boundary: Option<String>,
    pub thread: String,
    pub selection: Selection,
    pub source: ReceiptStamp,
    pub destination: Option<ReceiptStamp>,
    pub acknowledged: bool,
}
impl Receipt {
    pub fn valid(&self) -> bool {
        Intent {
            operation: self.operation.clone(),
            thread: self.thread.clone(),
            selection: self.selection.clone(),
        }
        .valid()
            && self
                .boundary
                .as_ref()
                .is_none_or(|boundary| crate::managed_auth::valid_hex(boundary, 32))
            && self.source.credential_generation > 0
            && self.destination.as_ref().is_none_or(|destination| {
                destination.credential_generation > 0
                    && destination.assignment_generation > 0
                    && self.boundary.is_some()
            })
            && (!self.acknowledged || self.destination.is_some())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Preparing,
    Draining,
    Committing,
    Rebinding,
    Initializing,
    Acknowledging,
    Applied,
    Refused,
    Reconciling,
    Cancelled,
}
#[derive(Clone, Debug)]
pub struct Observation {
    pub operation: String,
    pub progress: Progress,
    pub target: Option<String>,
    pub mode: Option<crate::assignment_service::Mode>,
    pub reason: Option<crate::assignment_service::Reason>,
}
pub type AdmissionFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Refused>> + Send + 'a>>;
pub trait Consumer: Send + Sync {
    fn request<'a>(&'a self, intent: Intent) -> AdmissionFuture<'a>;
    fn observation(&self) -> Option<Observation>;
}
static CONSUMERS: std::sync::OnceLock<
    Mutex<std::collections::BTreeMap<String, Weak<dyn Consumer>>>,
> = std::sync::OnceLock::new();
pub fn register_consumer(thread: &str, consumer: &Arc<dyn Consumer>) {
    if let Ok(mut consumers) = CONSUMERS.get_or_init(Default::default).lock() {
        consumers.retain(|_, consumer| consumer.strong_count() != 0);
        consumers.insert(thread.into(), Arc::downgrade(consumer));
    }
}
/// Capture at the explicit user action, so a later same-thread replacement cannot retarget it.
pub fn consumer(thread: &str) -> Option<Arc<dyn Consumer>> {
    CONSUMERS.get()?.lock().ok()?.get(thread)?.upgrade()
}

/// Owned SDK actor/native source admission. A transport-only target preview is not this proof.
/// Implementations retain the native source-work lease and the sealed local manager catalog;
/// every credential load/rebind must participate before an implementation may be installed.
pub trait ProcessAdmission: Send + Sync {
    fn validate(&self) -> Result<(), Refused>;
}
pub type ProcessFuture<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<Box<dyn ProcessAdmission>, Refused>> + Send + 'a>,
>;
pub trait ProcessHost: Send + Sync {
    /// Validate every registered manager's target policy while preventing uncensused source loads.
    fn admit<'a>(&'a self, target: &'a crate::assignment_service::Target) -> ProcessFuture<'a>;
}
static PROCESS_HOST: std::sync::OnceLock<Arc<dyn ProcessHost>> = std::sync::OnceLock::new();
pub fn install_process_host(host: Arc<dyn ProcessHost>) {
    let _ = PROCESS_HOST.set(host);
}
pub async fn admit_process(
    target: &crate::assignment_service::Target,
) -> Result<Box<dyn ProcessAdmission>, Refused> {
    let admission = PROCESS_HOST
        .get()
        .ok_or(Refused::Unavailable)?
        .admit(target)
        .await?;
    admission.validate()?;
    Ok(admission)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unresolved_startup_blocks_even_inherited_activity_without_fabricating_a_permit() {
        let domain = Arc::new(Domain::default());
        let start = domain.admit_start(None).unwrap();
        let work = domain.admit_work(None).unwrap();
        domain.hold_unresolved(intent()).unwrap();
        assert_eq!(domain.phase(), Phase::Uncertain);
        assert!(domain.admit_work(None).is_err());
        assert!(domain.admit_work(Some(&work)).is_err());
        assert!(domain.admit_start(Some(&work)).is_err());
        assert!(domain.admit_resource(Some(&work)).is_err());
        assert!(domain.admit_mutation().is_err());
        assert!(start.into_work().is_err());
        assert!(domain.cancel_pending(&intent().operation).is_err());
        assert!(domain.try_retire(&intent().operation).is_err());
        assert!(domain.recover_retirement(&intent().operation).is_err());
        domain.request(intent()).unwrap();
        assert_eq!(domain.phase(), Phase::Uncertain);
    }
    #[tokio::test]
    async fn failed_binding_and_resource_initialization_retain_the_same_owned_boundary() {
        let domain = Arc::new(Domain::default());
        domain.request(intent()).unwrap();
        let mut boundary = domain
            .retire(&intent().operation)
            .await
            .unwrap()
            .wait_drained()
            .await;
        boundary.set_native_intent("f".repeat(32)).unwrap();
        let binding = Binding {
            account: "1".repeat(64),
            user: "2".repeat(64),
            digest: "3".repeat(64),
            credential_generation: 7,
            assignment_generation: 3,
            generations: Generations {
                auth: 1,
                owner: 2,
                source: 3,
                login: 4,
            },
        };
        boundary.record_bound(binding.clone()).unwrap();
        let completion = Completion {
            operation: boundary.operation().into(),
            native_intent: "f".repeat(32),
            boundary: boundary.boundary().into(),
            binding,
        };
        let mut wrong = completion.binding.clone();
        wrong.generations.owner += 1;
        assert!(
            boundary
                .destination(&wrong, async { panic!("wrong source must not initialize") })
                .await
                .is_err()
        );
        for _ in 0..3 {
            let failure: Result<(), Refused> = boundary
                .destination(&completion.binding, async { Err(Refused::Unavailable) })
                .await
                .unwrap();
            assert!(failure.is_err());
            assert!(domain.admit_work(None).is_err());
            let mut invalid = Completion {
                operation: completion.operation.clone(),
                native_intent: completion.native_intent.clone(),
                boundary: completion.boundary.clone(),
                binding: completion.binding.clone(),
            };
            invalid.binding.generations.source += 1;
            assert!(boundary.try_finish(&invalid).is_err());
            assert!(boundary.protects(
                &domain,
                &completion.operation,
                Some(&completion.native_intent)
            ));
        }
        assert!(
            boundary.try_finish(&completion).is_err(),
            "failed initialization grants no readiness"
        );
        boundary
            .destination(&completion.binding, async {})
            .await
            .unwrap();
        boundary
            .record_resources_ready(&completion.binding)
            .unwrap();
        boundary.try_finish(&completion).unwrap();
        assert!(domain.admit_work(None).is_ok());
    }
    fn ready(boundary: &mut BoundaryPermit) -> Completion {
        if boundary.native_intent().is_none() {
            boundary.set_native_intent("f".repeat(32)).unwrap();
        }
        let binding = boundary.bound().unwrap_or(Binding {
            account: "1".repeat(64),
            user: "2".repeat(64),
            digest: "3".repeat(64),
            credential_generation: 1,
            assignment_generation: 1,
            generations: Generations {
                auth: 2,
                owner: 1,
                source: 1,
                login: 1,
            },
        });
        boundary.record_bound(binding.clone()).unwrap();
        boundary.record_resources_ready(&binding).unwrap();
        Completion {
            operation: boundary.operation().into(),
            native_intent: boundary.native_intent().unwrap().into(),
            boundary: boundary.boundary().into(),
            binding,
        }
    }
    fn intent() -> Intent {
        Intent {
            operation: "a".repeat(32),
            thread: "00000000-0000-0000-0000-000000000000".into(),
            selection: Selection::Manual {
                account: "b".repeat(64),
                revision: "c".repeat(64),
            },
        }
    }

    #[tokio::test]
    async fn pending_assignment_drains_nested_work_without_blocking_its_parent() {
        let domain = Arc::new(Domain::default());
        let parent = domain.admit_work(None).unwrap();
        let recorded = parent.downgrade();
        domain.request(intent()).unwrap();
        assert!(matches!(domain.admit_work(None), Err(Refused::Pending)));
        let child = domain
            .admit_start(Some(&parent))
            .unwrap()
            .into_work()
            .unwrap();
        drop(parent);
        let operation = intent().operation;
        let retirement = domain.retire(&operation);
        tokio::pin!(retirement);
        assert!(matches!(
            futures_poll(retirement.as_mut()),
            std::task::Poll::Pending
        ));
        drop(child);
        let retirement = tokio::time::timeout(std::time::Duration::from_secs(1), retirement)
            .await
            .unwrap()
            .unwrap();
        assert!(
            recorded.upgrade().is_none(),
            "a stored weak turn reference must not pin completed work"
        );
        let mut boundary = retirement.freeze().ok().unwrap();
        boundary.set_native_intent("d".repeat(32)).unwrap();
        assert!(boundary.protects(&domain, &intent().operation, Some(&"d".repeat(32))));
        assert!(!boundary.protects(
            &Arc::new(Domain::default()),
            &intent().operation,
            Some(&"d".repeat(32))
        ));
        let completion = ready(&mut boundary);
        boundary.finish(completion).unwrap();
        assert!(domain.admit_work(None).is_ok());
    }

    fn futures_poll<F: std::future::Future>(
        future: std::pin::Pin<&mut F>,
    ) -> std::task::Poll<F::Output> {
        future.poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    }

    #[tokio::test]
    async fn retirement_can_join_cancelled_unadmitted_prewarm_while_gate_stays_closed() {
        let domain = Arc::new(Domain::default());
        let source_mcp = domain.admit_resource(None).unwrap();
        domain.request(intent()).unwrap();
        let retirement = domain.try_retire(&intent().operation).unwrap();
        assert!(matches!(domain.admit_resource(None), Err(Refused::Pending)));
        let (cancel, stop) = tokio::sync::oneshot::channel();
        let prewarm_domain = Arc::clone(&domain);
        let prewarm = tokio::spawn(async move {
            assert!(prewarm_domain.admit_resource(None).is_err());
            let _ = stop.await;
        });
        cancel.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), prewarm)
            .await
            .unwrap()
            .unwrap();
        let shutdown_io = domain.admit_resource(Some(&source_mcp)).unwrap();
        assert!(retirement.freeze().is_err());
        drop(shutdown_io);
        drop(source_mcp);
        let recovered = domain.recover_retirement(&intent().operation).unwrap();
        let mut boundary = recovered.drained().await.unwrap();
        assert_eq!(domain.phase(), Phase::Frozen);
        let completion = ready(&mut boundary);
        boundary.finish(completion).unwrap();
    }

    #[tokio::test]
    async fn retirement_cancels_preparation_and_destination_admission_requires_exact_binding() {
        let domain = Arc::new(Domain::default());
        let resource = domain.admit_resource(None).unwrap();
        let old = Arc::clone(&domain);
        let preparing = tokio::spawn(resource.scope(async move {
            old.wait_retiring().await.unwrap();
        }));
        domain.request(intent()).unwrap();
        let retirement = domain.try_retire(&intent().operation).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), preparing)
            .await
            .unwrap()
            .unwrap();
        let mut boundary = retirement.drained().await.unwrap();
        let completion = ready(&mut boundary);
        assert!(domain.admit_current_resource().is_err());
        let mut wrong = completion.binding.clone();
        wrong.generations.owner += 1;
        assert!(
            boundary
                .destination(&wrong, async {
                    panic!("wrong owner must not run");
                })
                .await
                .is_err()
        );
        boundary
            .destination(&completion.binding, async {
                let destination = domain.admit_current_resource().unwrap();
                assert!(domain.admit_work(None).is_err());
                let unrelated = Arc::clone(&domain);
                assert!(
                    tokio::spawn(async move { unrelated.admit_current_resource().is_err() })
                        .await
                        .unwrap()
                );
                drop(destination);
            })
            .await
            .unwrap();
        assert!(domain.admit_current_resource().is_err());
        boundary.finish(completion).unwrap();
    }

    #[test]
    fn verified_unchanged_source_restoration_cannot_be_used_as_destination_ack() {
        let domain = Arc::new(Domain::default());
        domain.request(intent()).unwrap();
        let mut boundary = domain
            .try_retire(&intent().operation)
            .unwrap()
            .freeze()
            .ok()
            .unwrap();
        let completion = ready(&mut boundary);
        assert!(
            !boundary.restoration_allowed(),
            "an observed destination can never be reverted as unchanged"
        );
        assert!(boundary.record_restored(completion.binding).is_err());
        drop(boundary);
        let other = Arc::new(Domain::default());
        other.request(intent()).unwrap();
        let mut boundary = other
            .try_retire(&intent().operation)
            .unwrap()
            .freeze()
            .ok()
            .unwrap();
        let binding = Binding {
            account: "1".repeat(64),
            user: "2".repeat(64),
            digest: "3".repeat(64),
            credential_generation: 14,
            assignment_generation: 0,
            generations: Generations {
                auth: 1,
                owner: 0,
                source: 0,
                login: 0,
            },
        };
        boundary.record_restored(binding.clone()).unwrap();
        boundary.record_resources_ready(&binding).unwrap();
        boundary.set_native_intent("a".repeat(32)).unwrap();
        let fake = Completion {
            operation: boundary.operation().into(),
            native_intent: "a".repeat(32),
            boundary: boundary.boundary().into(),
            binding: binding.clone(),
        };
        assert!(
            boundary.finish(fake).is_err(),
            "a restored source is not an applied destination"
        );
        let boundary = other
            .recover_retirement(&intent().operation)
            .unwrap()
            .freeze()
            .ok()
            .unwrap();
        let completion = AbortCompletion {
            operation: boundary.operation().into(),
            boundary: boundary.boundary().into(),
            binding,
        };
        boundary.abort(completion).unwrap();
        assert_eq!(other.phase(), Phase::Open);
    }

    #[test]
    fn assignment_record_is_inert_and_prevents_stale_startup_reseeding() {
        let shared = crate::session::Shared::new();
        let intent = intent();
        let receipt = Receipt {
            operation: intent.operation,
            thread: intent.thread,
            selection: intent.selection,
            boundary: None,
            source: ReceiptStamp {
                credential_generation: 14,
                assignment_generation: 0,
                generations: Generations {
                    auth: 0,
                    owner: 0,
                    source: 0,
                    login: 0,
                },
            },
            destination: None,
            acknowledged: false,
        };
        assert!(shared.set_assignment_receipt(Some(receipt)));
        assert_eq!(shared.unrest(), Some(crate::session::Unrest::Acting));
        assert_eq!(
            shared.native_work(),
            (false, None, false),
            "account selection does not forge an owed model turn"
        );
        assert!(!shared.has_record_writer());
        assert!(shared.set_assignment_receipt(None));
        assert!(!shared.seed_before_native_work(|| panic!(
            "a stale record cannot resurrect the cleared intent"
        )));
        assert!(shared.assignment_receipt().is_none());
    }

    #[test]
    fn uncertain_commit_drop_stays_frozen_and_preserves_boundary_and_native_intent() {
        let domain = Arc::new(Domain::default());
        domain.request(intent()).unwrap();
        let mut boundary = domain
            .try_retire(&intent().operation)
            .unwrap()
            .freeze()
            .ok()
            .unwrap();
        boundary.set_native_intent("e".repeat(32)).unwrap();
        let original = boundary.boundary().to_owned();
        assert!(domain.recover_retirement(&intent().operation).is_err());
        drop(boundary);
        assert_eq!(domain.phase(), Phase::Frozen);
        assert!(domain.admit_work(None).is_err());
        let mut boundary = domain
            .recover_retirement(&intent().operation)
            .unwrap()
            .freeze()
            .ok()
            .unwrap();
        assert_eq!(boundary.boundary(), original);
        assert_eq!(boundary.native_intent(), Some("e".repeat(32).as_str()));
        let completion = ready(&mut boundary);
        boundary.finish(completion).unwrap();
    }

    #[test]
    fn completion_cannot_reopen_without_bound_resources_and_exact_owner_ack() {
        let domain = Arc::new(Domain::default());
        domain.request(intent()).unwrap();
        let mut boundary = domain
            .try_retire(&intent().operation)
            .unwrap()
            .freeze()
            .ok()
            .unwrap();
        boundary.set_native_intent("9".repeat(32)).unwrap();
        let completion = ready(&mut boundary);
        let mut wrong = completion.binding.clone();
        wrong.generations.owner += 1;
        let rejected = Completion {
            operation: completion.operation,
            native_intent: completion.native_intent,
            boundary: completion.boundary,
            binding: wrong,
        };
        assert!(boundary.finish(rejected).is_err());
        assert_eq!(domain.phase(), Phase::Frozen);
        let mut recovered = domain
            .recover_retirement(&intent().operation)
            .unwrap()
            .freeze()
            .ok()
            .unwrap();
        let completion = ready(&mut recovered);
        recovered.finish(completion).unwrap();
        assert_eq!(domain.phase(), Phase::Open);
    }
    #[tokio::test]
    async fn retirement_drains_old_resources_but_forbids_late_publication() {
        let domain = Arc::new(Domain::default());
        let resource = domain.admit_resource(None).unwrap();
        domain.request(intent()).unwrap();
        let retirement = domain.try_retire(&intent().operation).unwrap();
        resource
            .scope(async {
                assert!(
                    domain.admit_current_resource().is_ok(),
                    "old cleanup may finish"
                );
                assert!(
                    domain.admit_publication().is_err(),
                    "old cleanup cannot republish connections"
                );
            })
            .await;
        let mut boundary = retirement.drained().await.unwrap();
        let completion = ready(&mut boundary);
        boundary
            .destination(&completion.binding, async {
                assert!(
                    domain.admit_publication().is_ok(),
                    "only the exact bound destination may initialize"
                );
            })
            .await
            .unwrap();
        boundary.finish(completion).unwrap();
    }
}
