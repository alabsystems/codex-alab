// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! What the session says it is doing: aterm's `meta description` (CONTRACT §5 visibility).
//!
//! aterm shows a session's description in `sessions`, and a restore carries it, so a tab
//! aterm relaunched still says what it was doing. codex-alab writes the root thread's goal
//! and the plan step in progress (`update_plan`'s one `in_progress` step), one write per
//! change, only while its proven claim is held, and withdraws it when there is nothing left
//! to say, when the claim goes and at the exit. It never writes the title (the person's
//! field, which outranks the program's own) and never a description a person wrote: a
//! standing text is codex-alab's only when it is the one this process wrote, or, when a dead
//! predecessor of the thread left a record, one of [`is_ours`]'s shape. Pure: the supervisor
//! writes ([`crate::Supervisor::describe`]).

use codex_alab_aterm::DESCRIPTION_MAX_BYTES;
use codex_alab_aterm::wire;
use codex_protocol::plan_tool::StepStatus;
use codex_protocol::plan_tool::UpdatePlanArgs;

/// How a description that names the goal begins.
pub const GOAL: &str = "goal: ";
/// How a description that names only the plan step begins.
pub const PLAN: &str = "plan: ";
/// Between the goal and the step in progress.
const NOW: &str = " · now: ";

/// The plan step in progress in an `update_plan` call's arguments: `Some(None)` for a plan
/// with none in progress (all pending, or all done), `None` for arguments that are not a
/// plan (nothing is learned from them).
pub fn plan_step(arguments: &str) -> Option<Option<String>> {
    let args: UpdatePlanArgs = serde_json::from_str(arguments).ok()?;
    Some(
        args.plan
            .into_iter()
            .find(|item| matches!(item.status, StepStatus::InProgress))
            .map(|item| item.step)
            .filter(|step| !step.trim().is_empty()),
    )
}

/// The description for a goal and a plan step, one line of at most aterm's 1024 bytes;
/// `None` when there is neither (nothing to say: the one written is withdrawn).
pub fn describe(goal: Option<&str>, step: Option<&str>) -> Option<String> {
    let goal = goal.map(str::trim).filter(|g| !g.is_empty());
    let step = step.map(str::trim).filter(|s| !s.is_empty());
    let text = match (goal, step) {
        (Some(goal), Some(step)) => format!("{GOAL}{goal}{NOW}{step}"),
        (Some(goal), None) => format!("{GOAL}{goal}"),
        (None, Some(step)) => format!("{PLAN}{step}"),
        (None, None) => return None,
    };
    let text = wire::single_line(&text, DESCRIPTION_MAX_BYTES);
    (!text.is_empty()).then_some(text)
}

/// Whether a standing description has the shape of one codex-alab writes: trusted as a
/// dead predecessor's (a crash or a restore carried it), never otherwise.
pub fn is_ours(text: &str) -> bool {
    text.starts_with(GOAL) || text.starts_with(PLAN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_step_in_progress_is_the_plan_step() {
        let args = r#"{"explanation":"x","plan":[
            {"step":"read the code","status":"completed"},
            {"step":"write the inbox","status":"in_progress"},
            {"step":"test it","status":"pending"}]}"#;
        assert_eq!(plan_step(args), Some(Some("write the inbox".to_string())));
        let none =
            r#"{"plan":[{"step":"a","status":"completed"},{"step":"b","status":"pending"}]}"#;
        assert_eq!(plan_step(none), Some(None));
        // Not a plan: nothing learned (the description stays as it is).
        assert_eq!(plan_step("{"), None);
        assert_eq!(plan_step(r#"{"plan":"x"}"#), None);
    }

    #[test]
    fn the_goal_leads_and_the_step_follows() {
        assert_eq!(
            describe(Some("ship the inbox"), Some("write tests")),
            Some("goal: ship the inbox · now: write tests".to_string())
        );
        assert_eq!(
            describe(Some("ship the inbox"), None),
            Some("goal: ship the inbox".to_string())
        );
        assert_eq!(
            describe(None, Some("write tests")),
            Some("plan: write tests".to_string())
        );
        assert_eq!(describe(None, None), None);
        assert_eq!(describe(Some("  "), Some("")), None);
    }

    #[test]
    fn a_description_is_one_bounded_line() {
        let long = "ü".repeat(2000);
        let text = describe(Some(&format!("a\nb\u{202e}c {long}")), None).expect("text");
        assert!(text.len() <= DESCRIPTION_MAX_BYTES, "{}", text.len());
        assert!(text.starts_with("goal: a b c "), "{text}");
        assert!(!text.contains('\n'));
    }

    #[test]
    fn only_its_own_texts_are_ours() {
        assert!(is_ours("goal: x"));
        assert!(is_ours("plan: y"));
        assert!(!is_ours("my build box"));
        assert!(!is_ours("Goal: x"));
    }
}
