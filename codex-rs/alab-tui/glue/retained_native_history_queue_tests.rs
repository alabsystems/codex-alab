// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original queue consolidation/replay over real shared cells; no terminal IO.
use super::*;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::alab_retained_cells as cells;
use crate::transcript_view::alab_retained_view::layouts;
const LIMIT: usize = 1_048_576;
fn cell(text: &str) -> Arc<dyn HistoryCell> {
    Arc::new(PlainHistoryCell::new(vec![text.to_owned().into()]))
}
fn decoded(mut capture: Capture) -> io::Result<Restored> {
    let mut layouts = layouts::Capture::default();
    let bytes = capture.encode(layouts.lines(), LIMIT)?;
    let bytes_layout = layouts.encode(LIMIT)?;
    let mut layouts = layouts::Restored::decode(&bytes_layout, LIMIT)?;
    Restored::decode(
        &bytes,
        LIMIT,
        layouts.lines(),
        &mut cells::RestoreContext::default(),
    )
}
#[test]
fn original_partly_printed_consolidation_keeps_source_distinct_from_canonical_owner()
-> io::Result<()> {
    let printed = cell("already printed");
    let remaining = cell("unprinted remainder");
    let canonical = cell("canonical merged output");
    let mut queue = NativeHistory::default();
    queue.defer(&remaining);
    queue.consolidate(&[printed, remaining.clone()], &canonical);
    assert_eq!(queue.pending.len(), 1);
    assert!(Arc::ptr_eq(&queue.pending[0].source, &remaining));
    assert!(queue.pending[0].owner.ptr_eq(&Arc::downgrade(&canonical)));
    let mut graph = Capture::default();
    let owner = graph.intern(&canonical)?;
    Data::register_roots(&queue, &mut graph)?;
    let data = Data::capture(&queue, &mut graph)?;
    let raw = serde_json::to_vec(&data).map_err(io::Error::other)?;
    let data: Data = serde_json::from_slice(&raw).map_err(io::Error::other)?;
    let graph = decoded(graph)?;
    let canonical = graph.cell(owner)?;
    let mut restored = data.restore(&graph)?;
    drop(graph);
    assert_eq!(restored.pending.len(), 1);
    assert!(!Arc::ptr_eq(&restored.pending[0].source, &canonical));
    assert!(
        restored.pending[0]
            .owner
            .ptr_eq(&Arc::downgrade(&canonical))
    );
    restored.retain(std::slice::from_ref(&canonical));
    assert_eq!(restored.pending.len(), 1);
    drop(canonical);
    assert_eq!(restored.pending[0].owner.strong_count(), 0);
    restored.replayed();
    assert!(restored.pending.is_empty());
    Ok(())
}
#[test]
fn live_foreign_canonical_owner_refuses_and_actual_expired_owner_stays_dead_after_retry()
-> io::Result<()> {
    let printed = cell("old printed prefix");
    let source = cell("queued remainder");
    let owner = cell("canonical owner");
    let mut queue = NativeHistory::default();
    queue.defer(&source);
    queue.consolidate(&[printed, source], &owner);
    let mut graph = Capture::default();
    Data::register_roots(&queue, &mut graph)?;
    assert!(Data::capture(&queue, &mut graph).is_err());
    drop(owner);
    let data = Data::capture(&queue, &mut graph)?;
    let raw = serde_json::to_value(&data).map_err(io::Error::other)?;
    assert!(raw["pending"][0]["owner"].is_null());
    let mut omitted = raw;
    omitted["pending"][0]
        .as_object_mut()
        .ok_or_else(|| io::Error::other("native pending DATA absent"))?
        .remove("owner");
    assert!(serde_json::from_value::<Data>(omitted).is_err());
    let restored = data.restore(&decoded(graph)?)?;
    assert_eq!(restored.pending.len(), 1);
    assert_eq!(restored.pending[0].owner.strong_count(), 0);
    Ok(())
}
