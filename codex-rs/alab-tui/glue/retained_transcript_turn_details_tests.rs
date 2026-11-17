// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Real native indexing, summary clones and mutable-row continuation; no Native grant.
use super::*;
use crate::history_cell::UserMessagePresentation;
use crate::history_cell::alab_retained_cells as cells;
use crate::transcript_view::alab_retained_view::tables;
use crate::transcript_view::text::alab_retained_layout as layouts;
use codex_app_server_protocol::DynamicToolCallStatus;
use codex_app_server_protocol::ThreadItemPayload;
use codex_protocol::models::MessagePhase;
use serde_json::json;
const LIMIT: usize = 1_048_576;
fn tool(status: DynamicToolCallStatus) -> io::Result<DynamicToolCallCell> {
    DynamicToolCallCell::from_item(ThreadItemPayload::DynamicToolCall {
        id: "actual-turn-tool".into(),
        namespace: Some("original".into()),
        tool: "tool".into(),
        arguments: json!({"exact":"arguments"}),
        status,
        content_items: None,
        success: None,
        duration_ms: None,
    })
    .ok_or_else(|| invalid("original dynamic item not converted"))
}
fn prompt() -> Arc<dyn HistoryCell> {
    Arc::new(UserHistoryCell {
        message: "original prompt".into(),
        text_elements: vec![],
        local_image_paths: vec![],
        remote_image_urls: vec![],
        presentation: UserMessagePresentation::Typed,
    })
}
type NativeTurnFixture = (
    TranscriptView,
    Vec<Arc<dyn HistoryCell>>,
    DynamicToolCallCell,
);
fn fixture() -> io::Result<NativeTurnFixture> {
    let tool = tool(DynamicToolCallStatus::InProgress)?;
    let roots = vec![prompt(), Arc::new(tool.clone()) as Arc<dyn HistoryCell>];
    let mut view = TranscriptView::default();
    view.sync_turn_details(&roots, true, Some(&tool));
    Ok((view, roots, tool))
}
fn details(view: &TranscriptView) -> io::Result<&TurnDetails> {
    view.turn_details
        .as_ref()
        .ok_or_else(|| invalid("actual turn index missing"))
}
fn group(row: &TurnRow) -> io::Result<&Arc<ToolGroup>> {
    match row {
        TurnRow::Tools(group) => Ok(group),
        _ => Err(invalid("actual tools summary missing")),
    }
}
struct Decoded {
    cells: cells::Restored,
    layouts: layouts::Restored,
    resources: cells::RestoreContext,
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
    let cell_bytes = cells.encode(layouts.lines(), LIMIT)?;
    let layout_bytes = layouts.encode(LIMIT)?;
    let mut layouts = layouts::Restored::decode(&layout_bytes, LIMIT)?;
    let mut resources = cells::RestoreContext::default();
    let cells = cells::Restored::decode(&cell_bytes, LIMIT, layouts.lines(), &mut resources)?;
    Ok(Decoded {
        cells,
        layouts,
        resources,
        table,
    })
}
fn exact(
    old: &TurnDetails,
    new: &TurnDetails,
    old_roots: &[Arc<dyn HistoryCell>],
    new_roots: &[Arc<dyn HistoryCell>],
) -> io::Result<()> {
    assert_eq!(
        (old.start, old.live.clone(), old.indexed_snapshot),
        (new.start, new.live.clone(), new.indexed_snapshot)
    );
    assert_eq!(old.rows.len(), new.rows.len());
    let rebound = |key: EntryKey| -> io::Result<EntryKey> {
        match key {
            EntryKey::Live => Ok(EntryKey::Live),
            _ => old_roots
                .iter()
                .position(|cell| EntryKey::cell(cell) == key)
                .map(|at| EntryKey::cell(&new_roots[at]))
                .ok_or_else(|| invalid("old native key has no root")),
        }
    };
    let expected = old
        .source
        .map(|(length, last)| Ok::<_, io::Error>((length, last.map(rebound).transpose()?)))
        .transpose()?;
    assert_eq!(expected, new.source);
    let mut old_cells = cells::Capture::default();
    let mut old_refs = CaptureRefs::new(&mut old_cells);
    old_refs.register(old_roots)?;
    let mut new_cells = cells::Capture::default();
    let mut new_refs = CaptureRefs::new(&mut new_cells);
    new_refs.register(new_roots)?;
    for (old, new) in old.rows.iter().zip(&new.rows) {
        assert_eq!(rebound(old.key)?, new.key);
        assert_eq!(old.parent, new.parent);
        match (&old.row, &new.row) {
            (
                Some(TurnRow::Prompt { id: a, label: b }),
                Some(TurnRow::Prompt { id: c, label: d }),
            ) => {
                assert_eq!(b, d);
                assert_eq!(
                    serde_json::to_value(old_refs.activity(a)).map_err(invalid)?,
                    serde_json::to_value(new_refs.activity(c)).map_err(invalid)?
                );
            }
            (Some(TurnRow::Tools(a)), Some(TurnRow::Tools(b))) => assert_eq!(
                serde_json::to_value(GroupData::capture(a, &old_refs)?).map_err(invalid)?,
                serde_json::to_value(GroupData::capture(b, &new_refs)?).map_err(invalid)?
            ),
            (Some(TurnRow::Hidden), Some(TurnRow::Hidden)) | (None, None) => {}
            _ => return Err(invalid("native turn row kind changed")),
        }
    }
    assert_eq!(
        old.pending
            .iter()
            .map(|(at, tool)| (*at, tool.call_id()))
            .collect::<Vec<_>>(),
        new.pending
            .iter()
            .map(|(at, tool)| (*at, tool.call_id()))
            .collect::<Vec<_>>()
    );
    Ok(())
}
#[test]
fn native_summary_clones_share_group_and_pending_tools_keep_global_mutable_aliases()
-> io::Result<()> {
    let (view, original, registry) = fixture()?;
    let before = details(&view)?;
    assert_eq!(before.pending.len(), 1);
    assert!(!before.live.0.is_empty());
    let row = view
        .turn_row(&original, 1)
        .ok_or_else(|| invalid("original summary row missing"))?;
    let mut cells = cells::Capture::default();
    let refs = original
        .iter()
        .map(|root| cells.intern(root))
        .collect::<io::Result<Vec<_>>>()?;
    let mut layouts = layouts::Capture::default();
    let mut graph = Capture::new(&mut cells, &mut layouts);
    graph.register(&original)?;
    let one = Data::capture(before, &mut graph)?;
    let two = Data::capture(before, &mut graph)?;
    let row = RowData::capture(&row, &mut graph)?;
    let registry = graph.dynamic_cell(&registry)?;
    let bytes = serde_json::to_vec(&(one, two, row, registry)).map_err(invalid)?;
    let (one, two, row, registry): (Data, Data, RowData, DynamicData) =
        serde_json::from_slice(&bytes).map_err(invalid)?;
    let table = graph.export();
    drop(graph);
    let decoded = decode(cells, layouts, table)?;
    let graph = decoded.graph()?;
    let one = one.restore(&graph, &decoded.resources)?;
    let two = two.restore(&graph, &decoded.resources)?;
    let row = row.restore(&graph)?;
    let registry = decoded.resources.dynamic_cell(&registry)?;
    let roots = refs
        .into_iter()
        .map(|reference| decoded.cells.cell(reference))
        .collect::<io::Result<Vec<_>>>()?;
    exact(before, &one, &original, &roots)?;
    exact(before, &two, &original, &roots)?;
    let a = group(
        one.rows[1]
            .row
            .as_ref()
            .ok_or_else(|| invalid("first restored row missing"))?,
    )?;
    let b = group(
        two.rows[1]
            .row
            .as_ref()
            .ok_or_else(|| invalid("second restored row missing"))?,
    )?;
    assert!(Arc::ptr_eq(a, b));
    assert!(Arc::ptr_eq(a, group(&row)?));
    let original_group = group(
        before.rows[1]
            .row
            .as_ref()
            .ok_or_else(|| invalid("original tools row missing"))?,
    )?;
    assert!(!Arc::ptr_eq(a, original_group));
    let restored = roots[1]
        .as_any()
        .downcast_ref::<DynamicToolCallCell>()
        .ok_or_else(|| invalid("restored original tool type changed"))?;
    restored.mark_interrupted();
    assert!(!one.pending[0].1.is_active());
    assert!(!two.pending[0].1.is_active());
    assert!(!registry.is_active());
    assert!(before.pending[0].1.is_active());
    let mut resumed = TranscriptView {
        turn_details: Some(one),
        ..TranscriptView::default()
    };
    resumed.sync_turn_details(&roots, true, None);
    assert!(details(&resumed)?.pending.is_empty());
    Ok(())
}
#[test]
fn independent_native_tool_groups_do_not_merge_equal_values() -> io::Result<()> {
    let (view, roots, _) = fixture()?;
    let mut other = TranscriptView::default();
    other.sync_turn_details(&roots, true, Some(roots[1].as_ref()));
    let first = view
        .turn_row(&roots, 1)
        .ok_or_else(|| invalid("first actual row missing"))?;
    let second = other
        .turn_row(&roots, 1)
        .ok_or_else(|| invalid("second actual row missing"))?;
    assert!(!Arc::ptr_eq(group(&first)?, group(&second)?));
    let mut cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut graph = Capture::new(&mut cells, &mut layouts);
    graph.register(&roots)?;
    let first = RowData::capture(&first, &mut graph)?;
    let second = RowData::capture(&second, &mut graph)?;
    let table = graph.export();
    drop(graph);
    let decoded = decode(cells, layouts, table)?;
    let graph = decoded.graph()?;
    let first = first.restore(&graph)?;
    let second = second.restore(&graph)?;
    assert!(!Arc::ptr_eq(group(&first)?, group(&second)?));
    assert_eq!(group(&first)?.calls, group(&second)?.calls);
    Ok(())
}
#[test]
fn native_completed_turn_parent_rows_and_held_index_flags_survive_invalidation() -> io::Result<()> {
    let done = tool(DynamicToolCallStatus::Completed)?;
    let mut answer =
        AgentMarkdownCell::new("original answer".into(), std::path::Path::new("/original"));
    answer.phase = Some(MessagePhase::FinalAnswer);
    let mut exec = crate::exec_cell::new_active_exec_command(
        "actual-completed-exec".into(),
        vec!["echo".into(), "original".into()],
        Vec::new(),
        codex_app_server_protocol::CommandExecutionSource::Agent,
        None,
        false,
    );
    exec.complete_call(
        "actual-completed-exec",
        crate::exec_cell::CommandOutput::new(0, "original output".into()),
        std::time::Duration::ZERO,
    );
    let roots = vec![
        prompt(),
        Arc::new(done) as Arc<dyn HistoryCell>,
        Arc::new(exec),
        Arc::new(answer),
        Arc::new(FinalMessageSeparator::new(None, None)),
    ];
    let mut view = TranscriptView::default();
    view.sync_turn_details(&roots, true, None);
    let area = Rect::new(0, 0, 40, 8);
    view.render(area, &mut Buffer::empty(area), &roots);
    view.begin_selection(&roots, 0, 0, 1);
    view.invalidate_turn_details_from(0);
    view.sync_turn_details(&roots, true, None);
    view.invalidate_turn_details_from(0);
    let before = details(&view)?;
    assert!(before.indexed_snapshot);
    assert!(before.source.is_none());
    assert!(before.rows.iter().any(|entry| entry.parent.is_some()));
    assert!(
        before
            .rows
            .iter()
            .any(|entry| matches!(entry.row.as_ref(), Some(TurnRow::Prompt { .. })))
    );
    assert!(
        before
            .rows
            .iter()
            .any(|entry| matches!(entry.row.as_ref(),Some(TurnRow::Tools(group)) if group.ran==1))
    );
    let mut cells = cells::Capture::default();
    let refs = roots
        .iter()
        .map(|root| cells.intern(root))
        .collect::<io::Result<Vec<_>>>()?;
    let mut layouts = layouts::Capture::default();
    let mut graph = Capture::new(&mut cells, &mut layouts);
    graph.register(&roots)?;
    let data = Data::capture(before, &mut graph)?;
    let table = graph.export();
    drop(graph);
    let decoded = decode(cells, layouts, table)?;
    let graph = decoded.graph()?;
    let after = data.restore(&graph, &decoded.resources)?;
    let rebound = refs
        .into_iter()
        .map(|reference| decoded.cells.cell(reference))
        .collect::<io::Result<Vec<_>>>()?;
    exact(before, &after, &roots, &rebound)
}
#[test]
fn missing_option_fields_bad_row_indexes_and_foreign_shared_refs_refuse() -> io::Result<()> {
    let (view, roots, _) = fixture()?;
    let mut cells = cells::Capture::default();
    let refs = roots
        .iter()
        .map(|root| cells.intern(root))
        .collect::<io::Result<Vec<_>>>()?;
    let mut layouts = layouts::Capture::default();
    let mut graph = Capture::new(&mut cells, &mut layouts);
    graph.register(&roots)?;
    let pristine = Data::capture(&TurnDetails::default(), &mut graph)?;
    let wire =
        serde_json::to_value(Data::capture(details(&view)?, &mut graph)?).map_err(invalid)?;
    let mut missing_source = wire.clone();
    missing_source
        .as_object_mut()
        .ok_or_else(|| invalid("turn DATA missing"))?
        .remove("source");
    assert!(serde_json::from_value::<Data>(missing_source).is_err());
    for field in ["parent", "row"] {
        let mut bad = wire.clone();
        bad["rows"][0]
            .as_object_mut()
            .ok_or_else(|| invalid("row DATA missing"))?
            .remove(field);
        assert!(serde_json::from_value::<Data>(bad).is_err());
    }
    let mut missing = wire.clone();
    missing["source"]
        .as_object_mut()
        .ok_or_else(|| invalid("source DATA missing"))?
        .remove("last");
    assert!(serde_json::from_value::<Data>(missing).is_err());
    let table = graph.export();
    let mut missing_anchor = serde_json::to_value(&table).map_err(invalid)?;
    missing_anchor["groups"]["entries"][0]
        .as_object_mut()
        .ok_or_else(|| invalid("captured actual group missing"))?
        .remove("anchor");
    assert!(serde_json::from_value::<tables::Data>(missing_anchor).is_err());
    let mut extra = wire.clone();
    extra["rows"][0]["row"] = json!({"kind":"hidden","nativePermission":true});
    assert!(serde_json::from_value::<Data>(extra).is_err());
    let mut bad_counts = serde_json::to_value(&table).map_err(invalid)?;
    bad_counts["groups"]["entries"][0]["explored"] = json!(u64::MAX);
    bad_counts["groups"]["entries"][0]["ran"] = json!(1);
    let bad_counts: tables::Data = serde_json::from_value(bad_counts).map_err(invalid)?;
    drop(graph);
    let decoded = decode(cells, layouts, table)?;
    assert!(tables::Restore::new(bad_counts, &decoded.cells, &decoded.layouts).is_err());
    let graph = decoded.graph()?;
    let initial = pristine.restore(&graph, &decoded.resources)?;
    assert!(initial.rows.is_empty());
    let rebound = refs
        .into_iter()
        .map(|reference| decoded.cells.cell(reference))
        .collect::<io::Result<Vec<_>>>()?;
    let mut resumed = TranscriptView {
        turn_details: Some(initial),
        ..TranscriptView::default()
    };
    resumed.sync_turn_details(&rebound, true, None);
    assert_eq!(details(&resumed)?.rows.len(), rebound.len() + 1);
    for kind in 0..6 {
        let mut bad = wire.clone();
        match kind {
            0 => bad["start"] = json!(u64::MAX),
            1 => bad["rows"][0]["parent"] = json!(1_000_001),
            2 => bad["pending"][0][0] = json!(1_000_001),
            3 => bad["rows"][1]["row"]["group"] = json!(1_000_001),
            4 => bad["pending"][0][1]["state"] = json!(u32::MAX),
            _ => bad["start"] = json!(details(&view)?.rows.len()),
        }
        let bad: Data = serde_json::from_value(bad).map_err(invalid)?;
        assert!(bad.restore(&graph, &decoded.resources).is_err());
    }
    Ok(())
}

