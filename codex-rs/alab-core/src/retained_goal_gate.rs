// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Scheduling inhibitor for one retained host, never SDK/provider authority.
//! Admission guards cover the real automatic-goal read/submit window. Holding
//! drains those windows; it does not cancel already accepted turns or effects.
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::OwnedRwLockReadGuard;
use tokio::sync::RwLock;

struct State {
    epoch: u64,
    held: bool,
    next_inhibitor: u64,
    inhibitors: BTreeMap<u64, Arc<Ticket>>,
}
struct Inner {
    state: Arc<RwLock<State>>,
    withdrawn: AtomicBool,
    admission_closed: AtomicBool,
}
#[derive(Clone)]
pub struct Gate(Arc<Inner>);
pub struct Owner {
    gate: Gate,
}
pub struct Hold {
    gate: Gate,
    epoch: u64,
}
/// Monotonic automatic-admission closure on the SAME gate. It is not a
/// Native reservation, a new hold, or completed runtime/effects cleanup.
#[derive(Clone)]
pub struct ShutdownBarrier {
    gate: Gate,
}
impl ShutdownBarrier {
    /// Join every actual automatic read window that preceded closure. A
    /// canceled observation leaves closure intact; a later wait joins the
    /// same writer boundary without opening or replacing any owner resource.
    pub async fn drain(&self) {
        drop(self.gate.0.state.write().await);
    }
}
pub struct Admission {
    _guard: OwnedRwLockReadGuard<State>,
}
// Independent Source scheduling custody, never a Native claim or effect lease.
struct Ticket {
    id: u64,
    active: AtomicBool,
}
pub struct Inhibitor {
    gate: Gate,
    ticket: Arc<Ticket>,
}
impl Inhibitor {
    pub fn same_gate(&self, actual: &Gate) -> bool {
        self.gate.same(actual)
    }
    /// Actual Source inhibition only. It grants no SDK/role/currentness/Go.
    pub fn is_held(&self) -> bool {
        self.ticket.active.load(Ordering::Acquire) && !self.gate.0.withdrawn.load(Ordering::Acquire)
    }
}
// Like Hold, dropping this resource cannot reopen admission. An unknown
// accepted worker must retain the deny until actual owner cleanup/withdrawal.

