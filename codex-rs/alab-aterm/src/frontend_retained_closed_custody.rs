// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Continuously CLOSED Source custody on actual private accepted H1 records.
//! No old UI completion is invented. Native still independently owns whether
//! an H1 may be issued, its ACK, input seal, Go, UI and constructor admission.
use super::*;

enum Subject {
    Initial,
    Replacement {
        operation: [u8; 32],
        actual: Arc<AcceptedHold>,
    },
}
/// Same actual scheduling Hold/epoch, never a Native receipt or release grant.
/// Construction is restricted to the physical-bind GoalOwner's real ledger.
pub struct ClosedCustody {
    owner: Arc<GoalOwner>,
    subject: Subject,
    hold: Arc<Hold>,
}
impl std::fmt::Debug for ClosedCustody {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("ClosedCustody(<same actual closed Source Hold; no Go/UI/release>)")
    }
}
impl GoalOwner {
    pub(in crate::frontend_control) fn initial_closed_custody(
        self: &Arc<Self>,
    ) -> io::Result<ClosedCustody> {
        self.admitting_source()?;
        let state = self.state.lock().map_err(|_| unavailable())?;
        if state.initial_transferred {
            return Err(unavailable());
        }
        let hold = state.initial.clone().ok_or_else(unavailable)?;
        Ok(ClosedCustody {
            owner: self.clone(),
            subject: Subject::Initial,
            hold,
        })
    }
}
impl ClosedCustody {
    /// The Source caller retains the SAME actual configured Empty/version.
    /// Pending/captured Core work may exist; neither this resource nor Empty
    /// says otherwise. No Source scheduling admission is opened while waiting.
    pub async fn pending_hold(&self) -> io::Result<PendingHold> {
        let mut changed = self.owner.changed.subscribe();
        let mut phase = match &self.subject {
            Subject::Initial => None,
            Subject::Replacement { actual, .. } => Some(actual.phase.subscribe()),
        };
        loop {
            self.owner.admitting_source()?;
            let ready = {
                let state = self.owner.state.lock().map_err(|_| unavailable())?;
                subject_matches(&state, &self.subject, &self.hold)?;
                let acknowledged = match &self.subject {
                    Subject::Initial => true,
                    Subject::Replacement { actual, .. } => {
                        *actual.phase.borrow() == Phase::Acknowledged
                    }
                };
                if acknowledged {
                    state
                        .replacements
                        .iter()
                        .filter(|(_, record)| *record.phase.borrow() == Phase::Accepted)
                        .min_by_key(|(_, record)| record.order)
                        .map(|(operation, actual)| PendingHold {
                            owner: self.owner.clone(),
                            operation: *operation,
                            actual: actual.clone(),
                        })
                } else {
                    None
                }
            };
            if let Some(pending) = ready {
                return Ok(pending);
            }
            tokio::select! {
                biased;
                _ = self.owner.withdrawn() => return Err(unavailable()),
                result = self.owner.source_change(&mut changed) => result?,
                result = async {
                    match &mut phase {
                        Some(phase) => phase.changed().await.map_err(|_| unavailable()),
                        None => std::future::pending::<io::Result<()>>().await,
                    }
                } => result?,
            }
        }
    }

    /// Acquire the SDK mutation lease BEFORE acquiring the real Source slot
    /// guard in `unchanged_empty`. The callback's G spans this synchronous
    /// ledger mutation only; it cannot cross an await or grant Native use.
    /// Actual H1's independently owned worker still performs its physical ACK.
    pub async fn transfer_to<G>(
        &self,
        pending: &PendingHold,
        unchanged_empty: impl FnOnce() -> io::Result<G>,
    ) -> io::Result<ClosedCustody> {
        self.owner.admitting_source()?;
        if !Arc::ptr_eq(&self.owner, &pending.owner) {
            return Err(unavailable());
        }
        let request = ClosedMove {
            subject: &self.subject,
            hold: &self.hold,
            operation: &pending.operation,
            target: &pending.actual,
        };
        transfer_closed(
            &self.owner.mutation,
            &self.owner.state,
            request,
            unchanged_empty,
            || self.owner.admitting_source(),
        )
        .await?;
        self.owner.notify();
        self.owner.live()?;
        Ok(ClosedCustody {
            owner: self.owner.clone(),
            hold: self.hold.clone(),
            subject: Subject::Replacement {
                operation: pending.operation,
                actual: pending.actual.clone(),
            },
        })
    }
}

