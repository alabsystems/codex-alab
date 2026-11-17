// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual render/capture held revisions over the same native cell/layout graphs.
use super::*;
use crate::history_cell::UserHistoryCell;
use crate::history_cell::UserMessagePresentation;
use crate::history_cell::alab_retained_cells as cells;
use crate::transcript_view::text::alab_retained_layout as layouts;
const LIMIT: usize = 1_048_576;
fn original() -> ViewSnapshot {
    let cell: Arc<dyn HistoryCell> = Arc::new(UserHistoryCell {
        message: "original native held revision α β gamma".into(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: Vec::new(),
        presentation: UserMessagePresentation::Typed,
    });
    let cells = vec![cell];
    let mut view = TranscriptView::default();
    let area = Rect::new(0, 0, 18, 12);
    view.render(area, &mut Buffer::empty(area), &cells);
    view.capture_snapshot(&cells)
}
#[test]
fn actual_snapshot_clone_keeps_same_native_cell_slice_layout_and_activity_aliases() -> io::Result<()>
{
    let actual = original();
    let clone = actual.clone();
    assert!(!actual.pinned.is_empty());
    assert!(!actual.activities.is_empty());
    let mut cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut graph = Capture::new(&mut cells, &mut layouts);
    let first = Data::capture(&actual, &mut graph)?;
    let second = Data::capture(&clone, &mut graph)?;
    let tables = graph.export();
    drop(graph);
    let cells_raw = cells.encode(layouts.lines(), LIMIT)?;
    let layout_raw = layouts.encode(LIMIT)?;
    let mut layouts = layouts::Restored::decode(&layout_raw, LIMIT)?;
    let cells = cells::Restored::decode(
        &cells_raw,
        LIMIT,
        layouts.lines(),
        &mut cells::RestoreContext::default(),
    )?;
    let graph = Restore::new(tables, &cells, &layouts)?;
    let first = first.restore(&graph)?;
    let second = second.restore(&graph)?;
    assert!(Arc::ptr_eq(&first.cells, &second.cells));
    assert!(!Arc::ptr_eq(&first.cells, &actual.cells));
    assert!(Arc::ptr_eq(&first.cells[0], &second.cells[0]));
    assert!(first.cells[0].as_any().is::<UserHistoryCell>());
    let key = EntryKey::cell(&first.cells[0]);
    let old_key = EntryKey::cell(&actual.cells[0]);
    assert_ne!(key, old_key);
    assert!(Arc::ptr_eq(&first.pinned[&key], &second.pinned[&key]));
    assert_eq!(first.pinned[&key].text(), actual.pinned[&old_key].text());
    assert!(Arc::ptr_eq(
        &first.activities[&key],
        &second.activities[&key]
    ));
    Ok(())
}
#[test]
fn snapshot_requires_explicit_search_revision_and_rejects_duplicate_pinned_native_keys()
-> io::Result<()> {
    let actual = original();
    let mut cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut graph = Capture::new(&mut cells, &mut layouts);
    let data = Data::capture(&actual, &mut graph)?;
    let mut wire = serde_json::to_value(&data).map_err(std::io::Error::other)?;
    wire.as_object_mut().unwrap().remove("search_live");
    assert!(serde_json::from_value::<Data>(wire).is_err());
    let mut wire = serde_json::to_value(data).map_err(std::io::Error::other)?;
    let duplicate = wire["pinned"][0].clone();
    wire["pinned"].as_array_mut().unwrap().push(duplicate);
    let bad: Data = serde_json::from_value(wire).map_err(std::io::Error::other)?;
    let tables = graph.export();
    drop(graph);
    let cells_raw = cells.encode(layouts.lines(), LIMIT)?;
    let layout_raw = layouts.encode(LIMIT)?;
    let mut layouts = layouts::Restored::decode(&layout_raw, LIMIT)?;
    let cells = cells::Restored::decode(
        &cells_raw,
        LIMIT,
        layouts.lines(),
        &mut cells::RestoreContext::default(),
    )?;
    let graph = Restore::new(tables, &cells, &layouts)?;
    assert!(bad.restore(&graph).is_err());
    Ok(())
}
