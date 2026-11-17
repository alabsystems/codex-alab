// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Poll the caller separately from the real connection worker. A later reply
//! proves that a policy answer is buffered without resuming its original caller.

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
        .expect("the policy read settles without retaining another caller's gate")
}

async fn drive_to_policy_gate<F: Future>(mut future: Pin<&mut F>, fake: &FakeAterm) {
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            assert!(poll_once(future.as_mut()).await.is_pending());
            if fake
                .gated_requests()
                .iter()
                .any(|line| line.contains(" harness policy"))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the actual policy request reached its gate");
}

fn supported(answer: PolicyAnswer) -> crate::HarnessPolicy {
    let PolicyAnswer::Supported(policy) = answer else {
        panic!("the real fake host supports harness policy");
    };
    policy
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_policy_reads_retain_the_latest_five_hour_recovery_bound() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    let clone = c.clone();
    fake.gate("harness policy");
    let mut older = Box::pin(tokio::task::unconstrained(c.harness_policy()));
    drive_to_policy_gate(older.as_mut(), &fake).await;
    fake.open_gate("harness policy");
    c.whoami().await.expect("the old policy answer is buffered");
    fake.set_policy_key(crate::account_recovery::FIVE_HOUR_HANDOFF, "1800");

    let mut newer = Box::pin(tokio::task::unconstrained(clone.harness_policy()));
    assert!(poll_once(newer.as_mut()).await.is_pending());
    c.whoami().await.expect("later work on the same connection settles");
    // Without serialization this caller can finish first. With serialization
    // it waits until the older transaction returns; both schedules must retain
    // the newest actual policy rather than the last caller to resume.
    let early_newer = match poll_once(newer.as_mut()).await {
        Poll::Ready(answer) => Some(answer.expect("newer policy")),
        Poll::Pending => None,
    };
    let older = supported(finish(older).await.expect("older policy"));
    let newer = supported(match early_newer {
        Some(answer) => answer,
        None => finish(newer).await.expect("waiting newer policy"),
    });
    assert!(older.generation < newer.generation);
    assert_eq!(crate::account_recovery::five_hour_bound(&older), None);
    assert_eq!(
        crate::account_recovery::five_hour_bound(&newer),
        Some(Duration::from_secs(1800))
    );
    assert_eq!(c.policy(), Some(newer.clone()));
    assert_eq!(clone.policy(), Some(newer));
}

#[tokio::test(flavor = "current_thread")]
async fn forgetting_policy_invalidates_a_buffered_read_before_a_waiting_fresh_read() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    let clone = c.clone();
    c.harness_policy().await.expect("initial supported policy");
    fake.gate("harness policy");
    let mut older = Box::pin(tokio::task::unconstrained(c.harness_policy()));
    drive_to_policy_gate(older.as_mut(), &fake).await;
    fake.open_gate("harness policy");
    c.whoami().await.expect("the old policy answer is buffered");
    clone.forget_policy();
    assert_eq!(c.policy(), None);
    assert!(matches!(c.claim().await.unwrap(), ClaimOutcome::NotPermitted(_)));
    assert!(fake.claim().is_none());

    let mut fresh = Box::pin(tokio::task::unconstrained(clone.harness_policy()));
    assert!(poll_once(fresh.as_mut()).await.is_pending());
    let error = finish(older).await.expect_err("forget invalidates the old answer");
    assert_eq!(error.to_string(), "aterm harness policy read was invalidated");
    assert!(matches!(error, Error::PolicyInvalidated));
    assert_eq!(c.policy(), None);
    let fresh = supported(finish(fresh).await.expect("a fresh read is permitted"));
    assert_eq!(c.policy(), Some(fresh));
    assert_eq!(c.claim().await.unwrap(), ClaimOutcome::Held { changed: true });
    c.release().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn closing_keeps_policy_empty_when_a_buffered_read_and_a_waiter_finish() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = held(&fake).await;
    let clone = c.clone();
    fake.gate("harness policy");
    let mut older = Box::pin(tokio::task::unconstrained(c.harness_policy()));
    drive_to_policy_gate(older.as_mut(), &fake).await;
    fake.open_gate("harness policy");
    c.whoami().await.expect("the old policy answer is buffered");
    let mut waiting = Box::pin(tokio::task::unconstrained(clone.harness_policy()));
    assert!(poll_once(waiting.as_mut()).await.is_pending());
    clone.close();
    assert!(!c.is_open() && !clone.holds_claim());
    assert_eq!(c.policy(), None);
    // Close wakes a queued reader even while the old caller remains unpolled.
    assert!(matches!(finish(waiting).await, Err(Error::Closed)));
    let error = finish(older).await.expect_err("close invalidates the old answer");
    assert_eq!(error.to_string(), "aterm harness policy read was invalidated");
    assert!(matches!(error, Error::PolicyInvalidated));
    assert!(matches!(c.harness_policy().await, Err(Error::Closed)));
    assert_eq!(c.policy(), None);
    assert_eq!(clone.policy(), None);
    finish(c.closed()).await;
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_a_policy_read_does_not_block_the_next_fresh_policy() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    fake.gate("harness policy");
    let mut older = Box::pin(tokio::task::unconstrained(c.harness_policy()));
    drive_to_policy_gate(older.as_mut(), &fake).await;
    drop(older);
    fake.set_policy_key(crate::account_recovery::FIVE_HOUR_HANDOFF, "900");
    fake.open_gate("harness policy");
    let fresh = supported(finish(c.harness_policy()).await.expect("fresh read after drop"));
    assert_eq!(
        crate::account_recovery::five_hour_bound(&fresh),
        Some(Duration::from_secs(900))
    );
    assert_eq!(c.policy(), Some(fresh));
}

#[tokio::test(flavor = "current_thread")]
async fn losing_a_claim_keeps_its_supported_policy_available() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = held(&fake).await;
    let policy = c.policy().expect("held claim has a supported policy");
    c.note_claim_lost();
    assert!(!c.holds_claim());
    assert_eq!(c.policy(), Some(policy));
    c.release().await.unwrap();
}
