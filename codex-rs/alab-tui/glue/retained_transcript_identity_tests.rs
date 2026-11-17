// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual native cells/anchors only. No Native/controller/backend capability.
use super::*;
use crate::history_cell::UserHistoryCell;
use crate::history_cell::UserMessagePresentation;
const LIMIT: usize = 1_048_576;
fn user() -> Arc<dyn HistoryCell> {
    Arc::new(UserHistoryCell {
        message: "actual original user text".into(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: Vec::new(),
        presentation: UserMessagePresentation::Typed,
    })
}
#[test]
fn actual_anchor_rebinds_to_restored_cell_without_serializing_original_pointer() -> io::Result<()> {
    let cell = user();
    let mut graph = Capture::default();
    let mut refs = CaptureRefs::new(&mut graph);
    refs.register(std::slice::from_ref(&cell))?;
    let saved = refs.anchor(Anchor {
        key: EntryKey::cell(&cell),
        index: 7,
        offset: 13,
        row_bias: -2,
    })?;
    let KeyData::Cell { original } = saved.key else {
        unreachable!("actual registered cell")
    };
    let encoded = serde_json::to_value(saved).map_err(invalid)?;
    assert_eq!(
        encoded["key"],
        serde_json::json!({"kind":"cell","original":0})
    );
    let decoded: AnchorData = serde_json::from_value(encoded).map_err(invalid)?;
    drop(refs);
    let restored = super::super::restore_test_cells(&mut graph, LIMIT)?;
    let anchor = decoded.restore(&restored)?;
    assert_eq!(anchor.key, EntryKey::cell(&restored.cell(original)?));
    assert_ne!(anchor.key, EntryKey::cell(&cell));
    assert_eq!((anchor.index, anchor.offset, anchor.row_bias), (7, 13, -2));
    Ok(())
}
#[test]
fn equal_native_text_cannot_substitute_for_an_unregistered_actual_cell() -> io::Result<()> {
    let first = user();
    let other = user();
    let mut graph = Capture::default();
    let mut refs = CaptureRefs::new(&mut graph);
    refs.register(std::slice::from_ref(&first))?;
    assert!(refs.key(EntryKey::cell(&other)).is_err());
    assert!(refs.key(EntryKey::cell(&first)).is_ok());
    Ok(())
}
#[test]
fn original_reading_anchor_follows_the_same_cell_after_actual_history_is_prepended()
-> io::Result<()> {
    let a = user();
    let b = user();
    let older = user();
    let mut graph = Capture::default();
    let mut refs = CaptureRefs::new(&mut graph);
    refs.register(&[a.clone(), b.clone(), older.clone()])?;
    let a_key = refs.key(EntryKey::cell(&a))?;
    let b_key = refs.key(EntryKey::cell(&b))?;
    let older_key = refs.key(EntryKey::cell(&older))?;
    let saved = refs.position(Position::Reading(Anchor {
        key: EntryKey::cell(&a),
        index: 0,
        offset: 5,
        row_bias: 0,
    }))?;
    drop(refs);
    let restored = super::super::restore_test_cells(&mut graph, LIMIT)?;
    let native = |key| match key {
        KeyData::Cell { original } => restored.cell(original),
        KeyData::Live {} => Err(invalid("expected original cell")),
    };
    let cells = [native(older_key)?, native(a_key)?, native(b_key)?];
    let Position::Reading(anchor) = saved.restore(&restored)? else {
        unreachable!("original reading position")
    };
    assert_eq!(TranscriptView::default().resolve(&cells, anchor), 1);
    assert_eq!(anchor.offset, 5);
    Ok(())
}
#[test]
fn foreign_or_missing_key_fields_do_not_form_a_native_anchor() -> io::Result<()> {
    assert!(
        serde_json::from_str::<KeyData>(r#"{"kind":"cell","original":0,"pointer":9}"#).is_err()
    );
    assert!(serde_json::from_str::<KeyData>(r#"{"kind":"cell"}"#).is_err());
    let key: KeyData = serde_json::from_str(r#"{"kind":"cell","original":0}"#).map_err(invalid)?;
    let empty = super::super::restore_test_cells(&mut Capture::default(), LIMIT)?;
    assert!(key.restore(&empty).is_err());
    Ok(())
}
