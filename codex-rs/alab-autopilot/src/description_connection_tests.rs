// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual description operations retain publication custody until their own
//! request worker has dropped its socket, rather than merely receiving close().

use std::future::Future;
use std::future::poll_fn;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use codex_alab_aterm::Client;
use codex_alab_aterm::Options;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_core::session::Shared;

use super::DescriptionReceipt;
use super::SessionEnd;
use super::Supervisor;
use super::SupervisorOptions;

/// Poll the real operation on the same runtime thread as its independent Conn
/// worker. The worker cannot close the socket during this synchronous poll.
async fn retirement_boundary(
    sup: &Supervisor,
    operation: impl Future<Output = ()>,
    mut observed: tokio::sync::oneshot::Receiver<Client>,
) -> (bool, bool, bool) {
    tokio::pin!(operation);
    let client = tokio::time::timeout(Duration::from_secs(60), async {
        poll_fn(|cx| {
            assert!(operation.as_mut().poll(cx).is_pending());
            std::pin::Pin::new(&mut observed).poll(cx)
        })
        .await
    })
    .await
    .expect("actual auxiliary client is connected")
    .expect("client observer");
    let mut waiting = Box::pin(sup.description_sync.acquire());
    assert!(poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx).is_pending())).await);
    let (returned, closed, admitted) = tokio::time::timeout(Duration::from_secs(60), async {
        let closed = client.closed();
        tokio::pin!(closed);
        poll_fn(|cx| {
            let returned = operation.as_mut().poll(cx).is_ready();
            if !client.is_open() {
                let closed = closed.as_mut().poll(cx).is_ready();
                let admitted = waiting.as_mut().poll(cx).is_ready();
                Poll::Ready((returned, closed, admitted))
            } else {
                assert!(!returned, "description operation retires its client");
                Poll::Pending
            }
        })
        .await
    })
    .await
    .expect("auxiliary retirement reaches close");
    // The queued permit cannot obstruct teardown: only the actual connection
    // worker is awaited while the old publication owns the semaphore.
    drop(waiting);
    if !returned {
        tokio::time::timeout(Duration::from_secs(60), operation)
            .await
            .expect("auxiliary worker closes before the operation finishes");
    }
    tokio::time::timeout(Duration::from_secs(60), client.closed())
        .await
        .expect("actual auxiliary socket closes");
    (returned, closed, admitted)
}

async fn auxiliary_retirement(withdrawal: bool) {
    let dir = tempfile::tempdir().expect("scratch aterm");
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .expect("fake aterm");
    let endpoint = fake.endpoint();
    let shared = Box::leak(Box::new(Shared::new()));
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new({
            let endpoint = endpoint.clone();
            move || Some(endpoint.clone())
        }),
        client: Options::default(),
    });
    let (tui, mut changes) = tokio::sync::watch::channel(0);
    let session = {
        let sup = Arc::clone(&sup);
        let endpoint = endpoint.clone();
        tokio::spawn(async move {
            let mut tries = 0;
            sup.session(endpoint, &mut changes, &mut tries, None).await
        })
    };
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let ready = {
                let st = sup.lock();
                st.acting() && !st.attention_unknown() && !st.needs_unknown()
            };
            if ready {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("actual claim settles");
    sup.describe(Some("plan: the acknowledged task".to_owned()))
        .await;
    let receipt = DescriptionReceipt::of(&sup.lock()).expect("acknowledged description");
    assert_eq!(fake.description().as_deref(), Some(receipt.text.as_str()));
    assert!(sup.want_description(None));
    let (observer, observed) = tokio::sync::oneshot::channel();
    *sup.description_connection.lock().expect("observer slot") = Some(observer);
    let result = if withdrawal {
        retirement_boundary(
            &sup,
            sup.withdraw_description_at(endpoint, Options::default(), receipt),
            observed,
        )
        .await
    } else {
        retirement_boundary(&sup, sup.publish_description(), observed).await
    };
    let removed = fake.description();
    sup.describe(Some("plan: the fresh task".to_owned())).await;
    let fresh = fake.description();
    drop(tui);
    let ended = tokio::time::timeout(Duration::from_secs(60), session)
        .await
        .expect("scratch session ends")
        .expect("scratch session task");
    assert!(matches!(ended, SessionEnd::Closing));
    let (returned, closed, admitted) = result;
    assert_eq!(removed, None, "the actual auxiliary unset applied");
    assert_eq!(fresh.as_deref(), Some("plan: the fresh task"));
    assert!(
        (!returned && !admitted) || closed,
        "withdrawal={withdrawal}: operation returned={returned}, publisher admitted={admitted}, socket closed={closed}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn auxiliary_description_publication_closes_its_socket_before_admitting_a_publisher() {
    auxiliary_retirement(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn auxiliary_description_withdrawal_closes_its_socket_before_admitting_a_publisher() {
    auxiliary_retirement(true).await;
}
