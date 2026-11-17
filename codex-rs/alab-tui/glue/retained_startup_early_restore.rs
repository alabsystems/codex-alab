// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Restore the actual provisional editor BEFORE original input pumping. The
//! captured selection remains comparison DATA until the real original choice
//! arrives; no backend identity/Source construction is invented from it.
use super::*;

pub(crate) struct Prepared {
    data: StartupData,
    cwd: Option<PathBuf>,
    newline: Option<(Instant, String)>,
    bytes: usize,
    generations: usize,
}
/// Constructed only after the original real pump/editor mutation succeeds.
/// This local installation evidence is not a Native/Source capability.
pub(crate) struct Installed {
    expected: Option<SelectionData>,
}
impl Prepared {
    pub(crate) fn install_with_clock(
        self,
        pump: &mut StartupDraftPump,
        clock: crate::bottom_pane::alab_retained_delayed_approval::PreparedDelayed,
    ) -> io::Result<Installed> {
        // Both DATA decoders have completed before this synchronous native
        // mutation. This is before the original constructor's first draw/poll.
        let installed = self.install(pump)?;
        clock.prime_before_original_replay(&mut pump.bottom_pane)?;
        Ok(installed)
    }
    pub(crate) fn decode(
        raw: &serde_json::value::RawValue,
        bytes: usize,
        generations: usize,
    ) -> io::Result<Self> {
        let mut data: StartupData = serde_json::from_str(raw.get()).map_err(invalid)?;
        if data.version != VERSION {
            return Err(invalid("early retained startup schema differs"));
        }
        let cwd = data.configured_cwd.take().map(path_from_data).transpose()?;
        let newline = data
            .pending_paste_newline
            .take()
            .map(|pending| {
                pending
                    .at
                    .restore()
                    .map(|at| (Instant::from_std(at), pending.text))
                    .map_err(invalid)
            })
            .transpose()?;
        // Validate even unused raw path fields before any original UI mutation.
        if let Some(SelectionData::Resume { target } | SelectionData::Fork { target }) =
            &data.resolved_selection
        {
            if let Some(path) = &target.path {
                let _validated = path_from_data(path.clone())?;
            }
            if let Some(path) = &target.cwd {
                let _validated = path_from_data(path.clone())?;
            }
        }
        drop(BottomPane::alab_prepare_retained_no_primary_checkpoint(
            &data.composer,
            bytes,
            generations,
        )?);
        Ok(Self {
            data,
            cwd,
            newline,
            bytes,
            generations,
        })
    }
    pub(crate) fn install(self, pump: &mut StartupDraftPump) -> io::Result<Installed> {
        if pump.submission_pending
            || pump.pending_paste_newline.is_some()
            || pump.resolved_selection.is_some()
            || !pump.app_event_rx.is_empty()
            || pump.key_chord_matcher.is_pending()
        {
            return Err(invalid(
                "early retained pump already processed different input",
            ));
        }
        // The actual original constructor invokes this before first run_until.
        // The shared codec also refuses any fresh destination draft below.
        let editor = BottomPane::alab_prepare_retained_no_primary_checkpoint(
            &self.data.composer,
            self.bytes,
            self.generations,
        )?;
        editor.install(&mut pump.bottom_pane)?;
        pump.initial_screen = match self.data.initial_screen {
            InitialScreenData::Composer => StartupDraftInitialScreen::Composer,
            InitialScreenData::Onboarding => StartupDraftInitialScreen::Onboarding,
            InitialScreenData::SessionPicker => StartupDraftInitialScreen::SessionPicker,
        };
        pump.session_action = match self.data.session_action {
            SessionActionData::New => StartupDraftSessionAction::New,
            SessionActionData::NewFromCommandCenter => {
                StartupDraftSessionAction::NewFromCommandCenter
            }
            SessionActionData::Resume => StartupDraftSessionAction::Resume,
            SessionActionData::Fork => StartupDraftSessionAction::Fork,
        };
        pump.configured_cwd = self.cwd;
        pump.pending_paste_newline = self.newline;
        pump.submission_pending = self.data.submission_pending;
        pump.motion = crate::motion::MotionMode::from_animations_enabled(self.data.motion_animated);
        Ok(Installed {
            expected: self.data.resolved_selection,
        })
    }
}
impl Installed {
    /// Preserve EXACT original cancellation comparison: thread+cwd distinguish
    /// destinations; no new BackendId, SessionTarget or accepted work is made.
    pub(crate) fn original_destination_changed(&mut self, actual: &SessionSelection) -> bool {
        let Some(saved) = self.expected.take() else {
            return false;
        };
        !match (saved, actual) {
            (SelectionData::StartFresh {}, SessionSelection::StartFresh)
            | (SelectionData::AgentsOverview {}, SessionSelection::AgentsOverview)
            | (SelectionData::Exit {}, SessionSelection::Exit) => true,
            (SelectionData::Resume { target }, SessionSelection::Resume(current))
            | (SelectionData::Fork { target }, SessionSelection::Fork(current)) => {
                target.thread_id == current.thread_id
                    && target.cwd == current.cwd.as_deref().map(path_data)
            }
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "retained_startup_early_restore_tests.rs"]
mod tests;
