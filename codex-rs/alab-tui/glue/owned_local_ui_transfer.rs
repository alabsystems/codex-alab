// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A distinct retained E1 DATA frame. Never feed this to upgrade_state or its
//! process-local forest receiver. Decoding retains the whole genuine E1 owner;
//! neither decoding nor taking a section says that the UI has been installed.
use crate::clipboard_copy::worker::alab_retained_clipboard::ClipboardData;
use crate::owned_local_entered_readers::constructor::Messages;
use crate::owned_local_ui_snapshot::UiSnapshot;
use codex_alab_tui::frontend_control::EnteredRetainedFrontend;
use codex_alab_tui::frontend_control::retained_service::EditorCheckpoint;
use codex_alab_tui::upgrade_exec_custody::images::LocatorData;
use codex_alab_tui::upgrade_exec_custody::images::SealedLocator;
use codex_alab_tui::upgrade_frame;
use codex_app_server_client::ClientConstructionCheckpoint;
use codex_app_server_client::ClientConstructionOperation;
use codex_app_server_client::InitialConstructionCheckpoint;
use codex_app_server_client::InitialConstructionOperation;
use codex_app_server_client::SavedResumeQuery;
use codex_protocol::ThreadId;
use serde::Deserialize;
use serde::Serialize;
use serde_json::value::RawValue;
use std::collections::HashSet;
use std::io;
use std::sync::Arc;

// Native's receive_input_frame limit. The complete encoded E1 body, including
// header and every section, must fit; individual JSON section limits do not add.
const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAGIC: &[u8; 16] = b"ALAB-RETAIN-UI2\n";
const VERSION: u32 = 2;
// Native section bodies keep their independent schema; UI2 makes the
// clipboard resource checkpoint mandatory without reinterpreting body codecs.
const SECTION_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TerminalData {
    modes: codex_alab_tui::terminal_modes::Snapshot,
    startup: codex_alab_tui::upgrade_startup::Snapshot,
    input: Vec<u8>,
}

impl TerminalData {
    /// Only the original terminal capture owner supplies these snapshots. Its
    /// actual fence/commit remains with that owner, never with this structure.
    pub(crate) fn from_capture(
        actual: &codex_alab_tui::upgrade_terminal::Prepared,
    ) -> io::Result<Self> {
        if !actual.valid() {
            return Err(invalid("retained terminal capture is no longer current"));
        }
        let startup = actual
            .startup_snapshot()
            .ok_or_else(|| invalid("retained terminal capture lacks startup state"))?
            .clone();
        startup.validate()?;
        Ok(Self {
            modes: actual.mode_snapshot().clone(),
            startup,
            input: actual.input_snapshot().to_vec(),
        })
    }

    pub(crate) fn startup_data(&self) -> &codex_alab_tui::upgrade_startup::Snapshot {
        &self.startup
    }
    pub(crate) fn modes_data(&self) -> &codex_alab_tui::terminal_modes::Snapshot {
        &self.modes
    }
    pub(crate) fn input_data(&self) -> &[u8] {
        &self.input
    }
}

/// The body is native UI DATA, not a callback, sender, subscription, or guard.
/// Every section is mandatory. Section installation must use its original
/// private graph codec and actual destination/resource owner before UI ACK.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeSection {
    schema: u32,
    body: Box<RawValue>,
}

