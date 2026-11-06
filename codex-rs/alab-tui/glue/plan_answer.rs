// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(answer) glue, compiled inside codex-tui as `bottom_pane::list_selection_view::
//! alab_answer` (one `#[path]` line at the end of `list_selection_view.rs`, and the view's
//! one-line `pre_draw_tick`): a selection view that is Plan mode's "Implement this plan?"
//! box (its three rows, exactly) or a resume's "Resume paused goal?" box (its two rows,
//! exactly), while it is the TUI's active view, is reported to codex-alab's shared state each
//! frame; an answer posted for it selects its row and accepts it through the view's own
//! `accept`, as a person's Enter does.
//!
//! - The plan box: only the yes that grants no standing mode is ever accepted (`Yes,
//!   implement this plan`; never `clear context`), and only while that row is enabled. The
//!   turn the yes submits is counted as codex-alab's own.
//! - The goal box: only `Leave paused`, which changes nothing (never `Resume goal`); a
//!   history line says codex-alab answered it and how the goal goes on.
//!
//! Every other selection view is untouched.

use super::ListSelectionView;
use super::SelectionActionKind;
use super::ViewCompletion;
use crate::app_event::AppEvent;
use codex_alab_tui::boxes::BoxAnswer;
use codex_alab_tui::boxes::BoxKind;
use codex_alab_tui::boxes::BoxOutcome;
use codex_alab_tui::boxes::GOAL_LEAVE;
use codex_alab_tui::boxes::OpenBox;
use codex_alab_tui::boxes::PLAN_MESSAGE;
use codex_alab_tui::boxes::PLAN_YES;

/// Which of codex-alab's boxes a selection view is.
#[derive(Clone, Copy)]
enum Which {
    Plan,
    Goal,
}

/// The view's frame: report it if it is the plan box or the goal box, then apply a posted
/// answer.
pub(super) fn tick(view: &mut ListSelectionView) -> bool {
    let Some(state) = codex_alab_tui::boxes::tick_shared() else {
        return false;
    };
    let Some(which) = which(view) else {
        return false;
    };
    let addr = std::ptr::from_ref(view).addr();
    let id = state.view_box_id(
        match which {
            Which::Plan => "plan",
            Which::Goal => "goal",
        },
        addr,
    );
    if !state.touch_box(&id, None) {
        let kind = match which {
            Which::Plan => BoxKind::Plan {
                rows: view
                    .items
                    .iter()
                    .map(|item| {
                        let disabled = item
                            .disabled_reason
                            .clone()
                            .or_else(|| item.is_disabled.then(|| "the row is disabled".to_owned()));
                        (item.name.clone(), disabled)
                    })
                    .collect(),
            },
            Which::Goal => BoxKind::Goal {
                rows: view.items.iter().map(|item| item.name.clone()).collect(),
                stop: state.goal_prompt(),
            },
        };
        state.report_box(OpenBox {
            id: id.clone(),
            kind,
            opened_ms: codex_alab_tui::boxes::now_ms(),
        });
    }
    let Some(answer) = state.take_box_answer(&id) else {
        return false;
    };
    let applied = match which {
        Which::Plan => apply(view, answer, state),
        Which::Goal => leave_goal(view, answer),
    };
    let taken = applied.is_ok();
    if taken {
        codex_alab_tui::boxes::close_answered(state, &id, addr);
    }
    state.set_box_outcome(BoxOutcome { id, applied });
    taken
}

fn which(view: &ListSelectionView) -> Option<Which> {
    if view.completion.is_some() || view.search_items_active() || !view.tabs.is_empty() {
        return None;
    }
    let names: Vec<&str> = view.items.iter().map(|item| item.name.as_str()).collect();
    if codex_alab_tui::boxes::is_plan_box(&names) {
        Some(Which::Plan)
    } else if codex_alab_tui::boxes::is_goal_box(&names) {
        Some(Which::Goal)
    } else {
        None
    }
}

fn apply(
    view: &mut ListSelectionView,
    answer: BoxAnswer,
    state: &codex_alab_tui::boxes::Shared,
) -> Result<(), String> {
    let BoxAnswer::Plan { row, .. } = answer else {
        return Err("this answer is no plan box's".to_owned());
    };
    let Some(item) = view.items.get(row) else {
        return Err(format!("the plan box has no row {}", row + 1));
    };
    if item.name != PLAN_YES {
        return Err(format!(
            "codex-alab accepts only `{PLAN_YES}` (never `{}`)",
            item.name
        ));
    }
    if let Some(why) = &item.disabled_reason {
        return Err(format!("`{PLAN_YES}` is disabled: {why}"));
    }
    if item.is_disabled {
        return Err(format!("`{PLAN_YES}` is disabled"));
    }
    let Some(visible) = view.filtered_indices.iter().position(|&at| at == row) else {
        return Err(format!("`{PLAN_YES}` is not shown"));
    };
    view.state.selected_idx = Some(visible);
    state.expect_own_turn(PLAN_MESSAGE);
    view.accept(SelectionActionKind::Primary);
    if view.completion != Some(ViewCompletion::Accepted) {
        let _ = state.take_own_turn(PLAN_MESSAGE);
        return Err(format!("the plan box did not take `{PLAN_YES}`"));
    }
    Ok(())
}

/// The goal box's `Leave paused`: no action of its own, so the goal stays as it was.
fn leave_goal(view: &mut ListSelectionView, answer: BoxAnswer) -> Result<(), String> {
    let BoxAnswer::Goal { row, .. } = answer else {
        return Err("this answer is no goal box's".to_owned());
    };
    let Some(item) = view.items.get(row) else {
        return Err(format!("the goal box has no row {}", row + 1));
    };
    if item.name != GOAL_LEAVE {
        return Err(format!(
            "codex-alab answers only `{GOAL_LEAVE}` (never `{}`)",
            item.name
        ));
    }
    let Some(visible) = view.filtered_indices.iter().position(|&at| at == row) else {
        return Err(format!("`{GOAL_LEAVE}` is not shown"));
    };
    view.state.selected_idx = Some(visible);
    view.accept(SelectionActionKind::Primary);
    if view.completion != Some(ViewCompletion::Accepted) {
        return Err(format!("the goal box did not take `{GOAL_LEAVE}`"));
    }
    view.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
        crate::history_cell::new_info_event(
            codex_alab_tui::boxes::GOAL_LEFT.to_owned(),
            Some(codex_alab_tui::boxes::GOAL_LEFT_HINT.to_owned()),
        ),
    )));
    Ok(())
}
