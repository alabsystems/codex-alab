// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// The real client observes the matching saturated counter, so a mismatch cannot hide a
/// missing exhaustion guard. Only this private fixture jumps over the unreachable count.
#[tokio::test]
async fn an_exhausted_confirmation_stays_open_and_never_wraps() {
    let dir = tempfile::tempdir().expect("scratch fixture");
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .expect("fake aterm");
    println!(
        "retained exhausted confirmation fixture {}",
        dir.keep().display()
    );
    let client = crate::Client::connect(fake.endpoint(), crate::Options::default())
        .await
        .expect("client");
    let positive = "notice later limit tag=codex Needs setup -- Needs the owner -- Finish setup";
    assert!(
        client
            .request(positive)
            .await
            .expect("initial positive")
            .ok()
            .is_some()
    );
    let semantic_revision = {
        let mut state = lock(&fake.state);
        state.digest.get_mut("limit").expect("entry").confirmation = u64::MAX - 1;
        state.digest_revision
    };
    let observed = match client
        .observe_open_later("limit", "codex")
        .await
        .expect("observe")
    {
        crate::ObserveLaterOutcome::Open(observed) => observed,
        outcome => panic!("expected an open entry: {outcome:?}"),
    };
    for _ in 0..2 {
        assert!(
            client
                .request(positive)
                .await
                .expect("positive again")
                .ok()
                .is_some()
        );
        let state = lock(&fake.state);
        assert_eq!(
            state.digest.get("limit").expect("entry").confirmation,
            u64::MAX
        );
        assert_eq!(state.digest_revision, semantic_revision);
    }
    assert_eq!(
        client
            .settle_observed_later(&observed)
            .await
            .expect("old observation"),
        crate::EndOutcome::NotOpen
    );
    let current = match client
        .observe_open_later("limit", "codex")
        .await
        .expect("observe MAX")
    {
        crate::ObserveLaterOutcome::Open(observed) => observed,
        outcome => panic!("expected the saturated open entry: {outcome:?}"),
    };
    assert_eq!(
        client
            .settle_observed_later(&current)
            .await
            .expect("matching MAX observation"),
        crate::EndOutcome::NotOpen
    );
    assert_eq!(
        lock(&fake.state)
            .digest
            .get("limit")
            .expect("still open")
            .confirmation,
        u64::MAX
    );
    assert!(fake.dones().is_empty());
    assert_eq!(fake.settles().len(), 2);
}
