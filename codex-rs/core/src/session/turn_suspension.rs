use super::handlers;
use super::session::Session;
use crate::state::TaskKind;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::turn_input::SuspendTurnOutcome;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

pub(super) async fn suspend_turn_and_shutdown(
    session: &Arc<Session>,
    submission_id: String,
) -> CodexResult<SuspendTurnOutcome> {
    {
        let active = session.active_turn.lock().await;
        let Some(task) = active.as_ref().and_then(|turn| turn.task.as_ref()) else {
            return Ok(SuspendTurnOutcome::NotActive);
        };
        if task.kind != TaskKind::Regular && !crate::tasks::review_recovery::suspension_ready(task.kind, &task.turn_context) { // ALAB(exec-review): only an acknowledged frozen review may join regular suspension.
            return Ok(SuspendTurnOutcome::UnsupportedTask);
        }
    }

    // ALAB(exec-ephemeral): retain the durable barrier or validate canonical memory before stopping.
    let persistence = super::alab_exec_recovery_options::ephemeral::prepare_suspension(session).await?;

    // The flush can yield while the active turn completes or changes. Recheck its
    // kind under the same lock used to remove it.
    let mut turn = {
        let mut active = session.active_turn.lock().await;
        let Some(active_turn) = active.as_ref() else {
            return Ok(SuspendTurnOutcome::NotActive);
        };
        let Some(task) = active_turn.task.as_ref() else {
            return Ok(SuspendTurnOutcome::NotActive);
        };
        if task.kind != TaskKind::Regular && !crate::tasks::review_recovery::suspension_ready(task.kind, &task.turn_context) { // ALAB(exec-review): recheck the same frozen consumer before removing its task.
            return Ok(SuspendTurnOutcome::UnsupportedTask);
        }
        active.take().ok_or_else(|| {
            CodexErr::Fatal("accepted root turn suspension had no running turn".to_string())
        })?
    };

    let task = turn.task.take().ok_or_else(|| {
        CodexErr::Fatal("accepted root turn suspension had no running task".to_string())
    })?;
    let turn_id = task.turn_context.sub_id.clone();
    // Normal shutdown records a terminal turn event, preventing another worker from
    // recovering this turn under its original ID. Cancel the task without that event.
    super::alab_exec_recovery_options::transfer::mark(&task.turn_context); // ALAB(exec-transfer): distinguish an account handoff from a user abort.
    task.cancellation_token.cancel();
    task.turn_context
        .turn_metadata_state
        .cancel_git_enrichment_task();
    let mut task_handle = task.handle.detach();
    match tokio::time::timeout(
        Duration::from_millis(crate::tasks::GRACEFULL_INTERRUPTION_TIMEOUT_MS),
        &mut task_handle,
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            warn!(thread_id = %session.thread_id, %error, "suspended turn task exited abnormally");
        }
        Err(_) => {
            warn!(
                thread_id = %session.thread_id,
                "suspended turn task did not stop gracefully; aborting it"
            );
            task_handle.abort();
            let _ = task_handle.await;
        }
    }
    // Pending accepted input and interactive waiters live only in this process. Input
    // is retained for native exec; other callers retain the ordinary shutdown behavior.
    super::input_queue::exec_input_state::freeze(session, &turn, &turn_id).await; // ALAB(exec-input): keep accepted data before the removed turn is dropped.

    // The root task has stopped without a terminal event. Fence new starts and stop every
    // other local session, even if the suspension caller disconnects during cleanup.
    // The host owns the separate tree-cleanup wait and any persistence-writer fencing.
    session.services.local_agent_runtime.request_shutdown();

    // Stop all producers before flushing their final history and closing its writer.
    // If either persistence step fails, do not report success: the current worker
    // retains ownership until worker-failure recovery can take responsibility.
    handlers::shutdown_session_runtime(session).await;
    persistence.finish(&turn_id).await?; // ALAB(exec-ephemeral): stopped producers retain the same unfinished turn.
    // Announce completion only after extension cleanup and writer closure so a
    // replacement worker cannot write the same thread concurrently.
    session
        .deliver_event_raw(Event {
            id: submission_id,
            msg: EventMsg::ShutdownComplete,
        })
        .await;
    Ok(SuspendTurnOutcome::Suspended { turn_id })
}
