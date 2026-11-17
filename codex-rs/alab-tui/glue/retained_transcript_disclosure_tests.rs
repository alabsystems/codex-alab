// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original native disclosure methods and actual cell graph; no endpoint grants.
use super::*;
use crate::history_cell::UserHistoryCell;
use crate::history_cell::UserMessagePresentation;
use crate::history_cell::alab_retained_cells::Capture;
const LIMIT: usize = 1_048_576;
fn user() -> Arc<dyn HistoryCell> {
    Arc::new(UserHistoryCell {
        message: "original prompt".into(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: Vec::new(),
        presentation: UserMessagePresentation::Typed,
    })
}
#[test]
fn disclosure_restores_original_focus_and_control_to_same_decoded_native_cell() -> io::Result<()> {
    let cell = user();
    let mut graph = Capture::default();
    let mut refs = CaptureRefs::new(&mut graph);
    refs.register(std::slice::from_ref(&cell))?;
    let mut actual = TranscriptView::default();
    actual
        .disclosure
        .expanded
        .extend(["tool-b".into(), "tool-a".into()]);
    actual.disclosure.focused = Some((
        EntryKey::cell(&cell),
        vec!["tool-b".into(), "tool-a".into()],
    ));
    actual.disclosure.anchor_control = Some(EntryKey::cell(&cell));
    actual.disclosure.live_ids = vec!["live-2".into(), "live-1".into()];
    let raw = serde_json::value::to_raw_value(&Data::capture(&actual, &refs)?).map_err(invalid)?;
    drop(refs);
    let graph = super::super::restore_test_cells(&mut graph, LIMIT)?;
    let data = Data::decode(&raw, LIMIT)?;
    let mut destination = TranscriptView::default();
    data.restore(&mut destination, &graph)?;
    assert!(destination.disclosure.is_expanded(&["tool-a".into()]));
    assert!(!destination.disclosure.is_expanded(&["missing".into()]));
    let (focus, ids) = destination.disclosure.focused.as_ref().unwrap();
    assert_ne!(*focus, EntryKey::cell(&cell));
    assert_eq!(Some(*focus), destination.disclosure.anchor_control);
    assert_eq!(ids, &["tool-b", "tool-a"]);
    assert_eq!(destination.disclosure.live_ids, ["live-2", "live-1"]);
    Ok(())
}
#[test]
fn missing_disclosure_cell_refuses_before_mutating_destination() -> io::Result<()> {
    let mut graph = Capture::default();
    let restored = super::super::restore_test_cells(&mut graph, LIMIT)?;
    let raw = RawValue::from_string(serde_json::json!({"schema":2,"expanded":[{"kind":"literal","value":"incoming"}],"focused":null,"resume_after_approval":null,"anchor_control":{"kind":"cell","original":99},"live_ids":[]}).to_string()).map_err(invalid)?;
    let mut actual = TranscriptView::default();
    actual.disclosure.expanded.insert("original".into());
    assert!(
        Data::decode(&raw, LIMIT)?
            .restore(&mut actual, &restored)
            .is_err()
    );
    assert!(actual.disclosure.is_expanded(&["original".into()]));
    assert!(!actual.disclosure.is_expanded(&["incoming".into()]));
    Ok(())
}
#[test]
fn disclosure_rejects_omitted_option_fields_foreign_fields_and_duplicate_expansions()
-> io::Result<()> {
    let mut graph = Capture::default();
    let refs = CaptureRefs::new(&mut graph);
    let value =
        serde_json::to_value(Data::capture(&TranscriptView::default(), &refs)?).map_err(invalid)?;
    let decode = |value| -> io::Result<Data> {
        Data::decode(
            &serde_json::value::to_raw_value(&value).map_err(invalid)?,
            LIMIT,
        )
    };
    assert!(decode(value.clone()).is_ok());
    for key in [
        "focused",
        "resume_after_approval",
        "anchor_control",
        "live_ids",
        "schema",
        "expanded",
    ] {
        let mut bad = value.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(decode(bad).is_err(), "{key}");
    }
    let mut bad = value.clone();
    bad["expanded"] =
        serde_json::json!([{"kind":"literal","value":"same"}, {"kind":"literal","value":"same"}]);
    assert!(decode(bad).is_err());
    let mut bad = value.clone();
    bad["expanded"] =
        serde_json::json!([{"kind":"literal","value":"z"}, {"kind":"literal","value":"a"}]);
    assert!(decode(bad).is_err());
    let mut bad = value;
    bad["grant"] = true.into();
    assert!(decode(bad).is_err());
    Ok(())
}
