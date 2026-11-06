// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(session) glue, compiled inside codex-tui as `app::alab_session` (one `#[path]` line at
//! the end of `tui/src/app.rs`): `App` and `ChatWidget` keep this state private or
//! `pub(crate)`, so the frame's facts are read here and handed to `codex_alab_tui::session`
//! as plain data. Called first thing in `App::render_chat_widget_frame`, right after
//! `pre_draw_tick` refreshed the OSC 7501 state this reads and let the active view report
//! its box (`ALAB(answer)`): this frame's end closes a box no view reported, and the frame
//! request is handed over once so a posted answer is applied without waiting for a key.
//! The same facts set aterm's OSC 9;4 progress bar (`ALAB(progress)`), written here beside
//! the 7501 report and before the frame's draw.

use super::App;
use crate::AppServerTarget;
use crate::backend_id::BackendId;
use crate::chatwidget::ExternalEditorState;
use codex_alab_tui::boxes::GoalStop;
use codex_alab_tui::session::Frame;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_protocol::ThreadId;

#[cfg(test)]
#[path = "daybreak_app_recovery_tests.rs"]
mod daybreak_app_recovery_tests;
#[cfg(test)]
#[path = "goal_display_tests.rs"]
mod goal_display_tests;
#[cfg(test)]
#[path = "goal_prompt_tests.rs"]
mod goal_prompt_tests;
#[cfg(test)]
#[path = "turns_routing_tests.rs"]
mod turns_routing_tests;
#[cfg(all(test, unix))]
#[path = "upgrade_finish_order_tests.rs"]
mod upgrade_finish_order_tests;
#[cfg(test)]
#[path = "upgrade_handoff_native_tests.rs"]
mod upgrade_handoff_native_tests;

pub(super) fn observe(app: &App) {
    codex_alab_tui::interface::set_goal(app.chat_widget.alab_goal());
    // ALAB(turns): a turn seen on a thread the TUI no longer routes (another session shown,
    // a thread let go) is not read as running: its end can no longer reach `routed`.
    app.alab_backend_bookkeeping.retain(|thread| {
        ThreadId::from_string(thread).is_ok_and(|id| app.owns_thread_for_routing(id))
    });
    codex_alab_tui::turns::seen_keep(|thread| {
        ThreadId::from_string(thread).is_ok_and(|id| app.owns_thread_for_routing(id))
    });
    super::alab_backend_bookkeeping::observe_goal(app);
    if !codex_alab_tui::session::active() {
        return;
    }
    let chat = &app.chat_widget;
    let primary = app.primary_thread_id.map(|id| id.to_string());
    let displayed = chat.thread_id().map(|id| id.to_string());
    let rollout = chat.rollout_path();
    let config = chat.config_ref();
    let state = chat.alab_program_state();
    codex_alab_tui::session::set_background_terminals(chat.alab_background_terminals());
    codex_alab_tui::session::observe(&Frame {
        primary_thread: primary.as_deref(),
        displayed_thread: displayed.as_deref(),
        rollout_path: rollout.as_deref(),
        cwd: config.cwd.as_path(),
        model: chat.current_model(),
        codex_home: config.codex_home.as_path(),
        embedded: matches!(app.app_server_target, AppServerTarget::Embedded),
        state,
        draft_empty: chat.composer_is_empty(),
        queued_input: chat.alab_queued_input(),
        popup_open: !chat.no_modal_or_popup_active() || chat.has_active_view(),
        editor_open: chat.external_editor_state() != ExternalEditorState::Closed,
        pending_user_turn: chat.alab_pending_user_turn(),
    });
    codex_alab_tui::progress::observe(
        state,
        chat.alab_plan_progress(),
        config
            .config_layer_stack
            .layers_high_to_low()
            .map(|layer| &layer.config),
    );
    if codex_alab_tui::boxes::needs_waker() {
        let frames = chat.alab_frame_requester();
        codex_alab_tui::boxes::set_waker(move || frames.schedule_frame());
    }
    // The views ticked just before this frame (`pre_draw_tick`): a box none reported is shut.
    codex_alab_tui::boxes::end_frame();
}

