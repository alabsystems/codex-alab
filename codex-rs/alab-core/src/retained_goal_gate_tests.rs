// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Scheduling controls only: these fixtures issue no Native Go/SDK authority.
use super::*;
use std::future::Future;
use std::future::poll_fn;
use std::task::Poll;

#[tokio::test]
async fn actual_admission_guard_drains_before_replacement_hold() {
    let (owner, gate, first) = Owner::new_held();
    owner.release_if(&first, || Ok(())).await.unwrap();
    let admitted = gate.admit().await.unwrap();
    let mut pause = Box::pin(owner.hold());
    assert!(
        poll_fn(|cx| Poll::Ready(pause.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(gate.admit().await.is_none()); // No reader waits behind its queued writer.
    drop(admitted);
    let hold = pause.await.unwrap();
    assert!(gate.admit().await.is_none());
    assert!(
        owner
            .release_if(&hold, || Err(
                "actual SDK observation remains unresolved".into()
            ))
            .await
            .is_err()
    );
    assert!(gate.admit().await.is_none());
    owner.release_if(&hold, || Ok(())).await.unwrap();
    assert!(gate.admit().await.is_some());
}

#[tokio::test]
async fn foreign_and_stale_holds_never_run_release_callback() {
    let (owner, gate, first) = Owner::new_held();
    let (_, _, foreign) = Owner::new_held();
    assert!(
        owner
            .release_if(&foreign, || panic!("foreign hold cannot enter callback"))
            .await
            .is_err()
    );
    owner.release_if(&first, || Ok(())).await.unwrap();
    let current = owner.hold().await.unwrap();
    assert!(
        owner
            .release_if(&first, || panic!("stale hold cannot enter callback"))
            .await
            .is_err()
    );
    assert!(gate.admit().await.is_none());
    owner.release_if(&current, || Ok(())).await.unwrap();
    assert!(gate.admit().await.is_some());
}

#[tokio::test]
async fn dropping_replacement_observation_or_hold_does_not_reopen_admission() {
    let (owner, gate, first) = Owner::new_held();
    owner.release_if(&first, || Ok(())).await.unwrap();
    let admitted = gate.admit().await.unwrap();
    let mut observer = Box::pin(owner.hold());
    assert!(
        poll_fn(|cx| Poll::Ready(observer.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    drop(observer); // No hold was accepted: original admission is still owned.
    drop(admitted);
    let hold = owner.hold().await.unwrap();
    drop(hold);
    assert!(gate.admit().await.is_none());
    assert!(owner.hold().await.is_err());
}

#[tokio::test]
async fn actual_owner_withdrawal_and_drop_keep_future_automatic_work_inhibited() {
    let (owner, gate, first) = Owner::new_held();
    owner.release_if(&first, || Ok(())).await.unwrap();
    let accepted = gate.admit().await.unwrap();
    owner.withdraw();
    assert!(gate.admit().await.is_none());
    drop(accepted); // Accepted work keeps its original lifetime; no cancellation.
    assert!(owner.hold().await.is_err());
    assert!(
        owner
            .release_if(&first, || panic!("withdrawn owner cannot release"))
            .await
            .is_err()
    );
    drop(owner);
    assert!(gate.admit().await.is_none());
    let (owner, gate, first) = Owner::new_held();
    owner.release_if(&first, || Ok(())).await.unwrap();
    drop(owner);
    assert!(gate.admit().await.is_none());
}

#[tokio::test]
async fn automatic_work_without_a_goal_runtime_keeps_the_same_actual_read_window() {
    let (owner, gate, first) = Owner::new_held();
    owner.release_if(&first, || Ok(())).await.unwrap();
    let accepted = AutomaticPermit::retained_only(gate.admit().await.unwrap());
    let mut pause = Box::pin(owner.hold());
    assert!(
        poll_fn(|cx| Poll::Ready(pause.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    drop(accepted);
    let hold = pause.await.unwrap();
    assert!(gate.admit().await.is_none());
    owner.release_if(&hold, || Ok(())).await.unwrap();
    assert!(gate.admit().await.is_some());
}

#[test]
fn first_actual_start_seals_registration_before_a_root_could_be_published() {
    let legacy = Registration::default();
    assert!(legacy.enter().unwrap().is_none());
    let (_owner, gate, _hold) = Owner::new_held();
    assert!(legacy.bind(gate).is_err());
    let registration = Registration::default();
    let (_owner, gate, _hold) = Owner::new_held();
    registration.bind(gate.clone()).unwrap();
    assert!(registration.enter().unwrap().unwrap().same(&gate));
    registration.bind(gate).unwrap();
    let (_foreign_owner, foreign, _foreign_hold) = Owner::new_held();
    assert!(registration.bind(foreign).is_err());
}
