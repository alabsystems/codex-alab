// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original browsing/bookmark actions and graph identity; decode never calls the renderer.
use super::*;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::UserHistoryCell;
use crate::history_cell::UserMessagePresentation;
use crate::history_cell::alab_retained_cells as cells;
use crate::transcript_view::text::alab_retained_layout as layouts;
use serde_json::json;
const LIMIT: usize = 1_048_576;
fn cell(text: &str) -> Arc<dyn HistoryCell> {
    Arc::new(PlainHistoryCell::new(
        text.lines()
            .map(|line| Line::from(line.to_owned()))
            .collect(),
    ))
}
fn user(text: &str) -> Arc<dyn HistoryCell> {
    Arc::new(UserHistoryCell {
        message: text.into(),
        text_elements: vec![],
        local_image_paths: vec![],
        remote_image_urls: vec![],
        presentation: UserMessagePresentation::Typed,
    })
}
fn render(view: &mut TranscriptView, cells: &[Arc<dyn HistoryCell>], width: u16, height: u16) {
    let area = Rect::new(0, 0, width, height);
    view.render(area, &mut Buffer::empty(area), cells);
}
struct Decoded {
    cells: cells::Restored,
    layouts: layouts::Restored,
    table: tables::Data,
}
impl Decoded {
    fn graph(&self) -> io::Result<tables::Restore<'_>> {
        tables::Restore::new(self.table.clone(), &self.cells, &self.layouts)
    }
}
fn decode(
    mut cells: cells::Capture,
    mut layouts: layouts::Capture,
    table: tables::Data,
) -> io::Result<Decoded> {
    let raw = cells.encode(layouts.lines(), LIMIT)?;
    let layout_raw = layouts.encode(LIMIT)?;
    let mut layouts = layouts::Restored::decode(&layout_raw, LIMIT)?;
    let cells = cells::Restored::decode(
        &raw,
        LIMIT,
        layouts.lines(),
        &mut cells::RestoreContext::default(),
    )?;
    Ok(Decoded {
        cells,
        layouts,
        table,
    })
}
fn roundtrip(before: &TranscriptBookmark, originals: &[Arc<dyn HistoryCell>]) -> io::Result<()> {
    let mut cells = cells::Capture::default();
    let roots = originals
        .iter()
        .map(|cell| cells.intern(cell))
        .collect::<io::Result<Vec<_>>>()?;
    let mut layouts = layouts::Capture::default();
    let mut graph = tables::Capture::new(&mut cells, &mut layouts);
    graph.register(originals)?;
    let one = Data::capture(before, &mut graph)?;
    let two = Data::capture(before, &mut graph)?;
    let bytes = serde_json::to_vec(&(one, two)).map_err(invalid)?;
    let (one, two): (Data, Data) = serde_json::from_slice(&bytes).map_err(invalid)?;
    let table = graph.export();
    drop(graph);
    let decoded = decode(cells, layouts, table)?;
    let graph = decoded.graph()?;
    let after = one.restore(&graph)?;
    let second = two.restore(&graph)?;
    let rebound = roots
        .into_iter()
        .map(|reference| decoded.cells.cell(reference))
        .collect::<io::Result<Vec<_>>>()?;
    let key = |key: EntryKey| -> io::Result<EntryKey> {
        match key {
            EntryKey::Live => Ok(EntryKey::Live),
            _ => originals
                .iter()
                .position(|cell| EntryKey::cell(cell) == key)
                .map(|at| EntryKey::cell(&rebound[at]))
                .ok_or_else(|| invalid("old bookmark key owner missing")),
        }
    };
    let position = |old: Position, new: Position| -> io::Result<()> {
        match (old, new) {
            (Position::Latest, Position::Latest) => Ok(()),
            (Position::Reading(a), Position::Reading(b)) => {
                assert_eq!(
                    (a.index, a.offset, a.row_bias),
                    (b.index, b.offset, b.row_bias)
                );
                assert_eq!(key(a.key)?, b.key);
                Ok(())
            }
            _ => Err(invalid("native bookmark position kind changed")),
        }
    };
    position(before.position, after.position)?;
    match (before.saved_position, after.saved_position) {
        (Some(a), Some(b)) => position(a, b)?,
        (None, None) => {}
        _ => return Err(invalid("native saved bookmark position missing")),
    }
    assert_eq!(
        (before.detailed, before.mode, before.unseen_activity),
        (after.detailed, after.mode, after.unseen_activity)
    );
    assert_eq!(before.last_tail.map(key).transpose()?, after.last_tail);
    match (
        before.suppressed_prompt_header,
        after.suppressed_prompt_header,
    ) {
        (Some(a), Some(b)) => {
            assert!(a.viewport == b.viewport);
            assert_eq!(a.row, b.row);
            assert_eq!(key(a.key)?, b.key);
        }
        (None, None) => {}
        _ => return Err(invalid("native paused header presence changed")),
    }
    match (&before.snapshot, &after.snapshot, &second.snapshot) {
        (Some(a), Some(b), Some(c)) => {
            assert!(Arc::ptr_eq(&b.cells, &c.cells));
            assert!(!Arc::ptr_eq(&a.cells, &b.cells));
            assert_eq!(a.pinned.len(), b.pinned.len());
            for (old, layout) in &a.pinned {
                let target = key(*old)?;
                let b = b
                    .pinned
                    .get(&target)
                    .ok_or_else(|| invalid("new bookmark pin missing"))?;
                let c = c
                    .pinned
                    .get(&target)
                    .ok_or_else(|| invalid("second bookmark pin missing"))?;
                assert!(Arc::ptr_eq(b, c));
                assert!(!Arc::ptr_eq(layout, b));
                assert_eq!(layout.text(), b.text());
            }
        }
        (None, None, None) => {}
        _ => return Err(invalid("native bookmark snapshot presence changed")),
    }
    Ok(())
}
#[test]
fn native_browsing_bookmark_keeps_both_positions_raw_mode_tail_and_unseen_snapshot()
-> io::Result<()> {
    let mut roots = vec![
        cell("first prompt\nfirst response"),
        cell("second prompt\nsecond response"),
    ];
    let mut view = TranscriptView::default();
    render(&mut view, &roots, 30, 3);
    view.jump_to_entry(&roots, 0);
    view.set_presentation(true, HistoryRenderMode::Raw);
    render(&mut view, &roots, 30, 3);
    roots.push(cell("genuine new output"));
    view.sync_history_tail(&roots);
    let bookmark = view.bookmark(&roots);
    assert!(bookmark.saved_position.is_some());
    assert!(bookmark.snapshot.is_some());
    assert!(bookmark.unseen_activity);
    assert!(matches!(bookmark.mode, HistoryRenderMode::Raw));
    roundtrip(&bookmark, &roots)
}
#[test]
fn native_following_bookmark_has_no_fabricated_snapshot() -> io::Result<()> {
    let roots = vec![cell("actual visible output")];
    let mut view = TranscriptView::default();
    view.set_presentation(true, HistoryRenderMode::Rich);
    render(&mut view, &roots, 30, 3);
    let bookmark = view.bookmark(&roots);
    assert!(matches!(bookmark.position, Position::Latest));
    assert!(bookmark.snapshot.is_none());
    roundtrip(&bookmark, &roots)
}
#[test]
fn native_turn_boundary_paused_header_keeps_viewport_key_and_row() -> io::Result<()> {
    let roots = vec![
        user("First question"),
        cell("first answer"),
        user("Second question"),
        cell("done 10:16 AM"),
    ];
    // Native header reservation can advance the top into the next visible
    // prompt. Exercise the real viewport crossover, without constructing it.
    for height in 4..=9 {
        let mut view = TranscriptView::default();
        render(&mut view, &roots, 40, height);
        if view.suppressed_prompt_header.is_some() {
            let bookmark = view.bookmark(&roots);
            assert!(bookmark.suppressed_prompt_header.is_some());
            return roundtrip(&bookmark, &roots);
        }
    }
    Err(invalid(
        "native turn-boundary header reservation was not reached",
    ))
}
#[test]
fn omitted_native_options_foreign_owners_and_extra_mode_fields_refuse() -> io::Result<()> {
    let roots = vec![cell("original bookmark content")];
    let mut view = TranscriptView::default();
    render(&mut view, &roots, 24, 3);
    view.jump_to_entry(&roots, 0);
    let bookmark = view.bookmark(&roots);
    let mut cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut graph = tables::Capture::new(&mut cells, &mut layouts);
    let wire = serde_json::to_value(Data::capture(&bookmark, &mut graph)?).map_err(invalid)?;
    for field in [
        "savedPosition",
        "suppressedPromptHeader",
        "snapshot",
        "lastTail",
    ] {
        let mut bad = wire.clone();
        bad.as_object_mut()
            .ok_or_else(|| invalid("bookmark object missing"))?
            .remove(field);
        assert!(serde_json::from_value::<Data>(bad).is_err());
    }
    let mut extra = wire.clone();
    extra["mode"] = json!({"kind":"rich","nativePermission":true});
    assert!(serde_json::from_value::<Data>(extra).is_err());
    let mut foreign = wire;
    foreign["position"]["anchor"]["key"] = json!({"kind":"cell","original":1_000_001});
    let foreign: Data = serde_json::from_value(foreign).map_err(invalid)?;
    let table = graph.export();
    drop(graph);
    let decoded = decode(cells, layouts, table)?;
    assert!(foreign.restore(&decoded.graph()?).is_err());
    Ok(())
}
