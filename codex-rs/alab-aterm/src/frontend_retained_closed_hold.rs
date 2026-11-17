// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Same actual continuously closed Hold transfer; no Native grant.
use super::*;

/// Exact accepted H1 on this same inherited backend channel. This resource is
/// created only from the private receive table, never from an operation label.
#[derive(Clone)]
pub struct PendingHold {
    owner: Arc<GoalOwner>,
    operation: [u8; 32],
    actual: Arc<AcceptedHold>,
}
impl std::fmt::Debug for PendingHold {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("PendingHold(<same actual H1; no ACK or grant>)")
    }
}

impl RuntimeAck {
    /// Wait for the earliest actually accepted H1. Losing this observation
    /// neither cancels its independently owned worker nor creates another Hold.
    /// The configured-runtime owner retains this waiter through client loss.
    pub async fn pending_hold(&self) -> io::Result<PendingHold> {
        let mut changed = self.frontier.owner.changed.subscribe();
        loop {
            self.frontier.owner.live()?;
            let pending = {
                let state = self
                    .frontier
                    .owner
                    .state
                    .lock()
                    .map_err(|_| unavailable())?;
                state
                    .replacements
                    .iter()
                    .filter(|(_, record)| *record.phase.borrow() == Phase::Accepted)
                    .min_by_key(|(_, record)| record.order)
                    .map(|(operation, actual)| PendingHold {
                        owner: self.frontier.owner.clone(),
                        operation: *operation,
                        actual: actual.clone(),
                    })
            };
            if let Some(actual) = pending {
                return Ok(actual);
            }
            tokio::select! {
                biased;
                _ = self.frontier.owner.withdrawn() => return Err(unavailable()),
                result = changed.changed() => result.map_err(|_| unavailable())?,
            }
        }
    }
    /// A genuine final Native commit for the configured NoPrimary UI may keep
    /// automatic work held across a subsequent replacement. The actual Source
    /// empty-slot guard is acquired INSIDE this local callback while the SDK
    /// state is locked, and is retained only through this synchronous mutation.
    /// This grants no Native/SDK role, sends no ACK, and opens no Goal window.
    /// Its caller owns the NoPrimary policy and must not invoke release instead.
    pub async fn transfer_closed_hold<G>(
        &self,
        committed: &RuntimeCommit,
        pending: &PendingHold,
        unchanged_empty: impl FnOnce() -> io::Result<G>,
    ) -> io::Result<()> {
        // Acquire before any std SDK/Source slot guard. G is constructed only
        // after this final await and never survives a suspension point.
        let _mutation = self
            .frontier
            .owner
            .mutation
            .acquire()
            .await
            .map_err(|_| unavailable())?;
        self.frontier.owner.live()?;
        if !committed.same_frontier(&self.frontier)
            || !Arc::ptr_eq(&pending.owner, &self.frontier.owner)
        {
            return Err(unavailable());
        }
        let mut state = self
            .frontier
            .owner
            .state
            .lock()
            .map_err(|_| unavailable())?;
        let target = state
            .replacements
            .get(&pending.operation)
            .cloned()
            .ok_or_else(unavailable)?;
        if !Arc::ptr_eq(&target, &pending.actual)
            || *target.phase.borrow() != Phase::Accepted
            || state.replacements.values().any(|other| {
                other.order < target.order && *other.phase.borrow() != Phase::Acknowledged
            })
        {
            return Err(unavailable());
        }
        let mut destination = target.hold.lock().map_err(|_| unavailable())?;
        if destination.is_some() {
            return Err(unavailable());
        }
        let mut phase = self
            .frontier
            .record
            .phase
            .lock()
            .map_err(|_| unavailable())?;
        let stored = self
            .frontier
            .record
            .commit
            .lock()
            .map_err(|_| unavailable())?;
        let observed = stored.as_ref().ok_or_else(unavailable)?;
        if *phase != RuntimePhase::Committed
            || observed.activation.observation != committed.activation.observation
            || observed.app_owner != committed.app_owner
        {
            return Err(unavailable());
        }
        drop(stored);
        // Acquire every fallible lock and validate the exact old owned Hold
        // before changing either record. Old receipts cannot name the new H1.
        let empty = match &self.frontier.record.go {
            None => {
                if state.initial_transferred || state.initial.is_none() {
                    return Err(unavailable());
                }
                let empty = unchanged_empty()?;
                *destination = state.initial.take();
                state.initial_transferred = true;
                empty
            }
            Some(go) => {
                let operation: &[u8; 32] = go.as_bytes().try_into().map_err(|_| unavailable())?;
                if operation == &pending.operation {
                    return Err(unavailable());
                }
                let previous = state
                    .replacements
                    .get(operation)
                    .cloned()
                    .ok_or_else(unavailable)?;
                if *previous.phase.borrow() != Phase::Acknowledged
                    || previous
                        .transferred
                        .load(std::sync::atomic::Ordering::Acquire)
                {
                    return Err(unavailable());
                }
                let mut source = previous.hold.lock().map_err(|_| unavailable())?;
                if source.is_none() {
                    return Err(unavailable());
                }
                let empty = unchanged_empty()?;
                *destination = source.take();
                previous
                    .transferred
                    .store(true, std::sync::atomic::Ordering::Release);
                empty
            }
        };
        *phase = RuntimePhase::Transferred;
        target.phase.send_replace(Phase::Held);
        drop(empty);
        drop(phase);
        drop(destination);
        drop(state);
        self.frontier.owner.notify();
        Ok(())
    }
}

#[path = "frontend_retained_closed_custody.rs"]
mod custody;
pub use custody::ClosedCustody;
