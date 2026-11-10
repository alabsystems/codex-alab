// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Session-owned configuration work. Observer cancellation does not cancel a
//! job; explicit shutdown aborts and joins every accepted job before Stopped.
use std::future::Future;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::Notify;
use tokio::task::JoinSet;

#[derive(Default)]
struct State {
    closed: bool,
    joining: bool,
    joined: bool,
    jobs: JoinSet<()>,
}
#[derive(Default)]
pub(super) struct Jobs {
    state: Mutex<State>,
    changed: Notify,
}
fn unavailable() -> io::Error {
    io::Error::other("session configuration work unavailable")
}
impl Jobs {
    pub(super) fn closed(&self) -> bool {
        self.state.lock().map_or(true, |state| state.closed)
    }
    pub(super) fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.closed = true;
            state.jobs.abort_all();
        }
    }
    pub(super) fn spawn(
        &self,
        future: impl Future<Output = ()> + Send + 'static,
    ) -> io::Result<()> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        if state.closed {
            return Err(unavailable());
        }
        while state.jobs.try_join_next().is_some() {}
        state.jobs.spawn(future);
        Ok(())
    }
    pub(super) async fn shutdown(self: &Arc<Self>) -> io::Result<()> {
        let jobs = {
            let mut state = self.state.lock().map_err(|_| unavailable())?;
            state.closed = true;
            if state.joining {
                None
            } else {
                state.joining = true;
                state.jobs.abort_all();
                Some(std::mem::take(&mut state.jobs))
            }
        };
        if let Some(mut jobs) = jobs {
            // Retain the join owner if a shutdown observer itself is cancelled.
            // Repeated shutdown calls wait for THIS same drain, never an empty set.
            let owner = Arc::clone(self);
            tokio::spawn(async move {
                while jobs.join_next().await.is_some() {}
                if let Ok(mut state) = owner.state.lock() {
                    state.joined = true;
                }
                owner.changed.notify_waiters();
            });
        }
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.state.lock().map_err(|_| unavailable())?.joined {
                return Ok(());
            }
            changed.await;
        }
    }
}
impl Drop for Jobs {
    fn drop(&mut self) {
        // Fallback only: an abort request/Drop is not an acknowledgment of join.
        self.state
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .jobs
            .abort_all();
    }
}
