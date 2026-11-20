// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Destination admission shared by the restored goal runtimes and submission
//! senders. Only the original native operation can release the complete forest.
use std::sync::Arc;
use tokio::sync::watch;

#[derive(Clone)]
pub struct Fence(Arc<watch::Sender<State>>);
#[derive(Clone, Copy, Eq, PartialEq)]
enum State {
    Pending,
    Released,
    Invalid,
}
pub struct Owner {
    fence: Fence,
}
impl Default for Owner {
    fn default() -> Self {
        Self {
            fence: Fence(Arc::new(watch::Sender::new(State::Pending))),
        }
    }
}
impl Owner {
    pub fn fence(&self) -> Fence {
        self.fence.clone()
    }
    /// Join native Core/SDK activation with the goal hold's release. A failed
    /// callback or changed configuration leaves all goal continuation held.
    pub fn release_if(&self, release: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        let mut outcome = Err("goal startup hold changed before activation".into());
        self.fence.0.send_if_modified(|state| {
            if *state != State::Pending {
                return false;
            }
            outcome = release();
            if outcome.is_ok() {
                *state = State::Released;
                return true;
            }
            false
        });
        outcome
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.fence.invalidate();
    }
}
impl Fence {
    pub fn pending(&self) -> bool {
        *self.0.borrow() != State::Released
    }
    pub fn valid(&self) -> bool {
        *self.0.borrow() != State::Invalid
    }
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    /// A rejected or abandoned destination cannot accept a waiting input.
    pub async fn wait_for_release(&self) -> bool {
        let mut changed = self.0.subscribe();
        changed
            .wait_for(|state| *state != State::Pending)
            .await
            .is_ok_and(|state| *state == State::Released)
    }
    pub fn invalidate(&self) {
        self.0.send_if_modified(|state| {
            if *state != State::Pending {
                return false;
            }
            *state = State::Invalid;
            true
        });
    }
}
