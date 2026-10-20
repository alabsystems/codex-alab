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
use crate::chatwidget::ExternalEditorState;
use codex_alab_tui::session::Frame;

pub(super) fn observe(app: &App) {
    if !codex_alab_tui::session::active() {
        return;
    }
    let chat = &app.chat_widget;
    let primary = app.primary_thread_id.map(|id| id.to_string());
    let displayed = chat.thread_id().map(|id| id.to_string());
    let rollout = chat.rollout_path();
    let config = chat.config_ref();
    let state = chat.alab_program_state();
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

/// ALAB(session) hook, each TUI event before it is dispatched: whether a whole-screen
/// overlay (the transcript, a diff, a backtrack preview) has the screen. No chat frame is
/// drawn under one, so `observe` cannot see it.
pub(super) fn overlay(open: bool) {
    codex_alab_tui::session::overlay_open(open);
}
