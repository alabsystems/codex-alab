// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual transcript chrome and the same native viewport/editor graphs. No draw,
//! search advance, input replay, default keymap or clipboard operation on decode.
use super::*;
use crate::bottom_pane::TextArea;
use crate::clipboard_copy::worker::alab_retained_clipboard::ScopeCapture;
use crate::clipboard_copy::worker::alab_retained_clipboard::ScopeRestored;
use crate::copy_input_guard::alab_retained_graph as guards;
use crate::history_cell::alab_retained_cells as cells;
use crate::keymap::alab_retained as keymaps;
use crate::terminal_hyperlinks::alab_retained_text as native;
use crate::transcript_view::alab_retained_view as views;
use serde::Deserialize;
use serde::Serialize;
use std::io;

fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<T, D::Error> {
    T::deserialize(d)
}
fn action(name: &str) -> io::Result<crate::keymap::KeymapActionId> {
    crate::keymap::keymap_action_id("global", name)
        .ok_or_else(|| native::invalid("original transcript overlay action is unavailable"))
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Motion {
    Animated {},
    Reduced {},
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data<E = TextArea> {
    schema: u8,
    view: views::complete::Data<E>,
    motion: Motion,
    cells: Vec<cells::CellRef>,
    keymap: keymaps::PagerKeymapData,
    #[serde(deserialize_with = "required")]
    highlight_cell: Option<usize>,
    #[serde(deserialize_with = "required")]
    pending_highlight: Option<usize>,
    content_area: [u16; 4],
    #[serde(deserialize_with = "required")]
    cursor: Option<(u16, u16)>,
    #[serde(deserialize_with = "required")]
    notice: Option<String>,
    #[serde(deserialize_with = "required")]
    browsing_footer: Option<native::LineData>,
    find_transcript: keymaps::BindingsData,
    select_transcript: keymaps::BindingsData,
    #[serde(deserialize_with = "required")]
    pending_follow_close: Option<(u64, bool)>,
    pending_select_mode: bool,
    is_done: bool,
}
impl TranscriptOverlay {
    pub(crate) fn alab_register_retained_roots(
        &self,
        graph: &mut views::CaptureGraph<'_>,
    ) -> io::Result<()> {
        views::complete::register_roots(&self.view, &self.cells, graph)
    }
    pub(crate) fn alab_capture_retained<'a>(
        &'a self,
        graph: &mut views::CaptureGraph<'_>,
        scopes: &mut ScopeCapture,
        guards: &mut guards::Capture,
        keymaps: &mut keymaps::Capture,
    ) -> io::Result<Data<&'a TextArea>> {
        let Self {
            view,
            motion,
            cells,
            keymap,
            highlight_cell,
            pending_highlight,
            content_area,
            cursor,
            notice,
            browsing_footer,
            find_transcript,
            select_transcript,
            pending_follow_close,
            pending_select_mode,
            is_done,
        } = self;
        self.alab_register_retained_roots(graph)?;
        Ok(Data {
            schema: 1,
            view: views::complete::Data::capture(view, graph, scopes, guards)?,
            motion: match motion {
                MotionMode::Animated => Motion::Animated {},
                MotionMode::Reduced => Motion::Reduced {},
            },
            cells: cells
                .iter()
                .map(|cell| graph.root(cell))
                .collect::<io::Result<_>>()?,
            keymap: keymaps::PagerKeymapData::capture(keymap, keymaps)?,
            highlight_cell: *highlight_cell,
            pending_highlight: *pending_highlight,
            content_area: [
                content_area.x,
                content_area.y,
                content_area.width,
                content_area.height,
            ],
            cursor: *cursor,
            notice: notice.clone(),
            browsing_footer: browsing_footer.as_ref().map(native::LineData::capture),
            find_transcript: keymaps::BindingsData::capture(
                find_transcript,
                action("find_transcript")?,
            )?,
            select_transcript: keymaps::BindingsData::capture(
                select_transcript,
                action("select_transcript")?,
            )?,
            pending_follow_close: *pending_follow_close,
            pending_select_mode: *pending_select_mode,
            is_done: *is_done,
        })
    }
}
struct Chrome {
    motion: MotionMode,
    cells: Vec<cells::CellRef>,
    keymap: PagerKeymap,
    highlight_cell: Option<usize>,
    pending_highlight: Option<usize>,
    content_area: Rect,
    cursor: Option<(u16, u16)>,
    notice: Option<String>,
    browsing_footer: Option<Line<'static>>,
    find_transcript: Vec<crate::key_hint::KeyBinding>,
    select_transcript: Vec<crate::key_hint::KeyBinding>,
    pending_follow_close: Option<(u64, bool)>,
    pending_select_mode: bool,
    is_done: bool,
}
pub(crate) struct PreparedLines {
    view: views::complete::PreparedText<TextArea>,
    chrome: Chrome,
}
pub(crate) struct OwnersRefused {
    pub(crate) prepared: PreparedLines,
    pub(crate) error: io::Error,
}
pub(crate) struct Prepared {
    view: views::complete::Prepared,
    chrome: Chrome,
    cells: Vec<Arc<dyn HistoryCell>>,
}
pub(crate) struct LinesRefused {
    pub(crate) data: Data<TextArea>,
    pub(crate) error: io::Error,
}
fn copy_data<T: Serialize + serde::de::DeserializeOwned>(data: &T) -> io::Result<T> {
    let bytes = serde_json::to_vec(data).map_err(native::invalid)?;
    serde_json::from_slice(&bytes).map_err(native::invalid)
}
impl Data<TextArea> {
    fn prepare_chrome(&self, keymaps: &keymaps::Restored) -> io::Result<Chrome> {
        if self.schema != 1 {
            return Err(native::invalid("transcript overlay schema"));
        }
        let keymap: keymaps::PagerKeymapData = copy_data(&self.keymap)?;
        let find: keymaps::BindingsData = copy_data(&self.find_transcript)?;
        let select: keymaps::BindingsData = copy_data(&self.select_transcript)?;
        Ok(Chrome {
            motion: match self.motion {
                Motion::Animated {} => MotionMode::Animated,
                Motion::Reduced {} => MotionMode::Reduced,
            },
            cells: self.cells.clone(),
            keymap: keymap.restore(keymaps)?,
            highlight_cell: self.highlight_cell,
            pending_highlight: self.pending_highlight,
            content_area: Rect::new(
                self.content_area[0],
                self.content_area[1],
                self.content_area[2],
                self.content_area[3],
            ),
            cursor: self.cursor,
            notice: self.notice.clone(),
            browsing_footer: self
                .browsing_footer
                .as_ref()
                .map(native::LineData::restore)
                .transpose()?,
            find_transcript: find.restore(keymaps, action("find_transcript")?)?,
            select_transcript: select.restore(keymaps, action("select_transcript")?)?,
            pending_follow_close: self.pending_follow_close,
            pending_select_mode: self.pending_select_mode,
            is_done: self.is_done,
        })
    }
    pub(crate) fn prepare_lines(
        self,
        layouts: &mut views::layouts::Restored,
        keymaps: &keymaps::Restored,
    ) -> std::result::Result<PreparedLines, Box<LinesRefused>> {
        // Only inert chrome DATA is copied. The actual native search editor is
        // borrowed until every fallible chrome field has prepared successfully.
        let chrome = match self.prepare_chrome(keymaps) {
            Ok(value) => value,
            Err(error) => return Err(Box::new(LinesRefused { data: self, error })),
        };
        let Self {
            schema,
            view,
            motion,
            cells,
            keymap,
            highlight_cell,
            pending_highlight,
            content_area,
            cursor,
            notice,
            browsing_footer,
            find_transcript,
            select_transcript,
            pending_follow_close,
            pending_select_mode,
            is_done,
        } = self;
        match view.prepare_lines(layouts) {
            Ok(view) => Ok(PreparedLines { view, chrome }),
            Err(refused) => {
                let views::complete::LinesRefused { data: view, error } = *refused;
                Err(Box::new(LinesRefused {
                    data: Self {
                        schema,
                        view,
                        motion,
                        cells,
                        keymap,
                        highlight_cell,
                        pending_highlight,
                        content_area,
                        cursor,
                        notice,
                        browsing_footer,
                        find_transcript,
                        select_transcript,
                        pending_follow_close,
                        pending_select_mode,
                        is_done,
                    },
                    error,
                }))
            }
        }
    }
}

