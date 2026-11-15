// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Process-local correlation for one actual native tool dispatch. This token
//! supplies no goal authority: outcomes, committed mutations and live policy
//! remain independently checked. Provider IDs cannot recreate an invocation.

use std::future::Future;
use std::sync::Arc;
use tokio::task::futures::TaskLocalFuture;

#[derive(Clone, Debug)]
pub struct Invocation(Arc<()>);

impl Invocation {
    pub fn new() -> Self {
        Self(Arc::new(()))
    }

    /// Install only while polling this future. Spawned work and an outer abort
    /// callback must explicitly carry the same token; they cannot infer it from
    /// a reused provider ID or whichever invocation happens to be current.
    pub fn scope<F: Future>(&self, work: F) -> TaskLocalFuture<Self, F> {
        INVOCATION.scope(self.clone(), work)
    }
}

impl Default for Invocation {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for Invocation {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Invocation {}

tokio::task_local! { static INVOCATION: Invocation; }

pub fn current() -> Option<Invocation> {
    INVOCATION.try_with(Clone::clone).ok()
}

/// Keep native hooks to a scope on an existing future, including cancellation
/// notifications that run outside the spawned handler's task.
pub trait ScopeFuture: Future + Sized {
    fn with_model_goal_invocation(
        self,
        invocation: Invocation,
    ) -> TaskLocalFuture<Invocation, Self> {
        invocation.scope(self)
    }
}

impl<F: Future> ScopeFuture for F {}

#[cfg(test)]
#[path = "model_goal_invocation_tests.rs"]
mod tests;
