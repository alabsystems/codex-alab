// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Constructor admission precedes configuration and credential readers. The retained
//! allocation is permission to begin preload, never a Source or a readiness proof.

use crate::managed_auth::unavailable;
use crate::native_source::Member;
use crate::native_source::{self};
use std::future::Future;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::Notify;
use tokio::sync::OnceCell;

pub struct Allocation {
    pub(crate) operation: String,
    pub(crate) member: Arc<dyn Member>,
    pub(crate) parent_use: Option<crate::native_source_use::ParentUseRef>,
    pub(crate) derived_parent: Option<crate::native_source_lifetime::MemberIncarnation>,
    pub(crate) target: Option<Arc<dyn crate::native_target::Destination>>,
}
impl std::fmt::Debug for Allocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeConstructorAllocation(<private>)")
    }
}

#[derive(Default)]
struct Pending {
    result: OnceCell<Result<Mutex<Option<Allocation>>, ()>>,
    changed: Notify,
}
#[derive(Default)]
struct Startup {
    // The worker owns admission even when its original waiter is cancelled. A waiter
    // never manufactures another operation; transient transport loss is reconciled
    // by the owned allocation worker using its original native open nonce.
    seed: OnceLock<Arc<Pending>>,
}
impl Startup {
    async fn enter<F, Fut>(&self, required: bool, open: F) -> io::Result<()>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = io::Result<Allocation>> + Send + 'static,
    {
        if !required && self.seed.get().is_none() {
            return Ok(());
        }
        let pending = self.seed.get_or_init(|| {
            let pending = Arc::new(Pending::default());
            let result = Arc::clone(&pending);
            let worker = tokio::spawn(open());
            tokio::spawn(async move {
                let outcome = worker
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .map(|value| Mutex::new(Some(value)))
                    .ok_or(());
                let _ = result.result.set(outcome);
                result.changed.notify_waiters();
            });
            pending
        });
        loop {
            let changed = pending.changed.notified();
            if let Some(outcome) = pending.result.get() {
                return match outcome {
                    Ok(seed) => {
                        if seed
                            .lock()
                            .map_err(|_| unavailable())?
                            .as_ref()
                            .is_some_and(|seed| !seed.member.live())
                        {
                            return Err(unavailable());
                        }
                        Ok(())
                    }
                    Err(()) => Err(unavailable()),
                };
            }
            changed.await;
        }
    }
    fn take(&self) -> io::Result<Option<Allocation>> {
        match self.seed.get().and_then(|pending| pending.result.get()) {
            Some(Ok(seed)) => seed
                .lock()
                .map_err(|_| unavailable())
                .map(|mut seed| seed.take()),
            Some(Err(())) | None => Err(unavailable()),
        }
    }
}

#[derive(Default)]
struct Requirement(AtomicBool);
impl Requirement {
    fn observe(&self, present: bool) -> bool {
        self.0.fetch_or(present, Ordering::AcqRel) || present
    }
}
static REQUIRED: Requirement = Requirement(AtomicBool::new(false));

static STARTUP: Startup = Startup {
    seed: OnceLock::new(),
};

