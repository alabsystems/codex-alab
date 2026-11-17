// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::Invocation;
use super::ScopeFuture;
use super::current;
use crate::goal_persistence::State;
use crate::model_goal_stops::CommittedMutation;
use crate::model_goal_stops::CommittedStop;
use crate::model_goal_stops::PreviousStatus;
use crate::model_goal_stops::committed;
use crate::model_goal_stops::take;
use crate::session::WallGoal;
use pretty_assertions::assert_eq;
use std::future::Future;
use std::future::pending;
use std::future::poll_fn;
use std::sync::Arc;
use std::task::Poll;

const TURN: &str = "reused-turn";
const CALL: &str = "reused-provider-id";
const MODE_EPOCH: u64 = 7;

fn acknowledged(state: &Arc<State>, previous_status: PreviousStatus) -> CommittedMutation {
    CommittedMutation {
        transition: state
            .begin_mutation()
            .commit_mutation()
            .expect("an acknowledged native State mutation"),
        stop: Some(CommittedStop {
            goal: WallGoal {
                goal_id: "original-goal".into(),
                objective: "finish the original task".into(),
                token_budget: Some(500),
                chronology_fence_generation: None,
                turn: TURN.into(),
                person_stops: 0,
            },
            previous_status,
        }),
        mode_epoch: MODE_EPOCH,
    }
}

#[test]
fn cloned_invocations_match_without_reusing_another_invocations_identity() {
    let first = Invocation::new();
    let same = first.clone();
    let separate = Invocation::new();
    assert_eq!(first, same);
    assert!(first != separate);
    assert!(current().is_none());
}

#[tokio::test]
async fn scopes_follow_polling_and_restore_the_owning_scope_after_a_nested_wait() {
    let outer = Invocation::new();
    let inner = Invocation::new();
    let mut work = Box::pin(outer.scope(async {
        assert_eq!(current(), Some(outer.clone()));
        tokio::task::yield_now().await;
        assert_eq!(current(), Some(outer.clone()));
        inner
            .scope(async {
                assert_eq!(current(), Some(inner.clone()));
                tokio::task::yield_now().await;
                assert_eq!(current(), Some(inner.clone()));
            })
            .await;
        assert_eq!(current(), Some(outer.clone()));
    }));
    assert!(current().is_none(), "constructing a future enters no scope");
    let first_poll = poll_fn(|cx| Poll::Ready(work.as_mut().poll(cx))).await;
    assert!(first_poll.is_pending(), "the scoped work really yielded");
    assert!(current().is_none(), "a pending poll restores its caller");
    work.await;
    assert!(current().is_none(), "completion restores its caller too");
}

#[tokio::test]
async fn the_future_adapter_carries_the_same_identity_into_a_spawned_task() {
    let invocation = Invocation::new();
    let expected = invocation.clone();
    let work = async move {
        assert_eq!(current(), Some(expected.clone()));
        tokio::task::yield_now().await;
        assert_eq!(current(), Some(expected));
    }
    .with_model_goal_invocation(invocation);
    assert!(current().is_none());
    tokio::spawn(work)
        .await
        .expect("the adapted future is Send and keeps its scope");
    assert!(current().is_none());
}

#[tokio::test]
async fn concurrent_invocations_with_identical_textual_keys_keep_their_own_receipts() {
    const THREAD: &str = "invocation-concurrent-same-key";
    let state = Arc::new(State::default());
    let original = acknowledged(&state, PreviousStatus::Active);
    let duplicate = acknowledged(&state, PreviousStatus::Blocked);
    let (first_ready, first_entered) = tokio::sync::oneshot::channel();
    let (second_ready, second_entered) = tokio::sync::oneshot::channel();
    let (first_release, first_wait) = tokio::sync::oneshot::channel();
    let (second_release, second_wait) = tokio::sync::oneshot::channel();
    let first_stop = original.clone();
    let second_stop = duplicate.clone();
    let first = tokio::spawn(async move {
        Invocation::new()
            .scope(async {
                committed(THREAD, TURN, CALL, first_stop);
                first_ready.send(()).expect("first observer exists");
                first_wait.await.expect("first completion is released");
                take(THREAD, TURN, CALL)
            })
            .await
    });
    let second = tokio::spawn(async move {
        Invocation::new()
            .scope(async {
                committed(THREAD, TURN, CALL, second_stop);
                second_ready.send(()).expect("second observer exists");
                second_wait.await.expect("second completion is released");
                take(THREAD, TURN, CALL)
            })
            .await
    });
    first_entered
        .await
        .expect("first native marker was retained");
    second_entered
        .await
        .expect("second native marker was retained under the same strings");
    second_release.send(()).expect("second scope is alive");
    assert_eq!(
        second.await.expect("second task completed"),
        Some(duplicate)
    );
    first_release.send(()).expect("first scope is still alive");
    assert_eq!(first.await.expect("first task completed"), Some(original));
    assert!(current().is_none());
}

