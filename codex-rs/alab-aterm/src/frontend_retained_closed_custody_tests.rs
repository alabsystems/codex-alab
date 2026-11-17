// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private Source scheduling/ledger controls only. No public ClosedCustody,
//! PendingHold, Controller, SDK member, Native Go, UI or receipt is fabricated.
//! Genuine Native ownership is required separately by the actual bind/receiver.
use super::*;
use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Poll;

fn ledger(hold: Arc<Hold>) -> State {
    State {
        next: 0,
        initial: Some(hold),
        initial_transferred: false,
        replacements: BTreeMap::new(),
        origins: BTreeMap::new(),
        runtimes: BTreeMap::new(),
        source_selections: BTreeMap::new(),
    }
}
fn accepted(order: u64) -> Arc<AcceptedHold> {
    Arc::new(AcceptedHold {
        order,
        nonce: [b'c'; 32],
        deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(60),
        hold: Mutex::new(None),
        transferred: AtomicBool::new(false),
        phase: watch::channel(Phase::Accepted).0,
    })
}
fn poison() -> io::Error {
    io::Error::other("private Source control mutex poisoned")
}

#[tokio::test]
async fn same_closed_hold_moves_once_and_every_independent_ticket_stays_closed() -> io::Result<()> {
    let (owner, gate, hold) = Owner::new_held();
    let hold = Arc::new(hold);
    let ticket = owner.inhibit().await.map_err(io::Error::other)?;
    let mut state = ledger(hold.clone());
    let target = accepted(1);
    let operation = [b'a'; 32];
    state.replacements.insert(operation, target.clone());
    let dropped = AtomicBool::new(false);
    struct LocalGuard<'a>(&'a AtomicBool);
    impl Drop for LocalGuard<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    move_closed_hold(
        &mut state,
        ClosedMove {
            subject: &Subject::Initial,
            hold: &hold,
            operation: &operation,
            target: &target,
        },
        || Ok(LocalGuard(&dropped)),
        || Ok(()),
    )?;
    assert!(dropped.load(Ordering::Acquire));
    assert!(state.initial.is_none() && state.initial_transferred);
    assert!(*target.phase.borrow() == Phase::Held);
    assert!(
        target
            .hold
            .lock()
            .map_err(|_| poison())?
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &hold))
    );
    assert!(ticket.is_held() && gate.admit().await.is_none());
    assert!(subject_matches(&state, &Subject::Initial, &hold).is_err());
    // A later H1 cannot steal a Hold before its original worker's actual ACK.
    let next = accepted(2);
    let next_operation = [b'b'; 32];
    state.replacements.insert(next_operation, next.clone());
    let subject = Subject::Replacement {
        operation,
        actual: target.clone(),
    };
    assert!(
        move_closed_hold(
            &mut state,
            ClosedMove {
                subject: &subject,
                hold: &hold,
                operation: &next_operation,
                target: &next
            },
            || Ok(()),
            || Ok(())
        )
        .is_err()
    );
    assert!(target.hold.lock().map_err(|_| poison())?.is_some());
    assert!(next.hold.lock().map_err(|_| poison())?.is_none());
    assert!(ticket.is_held() && gate.admit().await.is_none());
    // Private ledger scheduling control only: the production worker alone
    // supplies this phase AFTER its real Native ACK. No SDK grant is tested.
    target.phase.send_replace(Phase::Acknowledged);
    move_closed_hold(
        &mut state,
        ClosedMove {
            subject: &subject,
            hold: &hold,
            operation: &next_operation,
            target: &next,
        },
        || Ok(()),
        || Ok(()),
    )?;
    assert!(target.transferred.load(Ordering::Acquire));
    assert!(target.hold.lock().map_err(|_| poison())?.is_none());
    assert!(
        next.hold
            .lock()
            .map_err(|_| poison())?
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &hold))
    );
    assert!(ticket.is_held() && gate.admit().await.is_none());
    Ok(())
}

