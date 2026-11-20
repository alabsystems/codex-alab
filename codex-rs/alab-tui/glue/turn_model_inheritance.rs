// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::ChatWidget;
use super::ChatWidgetInit;
use crate::app_command::AppCommand;
use crate::app_server_session::AppServerStartedThread;
use crate::app_server_session::ThreadParamsMode;
use crate::legacy_core::config::Config;
use crate::session_state::ThreadSessionState;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::SessionSource;
use codex_protocol::ThreadId;

fn created_in_process(
    handle: &AppServerRequestHandle,
    mode: ThreadParamsMode,
    source: &SessionSource,
) -> bool {
    mode == ThreadParamsMode::Embedded
        && matches!(handle, AppServerRequestHandle::InProcess(_))
        && *source == SessionSource::Cli
}

/// A local daemon and a retained backend can outlive the TUI executable.
/// Only the in-process server shares its startup preset implementation.
pub(crate) fn observe_fresh(
    session: &mut ThreadSessionState,
    source: &SessionSource,
    handle: &AppServerRequestHandle,
    mode: ThreadParamsMode,
) {
    if created_in_process(handle, mode, source) {
        session.collaboration_mode =
            Config::alab_fresh_tui_mode(&session.model, session.reasoning_effort.clone())
                .map(Box::new);
    }
}

impl ChatWidget {
    /// Resume and fork already have the server's selection before the first frame.
    pub(crate) fn alab_opened_thread_widget(
        mut init: ChatWidgetInit,
        opened: Option<AppServerStartedThread>,
    ) -> (Self, Option<AppServerStartedThread>) {
        let Some(thread) = opened.as_ref().filter(|_| codex_alab_core::active()) else {
            return (Self::new_with_app_event(init), opened);
        };
        let session = &thread.session;
        let mode = session.collaboration_mode.as_deref();
        init.model =
            Some(mode.map_or_else(|| session.model.clone(), |mode| mode.model().to_owned()));
        let mut widget = Self::new_with_app_event(init);
        if let Some(mode) = mode {
            widget.set_effective_collaboration_mode(mode.clone());
        } else {
            widget.set_reasoning_effort(session.reasoning_effort.clone());
        }
        (widget, opened)
    }

    /// Match the selection while composing. A queued message can outlive the
    /// old model and its settings notification can arrive before or after RPC.
    pub(super) fn alab_mark_inherited_turn(&self, mut turn: AppCommand) -> AppCommand {
        if codex_alab_core::active()
            && let AppCommand::UserTurn {
                turn_trigger: "user",
                model,
                effort,
                collaboration_mode,
                alab_inherited_model_thread,
                ..
            } = &mut turn
            && let Some(observed) = &self.alab_observed_thread_mode
            && model.as_str() == observed.model()
            && *effort == observed.settings.reasoning_effort
            && collaboration_mode
                .as_ref()
                .is_none_or(|mode| mode == observed)
        {
            *alab_inherited_model_thread = self.thread_id;
        }
        turn
    }
}

/// The same command can be submitted against a fork with an explicit recovery
/// selection. Older remote servers also use turn/start to apply client choices.
pub(crate) fn inherits(turn: &AppCommand, thread: ThreadId, supports_settings: bool) -> bool {
    supports_settings
        && codex_alab_core::active()
        && matches!(
            turn,
            AppCommand::UserTurn {
                turn_trigger: "user",
                alab_inherited_model_thread: Some(source),
                alab_safety_retries: 0,
                ..
            } if *source == thread
        )
}

#[cfg(test)]
#[path = "turn_model_inheritance_tests.rs"]
mod tests;
