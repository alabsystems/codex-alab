// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::sync::Arc;

/// A GATE HOLDS THE REQUEST AFTER ITS PASSES, AND LETS GO OF WHAT IT HOLDS WITHOUT OPENING. A
/// caller that sends several like requests in a row is held at the one a test means
/// ([`FakeAterm::gate_after`]), and one that sends one per attempt has each attempt held in
/// turn ([`FakeAterm::release_held`]): the request after a release meets the gate closed.
#[tokio::test]
async fn a_gate_passes_its_count_and_releases_what_it_holds_while_it_stays_closed() {
    let (_directory, fake) = fake().await;
    let client = Arc::new(
        Client::connect(fake.endpoint(), Options::default())
            .await
            .expect("authenticated client"),
    );
    let ask = || {
        let client = Arc::clone(&client);
        tokio::spawn(async move { client.status().await.map(|_| ()) })
    };
    fake.gate_after("status", 2);
    for _ in 0..2 {
        ask().await.expect("joined").expect("a pass is answered");
    }
    assert!(fake.gated_requests().is_empty(), "the passes met no gate");

    let third = ask();
    assert!(fake.wait_for(|host| host.gated_requests().len() == 1).await);
    assert!(!third.is_finished(), "the request after the passes is held");
    fake.release_held("status");
    third
        .await
        .expect("joined")
        .expect("the held one is answered");

    let fourth = ask();
    assert!(fake.wait_for(|host| host.gated_requests().len() == 2).await);
    assert!(!fourth.is_finished(), "the gate stayed closed behind it");
    fake.open_gate("status");
    fourth.await.expect("joined").expect("the gate opened");
    ask().await.expect("joined").expect("and stays open");
    assert_eq!(fake.gated_requests().len(), 2);
}
