// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native action/graph controls only; no synthetic clipboard, search job or Native success resource.
use super::*;
use crate::history_cell::HistoryCell;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::alab_retained_cells as cells;
use crate::transcript_view::text::alab_retained_layout as layouts;
use ratatui::text::Line;
use serde_json::json;
const LIMIT: usize = 1_048_576;
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn require(condition: bool, message: &str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(invalid(message))
    }
}
fn original_cells(lines: &[&str]) -> Vec<Arc<dyn HistoryCell>> {
    vec![Arc::new(PlainHistoryCell::new(
        lines
            .iter()
            .map(|line| Line::from((*line).to_owned()))
            .collect(),
    ))]
}
fn render(view: &mut TranscriptView, actual: &[Arc<dyn HistoryCell>]) {
    let area = Rect::new(0, 0, 32, 6);
    view.render(area, &mut Buffer::empty(area), actual);
}
fn press(
    view: &mut TranscriptView,
    actual: &[Arc<dyn HistoryCell>],
    code: KeyCode,
) -> io::Result<()> {
    require(
        matches!(
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE), actual),
            Some(ViewAction::Changed)
        ),
        "actual native selection action did not change view",
    )
}
fn mode(view: &TranscriptView) -> io::Result<SelectMode> {
    view.select_mode
        .ok_or_else(|| invalid("actual native select mode missing"))
}
fn rebound_anchor(
    before: Anchor,
    after: Anchor,
    originals: &[Arc<dyn HistoryCell>],
    restored: &[Arc<dyn HistoryCell>],
) -> io::Result<()> {
    require(
        (before.index, before.offset, before.row_bias)
            == (after.index, after.offset, after.row_bias),
        "native anchor coordinates changed",
    )?;
    match before.key {
        EntryKey::Live => require(after.key == EntryKey::Live, "logical live slot changed"),
        EntryKey::Cell(_) => {
            let index = originals
                .iter()
                .position(|cell| EntryKey::cell(cell) == before.key)
                .ok_or_else(|| invalid("actual old anchor owner missing"))?;
            require(
                after.key == EntryKey::cell(&restored[index]) && after.key != before.key,
                "native anchor did not bind actual new graph allocation",
            )
        }
    }
}
fn rebound_edges(
    before: Option<(Anchor, Anchor)>,
    after: Option<(Anchor, Anchor)>,
    originals: &[Arc<dyn HistoryCell>],
    restored: &[Arc<dyn HistoryCell>],
) -> io::Result<()> {
    match (before, after) {
        (Some((a, b)), Some((c, d))) => {
            rebound_anchor(a, c, originals, restored)?;
            rebound_anchor(b, d, originals, restored)
        }
        (None, None) => Ok(()),
        _ => Err(invalid("original optional native mark edges changed")),
    }
}
fn rebound_mark(
    before: Option<Mark>,
    after: Option<Mark>,
    originals: &[Arc<dyn HistoryCell>],
    restored: &[Arc<dyn HistoryCell>],
) -> io::Result<()> {
    match (before, after) {
        (Some(a), Some(b)) => {
            require(a.unit == b.unit, "native mark unit changed")?;
            rebound_anchor(a.anchor, b.anchor, originals, restored)?;
            rebound_anchor(a.active, b.active, originals, restored)?;
            rebound_edges(a.anchor_edges, b.anchor_edges, originals, restored)?;
            rebound_edges(a.active_edges, b.active_edges, originals, restored)
        }
        (None, None) => Ok(()),
        _ => Err(invalid("original optional native mark changed")),
    }
}
fn roundtrip(before: SelectMode, originals: &[Arc<dyn HistoryCell>]) -> io::Result<SelectMode> {
    let mut native_cells = cells::Capture::default();
    let mut native_layouts = layouts::Capture::default();
    let roots = originals
        .iter()
        .map(|actual| native_cells.intern(actual))
        .collect::<io::Result<Vec<_>>>()?;
    let mut capture = Capture::new(&mut native_cells, &mut native_layouts);
    capture.register(originals)?;
    let data = Data::capture(&before, &capture)?;
    let raw = serde_json::to_vec(&data).map_err(io::Error::other)?;
    let data: Data = serde_json::from_slice(&raw).map_err(io::Error::other)?;
    let tables = capture.export();
    drop(capture);
    let cells_raw = native_cells.encode(native_layouts.lines(), LIMIT)?;
    let layouts_raw = native_layouts.encode(LIMIT)?;
    let mut native_layouts = layouts::Restored::decode(&layouts_raw, LIMIT)?;
    let native_cells = cells::Restored::decode(
        &cells_raw,
        LIMIT,
        native_layouts.lines(),
        &mut cells::RestoreContext::default(),
    )?;
    let graph = Restore::new(tables, &native_cells, &native_layouts)?;
    let after = data.restore(&graph)?;
    // These are the real new graph allocations from the actual original refs,
    // never decoded memory pointers or integer-constructed CellRefs.
    let roots = roots
        .into_iter()
        .map(|reference| native_cells.cell(reference))
        .collect::<io::Result<Vec<_>>>()?;
    rebound_mark(before.mark, after.mark, originals, &roots)?;
    require(
        before.preferred_column == after.preferred_column,
        "native preferred column changed",
    )?;
    match (before.search_origin, after.search_origin) {
        (Some(a), Some(b)) => {
            rebound_mark(a.mark, b.mark, originals, &roots)?;
            require(
                a.preferred_column == b.preferred_column,
                "saved preferred column changed",
            )?;
            rebound_anchor(a.start, b.start, originals, &roots)?;
            rebound_anchor(a.end, b.end, originals, &roots)?;
        }
        (None, None) => {}
        _ => return Err(invalid("native saved search-origin presence changed")),
    }
    Ok(after)
}

