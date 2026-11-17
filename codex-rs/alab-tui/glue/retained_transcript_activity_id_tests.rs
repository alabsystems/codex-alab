// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Only actual registered native work IDs rebind; opaque backend IDs stay opaque.
use super::*;
use crate::history_cell::UserHistoryCell;
use crate::history_cell::UserMessagePresentation;
#[test]
fn native_work_ids_use_actual_new_cell_without_encoding_old_pointer_or_guessing_foreign_roots()
-> io::Result<()> {
    let cell: Arc<dyn HistoryCell> = Arc::new(UserHistoryCell {
        message: "actual original".into(),
        text_elements: vec![],
        local_image_paths: vec![],
        remote_image_urls: vec![],
        presentation: UserMessagePresentation::Typed,
    });
    let old = format!("work:{:?}", EntryKey::cell(&cell));
    let turn = format!("turn:{:?}", EntryKey::cell(&cell));
    let opaque = format!("mcp:{old}");
    let unknown = "work:Cell(0)";
    let mut cells = Capture::default();
    let original = cells.intern(&cell)?;
    let mut refs = CaptureRefs::new(&mut cells);
    refs.register(std::slice::from_ref(&cell))?;
    let data = (
        refs.activity(&old),
        refs.activity(&turn),
        refs.activity(&opaque),
        refs.activity(unknown),
    );
    let wire = serde_json::to_string(&data).map_err(invalid)?;
    // The opaque MCP identifier intentionally contains the old text; the real
    // work-cell variant contains only its bounded graph reference.
    let first = serde_json::to_value(&data.0).map_err(invalid)?;
    assert_eq!(first["kind"], "workCell");
    assert!(!first.to_string().contains(&old));
    let (work, turn_data, opaque_data, unknown_data): (
        ActivityIdData,
        ActivityIdData,
        ActivityIdData,
        ActivityIdData,
    ) = serde_json::from_str(&wire).map_err(invalid)?;
    drop(refs);
    let restored = super::super::restore_test_cells(&mut cells, 1_048_576)?;
    let expected = format!("work:{:?}", EntryKey::cell(&restored.cell(original)?));
    assert_ne!(expected, old);
    assert_eq!(work.restore(&restored)?, expected);
    assert_eq!(
        turn_data.restore(&restored)?,
        format!("turn:{:?}", EntryKey::cell(&restored.cell(original)?))
    );
    assert_eq!(opaque_data.restore(&restored)?, opaque);
    assert_eq!(unknown_data.restore(&restored)?, unknown);
    let foreign: ActivityIdData =
        serde_json::from_value(serde_json::json!({"kind":"workCell","original":1_000_001}))
            .map_err(invalid)?;
    assert!(foreign.restore(&restored).is_err());
    assert!(
        serde_json::from_value::<ActivityIdData>(
            serde_json::json!({"kind":"workCell","original":0,"nativePermission":true})
        )
        .is_err()
    );
    Ok(())
}
