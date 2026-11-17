// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Local native pause aliases, including guard-owned retired sender generations.
//! No callback, OS clipboard result, Source/Core or Native grant is reconstructed.
use super::*;
use codex_alab_tui::upgrade_editor_graph as graph;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;
#[path = "retained_app_event_channel.rs"]
mod event_channel;
pub(crate) use event_channel::Census;
pub(crate) use event_channel::channel;

const MAX_NODES: usize = 1_000_000;
pub(crate) type GuardRef = graph::GenerationId;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct SenderRef(u32);
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum Kind {
    Widget,
    RetiredLocal,
    ClosedLocal,
}
impl<'de> Deserialize<'de> for Kind {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        match String::deserialize(decoder)?.as_str() {
            "widget" => Ok(Self::Widget),
            "retiredLocal" => Ok(Self::RetiredLocal),
            "closedLocal" => Ok(Self::ClosedLocal),
            _ => Err(serde::de::Error::custom(
                "unknown original local copy sender kind",
            )),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SenderData {
    voice_only: bool,
    kind: Kind,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Data {
    schema: u8,
    senders: Vec<SenderData>,
    guards: Vec<SenderRef>,
}
struct SourceSender {
    actual: AppEventSender,
    kind: Kind,
}

/// Restrict-only Drop lifecycle. Closed is distinct from not-yet-installed:
/// a later C1 can capture a genuinely installed closed sender without arming it.
#[derive(Debug)]
pub(crate) struct DropState(AtomicU8);
impl DropState {
    fn dormant() -> Self {
        Self(AtomicU8::new(0))
    }
    fn captured(&self) -> bool {
        matches!(self.0.load(Ordering::Acquire), 1 | 2)
    }
    fn closed(&self) -> bool {
        self.0.load(Ordering::Acquire) == 2
    }
    fn event(&self) -> bool {
        self.0.load(Ordering::Acquire) == 1
    }
    fn install(&self, closed: bool) {
        self.0.store(if closed { 2 } else { 1 }, Ordering::Release);
    }
}
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
fn same_sender(a: &AppEventSender, b: &AppEventSender) -> bool {
    a.app_event_tx.same_channel(&b.app_event_tx) && Arc::ptr_eq(&a.voice_only, &b.voice_only)
}
fn plain_sender(actual: &AppEventSender) -> io::Result<()> {
    if actual.alab_original_input.is_some() {
        Err(invalid(
            "copy sender cannot acquire a claimed callback owner",
        ))
    } else {
        Ok(())
    }
}
fn captured(actual: &AppEventSender) -> bool {
    actual
        .alab_copy_guard_drop
        .as_ref()
        .is_none_or(|state| state.captured())
}
fn known_closed(actual: &AppEventSender) -> bool {
    actual
        .alab_copy_guard_drop
        .as_ref()
        .is_some_and(|state| state.closed())
}
pub(super) fn release_event(actual: &AppEventSender) -> bool {
    actual
        .alab_copy_guard_drop
        .as_ref()
        .is_none_or(|state| state.event())
}

#[derive(Default)]
pub(crate) struct Capture {
    census: Option<Census>,
    senders: Vec<SourceSender>,
    guards: graph::Capture<CopyInputGuard>,
}
impl Capture {
    pub(crate) fn for_app(census: &Census, actual: &AppEventSender) -> io::Result<Self> {
        plain_sender(actual)?;
        census.require_live(actual)?;
        Ok(Self {
            census: Some(census.clone()),
            ..Self::default()
        })
    }
    /// Register every actual Widget owner before exporting the full graph.
    /// A later registration upgrades an already discovered live retired alias.
    pub(crate) fn sender(&mut self, actual: &AppEventSender) -> io::Result<SenderRef> {
        plain_sender(actual)?;
        if actual.alab_copy_guard_drop.is_some() || actual.app_event_tx.is_closed() {
            return Err(invalid(
                "widget sender is closed or is a guard-only restored generation",
            ));
        }
        if let Some(census) = &self.census {
            census.require_live(actual)?;
        }
        self.register(actual, Kind::Widget)
    }
    fn register(&mut self, actual: &AppEventSender, kind: Kind) -> io::Result<SenderRef> {
        if let Some(index) = self
            .senders
            .iter()
            .position(|entry| same_sender(&entry.actual, actual))
        {
            let entry = &mut self.senders[index];
            if entry.kind == Kind::ClosedLocal && kind != Kind::ClosedLocal {
                return Err(invalid(
                    "closed original sender cannot become an active widget",
                ));
            }
            if kind == Kind::Widget {
                entry.kind = kind;
            }
            return Ok(SenderRef(u32::try_from(index).map_err(invalid)?));
        }
        if self.senders.len() >= MAX_NODES {
            return Err(invalid("copy sender capacity"));
        }
        let id = SenderRef(u32::try_from(self.senders.len()).map_err(invalid)?);
        self.senders.push(SourceSender {
            actual: actual.clone(),
            kind,
        });
        Ok(id)
    }
    pub(crate) fn strong(&mut self, actual: &Arc<CopyInputGuard>) -> io::Result<GuardRef> {
        plain_sender(&actual.0)?;
        if !captured(&actual.0) {
            return Err(invalid("uninstalled native copy guard remains held"));
        }
        if !self
            .senders
            .iter()
            .any(|sender| same_sender(&sender.actual, &actual.0))
        {
            let census = self
                .census
                .as_ref()
                .ok_or_else(|| invalid("guard has no actual widget or App receiver census"))?;
            let kind = if known_closed(&actual.0) || actual.0.app_event_tx.is_closed() {
                Kind::ClosedLocal
            } else {
                census.require_live(&actual.0)?;
                Kind::RetiredLocal
            };
            self.register(&actual.0, kind)?;
        }
        let id = self.guards.strong(actual);
        if self.guards.values().len() > MAX_NODES {
            return Err(invalid("copy guard capacity"));
        }
        Ok(id)
    }
    pub(crate) fn weak(&self, actual: &Weak<CopyInputGuard>) -> io::Result<Option<GuardRef>> {
        self.guards.weak(actual).map_err(invalid)
    }
    pub(crate) fn encode(&self, max_bytes: usize) -> io::Result<Vec<u8>> {
        if let Some(census) = &self.census {
            census.require_open()?;
        }
        let guards = self
            .guards
            .values()
            .map(|guard| {
                self.senders
                    .iter()
                    .position(|sender| same_sender(&sender.actual, &guard.0))
                    .ok_or_else(|| invalid("original copy sender missing"))
                    .and_then(|index| u32::try_from(index).map(SenderRef).map_err(invalid))
            })
            .collect::<io::Result<_>>()?;
        let senders = self
            .senders
            .iter()
            .map(|sender| {
                let closed = sender.actual.app_event_tx.is_closed() || known_closed(&sender.actual);
                if closed != (sender.kind == Kind::ClosedLocal) {
                    return Err(invalid(
                        "original copy sender closure changed before capture",
                    ));
                }
                if let Some(census) = &self.census
                    && !closed
                {
                    census.require_live(&sender.actual)?;
                }
                Ok(SenderData {
                    voice_only: sender.actual.voice_only.load(Ordering::Acquire),
                    kind: sender.kind,
                })
            })
            .collect::<io::Result<_>>()?;
        let bytes = serde_json::to_vec(&Data {
            schema: 2,
            senders,
            guards,
        })
        .map_err(invalid)?;
        if max_bytes == 0 || bytes.len() > max_bytes {
            return Err(invalid("copy graph byte budget"));
        }
        Ok(bytes)
    }
}
pub(crate) struct Prepared {
    data: Data,
    actual: Vec<Option<AppEventSender>>,
}
impl Prepared {
    pub(crate) fn decode(bytes: &[u8], max_bytes: usize) -> io::Result<Self> {
        if max_bytes == 0 || bytes.len() > max_bytes {
            return Err(invalid("copy graph byte budget"));
        }
        let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(invalid)?;
        let data: Data = serde_json::from_slice(bytes).map_err(invalid)?;
        if raw != serde_json::to_value(&data).map_err(invalid)?
            || data.schema != 2
            || data.senders.len() > MAX_NODES
            || data.guards.len() > MAX_NODES
            || data
                .guards
                .iter()
                .any(|sender| sender.0 as usize >= data.senders.len())
        {
            return Err(invalid("copy graph schema/identity/capacity"));
        }
        let actual = vec![None; data.senders.len()];
        Ok(Self { data, actual })
    }
    pub(crate) fn app_sender_refs(&self) -> io::Result<Vec<SenderRef>> {
        self.data
            .senders
            .iter()
            .enumerate()
            .filter(|(_, sender)| sender.kind != Kind::Widget)
            .map(|(index, _)| u32::try_from(index).map(SenderRef).map_err(invalid))
            .collect()
    }
    pub(crate) fn bind(&mut self, id: SenderRef, actual: &AppEventSender) -> io::Result<()> {
        plain_sender(actual)?;
        if actual.alab_copy_guard_drop.is_some() || actual.app_event_tx.is_closed() {
            return Err(invalid("actual widget sender is closed or guard-only"));
        }
        let saved = self
            .data
            .senders
            .get(id.0 as usize)
            .ok_or_else(|| invalid("unknown widget sender reference"))?;
        if saved.kind != Kind::Widget
            || saved.voice_only != actual.voice_only.load(Ordering::Acquire)
        {
            return Err(invalid("actual widget kind/voice policy differs"));
        }
        if let Some(original) = &self.actual[id.0 as usize] {
            return if same_sender(original, actual) {
                Ok(())
            } else {
                Err(invalid("copy sender already has another actual widget"))
            };
        }
        if self
            .actual
            .iter()
            .flatten()
            .any(|original| same_sender(original, actual))
        {
            return Err(invalid("distinct widget generations cannot merge"));
        }
        self.actual[id.0 as usize] = Some(actual.clone());
        Ok(())
    }
    /// Rebind a guard-owned PURE local sender using the real new App pair.
    /// ClosedLocal remains explicitly no-event; it never acquires an active Drop.
    pub(crate) fn bind_retired(
        &mut self,
        id: SenderRef,
        census: &Census,
        actual: &AppEventSender,
    ) -> io::Result<()> {
        plain_sender(actual)?;
        census.require_live(actual)?;
        if actual.alab_copy_guard_drop.is_some() {
            return Err(invalid("actual App binding is a guard-only sender"));
        }
        let saved = self
            .data
            .senders
            .get(id.0 as usize)
            .ok_or_else(|| invalid("unknown local sender reference"))?;
        if saved.kind == Kind::Widget {
            return Err(invalid("widget reference requires its real Widget owner"));
        }
        if let Some(original) = &self.actual[id.0 as usize] {
            return if census.matches(original)
                && saved.voice_only == original.voice_only.load(Ordering::Acquire)
            {
                Ok(())
            } else {
                Err(invalid("local sender already has another actual App owner"))
            };
        }
        let mut rebound = actual.clone();
        // The original UI voice-generation is local DATA, not a new Widget,
        // callback owner, Source/Core resource or a sender/channel from JSON.
        rebound.voice_only = Arc::new(AtomicBool::new(saved.voice_only));
        self.actual[id.0 as usize] = Some(rebound);
        Ok(())
    }
    pub(crate) fn restore(self) -> io::Result<Restored> {
        let actual = self
            .actual
            .into_iter()
            .map(|actual| {
                actual.ok_or_else(|| invalid("copy sender has no real destination binding"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let Data {
            senders, guards, ..
        } = self.data;
        let mut arms = Vec::with_capacity(guards.len());
        let guards = guards
            .into_iter()
            .map(|sender| {
                let arm = Arc::new(DropState::dormant());
                let mut actual = actual[sender.0 as usize].clone();
                actual.alab_copy_guard_drop = Some(arm.clone());
                arms.push((arm, senders[sender.0 as usize].kind == Kind::ClosedLocal));
                CopyInputGuard(actual)
            })
            .collect();
        // Retired/closed rows are ONLY guard-Drop bookkeeping. They must never
        // become general native UI senders through a forged pane reference.
        let senders = actual
            .into_iter()
            .zip(senders)
            .map(|(actual, saved)| (saved.kind == Kind::Widget).then_some(actual))
            .collect();
        Ok(Restored {
            guards: graph::Restore::new(guards),
            arms,
            senders,
        })
    }
}
pub(crate) struct Restored {
    guards: graph::Restore<CopyInputGuard>,
    arms: Vec<(Arc<DropState>, bool)>,
    senders: Vec<Option<AppEventSender>>,
}
impl Restored {
    /// SAME already-bound actual sender/voice generation; never allocate a channel.
    /// The enclosing scene releases this temporary graph after real installation.
    pub(crate) fn sender(&self, reference: SenderRef) -> io::Result<AppEventSender> {
        self.senders
            .get(reference.0 as usize)
            .and_then(Option::as_ref)
            .cloned()
            .ok_or_else(|| invalid("native pane sender has no actual Widget binding"))
    }
    pub(crate) fn strong(&self, id: GuardRef) -> io::Result<Arc<CopyInputGuard>> {
        self.guards.strong(id).map_err(invalid)
    }
    pub(crate) fn weak(&self, id: Option<GuardRef>) -> io::Result<Weak<CopyInputGuard>> {
        id.map(|id| self.guards.weak(id).map_err(invalid))
            .transpose()
            .map(Option::unwrap_or_default)
    }
    /// Actual whole-App graph installation must succeed in this synchronous
    /// closure under the retained input hold; decoding alone never arms Drops.
    pub(crate) fn install_with<T>(
        self,
        install: impl FnOnce(&Self) -> io::Result<T>,
    ) -> io::Result<T> {
        let actual = install(&self)?;
        self.guards.validate_roots().map_err(invalid)?;
        for (arm, closed) in &self.arms {
            arm.install(*closed);
        }
        Ok(actual)
    }
}