#[test]
fn original_keyboard_character_mark_and_vertical_motion_keep_preferred_column_and_new_graph_keys()
-> io::Result<()> {
    let actual = original_cells(&["abcdef", "x", "abcdef"]);
    let mut view = TranscriptView::default();
    render(&mut view, &actual);
    require(
        view.begin_select_mode(&actual),
        "native select entry denied",
    )?;
    for _ in 0..3 {
        press(&mut view, &actual, KeyCode::Right)?;
    }
    press(&mut view, &actual, KeyCode::Char('v'))?;
    press(&mut view, &actual, KeyCode::Up)?;
    let before = mode(&view)?;
    require(
        before
            .mark
            .is_some_and(|mark| mark.unit == SelectionUnit::Character),
        "actual character mark missing",
    )?;
    require(
        before.preferred_column.is_some(),
        "native vertical preferred column missing",
    )?;
    let _ = roundtrip(before, &actual)?;
    Ok(())
}
#[test]
fn original_mouse_word_and_line_marks_keep_both_edge_pairs_and_swapped_endpoints() -> io::Result<()>
{
    for (clicks, lines, row, unit) in [
        (2, vec!["alpha beta"], 0, SelectionUnit::Word),
        (3, vec!["first", "second"], 1, SelectionUnit::Line),
    ] {
        let actual = original_cells(&lines);
        let mut view = TranscriptView::default();
        render(&mut view, &actual);
        view.begin_selection(&actual, 1, row, clicks);
        require(
            view.begin_select_mode(&actual),
            "native mouse range adoption denied",
        )?;
        let mark = mode(&view)?
            .mark
            .ok_or_else(|| invalid("actual adopted mouse mark missing"))?;
        require(
            mark.unit == unit && mark.anchor_edges.is_some() && mark.active_edges.is_some(),
            "native mouse mark edge/unit state missing",
        )?;
        press(&mut view, &actual, KeyCode::Char('o'))?;
        let _ = roundtrip(mode(&view)?, &actual)?;
    }
    Ok(())
}
#[test]
fn original_search_entry_keeps_saved_mark_column_and_full_original_range() -> io::Result<()> {
    let actual = original_cells(&["abcdef", "x", "abcdef"]);
    let mut view = TranscriptView::default();
    render(&mut view, &actual);
    require(
        view.begin_select_mode(&actual),
        "native select entry denied",
    )?;
    press(&mut view, &actual, KeyCode::Right)?;
    press(&mut view, &actual, KeyCode::Char('v'))?;
    press(&mut view, &actual, KeyCode::Up)?;
    let before = mode(&view)?;
    let selection = view
        .selection
        .as_ref()
        .ok_or_else(|| invalid("actual keyboard selection missing"))?;
    let (start, end) = (selection.start, selection.end);
    view.begin_search();
    let current = mode(&view)?;
    let saved = current
        .search_origin
        .ok_or_else(|| invalid("actual select search origin missing"))?;
    require(
        current.mark.is_none()
            && saved.mark.is_some()
            && saved.preferred_column == before.preferred_column
            && saved.start == start
            && saved.end == end,
        "original search did not retain actual selection origin",
    )?;
    let _ = roundtrip(current, &actual)?;
    Ok(())
}
#[test]
fn missing_optional_fields_foreign_cell_refs_and_unknown_or_extra_units_refuse() -> io::Result<()> {
    let actual = original_cells(&["actual original text"]);
    let mut view = TranscriptView::default();
    render(&mut view, &actual);
    require(
        view.begin_select_mode(&actual),
        "native select entry denied",
    )?;
    press(&mut view, &actual, KeyCode::Char('v'))?;
    let current = mode(&view)?;
    let mut native_cells = cells::Capture::default();
    let mut native_layouts = layouts::Capture::default();
    let mut capture = Capture::new(&mut native_cells, &mut native_layouts);
    require(
        Data::capture(&current, &capture).is_err(),
        "an unregistered original pointer manufactured a graph owner",
    )?;
    capture.register(&actual)?;
    let wire =
        serde_json::to_value(Data::capture(&current, &capture)?).map_err(io::Error::other)?;
    for field in ["mark", "preferredColumn", "searchOrigin"] {
        let mut bad = wire.clone();
        bad.as_object_mut()
            .ok_or_else(|| invalid("DATA object missing"))?
            .remove(field);
        require(
            serde_json::from_value::<Data>(bad).is_err(),
            "omitted original optional mode field accepted",
        )?;
    }
    for field in ["anchorEdges", "activeEdges"] {
        let mut bad = wire.clone();
        bad["mark"]
            .as_object_mut()
            .ok_or_else(|| invalid("mark DATA missing"))?
            .remove(field);
        require(
            serde_json::from_value::<Data>(bad).is_err(),
            "omitted native mark edges accepted",
        )?;
    }
    for unit in [
        json!({"kind":"foreign"}),
        json!({"kind":"character","nativePermission":true}),
    ] {
        let mut bad = wire.clone();
        bad["mark"]["unit"] = unit;
        require(
            serde_json::from_value::<Data>(bad).is_err(),
            "unknown/foreign unit DATA accepted",
        )?;
    }
    let mut foreign = wire;
    foreign["mark"]["anchor"]["key"] = json!({"kind":"cell","original":1_000_001});
    let bad: Data = serde_json::from_value(foreign).map_err(io::Error::other)?;
    let tables = capture.export();
    drop(capture);
    let raw = native_cells.encode(native_layouts.lines(), LIMIT)?;
    let layout_raw = native_layouts.encode(LIMIT)?;
    let mut native_layouts = layouts::Restored::decode(&layout_raw, LIMIT)?;
    let native_cells = cells::Restored::decode(
        &raw,
        LIMIT,
        native_layouts.lines(),
        &mut cells::RestoreContext::default(),
    )?;
    let graph = Restore::new(tables, &native_cells, &native_layouts)?;
    require(
        bad.restore(&graph).is_err(),
        "foreign cell reference became an original mark owner",
    )?;
    view.begin_search();
    let mut captured_cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut capture = Capture::new(&mut captured_cells, &mut layouts);
    capture.register(&actual)?;
    let wire =
        serde_json::to_value(Data::capture(&mode(&view)?, &capture)?).map_err(io::Error::other)?;
    for field in ["mark", "preferredColumn"] {
        let mut bad = wire.clone();
        bad["searchOrigin"]
            .as_object_mut()
            .ok_or_else(|| invalid("actual saved search DATA missing"))?
            .remove(field);
        require(
            serde_json::from_value::<Data>(bad).is_err(),
            "omitted saved search optional accepted",
        )?;
    }
    Ok(())
}
