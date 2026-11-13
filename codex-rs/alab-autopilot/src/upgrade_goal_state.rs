// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Strict source data codec. Native SDK, startup policies and continuation
//! admission are independently constructed by the destination, never decoded.
use super::FinalGoalForest;
use super::FinalGoalState;
use chrono::DateTime;
use chrono::Utc;
use codex_alab_core::goal_clock::Stamp;
use codex_core::UpgradeForestSnapshot;
use codex_protocol::ThreadId;
use codex_state::ThreadGoal;
use codex_state::ThreadGoalStatus;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    version: u32,
    primary: ThreadId,
    forest: UpgradeForestSnapshot,
    goals: Vec<Goal>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Goal {
    thread: ThreadId,
    #[serde(deserialize_with = "required")]
    goal: Option<Row>,
    #[serde(deserialize_with = "required")]
    clock: Option<Clock>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    thread_id: ThreadId,
    goal_id: String,
    objective: String,
    status: String,
    #[serde(deserialize_with = "required")]
    token_budget: Option<i64>,
    tokens_used: i64,
    time_used_seconds: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Clock {
    domain: u8,
    seconds: u64,
    nanos: u32,
}

/// Validated data only. These records grant no native execution authority.
pub struct GoalForestState {
    pub forest: Arc<UpgradeForestSnapshot>,
    pub goals: Vec<FinalGoalState>,
}
impl GoalForestState {
    pub fn validate(&self, primary: ThreadId) -> Result<(), String> {
        validate(&self.forest, primary, &self.goals)
    }

    /// The caller obtains primary identity from its original launch routing.
    /// A transport must separately authenticate same-process custody.
    pub fn decode(bytes: &[u8], expected_primary: ThreadId) -> Result<Self, String> {
        if bytes.len() > codex_alab_tui::upgrade_exec_custody::MAX_STATE_BYTES {
            return Err("oversized goal forest state".into());
        }
        let wire: Wire = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        if wire.version != 1 || wire.primary != expected_primary {
            return Err("goal forest changed its version or primary".into());
        }
        let goals = wire
            .goals
            .into_iter()
            .map(Goal::decode)
            .collect::<Result<Vec<_>, _>>()?;
        validate(&wire.forest, expected_primary, &goals)?;
        Ok(Self {
            forest: Arc::new(wire.forest),
            goals,
        })
    }
}
impl FinalGoalForest {
    /// Call only after final source capture has settled. Encoding these rows is
    /// data preparation and neither closes readers nor commits a source fence.
    pub fn encode_state(&self, primary: ThreadId) -> Result<Vec<u8>, String> {
        validate(&self.forest, primary, &self.goals)?;
        let goals = self
            .goals
            .iter()
            .map(|entry| Goal {
                thread: entry.thread,
                goal: entry.goal.as_ref().map(|goal| Row {
                    thread_id: goal.thread_id,
                    goal_id: goal.goal_id.clone(),
                    objective: goal.objective.clone(),
                    status: goal.status.as_str().into(),
                    token_budget: goal.token_budget,
                    tokens_used: goal.tokens_used,
                    time_used_seconds: goal.time_used_seconds,
                    created_at: goal.created_at,
                    updated_at: goal.updated_at,
                }),
                clock: entry.clock.map(|stamp| {
                    let (domain, seconds, nanos) = stamp.parts();
                    Clock {
                        domain,
                        seconds,
                        nanos,
                    }
                }),
            })
            .collect();
        let bytes = serde_json::to_vec(&Wire {
            version: 1,
            primary,
            forest: self.forest.as_ref().clone(),
            goals,
        })
        .map_err(|error| error.to_string())?;
        if bytes.len() > codex_alab_tui::upgrade_exec_custody::MAX_STATE_BYTES {
            return Err("oversized goal forest state".into());
        }
        Ok(bytes)
    }
}
impl Goal {
    fn decode(self) -> Result<FinalGoalState, String> {
        let goal = self
            .goal
            .map(|row| -> Result<ThreadGoal, String> {
                Ok(ThreadGoal {
                    thread_id: row.thread_id,
                    goal_id: row.goal_id,
                    objective: row.objective,
                    status: ThreadGoalStatus::try_from(row.status.as_str())
                        .map_err(|error| error.to_string())?,
                    token_budget: row.token_budget,
                    tokens_used: row.tokens_used,
                    time_used_seconds: row.time_used_seconds,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                })
            })
            .transpose()?;
        let clock = self
            .clock
            .map(|clock| {
                Stamp::from_parts(clock.domain, clock.seconds, clock.nanos).map_err(str::to_owned)
            })
            .transpose()?;
        Ok(FinalGoalState {
            thread: self.thread,
            goal,
            clock,
        })
    }
}
fn validate(
    forest: &UpgradeForestSnapshot,
    primary: ThreadId,
    goals: &[FinalGoalState],
) -> Result<(), String> {
    for entry in goals {
        if entry
            .goal
            .as_ref()
            .is_some_and(|goal| goal.thread_id != entry.thread)
            || (entry.clock.is_some()
                && !entry
                    .goal
                    .as_ref()
                    .is_some_and(|goal| goal.status.is_active()))
        {
            return Err("goal row or clock differs from its original member".into());
        }
    }
    // Core validates complete inventory, duplicates, parent graph, root order
    // and the single global input/publication table as one forest.
    forest.validate(
        primary,
        &goals.iter().map(|entry| entry.thread).collect::<Vec<_>>(),
    )
}

// An omitted Option field is malformed, not an explicit absence checkpoint.
fn required<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer)
}