fn subject_matches(state: &State, subject: &Subject, hold: &Arc<Hold>) -> io::Result<()> {
    let matches = match subject {
        Subject::Initial => {
            !state.initial_transferred
                && state
                    .initial
                    .as_ref()
                    .is_some_and(|actual| Arc::ptr_eq(actual, hold))
        }
        Subject::Replacement { operation, actual } => {
            state
                .replacements
                .get(operation)
                .is_some_and(|stored| Arc::ptr_eq(stored, actual))
                && !actual
                    .transferred
                    .load(std::sync::atomic::Ordering::Acquire)
                && actual
                    .hold
                    .lock()
                    .map_err(|_| unavailable())?
                    .as_ref()
                    .is_some_and(|stored| Arc::ptr_eq(stored, hold))
        }
    };
    if matches { Ok(()) } else { Err(unavailable()) }
}
struct ClosedMove<'a> {
    subject: &'a Subject,
    hold: &'a Arc<Hold>,
    operation: &'a [u8; 32],
    target: &'a Arc<AcceptedHold>,
}
/// Shared by the real public operation and actual overlap/cancellation
/// controls. No Native receipt or Source slot can be constructed by this helper.
async fn transfer_closed<G>(
    mutation: &tokio::sync::Semaphore,
    state: &Mutex<State>,
    request: ClosedMove<'_>,
    unchanged_empty: impl FnOnce() -> io::Result<G>,
    live: impl Fn() -> io::Result<()>,
) -> io::Result<()> {
    let _mutation = mutation.acquire().await.map_err(|_| unavailable())?;
    live()?;
    let mut state = state.lock().map_err(|_| unavailable())?;
    move_closed_hold(&mut state, request, unchanged_empty, live)
}

/// This private function owns no Native constructor or public receipt. Tests
/// may exercise its scheduling/ledger facts, never a successful SDK conjunction.
fn move_closed_hold<G>(
    state: &mut State,
    request: ClosedMove<'_>,
    unchanged_empty: impl FnOnce() -> io::Result<G>,
    live: impl Fn() -> io::Result<()>,
) -> io::Result<()> {
    subject_matches(state, request.subject, request.hold)?;
    let target = state
        .replacements
        .get(request.operation)
        .ok_or_else(unavailable)?;
    if !Arc::ptr_eq(target, request.target)
        || *target.phase.borrow() != Phase::Accepted
        || tokio::time::Instant::now() >= target.deadline
    {
        return Err(unavailable());
    }
    let previous = match request.subject {
        Subject::Initial => None,
        Subject::Replacement { operation, actual } => {
            if operation == request.operation
                || actual.order >= target.order
                || *actual.phase.borrow() != Phase::Acknowledged
            {
                return Err(unavailable());
            }
            Some(actual.clone())
        }
    };
    if state.replacements.values().any(|other| {
        other.order < target.order
            && previous
                .as_ref()
                .is_none_or(|actual| !Arc::ptr_eq(actual, other))
            && (*other.phase.borrow() != Phase::Acknowledged
                || other.hold.lock().map_or(true, |hold| hold.is_some()))
    }) {
        return Err(unavailable());
    }
    let mut source = match previous.as_ref() {
        Some(previous) => Some(previous.hold.lock().map_err(|_| unavailable())?),
        None => None,
    };
    let mut destination = target.hold.lock().map_err(|_| unavailable())?;
    if destination.is_some() {
        return Err(unavailable());
    }
    // Fallible runtime-resource locks precede the Source callback and mutation.
    let records = state
        .runtimes
        .values()
        .filter(|record| match request.subject {
            Subject::Initial => record.go.is_none(),
            Subject::Replacement { operation, .. } => record
                .go
                .as_deref()
                .is_some_and(|go| go.as_bytes() == operation),
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut phases = records
        .iter()
        .map(|record| record.phase.lock().map_err(|_| unavailable()))
        .collect::<io::Result<Vec<_>>>()?;
    live()?;
    let empty = unchanged_empty()?;
    // The callback may block on an actual slot mutex. Recheck deadline/loss at
    // the real mutation point, not merely before the Source check began.
    live()?;
    if tokio::time::Instant::now() >= target.deadline {
        return Err(unavailable());
    }
    if !target.phase.send_if_modified(|phase| {
        if *phase != Phase::Accepted {
            return false;
        }
        *phase = Phase::Held;
        true
    }) {
        return Err(unavailable());
    }
    // No fallible operation or await from the first ownership mutation through
    // the complete same-Arc move/old-resource fencing. No ticket is touched.
    match request.subject {
        Subject::Initial => {
            state.initial.take();
            state.initial_transferred = true;
        }
        Subject::Replacement { actual, .. } => {
            if let Some(source) = &mut source {
                source.take();
            }
            actual
                .transferred
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
    *destination = Some(request.hold.clone());
    for phase in &mut phases {
        if **phase != RuntimePhase::Unknown {
            **phase = RuntimePhase::Transferred;
        }
    }
    drop(empty);
    Ok(())
}

#[cfg(test)]
#[path = "frontend_retained_closed_custody_tests.rs"]
mod tests;