tokio::task_local! {
    static DESTINATION: std::sync::Arc<DestinationAllocation>;
}
struct DestinationAllocation {
    allocation: std::sync::Mutex<Option<Allocation>>,
    member: Arc<dyn crate::native_source::Member>,
    operation: String,
    observer: OnceLock<crate::native_reader_retirement::Observer>,
    rejected: OnceLock<crate::native_target::OpenCleanup>,
}
impl Drop for DestinationAllocation {
    fn drop(&mut self) {
        // The runtime and owned native cleanup actor are still available during
        // constructor cancellation. An observer dropping never means Closed.
        if let Some(allocation) = self.allocation.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
            crate::native_target::retire_unused(allocation);
        }
    }
}
pub(crate) fn destination_scope<F: Future>(allocation: Allocation, constructor: F) -> impl Future<Output = crate::native_target::Constructed<F::Output>> {
    // Create the owner BEFORE returning a future: even cancellation before its
    // first poll must retain/release the exact accepted target OPEN.
    let allocation = Arc::new(DestinationAllocation {
        member: allocation.member.clone(), operation: allocation.operation.clone(),
        allocation: std::sync::Mutex::new(Some(allocation)), observer: OnceLock::new(), rejected: OnceLock::new(),
    });
    async move {
        let output = DESTINATION.scope(allocation.clone(), constructor).await;
        // Recover only ownership metadata from poison; allocate_manager itself
        // refused that poisoned admission, so this cannot authorize a load.
        if let Some(unused) = allocation.allocation.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
            let _ = allocation.rejected.set(crate::native_target::retire_unused(unused));
        }
        crate::native_target::Constructed { value: output, receipt: crate::native_target::ConstructionReceipt {
            operation: allocation.operation.clone(), member: Some(allocation.member.id().into()),
            retirement: allocation.observer.get().cloned(), unloaded_cleanup: allocation.rejected.get().cloned(),
        } }
    }
}
pub(crate) fn destination_unused(allocation: &Allocation, receipt: crate::native_target::OpenCleanup) {
    let _ = DESTINATION.try_with(|owner| {
        if owner.operation == allocation.operation && Arc::ptr_eq(&owner.member, &allocation.member) && owner.observer.get().is_none() {
            let _ = owner.rejected.set(receipt);
        }
    });
}
pub(crate) fn destination_member_created(member: &crate::native_source_lifetime::Lifetime) -> io::Result<()> {
    DESTINATION.try_with(|owner| {
        if owner.operation != member.constructor_operation() || !Arc::ptr_eq(&owner.member, member.member_handle()) {
            return Err(unavailable());
        }
        owner.observer.set(member.retirement_observer()).map_err(|_| unavailable())
    }).unwrap_or(Ok(()))
}

pub fn requested() -> bool {
    // Presence, including malformed/non-UTF8 values, forbids a legacy fallback. The
    // native constructor verifies the kernel launch environment, not a client token.
    REQUIRED.observe(std::env::var_os(native_source::LAUNCH_ENV).is_some())
        || STARTUP.seed.get().is_some()
}
async fn retry_open<T, F, Fut>(operation: String, mut open: F) -> io::Result<T>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = io::Result<T>>,
{
    loop {
        match open(operation.clone()).await {
            Ok(value) => return Ok(value),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::ConnectionAborted
                        | io::ErrorKind::NotConnected
                        | io::ErrorKind::BrokenPipe
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::NotFound
                        | io::ErrorKind::UnexpectedEof
                ) =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            Err(error) => return Err(error),
        }
    }
}
async fn allocate() -> io::Result<Allocation> {
    let operation = format!("{:032x}", rand::random::<u128>());
    let member = retry_open(operation.clone(), |operation| async move {
        native_source::open_member(&operation).await
    })
    .await?;
    Ok(Allocation {
        operation,
        member,
        parent_use: None,
        derived_parent: None,
        target: None,
    })
}

async fn allocate_owned() -> io::Result<Allocation> {
    let (send, receive) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        if let Err(Ok(allocation)) = send.send(allocate().await) {
            // The constructor waiter disappeared. Retire the admitted empty member;
            // EOF alone must not be treated as a credential-reader retirement.
            let _ = crate::native_source_retirement::start(allocation.member, None, None);
        }
    });
    receive.await.map_err(|_| unavailable())?
}

async fn allocate_derived_owned(
    source: crate::native_source_lifetime::Lifetime,
    parent: crate::native_source_use::ParentUseLease,
) -> io::Result<Allocation> {
    let (send, receive) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let outcome = async {
            let operation = format!("{:032x}", rand::random::<u128>());
            let receipt = parent.ready().await?;
            let member = retry_open(operation.clone(), |operation| {
                let source = source.clone();
                let receipt = receipt.clone();
                async move {
                    source
                        .member_handle()
                        .open_derived(&operation, &receipt)
                        .await
                }
            })
            .await?;
            Ok(Allocation {
                operation,
                member,
                parent_use: Some(parent.downgrade()),
                derived_parent: Some(source.incarnation()),
                target: source.target_role(),
            })
        }
        .await;
        if let Err(Ok(allocation)) = send.send(outcome) {
            let _ = crate::native_source_retirement::start(allocation.member, None, None);
        }
    });
    receive.await.map_err(|_| unavailable())?
}

