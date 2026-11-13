// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A native startup hold shared by every restored goal runtime. It only reduces
//! admission. Neither a fence nor its release supplies SDK/model permission.
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Clone)]
pub struct Fence(Arc<Mutex<State>>);
struct State {
    pending: bool,
    valid: bool,
}
pub struct Owner {
    fence: Fence,
}
impl Default for Owner {
    fn default() -> Self {
        Self {
            fence: Fence(Arc::new(Mutex::new(State {
                pending: true,
                valid: true,
            }))),
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
        let mut state = self
            .fence
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.pending || !state.valid {
            return Err("goal startup hold changed before activation".into());
        }
        release()?;
        state.pending = false;
        Ok(())
    }
}
impl Fence {
    pub fn pending(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending
    }
    pub fn valid(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .valid
    }
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn invalidate(&self) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.pending {
            state.valid = false;
        }
    }
}
