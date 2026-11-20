// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual native actions/capture and one enclosing editor encode. No restored
//! App, clipboard resource, Native permission or first-paint claim is fabricated.
use super::*;
use crate::app_event_sender::AppEventSender;
use crate::copy_input_guard::CopyInputGuard;
use crate::history_cell::AgentMarkdownCell;
use crate::history_cell::alab_retained_cells as cells;
use crate::transcript_view::text::alab_retained_layout as layouts;
use std::path::Path;
const LIMIT: usize = 1_048_576;
fn actual_view(
    roots: &[Arc<dyn HistoryCell>],
    guard: Arc<CopyInputGuard>,
) -> io::Result<TranscriptView> {
    let mut view = TranscriptView::default();
    let area = Rect::new(1, 2, 42, 8);
    view.render(area, &mut Buffer::empty(area), roots);
    if !view.begin_copy_mode(roots, Some(guard)) {
        return Err(invalid("original native copy control did not open"));
    }
    Ok(view)
}
#[test]
fn actual_native_view_capture_keeps_shared_rows_and_independent_equal_layouts_in_one_graph()
-> io::Result<()> {
    let roots: Vec<Arc<dyn HistoryCell>> = vec![Arc::new(AgentMarkdownCell::new(
        "actual response\n\n```sh\necho original\n```".into(),
        Path::new("."),
    ))];
    let (sender, _events, census) = guards::channel();
    let sender = AppEventSender::new(sender);
    let guard = Arc::new(CopyInputGuard(sender.clone()));
    let first = actual_view(&roots, guard)?;
    let other = actual_view(&roots, Arc::new(CopyInputGuard(sender.clone())))?;
    let mut cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut graph = tables::Capture::new(&mut cells, &mut layouts);
    register_roots(&first, &roots, &mut graph)?;
    register_roots(&other, &roots, &mut graph)?;
    let mut scopes = ScopeCapture::default();
    let mut guards = guards::Capture::for_app(&census, &sender)?;
    let a = Data::capture(&first, &mut graph, &mut scopes, &mut guards)?;
    let b = Data::capture(&first, &mut graph, &mut scopes, &mut guards)?;
    let c = Data::capture(&other, &mut graph, &mut scopes, &mut guards)?;
    let bytes = crate::bottom_pane::alab_retained_editors::encode(&(a, b, c), LIMIT)?;
    let (a, b, c): (Data, Data, Data) =
        crate::bottom_pane::alab_retained_editors::decode(&bytes, LIMIT, 128)?;
    assert!(!a.core.visible.is_empty());
    assert_eq!(a.core.visible.len(), first.visible.len());
    assert_eq!(
        serde_json::to_value(&a.core).map_err(invalid)?,
        serde_json::to_value(&b.core).map_err(invalid)?
    );
    assert_eq!(a.core.visible[0].layout, b.core.visible[0].layout);
    assert_ne!(a.core.visible[0].layout, c.core.visible[0].layout);
    assert_eq!(a.core.area.0, [1, 2, 42, 8]);
    assert!(a.core.copy_mode.is_some());
    assert!(a.core.selection.is_some());
    assert_eq!(
        a.core.mouse_scroll_speed_bits,
        first.mouse_scroll_speed.to_bits()
    );
    Ok(())
}
#[test]
fn original_native_whole_view_decoder_refuses_omitted_fields_and_noncanonical_disclosure()
-> io::Result<()> {
    let roots: Vec<Arc<dyn HistoryCell>> = vec![Arc::new(AgentMarkdownCell::new(
        "actual source".into(),
        Path::new("."),
    ))];
    let (sender, _events, census) = guards::channel();
    let sender = AppEventSender::new(sender);
    let first = actual_view(&roots, Arc::new(CopyInputGuard(sender.clone())))?;
    let mut cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut graph = tables::Capture::new(&mut cells, &mut layouts);
    register_roots(&first, &roots, &mut graph)?;
    let mut scopes = ScopeCapture::default();
    let mut guards = guards::Capture::for_app(&census, &sender)?;
    let data = Data::capture(&first, &mut graph, &mut scopes, &mut guards)?;
    let bytes = crate::bottom_pane::alab_retained_editors::encode(&data, LIMIT)?;
    let wire: serde_json::Value = serde_json::from_slice(&bytes).map_err(invalid)?;
    for field in [
        "selection",
        "copy_mode",
        "live",
        "last_click",
        "saved_position",
        "turn_details",
        "copy_feedback",
    ] {
        let mut bad = wire.clone();
        bad["body"]["core"]
            .as_object_mut()
            .ok_or_else(|| invalid("original core absent"))?
            .remove(field);
        assert!(
            crate::bottom_pane::alab_retained_editors::decode::<Data>(
                &serde_json::to_vec(&bad).map_err(invalid)?,
                LIMIT,
                128
            )
            .is_err(),
            "{field}"
        );
    }
    let mut bad = wire;
    bad["body"]["core"]["disclosure"]
        .as_object_mut()
        .ok_or_else(|| invalid("native disclosure absent"))?
        .remove("focused");
    assert!(
        crate::bottom_pane::alab_retained_editors::decode::<Data>(
            &serde_json::to_vec(&bad).map_err(invalid)?,
            LIMIT,
            128
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn selection_preparation_refusal_returns_same_actual_search_editor_buffer() -> io::Result<()> {
    use crate::history_cell::UserHistoryCell;
    use crate::history_cell::UserMessagePresentation;
    let roots: Vec<Arc<dyn HistoryCell>> = vec![Arc::new(UserHistoryCell {
        message: "native original selection".into(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: Vec::new(),
        presentation: UserMessagePresentation::Typed,
    })];
    let (sender, _events, census) = guards::channel();
    let sender = AppEventSender::new(sender);
    let mut original = TranscriptView::default();
    let area = Rect::new(0, 0, 40, 8);
    original.render(area, &mut Buffer::empty(area), &roots);
    if !original.begin_select_mode(&roots) {
        return Err(invalid("original native keyboard selection did not open"));
    }
    original.begin_search();
    if !original.paste_search("native retained editor") {
        return Err(invalid(
            "original native search editor did not accept input",
        ));
    }
    let mut cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut graph = tables::Capture::new(&mut cells, &mut layouts);
    register_roots(&original, &roots, &mut graph)?;
    let mut scopes = ScopeCapture::default();
    let mut guards = guards::Capture::for_app(&census, &sender)?;
    let data = Data::capture(&original, &mut graph, &mut scopes, &mut guards)?;
    let bytes = crate::bottom_pane::alab_retained_editors::encode(&data, LIMIT)?;
    let mut wire: serde_json::Value = serde_json::from_slice(&bytes).map_err(invalid)?;
    if wire["body"]["core"]["selection"].is_null() {
        return Err(invalid("actual native selection absent"));
    }
    wire["body"]["core"]["selection"]["schema"] = serde_json::json!(0);
    let data: Data = crate::bottom_pane::alab_retained_editors::decode(
        &serde_json::to_vec(&wire).map_err(invalid)?,
        LIMIT,
        128,
    )?;
    let tables = graph.export();
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
    let prepared = data
        .prepare_lines(&mut layouts)
        .map_err(|refused| refused.error)?;
    let graph = tables::Restore::new(tables, &cells, &layouts)?;
    let scopes = ScopeRestored::new(scopes.export()?)?;
    // Real native editor owns a nonempty allocation, so cloning/reconstruction
    // could not silently satisfy this actual allocation-identity observation.
    let buffer = prepared
        .search
        .editor_for_retained_control()
        .text()
        .as_ptr();
    let text = prepared
        .search
        .editor_for_retained_control()
        .text()
        .to_owned();
    assert!(!text.is_empty());
    let cursor = prepared.search.editor_for_retained_control().cursor();
    let refused = match prepared.prepare_owners(&graph, &scopes) {
        Err(refused) => refused,
        Ok(_) => return Err(invalid("malformed original Selection was accepted")),
    };
    assert_eq!(
        refused
            .prepared
            .search
            .editor_for_retained_control()
            .text()
            .as_ptr(),
        buffer
    );
    assert_eq!(
        refused.prepared.search.editor_for_retained_control().text(),
        text
    );
    assert_eq!(
        refused
            .prepared
            .search
            .editor_for_retained_control()
            .cursor(),
        cursor
    );
    Ok(())
}

#[test]
fn pure_composer_line_refusal_keeps_original_decoded_editor_then_same_input_retries()
-> io::Result<()> {
    use crate::history_cell::UserHistoryCell;
    use crate::history_cell::UserMessagePresentation;
    let roots: Vec<Arc<dyn HistoryCell>> = vec![Arc::new(UserHistoryCell {
        message: "original user text for rich tip".into(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: Vec::new(),
        presentation: UserMessagePresentation::Typed,
    })];
    let mut original = TranscriptView::default();
    let area = Rect::new(0, 0, 42, 8);
    original.render(area, &mut Buffer::empty(area), &roots);
    if !original.begin_select_mode(&roots) {
        return Err(invalid("actual keyboard selection did not open"));
    }
    original.begin_search();
    if !original.paste_search("same retained editor") {
        return Err(invalid("actual native editor rejected input"));
    }
    let tip = HyperlinkLine::from("actual original rich tip");
    original.composer_tip = Some((area, tip.clone()));
    let (sender, _events, census) = guards::channel();
    let sender = AppEventSender::new(sender);
    let mut guards = guards::Capture::for_app(&census, &sender)?;
    let mut cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut graph = tables::Capture::new(&mut cells, &mut layouts);
    register_roots(&original, &roots, &mut graph)?;
    let good_tip = graph.line(&tip)?;
    let mut scopes = ScopeCapture::default();
    let data = Data::capture(&original, &mut graph, &mut scopes, &mut guards)?;
    let bytes = crate::bottom_pane::alab_retained_editors::encode(&data, LIMIT)?;
    let mut wire: serde_json::Value = serde_json::from_slice(&bytes).map_err(invalid)?;
    wire["body"]["composer_tip"][1]["line"]["alignment"] = serde_json::json!(9);
    let data: Data = crate::bottom_pane::alab_retained_editors::decode(
        &serde_json::to_vec(&wire).map_err(invalid)?,
        LIMIT,
        128,
    )?;
    drop(graph);
    let bytes = layouts.encode(LIMIT)?;
    let mut layouts = layouts::Restored::decode(&bytes, LIMIT)?;
    let text = data.search.editor_for_retained_control().text().to_owned();
    let cursor = data.search.editor_for_retained_control().cursor();
    let buffer = data.search.editor_for_retained_control().text().as_ptr();
    assert!(!text.is_empty());
    let mut refused = match data.prepare_lines(&mut layouts) {
        Err(refused) => refused,
        Ok(_) => return Err(invalid("invalid retained rich-line alignment was accepted")),
    };
    assert_eq!(
        refused
            .data
            .search
            .editor_for_retained_control()
            .text()
            .as_ptr(),
        buffer
    );
    assert_eq!(
        refused.data.search.editor_for_retained_control().text(),
        text
    );
    assert_eq!(
        refused.data.search.editor_for_retained_control().cursor(),
        cursor
    );
    // Replace only malformed inert rich-line DATA; actual editor is never
    // encoded/decoded/cloned again, nor is any renderer/worker grant created.
    refused.data.composer_tip = Some((RectData::capture(area), good_tip));
    let prepared = refused
        .data
        .prepare_lines(&mut layouts)
        .map_err(|refused| refused.error)?;
    assert_eq!(
        prepared
            .search
            .editor_for_retained_control()
            .text()
            .as_ptr(),
        buffer
    );
    assert_eq!(prepared.search.editor_for_retained_control().text(), text);
    assert_eq!(
        prepared.search.editor_for_retained_control().cursor(),
        cursor
    );
    Ok(())
}
