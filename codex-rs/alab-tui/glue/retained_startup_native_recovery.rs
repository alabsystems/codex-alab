// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native provisional editor custody when real destination recovery changed its
//! composer. The original broker subscription retires at the same consumption
//! boundary as ordinary startup; no second reader or buffered-input discard.
use super::*;

pub(crate) struct NativeRecovery {
    pump: Box<StartupDraftPump>,
    _cause: io::Error,
}
impl NativeRecovery {
    pub(crate) fn retain(mut pump: StartupDraftPump, cause: io::Error) -> Self {
        let original = std::mem::replace(&mut pump.events, Box::pin(futures::stream::empty()));
        drop(original);
        Self {
            pump: Box::new(pump),
            _cause: cause,
        }
    }
    pub(crate) fn try_install(
        &mut self,
        widget: &mut crate::chatwidget::ChatWidget,
    ) -> io::Result<()> {
        if self.pump.pending_paste_newline.is_some()
            || self.pump.key_chord_matcher.is_pending()
            || !self.pump.app_event_rx.is_empty()
        {
            return Err(invalid(
                "original startup input still has native pending work",
            ));
        }
        widget.alab_handoff_original_startup_editor(&mut self.pump.bottom_pane)
    }
    #[cfg(test)]
    fn cause(&self) -> &io::Error {
        &self._cause
    }
    /// Mandatory Selected.application recovery leaf: None is supplied only by
    /// actual absence of this owner. Pending events/chords still refuse capture
    /// through the original checkpoint rather than silently dropping resources.
    pub(crate) fn capture(
        &self,
        session: &crate::app_server_session::AppServerSession,
        max_bytes: usize,
    ) -> io::Result<Box<serde_json::value::RawValue>> {
        let data = self
            .pump
            .alab_capture_retained_startup(session, max_bytes)?;
        serde_json::value::to_raw_value(&data).map_err(invalid)
    }
}

impl StartupDraftPump {
    pub(crate) fn alab_finish_entered_editor(
        mut self,
        widget: &mut crate::chatwidget::ChatWidget,
    ) -> (Option<ComposerDraftSnapshot>, Option<NativeRecovery>) {
        let owned = self
            .alab_constructor
            .as_ref()
            .map(|owner| owner.owns_entered_no_primary_editor())
            .transpose();
        match owned {
            Ok(Some(true)) => {}
            Ok(_) => return (self.alab_into_startup_draft(), None),
            Err(error) => return (None, Some(NativeRecovery::retain(self, error))),
        }
        let moved = if self.pending_paste_newline.is_some()
            || self.key_chord_matcher.is_pending()
            || !self.app_event_rx.is_empty()
        {
            Err(invalid(
                "original startup input still has native pending work",
            ))
        } else {
            widget.alab_handoff_original_startup_editor(&mut self.bottom_pane)
        };
        match moved {
            Ok(()) => (None, None),
            Err(error) => (None, Some(NativeRecovery::retain(self, error))),
        }
    }
}

#[cfg(test)]
#[path = "retained_startup_native_recovery_tests.rs"]
mod tests;
