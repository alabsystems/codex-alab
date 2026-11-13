// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use std::sync::atomic::AtomicUsize;

struct BackendOwner;
impl Backend for BackendOwner {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn check_target(&self, _: &Target) -> Result<(), Refused> {
        Ok(())
    }
    fn source_stamp(&self) -> Result<crate::assignment::Generations, Refused> {
        Ok(crate::assignment::Generations {
            auth: 0,
            owner: 1,
            source: 0,
            login: 1,
        })
    }
}
fn manager(registry: &Arc<Registry>) -> Arc<Domain> {
    let factory = registry.factory(None).unwrap();
    let domain = factory.domain();
    factory.record().attach(Arc::new(BackendOwner)).unwrap();
    factory.complete().unwrap();
    domain
}

#[test]
fn refused_upgrade_reopens_the_actual_catalog_without_retiring() {
    let registry = Arc::new(Registry::default());
    let domain = manager(&registry);
    let called = AtomicUsize::new(0);
    assert!(
        registry
            .commit_upgrade_if(|| {
                called.fetch_add(1, Ordering::SeqCst);
                false
            })
            .unwrap()
            .is_none()
    );
    assert_eq!(called.load(Ordering::SeqCst), 1);
    assert!(domain.admit_work(None).is_ok());
    assert!(registry.factory(None).unwrap().admitted());
}

#[test]
fn pending_work_or_constructor_never_calls_the_final_decision() {
    let registry = Arc::new(Registry::default());
    let domain = manager(&registry);
    let work = domain.admit_work(None).unwrap();
    assert!(matches!(
        registry.commit_upgrade_if(|| panic!("live work must refuse")),
        Err(Refused::Pending)
    ));
    assert!(
        domain.admit_work(None).is_ok(),
        "refusal must reopen admission"
    );
    drop(work);
    let loading = registry.factory(None).unwrap();
    assert!(matches!(
        registry.commit_upgrade_if(|| panic!("loading constructor must refuse")),
        Err(Refused::Pending)
    ));
    assert!(domain.admit_work(None).is_ok());
    loading.complete().unwrap();
}

#[test]
fn unknown_owner_is_not_an_empty_census_and_refusal_reopens_other_owners() {
    let registry = Arc::new(Registry::default());
    let domain = manager(&registry);
    let loading = registry.factory(None).unwrap();
    drop(loading);
    assert!(matches!(
        registry.commit_upgrade_if(|| panic!("unknown is not retired")),
        Err(Refused::Unavailable)
    ));
    assert!(domain.admit_work(None).is_ok());
}

#[test]
fn unwinding_host_decision_does_not_poison_or_leave_a_pre_effect_seal() {
    let registry = Arc::new(Registry::default());
    let domain = manager(&registry);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = registry.commit_upgrade_if(|| panic!("host decision unwound"));
        }))
        .is_err()
    );
    assert!(domain.admit_work(None).is_ok());
    assert!(registry.commit_upgrade_if(|| false).unwrap().is_none());
}

#[tokio::test]
async fn committed_census_retains_the_observed_domain_until_its_owner_finishes() {
    let registry = Arc::new(Registry::default());
    let domain = manager(&registry);
    let weak = Arc::downgrade(&domain);
    let committed = registry.commit_upgrade_if(|| true).unwrap().unwrap();
    drop(domain);
    assert!(weak.upgrade().is_some());
    committed.drain().await.unwrap();
    drop(committed);
    assert!(weak.upgrade().is_none());
}

struct Retirement(AtomicUsize);
impl Participant for Retirement {
    fn retire<'a>(
        &'a self,
        domain: &'a Arc<Domain>,
        proof: Arc<dyn crate::assignment::RetirementProof>,
    ) -> ResourceFuture<'a> {
        Box::pin(async move {
            assert!(proof.protects(domain));
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

#[tokio::test]
async fn committed_drain_survives_cancelled_waiter_and_waits_for_actual_body() {
    let registry = Arc::new(Registry::default());
    let domain = manager(&registry);
    let participant = Arc::new(Retirement(AtomicUsize::new(0)));
    domain
        .catalog()
        .unwrap()
        .participant(participant.clone())
        .unwrap();
    let body = domain.admit_resource(None).unwrap();
    let committed = registry.commit_upgrade_if(|| true).unwrap().unwrap();
    assert!(domain.admit_work(Some(&body)).is_err());
    assert!(domain.admit_publication().is_err());
    assert!(!registry.factory(Some(&body)).unwrap().admitted());
    let waiter = tokio::spawn({
        let committed = committed.clone();
        async move { committed.drain().await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while participant.0.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        !waiter.is_finished(),
        "a live resource body prevents completion"
    );
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    drop(body);
    tokio::time::timeout(std::time::Duration::from_secs(5), committed.drain())
        .await
        .unwrap()
        .unwrap();
    committed.drain().await.unwrap();
    assert_eq!(participant.0.load(Ordering::SeqCst), 1);
    drop(committed);
    assert!(
        domain.admit_work(None).is_err(),
        "committed closure outlives its token"
    );
}
