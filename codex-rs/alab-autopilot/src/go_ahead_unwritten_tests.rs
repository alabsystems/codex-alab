// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE GO-AHEAD'S LINES WHERE NOTHING TYPES IT (review of the port, finding 1; aterm's
//! DESIGN-harness-autopilot §15.6: `answering` empties where the act was not submitted): a goal
//! that stands owns the thread's pace, and Plan mode leaves the turn's end to a person, so the
//! go-ahead the turn's end foresaw and the decision judged is never submitted there. Its line
//! goes back on the menu bar (`meta set harness_needs`, aterm's `owner=needs`) in that
//! evaluation, never left hidden for a go-ahead nothing types. NEGATIVE CONTROL: with no goal
//! and no Plan mode, the go-ahead is typed and its line stays off.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::*;

/// The incident's park (2026-10-29): a NEEDS YOU line asking the owner to say a word for the
/// agent's own reversible work.
const PARK: &str = "Nothing has changed. The build is waiting only for your go-ahead.\n\nNEEDS \
                    YOU: say \"rerun the checks\" so I can build and test the upgrade fix branch, \
                    push it to main and remove the worktree.";

/// [`PARK`]'s line, as the menu bar shows it.
const LINE: &str = "say \"rerun the checks\" so I can build and test the upgrade fix branch, push \
                    it to main and remove the worktree.";

/// codex-alab's own turn ends on [`PARK`] now, as the turn's finish reads it: the needs folded,
/// the go-ahead foreseen ([`ThreadState::go_ahead_foreseen`]) and the menu bar's needs synced.
fn parked(h: &Harness) {
    let at = clock();
    let policy = h.autopilot.sup.policy().expect("proven policy");
    h.rt.with_state(|state| {
        state.observe_turn_end(
            decide::TurnEnd {
                at,
                by_autopilot: true,
                by_person: false,
                text: Some(PARK.to_string()),
                earlier: None,
                questions: false,
                worked: Duration::from_secs(600),
                wall: None,
                aborted: false,
                ran_tools: true,
                restored: false,
            },
            &h.autopilot.timing,
        );
        state.go_ahead_foreseen(&policy, at);
    });
    h.autopilot.surface_needs(&h.rt);
}

#[test]
fn a_go_ahead_nothing_types_puts_its_line_back_on_the_menu_bar() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            use codex_protocol::config_types::CollaborationMode;
            use codex_protocol::config_types::Settings;
            use codex_protocol::protocol::ThreadSettingsOverrides;
            use codex_state::ThreadGoalStatus;
            for (goal, plan) in [
                (Some(ThreadGoalStatus::Paused), false),
                (Some(ThreadGoalStatus::Blocked), false),
                (Some(ThreadGoalStatus::Active), false),
                (None, true),
                (None, false),
            ] {
                let goals = Arc::new(GoalService::new());
                let h = Harness::new_with_goals(Some(goals)).await;
                // No live upgrade takes the idle point first.
                h.fake.set_policy_key("upgrade", "false");
                upgrade_handoff_tests::wait_for_supervisor(|| {
                    h.autopilot
                        .sup
                        .policy()
                        .is_some_and(|policy| !policy.upgrade)
                })
                .await;
                h.immediate_point().await;
                h.hold_model_response().await;
                if let Some(status) = goal {
                    h.test
                        .codex
                        .state_db()
                        .expect("goal database")
                        .thread_goals()
                        .replace_thread_goal(h.rt.id, "the person's native goal", status, None)
                        .await
                        .expect("native goal");
                }
                if plan {
                    core_test_support::submit_thread_settings(
                        &h.test.codex,
                        ThreadSettingsOverrides {
                            collaboration_mode: Some(CollaborationMode {
                                mode: ModeKind::Plan,
                                settings: Settings {
                                    model: h.test.session_configured.model.clone(),
                                    reasoning_effort: None,
                                    developer_instructions: None,
                                },
                            }),
                            ..Default::default()
                        },
                    )
                    .await
                    .unwrap_or_else(|err| panic!("Plan mode: {err}"));
                }
                parked(&h);
                assert!(
                    h.rt.state().needs.shown().is_empty(),
                    "held for the go-ahead foreseen: {goal:?} {plan}"
                );
                Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
                if goal.is_none() && !plan {
                    // NEGATIVE CONTROL: the go-ahead goes, its line off the menu bar.
                    assert_eq!(
                        h.fake.acts(),
                        vec![crate::go_ahead::RULE_GO_AHEAD.to_string()]
                    );
                    assert!(h.rt.state().needs.shown().is_empty());
                    assert_eq!(h.fake.needs_told(), None, "{:?}", h.fake.requests());
                    continue;
                }
                assert!(
                    h.fake.acts().is_empty(),
                    "nothing typed past {goal:?} {plan}: {:?}",
                    h.fake.acts()
                );
                assert_eq!(
                    h.rt.state().needs.shown(),
                    vec![LINE.to_string()],
                    "the line is back: {goal:?} {plan}"
                );
                assert!(
                    h.fake
                        .wait_for(|fake| fake.needs_told() == Some(vec![LINE.to_string()]))
                        .await,
                    "on the menu bar: {goal:?} {plan}: {:?}",
                    h.fake.requests()
                );
            }
        })
        .await
        .expect("within a minute");
    });
}
