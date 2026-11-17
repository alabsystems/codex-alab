// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The configured runtime owns closed custody before any frontend/UI RPC.
//! Only actual private accepted H1 may move the SAME initially closed Hold.
//! Empty is an unchanged UI-selected slot, never proof that no Core exists.
use super::*;
use codex_alab_aterm::frontend_control::retained_service::ClosedCustody;
use std::io;

pub(super) struct Keeper {
    empty: EmptyPrimary,
    custody: StdMutex<Option<ClosedCustody>>,
    completion: Arc<ConfiguredCompletion>,
}
impl Keeper {
    pub(super) fn unobserved(&self) -> bool {
        self.completion.result.borrow().is_none()
            || self.completion.uncertain.load(Ordering::Acquire)
    }
}

/// Restore the SAME physical-bind resource if polling or its destruction is
/// canceled/panics. Missing/unknown custody never creates a second Hold.
struct CustodyLease<'a> {
    slot: &'a StdMutex<Option<ClosedCustody>>,
    actual: Option<ClosedCustody>,
}
impl<'a> CustodyLease<'a> {
    fn take(slot: &'a StdMutex<Option<ClosedCustody>>) -> io::Result<Self> {
        let actual = slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or_else(|| io::Error::other("configured closed custody already borrowed"))?;
        Ok(Self {
            slot,
            actual: Some(actual),
        })
    }
    fn actual(&self) -> io::Result<&ClosedCustody> {
        self.actual
            .as_ref()
            .ok_or_else(|| io::Error::other("configured closed custody unavailable"))
    }
}
impl Drop for CustodyLease<'_> {
    fn drop(&mut self) {
        if let Some(actual) = self.actual.take() {
            *self.slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(actual);
        }
    }
}

impl Binding {
    /// Called once by actual MessageProcessor installation, after the SAME
    /// InitialSessionOperation but before task-host/transport/root dispatch.
    /// The ordinary common prepared cleanup retains this owner on any error.
    pub(crate) fn start_configured_custody(self: &Arc<Self>) -> io::Result<()> {
        self.open()
            .map_err(|_| io::Error::other("configured custody owner closed"))?;
        let empty = configured_empty(&self.processor, &self.input)
            .map_err(|_| io::Error::other("actual configured empty-primary owner unavailable"))?;
        let actual = self.backend.initial_closed_custody()?;
        let keeper = Arc::new(Keeper {
            empty,
            custody: StdMutex::new(Some(actual)),
            completion: ConfiguredCompletion::new(),
        });
        let guard = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed || state.configured.custody.is_some() {
                return Err(io::Error::other(
                    "configured closed custody already installed or closed",
                ));
            }
            let guard = ConfiguredGuard::new(self, keeper.clone(), keeper.completion.clone());
            state.configured.custody = Some(keeper.clone());
            guard
        };
        let binding = self.clone();
        // Preguard + actual keeper are registered BEFORE spawn. Frontend/RPC
        // observer loss cannot abort this original backend-owned operation.
        tokio::spawn(async move {
            let observed = observe_configured(run(&binding, &keeper)).await;
            let result = match observed {
                Ok(Ok(())) => Ok(()),
                // Joined owner shutdown may interrupt a wait/transfer; it
                // asserts no transfer, UI success, no-effects or release.
                Ok(Err(_))
                    if binding.input.is_stopped()
                        || keeper.empty.source_stopped()
                        || binding
                            .state
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .closed =>
                {
                    Ok(())
                }
                Ok(Err(error)) => {
                    binding.input.fail(io::Error::other(
                        "configured closed custody outcome remains unobserved",
                    ));
                    keeper.completion.uncertain.store(true, Ordering::Release);
                    Err(native_error(error))
                }
                Err(()) => {
                    binding.input.fail(io::Error::other(
                        "configured closed custody worker was lost",
                    ));
                    keeper.completion.uncertain.store(true, Ordering::Release);
                    Err(internal_error("configured closed custody worker was lost"))
                }
            };
            keeper.completion.result.send_replace(Some(result));
            guard.finish();
        });
        Ok(())
    }
}

async fn run(binding: &Binding, keeper: &Keeper) -> io::Result<()> {
    if !keeper.empty.belongs_to(&binding.input) {
        return Err(io::Error::other(
            "configured custody belongs to another actual input",
        ));
    }
    let mut custody = CustodyLease::take(&keeper.custody)?;
    loop {
        // Do not race a temporary installing slot before acquiring the SDK
        // mutation lease. Only the exact EmptyGuard inside transfer_to may
        // authorize the synchronous same-Hold move.
        let pending = tokio::select! {
            biased;
            () = binding.input.withdrawn() => return Ok(()),
            actual = custody.actual()?.pending_hold() => actual,
        };
        let pending = match pending {
            Ok(actual) => actual,
            Err(error) => {
                return observe_selected_handoff(binding, &keeper.empty)
                    .await
                    .map_err(|_| error);
            }
        };
        // SDK mutation lease/state FIRST, then the real Source EmptyGuard.
        // The non-Send guard is created only after the final await and held
        // through the SDK synchronous same-Arc move. Every Source ticket stays.
        let transferred = custody
            .actual()?
            .transfer_to(&pending, || {
                keeper.empty.lock_unchanged().map_err(|_| {
                    io::Error::other("actual configured empty-primary selection changed")
                })
            })
            .await;
        let transferred = match transferred {
            Ok(actual) => actual,
            Err(error) => {
                return observe_selected_handoff(binding, &keeper.empty)
                    .await
                    .map_err(|_| error);
            }
        };
        let retired = custody.actual.replace(transferred);
        drop(retired);
        // The next iteration waits for THIS actual H1's original owned ACK
        // before considering a later accepted H1. No old UI receipt is used.
    }
}

/// Only real Source installation completion can end the initial keeper after
/// SDK Hold retirement. A matching in-progress transition is observed, not
/// treated as selected and not failed because SDK notified before Source finish.
/// Stop ends this observer only; it claims no Native cleanup or selection.
async fn observe_selected_handoff(binding: &Binding, empty: &EmptyPrimary) -> io::Result<()> {
    let selected = tokio::select! {
        biased;
        () = binding.input.withdrawn() => return Ok(()),
        () = binding.stopped() => return Ok(()),
        actual = empty.wait_selected_handoff() => actual.map_err(|_| io::Error::other("actual Source handoff remains unobserved"))?,
    };
    let super::super::super::alab_initial_session::selected_primary::SelectionHandoff::Selected(
        selected,
    ) = selected
    else {
        return Ok(());
    };
    if !selected.belongs_to(&binding.input) {
        return Err(io::Error::other(
            "actual selected handoff belongs to another input",
        ));
    }
    Ok(())
}
