// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual backend ownership inhibits only ordinary idle unloading of this exact
//! explicitly selected Core. No subscriber, transport, Native permission or Go
//! is manufactured. Other cores and task children retain original unload policy.
use super::*;
use std::sync::Weak;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

struct Slot {
    actual: Weak<CodexThread>,
    owners: AtomicUsize,
    changed: watch::Sender<()>,
}
impl Slot {
    fn new(actual: &Arc<CodexThread>) -> Self {
        Self {
            actual: Arc::downgrade(actual),
            owners: AtomicUsize::new(0),
            changed: watch::channel(()).0,
        }
    }
    fn is_for(&self, actual: &Arc<CodexThread>) -> bool {
        self.actual
            .upgrade()
            .is_some_and(|held| Arc::ptr_eq(&held, actual))
    }
    fn held(&self) -> bool {
        self.owners.load(Ordering::Acquire) != 0
            && self
                .actual
                .upgrade()
                .is_some_and(|actual| !actual.has_shutdown_status())
    }
}
fn slot(actual: &Arc<CodexThread>) -> Result<Arc<Slot>, JSONRPCErrorError> {
    let data = actual.thread_extension_data();
    data.insert_if(Slot::new(actual), |existing| existing.is_none());
    let slot = data
        .get::<Slot>()
        .ok_or_else(|| internal_error("primary lifetime slot unavailable"))?;
    if !slot.is_for(actual) {
        return Err(invalid_request(
            "primary lifetime belongs to another runtime",
        ));
    }
    Ok(slot)
}

#[cfg(unix)]
struct Token {
    // Strong custody is real Core ownership. Slot holds only Weak back to Core,
    // and Core never owns Token, so this introduces no owner/Core self-cycle.
    actual: Arc<CodexThread>,
    slot: Arc<Slot>,
}
#[cfg(unix)]
impl Drop for Token {
    fn drop(&mut self) {
        self.slot.owners.fetch_sub(1, Ordering::AcqRel);
        // Notification only; eligibility ALWAYS reads the atomic owner count.
        // Concurrent acquire/drop cannot publish a stale boolean as authority.
        self.slot.changed.send_replace(());
    }
}
#[cfg(unix)]
#[derive(Clone)]
pub(crate) struct PrimaryLease(Arc<Token>);
#[cfg(unix)]
impl PrimaryLease {
    pub(crate) fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub(crate) fn is_for(&self, actual: &Arc<CodexThread>) -> bool {
        Arc::ptr_eq(&self.0.actual, actual)
    }
    pub(crate) fn downgrade(&self) -> WeakPrimaryLease {
        WeakPrimaryLease(Arc::downgrade(&self.0))
    }

    /// Reuse the common listener's actual goal/pending/Current serialization.
    /// An already committed unload wins; no replacement is selected by ID.
    pub(in crate::request_processors) async fn acquire(
        context: &ListenerTaskContext,
        id: ThreadId,
        actual: Arc<CodexThread>,
        input: &codex_alab_core::native_incoming::Incoming,
    ) -> Result<Self, JSONRPCErrorError> {
        let owner = Candidate {
            actual: actual.clone(),
            input: input.clone(),
            token: std::sync::Mutex::new(None),
        };
        owner.open()?;
        let state = context
            .thread_state_manager
            .created_listener_state(id)
            .await;
        match start_listener(
            context.clone(),
            id,
            actual,
            state,
            ListenerStartup::Primary(&owner),
        )
        .await
        {
            ListenerStartOutcome::PrimaryAcquired => owner
                .token
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .ok_or_else(|| internal_error("primary lifetime was not installed")),
            ListenerStartOutcome::Started { .. }
            | ListenerStartOutcome::Stale
            | ListenerStartOutcome::Rejected => Err(thread_closing_for_resume(id)),
        }
    }
}

/// Identity-only weak marker. It retains no Core, count, recorder, subscriber or
/// transport. Upgrading succeeds only while an actual lease owner still lives.
#[cfg(unix)]
#[derive(Clone)]
pub(crate) struct WeakPrimaryLease(Weak<Token>);
#[cfg(unix)]
impl WeakPrimaryLease {
    pub(crate) fn upgrade(&self) -> Option<PrimaryLease> {
        self.0.upgrade().map(PrimaryLease)
    }
}

/// Exists only during this actual serialized listener acquisition. It cannot
/// be deserialized, selected by labels, or used as Native authority.
#[cfg(unix)]
pub(super) struct Candidate {
    actual: Arc<CodexThread>,
    input: codex_alab_core::native_incoming::Incoming,
    token: std::sync::Mutex<Option<PrimaryLease>>,
}
#[cfg(unix)]
impl Candidate {
    fn open(&self) -> Result<(), JSONRPCErrorError> {
        use futures::FutureExt;
        if self.input.is_stopped() || self.input.withdrawn().now_or_never().is_some() {
            Err(invalid_request("primary runtime owner withdrew"))
        } else {
            Ok(())
        }
    }
    /// Called ONLY in common start_listener after its actual goal-resume lock,
    /// pending-unload and exact Current checks. No new lock is held for lease
    /// lifetime; no alternate event reader/subscription/approval owner is made.
    pub(super) fn install_serialized(
        &self,
        actual: &Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        self.open()?;
        if !Arc::ptr_eq(&self.actual, actual) || actual.has_shutdown_status() {
            return Err(invalid_request("primary lifetime actual runtime withdrew"));
        }
        let mut token = self
            .token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if token.is_some() {
            return Ok(());
        }
        let slot = slot(actual)?;
        slot.owners
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                count.checked_add(1)
            })
            .map_err(|_| internal_error("primary lifetime owner capacity exhausted"))?;
        *token = Some(PrimaryLease(Arc::new(Token {
            actual: actual.clone(),
            slot: slot.clone(),
        })));
        slot.changed.send_replace(());
        Ok(())
    }
}

pub(super) struct LifetimeWatch {
    slot: Arc<Slot>,
    changed: watch::Receiver<()>,
}
impl LifetimeWatch {
    pub(super) fn held(&self) -> bool {
        self.slot.held()
    }
    pub(super) async fn changed(&mut self) {
        let _ = self.changed.changed().await;
    }
}
pub(super) fn watch(actual: &Arc<CodexThread>) -> Result<LifetimeWatch, JSONRPCErrorError> {
    let slot = slot(actual)?;
    // Subscribe before the first eligibility read. Sender remains with this
    // exact Core slot; Drop/acquisition only wakes, never claims completion.
    let changed = slot.changed.subscribe();
    Ok(LifetimeWatch { slot, changed })
}
