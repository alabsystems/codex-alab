// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! All native TranscriptState fields; shared resources belong to the enclosing scene.
//! No editor, scheduler, request, process, renderer or clipboard owner is constructed here.
use super::*;
use crate::chatwidget::ChatWidget;
use crate::clipboard_copy::worker::alab_retained_clipboard::BoundClipboard;
use crate::history_cell::alab_retained_cells as cells;
use crate::render::highlight::alab_retained_image_theme as theme;
use crate::terminal_hyperlinks::alab_retained_text as native;
use codex_alab_tui::retained_arc_graph::Id;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Mode {
    Rich {},
    Raw {},
}
impl Mode {
    fn capture(actual: HistoryRenderMode) -> Self {
        match actual {
            HistoryRenderMode::Rich => Self::Rich {},
            HistoryRenderMode::Raw => Self::Raw {},
        }
    }
    fn restore(&self) -> HistoryRenderMode {
        match self {
            Self::Rich {} => HistoryRenderMode::Rich,
            Self::Raw {} => HistoryRenderMode::Raw,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum CacheOwner {
    Active {},
    Invalidated {},
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Layout {
    owner: CacheOwner,
    revision: u64,
    width: u16,
    mode: Mode,
    theme_revision: u64,
    #[serde(deserialize_with = "required")]
    desired_height: Option<u16>,
    #[serde(deserialize_with = "required")]
    rendered_height: Option<usize>,
}
impl Layout {
    fn capture(actual: ActiveCellLayoutCache, active: Option<&dyn HistoryCell>) -> Self {
        let owner = if active.is_some_and(|cell| {
            actual.key.cell_identity == cell as *const dyn HistoryCell as *const () as usize
        }) {
            CacheOwner::Active {}
        } else {
            // Original lookup would reject the different Box identity. Keep historical
            // dimensions in DATA, but never write a process address or alias a new Box.
            CacheOwner::Invalidated {}
        };
        Self {
            owner,
            revision: actual.key.revision,
            width: actual.key.width,
            mode: Mode::capture(actual.key.render_mode),
            theme_revision: actual.key.syntax_theme_revision,
            desired_height: actual.desired_height,
            rendered_height: actual.rendered_height,
        }
    }
    fn restore(
        &self,
        active: Option<&dyn HistoryCell>,
        revision: Option<u64>,
    ) -> io::Result<Option<ActiveCellLayoutCache>> {
        let identity = match self.owner {
            CacheOwner::Invalidated {} => return Ok(None),
            CacheOwner::Active {} => active
                .map(|cell| cell as *const dyn HistoryCell as *const () as usize)
                .ok_or_else(|| native::invalid("active transcript cache has no actual Box"))?,
        };
        // A genuinely different renderer epoch follows the original cache miss path.
        let Some(theme_revision) = revision else {
            return Ok(None);
        };
        Ok(Some(ActiveCellLayoutCache {
            key: ActiveCellLayoutCacheKey {
                cell_identity: identity,
                revision: self.revision,
                width: self.width,
                render_mode: self.mode.restore(),
                syntax_theme_revision: theme_revision,
            },
            desired_height: self.desired_height,
            rendered_height: self.rendered_height,
        }))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StatusCopy {
    handle: cells::StatusRef,
    fields: Vec<(String, Id)>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Data {
    schema: u8,
    #[serde(deserialize_with = "required")]
    active_cell: Option<cells::UniqueBody>,
    dynamic_calls: Vec<(String, cells::DynamicData)>,
    active_cell_revision: u64,
    #[serde(deserialize_with = "required")]
    active_cell_layout: Option<Layout>,
    #[serde(deserialize_with = "required")]
    last_agent_markdown: Option<String>,
    #[serde(deserialize_with = "required")]
    last_agent_source: Option<String>,
    #[serde(deserialize_with = "required")]
    last_status_copy_targets: Option<StatusCopy>,
    #[serde(deserialize_with = "required")]
    last_completed_agent_message: Option<(String, String)>,
    #[serde(deserialize_with = "required")]
    replayed_cloud_text: Option<Vec<(String, Vec<String>)>>,
    recording_finished_cloud_text: bool,
    #[serde(deserialize_with = "required")]
    active_streamed_agent_message: Option<(String, String)>,
    #[serde(deserialize_with = "required")]
    last_flushed_agent_message: Option<(String, String)>,
    #[serde(deserialize_with = "required")]
    latest_proposed_plan_markdown: Option<String>,
    saw_copy_source_this_turn: bool,
    saw_plan_update_this_turn: bool,
    saw_plan_item_this_turn: bool,
    #[serde(deserialize_with = "required")]
    last_plan_progress: Option<(usize, usize)>,
    plan_delta_buffer: String,
    plan_item_active: bool,
}
impl Data {
    /// Call before final cells.encode/text.export, with the SAME scene captures.
    pub(crate) fn capture(
        actual: &ChatWidget,
        graph: &mut cells::Capture,
        text: &mut native::Capture,
    ) -> io::Result<Self> {
        Self::capture_state(&actual.transcript, graph, text)
    }
    fn capture_state(
        actual: &TranscriptState,
        graph: &mut cells::Capture,
        text: &mut native::Capture,
    ) -> io::Result<Self> {
        let mut dynamic_calls = actual.dynamic_calls.iter().collect::<Vec<_>>();
        dynamic_calls.sort_unstable_by(|a, b| a.0.cmp(b.0));
        let dynamic_calls = dynamic_calls
            .into_iter()
            .map(|(id, cell)| {
                if id != cell.call_id() {
                    return Err(native::invalid(
                        "original dynamic registry changed its call identity",
                    ));
                }
                Ok((id.clone(), graph.dynamic_cell(cell)?))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let replayed_cloud_text = actual.replayed_cloud_text.as_ref().map(|turns| {
            let mut turns = turns
                .iter()
                .map(|(turn, items)| {
                    let mut items = items.iter().cloned().collect::<Vec<_>>();
                    items.sort_unstable();
                    (turn.clone(), items)
                })
                .collect::<Vec<_>>();
            turns.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            turns
        });
        let data = Self {
            schema: 1,
            active_cell: actual
                .active_cell
                .as_deref()
                .map(|cell| graph.unique(cell, text))
                .transpose()?,
            dynamic_calls,
            active_cell_revision: actual.active_cell_revision,
            active_cell_layout: actual
                .active_cell_layout
                .get()
                .map(|layout| Layout::capture(layout, actual.active_cell.as_deref())),
            last_agent_markdown: actual.last_agent_markdown.clone(),
            last_agent_source: actual.last_agent_source.clone(),
            last_status_copy_targets: actual
                .last_status_copy_targets
                .as_ref()
                .map(|status| -> io::Result<_> {
                    Ok(StatusCopy {
                        handle: graph.status_handle(&status.handle)?,
                        fields: status
                            .fields
                            .iter()
                            .map(|(name, value)| Ok((name.clone(), text.string(value)?)))
                            .collect::<io::Result<Vec<_>>>()?,
                    })
                })
                .transpose()?,
            last_completed_agent_message: actual.last_completed_agent_message.clone(),
            replayed_cloud_text,
            recording_finished_cloud_text: actual.recording_finished_cloud_text,
            active_streamed_agent_message: actual.active_streamed_agent_message.clone(),
            last_flushed_agent_message: actual.last_flushed_agent_message.clone(),
            latest_proposed_plan_markdown: actual.latest_proposed_plan_markdown.clone(),
            saw_copy_source_this_turn: actual.saw_copy_source_this_turn,
            saw_plan_update_this_turn: actual.saw_plan_update_this_turn,
            saw_plan_item_this_turn: actual.saw_plan_item_this_turn,
            last_plan_progress: actual.last_plan_progress,
            plan_delta_buffer: actual.plan_delta_buffer.clone(),
            plan_item_active: actual.plan_item_active,
        };
        data.validate()?;
        Ok(data)
    }
    pub(crate) fn decode(raw: &serde_json::value::RawValue, max_bytes: usize) -> io::Result<Self> {
        if max_bytes == 0 || raw.get().len() > max_bytes {
            return Err(native::invalid("native widget transcript byte budget"));
        }
        let data: Self = serde_json::from_str(raw.get()).map_err(native::invalid)?;
        if serde_json::from_str::<serde_json::Value>(raw.get()).map_err(native::invalid)?
            != serde_json::to_value(&data).map_err(native::invalid)?
        {
            return Err(native::invalid(
                "native widget transcript omits original fields",
            ));
        }
        data.validate()?;
        Ok(data)
    }
    fn validate(&self) -> io::Result<()> {
        if self.schema != 1
            || self.dynamic_calls.len() > native::MAX_NODES
            || self
                .last_status_copy_targets
                .as_ref()
                .is_some_and(|value| value.fields.len() > native::MAX_NODES)
            || self.active_cell_layout.as_ref().is_some_and(|value| {
                matches!(value.owner, CacheOwner::Active {}) && self.active_cell.is_none()
            })
        {
            return Err(native::invalid(
                "native widget transcript schema/capacity/active cache",
            ));
        }
        let mut ids = HashSet::new();
        if self.dynamic_calls.iter().any(|(id, _)| !ids.insert(id)) {
            return Err(native::invalid("duplicate native dynamic registry entry"));
        }
        if let Some(turns) = &self.replayed_cloud_text {
            let mut ids = HashSet::new();
            let mut total = 0_usize;
            for (turn, items) in turns {
                total = total
                    .checked_add(items.len())
                    .ok_or_else(|| native::invalid("Cloud replay item overflow"))?;
                let mut item_ids = HashSet::new();
                if !ids.insert(turn) || items.iter().any(|item| !item_ids.insert(item)) {
                    return Err(native::invalid("duplicate native Cloud replay occurrence"));
                }
            }
            if turns.len() > native::MAX_NODES || total > native::MAX_NODES {
                return Err(native::invalid("native Cloud replay capacity"));
            }
        }
        Ok(())
    }
    /// No native transcript is exposed before the real renderer/worker admission.
    /// SAME context must already have decoded graph and all genuine frame/inline bindings.
    pub(crate) fn prepare(
        self,
        graph: &cells::Restored,
        text: &mut native::Restored,
        resources: &mut cells::RestoreContext,
        captured_theme: &theme::ThemeStamp,
    ) -> io::Result<Prepared> {
        self.validate()?;
        graph.require_context(resources)?;
        captured_theme.validate_data()?;
        let active_cell = self
            .active_cell
            .map(|body| graph.restore_unique(body, text, resources))
            .transpose()?;
        let mut dynamic_calls = HashMap::new();
        for (id, data) in self.dynamic_calls {
            let cell = resources.dynamic_cell(&data)?;
            if id != cell.call_id() || dynamic_calls.insert(id, cell).is_some() {
                return Err(native::invalid(
                    "restored dynamic registry changed its original binding",
                ));
            }
        }
        let replayed_cloud_text = self.replayed_cloud_text.map(|turns| {
            turns
                .into_iter()
                .map(|(turn, items)| (turn, items.into_iter().collect()))
                .collect()
        });
        Ok(Prepared {
            native: TranscriptState {
                active_cell,
                dynamic_calls,
                active_cell_revision: self.active_cell_revision,
                active_cell_layout: Cell::new(None),
                last_agent_markdown: self.last_agent_markdown,
                last_agent_source: self.last_agent_source,
                last_status_copy_targets: self
                    .last_status_copy_targets
                    .map(|status| -> io::Result<_> {
                        Ok(StatusCopySource {
                            handle: resources.status_handle(status.handle)?,
                            fields: status
                                .fields
                                .into_iter()
                                .map(|(name, value)| Ok((name, text.string(value)?)))
                                .collect::<io::Result<Vec<_>>>()?,
                        })
                    })
                    .transpose()?,
                last_completed_agent_message: self.last_completed_agent_message,
                replayed_cloud_text,
                recording_finished_cloud_text: self.recording_finished_cloud_text,
                active_streamed_agent_message: self.active_streamed_agent_message,
                last_flushed_agent_message: self.last_flushed_agent_message,
                latest_proposed_plan_markdown: self.latest_proposed_plan_markdown,
                saw_copy_source_this_turn: self.saw_copy_source_this_turn,
                saw_plan_update_this_turn: self.saw_plan_update_this_turn,
                saw_plan_item_this_turn: self.saw_plan_item_this_turn,
                last_plan_progress: self.last_plan_progress,
                plan_delta_buffer: self.plan_delta_buffer,
                plan_item_active: self.plan_item_active,
            },
            layout: self.active_cell_layout,
            theme: captured_theme.clone(),
            context: graph.context_identity(),
        })
    }
}
/// Native fields remain private until binding is checked inside the enclosing install.
pub(crate) struct Prepared {
    native: TranscriptState,
    layout: Option<Layout>,
    theme: theme::ThemeStamp,
    context: cells::ContextIdentity,
}
pub(crate) struct Refused {
    pub(crate) original: Prepared,
    pub(crate) error: io::Error,
}
/// The caller retains the exact displaced native owner until full scene installation settles.
pub(crate) struct Displaced {
    _native: TranscriptState,
}
pub(crate) struct Admitted(Prepared);
impl Prepared {
    fn refresh_binding(
        &self,
        tui: &crate::tui::Tui,
        renderer: &crate::retained_image_renderer::Binding,
        clipboard: &BoundClipboard,
        resources: &cells::RestoreContext,
    ) -> io::Result<()> {
        let generation = (|| {
            self.context.require(resources)?;
            renderer.check()?;
            if !renderer.belongs_to(tui) {
                return Err(native::invalid(
                    "widget transcript changed actual renderer Tui",
                ));
            }
            resources.require_image_renderer(renderer)?;
            clipboard.validate_tui(tui)?;
            if let Some(layout) = &self.layout {
                self.theme
                    .cache_generation(layout.theme_revision, renderer)
                    .map(Some)
            } else {
                Ok(None)
            }
        })()?;
        let revision = match generation {
            Some(theme::CacheGeneration::Equivalent(revision)) => Some(revision),
            _ => None,
        };
        self.refresh_layout(revision)
    }
    /// Native cache DATA only. Actual admitted currentness checks run first in
    /// refresh_binding; this helper creates no renderer/clipboard/Native proof.
    fn refresh_layout(&self, revision: Option<u64>) -> io::Result<()> {
        if let Some(layout) = &self.layout {
            // All fallible ownership admission ran before the first native widget move.
            // validate() already proved Active => actual active Box; no resource query here.
            let restored = layout.restore(self.native.active_cell.as_deref(), revision)?;
            self.native.active_cell_layout.set(restored);
        }
        Ok(())
    }
    pub(crate) fn admit(
        self,
        tui: &crate::tui::Tui,
        renderer: &crate::retained_image_renderer::Binding,
        clipboard: &BoundClipboard,
        resources: &cells::RestoreContext,
    ) -> Result<Admitted, Box<Refused>> {
        if let Err(error) = self.refresh_binding(tui, renderer, clipboard, resources) {
            return Err(Box::new(Refused {
                original: self,
                error,
            }));
        }
        Ok(Admitted(self))
    }
    pub(crate) fn install(
        self,
        widget: &mut ChatWidget,
        tui: &crate::tui::Tui,
        renderer: &crate::retained_image_renderer::Binding,
        clipboard: &BoundClipboard,
        resources: &cells::RestoreContext,
    ) -> Result<Displaced, Box<Refused>> {
        Ok(self
            .admit(tui, renderer, clipboard, resources)?
            .install(widget))
    }
}
impl Admitted {
    pub(crate) fn validate(
        &self,
        tui: &crate::tui::Tui,
        renderer: &crate::retained_image_renderer::Binding,
        clipboard: &BoundClipboard,
        resources: &cells::RestoreContext,
    ) -> io::Result<()> {
        self.0.refresh_binding(tui, renderer, clipboard, resources)
    }
    /// Apply inside the enclosing synchronous graph operation after every
    /// component has completed its fallible local admission.
    pub(crate) fn install(self, widget: &mut ChatWidget) -> Displaced {
        Displaced {
            _native: std::mem::replace(&mut widget.transcript, self.0.native),
        }
    }
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(decoder)
}

#[cfg(test)]
#[path = "retained_widget_transcript_tests.rs"]
mod tests;
