// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

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

use super::SessionEnd;
use super::Supervisor;
use super::SupervisorOptions;

/// Poll the actual session on one runtime thread. Once it sends close(), its
/// independent Conn worker cannot drop the stream until this poll yields; this
/// makes the close notification / actual socket closure boundary deterministic.
#[tokio::test(flavor = "current_thread")]
async fn subscription_loss_waits_for_the_old_request_socket_before_reconnect() {
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
    let (_tui, mut changes) = tokio::sync::watch::channel(0);
    let mut tries = 0;
    let session = sup.session(endpoint, &mut changes, &mut tries, None);
    tokio::pin!(session);
    tokio::time::timeout(Duration::from_secs(60), async {
        poll_fn(|cx| {
            assert!(
                session.as_mut().poll(cx).is_pending(),
                "session remains live"
            );
            let st = sup.lock();
            if st.acting() && !st.attention_unknown() && !st.needs_unknown() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    })
    .await
    .expect("initial claim settles");
    let client = sup.client().expect("request client");
    fake.end_subscriptions();
    let (returned, ended) = tokio::time::timeout(Duration::from_secs(60), async {
        let ended = client.closed();
        tokio::pin!(ended);
        poll_fn(|cx| {
            let returned = session.as_mut().poll(cx).is_ready();
            if !client.is_open() {
                Poll::Ready((returned, ended.as_mut().poll(cx).is_ready()))
            } else {
                assert!(!returned, "subscription exit closes its request client");
                Poll::Pending
            }
        })
        .await
    })
    .await
    .expect("subscription loss reaches client retirement");
    // Finish teardown before asserting the regression result. On the fixed
    // path this await lets the Conn task drop its actual socket and wake session.
    if !returned {
        let end = tokio::time::timeout(Duration::from_secs(60), session)
            .await
            .expect("old connection closes");
        assert!(matches!(end, SessionEnd::Retry(_)));
    }
    tokio::time::timeout(Duration::from_secs(60), client.closed())
        .await
        .expect("request worker ends");
    assert!(
        !returned || ended,
        "session returned to its reconnect loop before the old request socket actually closed"
    );
}

/// A rejected push subscription must retire the request socket too, even
/// though that session never reached the claimed-state event loop.
#[tokio::test(flavor = "current_thread")]
async fn subscription_refusal_waits_for_the_request_socket_before_reconnect() {
    let dir = tempfile::tempdir().expect("scratch aterm");
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .expect("fake aterm");
    let mut endpoint = fake.endpoint();
    endpoint.sid = "s-not-a-live-session".to_owned();
    let client = Client::connect(endpoint.clone(), Options::default())
        .await
        .expect("request connection authenticates before the session is selected");
    let shared = Box::leak(Box::new(Shared::new()));
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new({
            let endpoint = endpoint.clone();
            move || Some(endpoint.clone())
        }),
        client: Options::default(),
    });
    let (_tui, mut changes) = tokio::sync::watch::channel(0);
    let mut tries = 0;
    let session = sup.session(endpoint, &mut changes, &mut tries, Some(client.clone()));
    tokio::pin!(session);
    let (returned, ended) = tokio::time::timeout(Duration::from_secs(60), async {
        let ended = client.closed();
        tokio::pin!(ended);
        poll_fn(|cx| {
            let returned = session.as_mut().poll(cx).is_ready();
            if !client.is_open() {
                Poll::Ready((returned, ended.as_mut().poll(cx).is_ready()))
            } else {
                assert!(!returned, "subscription refusal closes its request client");
                Poll::Pending
            }
        })
        .await
    })
    .await
    .expect("subscription refusal reaches client retirement");
    if !returned {
        let end = tokio::time::timeout(Duration::from_secs(60), session)
            .await
            .expect("refused subscription request connection closes");
        assert!(matches!(end, SessionEnd::Retry(_)));
    }
    tokio::time::timeout(Duration::from_secs(60), client.closed())
        .await
        .expect("request worker ends");
    assert!(
        !returned || ended,
        "subscription refusal returned to reconnect before its request socket actually closed"
    );
}
