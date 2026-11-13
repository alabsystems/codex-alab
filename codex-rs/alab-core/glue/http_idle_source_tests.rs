// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_alab_core::assignment::Generations;
use codex_alab_core::assignment::Refused;
use codex_alab_core::assignment_service::Target;
use codex_alab_core::manager_catalog::Backend;
use codex_alab_core::manager_catalog::Registry;
use codex_alab_core::manager_catalog::Scope;
use codex_alab_core::manager_catalog::Version;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Poll;

struct Owner(tokio::sync::watch::Sender<Generations>);

impl Version for Owner {
    fn current(&self) -> Generations {
        self.0.borrow().clone()
    }

    fn changed<'a>(
        &'a self,
        observed: &'a Generations,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let mut changes = self.0.subscribe();
            let _ = changes.wait_for(|current| current != observed).await;
        })
    }
}

impl Backend for Owner {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn check_target(&self, _: &Target) -> Result<(), Refused> {
        Ok(())
    }

    fn source_stamp(&self) -> Result<Generations, Refused> {
        Ok(self.current())
    }
}

struct Fixture {
    registry: Arc<Registry>,
    owner: Arc<Owner>,
    policy: NetworkPolicy,
    url: Url,
}

fn fixture() -> Fixture {
    let registry = Arc::new(Registry::default());
    let factory = registry.factory(None).unwrap();
    let domain = factory.domain();
    let owner = Arc::new(Owner(
        tokio::sync::watch::channel(Generations {
            auth: 1,
            owner: 1,
            source: 1,
            login: 1,
        })
        .0,
    ));
    factory.record().attach(owner.clone()).unwrap();
    let record = factory.complete().unwrap();
    assert_eq!(
        record.phase(),
        Ok(codex_alab_core::manager_catalog::Phase::Ready)
    );
    assert!(record.backend().unwrap().is_some());
    let scope = Scope::capture(domain, owner.clone()).unwrap();
    Fixture {
        registry,
        owner,
        policy: NetworkPolicy::unmanaged().with_manager_source(Ok(scope)),
        url: Url::parse("https://example.invalid/").unwrap(),
    }
}

fn poll_once<F: Future + ?Sized>(future: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
}

fn assert_drains(registry: &Arc<Registry>) {
    let seal = registry.seal().unwrap();
    let mut drain = std::pin::pin!(seal.drain_resources());
    assert_eq!(poll_once(drain.as_mut()), Poll::Ready(Ok(())));
}

#[tokio::test]
async fn idle_recovery_counts_only_the_polled_episode_and_releases_on_cancel() {
    // Draining is an irreversible public lifecycle transition, so each state
    // gets its own catalog instead of reopening one merely to inspect counters.
    let retained = fixture();
    let _idle = retained.policy.alab_idle_permit(&retained.url).unwrap();
    assert_drains(&retained.registry);

    for polled in [false, true] {
        let fixture = fixture();
        let idle = fixture.policy.alab_idle_permit(&fixture.url).unwrap();
        idle.check().unwrap();
        let entered = AtomicBool::new(false);
        let mut episode = Box::pin(idle.run(async {
            entered.store(true, Ordering::Release);
            std::future::pending::<()>().await;
        }));
        if polled {
            assert_eq!(poll_once(episode.as_mut()), Poll::Pending);
        }
        assert_eq!(entered.load(Ordering::Acquire), polled);
        let seal = fixture.registry.seal().unwrap();
        let mut drain = std::pin::pin!(seal.drain_resources());
        assert_eq!(
            poll_once(drain.as_mut()),
            if polled {
                Poll::Pending
            } else {
                Poll::Ready(Ok(()))
            },
            "only a polled episode delays the public resource drain"
        );
        drop(episode);
        if polled {
            assert_eq!(
                poll_once(drain.as_mut()),
                Poll::Ready(Ok(())),
                "cancellation releases episode custody"
            );
        }
    }

    let fixture = fixture();
    let idle = fixture.policy.alab_idle_permit(&fixture.url).unwrap();
    assert_eq!(idle.run(async { 7 }).await.unwrap(), 7);
    assert_drains(&fixture.registry);
}

#[tokio::test]
async fn idle_recovery_cannot_adopt_a_new_source_with_the_same_owner_and_login() {
    let fixture = fixture();
    let idle = fixture.policy.alab_idle_permit(&fixture.url).unwrap();
    let mut changed = fixture.owner.current();
    changed.source += 1;
    fixture.owner.0.send_replace(changed);
    assert_eq!(idle.check(), Err(NetworkPolicyDenied::Unavailable));
    let entered = AtomicBool::new(false);
    assert_eq!(
        idle.run(async { entered.store(true, Ordering::Release) })
            .await,
        Err(NetworkPolicyDenied::Unavailable)
    );
    assert!(!entered.load(Ordering::Acquire));
    assert_drains(&fixture.registry);
}

#[tokio::test]
async fn idle_recovery_source_retirement_cancels_its_active_episode() {
    let fixture = fixture();
    let idle = fixture.policy.alab_idle_permit(&fixture.url).unwrap();
    let mut episode = Box::pin(idle.run(std::future::pending::<()>()));
    assert_eq!(poll_once(episode.as_mut()), Poll::Pending);
    let mut changed = fixture.owner.current();
    changed.owner += 1;
    fixture.owner.0.send_replace(changed);
    assert_eq!(
        poll_once(episode.as_mut()),
        Poll::Ready(Err(NetworkPolicyDenied::Revoked))
    );
    drop(episode);
    assert_drains(&fixture.registry);
}

#[tokio::test]
async fn idle_recovery_preserves_sticky_application_revocation() {
    let controller = NetworkPolicyController::default();
    let policy = controller.policy();
    assert!(controller.publish(policy.revision(), DestinationPolicy::Unrestricted));
    let url = Url::parse("https://example.invalid/").unwrap();
    let idle = policy.alab_idle_permit(&url).unwrap();
    controller.unavailable(policy.revision());
    assert!(controller.publish(policy.revision(), DestinationPolicy::Unrestricted));
    assert!(
        policy.acquire(&url).is_ok(),
        "the newly allowed policy is usable"
    );
    assert_eq!(idle.check(), Err(NetworkPolicyDenied::Revoked));
    let entered = AtomicBool::new(false);
    assert_eq!(
        idle.run(async { entered.store(true, Ordering::Release) })
            .await,
        Err(NetworkPolicyDenied::Revoked)
    );
    assert!(!entered.load(Ordering::Acquire));
}

#[test]
fn idle_recovery_refuses_missing_source_and_disallowed_destination() {
    let url = Url::parse("https://example.invalid/").unwrap();
    assert!(matches!(
        NetworkPolicy::unmanaged()
            .with_native_source(Err(()))
            .alab_idle_permit(&url),
        Err(NetworkPolicyDenied::Unavailable)
    ));
    let policy =
        NetworkPolicy::unmanaged().restrict_to_endpoints(std::collections::BTreeSet::new());
    assert!(matches!(
        policy.alab_idle_permit(&url),
        Err(NetworkPolicyDenied::Destination)
    ));
}