#[test]
fn native_work_disclosure_and_shared_activity_arrays_follow_new_cells_through_original_reindex()
-> io::Result<()> {
    use crate::transcript_view::alab_retained_view::disclosures;
    use crate::transcript_view::alab_retained_view::identity::CaptureRefs;
    let (mut view, originals, _) = fixture()?;
    let area = Rect::new(0, 0, 48, 10);
    view.render(area, &mut Buffer::empty(area), &originals);
    let (column, row) = view
        .visible
        .iter()
        .enumerate()
        .find_map(|(index, row)| {
            (row.index == 1 && row.layout.disclosure_row() == Some(row.row)).then(|| {
                (
                    view.area.x + row.layout.disclosure_columns().start,
                    view.area.y + u16::try_from(index).unwrap(),
                )
            })
        })
        .ok_or_else(|| invalid("actual native summary control not visible"))?;
    assert!(view.toggle_disclosure_at(&originals, column, row));
    let old_work = format!("work:{:?}", EntryKey::cell(&originals[1]));
    assert!(view.disclosure.expanded.contains(&old_work));
    let actual_ids = view.displayed_activity_ids(&originals, 1);
    assert!(actual_ids.contains(&old_work));
    let mut cells = cells::Capture::default();
    let roots = originals
        .iter()
        .map(|cell| cells.intern(cell))
        .collect::<io::Result<Vec<_>>>()?;
    let mut refs = CaptureRefs::new(&mut cells);
    refs.register(&originals)?;
    let disclosure = disclosures::Data::capture(&view, &refs)?;
    let disclosure = disclosures::Data::decode(
        &serde_json::value::to_raw_value(&disclosure).map_err(invalid)?,
        LIMIT,
    )?;
    drop(refs);
    let mut layouts = layouts::Capture::default();
    let mut graph = Capture::new(&mut cells, &mut layouts);
    graph.register(&originals)?;
    let turns = Data::capture(details(&view)?, &mut graph)?;
    let first_ids = graph.activity(&actual_ids)?;
    let same_ids = graph.activity(&actual_ids)?;
    let table = graph.export();
    drop(graph);
    let decoded = decode(cells, layouts, table)?;
    let graph = decoded.graph()?;
    let mut new_roots = roots
        .into_iter()
        .map(|reference| decoded.cells.cell(reference))
        .collect::<io::Result<Vec<_>>>()?;
    let mut restored = TranscriptView {
        turn_details: Some(turns.restore(&graph, &decoded.resources)?),
        ..TranscriptView::default()
    };
    disclosure.restore(&mut restored, &decoded.cells)?;
    let new_work = format!("work:{:?}", EntryKey::cell(&new_roots[1]));
    assert_ne!(new_work, old_work);
    assert!(restored.disclosure.expanded.contains(&new_work));
    assert!(!restored.disclosure.expanded.contains(&old_work));
    let first = graph.activity(first_ids)?;
    let same = graph.activity(same_ids)?;
    assert!(Arc::ptr_eq(&first, &same));
    assert!(first.contains(&new_work));
    assert!(!first.contains(&old_work));
    let before = restored
        .turn_row(&new_roots, 1)
        .ok_or_else(|| invalid("restored actual summary missing"))?;
    assert!(group(&before)?.ids.contains(&new_work));
    assert!(restored.disclosure.is_expanded(&group(&before)?.ids));
    new_roots.push(Arc::new(crate::history_cell::PlainHistoryCell::new(vec![
        Line::from("actual later output"),
    ])));
    restored.sync_turn_details(&new_roots, true, None);
    let after = restored
        .turn_row(&new_roots, 1)
        .ok_or_else(|| invalid("native reindexed summary missing"))?;
    assert!(group(&after)?.ids.contains(&new_work));
    assert!(restored.disclosure.is_expanded(&group(&after)?.ids));
    Ok(())
}