impl Owner {
    /// Bind this initially closed gate to the actual retained host and every
    /// real child before publishing a root. This grants no work permission.
    pub fn new_held() -> (Self, Gate, Hold) {
        let gate = Gate(Arc::new(Inner {
            state: Arc::new(RwLock::new(State {
                epoch: 1,
                held: true,
                next_inhibitor: 0,
                inhibitors: BTreeMap::new(),
            })),
            withdrawn: AtomicBool::new(false),
            admission_closed: AtomicBool::new(false),
        }));
        (
            Self { gate: gate.clone() },
            gate.clone(),
            Hold { gate, epoch: 1 },
        )
    }
    /// The accepted native replacement worker owns this future, not a UI
    /// observer. After return no automatic admission window predates the hold.
    pub async fn hold(&self) -> Result<Hold, String> {
        let mut state = self.gate.0.state.write().await;
        if self.gate.0.withdrawn.load(Ordering::Acquire) {
            return Err("retained goal owner withdrew".into());
        }
        if state.held {
            return Err("retained goal admission already has an owner".into());
        }
        let epoch = state
            .epoch
            .checked_add(1)
            .ok_or("retained goal admission epoch exhausted")?;
        state.epoch = epoch;
        state.held = true;
        Ok(Hold {
            gate: self.gate.clone(),
            epoch,
        })
    }
    /// Drain the SAME actual automatic read windows and install a distinct
    /// Source inhibitor even while an original Native Hold already exists.
    /// The owning accepted operation retains this future and returned ticket.
    /// No Native action, fresh Gate, credential role or operation is made.
    pub async fn inhibit(&self) -> Result<Inhibitor, String> {
        let mut state = self.gate.0.state.write().await;
        if self.gate.0.withdrawn.load(Ordering::Acquire) {
            return Err("retained goal owner withdrew".into());
        }
        if state.inhibitors.len() >= 4096 {
            return Err("retained Source inhibitor capacity reached".into());
        }
        let id = state
            .next_inhibitor
            .checked_add(1)
            .ok_or("retained Source inhibitor sequence exhausted")?;
        let ticket = Arc::new(Ticket {
            id,
            active: AtomicBool::new(true),
        });
        state.next_inhibitor = id;
        state.inhibitors.insert(id, ticket.clone());
        Ok(Inhibitor {
            gate: self.gate.clone(),
            ticket,
        })
    }
    /// The actual SDK selection consumer must first observe its genuine
    /// Native/UI commit, then repeat exact candidate/prior ownership here.
    /// Removes only this Source ticket: another ticket or Native Hold still
    /// inhibits admission. The callback has no Native authority itself.
    pub async fn release_inhibitor_if(
        &self,
        inhibitor: &Inhibitor,
        recheck: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        if !self.gate.same(&inhibitor.gate) {
            return Err("foreign retained Source inhibitor".into());
        }
        let mut state = self.gate.0.state.write().await;
        if self.gate.0.withdrawn.load(Ordering::Acquire) {
            return Err("retained goal owner withdrew".into());
        }
        if !state
            .inhibitors
            .get(&inhibitor.ticket.id)
            .is_some_and(|actual| Arc::ptr_eq(actual, &inhibitor.ticket))
        {
            return Err("retained Source inhibitor changed".into());
        }
        recheck()?;
        if self.gate.0.withdrawn.load(Ordering::Acquire) {
            return Err("retained goal owner withdrew".into());
        }
        state.inhibitors.remove(&inhibitor.ticket.id);
        inhibitor.ticket.active.store(false, Ordering::Release);
        Ok(())
    }
    /// After the genuine SDK selection commit, retire this exact Source
    /// ticket and, when provided, the exact still-closed Native Hold under ONE
    /// gate writer. None removes only the ticket. These resources correlate
    /// Source scheduling custody; the callback and this method grant no SDK,
    /// Native or UI authority. A failure leaves both resources unchanged.
    pub async fn release_selection_if(
        &self,
        inhibitor: &Inhibitor,
        hold: Option<&Hold>,
        recheck: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        if !self.gate.same(&inhibitor.gate) {
            return Err("foreign retained Source inhibitor".into());
        }
        if hold.is_some_and(|hold| !self.gate.same(&hold.gate)) {
            return Err("foreign retained goal hold".into());
        }
        let mut state = self.gate.0.state.write().await;
        if self.gate.0.withdrawn.load(Ordering::Acquire) {
            return Err("retained goal owner withdrew".into());
        }
        if !state
            .inhibitors
            .get(&inhibitor.ticket.id)
            .is_some_and(|actual| Arc::ptr_eq(actual, &inhibitor.ticket))
        {
            return Err("retained Source inhibitor changed".into());
        }
        if hold.is_some_and(|hold| !state.held || state.epoch != hold.epoch) {
            return Err("retained goal hold changed".into());
        }
        recheck()?;
        if self.gate.0.withdrawn.load(Ordering::Acquire) {
            return Err("retained goal owner withdrew".into());
        }
        state.inhibitors.remove(&inhibitor.ticket.id);
        inhibitor.ticket.active.store(false, Ordering::Release);
        if hold.is_some() {
            state.held = false;
        }
        Ok(())
    }
    /// The real SDK adapter must FIRST await the exact Native Go/frontier
    /// activation, then recheck its actual runtime/hold binding here. This call
    /// only removes a Source inhibitor; it supplies no Native/model admission.
    /// Failed or cancelled release leaves the same hold closed.
    pub async fn release_if(
        &self,
        hold: &Hold,
        recheck: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        if !Arc::ptr_eq(&self.gate.0, &hold.gate.0) {
            return Err("foreign retained goal hold".into());
        }
        let mut state = self.gate.0.state.write().await;
        if self.gate.0.withdrawn.load(Ordering::Acquire) {
            return Err("retained goal owner withdrew".into());
        }
        if !state.held || state.epoch != hold.epoch {
            return Err("retained goal hold changed".into());
        }
        recheck()?;
        if self.gate.0.withdrawn.load(Ordering::Acquire) {
            return Err("retained goal owner withdrew".into());
        }
        state.held = false;
        Ok(())
    }
    /// Close only new automatic admission, synchronously. Original Source
    /// cleanup still owns its exact Hold/Inhibitor bookkeeping and may observe
    /// genuine cancellation receipts before final owner withdrawal. No call,
    /// resource drop, release, or cancellation can reopen this flag.
    pub fn begin_shutdown(&self) -> ShutdownBarrier {
        self.gate.0.admission_closed.store(true, Ordering::Release);
        ShutdownBarrier {
            gate: self.gate.clone(),
        }
    }
    /// Actual owning-runtime withdrawal closes new automatic admissions. It
    /// does not cancel accepted guards/turns or certify any Native cleanup.
    pub fn withdraw(&self) {
        self.gate.0.withdrawn.store(true, Ordering::Release);
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.withdraw();
    }
}
impl Gate {
    /// None defers this automatic attempt. Keep Some's real guard through the
    /// existing actual read/submit permit; do not merely test a bool then drop.
    pub async fn admit(&self) -> Option<Admission> {
        if self.0.withdrawn.load(Ordering::Acquire)
            || self.0.admission_closed.load(Ordering::Acquire)
        {
            return None;
        }
        // Pending writers inhibit new work immediately. Waiting behind the
        // writer could deadlock a nested/serialized automatic read window.
        let guard = self.0.state.clone().try_read_owned().ok()?;
        if guard.held
            || !guard.inhibitors.is_empty()
            || self.0.withdrawn.load(Ordering::Acquire)
            || self.0.admission_closed.load(Ordering::Acquire)
        {
            None
        } else {
            Some(Admission { _guard: guard })
        }
    }
}
// Dropping Hold does not reopen admission. Unknown/lost observers retain the
// inhibitor until the owning operation observes a real continuation or cleanup.

