// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Complete append-only BackendSessions DATA plus consuming actual resources.
//! Capture borrows input leaves into the enclosing scene. No local graph encode,
//! ConfigBuilder, endpoint connect, Native grant or extra Source S2 occurs here.
use super::*;
use codex_app_server_client::AppServerRequestHandle;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::sync::Arc;
#[path = "retained_backend_events.rs"]
mod events;
pub(crate) use events::EventData;
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
fn required<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d)
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct BackendRef(u32);
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry<I> {
    #[serde(deserialize_with = "required")]
    preference_key: Option<String>,
    #[serde(deserialize_with = "required")]
    service: Option<String>,
    transport: u8,
    listening: bool,
    pending_events: Vec<EventData>,
    side_startup: Vec<EventData>,
    session_deferred: Vec<EventData>,
    parked_inputs: Vec<(codex_protocol::ThreadId, I)>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data<I, S> {
    schema: u8,
    capture: uuid::Uuid,
    entries: Vec<Entry<I>>,
    displayed: BackendRef,
    settings: Vec<(BackendRef, S)>,
    next_source: usize,
}
pub(crate) struct CaptureRefs {
    owners: Vec<BackendId>,
}
impl CaptureRefs {
    pub(crate) fn reference(&self, actual: BackendId) -> io::Result<BackendRef> {
        self.owners
            .iter()
            .position(|owner| *owner == actual)
            .and_then(|i| u32::try_from(i).ok())
            .map(BackendRef)
            .ok_or_else(|| invalid("App root references an unregistered backend"))
    }
}
/// This original parked type contains plain draft/queue snapshots, no live
/// TextArea or editor-generation Arc. Capture its pure DATA/clock tuple ONCE,
/// then share that same observation with the enclosing scene and local pin.
/// No per-input editor graph is encoded or decoded here.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InputData {
    native: serde_json::Value,
}
impl InputData {
    fn capture(actual: &crate::chatwidget::ThreadInputState) -> io::Result<Self> {
        actual.alab_validate_retained_backend_input()?;
        Ok(Self {
            native: serde_json::to_value(actual).map_err(invalid)?,
        })
    }
    fn prepare(&self) -> io::Result<crate::chatwidget::ThreadInputState> {
        let native: crate::chatwidget::ThreadInputState =
            serde_json::from_value(self.native.clone()).map_err(invalid)?;
        native.alab_validate_retained_backend_input()?;
        Ok(native)
    }
}
struct Pin {
    request: codex_app_server_client::OriginalRequestIdentity,
    pending: Vec<AppServerEvent>,
    side: Vec<AppServerEvent>,
    inputs: HashMap<codex_protocol::ThreadId, crate::chatwidget::ThreadInputState>,
    input_data: Vec<(codex_protocol::ThreadId, InputData)>,
}
struct OriginalEntry {
    preference_key: Option<String>,
    listening: bool,
    pending_events: VecDeque<AppServerSessionEvent>,
    side_startup: Vec<AppServerSessionEvent>,
    parked_inputs: HashMap<codex_protocol::ThreadId, crate::chatwidget::ThreadInputState>,
}
/// Actual source resources; serde DATA cannot reconstruct this local proof.
pub(crate) struct OriginalPins {
    capture: uuid::Uuid,
    entries: Vec<Arc<Pin>>,
}
pub(crate) struct Captured<S> {
    pub(crate) data: Data<InputData, S>,
    pub(crate) refs: CaptureRefs,
    pub(crate) pins: OriginalPins,
}
fn transport(session: &AppServerSession) -> u8 {
    match session.request_handle() {
        AppServerRequestHandle::InProcess(_) => 0,
        AppServerRequestHandle::Remote(_) => 1,
        AppServerRequestHandle::Cloud(_) => 2,
    }
}
fn service(session: &AppServerSession) -> io::Result<Option<String>> {
    session
        .retained_frontend()
        .map(|tenure| tenure.alab_backend_service_data().map(str::to_owned))
        .transpose()
}
fn event_data<'a>(
    events: impl IntoIterator<Item = &'a AppServerSessionEvent>,
    owner: BackendId,
) -> io::Result<Vec<EventData>> {
    events
        .into_iter()
        .map(|event| {
            if event.backend_id != owner {
                return Err(invalid("queued event changed actual backend owner"));
            }
            EventData::capture(&event.event)
        })
        .collect()
}
impl BackendSessions {
    pub(crate) fn alab_capture_all<S>(
        &self,
        mut settings_data: impl FnMut(BackendRef, &BackendSettings) -> io::Result<S>,
    ) -> io::Result<Captured<S>> {
        if self.entries.is_empty()
            || self.entries.len() > 1024
            || self.displayed.0 >= self.entries.len()
            || self.next_source >= self.entries.len()
        {
            return Err(invalid("backend aggregate native slot/fairness bounds"));
        }
        let refs = CaptureRefs {
            owners: self
                .entries
                .iter()
                .map(|entry| entry.session.backend_id())
                .collect(),
        };
        let capture = uuid::Uuid::new_v4();
        let mut pins = Vec::new();
        let mut entries = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for entry in &self.entries {
            let owner = entry.session.backend_id();
            if !seen.insert(owner) {
                return Err(invalid("duplicate actual backend owner"));
            }
            let mut inputs = entry.parked_inputs.iter().collect::<Vec<_>>();
            inputs.sort_by_key(|(id, _)| id.to_string());
            let input_data = inputs
                .into_iter()
                .map(|(id, input)| Ok((*id, InputData::capture(input)?)))
                .collect::<io::Result<Vec<_>>>()?;
            entries.push(Entry {
                preference_key: entry.preference_key.clone(),
                service: service(&entry.session)?,
                transport: transport(&entry.session),
                listening: entry.listening,
                pending_events: event_data(&entry.pending_events, owner)?,
                side_startup: event_data(&entry.side_startup, owner)?,
                session_deferred: entry.session.alab_backend_deferred_data()?,
                parked_inputs: input_data.clone(),
            });
            pins.push(Arc::new(Pin {
                request: entry.session.request_handle().retained_original_resource(),
                pending: entry
                    .pending_events
                    .iter()
                    .map(|e| e.event.clone())
                    .collect(),
                side: entry.side_startup.iter().map(|e| e.event.clone()).collect(),
                inputs: entry.parked_inputs.clone(),
                input_data,
            }));
        }
        let mut settings = self
            .settings
            .iter()
            .map(|(owner, value)| Ok((refs.reference(*owner)?, value)))
            .collect::<io::Result<Vec<_>>>()?;
        settings.sort_by_key(|(slot, _)| slot.0);
        let settings = settings
            .into_iter()
            .map(|(slot, value)| Ok((slot, settings_data(slot, value)?)))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Captured {
            data: Data {
                schema: 1,
                capture,
                entries,
                displayed: BackendRef(u32::try_from(self.displayed.0).map_err(invalid)?),
                settings,
                next_source: self.next_source,
            },
            refs,
            pins: OriginalPins {
                capture,
                entries: pins,
            },
        })
    }
}
enum Origin {
    Original {
        capture: uuid::Uuid,
        slot: BackendRef,
        pin: Option<Arc<Pin>>,
    },
    Retained,
}
/// Supplied actual owning session plus actual original-installed settings. No
/// endpoint/session label/DTO can construct an AppServerSession here.
pub(crate) struct SessionResource<S> {
    session: AppServerSession,
    settings: Option<(BackendSettings, S)>,
    origin: Origin,
    original_entry: Option<OriginalEntry>,
}
impl OriginalPins {
    fn bind<S>(
        &self,
        slot: BackendRef,
        entry: SessionEntry,
        settings: Option<(BackendSettings, S)>,
    ) -> SessionResource<S> {
        // Move the complete actual original entry, not only its client. A
        // post-capture event/input change must refuse while remaining owned.
        let pin = self.entries.get(slot.0 as usize).cloned();
        let SessionEntry {
            session,
            preference_key,
            listening,
            pending_events,
            side_startup,
            parked_inputs,
        } = entry;
        SessionResource {
            session,
            settings,
            origin: Origin::Original {
                capture: self.capture,
                slot,
                pin,
            },
            original_entry: Some(OriginalEntry {
                preference_key,
                listening,
                pending_events,
                side_startup,
                parked_inputs,
            }),
        }
    }
}
impl<S> SessionResource<S> {
    pub(in crate::app) fn retained_candidate(
        actual: AppServerSession,
        settings: Option<(BackendSettings, S)>,
    ) -> Self {
        Self {
            session: actual,
            settings,
            origin: Origin::Retained,
            original_entry: None,
        }
    }
}
/// Actual slot resources stay here through every refused preflight.
pub(crate) struct Resources<S> {
    slots: Vec<SessionResource<S>>,
    orphan_settings: HashMap<BackendId, BackendSettings>,
}
impl<S> Resources<S> {
    pub(crate) fn new(slots: Vec<SessionResource<S>>) -> Self {
        Self {
            slots,
            orphan_settings: HashMap::new(),
        }
    }
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }
    pub(crate) fn actual_session(&self, slot: BackendRef) -> io::Result<&AppServerSession> {
        self.slots
            .get(slot.0 as usize)
            .map(|resource| &resource.session)
            .ok_or_else(|| invalid("actual aggregate session slot missing"))
    }
}
impl BackendSessions {
    /// Local aggregate round-trip uses the SAME actual original clients, not
    /// reconstructed endpoints or DTO-made sessions. All original native
    /// settings stay owned, including any unexpected unbound settings.
    pub(crate) fn alab_into_original_resources<S>(
        self,
        pins: &OriginalPins,
        mut settings_data: impl FnMut(&BackendSettings) -> S,
    ) -> Resources<S> {
        let Self {
            entries,
            mut settings,
            ..
        } = self;
        let slots = entries
            .into_iter()
            .enumerate()
            .map(|(index, entry)| {
                let native = settings.remove(&entry.session.backend_id()).map(|native| {
                    let data = settings_data(&native);
                    (native, data)
                });
                pins.bind(
                    BackendRef(u32::try_from(index).unwrap_or(u32::MAX)),
                    entry,
                    native,
                )
            })
            .collect();
        Resources {
            slots,
            orphan_settings: settings,
        }
    }
}
pub(crate) struct RestoreRefs {
    owners: Vec<BackendId>,
}
impl RestoreRefs {
    pub(crate) fn owner(&self, key: BackendRef) -> io::Result<BackendId> {
        self.owners
            .get(key.0 as usize)
            .copied()
            .ok_or_else(|| invalid("restored App backend reference missing"))
    }
}
pub(crate) struct Prepared<S> {
    backends: BackendSessions,
    pub(crate) refs: RestoreRefs,
    pub(crate) settings_data: Vec<(BackendRef, S)>,
}
pub(crate) struct Refused<S> {
    pub(crate) error: io::Error,
    pub(crate) data: Data<InputData, S>,
    pub(crate) resources: Resources<S>,
}
type PreparedBackendEvents = (
    Vec<AppServerEvent>,
    Vec<AppServerEvent>,
    HashMap<codex_protocol::ThreadId, crate::chatwidget::ThreadInputState>,
);

