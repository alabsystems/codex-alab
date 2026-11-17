// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! SAME-owner scheduling controls only. No fixture issues Native/SDK/UI
//! authority or treats the test callback as an actual selection commit.
use super::*;
use std::future::Future;
use std::future::poll_fn;
use std::task::Poll;

type TestResult = Result<(), String>;

#[tokio::test]
async fn candidate_inhibitor_survives_original_native_hold_release() -> TestResult {
    let (owner, gate, initial) = Owner::new_held();
    let candidate = owner.inhibit().await?;
    assert!(candidate.same_gate(&gate) && candidate.is_held());
    owner.release_if(&initial, || Ok(())).await?;
    assert!(
        gate.admit().await.is_none(),
        "Native Hold release bypassed candidate custody"
    );
    owner.release_inhibitor_if(&candidate, || Ok(())).await?;
    assert!(!candidate.is_held());
    assert!(gate.admit().await.is_some());
    Ok(())
}

#[tokio::test]
async fn later_candidate_drains_actual_read_window_before_original_worker_entry() -> TestResult {
    let (owner, gate, initial) = Owner::new_held();
    owner.release_if(&initial, || Ok(())).await?;
    let accepted = gate.admit().await.ok_or("original window was denied")?;
    let mut pending = Box::pin(owner.inhibit());
    assert!(
        poll_fn(|cx| Poll::Ready(pending.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(
        gate.admit().await.is_none(),
        "new automatic reader bypassed pending drain"
    );
    drop(accepted);
    let candidate = pending.await?;
    assert!(candidate.is_held() && gate.admit().await.is_none());
    assert!(
        owner
            .release_inhibitor_if(&candidate, || Err(
                "actual selection still unobserved".into()
            ))
            .await
            .is_err()
    );
    assert!(candidate.is_held() && gate.admit().await.is_none());
    owner.release_inhibitor_if(&candidate, || Ok(())).await?;
    assert!(gate.admit().await.is_some());
    Ok(())
}

#[tokio::test]
async fn independent_candidates_and_native_replacement_each_keep_their_own_inhibitor() -> TestResult
{
    let (owner, gate, initial) = Owner::new_held();
    let first = owner.inhibit().await?;
    let second = owner.inhibit().await?;
    owner.release_if(&initial, || Ok(())).await?;
    let replacement = owner.hold().await?;
    owner.release_inhibitor_if(&first, || Ok(())).await?;
    assert!(!first.is_held() && second.is_held() && gate.admit().await.is_none());
    owner.release_inhibitor_if(&second, || Ok(())).await?;
    assert!(
        gate.admit().await.is_none(),
        "candidate release opened another operation's Native Hold"
    );
    owner.release_if(&replacement, || Ok(())).await?;
    assert!(gate.admit().await.is_some());
    Ok(())
}

#[tokio::test]
async fn foreign_and_removed_source_tickets_never_enter_release_check() -> TestResult {
    let (owner, gate, initial) = Owner::new_held();
    let (foreign_owner, _, _) = Owner::new_held();
    let foreign = foreign_owner.inhibit().await?;
    assert!(
        owner
            .release_inhibitor_if(&foreign, || panic!("foreign source ticket reached check"))
            .await
            .is_err()
    );
    let actual = owner.inhibit().await?;
    owner.release_if(&initial, || Ok(())).await?;
    owner.release_inhibitor_if(&actual, || Ok(())).await?;
    assert!(
        owner
            .release_inhibitor_if(&actual, || panic!("removed source ticket reached check"))
            .await
            .is_err()
    );
    assert!(!actual.is_held() && gate.admit().await.is_some());
    Ok(())
}

#[tokio::test]
async fn accepted_ticket_drop_does_not_reopen_admission() -> TestResult {
    let (owner, gate, initial) = Owner::new_held();
    owner.release_if(&initial, || Ok(())).await?;
    let accepted = owner.inhibit().await?;
    drop(accepted);
    assert!(
        gate.admit().await.is_none(),
        "lost accepted Source custody reopened scheduling"
    );
    owner.withdraw();
    assert!(gate.admit().await.is_none());
    Ok(())
}

#[tokio::test]
async fn canceled_unaccepted_wait_and_actual_owner_withdrawal_preserve_original_boundaries()
-> TestResult {
    let (owner, gate, initial) = Owner::new_held();
    owner.release_if(&initial, || Ok(())).await?;
    let reader = gate.admit().await.ok_or("original reader unavailable")?;
    let mut waiting = Box::pin(owner.inhibit());
    assert!(
        poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    drop(waiting); // No ticket was issued and no candidate entered Core.
    drop(reader);
    assert!(gate.admit().await.is_some());
    let accepted = owner.inhibit().await?;
    owner.withdraw();
    assert!(!accepted.is_held() && gate.admit().await.is_none());
    assert!(
        owner
            .release_inhibitor_if(&accepted, || panic!("withdrawn owner reached check"))
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn selection_releases_exact_source_ticket_and_initial_hold_in_one_check() -> TestResult {
    let (owner, gate, initial) = Owner::new_held();
    let selected = owner.inhibit().await?;
    let checks = std::sync::atomic::AtomicUsize::new(0);
    owner
        .release_selection_if(&selected, Some(&initial), || {
            checks.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
        .await?;
    assert_eq!(checks.load(Ordering::Relaxed), 1);
    assert!(!selected.is_held() && gate.admit().await.is_some());
    assert!(
        owner
            .release_if(&initial, || panic!("retired hold reached check"))
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn selection_none_preserves_native_hold_and_unrelated_source_ticket() -> TestResult {
    let (owner, gate, initial) = Owner::new_held();
    let selected = owner.inhibit().await?;
    let other = owner.inhibit().await?;
    owner
        .release_selection_if(&selected, None, || Ok(()))
        .await?;
    assert!(!selected.is_held() && other.is_held() && gate.admit().await.is_none());
    owner
        .release_selection_if(&other, Some(&initial), || Ok(()))
        .await?;
    assert!(gate.admit().await.is_some());
    let later = owner.inhibit().await?;
    owner.release_selection_if(&later, None, || Ok(())).await?;
    assert!(gate.admit().await.is_some());
    Ok(())
}

#[tokio::test]
async fn selection_foreign_stale_resources_never_check_or_partially_release() -> TestResult {
    let (owner, gate, initial) = Owner::new_held();
    let selected = owner.inhibit().await?;
    let (foreign_owner, _, foreign_hold) = Owner::new_held();
    let foreign_ticket = foreign_owner.inhibit().await?;
    assert!(
        owner
            .release_selection_if(&selected, Some(&foreign_hold), || panic!(
                "foreign hold reached check"
            ))
            .await
            .is_err()
    );
    assert!(
        owner
            .release_selection_if(&foreign_ticket, Some(&initial), || panic!(
                "foreign ticket reached check"
            ))
            .await
            .is_err()
    );
    assert!(selected.is_held() && gate.admit().await.is_none());
    owner.release_if(&initial, || Ok(())).await?;
    let replacement = owner.hold().await?;
    assert!(
        owner
            .release_selection_if(&selected, Some(&initial), || panic!(
                "stale epoch reached check"
            ))
            .await
            .is_err()
    );
    assert!(selected.is_held() && gate.admit().await.is_none());
    owner
        .release_selection_if(&selected, Some(&replacement), || Ok(()))
        .await?;
    assert!(gate.admit().await.is_some());
    Ok(())
}

#[tokio::test]
async fn selection_failed_check_or_withdrawal_leaves_both_resources_closed() -> TestResult {
    let (owner, gate, initial) = Owner::new_held();
    let selected = owner.inhibit().await?;
    assert!(
        owner
            .release_selection_if(&selected, Some(&initial), || Err(
                "UI selection not observed".into()
            ))
            .await
            .is_err()
    );
    assert!(selected.is_held() && gate.admit().await.is_none());
    owner.release_inhibitor_if(&selected, || Ok(())).await?;
    assert!(
        owner
            .release_selection_if(&selected, Some(&initial), || panic!(
                "removed ticket reached check"
            ))
            .await
            .is_err()
    );
    assert!(
        gate.admit().await.is_none(),
        "stale ticket release partially opened Native Hold"
    );
    let selected = owner.inhibit().await?;
    assert!(
        owner
            .release_selection_if(&selected, Some(&initial), || {
                owner.withdraw();
                Ok(())
            })
            .await
            .is_err()
    );
    assert!(gate.admit().await.is_none());
    // The failed callback did not remove either scheduling resource.
    let state = owner.gate.0.state.read().await;
    assert!(state.held && state.inhibitors.contains_key(&selected.ticket.id));
    Ok(())
}
