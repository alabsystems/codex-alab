// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Poll callers independently of the actual connection worker. Later replies on
//! that same connection prove the old proof is ready without resuming its caller.

use super::*;
use pretty_assertions::assert_eq;
use std::future::Future;
use std::future::poll_fn;
use std::pin::Pin;
use std::task::Poll;

async fn poll_once<F: Future>(mut future: Pin<&mut F>) -> Poll<F::Output> {
    poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await
}

async fn finish<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(60), future)
        .await
        .expect("the claim transition finishes without reentering its gate")
}

async fn drive_to_gate<F: Future>(mut future: Pin<&mut F>, fake: &FakeAterm, needle: &str) {
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            assert!(poll_once(future.as_mut()).await.is_pending());
            if fake.gated_requests().iter().any(|line| line.contains(needle)) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the actual request reached its gate");
}

#[tokio::test(flavor = "current_thread")]
async fn release_cannot_be_overwritten_by_an_already_answered_claim_proof() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    let clone = c.clone();
    c.harness_policy().await.expect("policy");
    fake.gate("status");
    let mut claim = Box::pin(tokio::task::unconstrained(c.claim()));
    drive_to_gate(claim.as_mut(), &fake, " status").await;
    let mut release = Box::pin(tokio::task::unconstrained(clone.release()));
    assert!(poll_once(release.as_mut()).await.is_pending());
    assert!(!clone.holds_claim());
    fake.open_gate("status");
    // This later reply proves the old proving status has reached its oneshot.
    // On the unfenced implementation the unset has also completed by now.
    c.whoami().await.expect("reply after the old proving read");
    assert_eq!(finish(claim).await.expect("claim completes"), ClaimOutcome::Unproven);
    finish(release).await.expect("release completes after claim cleanup");
    assert!(!c.holds_claim() && !clone.holds_claim());
    assert!(fake.claim().is_none());
    assert_eq!(clone.harness_act("continue").await.unwrap(), ActOutcome::NotHeld);
    assert!(fake.acts().is_empty());
    assert_eq!(c.claim().await.unwrap(), ClaimOutcome::Held { changed: true });
    c.release().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_local_loss_invalidates_an_already_answered_claim_proof() {
    for foreign in [false, true] {
        let (_dir, fake) = fake(Mode::Current).await;
        let c = client(&fake).await;
        let clone = c.clone();
        c.harness_policy().await.expect("policy");
        fake.gate("status");
        let mut claim = Box::pin(tokio::task::unconstrained(c.claim()));
        drive_to_gate(claim.as_mut(), &fake, " status").await;
        fake.open_gate("status");
        c.whoami().await.expect("old proof reply is ready");
        fake.clear_claim();
        if foreign {
            fake.claim_as("driver@lead");
        }
        clone.note_claim_lost();
        assert_eq!(finish(claim).await.unwrap(), ClaimOutcome::Unproven);
        assert!(!c.holds_claim() && !clone.holds_claim());
        assert_eq!(
            fake.claim().map(|claim| claim.holder),
            foreign.then(|| "driver@lead".to_string())
        );
        assert_eq!(clone.harness_act("continue").await.unwrap(), ActOutcome::NotHeld);
        assert!(fake.acts().is_empty());
        if foreign {
            fake.clear_claim();
        }
        assert_eq!(c.claim().await.unwrap(), ClaimOutcome::Held { changed: true });
        c.release().await.unwrap();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn release_revokes_an_in_flight_reconfirmation_before_it_can_authorize() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = held(&fake).await;
    let clone = c.clone();
    fake.gate("meta set supervisor");
    let mut reconfirm = Box::pin(tokio::task::unconstrained(c.reconfirm()));
    drive_to_gate(reconfirm.as_mut(), &fake, "meta set supervisor").await;
    let mut release = Box::pin(tokio::task::unconstrained(clone.release()));
    assert!(poll_once(release.as_mut()).await.is_pending());
    assert!(!clone.holds_claim());
    fake.open_gate("meta set supervisor");
    c.whoami().await.expect("reconfirmation reply is ready");
    assert!(!finish(reconfirm).await.unwrap());
    finish(release).await.unwrap();
    assert!(!c.holds_claim() && !clone.holds_claim());
    assert!(fake.claim().is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn a_reconfirmation_that_retakes_a_locally_lost_claim_is_given_back() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = held(&fake).await;
    fake.gate("meta set supervisor");
    let mut reconfirm = Box::pin(tokio::task::unconstrained(c.reconfirm()));
    drive_to_gate(reconfirm.as_mut(), &fake, "meta set supervisor").await;
    fake.clear_claim();
    c.note_claim_lost();
    fake.open_gate("meta set supervisor");
    c.whoami().await.expect("the admitted set has answered");
    assert_eq!(fake.claim().unwrap().holder, c.holder());
    assert!(!finish(reconfirm).await.unwrap());
    assert!(!c.holds_claim());
    assert!(fake.claim().is_none(), "stale acknowledgement cannot retain its retake");
}

#[tokio::test(flavor = "current_thread")]
async fn failed_proving_status_releases_without_reentering_the_transition_gate() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    c.harness_policy().await.unwrap();
    fake.reply_once(&format!("@{} status", fake.sid()), "OK schema=invalid\n");
    let result = tokio::time::timeout(Duration::from_secs(60), c.claim())
        .await
        .expect("malformed proof cleanup does not deadlock");
    assert!(matches!(result, Err(Error::Malformed(_))));
    assert!(!c.holds_claim());
    assert!(fake.claim().is_none());
    assert_eq!(c.claim().await.unwrap(), ClaimOutcome::Held { changed: true });
    c.release().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn close_wakes_queued_claim_transitions_before_a_buffered_proof_resumes() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    let clone = c.clone();
    c.harness_policy().await.unwrap();
    fake.gate("status");
    let mut older = Box::pin(tokio::task::unconstrained(c.claim()));
    drive_to_gate(older.as_mut(), &fake, " status").await;
    fake.open_gate("status");
    c.whoami().await.expect("the old claim proof is buffered");
    let mut queued_claim = std::pin::pin!(tokio::task::unconstrained(clone.claim()));
    let mut queued_reconfirm = std::pin::pin!(tokio::task::unconstrained(clone.reconfirm()));
    let mut queued_release = std::pin::pin!(tokio::task::unconstrained(clone.release()));
    assert!(poll_once(queued_claim.as_mut()).await.is_pending());
    assert!(poll_once(queued_reconfirm.as_mut()).await.is_pending());
    assert!(poll_once(queued_release.as_mut()).await.is_pending());

    c.close();
    assert!(matches!(
        poll_once(queued_claim.as_mut()).await,
        Poll::Ready(Err(Error::Closed))
    ));
    assert!(matches!(
        poll_once(queued_reconfirm.as_mut()).await,
        Poll::Ready(Err(Error::Closed))
    ));
    assert!(matches!(
        poll_once(queued_release.as_mut()).await,
        Poll::Ready(Err(Error::Closed))
    ));
    assert_eq!(finish(older).await.unwrap(), ClaimOutcome::Unproven);
    assert!(!c.holds_claim() && !clone.holds_claim());
    assert_eq!(c.policy(), None);
    finish(c.closed()).await;
}

// These ordinary meta claims belong to their connection: abandoning an
// effectful transaction retires it, and actual EOF releases that ownership.
// Host-owned native handoff custody may survive EOF for authorized re-adoption;
// these fixtures neither model nor erase that independently held custody.
// Read-only requests and claim callers still waiting for the gate are controls:
// abandoning either must leave the shared connection healthy.
#[tokio::test(flavor = "current_thread")]
async fn dropping_a_claim_with_a_buffered_proof_releases_its_retained_connection() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    let clone = c.clone();
    c.harness_policy().await.unwrap();
    fake.gate("status");
    // Drop the owned future itself, rather than just a pinned reference to it.
    let mut claim = Box::pin(tokio::task::unconstrained(c.claim()));
    drive_to_gate(claim.as_mut(), &fake, " status").await;
    fake.open_gate("status");
    c.whoami().await.expect("the proof reply reached its oneshot");
    assert_eq!(fake.claim().unwrap().holder, c.holder());
    assert!(!c.holds_claim() && !clone.holds_claim());

    drop(claim);
    assert!(
        !c.is_open() && !clone.is_open(),
        "abandoned proof retires every clone's connection"
    );
    finish(c.closed()).await;
    assert!(
        fake.wait_for(|host| host.claim().is_none()).await,
        "actual socket EOF releases native ownership"
    );
    assert_eq!(
        clone.harness_act("continue").await.unwrap(),
        ActOutcome::NotHeld
    );
    assert!(fake.acts().is_empty());

    let fresh = client(&fake).await;
    fresh.harness_policy().await.unwrap();
    assert_eq!(
        fresh.claim().await.unwrap(),
        ClaimOutcome::Held { changed: true }
    );
    fresh.release().await.unwrap();
    assert!(fresh.is_open());
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_a_claim_with_a_buffered_set_reply_releases_its_retained_connection() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    let clone = c.clone();
    c.harness_policy().await.unwrap();
    fake.gate("meta set supervisor");
    let mut claim = Box::pin(tokio::task::unconstrained(c.claim()));
    drive_to_gate(claim.as_mut(), &fake, "meta set supervisor").await;
    fake.open_gate("meta set supervisor");
    c.whoami()
        .await
        .expect("the claim set reply reached its oneshot");
    assert_eq!(fake.claim().unwrap().holder, c.holder());
    assert!(!c.holds_claim() && !clone.holds_claim());

    drop(claim);
    assert!(
        !c.is_open() && !clone.is_open(),
        "a claim set already took native ownership"
    );
    finish(c.closed()).await;
    assert!(
        fake.wait_for(|host| host.claim().is_none()).await,
        "actual socket EOF releases the abandoned set"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_a_reconfirmation_with_a_buffered_retake_releases_its_connection() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = held(&fake).await;
    let clone = c.clone();
    fake.gate("meta set supervisor");
    let mut reconfirm = Box::pin(tokio::task::unconstrained(c.reconfirm()));
    drive_to_gate(reconfirm.as_mut(), &fake, "meta set supervisor").await;
    fake.clear_claim();
    clone.note_claim_lost();
    fake.open_gate("meta set supervisor");
    c.whoami()
        .await
        .expect("the retaking set reply reached its oneshot");
    assert_eq!(fake.claim().unwrap().holder, c.holder());
    assert!(!c.holds_claim() && !clone.holds_claim());

    drop(reconfirm);
    assert!(
        !c.is_open() && !clone.is_open(),
        "abandoned reconfirmation cannot retain its retake"
    );
    finish(c.closed()).await;
    assert!(
        fake.wait_for(|host| host.claim().is_none()).await,
        "actual socket EOF releases the abandoned retake"
    );
    assert_eq!(
        clone.harness_act("continue").await.unwrap(),
        ActOutcome::NotHeld
    );
    assert!(fake.acts().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_a_claim_waiting_for_the_gate_keeps_the_active_transaction_healthy() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    let clone = c.clone();
    c.harness_policy().await.unwrap();
    fake.gate("status");
    let mut active = std::pin::pin!(tokio::task::unconstrained(c.claim()));
    drive_to_gate(active.as_mut(), &fake, " status").await;
    let mut waiting = Box::pin(tokio::task::unconstrained(clone.claim()));
    assert!(poll_once(waiting.as_mut()).await.is_pending());
    drop(waiting);
    assert!(c.is_open() && clone.is_open());

    fake.open_gate("status");
    c.whoami()
        .await
        .expect("the active transaction's proof is buffered");
    assert_eq!(
        finish(active).await.unwrap(),
        ClaimOutcome::Held { changed: true }
    );
    assert!(c.holds_claim() && clone.holds_claim());
    c.release().await.unwrap();
    assert!(c.is_open() && clone.is_open());
    assert!(fake.claim().is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_a_policy_read_with_a_buffered_answer_keeps_the_claim_connection_healthy() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = held(&fake).await;
    let clone = c.clone();
    fake.gate("harness policy");
    let mut read = Box::pin(tokio::task::unconstrained(c.harness_policy()));
    drive_to_gate(read.as_mut(), &fake, " harness policy").await;
    fake.open_gate("harness policy");
    c.whoami().await.expect("the policy reply reached its oneshot");
    drop(read);
    assert!(c.is_open() && clone.is_open());
    assert!(c.holds_claim() && clone.holds_claim());
    assert!(matches!(
        clone.harness_policy().await.unwrap(),
        PolicyAnswer::Supported(_)
    ));
    assert!(clone.reconfirm().await.unwrap());
    c.release().await.unwrap();
    assert!(c.is_open() && clone.is_open());
    assert!(fake.claim().is_none());
}