impl PreparedLines {
    pub(crate) fn prepare_owners(
        self,
        graph: &views::RestoreGraph<'_>,
        scopes: &ScopeRestored,
    ) -> std::result::Result<Prepared, Box<OwnersRefused>> {
        let cells = match self
            .chrome
            .cells
            .iter()
            .map(|id| graph.cell(*id))
            .collect::<io::Result<_>>()
        {
            Ok(cells) => cells,
            Err(error) => {
                return Err(Box::new(OwnersRefused {
                    prepared: self,
                    error,
                }));
            }
        };
        match self.view.prepare_owners(graph, scopes) {
            Ok(view) => Ok(Prepared {
                view,
                chrome: self.chrome,
                cells,
            }),
            Err(refused) => {
                let views::complete::OwnerRefused {
                    prepared: view,
                    error,
                } = *refused;
                Err(Box::new(OwnersRefused {
                    prepared: Self {
                        view,
                        chrome: self.chrome,
                    },
                    error,
                }))
            }
        }
    }
}
// Bound viewport admission is supplied with the enclosing scene's exact actual
// Tui/renderer/clipboard owners. Keep this section staged until that admission.
pub(crate) struct Refused {
    pub(crate) prepared: Prepared,
    pub(crate) error: io::Error,
}
impl Prepared {
    pub(crate) fn admit(
        self,
        bindings: &views::complete::RestoreBindings<'_, '_>,
    ) -> std::result::Result<TranscriptOverlay, Box<Refused>> {
        let Self {
            view,
            chrome,
            cells,
        } = self;
        let view = match view.restore_bound(bindings) {
            Ok(view) => view,
            Err(refused) => {
                let views::complete::Refused {
                    prepared: view,
                    error,
                } = *refused;
                return Err(Box::new(Refused {
                    prepared: Self {
                        view,
                        chrome,
                        cells,
                    },
                    error,
                }));
            }
        };
        let Chrome {
            motion,
            cells: _,
            keymap,
            highlight_cell,
            pending_highlight,
            content_area,
            cursor,
            notice,
            browsing_footer,
            find_transcript,
            select_transcript,
            pending_follow_close,
            pending_select_mode,
            is_done,
        } = chrome;
        Ok(TranscriptOverlay {
            view: Box::new(view),
            motion,
            cells,
            keymap,
            highlight_cell,
            pending_highlight,
            content_area,
            cursor,
            notice,
            browsing_footer,
            find_transcript,
            select_transcript,
            pending_follow_close,
            pending_select_mode,
            is_done,
        })
    }
}

