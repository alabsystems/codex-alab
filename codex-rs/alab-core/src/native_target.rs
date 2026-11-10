// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Destination initialization authority received on an authenticated native channel.

use crate::managed_auth::unavailable;
use crate::managed_auth::valid_hex;
use crate::native_source::Source;
use std::future::Future;
use std::io;
use std::pin::Pin;

#[path = "native_target_resource.rs"]
mod resource;
pub use resource::Applied;
pub use resource::AppliedFuture;
pub use resource::ResourceFuture;
pub use resource::ResourceReceipt;
pub use resource::ResourceReport;

#[path = "native_target_open_retirement.rs"]
mod open_retirement;
pub use open_retirement::OpenRetirement;
pub use open_retirement::OpenRetirementFuture;

/// Retained destination role supplied by the private transport. Opening each
/// member uses its own connection while borrowing this initialization owner.
pub trait Destination: Send + Sync {
    fn context(&self) -> &Context;
    fn open_member<'a>(&'a self, operation: &'a str) -> crate::native_source::MemberFuture<'a>;
    fn retire_open<'a>(
        &'a self,
        _operation: &'a str,
        _open_operation: &'a str,
        _member: Option<&'a str>,
    ) -> OpenRetirementFuture<'a> {
        Box::pin(async { Err(unavailable()) })
    }
    fn acknowledge<'a>(&'a self, _receipt: &'a ResourceReceipt) -> AppliedFuture<'a> {
        Box::pin(async { Err(unavailable()) })
    }
}

/// Construct under one actual installed target allocation. The constructor must
/// consume it through the normal member loader; this creates no local catalog
/// admission or permission to publish/reopen a process.
pub async fn construct<F: Future>(
    destination: std::sync::Arc<dyn Destination>,
    operation: &str,
    constructor: F,
) -> Result<Constructed<F::Output>, ConstructionFailure> {
    let (allocation, cleanup) = allocate_observed(destination, operation).await;
    match allocation {
        Ok(allocation) => {
            Ok(crate::native_source_launch::destination_scope(allocation, constructor).await)
        }
        Err(error) => Err(ConstructionFailure {
            error,
            receipt: ConstructionReceipt {
                operation: operation.into(),
                member: None,
                retirement: None,
                unloaded_cleanup: Some(cleanup),
            },
        }),
    }
}
pub struct ConstructionFailure {
    pub error: io::Error,
    pub receipt: ConstructionReceipt,
}
pub struct Constructed<T> {
    pub value: T,
    pub receipt: ConstructionReceipt,
}
pub struct ConstructionReceipt {
    pub(crate) operation: String,
    pub(crate) member: Option<String>,
    pub(crate) retirement: Option<crate::native_reader_retirement::Observer>,
    pub(crate) unloaded_cleanup: Option<OpenCleanup>,
}
pub enum ConstructionClosed {
    Member(crate::native_reader_retirement::Closed),
    Unloaded(std::sync::Arc<OpenRetirement>),
}
impl ConstructionReceipt {
    pub fn operation(&self) -> &str {
        &self.operation
    }
    pub fn member(&self) -> Option<&str> {
        self.member.as_deref()
    }
    pub fn retirement_observer(&self) -> io::Result<crate::native_reader_retirement::Observer> {
        self.retirement.clone().ok_or_else(unavailable)
    }
    pub(crate) fn retire_quiescent(
        &self,
        domain: &std::sync::Arc<crate::assignment::Domain>,
    ) -> io::Result<()> {
        if !domain.catalog_idle(true).map_err(|_| unavailable())?
            || !crate::native_source_use::settled_for(domain)
        {
            return Err(unavailable());
        }
        if let Some(observer) = &self.retirement
            && let Some(member) = observer.incarnation().upgrade()
        {
            if member
                .domain()
                .is_some_and(|actual| !std::sync::Arc::ptr_eq(&actual, domain))
            {
                return Err(unavailable());
            }
            // The owning destination role has closed new callbacks and drained
            // real activities. Idle metadata contexts may still retain Arc<M>;
            // explicitly retire the exact member instead of waiting for Arc Drop.
            let _ = member.retire_reader()?;
        }
        Ok(())
    }
    pub async fn closed(&self) -> io::Result<ConstructionClosed> {
        if let Some(observer) = &self.retirement {
            return observer.closed().await.map(ConstructionClosed::Member);
        }
        if let Some(cleanup) = &self.unloaded_cleanup {
            return cleanup.closed().await.map(ConstructionClosed::Unloaded);
        }
        // No selected lifetime/cleanup outcome is Unknown, never NoReader/Closed.
        Err(unavailable())
    }
    pub(crate) fn matches_closed(&self, context: &Context, closed: &ConstructionClosed) -> bool {
        match closed {
            ConstructionClosed::Member(closed) => self
                .retirement
                .as_ref()
                .is_some_and(|observer| closed.matches(observer.incarnation())),
            ConstructionClosed::Unloaded(closed) => {
                closed.assignment() == context.assignment()
                    && closed.open_operation() == self.operation
                    && closed.member() == self.member.as_deref()
            }
        }
    }
}
#[derive(Clone)]
pub struct OpenCleanup(tokio::sync::watch::Receiver<Option<std::sync::Arc<OpenRetirement>>>);
impl OpenCleanup {
    pub async fn closed(&self) -> io::Result<std::sync::Arc<OpenRetirement>> {
        let mut replies = self.0.clone();
        loop {
            if let Some(receipt) = replies.borrow_and_update().clone() {
                return Ok(receipt);
            }
            replies.changed().await.map_err(|_| unavailable())?;
        }
    }
}

