// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native Find actions, editor generations and historical layout identity.
use super::*;
use crate::bottom_pane::alab_retained_editors as editors;
use crate::history_cell::UserHistoryCell;
use crate::history_cell::UserMessagePresentation;
use crate::history_cell::alab_retained_cells as cells;
use crate::transcript_view::text::alab_retained_layout as layouts;
const LIMIT: usize = 1_048_576;

fn original() -> (TranscriptView, Vec<Arc<dyn HistoryCell>>) {
    let cells: Vec<Arc<dyn HistoryCell>> = vec![Arc::new(UserHistoryCell {
        message: "original α needle β revision".into(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: Vec::new(),
        presentation: UserMessagePresentation::Typed,
    })];
    let mut view = TranscriptView::default();
    let area = Rect::new(0, 0, 24, 9);
    view.render(area, &mut Buffer::empty(area), &cells);
    view.begin_search();
    view.paste_search("needle");
    for _ in 0..512 {
        if !view.advance_search(&cells) {
            break;
        }
    }
    assert!(matches!(view.search.progress, Progress::Found));
    assert!(view.handle_search_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &cells));
    assert!(view.search.is_reading());
    (view, cells)
}

#[test]
fn actual_find_keeps_editor_generation_and_same_match_layout_across_restored_views()
-> io::Result<()> {
    let (actual, original_cells) = original();
    let mut cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut graph = tables::Capture::new(&mut cells, &mut layouts);
    graph.register(&original_cells)?;
    let first = Data::capture(&actual.search, &mut graph)?;
    let second = Data::capture(&actual.search, &mut graph)?;
    // The actual editor is borrowed until this ONE enclosing generation encode.
    let bytes = editors::encode(&(first, second), LIMIT).map_err(invalid)?;
    let raw: serde_json::Value = serde_json::from_slice(&bytes).map_err(invalid)?;
    assert_eq!(raw["generations"].as_array().unwrap().len(), 1);
    let (first, second): (Data, Data) = editors::decode::<_>(&bytes, LIMIT, 1).map_err(invalid)?;
    let table_data = graph.export();
    drop(graph);
    let cell_bytes = cells.encode(layouts.lines(), LIMIT)?;
    let layout_bytes = layouts.encode(LIMIT)?;
    let mut layouts = layouts::Restored::decode(&layout_bytes, LIMIT)?;
    let cells = cells::Restored::decode(
        &cell_bytes,
        LIMIT,
        layouts.lines(),
        &mut cells::RestoreContext::default(),
    )?;
    let graph = tables::Restore::new(table_data, &cells, &layouts)?;
    let first = first.restore(&graph, &RuntimeKeymap::defaults())?;
    let second = second.restore(&graph, &RuntimeKeymap::defaults())?;
    let old = actual.search.current.as_ref().unwrap();
    let one = first.current.as_ref().unwrap();
    let two = second.current.as_ref().unwrap();
    assert!(matches!(first.mode, SearchMode::Reading));
    assert_eq!(first.editor.text(), actual.search.editor.text());
    assert_eq!(first.editor.cursor(), actual.search.editor.cursor());
    assert_eq!(first.folded_query, actual.search.folded_query);
    assert_eq!(one.end, old.end);
    assert_eq!(one.anchor.offset, old.anchor.offset);
    assert_ne!(one.anchor.key, old.anchor.key);
    assert_eq!(one.anchor.key, two.anchor.key);
    assert!(Arc::ptr_eq(&one.layout, &two.layout));
    assert!(!Arc::ptr_eq(&one.layout, &old.layout));
    assert_eq!(one.layout.text(), old.layout.text());
    assert_eq!(&one.layout.text()[one.anchor.offset..one.end], "needle");
    Ok(())
}

