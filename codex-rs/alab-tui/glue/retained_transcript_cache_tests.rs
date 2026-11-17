// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual native cache/revision ownership; no rendering or file loads on restore.
use super::*;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::alab_retained_cells as cells;
use crate::transcript_view::alab_retained_view::snapshots;
use crate::transcript_view::text::alab_retained_layout as layouts;
const LIMIT: usize = 1_048_576;

fn original(
    protocol: Option<crate::pets::ImageProtocol>,
) -> (TranscriptView, Vec<Arc<dyn HistoryCell>>) {
    let originals: Vec<Arc<dyn HistoryCell>> = vec![
        Arc::new(PlainHistoryCell::new(vec![Line::from("original first α")])),
        Arc::new(PlainHistoryCell::new(vec![Line::from("original second β")])),
    ];
    let mut view = TranscriptView {
        image_protocol: protocol,
        ..TranscriptView::default()
    };
    let area = Rect::new(0, 0, 32, 9);
    view.render(area, &mut Buffer::empty(area), &originals);
    assert_eq!(view.cache.entries.len(), 2);
    (view, originals)
}

#[test]
fn actual_native_cache_keeps_lru_metadata_and_snapshot_layout_aliases_without_rerender()
-> io::Result<()> {
    for protocol in [
        None,
        Some(crate::pets::ImageProtocol::Kitty),
        Some(crate::pets::ImageProtocol::KittyLocalFile),
        Some(crate::pets::ImageProtocol::Sixel),
    ] {
        // These native cells have no image path, so protocol is DATA only.
        let (mut view, originals) = original(protocol);
        let snapshot = view.capture_snapshot(&originals);
        let mut cells = cells::Capture::default();
        let roots = originals
            .iter()
            .map(|cell| cells.intern(cell))
            .collect::<io::Result<Vec<_>>>()?;
        let mut layouts = layouts::Capture::default();
        let mut graph = Capture::new(&mut cells, &mut layouts);
        graph.register(&originals)?;
        let held = snapshots::Data::capture(&snapshot, &mut graph)?;
        let cache = Data::capture(&view.cache, &mut graph)?;
        let bytes = serde_json::to_vec(&(held, cache)).map_err(io::Error::other)?;
        let (held, cache): (snapshots::Data, Data) =
            serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        let table = graph.export();
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
        let roots = roots
            .into_iter()
            .map(|reference| cells.cell(reference))
            .collect::<io::Result<Vec<_>>>()?;
        let graph = Restore::new(table, &cells, &layouts)?;
        let wire = serde_json::to_value(&cache).map_err(io::Error::other)?;
        let mut duplicate = wire.clone();
        duplicate["entries"][1]["source"] = duplicate["entries"][0]["source"].clone();
        let duplicate: Data = serde_json::from_value(duplicate).map_err(io::Error::other)?;
        assert!(duplicate.restore(&graph).is_err());
        let mut oversized = wire;
        let mut expired = oversized["entries"][0].clone();
        expired["source"] = serde_json::Value::Null;
        oversized["entries"] = serde_json::Value::Array(vec![expired; MAX_CACHED_ENTRIES + 1]);
        let oversized: Data = serde_json::from_value(oversized).map_err(io::Error::other)?;
        assert!(oversized.restore(&graph).is_err());
        let held = held.restore(&graph)?;
        let mut cache = cache.restore(&graph)?;
        assert_eq!(cache.frame, view.cache.frame);
        assert!(cache.render_state == view.cache.render_state);
        assert_eq!(cache.entries.len(), view.cache.entries.len());
        for (before, after) in view.cache.entries.iter().zip(&cache.entries) {
            let old = before.source.upgrade().unwrap();
            let index = originals
                .iter()
                .position(|cell| Arc::ptr_eq(&old, cell))
                .unwrap();
            let new = after.source.upgrade().unwrap();
            assert!(Arc::ptr_eq(&new, &roots[index]));
            assert!(!Arc::ptr_eq(&new, &old));
            assert_eq!(after.width, before.width);
            assert!(after.presentation == before.presentation);
            assert_eq!(after.rendered_frame, before.rendered_frame);
            assert_eq!(after.animation_tick, before.animation_tick);
            assert_eq!(after.layout.text(), before.layout.text());
            assert!(Arc::ptr_eq(
                &after.layout,
                &held.pinned[&EntryKey::cell(&new)]
            ));
        }
        let entry = &cache.entries[0];
        let source = entry.source.upgrade().unwrap();
        let (width, presentation, expected) =
            (entry.width, entry.presentation, entry.layout.clone());
        let reused = cache.get(&source, width, presentation, || {
            panic!("restored native cache rerendered its retained entry")
        });
        assert!(Arc::ptr_eq(&reused, &expected));
    }
    Ok(())
}

#[test]
fn actual_expired_cache_sources_stay_expired_and_unregistered_live_sources_do_not_gain_roots()
-> io::Result<()> {
    let (view, originals) = original(None);
    let mut cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut graph = Capture::new(&mut cells, &mut layouts);
    assert!(Data::capture(&view.cache, &mut graph).is_err());
    drop(originals);
    assert!(
        view.cache
            .entries
            .iter()
            .all(|entry| entry.source.upgrade().is_none())
    );
    let data = Data::capture(&view.cache, &mut graph)?;
    let wire = serde_json::to_value(&data).map_err(io::Error::other)?;
    let mut omitted = wire.clone();
    omitted["entries"][0]
        .as_object_mut()
        .unwrap()
        .remove("source");
    assert!(serde_json::from_value::<Data>(omitted).is_err());
    let mut missing = wire;
    missing["entries"][0]["source"] = serde_json::json!(1_000_001);
    let missing: Data = serde_json::from_value(missing).map_err(io::Error::other)?;
    let table = graph.export();
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
    let graph = Restore::new(table, &cells, &layouts)?;
    assert!(missing.restore(&graph).is_err());
    let cache = data.restore(&graph)?;
    assert_eq!(cache.entries.len(), 2);
    assert!(
        cache
            .entries
            .iter()
            .all(|entry| entry.source.upgrade().is_none())
    );
    for (old, new) in view.cache.entries.iter().zip(&cache.entries) {
        assert_eq!(old.layout.text(), new.layout.text());
    }
    Ok(())
}
