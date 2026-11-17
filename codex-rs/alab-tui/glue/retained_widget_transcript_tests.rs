// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native field/row/cache controls only. No Entry, Native, renderer or worker grant is faked.
use super::*;
use codex_app_server_protocol::DynamicToolCallStatus;
use codex_app_server_protocol::ThreadItemPayload;
use serde_json::json;
const LIMIT: usize = 8 * 1024 * 1024;
fn check(value: bool, message: &str) -> io::Result<()> {
    if value {
        Ok(())
    } else {
        Err(native::invalid(message))
    }
}
fn item(id: &str, status: DynamicToolCallStatus) -> ThreadItemPayload {
    ThreadItemPayload::DynamicToolCall {
        id: id.into(),
        namespace: Some("original".into()),
        tool: "native-data".into(),
        arguments: json!({"original": [1,2]}),
        status,
        content_items: None,
        success: None,
        duration_ms: Some(17),
    }
}
fn dynamic(id: &str) -> io::Result<crate::history_cell::DynamicToolCallCell> {
    crate::history_cell::DynamicToolCallCell::from_item(item(id, DynamicToolCallStatus::InProgress))
        .ok_or_else(|| native::invalid("original dynamic constructor refused fixture"))
}
fn state() -> TranscriptState {
    TranscriptState {
        active_cell: Some(Box::new(crate::history_cell::PlainHistoryCell::new(vec![
            ratatui::text::Line::from("same actual native cell"),
        ]))),
        active_cell_revision: 97,
        last_agent_markdown: Some("display markdown".into()),
        last_agent_source: Some("raw original **source**".into()),
        last_completed_agent_message: Some(("complete-turn".into(), "complete-item".into())),
        replayed_cloud_text: Some(HashMap::from([
            ("older".into(), HashSet::from(["one".into(), "two".into()])),
            ("empty".into(), HashSet::new()),
        ])),
        recording_finished_cloud_text: true,
        active_streamed_agent_message: Some(("active-turn".into(), "active-item".into())),
        last_flushed_agent_message: Some(("flushed-turn".into(), "flushed-item".into())),
        latest_proposed_plan_markdown: Some("original proposed plan".into()),
        saw_copy_source_this_turn: true,
        saw_plan_update_this_turn: true,
        saw_plan_item_this_turn: true,
        last_plan_progress: Some((2, 7)),
        plan_delta_buffer: "unfinished plan **".into(),
        plan_item_active: true,
        ..TranscriptState::default()
    }
}
fn roundtrip(
    before: &TranscriptState,
    roots: &[Arc<dyn HistoryCell>],
) -> io::Result<(
    Prepared,
    cells::Restored,
    native::Restored,
    cells::RestoreContext,
)> {
    let mut graph = cells::Capture::default();
    for root in roots {
        graph.intern(root)?;
    }
    let mut text = native::Capture::default();
    let captured = Data::capture_state(before, &mut graph, &mut text)?;
    let raw = serde_json::value::to_raw_value(&captured).map_err(native::invalid)?;
    let data = Data::decode(&raw, LIMIT)?;
    let cells_raw = graph.encode(&mut text, LIMIT)?;
    let mut text = native::Restored::new(text.export()?)?;
    let mut context = cells::RestoreContext::default();
    let graph = cells::Restored::decode(&cells_raw, LIMIT, &mut text, &mut context)?;
    let prepared = data.prepare(
        &graph,
        &mut text,
        &mut context,
        &theme::ThemeStamp::capture()?,
    )?;
    Ok((prepared, graph, text, context))
}
#[test]
fn all_original_scalar_buffers_cloud_occurrences_and_unique_cell_survive() -> io::Result<()> {
    let before = state();
    let (after, _, _, _) = roundtrip(&before, &[])?;
    let mut graph = cells::Capture::default();
    let mut text = native::Capture::default();
    let actual = Data::capture_state(&before, &mut graph, &mut text)?;
    let mut graph = cells::Capture::default();
    let mut text = native::Capture::default();
    let restored = Data::capture_state(&after.native, &mut graph, &mut text)?;
    check(
        serde_json::to_value(actual).map_err(native::invalid)?
            == serde_json::to_value(restored).map_err(native::invalid)?,
        "native transcript fields changed",
    )?;
    check(
        after.native.has_replayed_cloud_text("older", "two"),
        "original replay occurrence lost",
    )?;
    check(
        after
            .native
            .replayed_cloud_text
            .as_ref()
            .is_some_and(|v| v.get("empty").is_some_and(HashSet::is_empty)),
        "Some(empty replay set) changed",
    )?;
    let mut after = after.native;
    after.plan_delta_buffer.push_str("continued");
    check(
        after.plan_delta_buffer == "unfinished plan **continued",
        "original plan buffer could not continue",
    )?;
    after.reset_turn_flags();
    check(
        after.active_streamed_agent_message.is_none()
            && after.plan_delta_buffer.is_empty()
            && !after.plan_item_active
            && after.last_agent_source.as_deref() == Some("raw original **source**"),
        "original reset reducer changed after restore",
    )
}
#[test]
fn registry_updates_same_live_row_without_rewriting_equal_distinct_historical_row() -> io::Result<()>
{
    let row = dynamic("same-call")?;
    let historic = dynamic("same-call")?;
    let root: Arc<dyn HistoryCell> = Arc::new(row.clone());
    let historical: Arc<dyn HistoryCell> = Arc::new(historic);
    let mut before = state();
    before.dynamic_calls.insert(row.call_id().into(), row);
    let mut capture = cells::Capture::default();
    let live = capture.intern(&root)?;
    let old = capture.intern(&historical)?;
    let mut text = native::Capture::default();
    let data = Data::capture_state(&before, &mut capture, &mut text)?;
    let bytes = capture.encode(&mut text, LIMIT)?;
    let mut text = native::Restored::new(text.export()?)?;
    let mut context = cells::RestoreContext::default();
    let graph = cells::Restored::decode(&bytes, LIMIT, &mut text, &mut context)?;
    let after = data.prepare(
        &graph,
        &mut text,
        &mut context,
        &theme::ThemeStamp::capture()?,
    )?;
    check(
        after.native.dynamic_calls["same-call"]
            .update_from_item(item("same-call", DynamicToolCallStatus::Completed)),
        "original update refused exact registry row",
    )?;
    let live = graph.cell(live)?;
    let old = graph.cell(old)?;
    let live = live
        .as_any()
        .downcast_ref::<crate::history_cell::DynamicToolCallCell>()
        .ok_or_else(|| native::invalid("native dynamic type changed"))?;
    let old = old
        .as_any()
        .downcast_ref::<crate::history_cell::DynamicToolCallCell>()
        .ok_or_else(|| native::invalid("native historical dynamic type changed"))?;
    check(
        !live.is_active() && old.is_active(),
        "registry clone split or equal historical row merged",
    )
}
#[test]
fn unique_active_exec_reuses_global_detail_and_refuses_another_restore_context() -> io::Result<()> {
    let row = dynamic("detail")?;
    let root: Arc<dyn HistoryCell> = Arc::new(row.clone());
    let mut exec = crate::exec_cell::new_active_exec_command(
        "exec".into(),
        vec!["cat".into()],
        vec![],
        codex_app_server_protocol::CommandExecutionSource::Agent,
        None,
        false,
    );
    exec.group.details.push(1, root.clone());
    let mut before = state();
    before.active_cell = Some(Box::new(exec));
    before.dynamic_calls.insert("detail".into(), row);
    let mut capture = cells::Capture::default();
    capture.intern(&root)?;
    let mut text = native::Capture::default();
    let data = Data::capture_state(&before, &mut capture, &mut text)?;
    let mut malformed = serde_json::to_value(&data).map_err(native::invalid)?["activeCell"].clone();
    malformed["original"]["details"]["entries"][0][0] = json!(2);
    let malformed: cells::UniqueBody =
        serde_json::from_value(malformed).map_err(native::invalid)?;
    let bytes = capture.encode(&mut text, LIMIT)?;
    let mut text = native::Restored::new(text.export()?)?;
    let mut context = cells::RestoreContext::default();
    let graph = cells::Restored::decode(&bytes, LIMIT, &mut text, &mut context)?;
    check(
        graph
            .restore_unique(malformed, &mut text, &mut context)
            .is_err(),
        "out-of-range original detail accepted",
    )?;
    // Failure above must release its temporary resolver; SAME real body still restores.
    check(
        graph
            .require_context(&cells::RestoreContext::default())
            .is_err(),
        "foreign context admitted",
    )?;
    let after = data.prepare(
        &graph,
        &mut text,
        &mut context,
        &theme::ThemeStamp::capture()?,
    )?;
    after.context.require(&context)?;
    check(
        after
            .context
            .require(&cells::RestoreContext::default())
            .is_err(),
        "prepared native transcript lost its install context identity",
    )?;
    let exec = after
        .native
        .active_cell
        .as_deref()
        .and_then(|cell| cell.as_any().downcast_ref::<crate::exec_cell::ExecCell>())
        .ok_or_else(|| native::invalid("unique Exec Box changed type"))?;
    let previous = exec
        .group
        .details
        .lines_after(1, 80, HistoryRenderMode::Rich);
    check(
        after.native.dynamic_calls["detail"]
            .update_from_item(item("detail", DynamicToolCallStatus::Completed)),
        "restored actual detail refused update",
    )?;
    check(
        exec.group
            .details
            .lines_after(1, 80, HistoryRenderMode::Rich)
            != previous,
        "unique active details split from SAME global dynamic row",
    )
}
#[test]
fn layout_rebinds_actual_unique_box_and_original_stale_identity_invalidates() -> io::Result<()> {
    let before = state();
    let active = before
        .active_cell
        .as_deref()
        .ok_or_else(|| native::invalid("missing original active"))?;
    let old_identity = active as *const dyn HistoryCell as *const () as usize;
    let original = ActiveCellLayoutCache {
        key: ActiveCellLayoutCacheKey {
            cell_identity: old_identity,
            revision: 97,
            width: 71,
            render_mode: HistoryRenderMode::Raw,
            syntax_theme_revision: 19,
        },
        desired_height: Some(3),
        rendered_height: Some(17),
    };
    let (after, _, _, _) = roundtrip(&before, &[])?;
    let layout = Layout::capture(original, Some(active));
    let actual = after
        .native
        .active_cell
        .as_deref()
        .ok_or_else(|| native::invalid("restored active missing"))?;
    let restored = layout
        .restore(Some(actual), Some(29))?
        .ok_or_else(|| native::invalid("actual bound layout lost"))?;
    check(
        restored.key.cell_identity == actual as *const dyn HistoryCell as *const () as usize
            && restored.key.cell_identity != old_identity
            && restored.key.syntax_theme_revision == 29
            && restored.desired_height == Some(3)
            && restored.rendered_height == Some(17),
        "cache retained old pointer or changed native heights",
    )?;
    check(
        layout.restore(Some(actual), None)?.is_none(),
        "changed actual epoch reused old layout",
    )?;
    let stale = Layout::capture(original, Some(actual));
    check(
        stale.restore(Some(actual), Some(29))?.is_none(),
        "foreign original Box cache reused",
    )?;
    check(
        layout.restore(None, Some(29)).is_err(),
        "active cache without actual unique Box accepted",
    )
}
#[tokio::test]
async fn actual_status_copy_handle_and_fields_share_the_same_scene_tables() -> io::Result<()> {
    let (mut widget, _sender, mut events, _ops) =
        crate::boxed_future!(crate::chatwidget::tests::make_chatwidget_manual_with_sender()).await;
    widget.add_status_output(true, Some(91));
    let shared: Arc<str> = Arc::from("exact copy field");
    let source = widget
        .transcript
        .last_status_copy_targets
        .as_mut()
        .ok_or_else(|| native::invalid("original status producer did not retain copy handle"))?;
    source.fields.push(("first".into(), shared.clone()));
    source.fields.push(("second".into(), shared));
    let original_copy = source.handle.copy_text();
    let roots = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            crate::app_event::AppEvent::InsertHistoryCell(cell) => {
                Some(Arc::<dyn HistoryCell>::from(cell))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    check(
        !roots.is_empty(),
        "original status producer published no native history",
    )?;
    let (after, _, _, _) = roundtrip(&widget.transcript, &roots)?;
    let restored = after
        .native
        .last_status_copy_targets
        .as_ref()
        .ok_or_else(|| native::invalid("restored native status copy source missing"))?;
    check(
        restored.handle.copy_text() == original_copy,
        "status handle copy state changed",
    )?;
    let n = restored.fields.len();
    check(
        n >= 2 && Arc::ptr_eq(&restored.fields[n - 2].1, &restored.fields[n - 1].1),
        "actual copy field text aliases split",
    )
}
#[test]
fn missing_options_duplicate_occurrences_and_fake_cache_owner_fail_closed() -> io::Result<()> {
    let mut before = state();
    before
        .dynamic_calls
        .insert("original-row".into(), dynamic("original-row")?);
    let mut capture = cells::Capture::default();
    let data = Data::capture_state(&before, &mut capture, &mut native::Capture::default())?;
    let original = serde_json::to_value(data).map_err(native::invalid)?;
    let rejects = |value: serde_json::Value| -> io::Result<()> {
        let raw = serde_json::value::to_raw_value(&value).map_err(native::invalid)?;
        check(
            Data::decode(&raw, LIMIT).is_err(),
            "malformed native transcript accepted",
        )
    };
    for field in [
        "activeCell",
        "lastAgentSource",
        "lastStatusCopyTargets",
        "replayedCloudText",
        "lastPlanProgress",
    ] {
        let mut value = original.clone();
        value
            .as_object_mut()
            .ok_or_else(|| native::invalid("fixture not object"))?
            .remove(field);
        rejects(value)?;
    }
    let mut value = original.clone();
    let entry = value["dynamicCalls"][0].clone();
    value["dynamicCalls"]
        .as_array_mut()
        .ok_or_else(|| native::invalid("missing actual dynamic array"))?
        .push(entry);
    rejects(value)?;
    let mut value = original.clone();
    value["replayedCloudText"] = json!([["turn", ["item", "item"]]]);
    rejects(value)?;
    let mut value = original;
    value["activeCell"] = serde_json::Value::Null;
    value["activeCellLayout"] = json!({"owner":{"kind":"active"},"revision":1,"width":1,
        "mode":{"kind":"rich"},"themeRevision":0,"desiredHeight":null,"renderedHeight":null});
    rejects(value)
}

#[test]
fn same_prepared_unique_owner_and_declaration_survive_local_cache_retry_invalidation()
-> io::Result<()> {
    let before = state();
    let original = before
        .active_cell
        .as_deref()
        .ok_or_else(|| native::invalid("original unique active Box missing"))?;
    let captured_revision = crate::render::highlight::syntax_theme_revision();
    before.active_cell_layout.set(Some(ActiveCellLayoutCache {
        key: ActiveCellLayoutCacheKey {
            cell_identity: original as *const dyn HistoryCell as *const () as usize,
            revision: 97,
            width: 71,
            render_mode: HistoryRenderMode::Raw,
            syntax_theme_revision: captured_revision,
        },
        desired_height: Some(3),
        rendered_height: Some(17),
    }));
    let (prepared, _cells, _text, context) = roundtrip(&before, &[])?;
    let actual = prepared
        .native
        .active_cell
        .as_deref()
        .ok_or_else(|| native::invalid("prepared real unique Box missing"))?;
    let identity = actual as *const dyn HistoryCell as *const () as usize;
    let declared = prepared
        .layout
        .as_ref()
        .ok_or_else(|| native::invalid("original layout declaration lost"))?
        .theme_revision;
    let stamp = prepared.theme.revision_data();
    // Exercise the SAME production cache-data helper after different local
    // generation decisions. This does not construct Admitted/Binding or claim
    // that these numeric DATA values prove a real renderer/Native permission.
    prepared.refresh_layout(Some(29))?;
    check(
        prepared
            .native
            .active_cell_layout
            .get()
            .is_some_and(|value| {
                value.key.cell_identity == identity && value.key.syntax_theme_revision == 29
            }),
        "first data binding lost actual Box identity",
    )?;
    prepared.refresh_layout(None)?;
    check(
        prepared.native.active_cell_layout.get().is_none(),
        "pending original invalidation retained stale cache",
    )?;
    prepared.refresh_layout(Some(31))?;
    check(
        prepared
            .native
            .active_cell_layout
            .get()
            .is_some_and(|value| {
                value.key.cell_identity == identity
                    && value.key.syntax_theme_revision == 31
                    && value.desired_height == Some(3)
                    && value.rendered_height == Some(17)
            }),
        "later data rebind lost native cache declaration",
    )?;
    check(
        prepared
            .layout
            .as_ref()
            .is_some_and(|value| value.theme_revision == declared)
            && prepared.theme.revision_data() == stamp,
        "retry discarded captured layout/theme declaration",
    )?;
    prepared.context.require(&context)?;
    check(
        prepared
            .context
            .require(&cells::RestoreContext::default())
            .is_err(),
        "retained context became unrelated context",
    )?;
    let after = prepared
        .native
        .active_cell
        .as_deref()
        .ok_or_else(|| native::invalid("actual unique Box dropped during data retry"))?;
    check(
        after as *const dyn HistoryCell as *const () as usize == identity,
        "cache retries replaced the actual unique Box",
    )
}