#[test]
fn find_requires_observed_optional_fields_and_rejects_match_outside_original_revision()
-> io::Result<()> {
    let (actual, original_cells) = original();
    let mut cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut graph = tables::Capture::new(&mut cells, &mut layouts);
    graph.register(&original_cells)?;
    let data = Data::capture(&actual.search, &mut graph)?;
    let bytes = editors::encode(&data, LIMIT).map_err(invalid)?;
    let raw: serde_json::Value = serde_json::from_slice(&bytes).map_err(invalid)?;
    for field in [
        "saved_snapshot",
        "current",
        "saved_select",
        "scanning_layout",
        "page_start",
        "live",
        "live_key",
    ] {
        let mut omitted = raw.clone();
        omitted["body"].as_object_mut().unwrap().remove(field);
        let bytes = serde_json::to_vec(&omitted).map_err(invalid)?;
        assert!(
            editors::decode::<Data>(&bytes, LIMIT, 1).is_err(),
            "{field}"
        );
    }
    let mut bad = raw;
    bad["body"]["current"]["end"] = serde_json::json!(u64::MAX);
    let bytes = serde_json::to_vec(&bad).map_err(invalid)?;
    let bad: Data = editors::decode::<_>(&bytes, LIMIT, 1).map_err(invalid)?;
    let data = graph.export();
    drop(graph);
    let cell_bytes = cells.encode(layouts.lines(), LIMIT)?;
    let layout_bytes = layouts.encode(LIMIT)?;
    let mut layouts = layouts::Restored::decode(&layout_bytes, LIMIT)?;
    let cells = cells::Restored::decode(
        &cell_bytes,
        LIMIT,
        layouts.lines(),
        &mut cells::RestoreContext::default(),
    )?;
    let graph = tables::Restore::new(data, &cells, &layouts)?;
    assert!(bad.restore(&graph, &RuntimeKeymap::defaults()).is_err());
    Ok(())
}

#[test]
fn borrowed_failed_field_resolution_keeps_actual_editor_buffer_then_final_move_reuses_it()
-> io::Result<()> {
    let (actual, roots) = original();
    let mut capture_cells = cells::Capture::default();
    let mut capture_layouts = layouts::Capture::default();
    let mut capture = tables::Capture::new(&mut capture_cells, &mut capture_layouts);
    capture.register(&roots)?;
    let data = Data::capture(&actual.search, &mut capture)?;
    let bytes = editors::encode(&data, LIMIT).map_err(invalid)?;
    let mut data: Data = editors::decode(&bytes, LIMIT, 1).map_err(invalid)?;
    let tables = capture.export();
    drop(capture);
    let cell_bytes = capture_cells.encode(capture_layouts.lines(), LIMIT)?;
    let layout_bytes = capture_layouts.encode(LIMIT)?;
    let mut layouts = layouts::Restored::decode(&layout_bytes, LIMIT)?;
    let cells = cells::Restored::decode(
        &cell_bytes,
        LIMIT,
        layouts.lines(),
        &mut cells::RestoreContext::default(),
    )?;
    let graph = tables::Restore::new(tables, &cells, &layouts)?;
    let buffer = data.editor.text().as_ptr();
    let text = data.editor.text().to_owned();
    let cursor = data.editor.cursor();
    let end = data
        .current
        .as_ref()
        .ok_or_else(|| invalid("actual native search match missing"))?
        .end;
    data.current
        .as_mut()
        .ok_or_else(|| invalid("actual native search match missing"))?
        .end = u64::MAX;
    assert!(data.prepare_fields(&graph).is_err());
    assert_eq!(data.editor.text().as_ptr(), buffer);
    assert_eq!(data.editor.text(), text);
    assert_eq!(data.editor.cursor(), cursor);
    data.current
        .as_mut()
        .ok_or_else(|| invalid("actual native search match missing"))?
        .end = end;
    // Two pure preflights must not create/encode another native editor graph.
    let unused = data.prepare_fields(&graph)?;
    drop(unused);
    let fields = data.prepare_fields(&graph)?;
    let restored = data.finish_fields(fields, &RuntimeKeymap::defaults());
    assert_eq!(restored.editor.text().as_ptr(), buffer);
    assert_eq!(restored.editor.text(), text);
    assert_eq!(restored.editor.cursor(), cursor);
    Ok(())
}
