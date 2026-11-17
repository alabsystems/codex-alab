// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep constructor-custody input queued until native UI layout and SDK activation.
//! This scheduling barrier never supplies SDK admission or constructor authority.
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

static READY: AtomicBool = AtomicBool::new(false);

pub fn deferred() -> bool {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(deferred) = fixture::deferred() {
        return deferred;
    }
    crate::upgrade_state::pending().is_some() && !READY.load(Ordering::Acquire)
}
/// A protected screen must retry or fail without consuming constructor-custody
/// keys. Returning success here would falsely certify a discarded boundary.
pub fn check_input_discard() -> std::io::Result<()> {
    if deferred() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "runtime restoration still owns pending terminal input",
        ));
    }
    Ok(())
}

/// Called after actual restored editor layout and successful native activation.
/// Its caller wakes the actual TUI frame stream after releasing this barrier.
pub fn release_after_activation() {
    READY.store(true, Ordering::Release);
}

#[cfg(any(test, feature = "test-support"))]
mod fixture {
    use super::*;
    use std::cell::RefCell;
    use std::marker::PhantomData;
    use std::rc::Rc;
    use std::sync::Arc;
    thread_local! { static LOCAL: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) }; }
    pub struct Scope {
        ready: Arc<AtomicBool>,
        _thread: PhantomData<Rc<()>>,
    }
    impl Scope {
        pub fn deferred() -> Self {
            let ready = Arc::new(AtomicBool::new(false));
            LOCAL.with(|slot| {
                let mut slot = slot.borrow_mut();
                assert!(slot.is_none(), "one scheduling fixture per thread");
                *slot = Some(ready.clone());
            });
            Self {
                ready,
                _thread: PhantomData,
            }
        }
        pub fn release(&self) {
            self.ready.store(true, Ordering::Release);
        }
    }
    impl Drop for Scope {
        fn drop(&mut self) {
            LOCAL.with(|slot| *slot.borrow_mut() = None);
        }
    }
    pub fn deferred() -> Option<bool> {
        LOCAL.with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|ready| !ready.load(Ordering::Acquire))
        })
    }
}
/// Scheduling-only fixture; cannot create a native SDK request or continuation.
#[cfg(any(test, feature = "test-support"))]
pub use fixture::Scope as TestScope;
