// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app::resume_preparation::PreparedResume;
use crate::chatwidget::ThreadInputStateRestoreMode;

impl App {
    pub(in crate::app) async fn alab_activate_selected_resume(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        requested: ThreadId,
        prepared: PreparedResume,
    ) -> Result<AppRunControl> {
        if prepared.thread.session.thread_id != requested {
            self.add_session_picker_error("The server returned a different conversation.".into());
            return Ok(AppRunControl::Continue);
        }
        let switching = self.current_displayed_thread_id() != Some(requested);
        let read_only = prepared.read_only;
        if switching {
            self.capture_session_inputs(app_server.backend_id()).await;
        }
        let control = self
            .activate_prepared_session(tui, app_server, prepared)
            .await?;
        if switching
            && matches!(control, AppRunControl::Continue)
            && self.active_thread_id == Some(requested)
            && self.chat_widget.thread_id() == Some(requested)
            && let Some(mut input) = self.agents_overview.input_states.remove(&requested)
        {
            // The resumed widget already reflects the latest server result.
            input.provider_error = None;
            self.reconcile_saved_session_input(requested, &mut input)
                .await;
            self.chat_widget
                .alab_prepare_resumed_input(&mut input, read_only, /*previously_held*/ false);
            let preserve_in_flight_turn = self.active_turn_id_for_thread(requested).await.is_some();
            self.chat_widget.restore_thread_input_state(
                Some(input),
                ThreadInputStateRestoreMode {
                    preserve_in_flight_turn,
                },
            );
            self.chat_widget.submit_initial_user_message_if_pending();
        }
        Ok(control)
    }
}
