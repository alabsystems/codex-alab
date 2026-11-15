// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use codex_core::TurnInputRequest;
use codex_protocol::protocol::AdditionalContextEntry;
use codex_protocol::protocol::AdditionalContextKind;
use codex_protocol::protocol::ThreadGoal;

pub(crate) fn continuation_request_with_policy(
    goal: &ThreadGoal,
    update_plan_enabled: bool,
    policy: Option<String>,
) -> TurnInputRequest {
    let request = super::continuation_request(goal, update_plan_enabled);
    let Some(policy) = policy else {
        return request;
    };
    request.with_additional_context(
        [
            (
                "alab_goal_objective".to_string(),
                AdditionalContextEntry::new(
                    goal.objective.clone(),
                    AdditionalContextKind::Application,
                ),
            ),
            (
                "alab_policy_continuation".to_string(),
                AdditionalContextEntry::new(policy, AdditionalContextKind::Application),
            ),
        ]
        .into(),
    )
}