impl NativeSection {
    pub(crate) fn captured(body: Box<RawValue>) -> Self {
        Self {
            schema: SECTION_VERSION,
            body,
        }
    }
    pub(crate) fn body(&self) -> &RawValue {
        &self.body
    }
    fn validate(&self) -> io::Result<()> {
        if self.schema != SECTION_VERSION {
            return Err(invalid("retained native section version differs"));
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum SelectionData {
    NoPrimary {
        // Explicit None is genuine absence, not an inferred fresh selection.
        #[serde(deserialize_with = "required_option")]
        initial: Option<InitialConstructionCheckpoint>,
    },
    Selected {
        primary: ThreadId,
        displayed: ThreadId,
        #[serde(deserialize_with = "required_option")]
        initial: Option<InitialConstructionCheckpoint>,
        later: Vec<Arc<ClientConstructionCheckpoint>>,
    },
}

/// Immutable data for one attempted opening whose original reply is missing.
/// Both references originate from the same selected checkpoint.
#[derive(Clone)]
pub(crate) struct SavedResumeHandoff {
    checkpoint: Arc<ClientConstructionCheckpoint>,
    query: Arc<SavedResumeQuery>,
}

impl SavedResumeHandoff {
    pub(crate) fn from_checkpoint(
        checkpoint: &Arc<ClientConstructionCheckpoint>,
    ) -> io::Result<Option<Self>> {
        Ok(checkpoint.saved_resume_query()?.map(|query| Self {
            checkpoint: Arc::clone(checkpoint),
            query: Arc::new(query),
        }))
    }

    pub(crate) fn checkpoint(&self) -> &Arc<ClientConstructionCheckpoint> {
        &self.checkpoint
    }

    pub(crate) fn query(&self) -> &Arc<SavedResumeQuery> {
        &self.query
    }
}

impl SelectionData {
    fn validate(&self) -> io::Result<()> {
        let initial = match self {
            Self::NoPrimary { initial } => initial,
            Self::Selected { initial, later, .. } => {
                let mut openings = HashSet::new();
                for checkpoint in later {
                    // The existing owner validates its own frozen DATA; it does
                    // not poll/reissue the saved original operation here.
                    ClientConstructionOperation::validate_checkpoint(checkpoint)?;
                    if let Some((opening, _)) = checkpoint.saved_opening()
                        && !openings.insert(opening)
                    {
                        return Err(invalid("duplicate saved opening in retained selection"));
                    }
                }
                initial
            }
        };
        if let Some(checkpoint) = initial {
            InitialConstructionOperation::validate_checkpoint(checkpoint)?;
        }
        Ok(())
    }

    fn saved_resume_queries(&self) -> io::Result<Vec<SavedResumeHandoff>> {
        let Self::Selected { later, .. } = self else {
            return Ok(Vec::new());
        };
        later
            .iter()
            .filter(|checkpoint| checkpoint.saved_opening().is_some())
            .filter_map(|checkpoint| SavedResumeHandoff::from_checkpoint(checkpoint).transpose())
            .collect()
    }
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum UiData {
    NoPrimary {
        // Actual StartupDraftPump metadata plus its rich BottomPane graph;
        // never a default ChatWidget standing in for a missing Core.
        startup: NativeSection,
        pending_views: NativeSection,
        initial_message: NativeSection,
        presentation: NativeSection,
    },
    Selected {
        messages: NativeSection,
        composer: NativeSection,
        questions: NativeSection,
        pending_views: NativeSection,
        // Original App thread/view/sidebar/search/scroll/provenance DATA. The
        // fresh backend projection is reconciled with it by the original UI
        // consumers; this section is not a serialized app-server/Core forest.
        application: NativeSection,
    },
}

// Serde's internally tagged Content buffer cannot carry RawValue. Keep the
// original wire schema, but parse each closed original variant directly from
// the JSON deserializer, preserving its native section bytes.
impl<'de> Deserialize<'de> for UiData {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Tag {
            state: String,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct NoPrimary {
            state: String,
            startup: NativeSection,
            pending_views: NativeSection,
            initial_message: NativeSection,
            presentation: NativeSection,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Selected {
            state: String,
            messages: NativeSection,
            composer: NativeSection,
            questions: NativeSection,
            pending_views: NativeSection,
            application: NativeSection,
        }
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        let tag: Tag = serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
        match tag.state.as_str() {
            "noPrimary" => {
                let actual: NoPrimary =
                    serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
                if actual.state != "noPrimary" {
                    return Err(serde::de::Error::custom("retained UI state changed"));
                }
                Ok(Self::NoPrimary {
                    startup: actual.startup,
                    pending_views: actual.pending_views,
                    initial_message: actual.initial_message,
                    presentation: actual.presentation,
                })
            }
            "selected" => {
                let actual: Selected =
                    serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
                if actual.state != "selected" {
                    return Err(serde::de::Error::custom("retained UI state changed"));
                }
                Ok(Self::Selected {
                    messages: actual.messages,
                    composer: actual.composer,
                    questions: actual.questions,
                    pending_views: actual.pending_views,
                    application: actual.application,
                })
            }
            _ => Err(serde::de::Error::custom(
                "unknown retained original UI state",
            )),
        }
    }
}

impl UiData {
    fn validate_sections(&self) -> io::Result<()> {
        let sections = match self {
            Self::NoPrimary {
                startup,
                pending_views,
                initial_message,
                presentation,
            } => {
                vec![startup, pending_views, initial_message, presentation]
            }
            Self::Selected {
                messages,
                composer,
                questions,
                pending_views,
                application,
            } => {
                vec![messages, composer, questions, pending_views, application]
            }
        };
        for section in sections {
            section.validate()?;
        }
        Ok(())
    }
}

/// Active carries the SDK's exact closed DATA type. Actual document/child/TTY/
/// sole-reap ownership must be claimed before initializing TUI/input.
#[derive(Serialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum EditorData {
    Closed {},
    // Requested is original input intent; it does not say an editor was born.
    Requested { original_request: NativeSection },
    Active { checkpoint: EditorCheckpoint },
}

impl<'de> Deserialize<'de> for EditorData {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Tag {
            state: String,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Closed {
            state: String,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Requested {
            state: String,
            original_request: NativeSection,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Active {
            state: String,
            checkpoint: EditorCheckpoint,
        }
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        let tag: Tag = serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
        match tag.state.as_str() {
            "closed" => {
                let actual: Closed =
                    serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
                if actual.state != "closed" {
                    return Err(serde::de::Error::custom("retained editor state changed"));
                }
                Ok(Self::Closed {})
            }
            "requested" => {
                let actual: Requested =
                    serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
                if actual.state != "requested" {
                    return Err(serde::de::Error::custom("retained editor state changed"));
                }
                Ok(Self::Requested {
                    original_request: actual.original_request,
                })
            }
            "active" => {
                let actual: Active =
                    serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
                if actual.state != "active" {
                    return Err(serde::de::Error::custom("retained editor state changed"));
                }
                Ok(Self::Active {
                    checkpoint: actual.checkpoint,
                })
            }
            _ => Err(serde::de::Error::custom("unknown retained editor state")),
        }
    }
}

impl EditorData {
    fn validate(&self) -> io::Result<()> {
        match self {
            Self::Closed {} => Ok(()),
            Self::Requested { original_request } => original_request.validate(),
            Self::Active { checkpoint } => checkpoint.validate_data(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    version: u32,
    service: String,
    configuration: UiSnapshot,
    selection: SelectionData,
    terminal: TerminalData,
    ui: UiData,
    editor: EditorData,
    image_custody: LocatorData,
    clipboard: ClipboardData,
}

fn token(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

fn decode(bytes: &[u8], service: &str) -> io::Result<Frame> {
    if bytes.len() <= MAGIC.len() || bytes.len() > MAX_BYTES || !bytes.starts_with(MAGIC) {
        return Err(invalid(
            "missing or oversized retained whole-UI E1 envelope",
        ));
    }
    // from_slice rejects trailing non-whitespace and duplicate struct fields.
    let frame: Frame = serde_json::from_slice(&bytes[MAGIC.len()..]).map_err(invalid)?;
    if frame.version != VERSION || !token(&frame.service) || frame.service != service {
        return Err(invalid(
            "retained whole-UI version/service differs from actual entry",
        ));
    }
    frame.configuration.validate_data()?;
    frame.terminal.startup.validate()?;
    frame.selection.validate()?;
    frame.ui.validate_sections()?;
    frame.editor.validate()?;
    frame.image_custody.validate_data()?;
    frame.clipboard.validate_data()?;
    if !matches!(
        (&frame.selection, &frame.ui),
        (SelectionData::NoPrimary { .. }, UiData::NoPrimary { .. })
            | (SelectionData::Selected { .. }, UiData::Selected { .. })
    ) {
        return Err(invalid(
            "retained whole-UI selection and original UI disagree",
        ));
    }
    Ok(frame)
}

fn decode_with_pending_input(bytes: &[u8], service: &str) -> io::Result<(Frame, Messages)> {
    let frame = decode(bytes, service)?;
    let messages = Messages::from_ui(&frame.ui)?;
    Ok((frame, messages))
}

/// Keeps the actual E1 owner even if native sections still need resource binding.
/// No consuming getter, decoded/applied Boolean, or synthetic UI ACK is exposed.
pub(crate) struct DecodedUiTransfer {
    entry: Arc<EnteredRetainedFrontend>,
    frame: Frame,
    messages: Messages,
    // This locator is extracted from the SAME whole actual entry. Pure DATA
    // decoding above never constructs image-file custody or installation.
    images: SealedLocator,
}

/// A decoder failure retains the same actual E1/controller/raw input owner.
/// The caller must drain/close that genuine owner; it cannot fall back to an
/// ordinary fresh startup merely because a future image cannot decode it.
pub(crate) struct DecodeFailure {
    entry: Arc<EnteredRetainedFrontend>,
    error: io::Error,
}
impl DecodeFailure {
    pub(crate) fn entry(&self) -> &Arc<EnteredRetainedFrontend> {
        &self.entry
    }
    pub(crate) fn error(&self) -> &io::Error {
        &self.error
    }
}
impl std::fmt::Debug for DecodeFailure {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("DecodeFailure")
            .field("kind", &self.error.kind())
            .finish_non_exhaustive()
    }
}
impl std::fmt::Display for DecodeFailure {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(out)
    }
}
impl std::error::Error for DecodeFailure {}

impl DecodedUiTransfer {
    pub(crate) fn from_entered(entry: Arc<EnteredRetainedFrontend>) -> Result<Self, DecodeFailure> {
        let (frame, messages) =
            match decode_with_pending_input(entry.input_data(), entry.service_data()) {
                Ok(decoded) => decoded,
                Err(error) => return Err(DecodeFailure { entry, error }),
            };
        let images = match SealedLocator::from_entered(entry.clone()) {
            Ok(images) => images,
            Err(error) => return Err(DecodeFailure { entry, error }),
        };
        Ok(Self {
            entry,
            frame,
            messages,
            images,
        })
    }
    pub(crate) fn entry(&self) -> &Arc<EnteredRetainedFrontend> {
        &self.entry
    }
    pub(crate) fn image_custody(&self) -> &SealedLocator {
        &self.images
    }
    pub(crate) fn clipboard_data(&self) -> &ClipboardData {
        &self.frame.clipboard
    }
    pub(crate) fn snapshot(&self) -> &UiSnapshot {
        &self.frame.configuration
    }
    pub(crate) fn selection_data(&self) -> &SelectionData {
        &self.frame.selection
    }
    pub(crate) fn saved_resume_queries(&self) -> io::Result<Vec<SavedResumeHandoff>> {
        self.frame.selection.saved_resume_queries()
    }
    pub(crate) fn terminal_data(&self) -> &TerminalData {
        &self.frame.terminal
    }
    pub(crate) fn ui_data(&self) -> &UiData {
        &self.frame.ui
    }
    pub(crate) fn editor_data(&self) -> &EditorData {
        &self.frame.editor
    }

    /// Original terminal/input restoration only, before Tui::init or input
    /// probes. This calls the existing terminal-only owner, not upgrade_state,
    /// legacy Core forest replay, or its editor-ready scheduling flag.
    pub(crate) fn restore_terminal_before_init(&self) -> io::Result<()> {
        if matches!(self.frame.editor, EditorData::Active { .. }) {
            // An active editor owns the terminal. Its genuine SDK continuation
            // must choose the editor presentation/adoption route; drawing or
            // adopting raw-mode state here would overwrite that real owner.
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "active retained editor requires actual SDK terminal continuation",
            ));
        }
        let terminal = &self.frame.terminal;
        codex_alab_tui::upgrade_startup::restore(
            terminal.startup.clone(),
            &terminal.modes,
            &terminal.input,
        )
    }

    pub(crate) fn messages(&self) -> &Messages {
        &self.messages
    }
}

/// Mandatory original DATA from ONE real capture. There is no default or
/// partial constructor; resource owners remain with the actual C1 caller.
/// Invocation and scene-specific whole-App payloads are separate forthcoming
/// required fields, never inferred from this configuration/selection DATA.
pub(crate) struct EncodeInputs {
    pub(crate) configuration: UiSnapshot,
    pub(crate) selection: SelectionData,
    pub(crate) terminal: TerminalData,
    pub(crate) ui: UiData,
    pub(crate) editor: EditorData,
    pub(crate) image_custody: LocatorData,
    pub(crate) clipboard: ClipboardData,
}

/// All mandatory sections come from the actual same capture boundary. Callers
/// retain their original terminal/input/UI/native/image Offer owners across
/// encoding and the actual native command's exec scope.
pub(crate) fn encode(service: &str, original: EncodeInputs) -> io::Result<Vec<u8>> {
    let EncodeInputs {
        configuration,
        selection,
        terminal,
        ui,
        editor,
        image_custody,
        clipboard,
    } = original;
    let frame = Frame {
        version: VERSION,
        service: service.into(),
        configuration,
        selection,
        terminal,
        ui,
        editor,
        image_custody,
        clipboard,
    };
    let bytes = upgrade_frame::encode(
        MAGIC,
        &frame,
        MAX_BYTES,
        "retained UI exceeds Native E1 input limit",
    )
    .map_err(invalid)?;
    drop(frame);
    drop(decode_with_pending_input(&bytes, service)?);
    Ok(bytes)
}

#[cfg(test)]
#[path = "retained_e1_rawvalue_tests.rs"]
mod rawvalue_tests;

// Explicit null records observed absence. Omission cannot erase an actual
// initial construction owner/checkpoint from the mandatory E1 selection.
fn required_option<'de, D, T>(decoder: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(decoder)
}

#[path = "retained_image_schema_startup.rs"]
pub(crate) mod image_startup;