/// The CLI calls this before dispatching any configuration or authentication workflow.
/// No token means no native constructor was requested; a failed token never falls back.
pub async fn enter() -> io::Result<()> {
    STARTUP.enter(requested(), allocate).await
}

/// A manager takes the real early allocation once. Other managers allocate independently;
/// they do not inherit another manager's source, policy, or AuthChangeState counters.
pub async fn allocate_manager() -> io::Result<Option<Allocation>> {
    if let Ok(allocation) = DESTINATION.try_with(|owner| {
        owner.allocation.lock().map_err(|_| unavailable()).and_then(|mut allocation| allocation.take().ok_or_else(unavailable))
    }) {
        // A target scope can be consumed once. A second manager needs a real
        // separate native destination/derived constructor, never startup A.
        return allocation.map(Some);
    }
    if !requested() {
        return Ok(None);
    }
    enter().await?;
    match STARTUP.take()? {
        Some(seed) if seed.member.live() => Ok(Some(seed)),
        Some(_) => Err(unavailable()),
        None => {
            if let Some(activity) = crate::assignment::current_activity()
                && let Some(source) = activity.domain().native_member()?
                && let Some(parent) = activity.native_use_for(&source)?
            {
                return allocate_derived_owned(source, parent).await.map(Some);
            }
            allocate_owned().await.map(Some)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    #[tokio::test]
    async fn absent_constructor_does_not_open_or_read_credentials() {
        let startup = Startup::default();
        startup
            .enter(false, || async { panic!("no constructor requested") })
            .await
            .unwrap();
        assert!(startup.seed.get().is_none());
    }

    #[tokio::test]
    async fn denied_constructor_cannot_restart_or_fall_back_when_its_hint_disappears() {
        let startup = Startup::default();
        let calls = Arc::new(AtomicUsize::new(0));
        for required in [true, false] {
            assert!(
                startup
                    .enter(required, {
                        let calls = Arc::clone(&calls);
                        move || async move {
                            calls.fetch_add(1, Ordering::Relaxed);
                            Err(io::Error::new(
                                io::ErrorKind::PermissionDenied,
                                "private-native-error",
                            ))
                        }
                    })
                    .await
                    .is_err()
            );
        }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(startup.take().is_err());
    }
    #[tokio::test]
    async fn cancelling_the_first_waiter_keeps_the_owned_admission_and_same_attempt() {
        let startup = Arc::new(Startup::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let waiter = {
            let startup = Arc::clone(&startup);
            let calls = Arc::clone(&calls);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            tokio::spawn(async move {
                startup
                    .enter(true, move || async move {
                        calls.fetch_add(1, Ordering::Relaxed);
                        started.notify_one();
                        release.notified().await;
                        Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "denied constructor admission",
                        ))
                    })
                    .await
            })
        };
        started.notified().await;
        waiter.abort();
        let _ = waiter.await;
        release.notify_one();
        assert!(
            startup
                .enter(true, || async {
                    panic!("must retain the original attempt")
                })
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn constructor_requirement_cannot_disappear_with_its_environment_hint() {
        let requirement = Requirement::default();
        assert!(!requirement.observe(false));
        assert!(requirement.observe(true));
        assert!(requirement.observe(false));
        // A latched requirement is not itself admission or permission to read.
        assert!(Startup::default().take().is_err());
    }
    #[tokio::test]
    async fn uncertain_open_retries_only_the_same_semantic_nonce() {
        let mut calls = Vec::new();
        let result = retry_open("7".repeat(32), |operation| {
            calls.push(operation);
            let attempt = calls.len();
            async move {
                if attempt < 3 {
                    Err(io::Error::from(io::ErrorKind::ConnectionReset))
                } else {
                    Ok(42)
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(result, 42);
        assert_eq!(calls, vec!["7".repeat(32); 3]);
    }
    #[tokio::test]
    async fn denied_or_malformed_open_is_never_retried() {
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::Unsupported,
            io::ErrorKind::InvalidData,
        ] {
            let mut calls = 0;
            assert!(
                retry_open("7".repeat(32), |_| {
                    calls += 1;
                    async move { Err::<(), _>(io::Error::from(kind)) }
                })
                .await
                .is_err()
            );
            assert_eq!(calls, 1);
        }
    }
}
