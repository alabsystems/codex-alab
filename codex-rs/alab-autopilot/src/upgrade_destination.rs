// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native goal restoration immediately before the actual forest release.
use crate::upgrade_goals::GoalForestState;
use codex_alab_core::upgrade_startup_hold::Fence;
use codex_core::CodexThread;
use codex_goal_extension::GoalRuntimeHandle;
use std::collections::HashSet;
use std::sync::Arc;

pub async fn restore_goals(
    threads: &[Arc<CodexThread>],
    hold: &Fence,
    state: Option<&GoalForestState>,
) -> Result<(), String> {
    let actual = threads
        .iter()
        .map(|thread| thread.exec_thread_id())
        .collect::<HashSet<_>>();
    if actual.len() != threads.len() {
        return Err("restored forest repeats an actual goal runtime".into());
    }
    if let Some(state) = state {
        let expected = state
            .goals
            .iter()
            .map(|goal| goal.thread)
            .collect::<HashSet<_>>();
        if actual != expected || expected.len() != state.goals.len() {
            return Err("goal data differs from actual restored forest".into());
        }
    }
    for thread in threads {
        let id = thread.exec_thread_id();
        let runtime = thread
            .thread_extension_data()
            .get::<GoalRuntimeHandle>()
            .ok_or("restored native goal runtime is unavailable")?;
        match state {
            Some(state) => {
                let saved = state
                    .goals
                    .iter()
                    .find(|goal| goal.thread == id)
                    .ok_or("restored goal state is absent")?;
                runtime
                    .alab_restore_forest_goal(hold, id, saved.goal.as_ref(), saved.clock)
                    .await?;
            }
            None => runtime.alab_restore_stopped_forest_goal(hold).await?,
        }
    }
    Ok(())
}