#[cfg(test)]
mod controls {
    use super::*;
    use crate::app_event_sender::AppEventSender;
    use crate::bottom_pane::alab_retained_editors as editors;
    use crate::history_cell::AgentMarkdownCell;
    const LIMIT: usize = 1_048_576;

    #[test]
    fn retained_native_overlay_pending_navigation_and_chrome_use_one_view_editor_graph()
    -> io::Result<()> {
        let keymap = RuntimeKeymap::defaults();
        let roots: Vec<Arc<dyn HistoryCell>> = vec![Arc::new(AgentMarkdownCell::new(
            "actual overlay source".into(),
            std::path::Path::new("."),
        ))];
        let mut actual = TranscriptOverlay::new(roots, keymap.pager.clone());
        actual.set_keymap_bindings(&keymap);
        actual.motion = MotionMode::Animated;
        let area = Rect::new(2, 4, 38, 14);
        actual.render(area, &mut Buffer::empty(area));
        actual.begin_search();
        actual.view.paste_search("original query");
        actual.notice = Some("existing notice".into());
        actual.browsing_footer = Some(Line::from("original footer").dim());
        actual.highlight_cell = Some(0);
        actual.pending_highlight = Some(0);
        actual.pending_follow_close = Some((91, true));
        actual.pending_select_mode = true;
        let (sender, _events, census) = guards::channel();
        let sender = AppEventSender::new(sender);
        let mut guards = guards::Capture::for_app(&census, &sender)?;
        let mut cells = cells::Capture::default();
        let mut layouts = views::layouts::Capture::default();
        let mut graph = views::CaptureGraph::new(&mut cells, &mut layouts);
        let mut scopes = ScopeCapture::default();
        let mut keymaps = keymaps::Capture::default();
        let a = actual.alab_capture_retained(&mut graph, &mut scopes, &mut guards, &mut keymaps)?;
        let b = actual.alab_capture_retained(&mut graph, &mut scopes, &mut guards, &mut keymaps)?;
        let bytes = editors::encode(&(a, b), LIMIT)?;
        let (a, b): (Data, Data) = editors::decode(&bytes, LIMIT, 128)?;
        assert_eq!(a.cells, b.cells);
        assert_eq!(a.highlight_cell, Some(0));
        assert_eq!(a.pending_highlight, Some(0));
        assert_eq!(a.pending_follow_close, Some((91, true)));
        assert!(a.pending_select_mode);
        assert_eq!(a.notice.as_deref(), Some("existing notice"));
        assert_eq!(
            a.content_area,
            [
                actual.content_area.x,
                actual.content_area.y,
                actual.content_area.width,
                actual.content_area.height
            ]
        );
        assert_eq!(
            a.browsing_footer.as_ref().unwrap().restore()?,
            actual.browsing_footer.clone().unwrap()
        );
        // Native TextArea serialization must remain inside the enclosing editor
        // graph even for a second capture of the same actual viewport.
        let encoded = editors::encode(&(a, b), LIMIT)?;
        let _: (Data, Data) = editors::decode(&encoded, LIMIT, 128)?;
        Ok(())
    }

