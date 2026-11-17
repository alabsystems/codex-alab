// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native copy actions and shared local graph identities. No OS clipboard call.
use super::*;
use crate::app_event_sender::AppEventSender;
use crate::copy_input_guard::CopyInputGuard;
use crate::history_cell::alab_retained_cells as cells;
use crate::transcript_view::text::alab_retained_layout as layouts;
use std::path::Path;
const LIMIT: usize = 1_048_576;
fn original_cells() -> Vec<Arc<dyn HistoryCell>> {
    vec![Arc::new(AgentMarkdownCell::new(
        "original response\n\n```sh\necho source\n```".into(),
        Path::new("."),
    ))]
}
fn original_view(
    cells: &[Arc<dyn HistoryCell>],
    guard: Option<Arc<CopyInputGuard>>,
) -> TranscriptView {
    let mut view = TranscriptView::default();
    let area = Rect::new(0, 0, 40, 10);
    view.render(area, &mut Buffer::empty(area), cells);
    assert!(view.begin_copy_mode(cells, guard));
    view
}
fn prepared_guards(capture: &guards::Capture) -> io::Result<guards::Prepared> {
    guards::Prepared::decode(&capture.encode(LIMIT)?, LIMIT)
}

#[test]
fn actual_copy_mode_keeps_same_guard_and_text_aliases_but_equal_independent_text_stays_distinct()
-> io::Result<()> {
    let originals = original_cells();
    let (source, _source_events, source_census) = guards::channel();
    let source_app = AppEventSender::new(source);
    let guard = Arc::new(CopyInputGuard(source_app.clone()));
    let first = original_view(&originals, Some(guard.clone()));
    let separate = original_view(&originals, Some(guard));
    let one = first.copy_mode.as_ref().unwrap();
    let other = separate.copy_mode.as_ref().unwrap();
    assert!(!Arc::ptr_eq(
        &one.response.as_ref().unwrap().1,
        &other.response.as_ref().unwrap().1
    ));
    let mut cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut graph = Capture::new(&mut cells, &mut layouts);
    graph.register(&originals)?;
    let mut guard_capture = guards::Capture::for_app(&source_census, &source_app)?;
    let data = (
        Data::capture(one, &mut graph, &mut guard_capture)?,
        Data::capture(one, &mut graph, &mut guard_capture)?,
        Data::capture(other, &mut graph, &mut guard_capture)?,
    );
    let raw = serde_json::to_vec(&data).map_err(io::Error::other)?;
    let (a, b, c): (Data, Data, Data) = serde_json::from_slice(&raw).map_err(io::Error::other)?;
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
    let (target, mut target_events, target_census) = guards::channel();
    let target_app = AppEventSender::new(target);
    let mut pending = prepared_guards(&guard_capture)?;
    for reference in pending.app_sender_refs()? {
        pending.bind_retired(reference, &target_census, &target_app)?;
    }
    let guards = pending.restore()?;
    let a = a.restore(&graph, &guards)?;
    let b = b.restore(&graph, &guards)?;
    let c = c.restore(&graph, &guards)?;
    assert!(Arc::ptr_eq(
        a._guard.as_ref().unwrap(),
        b._guard.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        a._guard.as_ref().unwrap(),
        c._guard.as_ref().unwrap()
    ));
    let old = one.response.as_ref().unwrap();
    let response = a.response.as_ref().unwrap();
    assert_ne!(response.0, old.0);
    assert_eq!(response.1.as_ref(), old.1.as_ref());
    assert!(Arc::ptr_eq(&response.1, &b.response.as_ref().unwrap().1));
    assert!(!Arc::ptr_eq(&response.1, &c.response.as_ref().unwrap().1));
    assert!(matches!(&a.label, std::borrow::Cow::Owned(_)));
    assert_eq!(a.label, one.label);
    assert_eq!(a.whole, one.whole);
    assert_eq!(a.source, one.source);
    drop((a, b, c, guards));
    assert!(
        matches!(
            target_events.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "leaf restore armed guards before the actual App installation"
    );
    Ok(())
}

#[test]
fn actual_copy_navigation_keeps_original_block_source_and_historical_owned_label() -> io::Result<()>
{
    let originals = original_cells();
    let mut view = original_view(&originals, None);
    view.handle_copy_mode_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), &originals);
    let mode = view.copy_mode.as_ref().unwrap();
    assert!(!mode.whole);
    assert!(
        mode.source
            .as_ref()
            .is_some_and(|source| source.contains("echo source"))
    );
    let (sender, _events, census) = guards::channel();
    let app = AppEventSender::new(sender);
    let mut guard_capture = guards::Capture::for_app(&census, &app)?;
    let mut cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut graph = Capture::new(&mut cells, &mut layouts);
    graph.register(&originals)?;
    let data = Data::capture(mode, &mut graph, &mut guard_capture)?;
    let data: Data = serde_json::from_slice(&serde_json::to_vec(&data).map_err(io::Error::other)?)
        .map_err(io::Error::other)?;
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
    let guards = prepared_guards(&guard_capture)?.restore()?;
    let restored = data.restore(&graph, &guards)?;
    assert!(restored._guard.is_none());
    assert_eq!(restored.source, mode.source);
    assert_eq!(restored.label, mode.label);
    assert_eq!(restored.whole, mode.whole);
    Ok(())
}

#[test]
fn copy_mode_requires_observed_fields_and_real_cell_text_and_guard_references() -> io::Result<()> {
    let originals = original_cells();
    let view = original_view(&originals, None);
    let (sender, _events, census) = guards::channel();
    let app = AppEventSender::new(sender);
    let mut guard_capture = guards::Capture::for_app(&census, &app)?;
    let mut cells = cells::Capture::default();
    let mut layouts = layouts::Capture::default();
    let mut graph = Capture::new(&mut cells, &mut layouts);
    graph.register(&originals)?;
    let wire = serde_json::to_value(Data::capture(
        view.copy_mode.as_ref().unwrap(),
        &mut graph,
        &mut guard_capture,
    )?)
    .map_err(io::Error::other)?;
    for field in ["guard", "response", "source"] {
        let mut omitted = wire.clone();
        omitted.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<Data>(omitted).is_err(), "{field}");
    }
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
    let guards = prepared_guards(&guard_capture)?.restore()?;
    for field in ["text", "cell", "guard"] {
        let mut bad = wire.clone();
        match field {
            "text" => bad["response"][1] = serde_json::json!(1_000_001),
            "cell" => bad["response"][0] = serde_json::json!({"kind":"cell","original":1_000_001}),
            "guard" => bad["guard"] = serde_json::json!(1_000_001),
            _ => unreachable!(),
        }
        let data: Data = serde_json::from_value(bad).map_err(io::Error::other)?;
        assert!(data.restore(&graph, &guards).is_err(), "{field}");
    }
    Ok(())
}