impl<S: Serialize> Data<InputData, S> {
    fn validate(&self, resources: &Resources<S>) -> io::Result<Vec<PreparedBackendEvents>> {
        if self.schema != 1
            || self.entries.is_empty()
            || self.entries.len() > 1024
            || !resources.orphan_settings.is_empty()
            || resources.slots.len() != self.entries.len()
            || self.displayed.0 as usize >= self.entries.len()
            || self.next_source >= self.entries.len()
        {
            return Err(invalid("backend aggregate schema/slot/fairness bounds"));
        }
        let mut prepared_events = Vec::with_capacity(self.entries.len());
        let mut owners = std::collections::HashSet::new();
        let mut preferences = std::collections::HashSet::new();
        let mut settings = std::collections::HashSet::new();
        for (slot, _) in &self.settings {
            if slot.0 as usize >= self.entries.len() || !settings.insert(slot.0) {
                return Err(invalid("backend aggregate duplicate/unknown settings slot"));
            }
        }
        for (index, (entry, resource)) in self.entries.iter().zip(&resources.slots).enumerate() {
            if !resource.session.alab_backend_is_owning()
                || !owners.insert(resource.session.backend_id())
                || transport(&resource.session) != entry.transport
            {
                return Err(invalid(
                    "backend aggregate lacks distinct actual owning sessions",
                ));
            }
            if let Some(key) = &entry.preference_key
                && (key.is_empty()
                    || !key
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
                    || !preferences.insert(key))
            {
                return Err(invalid("backend aggregate preference keys differ"));
            }
            if service(&resource.session)? != entry.service {
                return Err(invalid("backend resource changed actual retained service"));
            }
            let original = match &resource.origin {
                Origin::Original { capture, slot, pin } => {
                    *capture == self.capture
                        && slot.0 as usize == index
                        && pin.as_ref().is_some_and(|pin| {
                            pin.request.matches(&resource.session.request_handle())
                        })
                }
                Origin::Retained => entry.service.is_some()
                    && (!entry.listening
                        || resource.session.retained_frontend().is_some_and(
                            crate::owned_local_frontend::FrontendTenure::alab_backend_delivery_open,
                        )),
            };
            if !original {
                return Err(invalid(
                    "backend slot lacks its actual original or retained resource",
                ));
            }
            if matches!(&resource.origin, Origin::Retained) {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "actual cross-exec session state/jobs and callback owners are not yet bound; service DATA alone is insufficient",
                ));
            }
            let mut inputs = HashMap::new();
            for (id, input) in &entry.parked_inputs {
                if inputs.insert(*id, input.prepare()?).is_some() {
                    return Err(invalid("duplicate original parked input"));
                }
            }
            for event in entry
                .pending_events
                .iter()
                .chain(&entry.side_startup)
                .chain(&entry.session_deferred)
            {
                event.validate_data()?;
            }
            if let Origin::Original { pin: Some(pin), .. } = &resource.origin {
                let current = resource
                    .original_entry
                    .as_ref()
                    .ok_or_else(|| invalid("original native entry resource missing"))?;
                if current.preference_key != entry.preference_key
                    || current.listening != entry.listening
                    || current.parked_inputs != pin.inputs
                    || entry.parked_inputs != pin.input_data
                    || event_data(&current.pending_events, resource.session.backend_id())?
                        != entry.pending_events
                    || event_data(&current.side_startup, resource.session.backend_id())?
                        != entry.side_startup
                    || pin
                        .pending
                        .iter()
                        .map(EventData::capture)
                        .collect::<io::Result<Vec<_>>>()?
                        != entry.pending_events
                    || pin
                        .side
                        .iter()
                        .map(EventData::capture)
                        .collect::<io::Result<Vec<_>>>()?
                        != entry.side_startup
                {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "actual original entry changed after captured cut; resources remain owned",
                    ));
                }
            } else if entry
                .pending_events
                .iter()
                .chain(&entry.side_startup)
                .chain(&entry.session_deferred)
                .any(EventData::is_request)
            {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "retained queued request still requires its exact private Source callback owner/outcome cut",
                ));
            }
            if resource.session.alab_backend_deferred_data()? != entry.session_deferred {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "actual same-session deferred queue resource is not yet reconciled",
                ));
            }
            match (
                self.settings
                    .iter()
                    .find(|(slot, _)| slot.0 as usize == index),
                &resource.settings,
            ) {
                (Some((_, saved)), Some((_, actual)))
                    if serde_json::to_value(saved).map_err(invalid)?
                        == serde_json::to_value(actual).map_err(invalid)? => {}
                (None, None) => {}
                _ => {
                    return Err(invalid(
                        "actual backend settings resource/complete DATA is unbound",
                    ));
                }
            }
            let events = match &resource.origin {
                Origin::Original { pin: Some(pin), .. } => (pin.pending.clone(), pin.side.clone()),
                Origin::Original { pin: None, .. } => {
                    return Err(invalid("original event resource was not bound"));
                }
                Origin::Retained => (
                    entry
                        .pending_events
                        .iter()
                        .cloned()
                        .map(events::EventData::into_data_event)
                        .collect::<io::Result<Vec<_>>>()?,
                    entry
                        .side_startup
                        .iter()
                        .cloned()
                        .map(events::EventData::into_data_event)
                        .collect::<io::Result<Vec<_>>>()?,
                ),
            };
            prepared_events.push((events.0, events.1, inputs));
        }
        Ok(prepared_events)
    }
    pub(crate) fn prepare(
        self,
        resources: Resources<S>,
    ) -> std::result::Result<Prepared<S>, Refused<S>> {
        let prepared_events = match self.validate(&resources) {
            Ok(events) => events,
            Err(error) => {
                return Err(Refused {
                    error,
                    data: self,
                    resources,
                });
            }
        };
        // All fallible validation completes before moving any actual session.
        let mut native_settings = HashMap::new();
        let mut entries = Vec::with_capacity(self.entries.len());
        let mut owners = Vec::with_capacity(self.entries.len());
        for ((entry, resource), (pending, side, inputs)) in self
            .entries
            .into_iter()
            .zip(resources.slots)
            .zip(prepared_events)
        {
            let owner = resource.session.backend_id();
            owners.push(owner);
            if let Some((settings, _)) = resource.settings {
                native_settings.insert(owner, settings);
            }
            let (pending_events, side_startup, parked_inputs) = match resource.original_entry {
                Some(original) => (
                    original.pending_events,
                    original.side_startup,
                    original.parked_inputs,
                ),
                None => (
                    pending
                        .into_iter()
                        .map(|event| AppServerSessionEvent {
                            backend_id: owner,
                            event,
                        })
                        .collect(),
                    side.into_iter()
                        .map(|event| AppServerSessionEvent {
                            backend_id: owner,
                            event,
                        })
                        .collect(),
                    inputs,
                ),
            };
            entries.push(SessionEntry {
                preference_key: entry.preference_key,
                session: resource.session,
                listening: entry.listening,
                pending_events,
                side_startup,
                parked_inputs,
            });
        }
        Ok(Prepared {
            backends: BackendSessions {
                entries,
                displayed: SessionSlot(self.displayed.0 as usize),
                settings: native_settings,
                next_source: self.next_source,
            },
            refs: RestoreRefs { owners },
            settings_data: self.settings,
        })
    }
}
impl<S> Prepared<S> {
    pub(in crate::app) fn into_parts(self) -> (BackendSessions, RestoreRefs, Vec<(BackendRef, S)>) {
        (self.backends, self.refs, self.settings_data)
    }
}