#[test]
fn refused_partial_native_capture_retries_without_stale_group_or_activity_ids() -> io::Result<()> {
    let roots: Vec<Arc<dyn HistoryCell>> = vec![
        prompt(),
        Arc::new(tool(DynamicToolCallStatus::Completed)?),
        Arc::new(tool(DynamicToolCallStatus::Completed)?),
    ];
    let mut view = TranscriptView::default();
    view.sync_turn_details(&roots, true, None);
    let old_group = view
        .turn_row(&roots, 1)
        .ok_or_else(|| invalid("actual group missing"))?;
    let old_id = format!("work:{:?}", EntryKey::cell(&roots[2]));
    assert!(group(&old_group)?.ids.contains(&old_id));
    let ids: Arc<[String]> = Arc::from(group(&old_group)?.ids.clone());
    let mut cells = cells::Capture::default();
    let root_ids = roots
        .iter()
        .map(|root| cells.intern(root))
        .collect::<io::Result<Vec<_>>>()?;
    let mut layouts = layouts::Capture::default();
    let mut capture = Capture::new(&mut cells, &mut layouts);
    capture.register(&roots[..2])?;
    let activity = capture.activity(&ids)?;
    assert!(Data::capture(details(&view)?, &mut capture).is_err());
    capture.register(&roots[2..])?;
    let retry = Data::capture(details(&view)?, &mut capture)?;
    // Do not recapture the activity array after registering the missing root.
    let table = capture.export();
    drop(capture);
    let decoded = decode(cells, layouts, table)?;
    let graph = decoded.graph()?;
    let restored = retry.restore(&graph, &decoded.resources)?;
    let new_roots = root_ids
        .into_iter()
        .map(|id| decoded.cells.cell(id))
        .collect::<io::Result<Vec<_>>>()?;
    let new_id = format!("work:{:?}", EntryKey::cell(&new_roots[2]));
    assert_ne!(old_id, new_id);
    let restored_group = group(
        restored.rows[1]
            .row
            .as_ref()
            .ok_or_else(|| invalid("restored group missing"))?,
    )?;
    assert!(restored_group.ids.contains(&new_id));
    assert!(!restored_group.ids.contains(&old_id));
    let restored_ids = graph.activity(activity)?;
    assert!(restored_ids.contains(&new_id));
    assert!(!restored_ids.contains(&old_id));
    exact(details(&view)?, &restored, &roots, &new_roots)
}
