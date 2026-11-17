// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Source admission closure is distinct from actual Native/IO withdrawal.
//! Existing exact cancellation retains every original live()/Native check.
use super::*;
use codex_alab_core::retained_goal_gate::ShutdownBarrier;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// SAME physical-bind owner and monotonically closed Source scheduling gate.
/// No new Hold, Source ticket, Native reservation, Go or role is issued.
#[derive(Clone)]
pub struct SourceShutdown {
    owner: Arc<GoalOwner>,
    barrier: ShutdownBarrier,
}
impl SourceShutdown {
    /// Always drain old automatic windows, even if actual control is already
    /// unavailable. The writer is RELEASED before caller-owned worker/Native
    /// cancellation cleanup. Closure remains sticky through every outcome.
    pub async fn drain(&self) -> io::Result<()> {
        let before = self.owner.live();
        self.barrier.drain().await;
        before?;
        self.owner.live()?;
        if self.owner.failed.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        Ok(())
    }
}
impl GoalOwner {
    pub(in crate::frontend_control) fn begin_source_shutdown(self: &Arc<Self>) -> SourceShutdown {
        let barrier = close_source_records(
            &self.owner,
            &self.source_closing,
            &self.tasks,
            &self.state,
            &self.failed,
            &self.changed,
        );
        SourceShutdown {
            owner: self.clone(),
            barrier,
        }
    }
    /// New Source production only. The original live() remains unchanged and
    /// is still required by every existing exact registration/retirement IO.
    pub(super) fn admitting_source(&self) -> io::Result<()> {
        self.live()?;
        if self.source_closing.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        Ok(())
    }
    /// End only the Source observer wait, retaining original Native/control
    /// liveness and every actual H1/Source worker and closed ledger resource.
    pub(super) async fn source_change(&self, changed: &mut watch::Receiver<u64>) -> io::Result<()> {
        self.live()?;
        wait_source_change(&self.source_closing, changed).await?;
        self.live()
    }
}

/// Shared by production and private scheduling/custody controls. No public
/// Native/SDK receipt or actual controller can be constructed by this helper.
fn close_source_records(
    owner: &Owner,
    closing: &AtomicBool,
    tasks: &Mutex<Vec<tokio::task::JoinHandle<()>>>,
    state: &Mutex<State>,
    failed: &AtomicBool,
    changed: &watch::Sender<u64>,
) -> ShutdownBarrier {
    // Deny fresh automatic windows before a lock wait. This is NOT terminal
    // Owner.withdraw: existing Hold/Inhibitor retirement must still take the
    // SAME writer after the returned barrier's short drain has completed.
    let barrier = owner.begin_shutdown();
    closing.store(true, Ordering::Release);
    // SAME order as actual spawn/record acceptance. Close has no success or
    // permission mutation, so poison recovery can only retain a failed deny.
    let tasks = tasks.lock().unwrap_or_else(|poison| {
        failed.store(true, Ordering::Release);
        poison.into_inner()
    });
    let state = state.lock().unwrap_or_else(|poison| {
        failed.store(true, Ordering::Release);
        poison.into_inner()
    });
    drop(state);
    drop(tasks);
    changed.send_modify(|value| *value = value.saturating_add(1));
    barrier
}

async fn wait_source_change(
    closing: &AtomicBool,
    changed: &mut watch::Receiver<u64>,
) -> io::Result<()> {
    if closing.load(Ordering::Acquire) {
        return Err(unavailable());
    }
    changed.changed().await.map_err(|_| unavailable())?;
    if closing.load(Ordering::Acquire) {
        return Err(unavailable());
    }
    Ok(())
}

#[cfg(test)]
#[path = "frontend_retained_source_shutdown_tests.rs"]
mod tests;
