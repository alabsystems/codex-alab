// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;

struct ObserveStateLock {
    client: Client,
    wakes: AtomicUsize,
    locked: AtomicBool,
}

impl ObserveStateLock {
    fn observe(&self) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
        if self.client.shared.state.try_lock().is_err() {
            self.locked.store(true, Ordering::SeqCst);
        }
    }
}

impl Wake for ObserveStateLock {
    fn wake(self: Arc<Self>) {
        self.observe();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.observe();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn close_wakes_external_readers_without_holding_the_state_lock() {
    let directory = tempfile::tempdir().unwrap();
    let fake = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
        .await
        .unwrap();
    let client = Client::connect(fake.endpoint(), Options::default())
        .await
        .unwrap();
    let _policy_transaction = client.shared.policy_gate.acquire().await.unwrap();
    let _claim_transaction = client.shared.claim_gate.acquire().await.unwrap();
    let observer = Arc::new(ObserveStateLock {
        client: client.clone(),
        wakes: AtomicUsize::new(0),
        locked: AtomicBool::new(false),
    });
    let waker = Waker::from(Arc::clone(&observer));
    let mut context = Context::from_waker(&waker);
    let mut policy = std::pin::pin!(tokio::task::unconstrained(client.harness_policy()));
    let mut claim = std::pin::pin!(tokio::task::unconstrained(client.claim()));
    let mut reconfirm = std::pin::pin!(tokio::task::unconstrained(client.reconfirm()));
    assert!(policy.as_mut().poll(&mut context).is_pending());
    assert!(claim.as_mut().poll(&mut context).is_pending());
    assert!(reconfirm.as_mut().poll(&mut context).is_pending());

    client.close();
    assert!(observer.wakes.load(Ordering::SeqCst) > 0);
    // The legal waker observes with try_lock, so a broken close fails without
    // blocking the closer, creating a thread or leaving a hung fixture behind.
    assert!(!observer.locked.load(Ordering::SeqCst));
    assert!(matches!(
        policy.as_mut().poll(&mut context),
        Poll::Ready(Err(Error::Closed))
    ));
    assert!(matches!(
        claim.as_mut().poll(&mut context),
        Poll::Ready(Err(Error::Closed))
    ));
    assert!(matches!(
        reconfirm.as_mut().poll(&mut context),
        Poll::Ready(Err(Error::Closed))
    ));
    assert!(!client.is_open());
    assert!(client.policy().is_none());
}
