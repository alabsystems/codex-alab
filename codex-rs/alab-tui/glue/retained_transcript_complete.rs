// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Enclosing actual viewport, using one App graph and borrowed native editors.
//! Historical preparation remains private until actual clipboard/renderer binding.
use super::identity::KeyData;
use super::identity::PositionData;
use super::*;
use crate::bottom_pane::TextArea;
use crate::clipboard_copy::worker::alab_retained_clipboard::BoundClipboard;
use crate::clipboard_copy::worker::alab_retained_clipboard::ScopeCapture;
use crate::clipboard_copy::worker::alab_retained_clipboard::ScopeRestored;
use crate::copy_input_guard::alab_retained_graph as guards;
use crate::terminal_hyperlinks::alab_retained_text::HyperlinkData;
use crate::transcript_view::composer_gap::alab_retained_feedback as feedback;
use crate::transcript_view::copy_mode::alab_retained_copy as copies;
use crate::transcript_view::follow_control::alab_retained as follow;
use crate::transcript_view::layout::alab_retained_cache as cache;
use crate::transcript_view::search::alab_retained_search as search_data;
use crate::transcript_view::select_mode::alab_retained_select as selects;
use crate::transcript_view::selection::alab_retained_selection as selections;
use crate::transcript_view::text::alab_retained_layout::LayoutRef;
use crate::transcript_view::text::alab_retained_layout::Restored as RestoredLayouts;
use crate::transcript_view::turn_details::alab_retained_turns as turns;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use serde::Deserialize;
use serde::Serialize;
use std::io;
fn invalid(why: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.to_string())
}
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
struct RectData([u16; 4]);
impl RectData {
    fn capture(r: Rect) -> Self {
        Self([r.x, r.y, r.width, r.height])
    }
    fn restore(self) -> Rect {
        let [x, y, width, height] = self.0;
        Rect {
            x,
            y,
            width,
            height,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum ProtocolData {
    None {},
    Kitty {},
    KittyLocalFile {},
    Sixel {},
}
impl ProtocolData {
    fn capture(p: Option<crate::pets::ImageProtocol>) -> Self {
        match p {
            None => Self::None {},
            Some(crate::pets::ImageProtocol::Kitty) => Self::Kitty {},
            Some(crate::pets::ImageProtocol::KittyLocalFile) => Self::KittyLocalFile {},
            Some(crate::pets::ImageProtocol::Sixel) => Self::Sixel {},
        }
    }
    fn restore(self) -> Option<crate::pets::ImageProtocol> {
        match self {
            Self::None {} => None,
            Self::Kitty {} => Some(crate::pets::ImageProtocol::Kitty),
            Self::KittyLocalFile {} => Some(crate::pets::ImageProtocol::KittyLocalFile),
            Self::Sixel {} => Some(crate::pets::ImageProtocol::Sixel),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum ModeData {
    Rich {},
    Raw {},
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum HistoryData {
    Idle {},
    LoadingOlder {},
    LoadingBeginning {},
    Partial {},
    Failed {},
    Complete {},
}
impl HistoryData {
    fn capture(h: TranscriptHistoryState) -> Self {
        match h {
            TranscriptHistoryState::Idle => Self::Idle {},
            TranscriptHistoryState::LoadingOlder => Self::LoadingOlder {},
            TranscriptHistoryState::LoadingBeginning => Self::LoadingBeginning {},
            TranscriptHistoryState::Partial => Self::Partial {},
            TranscriptHistoryState::Failed => Self::Failed {},
            TranscriptHistoryState::Complete => Self::Complete {},
        }
    }
    fn restore(self) -> TranscriptHistoryState {
        match self {
            Self::Idle {} => TranscriptHistoryState::Idle,
            Self::LoadingOlder {} => TranscriptHistoryState::LoadingOlder,
            Self::LoadingBeginning {} => TranscriptHistoryState::LoadingBeginning,
            Self::Partial {} => TranscriptHistoryState::Partial,
            Self::Failed {} => TranscriptHistoryState::Failed,
            Self::Complete {} => TranscriptHistoryState::Complete,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VisibleData {
    index: u64,
    row: u64,
    layout: LayoutRef,
    key: KeyData,
    activity_ids: tables::ActivityRef,
}
impl VisibleData {
    fn capture(v: &VisibleRow, g: &mut tables::Capture<'_>) -> io::Result<Self> {
        Ok(Self {
            index: u64::try_from(v.index).map_err(invalid)?,
            row: u64::try_from(v.row).map_err(invalid)?,
            layout: g.layout(&v.layout)?,
            key: g.key(v.key)?,
            activity_ids: g.activity(&v.activity_ids)?,
        })
    }
    fn restore(self, g: &tables::Restore<'_>) -> io::Result<VisibleRow> {
        let layout = g.layouts.layout(self.layout)?;
        let row = usize::try_from(self.row).map_err(invalid)?;
        if row >= layout.row_count() {
            return Err(invalid("native visible row outside retained layout"));
        }
        Ok(VisibleRow {
            index: usize::try_from(self.index).map_err(invalid)?,
            row,
            layout,
            key: self.key.restore(g.cells)?,
            activity_ids: g.activity(self.activity_ids)?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HeaderData {
    viewport: RectData,
    key: KeyData,
    row: u64,
}
impl HeaderData {
    fn capture(h: prompt_header::SuppressedHeader, g: &tables::Capture<'_>) -> io::Result<Self> {
        Ok(Self {
            viewport: RectData::capture(h.viewport),
            key: g.key(h.key)?,
            row: u64::try_from(h.row).map_err(invalid)?,
        })
    }
    fn restore(self, g: &tables::Restore<'_>) -> io::Result<prompt_header::SuppressedHeader> {
        Ok(prompt_header::SuppressedHeader {
            viewport: self.viewport.restore(),
            key: self.key.restore(g.cells)?,
            row: usize::try_from(self.row).map_err(invalid)?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Core {
    schema: u8,
    image_protocol: ProtocolData,
    mouse_scroll_speed_bits: u64,
    pending_mouse_scroll_bits: u64,
    copy_on_select: bool,
    copy_on_select_clear_selection: bool,
    primary_selection: bool,
    position: PositionData,
    #[serde(deserialize_with = "decode_follow")]
    follow_control: follow::Data,
    #[serde(deserialize_with = "required")]
    copy_feedback: Option<feedback::Data>,
    #[serde(deserialize_with = "required")]
    turn_tip_key: Option<KeyData>,
    cache: cache::Data,
    #[serde(deserialize_with = "required")]
    live: Option<LayoutRef>,
    #[serde(deserialize_with = "required")]
    live_separated: Option<LayoutRef>,
    #[serde(deserialize_with = "required")]
    live_key: Option<search_data::LiveKeyData>,
    live_continuation: bool,
    area: RectData,
    #[serde(deserialize_with = "required")]
    prompt_header_target: Option<(RectData, KeyData)>,
    #[serde(deserialize_with = "required")]
    suppressed_prompt_header: Option<HeaderData>,
    visible: Vec<VisibleData>,
    #[serde(deserialize_with = "required")]
    selection: Option<selections::Data>,
    #[serde(deserialize_with = "required")]
    select_mode: Option<selects::Data>,
    #[serde(deserialize_with = "required")]
    copy_mode: Option<copies::Data>,
    #[serde(deserialize_with = "required")]
    held_reading: Option<snapshots::Data>,
    detailed: bool,
    mode: ModeData,
    history: HistoryData,
    #[serde(deserialize_with = "required")]
    highlight: Option<u64>,
    #[serde(deserialize_with = "required")]
    saved_position: Option<PositionData>,
    unseen_activity: bool,
    tail_visible: bool,
    #[serde(deserialize_with = "required")]
    last_tail: Option<KeyData>,
    #[serde(deserialize_with = "required")]
    last_click: Option<(Snapshot, u16, u16, u8)>,
    #[serde(deserialize_with = "decode_disclosure")]
    disclosure: disclosures::Data,
    #[serde(deserialize_with = "required")]
    turn_details: Option<turns::Data>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data<E = TextArea> {
    core: Core,
    search: search_data::Data<E>,
    #[serde(deserialize_with = "required")]
    composer_tip: Option<(RectData, HyperlinkData)>,
}
pub(crate) struct PreparedText<E> {
    core: Core,
    search: search_data::Data<E>,
    composer_tip: Option<(Rect, HyperlinkLine)>,
}
impl Data<TextArea> {
    /// All actual App cell roots register before capturing any native view.
    /// Held revisions register before cache weak sources and disclosure keys.
    pub(crate) fn capture<'a>(
        v: &'a TranscriptView,
        g: &mut tables::Capture<'_>,
        scopes: &mut ScopeCapture,
        guards: &mut guards::Capture,
    ) -> io::Result<Data<&'a TextArea>> {
        let selection = v
            .selection
            .as_ref()
            .map(|s| selections::Data::capture(s, g, scopes))
            .transpose()?;
        let held_reading = v
            .held_reading
            .as_ref()
            .map(|s| snapshots::Data::capture(s, g))
            .transpose()?;
        let search = search_data::Data::capture(&v.search, g)?;
        let core = Core {
            schema: 1,
            image_protocol: ProtocolData::capture(v.image_protocol),
            mouse_scroll_speed_bits: v.mouse_scroll_speed.to_bits(),
            pending_mouse_scroll_bits: v.pending_mouse_scroll.to_bits(),
            copy_on_select: v.copy_on_select,
            copy_on_select_clear_selection: v.copy_on_select_clear_selection,
            primary_selection: v.primary_selection,
            position: g.position(v.position)?,
            follow_control: follow::Data::capture(v),
            copy_feedback: v
                .copy_feedback
                .as_ref()
                .map(feedback::Data::capture)
                .transpose()?,
            turn_tip_key: v.turn_tip_key.map(|k| g.key(k)).transpose()?,
            cache: cache::Data::capture(&v.cache, g)?,
            live: v.live.as_ref().map(|l| g.layout(l)).transpose()?,
            live_separated: v.live_separated.as_ref().map(|l| g.layout(l)).transpose()?,
            live_key: v.live_key.map(search_data::LiveKeyData::capture),
            live_continuation: v.live_continuation,
            area: RectData::capture(v.area),
            prompt_header_target: v
                .prompt_header_target
                .map(|(r, k)| g.key(k).map(|k| (RectData::capture(r), k)))
                .transpose()?,
            suppressed_prompt_header: v
                .suppressed_prompt_header
                .map(|h| HeaderData::capture(h, g))
                .transpose()?,
            visible: v
                .visible
                .iter()
                .map(|r| VisibleData::capture(r, g))
                .collect::<io::Result<_>>()?,
            selection,
            select_mode: v
                .select_mode
                .as_ref()
                .map(|s| selects::Data::capture(s, g))
                .transpose()?,
            copy_mode: v
                .copy_mode
                .as_ref()
                .map(|m| copies::Data::capture(m, g, guards))
                .transpose()?,
            held_reading,
            detailed: v.detailed,
            mode: match v.mode {
                HistoryRenderMode::Rich => ModeData::Rich {},
                HistoryRenderMode::Raw => ModeData::Raw {},
            },
            history: HistoryData::capture(v.history),
            highlight: v
                .highlight
                .map(u64::try_from)
                .transpose()
                .map_err(invalid)?,
            saved_position: v.saved_position.map(|p| g.position(p)).transpose()?,
            unseen_activity: v.unseen_activity,
            tail_visible: v.tail_visible,
            last_tail: v.last_tail.map(|k| g.key(k)).transpose()?,
            last_click: v
                .last_click
                .map(|(i, x, y, n)| Snapshot::capture(i).map(|i| (i, x, y, n)))
                .transpose()
                .map_err(invalid)?,
            disclosure: disclosures::Data::capture(v, g.identity())?,
            turn_details: v
                .turn_details
                .as_ref()
                .map(|t| turns::Data::capture(t, g))
                .transpose()?,
        };
        Ok(Data {
            core,
            search,
            composer_tip: v
                .composer_tip
                .as_ref()
                .map(|(r, line)| g.line(line).map(|line| (RectData::capture(*r), line)))
                .transpose()?,
        })
    }
}
pub(crate) struct LinesRefused<E> {
    pub(crate) data: Data<E>,
    pub(crate) error: io::Error,
}
impl<E> Data<E> {
    pub(crate) fn prepare_lines(
        self,
        layouts: &mut RestoredLayouts,
    ) -> std::result::Result<PreparedText<E>, Box<LinesRefused<E>>> {
        let composer_tip = (|| {
            self.composer_tip
                .as_ref()
                .map(|(r, line)| {
                    layouts.lines().line(line).map(|line| {
                        let [x, y, width, height] = r.0;
                        (
                            Rect {
                                x,
                                y,
                                width,
                                height,
                            },
                            line,
                        )
                    })
                })
                .transpose()
        })();
        let composer_tip = match composer_tip {
            Ok(value) => value,
            Err(error) => return Err(Box::new(LinesRefused { data: self, error })),
        };
        Ok(PreparedText {
            core: self.core,
            search: self.search,
            composer_tip,
        })
    }
}

// Preparation alone exposes no native viewport or ACK grant. The enclosing
// App still owns installation, original first paint and all resource settlement.
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}

fn decode_follow<'de, D: serde::Deserializer<'de>>(d: D) -> Result<follow::Data, D::Error> {
    let raw = Box::<serde_json::value::RawValue>::deserialize(d)?;
    follow::Data::decode(&raw, usize::MAX).map_err(serde::de::Error::custom)
}
fn decode_disclosure<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<disclosures::Data, D::Error> {
    let raw = Box::<serde_json::value::RawValue>::deserialize(d)?;
    disclosures::Data::decode(&raw, usize::MAX).map_err(serde::de::Error::custom)
}
pub(crate) struct Prepared {
    body: PreparedText<TextArea>,
    selection: Option<selections::Prepared>,
}
pub(crate) struct OwnerRefused {
    pub(crate) prepared: PreparedText<TextArea>,
    pub(crate) error: io::Error,
}
impl PreparedText<TextArea> {
    pub(crate) fn prepare_owners(
        mut self,
        graph: &tables::Restore<'_>,
        scopes: &ScopeRestored,
    ) -> std::result::Result<Prepared, Box<OwnerRefused>> {
        let prepared = (|| {
            let bytes = serde_json::to_vec(&self.core.selection).map_err(invalid)?;
            let selection: Option<selections::Data> =
                serde_json::from_slice(&bytes).map_err(invalid)?;
            selection
                .map(|data| data.prepare(graph, scopes))
                .transpose()
        })();
        let selection = match prepared {
            Ok(selection) => selection,
            Err(error) => {
                return Err(Box::new(OwnerRefused {
                    prepared: self,
                    error,
                }));
            }
        };
        let _selection_data = self.core.selection.take();
        Ok(Prepared {
            body: self,
            selection,
        })
    }
}

/// Actual resources from ONE enclosing scene. None of these references can be
/// created by decoding viewport DATA or by copying an endpoint/selector.
pub(crate) struct RestoreBindings<'a, 'graph> {
    pub(crate) graph: &'a tables::Restore<'graph>,
    pub(crate) guards: &'a guards::Restored,
    pub(crate) keymap: &'a crate::keymap::RuntimeKeymap,
    pub(crate) captured_theme: &'a crate::render::highlight::alab_retained_image_theme::ThemeStamp,
    pub(crate) renderer: &'a crate::retained_image_renderer::Binding,
    pub(crate) tui: &'a crate::tui::Tui,
    pub(crate) clipboard: &'a BoundClipboard,
    pub(crate) cell_resources: &'a crate::history_cell::alab_retained_cells::RestoreContext,
}
pub(crate) struct Refused {
    pub(crate) prepared: Prepared,
    pub(crate) error: io::Error,
}
struct BoundFields {
    search: search_data::PreparedFields,
    finish:
        Box<dyn FnOnce(Search, Option<Selection>, Option<(Rect, HyperlinkLine)>) -> TranscriptView>,
}
impl Prepared {
    /// Entire fallible preflight borrows actual editor/Selection owners. Its
    /// private temporary DTO copy contains no editor or independently encoded
    /// graph, and no native operation/render/copy runs on failure.
    fn preflight(&self, bindings: &RestoreBindings<'_, '_>) -> io::Result<BoundFields> {
        let g = bindings.graph;
        let guards = bindings.guards;
        let renderer = bindings.renderer;
        let tui = bindings.tui;
        let clipboard = bindings.clipboard;
        let resources = bindings.cell_resources;
        resources.require_image_renderer(renderer)?;
        clipboard.validate_tui(tui)?;
        if !renderer.belongs_to(tui) {
            return Err(invalid("native viewport renderer changed actual Tui"));
        }
        if let Some(selection) = &self.selection {
            selection.validate(clipboard)?;
        }
        let bytes = serde_json::to_vec(&self.body.core).map_err(invalid)?;
        let c: Core = serde_json::from_slice(&bytes).map_err(invalid)?;
        if c.schema != 1 || c.selection.is_some() {
            return Err(invalid("native whole viewport prepared schema/selection"));
        }
        if let Some(id) = c
            .copy_feedback
            .as_ref()
            .and_then(feedback::Data::pending_id)
        {
            clipboard.validate_selection(Some(id), None)?;
        }
        let search = self.body.search.prepare_fields(g)?;
        let cache = c
            .cache
            .prepare_bound(g, bindings.captured_theme)?
            .install(renderer, tui)?;
        let image_protocol = c.image_protocol.restore();
        let mouse_scroll_speed = f64::from_bits(c.mouse_scroll_speed_bits);
        let pending_mouse_scroll = f64::from_bits(c.pending_mouse_scroll_bits);
        let copy_on_select = c.copy_on_select;
        let copy_on_select_clear_selection = c.copy_on_select_clear_selection;
        let primary_selection = c.primary_selection;
        let position = c.position.restore(g.cells)?;
        let follow_control = c.follow_control.into_native();
        let copy_feedback = c.copy_feedback.map(feedback::Data::restore).transpose()?;
        let turn_tip_key = c.turn_tip_key.map(|k| k.restore(g.cells)).transpose()?;
        let live = c.live.map(|r| g.layouts.layout(r)).transpose()?;
        let live_separated = c.live_separated.map(|r| g.layouts.layout(r)).transpose()?;
        let live_key = c.live_key.map(search_data::LiveKeyData::restore);
        let live_continuation = c.live_continuation;
        let area = c.area.restore();
        let prompt_header_target = c
            .prompt_header_target
            .map(|(r, k)| k.restore(g.cells).map(|k| (r.restore(), k)))
            .transpose()?;
        let suppressed_prompt_header = c
            .suppressed_prompt_header
            .map(|h| h.restore(g))
            .transpose()?;
        let visible = c
            .visible
            .into_iter()
            .map(|v| v.restore(g))
            .collect::<io::Result<_>>()?;
        let select_mode = c.select_mode.map(|m| m.restore(g)).transpose()?;
        let copy_mode = c.copy_mode.map(|m| m.restore(g, guards)).transpose()?;
        let held_reading = c.held_reading.map(|s| s.restore(g)).transpose()?;
        let detailed = c.detailed;
        let mode = match c.mode {
            ModeData::Rich {} => HistoryRenderMode::Rich,
            ModeData::Raw {} => HistoryRenderMode::Raw,
        };
        let history = c.history.restore();
        let highlight = c
            .highlight
            .map(usize::try_from)
            .transpose()
            .map_err(invalid)?;
        let saved_position = c.saved_position.map(|p| p.restore(g.cells)).transpose()?;
        let unseen_activity = c.unseen_activity;
        let tail_visible = c.tail_visible;
        let last_tail = c.last_tail.map(|k| k.restore(g.cells)).transpose()?;
        let last_click = c
            .last_click
            .map(|(i, x, y, n)| i.restore().map(|i| (i, x, y, n)))
            .transpose()
            .map_err(invalid)?;
        let disclosure = c.disclosure.prepare_native(g.cells, bindings.keymap)?;
        let turn_details = c
            .turn_details
            .map(|t| t.restore(g, resources))
            .transpose()?;
        Ok(BoundFields {
            search,
            finish: Box::new(move |search, selection, composer_tip| TranscriptView {
                image_protocol,
                mouse_scroll_speed,
                pending_mouse_scroll,
                copy_on_select,
                copy_on_select_clear_selection,
                primary_selection,
                position,
                follow_control,
                copy_feedback,
                composer_tip,
                turn_tip_key,
                cache,
                live,
                live_separated,
                live_key,
                live_continuation,
                area,
                prompt_header_target,
                suppressed_prompt_header,
                visible,
                selection,
                select_mode,
                copy_mode,
                held_reading,
                search,
                detailed,
                mode,
                history,
                highlight,
                saved_position,
                unseen_activity,
                tail_visible,
                last_tail,
                last_click,
                disclosure,
                turn_details,
            }),
        })
    }
    /// Every refusal returns the SAME prepared native editor/Selection owner.
    /// No fallible operation remains after those owners are moved.
    pub(crate) fn restore_bound(
        self,
        bindings: &RestoreBindings<'_, '_>,
    ) -> std::result::Result<TranscriptView, Box<Refused>> {
        let fields = match self.preflight(bindings) {
            Ok(fields) => fields,
            Err(error) => {
                return Err(Box::new(Refused {
                    prepared: self,
                    error,
                }));
            }
        };
        let Self { body, selection } = self;
        let search = body.search.finish_fields(fields.search, bindings.keymap);
        let selection = selection.map(selections::Prepared::into_native);
        Ok((fields.finish)(search, selection, body.composer_tip))
    }
}

/// Call for EVERY current/parked viewport before capturing any weak leaf.
pub(crate) fn register_roots(
    actual: &TranscriptView,
    current: &[Arc<dyn HistoryCell>],
    graph: &mut tables::Capture<'_>,
) -> io::Result<()> {
    graph.register(current)?;
    if let Some(selection) = &actual.selection {
        graph.register(&selection.snapshot.cells)?;
    }
    if let Some(snapshot) = &actual.held_reading {
        graph.register(&snapshot.cells)?;
    }
    search_data::register_roots(&actual.search, graph)
}

#[cfg(test)]
#[path = "retained_transcript_complete_tests.rs"]
mod tests;

/// Recheck an already staged native owner on retry/final scene validation.
/// No graph, editor, selection or cache is rebuilt from this observation.
pub(crate) fn validate_bound(
    view: &TranscriptView,
    resources: &crate::history_cell::alab_retained_cells::RestoreContext,
    tui: &crate::tui::Tui,
    renderer: &crate::retained_image_renderer::Binding,
    clipboard: &BoundClipboard,
) -> io::Result<()> {
    resources.require_image_renderer(renderer)?;
    clipboard.validate_tui(tui)?;
    if !renderer.belongs_to(tui) {
        return Err(invalid("staged viewport renderer changed actual Tui"));
    }
    if let Some(selection) = &view.selection {
        selections::validate_native(selection, clipboard)?;
    }
    if let Some(id) = view
        .copy_feedback
        .as_ref()
        .and_then(|feedback| match feedback.result {
            Ok(crate::clipboard_copy::CopyStatus::Pending(id)) => Some(id),
            _ => None,
        })
    {
        clipboard.validate_selection(Some(id), None)?;
    }
    Ok(())
}
