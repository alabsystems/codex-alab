// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Same-E1 original clipboard state, staged global aliases and actual destination
//! worker binding. DATA/decoding alone never constructs BoundClipboard.
use super::*;
use crate::history_cell::alab_retained_cells::Capture as Cells;
use crate::history_cell::alab_retained_cells::FrameRef;
use crate::history_cell::alab_retained_cells::RestoreContext;
use crate::owned_local_ui_transfer::DecodedUiTransfer;
use crate::terminal_hyperlinks::alab_retained_text as text;
use codex_alab_tui::frontend_control::EnteredRetainedFrontend;
use codex_alab_tui::frontend_control::current_retained_frontend;
use codex_alab_tui::retained_arc_graph::Id;
use codex_alab_tui::upgrade_editor_clock::Snapshot as Clock;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::AtomicU64;
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
fn original_id() -> String {
    format!(
        "{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    )
}
fn valid_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
#[derive(Default)]
pub(crate) struct UiProgress {
    pub(super) state: Mutex<UiState>,
}
#[derive(Default)]
pub(crate) struct UiState {
    pub(super) active: bool,
    pub(super) unknown: bool,
    pub(super) last_event: Option<crate::tui::TuiEvent>,
    pub(super) completed: u64,
}
pub(super) struct WorkerDomain {
    selector: String,
    entry: Option<Weak<EnteredRetainedFrontend>>,
    pub(super) frame: OnceLock<FrameRequester>,
    pub(super) next_id: AtomicU64,
    pub(super) closed: AtomicBool,
    install: AtomicU8,
    scopes: Vec<Weak<()>>,
    pub(super) ui: Arc<UiProgress>,
}
impl Default for WorkerDomain {
    fn default() -> Self {
        Self {
            selector: original_id(),
            entry: None,
            frame: OnceLock::new(),
            next_id: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            install: AtomicU8::new(0),
            scopes: Vec::new(),
            ui: Arc::new(UiProgress::default()),
        }
    }
}
/// Historical handles are weak: they cannot retain an old worker, E1/controller,
/// scheduler or strong PRIMARY/read owner after the actual Tui retires them.
pub(crate) struct BoundClipboard {
    actual: Weak<WorkerDomain>,
}
// 0 = original worker, 1 = staged, 2 = installing, 3 = actual install
// observed, 4 = unknown. This restricts clipboard admission, not Native roles.
struct InstallAttempt {
    actual: Arc<WorkerDomain>,
    complete: bool,
}
impl Drop for InstallAttempt {
    fn drop(&mut self) {
        if !self.complete {
            self.actual.install.store(4, Ordering::Release);
        }
    }
}
impl WorkerDomain {
    pub(super) fn install_held(&self) -> bool {
        !matches!(self.install.load(Ordering::Acquire), 0 | 3)
    }
    fn install_graph(self: &Arc<Self>, install: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
        self.install
            .compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| invalid("clipboard graph install is not a fresh owned stage"))?;
        let mut guard = InstallAttempt {
            actual: self.clone(),
            complete: false,
        };
        install()?;
        self.install.store(3, Ordering::Release);
        guard.complete = true;
        Ok(())
    }
    fn validate_ids_and_scopes(
        &self,
        pending_copy: Option<u64>,
        primary_owner: Option<&Arc<()>>,
    ) -> io::Result<()> {
        // Native hidden selections can retain IDs older than cached completed.
        if pending_copy.is_some_and(|id| id == 0 || id > self.next_id.load(Ordering::Acquire)) {
            return Err(invalid(
                "selection ID outside original clipboard request domain",
            ));
        }
        if primary_owner.is_some_and(|owner| {
            !self
                .scopes
                .iter()
                .any(|known| known.ptr_eq(&Arc::downgrade(owner)))
        }) {
            return Err(invalid("selection owner has another actual Scope graph"));
        }
        Ok(())
    }
}
impl BoundClipboard {
    /// Correlate an enclosing native viewport with the exact installed worker,
    /// not merely another Tui under the same physical entered frontend.
    pub(crate) fn validate_tui(&self, tui: &crate::tui::Tui) -> io::Result<()> {
        let actual = self
            .actual
            .upgrade()
            .ok_or_else(|| invalid("actual clipboard worker incarnation retired"))?;
        if !Arc::ptr_eq(&actual, &tui.clipboard.alab_retained.domain)
            || !actual
                .frame
                .get()
                .is_some_and(|frame| frame.alab_same_scheduler(&tui.frame_requester()))
        {
            return Err(invalid(
                "clipboard binding belongs to another actual Tui worker",
            ));
        }
        self.validate_selection(None, None)
    }
    /// ONE actual enclosing App/all-sessions/queues install; never per view.
    /// Borrow the original operation's closure so preflight refusal cannot drop
    /// its captured owners. A failed/panicked install stays Unknown/held.
    pub(crate) fn install_graph(
        &self,
        tui: &mut crate::tui::Tui,
        install: &mut impl FnMut(&mut crate::tui::Tui) -> io::Result<()>,
    ) -> io::Result<()> {
        self.validate_tui(tui)?;
        let actual = self
            .actual
            .upgrade()
            .ok_or_else(|| invalid("actual clipboard worker incarnation retired"))?;
        actual.install_graph(|| {
            install(tui)?;
            self.validate_tui(tui)
        })
    }
    pub(crate) fn validate_selection(
        &self,
        pending_copy: Option<u64>,
        primary_owner: Option<&Arc<()>>,
    ) -> io::Result<()> {
        let actual = self
            .actual
            .upgrade()
            .ok_or_else(|| invalid("actual clipboard worker incarnation retired"))?;
        if actual.closed.load(Ordering::Acquire) {
            return Err(invalid("actual clipboard worker closed"));
        }
        let entry = actual
            .entry
            .as_ref()
            .and_then(Weak::upgrade)
            .ok_or_else(|| invalid("actual entered clipboard owner missing"))?;
        if !current_retained_frontend().is_some_and(|current| Arc::ptr_eq(&current, &entry)) {
            return Err(invalid("clipboard belongs to another actual E1 scope"));
        }
        if actual.frame.get().is_none() {
            return Err(invalid("actual clipboard scheduler not bound"));
        }
        actual.validate_ids_and_scopes(pending_copy, primary_owner)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Status {
    Confirmed {},
    Unconfirmed {},
}
impl Status {
    fn capture(value: CopyStatus) -> io::Result<Self> {
        match value {
            CopyStatus::Confirmed => Ok(Self::Confirmed {}),
            CopyStatus::Unconfirmed => Ok(Self::Unconfirmed {}),
            CopyStatus::Pending(_) | CopyStatus::Busy => Err(invalid(
                "native completed clipboard result has submission-only status",
            )),
        }
    }
    fn restore(self) -> CopyStatus {
        match self {
            Self::Confirmed {} => CopyStatus::Confirmed,
            Self::Unconfirmed {} => CopyStatus::Unconfirmed,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum ResultData {
    Ok { status: Status },
    Error { message: String },
}
impl ResultData {
    fn capture(value: &CopyResult) -> io::Result<Self> {
        match value {
            Ok(status) => Ok(Self::Ok {
                status: Status::capture(*status)?,
            }),
            Err(message) => Ok(Self::Error {
                message: message.clone(),
            }),
        }
    }
    fn restore(self) -> CopyResult {
        match self {
            Self::Ok { status } => Ok(status.restore()),
            Self::Error { message } => Err(message),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Source {
    Clipboard {},
    Primary {},
}
impl Source {
    fn capture(actual: PasteSource) -> Self {
        match actual {
            PasteSource::Clipboard => Self::Clipboard {},
            PasteSource::Primary => Self::Primary {},
        }
    }
    fn restore(self) -> PasteSource {
        match self {
            Self::Clipboard {} => PasteSource::Clipboard,
            Self::Primary {} => PasteSource::Primary,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadData {
    frames: FrameRef,
    deadline: Clock,
    source: Source,
    #[serde(deserialize_with = "required")]
    owner: Option<ScopeRef>,
    #[serde(deserialize_with = "required")]
    response: Option<()>,
    expired: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ClipboardData {
    schema: u8,
    original: String,
    frame: FrameRef,
    next_id: u64,
    scopes: ScopeData,
    #[serde(deserialize_with = "required")]
    pending: Option<()>,
    #[serde(deserialize_with = "required")]
    completed: Option<(u64, ResultData)>,
    #[serde(deserialize_with = "required")]
    primary: Option<(Id, Option<ScopeRef>)>,
    #[serde(deserialize_with = "required")]
    pending_read: Option<ReadData>,
    #[serde(deserialize_with = "required")]
    read_result: Option<()>,
}
impl ClipboardData {
    /// Capture only the exact original Tui's genuine joined/no-resident worker.
    /// Scene captures ALL real UI strong scopes before these weak references.
    pub(crate) fn capture(
        tui: &crate::tui::Tui,
        joined: &JoinedWorker,
        cells: &mut Cells,
        strings: &mut text::Capture,
        scopes: &ScopeCapture,
    ) -> io::Result<Self> {
        Self::capture_worker(
            &tui.clipboard,
            tui.frame_requester(),
            joined,
            cells,
            strings,
            scopes,
        )
    }
    fn capture_worker(
        worker: &ClipboardWorker,
        frame: FrameRequester,
        joined: &JoinedWorker,
        cells: &mut Cells,
        strings: &mut text::Capture,
        scopes: &ScopeCapture,
    ) -> io::Result<Self> {
        if worker.alab_retained.domain.closed.load(Ordering::Acquire)
            || worker.alab_retained.domain.install_held()
            || !joined.is_for(worker)
        {
            return Err(invalid("clipboard capture has no actual joined resource"));
        }
        if !joined.actual.frames.alab_same_scheduler(&frame) {
            return Err(invalid(
                "joined clipboard preparation has another actual Tui scheduler",
            ));
        }
        let progress = worker
            .alab_retained
            .domain
            .ui
            .state
            .lock()
            .map_err(|_| invalid("original clipboard UI delivery custody poisoned"))?;
        if progress.active || progress.unknown {
            return Err(invalid("original clipboard UI event not fully observed"));
        }
        if worker.pending.is_some()
            || worker.read_result.is_some()
            || worker
                .pending_read
                .as_ref()
                .is_some_and(|read| read.response.is_some())
        {
            return Err(invalid(
                "original clipboard result not delivered before capture",
            ));
        }
        if worker
            .alab_retained
            .frames
            .as_ref()
            .is_some_and(|old| !old.alab_same_scheduler(&frame))
        {
            return Err(invalid("clipboard is from another original Tui scheduler"));
        }
        let pending_read = worker
            .pending_read
            .as_ref()
            .map(|read| {
                Ok::<_, io::Error>(ReadData {
                    frames: cells.frame(&read.frames)?,
                    deadline: Clock::capture(read.deadline).map_err(invalid)?,
                    source: Source::capture(read.source),
                    owner: scopes.weak(&read.owner)?,
                    response: None,
                    expired: read.expired,
                })
            })
            .transpose()?;
        let primary = worker
            .primary
            .as_ref()
            .map(|(value, owner)| Ok::<_, io::Error>((strings.string(value)?, scopes.weak(owner)?)))
            .transpose()?;
        let data = Self {
            schema: 1,
            original: worker.alab_retained.domain.selector.clone(),
            frame: cells.frame(&frame)?,
            next_id: worker.next_id,
            scopes: scopes.export()?,
            pending: None,
            completed: worker
                .completed
                .as_ref()
                .map(|(id, result)| Ok::<_, io::Error>((*id, ResultData::capture(result)?)))
                .transpose()?,
            primary,
            pending_read,
            read_result: None,
        };
        data.validate_data()?;
        Ok(data)
    }
    /// Private injected original backend for the real App callback control.
    /// It produces no native clipboard lease or terminal escape sequence.
    #[cfg(test)]
    pub(crate) fn test_submit_original(
        worker: &mut ClipboardWorker,
        frames: FrameRequester,
    ) -> io::Result<u64> {
        worker
            .start(
                frames.clone(),
                |_, _, setup| {
                    (
                        setup
                            .begin_delivery()
                            .map(|()| crate::clipboard_copy::CopyOutcome::Requested),
                        None,
                    )
                },
                || |_| Ok("fixture read".into()),
            )
            .map_err(invalid)?;
        match worker
            .copy(
                Arc::from("original App callback fixture"),
                CopyFormat::PlainText,
                frames,
            )
            .map_err(invalid)?
        {
            CopyStatus::Pending(id) => Ok(id),
            _ => Err(invalid("original fixture copy was not accepted")),
        }
    }
    #[cfg(test)]
    pub(crate) fn test_join_cleanup(worker: &mut ClipboardWorker) -> io::Result<()> {
        worker.alab_retained.owner_dropped();
        worker.requests.take();
        if let Some(handle) = worker.alab_retained.worker.take() {
            handle
                .join()
                .map_err(|_| invalid("original callback fixture worker panicked"))?;
        }
        Ok(())
    }
    /// Real lower DATA fixture: original no-resident lifecycle, no resource grant.
    #[cfg(test)]
    pub(crate) fn test_joined_original() -> io::Result<Self> {
        let (frames, _events) = FrameRequester::test_channel();
        let mut worker = ClipboardWorker::default();
        let prepared = worker.alab_prepare_join(frames.clone()).map_err(invalid)?;
        if !worker.alab_close_join(&prepared).map_err(invalid)? {
            return Err(invalid("original no-resident close not observed"));
        }
        let JoinObservation::Joined(joined) =
            worker.alab_observe_join(&prepared).map_err(invalid)?
        else {
            return Err(invalid("original no-resident join not observed"));
        };
        Self::capture_worker(
            &worker,
            frames,
            &joined,
            &mut Cells::default(),
            &mut text::Capture::default(),
            &ScopeCapture::default(),
        )
    }
    pub(crate) fn validate_data(&self) -> io::Result<()> {
        self.scopes.validate_data()?;
        if self.schema != 1
            || !valid_id(&self.original)
            || self.pending.is_some()
            || self.read_result.is_some()
            || self
                .pending_read
                .as_ref()
                .is_some_and(|read| read.response.is_some())
            || self
                .completed
                .as_ref()
                .is_some_and(|(id, _)| *id == 0 || *id > self.next_id)
        {
            return Err(invalid("invalid closed original clipboard checkpoint"));
        }
        Ok(())
    }
}
/// Owns the mandatory WHOLE decoded input from one genuine actual Native E1.
/// It never accepts a separately supplied DTO/selector with another entry.
pub(crate) struct SealedClipboard {
    actual: Arc<DecodedUiTransfer>,
}
impl SealedClipboard {
    pub(crate) fn from_decoded(actual: &Arc<DecodedUiTransfer>) -> io::Result<Arc<Self>> {
        actual.clipboard_data().validate_data()?;
        Ok(Arc::new(Self {
            actual: actual.clone(),
        }))
    }
    pub(crate) fn from_entered(entry: &Arc<EnteredRetainedFrontend>) -> io::Result<Arc<Self>> {
        let actual = Arc::new(DecodedUiTransfer::from_entered(entry.clone()).map_err(invalid)?);
        Self::from_decoded(&actual)
    }
    pub(crate) fn frame_ref(&self) -> FrameRef {
        self.actual.clipboard_data().frame
    }
    pub(crate) fn stage(self: &Arc<Self>) -> io::Result<PreparedClipboard> {
        let data = self.actual.clipboard_data();
        data.validate_data()?;
        Ok(PreparedClipboard {
            owner: self.clone(),
            scopes: ScopeRestored::new(data.scopes.clone())?,
        })
    }
}
pub(crate) struct PreparedClipboard {
    owner: Arc<SealedClipboard>,
    scopes: ScopeRestored,
}
pub(crate) struct BindFailure {
    pub(crate) original: PreparedClipboard,
    pub(crate) error: io::Error,
}
impl PreparedClipboard {
    pub(crate) fn scopes(&self) -> &ScopeRestored {
        &self.scopes
    }
    /// Caller stages the real scene strong owners first. This starts ONLY the
    /// original idle service in the real Tui; no pending ID is completed/replayed.
    pub(crate) fn bind_current_tui(
        self,
        tui: &mut crate::tui::Tui,
        strings: &text::Restored,
        resources: &RestoreContext,
    ) -> Result<BoundClipboard, BindFailure> {
        match self.bind_borrowed(tui, strings, resources) {
            Ok(bound) => Ok(bound),
            Err(error) => Err(BindFailure {
                original: self,
                error,
            }),
        }
    }
    fn bind_borrowed(
        &self,
        tui: &mut crate::tui::Tui,
        strings: &text::Restored,
        resources: &RestoreContext,
    ) -> io::Result<BoundClipboard> {
        let entry = self.owner.actual.entry();
        if !current_retained_frontend().is_some_and(|actual| Arc::ptr_eq(&actual, entry)) {
            return Err(invalid("clipboard restore lost SAME actual E1 scope"));
        }
        let data = self.owner.actual.clipboard_data();
        data.validate_data()?;
        self.scopes.validate_roots()?;
        let frame = resources.frame(data.frame)?;
        if !frame.alab_same_scheduler(&tui.frame_requester()) {
            return Err(invalid(
                "clipboard restore has another actual Tui scheduler",
            ));
        }
        let old = &tui.clipboard;
        if old.requests.is_some()
            || old.responses.is_some()
            || old.pending.is_some()
            || old.next_id != 0
            || old.completed.is_some()
            || old.primary.is_some()
            || old.pending_read.is_some()
            || old.read_result.is_some()
            || old.alab_retained.life != ThreadLife::NeverStarted
            || old.alab_retained.worker.is_some()
            || old.alab_retained.current.is_some()
        {
            return Err(invalid(
                "destination original clipboard already accepted work",
            ));
        }
        let primary = data
            .primary
            .as_ref()
            .map(|(value, owner)| {
                Ok::<_, io::Error>((strings.string(*value)?, self.scopes.weak(*owner)?))
            })
            .transpose()?;
        let pending_read = data
            .pending_read
            .as_ref()
            .map(|read| {
                Ok::<_, io::Error>(PendingRead {
                    frames: resources.frame(read.frames)?,
                    deadline: read.deadline.restore().map_err(invalid)?,
                    source: read.source.clone().restore(),
                    owner: self.scopes.weak(read.owner)?,
                    response: None,
                    expired: read.expired,
                })
            })
            .transpose()?;
        let domain = Arc::new(WorkerDomain {
            selector: data.original.clone(),
            entry: Some(Arc::downgrade(entry)),
            frame: OnceLock::new(),
            next_id: AtomicU64::new(data.next_id),
            closed: AtomicBool::new(false),
            install: AtomicU8::new(0),
            scopes: self.scopes.weak_index(),
            ui: Arc::new(UiProgress::default()),
        });
        let _ = domain.frame.set(frame.clone());
        let mut worker = ClipboardWorker {
            requests: None,
            responses: None,
            pending: None,
            next_id: data.next_id,
            completed: data
                .completed
                .clone()
                .map(|(id, result)| (id, result.restore())),
            primary,
            pending_read,
            read_result: None,
            alab_retained: Custody {
                domain: domain.clone(),
                ..Custody::default()
            },
        };
        // Original ensure_started has no post-spawn fallible step. Every
        // failure here is before birth; the destination and sealed owner stay.
        worker.ensure_started(frame).map_err(invalid)?;
        // The local unpublished service accepted no requests. Restrict it BEFORE
        // publishing native deferred fields into the actual destination Tui.
        domain.install.store(1, Ordering::Release);
        tui.clipboard = worker;
        Ok(BoundClipboard {
            actual: Arc::downgrade(&domain),
        })
    }
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(decoder)
}

#[cfg(test)]
#[path = "retained_clipboard_checkpoint_tests.rs"]
mod tests;
