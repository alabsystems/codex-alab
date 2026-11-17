// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Return held ORIGINAL inputs only after a real primary Runtime/Source commit.
//! ConfigurationCommit itself deliberately has no release surface.
use super::*;
impl Pending {
    pub(crate) fn release_original_inputs(
        &self,
        session: &AppServerSession,
        actual: &Arc<crate::app_server_session::alab_retained_ui::Pending>,
        widget: &mut crate::chatwidget::ChatWidget,
    ) -> io::Result<()> {
        let tenure = session.retained_frontend().ok_or_else(unavailable)?;
        if !self.belongs_to(tenure)
            || !actual.current_on(session)
            || actual
                .callback_activation(session)
                .map_err(io::Error::other)?
                .is_none()
            || actual.projection_target().is_none_or(|(thread, primary)| {
                !primary
                    || widget
                        .thread_id()
                        .is_none_or(|current| current.to_string() != thread)
            })
        {
            return Err(unavailable());
        }
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if state.phase != Phase::Complete || state._completion.is_none() {
            return Err(unavailable());
        }
        if state._initial_message.is_none() && state._submission.is_none() {
            return Ok(());
        }
        if state
            ._submission
            .as_ref()
            .is_some_and(|submission| submission.original)
        {
            return Err(io::Error::other(
                "original confirmation has not moved into its actual widget owner",
            ));
        }
        widget
            .alab_validate_configured_input_destination(state._initial_message.is_some(), false)?;
        let initial = state._initial_message.take();
        // Only an explicit false marker can remain; actual true intent is
        // required to have moved to the original queue before App events.
        drop(state._submission.take());
        drop(state);
        // All refusal paths precede taking the real inputs. No await can split
        // this one synchronous move into the same original widget.
        widget.alab_install_configured_inputs(initial, None);
        Ok(())
    }
}

impl Pending {
    pub(crate) fn install_original_confirmation(
        &self,
        widget: &mut crate::chatwidget::ChatWidget,
    ) -> io::Result<()> {
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(submission) = state._submission.as_ref() else {
            return Ok(());
        };
        if submission.original {
            if !widget.alab_original_confirmation_is_held() {
                return Err(unavailable());
            }
            widget.alab_validate_configured_input_destination(false, true)?;
        }
        let submission = state._submission.take().ok_or_else(unavailable)?;
        drop(state);
        if submission.original {
            // SAME original snapshot now lives in the original queue. Its real
            // Escape/edit handler can cancel it before later primary commit.
            // The restrict-only target gate below prevents submission here.
            widget.alab_install_configured_inputs(None, Some(submission.draft));
        }
        Ok(())
    }
}