#[cfg(test)]
#[path = "retained_goal_gate_tests.rs"]
mod tests;

/// Actual original goal semaphore plus retained read-window custody.
pub struct AutomaticPermit<'a> {
    _goal: Option<tokio::sync::SemaphorePermit<'a>>,
    _retained: Option<Admission>,
}
impl<'a> AutomaticPermit<'a> {
    pub fn new(goal: tokio::sync::SemaphorePermit<'a>, retained: Option<Admission>) -> Self {
        Self {
            _goal: Some(goal),
            _retained: retained,
        }
    }
    pub fn retained_only(retained: Admission) -> Self {
        Self {
            _goal: None,
            _retained: Some(retained),
        }
    }
}
#[derive(Default)]
pub struct Registration(std::sync::Mutex<RegistrationState>);
#[derive(Default)]
enum RegistrationState {
    #[default]
    Unused,
    Legacy,
    Retained(Gate),
}
impl Registration {
    pub fn bind(&self, gate: Gate) -> Result<(), String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "retained manager gate unavailable")?;
        match &*state {
            RegistrationState::Unused => {
                *state = RegistrationState::Retained(gate);
                Ok(())
            }
            RegistrationState::Retained(existing) if Arc::ptr_eq(&existing.0, &gate.0) => Ok(()),
            _ => Err("retained gate must bind before the first real thread start".into()),
        }
    }
    /// The common real Core start seals the choice BEFORE any Core is built.
    /// Installing after an in-flight unregistered root is therefore refused.
    pub fn enter(&self) -> Result<Option<Gate>, String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "retained manager gate unavailable")?;
        match &*state {
            RegistrationState::Unused => {
                *state = RegistrationState::Legacy;
                Ok(None)
            }
            RegistrationState::Legacy => Ok(None),
            RegistrationState::Retained(gate) => Ok(Some(gate.clone())),
        }
    }
}
impl Gate {
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[cfg(test)]
#[path = "retained_goal_inhibitor_tests.rs"]
mod inhibitor_tests;

#[cfg(test)]
#[path = "retained_goal_shutdown_tests.rs"]
mod shutdown_tests;