pub(crate) fn retire_unused(allocation: crate::native_source_launch::Allocation) -> OpenCleanup {
    let (reply, result) = tokio::sync::watch::channel(None);
    let receipt = OpenCleanup(result);
    crate::native_source_launch::destination_unused(&allocation, receipt.clone());
    tokio::spawn(async move {
        let Some(destination) = allocation.target else {
            return;
        };
        let cleanup = format!("{:032x}", rand::random::<u128>());
        loop {
            if let Ok(receipt) = destination
                .retire_open(
                    &cleanup,
                    &allocation.operation,
                    Some(allocation.member.id()),
                )
                .await
                && receipt.assignment() == destination.context().assignment()
                && receipt.operation() == cleanup
                && receipt.open_operation() == allocation.operation
                && receipt.member() == Some(allocation.member.id())
            {
                reply.send_replace(Some(std::sync::Arc::new(receipt)));
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    });
    receipt
}

/// Allocate through the actual retained destination. The caller owns the OPEN
/// operation before invoking this; its detached actor retains exact cleanup on
/// cancellation or uncertainty.
/// This does not install a target, open a local catalog, or authorize A work.
pub async fn allocate(
    destination: std::sync::Arc<dyn Destination>,
    operation: &str,
) -> io::Result<crate::native_source_launch::Allocation> {
    allocate_observed(destination, operation).await.0
}
async fn allocate_observed(
    destination: std::sync::Arc<dyn Destination>,
    operation: &str,
) -> (
    io::Result<crate::native_source_launch::Allocation>,
    OpenCleanup,
) {
    let (closed, cleanup) = tokio::sync::watch::channel(None);
    let cleanup = OpenCleanup(cleanup);
    if !valid_hex(operation, 32) {
        return (Err(unavailable()), cleanup);
    }
    let operation = operation.to_owned();
    let (send, receive) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = destination
            .open_member(&operation)
            .await
            .and_then(|member| {
                if !valid_hex(member.id(), 32) || !member.live() {
                    return Err(unavailable());
                }
                Ok(crate::native_source_launch::Allocation {
                    operation: operation.clone(),
                    member,
                    parent_use: None,
                    derived_parent: None,
                    target: Some(destination.clone()),
                })
            });
        let member = result
            .as_ref()
            .ok()
            .map(|allocation| allocation.member.clone());
        let failed = result.is_err();
        let abandoned = send.send(result).is_err();
        if failed || abandoned {
            // The OPEN body is retained before its first write. A lost response
            // never proves that no member was issued; only correlated Closed does.
            let cleanup = format!("{:032x}", rand::random::<u128>());
            loop {
                if let Ok(receipt) = destination
                    .retire_open(
                        &cleanup,
                        &operation,
                        member.as_ref().map(|member| member.id()),
                    )
                    .await
                    && receipt.assignment() == destination.context().assignment()
                    && receipt.operation() == cleanup
                    && receipt.open_operation() == operation
                    && receipt.member() == member.as_ref().map(|member| member.id())
                {
                    closed.send_replace(Some(std::sync::Arc::new(receipt)));
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
    });
    (
        receive.await.unwrap_or_else(|_| Err(unavailable())),
        cleanup,
    )
}

pub type DestinationFuture<'a> =
    Pin<Box<dyn Future<Output = io::Result<Box<dyn Destination>>> + Send + 'a>>;

/// An actual native Installed role, never an old-source borrow or use receipt.
/// No Clone/serialization implementation: the initialization owner retains it
/// through resource initialization and exact ACK reconciliation.
pub struct Context {
    assignment: String,
    capability: zeroize::Zeroizing<String>,
    generation: u64,
    target_ref: String,
}

impl Context {
    /// Called by the private adapter after authenticating the complete reply and
    /// binding its caller/session/native instance. Shape validation alone is not
    /// source-seal, census, grant-lineage or migration authority.
    pub fn from_native(
        assignment: String,
        capability: String,
        generation: u64,
        target_ref: String,
    ) -> io::Result<Self> {
        let capability = zeroize::Zeroizing::new(capability);
        if !valid_hex(&assignment, 32)
            || !valid_hex(&capability, 64)
            || generation == 0
            || !valid_hex(&target_ref, 64)
        {
            return Err(unavailable());
        }
        Ok(Self {
            assignment,
            capability,
            generation,
            target_ref,
        })
    }

    pub fn assignment(&self) -> &str {
        &self.assignment
    }

    pub fn assignment_generation(&self) -> u64 {
        self.generation
    }

    pub fn target_ref(&self) -> &str {
        &self.target_ref
    }

    /// Only the authenticated private adapter may put this into a request.
    /// The native server rechecks the actual retained Installed role each time.
    pub fn private_capability(&self) -> &str {
        self.capability.as_str()
    }

    /// Correlate a freshly authenticated target preload. The native endpoint
    /// additionally checks target_ref against its actual Book/Slot/owner role;
    /// these SDK fields cannot replace that check or authorize generic reads.
    pub fn validate_source(&self, source: &Source) -> io::Result<()> {
        source.validate()?;
        if source.assignment_generation != self.generation
            || source.native_intent.as_deref() != Some(self.assignment.as_str())
        {
            return Err(unavailable());
        }
        Ok(())
    }
}

impl std::fmt::Debug for Context {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeTargetContext(<private>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_rejects_noncanonical_authority_and_redacts_all_fields() {
        for (assignment, capability, generation, target) in [
            ("a".repeat(31), "b".repeat(64), 1, "c".repeat(64)),
            ("a".repeat(32), "B".repeat(64), 1, "c".repeat(64)),
            ("a".repeat(32), "b".repeat(64), 0, "c".repeat(64)),
            ("a".repeat(32), "b".repeat(64), 1, "c".repeat(63)),
        ] {
            assert!(Context::from_native(assignment, capability, generation, target).is_err());
        }
        let context =
            Context::from_native("a".repeat(32), "b".repeat(64), 7, "c".repeat(64)).unwrap();
        assert_eq!(format!("{context:?}"), "NativeTargetContext(<private>)");
        assert_eq!(context.assignment_generation(), 7);
    }
    struct Peer;
    impl crate::native_source::Member for Peer {
        fn id(&self) -> &str {
            "11111111111111111111111111111111"
        }
        fn live(&self) -> bool {
            true
        }
        fn ready<'a>(
            &'a self,
            _: &'a str,
            _: Option<&'a str>,
            _: &'a crate::native_source::Loaded,
        ) -> crate::native_source::ReadyFuture<'a> {
            Box::pin(async { Err(unavailable()) })
        }
        fn retire<'a>(&'a self, _: Option<&'a str>) -> crate::native_source::UnitFuture<'a> {
            Box::pin(async { panic!("never-preloaded target uses exact OPEN cleanup") })
        }
    }
    struct Target {
        context: Context,
        opened: tokio::sync::Notify,
        release_open: tokio::sync::Notify,
        cleaning: tokio::sync::Notify,
        release_cleanup: tokio::sync::Notify,
        retired: tokio::sync::Notify,
        bodies: std::sync::Mutex<Vec<(String, String, Option<String>)>>,
    }
    impl Destination for Target {
        fn context(&self) -> &Context {
            &self.context
        }
        fn open_member<'a>(&'a self, operation: &'a str) -> crate::native_source::MemberFuture<'a> {
            Box::pin(async move {
                assert_eq!(operation, "f".repeat(32));
                self.opened.notify_one();
                self.release_open.notified().await;
                Ok(std::sync::Arc::new(Peer) as std::sync::Arc<dyn crate::native_source::Member>)
            })
        }
        fn retire_open<'a>(
            &'a self,
            operation: &'a str,
            open: &'a str,
            member: Option<&'a str>,
        ) -> OpenRetirementFuture<'a> {
            Box::pin(async move {
                self.bodies.lock().unwrap().push((
                    operation.into(),
                    open.into(),
                    member.map(str::to_owned),
                ));
                self.cleaning.notify_one();
                self.release_cleanup.notified().await;
                let receipt = OpenRetirement::from_native(
                    &self.context,
                    operation.into(),
                    open.into(),
                    member.map(str::to_owned),
                )?;
                self.retired.notify_one();
                Ok(receipt)
            })
        }
    }
    fn constructor_target() -> std::sync::Arc<Target> {
        std::sync::Arc::new(Target {
            context: Context::from_native("a".repeat(32), "b".repeat(64), 1, "c".repeat(64))
                .unwrap(),
            opened: Default::default(),
            release_open: Default::default(),
            cleaning: Default::default(),
            release_cleanup: Default::default(),
            retired: Default::default(),
            bodies: Default::default(),
        })
    }
    fn constructor_allocation(
        target: std::sync::Arc<Target>,
    ) -> crate::native_source_launch::Allocation {
        crate::native_source_launch::Allocation {
            operation: "f".repeat(32),
            member: std::sync::Arc::new(Peer),
            target: Some(target),
            parent_use: None,
            derived_parent: None,
        }
    }

    #[tokio::test]
    async fn unpolled_destination_constructor_retains_exact_open_cleanup() {
        let target = constructor_target();
        let constructor = crate::native_source_launch::destination_scope(
            constructor_allocation(target.clone()),
            async {},
        );
        drop(constructor);
        target.cleaning.notified().await;
        let bodies = target.bodies.lock().unwrap().clone();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].1, "f".repeat(32));
        assert_eq!(
            bodies[0].2.as_deref(),
            Some("11111111111111111111111111111111")
        );
        target.release_cleanup.notify_one();
        target.retired.notified().await;
    }

    #[tokio::test]
    async fn destination_allocation_precedes_startup_and_cannot_be_consumed_twice() {
        let target = constructor_target();
        let allocation = crate::native_source_launch::destination_scope(
            constructor_allocation(target.clone()),
            async {
                let actual = crate::native_source_launch::allocate_manager()
                    .await
                    .unwrap()
                    .unwrap();
                assert!(actual.target.is_some());
                assert!(actual.parent_use.is_none());
                assert!(
                    crate::native_source_launch::allocate_manager()
                        .await
                        .is_err()
                );
                actual
            },
        )
        .await;
        retire_unused(allocation.value);
        target.cleaning.notified().await;
        target.release_cleanup.notify_one();
        target.retired.notified().await;
    }
    #[tokio::test]
    async fn unused_constructor_returns_only_actual_open_cleanup_receipt() {
        let target = constructor_target();
        let constructed = crate::native_source_launch::destination_scope(
            constructor_allocation(target.clone()),
            async { 42 },
        )
        .await;
        assert_eq!(constructed.value, 42);
        assert!(constructed.receipt.retirement_observer().is_err());
        target.cleaning.notified().await;
        let closed = constructed.receipt.closed();
        tokio::pin!(closed);
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(
                closed.as_mut().poll(cx).is_pending()
            ))
            .await
        );
        target.release_cleanup.notify_one();
        let proof = closed.await.unwrap();
        assert!(constructed.receipt.matches_closed(&target.context, &proof));
        assert!(matches!(proof, ConstructionClosed::Unloaded(_)));
    }
    #[tokio::test]
    async fn actual_constructor_member_observer_waits_for_last_lifetime_then_closed() {
        let target = constructor_target();
        let constructed = crate::native_source_launch::destination_scope(
            constructor_allocation(target.clone()),
            async {
                let allocation = crate::native_source_launch::allocate_manager()
                    .await
                    .unwrap()
                    .unwrap();
                crate::native_source_lifetime::Lifetime::new(allocation).unwrap()
            },
        )
        .await;
        assert!(constructed.receipt.retirement_observer().is_ok());
        let closed = constructed.receipt.closed();
        tokio::pin!(closed);
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(
                closed.as_mut().poll(cx).is_pending()
            ))
            .await
        );
        let last_use = constructed.value.clone();
        drop(constructed.value);
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(
                closed.as_mut().poll(cx).is_pending()
            ))
            .await
        );
        drop(last_use);
        target.cleaning.notified().await;
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(
                closed.as_mut().poll(cx).is_pending()
            ))
            .await
        );
        target.release_cleanup.notify_one();
        let proof = closed.await.unwrap();
        assert!(constructed.receipt.matches_closed(&target.context, &proof));
        assert!(matches!(proof, ConstructionClosed::Member(_)));
    }
    #[tokio::test]
    async fn quiescent_destination_retires_without_waiting_for_idle_metadata_contexts() {
        let target = constructor_target();
        let domain = std::sync::Arc::new(crate::assignment::Domain::default());
        let constructed = crate::native_source_launch::destination_scope(
            constructor_allocation(target.clone()),
            async {
                let allocation = crate::native_source_launch::allocate_manager()
                    .await
                    .unwrap()
                    .unwrap();
                crate::native_source_lifetime::Lifetime::new(allocation).unwrap()
            },
        )
        .await;
        constructed.value.attach_domain(domain.clone()).unwrap();
        let resource = domain.admit_resource(None).unwrap();
        assert!(constructed.receipt.retire_quiescent(&domain).is_err());
        drop(resource);
        let idle_metadata = constructed.value.clone();
        constructed.receipt.retire_quiescent(&domain).unwrap();
        target.cleaning.notified().await;
        assert!(!idle_metadata.ready());
        target.release_cleanup.notify_one();
        let proof = constructed.receipt.closed().await.unwrap();
        assert!(constructed.receipt.matches_closed(&target.context, &proof));
        assert!(matches!(proof, ConstructionClosed::Member(_)));
        drop(idle_metadata);
        drop(constructed.value);
    }
    #[tokio::test]
    async fn cancelled_target_observer_keeps_original_open_until_correlated_cleanup() {
        let target = std::sync::Arc::new(Target {
            context: Context::from_native("a".repeat(32), "b".repeat(64), 1, "c".repeat(64))
                .unwrap(),
            opened: Default::default(),
            release_open: Default::default(),
            cleaning: Default::default(),
            release_cleanup: Default::default(),
            retired: Default::default(),
            bodies: Default::default(),
        });
        let destination = target.clone();
        let observer = tokio::spawn(async move { allocate(destination, &"f".repeat(32)).await });
        target.opened.notified().await;
        observer.abort();
        let _ = observer.await;
        target.release_open.notify_one();
        target.cleaning.notified().await;
        let bodies = target.bodies.lock().unwrap().clone();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].1, "f".repeat(32));
        assert_eq!(
            bodies[0].2.as_deref(),
            Some("11111111111111111111111111111111")
        );
        assert_ne!(
            bodies[0].0, bodies[0].1,
            "cleanup retains a separate semantic operation"
        );
        target.release_cleanup.notify_one();
        target.retired.notified().await;
    }
}
