// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Scoped wall-clock references for synchronous rendering fixtures.
//! Normal product builds always read the real clock.

use chrono::DateTime;
use chrono::Utc;
#[cfg(any(test, feature = "test-support"))]
use std::cell::RefCell;
#[cfg(any(test, feature = "test-support"))]
use std::rc::Rc;

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static REFERENCES: RefCell<Vec<(Rc<()>, DateTime<Utc>)>> = const { RefCell::new(Vec::new()) };
}

pub fn utc_now() -> DateTime<Utc> {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(reference) =
        REFERENCES.with(|references| references.borrow().last().map(|(_, reference)| *reference))
    {
        return reference;
    }
    Utc::now()
}

/// Keep the reference on its owning thread. Dropping nested scopes in any order
/// removes only that scope, so a surviving newer reference cannot be overwritten.
#[cfg(any(test, feature = "test-support"))]
#[must_use = "the fixture clock is restored when its scope is dropped"]
pub struct Scope {
    token: Rc<()>,
}

#[cfg(any(test, feature = "test-support"))]
pub fn scoped(reference: DateTime<Utc>) -> Scope {
    let token = Rc::new(());
    REFERENCES.with(|references| {
        references.borrow_mut().push((Rc::clone(&token), reference));
    });
    Scope { token }
}

/// Fix one synchronous render without holding a clock override across an await.
#[cfg(any(test, feature = "test-support"))]
#[allow(
    clippy::expect_used,
    reason = "an invalid test-only fixture must fail before installing its clock or rendering"
)]
pub fn with_timestamp<R>(seconds: i64, render: impl FnOnce() -> R) -> R {
    let _scope = scoped(DateTime::from_timestamp(seconds, 0).expect("fixture timestamp"));
    render()
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for Scope {
    fn drop(&mut self) {
        REFERENCES.with(|references| {
            references
                .borrow_mut()
                .retain(|(token, _)| !Rc::ptr_eq(token, &self.token));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn reference(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("valid fixture timestamp")
    }

    #[test]
    fn nested_scopes_restore_and_out_of_order_drop_keeps_the_current_reference() {
        assert!(REFERENCES.with(|references| references.borrow().is_empty()));
        let first = scoped(reference(1));
        let second = scoped(reference(2));
        assert_eq!(utc_now(), reference(2));
        drop(second);
        assert_eq!(utc_now(), reference(1));
        let third = scoped(reference(3));
        drop(first);
        assert_eq!(utc_now(), reference(3));
        drop(third);
        assert!(REFERENCES.with(|references| references.borrow().is_empty()));
    }

    #[test]
    fn unwinding_restores_the_surviving_scope() {
        let outer = scoped(reference(1));
        let result = std::panic::catch_unwind(|| {
            let _inner = scoped(reference(2));
            assert_eq!(utc_now(), reference(2));
            panic!("fixture unwind");
        });
        assert!(result.is_err());
        assert_eq!(utc_now(), reference(1));
        drop(outer);
        assert!(REFERENCES.with(|references| references.borrow().is_empty()));
    }

    #[test]
    fn simultaneous_threads_keep_independent_references() {
        let _scope = scoped(reference(1));
        let (entered, ready) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let other = std::thread::spawn(move || {
            assert!(REFERENCES.with(|references| references.borrow().is_empty()));
            let _scope = scoped(reference(2));
            entered.send(()).expect("parent is waiting");
            released
                .recv_timeout(Duration::from_secs(5))
                .expect("parent releases the other thread");
            assert_eq!(utc_now(), reference(2));
        });
        ready
            .recv_timeout(Duration::from_secs(5))
            .expect("other thread installed its reference");
        assert_eq!(utc_now(), reference(1));
        release.send(()).expect("other thread is waiting");
        other.join().expect("other fixture thread succeeds");
        assert_eq!(utc_now(), reference(1));
    }
}
