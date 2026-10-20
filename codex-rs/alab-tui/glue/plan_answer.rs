// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(answer) glue, compiled inside codex-tui as `bottom_pane::list_selection_view::
//! alab_answer` (one `#[path]` line at the end of `list_selection_view.rs`, and the view's
//! one-line `pre_draw_tick`): a selection view that is Plan mode's "Implement this plan?"
//! box (its three rows, exactly), while it is the TUI's active view, is reported to
//! codex-alab's shared state each frame; an answer posted for it selects its row and
//! accepts it through the view's own `accept`, as a person's Enter does. Only the yes that
//! grants no standing mode is ever accepted (`Yes, implement this plan`; never `clear
//! context`), and only while that row is enabled. The turn the yes submits is counted as
//! codex-alab's own. Every other selection view is untouched.

use super::ListSelectionView;
use super::SelectionActionKind;
use super::ViewCompletion;
use codex_alab_tui::boxes::BoxAnswer;
use codex_alab_tui::boxes::BoxKind;
use codex_alab_tui::boxes::BoxOutcome;
use codex_alab_tui::boxes::OpenBox;
use codex_alab_tui::boxes::PLAN_CLEAR_CONTEXT;
use codex_alab_tui::boxes::PLAN_MESSAGE;
use codex_alab_tui::boxes::PLAN_NO;
use codex_alab_tui::boxes::PLAN_YES;

/// The view's frame: report it if it is the plan box, then apply a posted answer.
pub(super) fn tick(view: &mut ListSelectionView) -> bool {
    let Some(state) = codex_alab_tui::boxes::tick_shared() else {
        return false;
    };
    if !is_plan_box(view) {
        return false;
    }
    let id = codex_alab_tui::boxes::plan_id(std::ptr::from_ref(view) as usize);
    if !state.touch_box(&id, None) {
        state.report_box(OpenBox {
            id: id.clone(),
            kind: BoxKind::Plan {
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
            opened_ms: codex_alab_tui::boxes::now_ms(),
        });
    }
    let Some(answer) = state.take_box_answer(&id) else {
        return false;
    };
    let applied = apply(view, answer, state);
    let taken = applied.is_ok();
    if taken {
        state.close_box(&id);
    }
    state.set_box_outcome(BoxOutcome { id, applied });
    taken
}

fn is_plan_box(view: &ListSelectionView) -> bool {
    view.completion.is_none()
        && !view.search_items_active()
        && view.tabs.is_empty()
        && view.items.len() == 3
        && view.items[0].name == PLAN_YES
        && view.items[1].name == PLAN_CLEAR_CONTEXT
        && view.items[2].name == PLAN_NO
}

fn apply(
    view: &mut ListSelectionView,
    answer: BoxAnswer,
    state: &codex_alab_tui::boxes::Shared,
) -> Result<(), String> {
    let BoxAnswer::Plan { row, .. } = answer else {
        return Err("a question's answer is no plan box's".to_owned());
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
