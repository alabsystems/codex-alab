// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original navigation state; no revert, selection or copy operation runs on decode.
use super::*;
use crate::key_hint::alab_retained_keys::Key as KeyData;
use crate::streaming::alab_retained::required;
use crate::transcript_view::alab_retained_view::BookmarkData;
use crate::transcript_view::alab_retained_view::CaptureGraph;
use crate::transcript_view::alab_retained_view::RestoreGraph;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Origin {
    Owned { bookmark: BookmarkData },
    Overlay { bookmark: BookmarkData },
    Inline {},
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    primed: bool,
    #[serde(deserialize_with = "required")]
    base_id: Option<ThreadId>,
    nth_user_message: usize,
    overlay_preview_active: bool,
    #[serde(deserialize_with = "required")]
    select_exit_key: Option<KeyData>,
    #[serde(deserialize_with = "required")]
    pending_select_exit_key: Option<(u64, KeyData)>,
    #[serde(deserialize_with = "required")]
    origin: Option<Origin>,
}
impl Data {
    pub(crate) fn register_roots(
        actual: &BacktrackState,
        graph: &mut CaptureGraph<'_>,
    ) -> io::Result<()> {
        match &actual.origin {
            Some(
                browsing::BrowsingOrigin::Owned(bookmark)
                | browsing::BrowsingOrigin::Overlay(bookmark),
            ) => BookmarkData::register_roots(bookmark, graph),
            _ => Ok(()),
        }
    }
    pub(crate) fn capture(
        actual: &BacktrackState,
        graph: &mut CaptureGraph<'_>,
    ) -> io::Result<Self> {
        let BacktrackState {
            primed,
            base_id,
            nth_user_message,
            overlay_preview_active,
            select_exit_key,
            pending_select_exit_key,
            origin,
        } = actual;
        Ok(Self {
            primed: *primed,
            base_id: *base_id,
            nth_user_message: *nth_user_message,
            overlay_preview_active: *overlay_preview_active,
            select_exit_key: select_exit_key.map(KeyData::capture),
            pending_select_exit_key: pending_select_exit_key
                .map(|(id, key)| (id, KeyData::capture(key))),
            origin: origin
                .as_ref()
                .map(|origin| -> io::Result<Origin> {
                    Ok(match origin {
                        browsing::BrowsingOrigin::Owned(bookmark) => Origin::Owned {
                            bookmark: BookmarkData::capture(bookmark, graph)?,
                        },
                        browsing::BrowsingOrigin::Overlay(bookmark) => Origin::Overlay {
                            bookmark: BookmarkData::capture(bookmark, graph)?,
                        },
                        browsing::BrowsingOrigin::Inline => Origin::Inline {},
                    })
                })
                .transpose()?,
        })
    }
    pub(crate) fn restore(self, graph: &RestoreGraph<'_>) -> io::Result<BacktrackState> {
        Ok(BacktrackState {
            primed: self.primed,
            base_id: self.base_id,
            nth_user_message: self.nth_user_message,
            overlay_preview_active: self.overlay_preview_active,
            select_exit_key: self.select_exit_key.map(KeyData::restore).transpose()?,
            pending_select_exit_key: self
                .pending_select_exit_key
                .map(|(id, key)| key.restore().map(|key| (id, key)))
                .transpose()?,
            origin: self
                .origin
                .map(|origin| -> io::Result<browsing::BrowsingOrigin> {
                    Ok(match origin {
                        Origin::Owned { bookmark } => {
                            browsing::BrowsingOrigin::Owned(bookmark.restore(graph)?)
                        }
                        Origin::Overlay { bookmark } => {
                            browsing::BrowsingOrigin::Overlay(bookmark.restore(graph)?)
                        }
                        Origin::Inline {} => browsing::BrowsingOrigin::Inline,
                    })
                })
                .transpose()?,
        })
    }
}

#[cfg(test)]
mod controls {
    use super::*;
    use crate::history_cell::alab_retained_cells as cells;
    use crate::transcript_view::alab_retained_view as views;
    #[test]
    fn retained_backtrack_origin_registers_held_roots_before_any_current_view_and_keeps_native_keys()
    -> io::Result<()> {
        let roots: Vec<Arc<dyn crate::history_cell::HistoryCell>> =
            vec![Arc::new(crate::history_cell::PlainHistoryCell::new(vec![
                ratatui::text::Line::from("original browsing source"),
                ratatui::text::Line::from("original second row"),
            ]))];
        let mut view = crate::transcript_view::TranscriptView::default();
        let area = ratatui::layout::Rect::new(0, 0, 20, 1);
        view.render(area, &mut ratatui::buffer::Buffer::empty(area), &roots);
        view.jump_to_entry(&roots, 0);
        let actual = BacktrackState {
            primed: true,
            base_id: Some(ThreadId::new()),
            nth_user_message: 4,
            overlay_preview_active: true,
            select_exit_key: Some(crate::key_hint::ctrl(KeyCode::Char(' '))),
            pending_select_exit_key: Some((29, crate::key_hint::plain(KeyCode::Enter))),
            origin: Some(browsing::BrowsingOrigin::Owned(view.bookmark(&roots))),
        };
        let mut cells = cells::Capture::default();
        let mut layouts = views::layouts::Capture::default();
        let mut graph = CaptureGraph::new(&mut cells, &mut layouts);
        Data::register_roots(&actual, &mut graph)?;
        let data = Data::capture(&actual, &mut graph)?;
        let expected = serde_json::to_value(&data).map_err(io::Error::other)?;
        let table = graph.export();
        drop(graph);
        let bytes = cells.encode(layouts.lines(), 1_048_576)?;
        let raw = layouts.encode(1_048_576)?;
        let mut layouts = views::layouts::Restored::decode(&raw, 1_048_576)?;
        let cells = cells::Restored::decode(
            &bytes,
            1_048_576,
            layouts.lines(),
            &mut cells::RestoreContext::default(),
        )?;
        let graph = RestoreGraph::new(table, &cells, &layouts)?;
        let restored = data.restore(&graph)?;
        assert!(restored.primed);
        assert_eq!(restored.base_id, actual.base_id);
        assert_eq!(restored.nth_user_message, 4);
        assert_eq!(restored.select_exit_key, actual.select_exit_key);
        assert_eq!(
            restored.pending_select_exit_key,
            actual.pending_select_exit_key
        );
        let mut cells = cells::Capture::default();
        let mut layouts = views::layouts::Capture::default();
        let mut graph = CaptureGraph::new(&mut cells, &mut layouts);
        Data::register_roots(&restored, &mut graph)?;
        let result = Data::capture(&restored, &mut graph)?;
        assert_eq!(
            serde_json::to_value(result).map_err(io::Error::other)?,
            expected
        );
        Ok(())
    }
}
