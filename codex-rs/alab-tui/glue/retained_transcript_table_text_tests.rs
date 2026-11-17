// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact actual text Arcs share the enclosing native layout/line table.
use super::*;
#[test]
fn actual_text_aliases_share_one_native_table_and_equal_distinct_text_does_not_merge()
-> io::Result<()> {
    const LIMIT: usize = 1_048_576;
    let first: Arc<str> = String::from("actual shared response α").into();
    let equal: Arc<str> = String::from("actual shared response α").into();
    let weak = Arc::downgrade(&first);
    let mut cells = Cells::default();
    let mut layouts = Layouts::default();
    let mut capture = Capture::new(&mut cells, &mut layouts);
    let a = capture.text(&first)?;
    let repeated = capture.text(&first)?;
    let b = capture.text(&equal)?;
    drop(first);
    assert!(weak.upgrade().is_some());
    let data = capture.export();
    drop(capture);
    let cell_bytes = cells.encode(layouts.lines(), LIMIT)?;
    let layout_bytes = layouts.encode(LIMIT)?;
    let mut layouts = RestoredLayouts::decode(&layout_bytes, LIMIT)?;
    let cells = RestoredCells::decode(
        &cell_bytes,
        LIMIT,
        layouts.lines(),
        &mut crate::history_cell::alab_retained_cells::RestoreContext::default(),
    )?;
    let restored = Restore::new(data, &cells, &layouts)?;
    let a = restored.text(a)?;
    let repeated = restored.text(repeated)?;
    let b = restored.text(b)?;
    assert!(Arc::ptr_eq(&a, &repeated));
    assert!(!Arc::ptr_eq(&a, &b));
    assert_eq!(&*a, &*b);
    let bad: TextRef =
        serde_json::from_value(serde_json::json!(1_000_001)).map_err(io::Error::other)?;
    assert!(restored.text(bad).is_err());
    Ok(())
}
