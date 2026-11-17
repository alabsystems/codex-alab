use codex_protocol::ThreadId;
use codex_protocol::protocol::validate_thread_goal_objective;
use codex_state::StateRuntime;

pub(super) async fn inherit_thread_goal_snapshot(
    state_db: &StateRuntime,
    source_thread_id: ThreadId,
    target_thread_id: ThreadId,
) -> anyhow::Result<bool> {
    let Some(mut goal) = state_db
        .thread_goals()
        .get_thread_goal(source_thread_id)
        .await?
    else {
        return Ok(false);
    };
    if let Err(err) = validate_thread_goal_objective(&goal.objective) {
        tracing::warn!(%source_thread_id, "skipping invalid inherited thread goal: {err}");
        return Ok(false);
    }

    goal.thread_id = target_thread_id;
    state_db
        .thread_goals()
        .replace_thread_goal_snapshot(&goal)
        .await?;
    state_db.thread_goals().alab_durable_goal_barrier().await?; // ALAB(goal-recovery): the fork cannot restore or acknowledge an inherited goal before its copied row is power-loss durable.
    Ok(true)
}

#[cfg(test)]
#[path = "../../../alab-core/glue/thread_fork_goal_tests.rs"]
mod alab_tests; // ALAB(goal-recovery): exercise fork-copy durability through the real helper without expanding this upstream-owned file.
