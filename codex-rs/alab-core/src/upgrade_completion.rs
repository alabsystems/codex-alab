// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Owned post-commit capture. Cancelling a TUI waiter cannot cancel source cleanup.
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
type Operation = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
type Outcome = Result<(), String>;
struct State {
    operation: Mutex<Option<Operation>>,
    outcome: OnceLock<tokio::sync::watch::Receiver<Option<Outcome>>>,
}
#[derive(Clone)]
pub struct UpgradeCompletion(Arc<State>);
impl UpgradeCompletion {
    /// The operation must retain its actual committed source owners. This is a
    /// completion/lifetime mechanism, never native SDK or execution admission.
    pub fn new(operation: impl Future<Output = Outcome> + Send + 'static) -> Self {
        Self(Arc::new(State {
            operation: Mutex::new(Some(Box::pin(operation))),
            outcome: OnceLock::new(),
        }))
    }
    pub(crate) fn runtime() -> Option<tokio::runtime::Handle> {
        tokio::runtime::Handle::try_current().ok()
    }
    /// Dispatch starts ownership synchronously, before its first cancellable
    /// wait. The TUI runtime remains alive until the operation settles.
    pub(crate) fn start_on(&self, runtime: tokio::runtime::Handle) {
        let owner = self.0.clone();
        self.0.outcome.get_or_init(move || {
            let (send, receive) = tokio::sync::watch::channel(None);
            let operation = owner
                .operation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            runtime.spawn(async move {
                let result = match operation {
                    Some(operation) => operation.await,
                    None => Err("upgrade capture operation was already taken".into()),
                };
                drop(owner);
                send.send_replace(Some(result));
            });
            receive
        });
    }
    pub async fn wait(&self) -> Outcome {
        if self.0.outcome.get().is_none() {
            let runtime =
                Self::runtime().ok_or("upgrade capture requires its running TUI runtime")?;
            self.start_on(runtime);
        }
        let mut outcome = self
            .0
            .outcome
            .get()
            .ok_or("upgrade capture did not start")?
            .clone();
        loop {
            if let Some(result) = outcome.borrow_and_update().clone() {
                return result;
            }
            outcome
                .changed()
                .await
                .map_err(|_| "upgrade capture worker ended before completion".to_owned())?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::UpgradeDispatch;
    use crate::session::UpgradeRequest;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    #[tokio::test]
    async fn dispatch_starts_owned_capture_before_wait_and_a_lost_waiter_cannot_cancel_it() {
        struct Owner(Arc<AtomicBool>);
        impl Drop for Owner {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let count = calls.clone();
        let (request, cancellation) = UpgradeRequest::with_context(move |owner: Owner| {
            Some(UpgradeCompletion::new(async move {
                count.fetch_add(1, Ordering::SeqCst);
                entered.send(()).unwrap();
                released.await.unwrap();
                drop(owner);
                Ok(())
            }))
        });
        assert!(request.requires_context());
        let UpgradeDispatch::Capturing(completion) =
            request.decide_with_context(true, Owner(dropped.clone()))
        else {
            panic!("capture was not dispatched");
        };
        drop(cancellation);
        tokio::time::timeout(std::time::Duration::from_secs(2), started)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !dropped.load(Ordering::SeqCst),
            "the source remains owned before completion"
        );
        let waiter = tokio::spawn({
            let completion = completion.clone();
            async move { completion.wait().await }
        });
        tokio::task::yield_now().await;
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        completion.wait().await.unwrap();
        completion.wait().await.unwrap();
        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn capture_context_cancellation_refusal_and_wrong_type_have_no_source_effects() {
        for case in 0..4 {
            let called = Arc::new(AtomicBool::new(false));
            let observed = called.clone();
            let (request, cancellation) = UpgradeRequest::with_context(move |_: u64| {
                observed.store(true, Ordering::SeqCst);
                Some(UpgradeCompletion::new(async { Ok(()) }))
            });
            match case {
                0 => {
                    drop(cancellation);
                    assert!(matches!(
                        request.decide_with_context(true, 7u64),
                        UpgradeDispatch::Refused
                    ));
                }
                1 => {
                    assert!(!request.decide(true));
                }
                2 => {
                    assert!(matches!(
                        request.decide_with_context(true, "wrong owner type"),
                        UpgradeDispatch::Refused
                    ));
                }
                3 => {
                    assert!(matches!(
                        request.decide_with_context(false, 7u64),
                        UpgradeDispatch::Refused
                    ));
                }
                _ => unreachable!(),
            }
            assert!(!called.load(Ordering::SeqCst));
        }
    }
    #[test]
    fn a_capture_request_refuses_before_effects_without_a_running_runtime() {
        let (request, _cancellation) =
            UpgradeRequest::with_context(|_: u64| -> Option<UpgradeCompletion> {
                panic!("no runtime cannot commit source effects");
            });
        assert!(matches!(
            request.decide_with_context(true, 7u64),
            UpgradeDispatch::Refused
        ));
    }
    #[tokio::test]
    async fn capture_failure_is_not_a_ready_to_teardown_result() {
        let (request, cancellation) = UpgradeRequest::with_context(|_: u64| {
            Some(UpgradeCompletion::new(async {
                Err("source closure failed".into())
            }))
        });
        let UpgradeDispatch::Capturing(completion) = request.decide_with_context(true, 7u64) else {
            panic!("capture was not dispatched");
        };
        drop(cancellation);
        assert_eq!(completion.wait().await, Err("source closure failed".into()));
    }
}