    #[test]
    fn retained_overlay_decoder_requires_every_nullable_native_chrome_field() -> io::Result<()> {
        let keymap = RuntimeKeymap::defaults();
        let actual = TranscriptOverlay::new(Vec::new(), keymap.pager);
        let (sender, _events, census) = guards::channel();
        let sender = AppEventSender::new(sender);
        let mut guards = guards::Capture::for_app(&census, &sender)?;
        let mut cells = cells::Capture::default();
        let mut layouts = views::layouts::Capture::default();
        let mut graph = views::CaptureGraph::new(&mut cells, &mut layouts);
        let mut scopes = ScopeCapture::default();
        let mut keymaps = keymaps::Capture::default();
        let data =
            actual.alab_capture_retained(&mut graph, &mut scopes, &mut guards, &mut keymaps)?;
        let wire: serde_json::Value =
            serde_json::from_slice(&editors::encode(&data, LIMIT)?).map_err(native::invalid)?;
        for field in [
            "highlight_cell",
            "pending_highlight",
            "cursor",
            "notice",
            "browsing_footer",
            "pending_follow_close",
        ] {
            let mut invalid = wire.clone();
            invalid["body"].as_object_mut().unwrap().remove(field);
            assert!(
                editors::decode::<Data>(
                    &serde_json::to_vec(&invalid).map_err(native::invalid)?,
                    LIMIT,
                    128
                )
                .is_err(),
                "{field}"
            );
        }
        Ok(())
    }
}
