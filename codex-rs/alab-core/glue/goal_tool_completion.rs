// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native goal dispatch completion belongs to its thread even when the sampling
//! task is forcibly destroyed. Preserve the existing abort/join/notify future;
//! a claimed terminal outcome is not proof that every observer has received it.

use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_alab_core::retained_future::Abandonments;
use codex_alab_core::retained_future::Lease;
use codex_alab_core::retained_future::Owner;
use codex_alab_core::retained_future::Retained;
use std::future::Future;
use tokio_util::either::Either;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct GoalCompletions(Owner);

#[derive(Default)]
struct GoalAbandonments(Abandonments);

pub(crate) struct Prepared {
    retained: Option<(Lease, Abandonments)>,
    cancellation: CancellationToken,
}

/// Register before dispatch (including ID reservation) so shutdown cannot miss
/// a call whose completion future has not been constructed or polled yet.
pub(crate) fn prepare(
    session: &Session,
    turn: &TurnContext,
    name: &codex_tools::ToolName,
    cancellation: CancellationToken,
) -> Result<(Prepared, CancellationToken), FunctionCallError> {
    let covered = codex_alab_core::is_alab_binary()
        && name.is_default_namespace()
        && matches!(
            name.name.as_str(),
            "get_goal" | "create_goal" | "update_goal"
        );
    let retained = if covered {
        let owner = session
            .services
            .thread_extension_data
            .get_or_init(GoalCompletions::default);
        let lease = owner.0.register().map_err(|_| {
            FunctionCallError::RespondToModel("goal tool runtime is shutting down".to_owned())
        })?;
        let abandoned = turn.extension_data.get_or_init(GoalAbandonments::default);
        Some((lease, abandoned.0.clone()))
    } else {
        None
    };
    // Dropping one wait must never cancel the turn or another CodeMode call.
    let cancellation = if covered {
        cancellation.child_token()
    } else {
        cancellation
    };
    Ok((
        Prepared {
            retained,
            cancellation: cancellation.clone(),
        },
        cancellation,
    ))
}

impl Prepared {
    pub(crate) fn retain<F>(self, completion: F) -> Either<F, Retained<F>>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        match self.retained {
            Some((lease, abandoned)) => Either::Right(codex_alab_core::retained_future::retain(
                completion,
                lease,
                abandoned,
                move || self.cancellation.cancel(),
            )),
            None => Either::Left(completion),
        }
    }
}

/// Called after the sampling task has been destroyed and joined. Intentional
/// live CodeMode waiters do not count as abandoned and retain their semantics.
pub(crate) async fn settle_turn(turn: &TurnContext) {
    if let Some(abandoned) = turn.extension_data.get::<GoalAbandonments>() {
        abandoned.0.wait().await;
    }
}

/// The existing already-cancelled branch otherwise drops its task only after
/// publishing TurnAbort. Join first so Drop transfers are visible to the drain.
pub(crate) async fn settle_cancelled_task(task: &mut crate::state::RunningTask) {
    if codex_alab_core::is_alab_binary() {
        task.handle.abort();
        let _ = (&mut task.handle).await;
        settle_turn(&task.turn_context).await;
    }
}

pub(crate) fn close(session: &Session) {
    if codex_alab_core::is_alab_binary() {
        session
            .services
            .thread_extension_data
            .get_or_init(GoalCompletions::default)
            .0
            .close();
    }
}

/// Producers have stopped; keep the goal runtime and its persistence writer
/// alive until all admitted completions (including transferred waits) settle.
pub(crate) async fn settle_thread(session: &Session) {
    if let Some(owner) = session
        .services
        .thread_extension_data
        .get::<GoalCompletions>()
    {
        owner.0.close_and_wait().await;
    }
}