/// ALAB(answer) hook, as a resume opens "Resume paused goal?" (`thread_goal_actions.rs`): why
/// the goal stopped, so codex-alab answers the goal box by it (a blocked goal is a person's).
pub(super) fn goal_prompt_over(status: ThreadGoalStatus) {
    let Some(state) = codex_alab_tui::boxes::tick_shared() else {
        return;
    };
    let stop = match status {
        ThreadGoalStatus::Paused => GoalStop::Paused,
        ThreadGoalStatus::Blocked => GoalStop::Blocked,
        ThreadGoalStatus::UsageLimited => GoalStop::UsageLimited,
        ThreadGoalStatus::Active | ThreadGoalStatus::BudgetLimited | ThreadGoalStatus::Complete => {
            return;
        }
    };
    state.set_goal_prompt(stop);
}

/// ALAB(turns) hook, each app-server notification the TUI routes to a thread it owns
/// (`handle_server_notification_event`, before the thread's store takes it): a turn's start is
/// booked for the OSC 7501 report (`codex_alab_tui::turns::seen`), whatever the thread (a
/// sub-agent's, a side thread's, a root's) and wherever its server runs (in this process or
/// remote). A turn of a thread the TUI does not show holds a report standing in a turn, so a
/// switch back to it begins none. Its end is booked by [`stream`], which no routing return
/// passes by.
pub(super) fn routed(app: &App, owner: BackendId, notification: &ServerNotification) {
    match notification {
        ServerNotification::TurnStarted(started) => {
            codex_alab_tui::turns::seen(&started.thread_id, &started.turn.id, true);
        }
        ServerNotification::ThreadGoalUpdated(updated) => {
            let goal = (updated.goal.status == ThreadGoalStatus::Active)
                .then_some(updated.goal.objective.as_str());
            app.alab_backend_bookkeeping
                .goal(owner, &updated.thread_id, goal);
        }
        ServerNotification::ThreadGoalCleared(cleared) => {
            app.alab_backend_bookkeeping
                .goal(owner, &cleared.thread_id, None);
        }
        _ => {}
    }
}

/// ALAB(turns) hook, each app-server event before it is handled: a turn's end and a thread's
/// close are booked for their observed backend whatever routing does with them after (with no primary thread and no
/// startup pending, after an archive or a delete of the current task, upstream drops every
/// thread notification, a side thread's `turn/completed` among them, while the side thread
/// stays owned). A stream that lagged or ended may have lost its own seen turn's end;
/// another backend's turns remain standing. The autopilot's own book is unaffected.
pub(super) fn stream(app: &App, owner: BackendId, event: &AppServerEvent) {
    app.alab_backend_bookkeeping.stream(owner, event);
}

/// ALAB(session) hook, each TUI event before it is dispatched: whether a whole-screen
/// overlay (the transcript, a diff, a backtrack preview) has the screen. No chat frame is
/// drawn under one, so `observe` cannot see it.
pub(super) fn overlay(open: bool) {
    codex_alab_tui::session::overlay_open(open);
}

/// This runs in the TUI event loop immediately before shutdown, without an await.
/// Refresh facts that may have changed since the extension queued its request.
pub(super) fn accept_upgrade(app: &App, request: codex_alab_tui::upgrade::UpgradeRequest) -> bool {
    overlay(app.overlay.is_some());
    observe(app);
    let chat = &app.chat_widget;
    request.decide(
        !chat.is_user_turn_pending_or_running()
            && matches!(
                chat.alab_program_state(),
                codex_alab_tui::session::ProgramState::Idle
                    | codex_alab_tui::session::ProgramState::Done
                    | codex_alab_tui::session::ProgramState::Error
            )
            && chat.composer_is_empty()
            && !chat.alab_queued_input()
            && chat.no_modal_or_popup_active()
            && !chat.has_active_view()
            && chat.external_editor_state() == ExternalEditorState::Closed
            && app.overlay.is_none(),
    )
}
