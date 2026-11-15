// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Retain an admitted future when its waiter is abandoned. The original future
//! remains responsible for cancellation, joining its children and publishing
//! its outcome; this module supplies no replacement outcome or callback.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;
use tokio::runtime::Handle;
use tokio::sync::Notify;

#[derive(Debug, Default)]
struct Counter {
    closed: Mutex<bool>,
    // Only actual leases clone this token. Owner/tracker clones share Counter
    // instead, so one remaining strong reference means all work has settled.
    registrations: Arc<()>,
    changed: Notify,
}

impl Counter {
    fn acquire(self: &Arc<Self>) -> Lease {
        Lease {
            counter: Arc::clone(self),
            registration: Some(Arc::clone(&self.registrations)),
        }
    }

    async fn wait(&self) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            // Register before checking so the final release cannot be missed.
            changed.as_mut().enable();
            if Arc::strong_count(&self.registrations) == 1 {
                return;
            }
            changed.await;
        }
    }
}

/// Admission and completion barrier for every future an owner starts. Register
/// before starting any eager work, close admission before shutdown, and keep
/// the runtime alive until `wait` returns.
#[derive(Clone, Debug, Default)]
pub struct Owner(Arc<Counter>);

impl Owner {
    pub fn register(&self) -> Result<Lease, Closed> {
        let closed = self
            .0
            .closed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *closed {
            return Err(Closed);
        }
        // Retain registration while admission is locked, so close cannot pass
        // an admitted future whose lease has not yet been acquired.
        let lease = self.0.acquire();
        drop(closed);
        Ok(lease)
    }

    /// Close synchronously, including if a subsequent wait is cancelled.
    pub fn close(&self) {
        *self
            .0
            .closed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
    }

    /// Without `close`, this observes a drain but does not exclude new work.
    pub async fn wait(&self) {
        self.0.wait().await;
    }

    pub async fn close_and_wait(&self) {
        self.close();
        self.wait().await;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Closed;

impl fmt::Display for Closed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("future owner is closed to new work")
    }
}

impl std::error::Error for Closed {}

/// One admitted future. Moving it into retained work never releases admission
/// ownership; completion or destruction releases it exactly once.
#[derive(Debug)]
pub struct Lease {
    counter: Arc<Counter>,
    registration: Option<Arc<()>>,
}

impl Drop for Lease {
    fn drop(&mut self) {
        // Release first: a woken waiter must be able to observe the final
        // reference disappearing, including when it registers concurrently.
        drop(self.registration.take());
        self.counter.changed.notify_waiters();
    }
}

/// A separate barrier for abandoned transfers only. Wait after the producer
/// task has joined, so it cannot still create another transfer. Normal pending
/// waiters are deliberately absent from this count.
#[derive(Clone, Debug, Default)]
pub struct Abandonments(Arc<Counter>);

impl Abandonments {
    pub async fn wait(&self) {
        self.0.wait().await;
    }
}

/// Register with the owner before creating eager work. `on_abandon` must cancel
/// only that work's own cancellation domain and must not panic. No blocking or
/// waiting happens in Drop. A captured runtime handle permits abandonment on a
/// thread outside the runtime; the owner must keep that runtime alive.
pub fn retain<F, C>(work: F, lease: Lease, abandonments: Abandonments, on_abandon: C) -> Retained<F>
where
    F: Future + Send + 'static,
    C: FnOnce() + Send + 'static,
{
    Retained {
        pending: Some(Pending {
            work: Box::pin(work),
            on_abandon: Box::new(on_abandon),
            lease,
            abandonments,
        }),
        runtime: Handle::current(),
    }
}

#[must_use = "poll the future or drop it to transfer its completion"]
/// After returning Ready, later polls remain Pending and cannot restart work.
pub struct Retained<F: Future + Send + 'static> {
    pending: Option<Pending<F>>,
    runtime: Handle,
}

// Declaration order keeps future/cancellation destructors within ownership.
struct Pending<F> {
    work: Pin<Box<F>>,
    on_abandon: Box<dyn FnOnce() + Send>,
    lease: Lease,
    abandonments: Abandonments,
}

impl<F: Future + Send + 'static> Future for Retained<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let Some(pending) = this.pending.as_mut() else {
            // Like a fused future, completion remains inert on later polls.
            return Poll::Pending;
        };
        let result = pending.work.as_mut().poll(context);
        if result.is_ready() {
            // Destroy the completed future before releasing either ownership
            // or the cancellation closure's captured resources.
            drop(this.pending.take());
        }
        result
    }
}

impl<F: Future + Send + 'static> Drop for Retained<F> {
    fn drop(&mut self) {
        let Some(Pending {
            work,
            on_abandon,
            lease: owner,
            abandonments,
        }) = self.pending.take()
        else {
            return;
        };
        let abandoned = abandonments.0.acquire();
        on_abandon();
        self.runtime.spawn(async move {
            let _owner = owner;
            let _abandoned = abandoned;
            drop(work.await);
        });
    }
}

#[cfg(test)]
#[path = "retained_future_tests.rs"]
mod tests;