#[tokio::test]
async fn changed_source_guard_foreign_record_and_expired_h1_never_move_custody() -> io::Result<()> {
    let (owner, gate, hold) = Owner::new_held();
    let hold = Arc::new(hold);
    let ticket = owner.inhibit().await.map_err(io::Error::other)?;
    let mut state = ledger(hold.clone());
    let target = accepted(1);
    let operation = [b'a'; 32];
    state.replacements.insert(operation, target.clone());
    assert!(
        move_closed_hold(
            &mut state,
            ClosedMove {
                subject: &Subject::Initial,
                hold: &hold,
                operation: &operation,
                target: &target
            },
            || Err::<(), _>(io::Error::other("actual Source slot changed")),
            || Ok(())
        )
        .is_err()
    );
    let foreign = accepted(1);
    assert!(
        move_closed_hold(
            &mut state,
            ClosedMove {
                subject: &Subject::Initial,
                hold: &hold,
                operation: &operation,
                target: &foreign
            },
            || Ok(()),
            || Ok(())
        )
        .is_err()
    );
    let expired = Arc::new(AcceptedHold {
        order: 2,
        nonce: [b'd'; 32],
        deadline: tokio::time::Instant::now() - std::time::Duration::from_secs(1),
        hold: Mutex::new(None),
        transferred: AtomicBool::new(false),
        phase: watch::channel(Phase::Accepted).0,
    });
    let expired_operation = [b'e'; 32];
    state
        .replacements
        .insert(expired_operation, expired.clone());
    assert!(
        move_closed_hold(
            &mut state,
            ClosedMove {
                subject: &Subject::Initial,
                hold: &hold,
                operation: &expired_operation,
                target: &expired
            },
            || Ok(()),
            || Ok(())
        )
        .is_err()
    );
    assert!(
        state
            .initial
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &hold))
            && !state.initial_transferred
    );
    assert!(
        *target.phase.borrow() == Phase::Accepted
            && target.hold.lock().map_err(|_| poison())?.is_none()
    );
    assert!(ticket.is_held() && gate.admit().await.is_none());
    Ok(())
}

#[tokio::test]
async fn cancelling_the_wait_for_shared_mutation_leaves_the_original_closed_owner() -> io::Result<()>
{
    let (owner, gate, hold) = Owner::new_held();
    let hold = Arc::new(hold);
    let ticket = owner.inhibit().await.map_err(io::Error::other)?;
    let target = accepted(1);
    let operation = [b'a'; 32];
    let mut original = ledger(hold.clone());
    original.replacements.insert(operation, target.clone());
    let state = Mutex::new(original);
    let mutation = tokio::sync::Semaphore::new(1);
    let current = mutation.acquire().await.map_err(io::Error::other)?;
    let entered = AtomicBool::new(false);
    let subject = Subject::Initial;
    let mut moving = Box::pin(transfer_closed(
        &mutation,
        &state,
        ClosedMove {
            subject: &subject,
            hold: &hold,
            operation: &operation,
            target: &target,
        },
        || {
            entered.store(true, Ordering::Release);
            Ok(())
        },
        || Ok(()),
    ));
    std::future::poll_fn(|context| {
        assert!(moving.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(moving);
    drop(current);
    assert!(!entered.load(Ordering::Acquire));
    {
        let state = state.lock().map_err(|_| poison())?;
        assert!(
            state
                .initial
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, &hold))
                && !state.initial_transferred
        );
    }
    assert!(
        *target.phase.borrow() == Phase::Accepted
            && target.hold.lock().map_err(|_| poison())?.is_none()
    );
    assert!(ticket.is_held() && gate.admit().await.is_none());
    Ok(())
}

#[tokio::test]
async fn an_original_committed_release_wins_serialization_and_transfer_cannot_recreate_it()
-> io::Result<()> {
    let (owner, gate, hold) = Owner::new_held();
    let hold = Arc::new(hold);
    let ticket = owner.inhibit().await.map_err(io::Error::other)?;
    let target = accepted(1);
    let operation = [b'a'; 32];
    let mut original = ledger(hold.clone());
    original.replacements.insert(operation, target.clone());
    let state = Mutex::new(original);
    let mutation = tokio::sync::Semaphore::new(1);
    let current = mutation.acquire().await.map_err(io::Error::other)?;
    let checked = AtomicBool::new(false);
    let subject = Subject::Initial;
    let mut moving = Box::pin(transfer_closed(
        &mutation,
        &state,
        ClosedMove {
            subject: &subject,
            hold: &hold,
            operation: &operation,
            target: &target,
        },
        || {
            checked.store(true, Ordering::Release);
            Ok(())
        },
        || Ok(()),
    ));
    // The competing custody operation really waits on the SAME SDK lease.
    std::future::poll_fn(|context| {
        assert!(moving.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    // Actual pure Source writer/release, not a Native permission fixture. The
    // real SDK release performs the same immediate ledger retirement afterward.
    owner
        .release_selection_if(&ticket, Some(&hold), || Ok(()))
        .await
        .map_err(io::Error::other)?;
    state.lock().map_err(|_| poison())?.initial.take();
    drop(current);
    let result = moving.await;
    assert!(result.is_err() && !checked.load(Ordering::Acquire));
    assert!(
        target.hold.lock().map_err(|_| poison())?.is_none()
            && *target.phase.borrow() == Phase::Accepted
    );
    assert!(!ticket.is_held());
    assert!(
        gate.admit().await.is_some(),
        "transfer recreated or closed an already released epoch"
    );
    Ok(())
}
