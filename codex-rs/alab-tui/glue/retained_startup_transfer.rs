// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The original provisional StartupDraftPump's choice/submission DATA. The
//! actual event stream, app receiver, input fence, and keymap stay real owners.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StartupData {
    version: u32,
    initial_screen: InitialScreenData,
    session_action: SessionActionData,
    resolved_selection: Option<SelectionData>,
    configured_cwd: Option<Vec<u8>>,
    pending_paste_newline: Option<PendingNewline>,
    submission_pending: bool,
    motion_animated: bool,
    composer: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
enum InitialScreenData {
    Composer,
    Onboarding,
    SessionPicker,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
enum SessionActionData {
    New,
    NewFromCommandCenter,
    Resume,
    Fork,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "choice", rename_all = "camelCase", deny_unknown_fields)]
enum SelectionData {
    StartFresh {},
    AgentsOverview {},
    Exit {},
    Resume { target: TargetData },
    Fork { target: TargetData },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TargetData {
    path: Option<Vec<u8>>,
    thread_id: codex_protocol::ThreadId,
    cwd: Option<Vec<u8>>,
    history_mode: Option<codex_app_server_protocol::ThreadHistoryMode>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PendingNewline {
    at: codex_alab_tui::upgrade_editor_clock::Snapshot,
    text: String,
}

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
fn path_data(path: &Path) -> Vec<u8> {
    path.as_os_str().as_bytes().to_vec()
}
fn path_from_data(data: Vec<u8>) -> io::Result<PathBuf> {
    if data.contains(&0) {
        return Err(invalid("retained startup path contains NUL"));
    }
    Ok(PathBuf::from(std::ffi::OsString::from_vec(data)))
}
impl TargetData {
    fn capture(actual: &crate::resume_picker::SessionTarget) -> Self {
        Self {
            path: actual.path.as_deref().map(path_data),
            thread_id: actual.thread_id,
            cwd: actual.cwd.as_deref().map(path_data),
            history_mode: actual.history_mode,
        }
    }
    fn restore(
        self,
        actual_backend: crate::backend_id::BackendId,
    ) -> io::Result<crate::resume_picker::SessionTarget> {
        Ok(crate::resume_picker::SessionTarget {
            // Backend identity is taken from the actual authenticated session,
            // never deserialized or inferred from a saved UUID/thread selector.
            backend_id: actual_backend,
            path: self.path.map(path_from_data).transpose()?,
            thread_id: self.thread_id,
            cwd: self.cwd.map(path_from_data).transpose()?,
            history_mode: self.history_mode,
        })
    }
}
impl SelectionData {
    fn capture(
        actual: &SessionSelection,
        backend: crate::backend_id::BackendId,
    ) -> io::Result<Self> {
        Ok(match actual {
            SessionSelection::StartFresh => Self::StartFresh {},
            SessionSelection::AgentsOverview => Self::AgentsOverview {},
            SessionSelection::Exit => Self::Exit {},
            SessionSelection::Resume(target) => {
                target.ensure_backend(backend).map_err(invalid)?;
                Self::Resume {
                    target: TargetData::capture(target),
                }
            }
            SessionSelection::Fork(target) => {
                target.ensure_backend(backend).map_err(invalid)?;
                Self::Fork {
                    target: TargetData::capture(target),
                }
            }
        })
    }
    fn restore(self, backend: crate::backend_id::BackendId) -> io::Result<SessionSelection> {
        Ok(match self {
            Self::StartFresh {} => SessionSelection::StartFresh,
            Self::AgentsOverview {} => SessionSelection::AgentsOverview,
            Self::Exit {} => SessionSelection::Exit,
            Self::Resume { target } => SessionSelection::Resume(target.restore(backend)?),
            Self::Fork { target } => SessionSelection::Fork(target.restore(backend)?),
        })
    }
}

impl StartupDraftPump {
    /// Snapshot only original UI DATA, while validating the actual selected
    /// backend separately. No operation is polled or primary reconstructed.
    pub(crate) fn alab_capture_retained_startup(
        &self,
        actual_session: &crate::app_server_session::AppServerSession,
        max_bytes: usize,
    ) -> io::Result<StartupData> {
        let selection = self
            .resolved_selection
            .as_ref()
            .map(|selection| SelectionData::capture(selection, actual_session.backend_id()))
            .transpose()?;
        capture_ui(self, selection, max_bytes)
    }
}
fn capture_ui(
    pump: &StartupDraftPump,
    resolved_selection: Option<SelectionData>,
    max_bytes: usize,
) -> io::Result<StartupData> {
    // These owners do not yet have transferable per-event/chord codecs.
    // Keep their original owner intact rather than silently clearing them.
    if !pump.app_event_rx.is_empty() || pump.key_chord_matcher.is_pending() {
        return Err(invalid(
            "retained startup still needs original pending-event/chord DATA",
        ));
    }
    Ok(StartupData {
        version: VERSION,
        initial_screen: match pump.initial_screen {
            StartupDraftInitialScreen::Composer => InitialScreenData::Composer,
            StartupDraftInitialScreen::Onboarding => InitialScreenData::Onboarding,
            StartupDraftInitialScreen::SessionPicker => InitialScreenData::SessionPicker,
        },
        session_action: match pump.session_action {
            StartupDraftSessionAction::New => SessionActionData::New,
            StartupDraftSessionAction::NewFromCommandCenter => {
                SessionActionData::NewFromCommandCenter
            }
            StartupDraftSessionAction::Resume => SessionActionData::Resume,
            StartupDraftSessionAction::Fork => SessionActionData::Fork,
        },
        resolved_selection,
        configured_cwd: pump.configured_cwd.as_deref().map(path_data),
        pending_paste_newline: pump
            .pending_paste_newline
            .as_ref()
            .map(|(at, text)| {
                Ok::<_, io::Error>(PendingNewline {
                    at: codex_alab_tui::upgrade_editor_clock::Snapshot::capture(at.into_std())
                        .map_err(invalid)?,
                    text: text.clone(),
                })
            })
            .transpose()?,
        submission_pending: pump.submission_pending,
        motion_animated: pump.motion == crate::motion::MotionMode::Animated,
        composer: pump
            .bottom_pane
            .alab_capture_retained_no_primary_checkpoint(max_bytes)?,
    })
}

pub(crate) struct PreparedStartup {
    data: StartupData,
    selection: Option<SessionSelection>,
    cwd: Option<PathBuf>,
    newline: Option<(Instant, String)>,
}
impl PreparedStartup {
    pub(crate) fn decode(
        bytes: &serde_json::value::RawValue,
        actual_session: &crate::app_server_session::AppServerSession,
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<Self> {
        let mut data: StartupData = serde_json::from_str(bytes.get()).map_err(invalid)?;
        if data.version != VERSION {
            return Err(invalid("retained startup schema differs"));
        }
        let selection = data
            .resolved_selection
            .take()
            .map(|selection| selection.restore(actual_session.backend_id()))
            .transpose()?;
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
        drop(BottomPane::alab_prepare_retained_no_primary_checkpoint(
            &data.composer,
            max_bytes,
            max_generations,
        )?);
        Ok(Self {
            data,
            selection,
            cwd,
            newline,
        })
    }

    /// The destination is the original fresh pump with actual input/events and
    /// final config keymap. No Core/default ChatWidget or event receiver is made.
    pub(crate) fn install(
        self,
        pump: &mut StartupDraftPump,
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<()> {
        if pump.submission_pending
            || pump.pending_paste_newline.is_some()
            || pump.resolved_selection.is_some()
            || !pump.app_event_rx.is_empty()
            || pump.key_chord_matcher.is_pending()
        {
            return Err(invalid(
                "retained startup destination already owns different input",
            ));
        }
        let editor = BottomPane::alab_prepare_retained_no_primary_checkpoint(
            &self.data.composer,
            max_bytes,
            max_generations,
        )?;
        editor.install_with_clock(&mut pump.bottom_pane, /*clock*/ None)?;
        // Every remaining operation is an infallible move after editor preflight.
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
        pump.resolved_selection = self.selection;
        pump.configured_cwd = self.cwd;
        pump.pending_paste_newline = self.newline;
        pump.submission_pending = self.data.submission_pending;
        pump.motion = crate::motion::MotionMode::from_animations_enabled(self.data.motion_animated);
        Ok(())
    }
}

#[path = "retained_startup_early_restore.rs"]
pub(crate) mod early;

#[path = "retained_startup_native_recovery.rs"]
pub(crate) mod recovery;