#[tokio::test]
async fn unscoped_or_unrelated_completion_cannot_consume_a_scoped_commit() {
    const THREAD: &str = "invocation-unscoped-completion";
    let state = Arc::new(State::default());
    let stop = acknowledged(&state, PreviousStatus::Active);
    let invocation = Invocation::new();
    invocation
        .scope(async { committed(THREAD, TURN, CALL, stop.clone()) })
        .await;
    assert!(take(THREAD, TURN, CALL).is_none());
    assert!(
        Invocation::new()
            .scope(async { take(THREAD, TURN, CALL) })
            .await
            .is_none()
    );
    assert_eq!(
        invocation.scope(async { take(THREAD, TURN, CALL) }).await,
        Some(stop)
    );
    assert!(
        invocation
            .scope(async { take(THREAD, TURN, CALL) })
            .await
            .is_none(),
        "the exact completion consumes its marker once"
    );
}

#[tokio::test]
async fn an_unscoped_commit_cannot_create_authority_for_a_later_invocation() {
    const THREAD: &str = "invocation-unscoped-commit";
    let state = Arc::new(State::default());
    committed(
        THREAD,
        TURN,
        CALL,
        acknowledged(&state, PreviousStatus::Active),
    );
    assert!(take(THREAD, TURN, CALL).is_none());
    assert!(
        Invocation::new()
            .scope(async { take(THREAD, TURN, CALL) })
            .await
            .is_none()
    );
}

#[tokio::test]
async fn a_spawned_task_inherits_no_scope_without_an_explicit_clone() {
    let invocation = Invocation::new();
    invocation
        .scope(async {
            let inherited = tokio::spawn(async { current() })
                .await
                .expect("the independent task finishes");
            assert!(inherited.is_none());
            let child = invocation.clone();
            let explicit = tokio::spawn(async move { child.scope(async { current() }).await })
                .await
                .expect("the explicitly scoped task finishes");
            assert_eq!(explicit, Some(invocation.clone()));
            assert_eq!(current(), Some(invocation.clone()));
        })
        .await;
    assert!(current().is_none());
}

#[tokio::test]
async fn an_outer_abort_callback_can_reuse_only_the_aborted_dispatchs_exact_scope() {
    const THREAD: &str = "invocation-post-commit-abort";
    let state = Arc::new(State::default());
    let stop = acknowledged(&state, PreviousStatus::Active);
    let invocation = Invocation::new();
    let handler_scope = invocation.clone();
    let handler_stop = stop.clone();
    let (ready, entered) = tokio::sync::oneshot::channel();
    let handler = tokio::spawn(async move {
        handler_scope
            .scope(async {
                committed(THREAD, TURN, CALL, handler_stop);
                ready.send(()).expect("the outer cancellation owner exists");
                pending::<()>().await;
            })
            .await;
    });
    entered
        .await
        .expect("the native stop committed before the post-hook wait");
    handler.abort();
    assert!(
        handler
            .await
            .expect_err("the handler was aborted")
            .is_cancelled()
    );
    assert!(take(THREAD, TURN, CALL).is_none());
    assert!(
        Invocation::new()
            .scope(async { take(THREAD, TURN, CALL) })
            .await
            .is_none(),
        "an unrelated aborted call has no committed authority"
    );
    let callback = async { take(THREAD, TURN, CALL) }.with_model_goal_invocation(invocation);
    assert_eq!(callback.await, Some(stop));
    assert!(current().is_none());
}

#[tokio::test]
async fn dropping_an_outer_future_cannot_lend_its_marker_to_a_fresh_reused_call() {
    const THREAD: &str = "invocation-dropped-outer";
    let state = Arc::new(State::default());
    let stop = acknowledged(&state, PreviousStatus::Active);
    let old = Invocation::new();
    let mut outer = Box::pin(old.scope(async {
        committed(THREAD, TURN, CALL, stop.clone());
        pending::<()>().await;
    }));
    assert!(
        poll_fn(|cx| Poll::Ready(outer.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    drop(outer);
    assert!(current().is_none());
    assert!(take(THREAD, TURN, CALL).is_none());
    assert!(
        Invocation::new()
            .scope(async { take(THREAD, TURN, CALL) })
            .await
            .is_none(),
        "the same provider strings cannot recreate the dropped scope"
    );
    assert_eq!(
        old.scope(async { take(THREAD, TURN, CALL) }).await,
        Some(stop),
        "only the retained exact owner can consume the old marker"
    );
}
