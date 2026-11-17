// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual native-model roundtrips and live continuation, never Native/SDK permission.
use super::*;
use crate::exec_cell::ExecCell;
use crate::exec_cell::new_active_exec_command;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::DynamicToolCallStatus;
use codex_app_server_protocol::ThreadItemPayload;
use codex_protocol::mcp::CallToolResult;
use codex_protocol::parse_command::ParsedCommand;
use serde_json::json;
use std::time::Duration;
const LIMIT: usize = 8 * 1024 * 1024;

fn check(condition: bool, message: &str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(native::invalid(message))
    }
}
fn native_cell<T: HistoryCell + 'static>(value: &mut Arc<dyn HistoryCell>) -> io::Result<&mut T> {
    Arc::get_mut(value)
        .ok_or_else(|| native::invalid("test original cell unexpectedly shared"))?
        .as_any_mut()
        .downcast_mut::<T>()
        .ok_or_else(|| native::invalid("restored native type changed"))
}
fn roundtrip(originals: &[Arc<dyn HistoryCell>]) -> io::Result<Vec<Arc<dyn HistoryCell>>> {
    let mut capture = Capture::default();
    let ids = originals
        .iter()
        .map(|actual| capture.intern(actual))
        .collect::<io::Result<Vec<_>>>()?;
    let mut text = native::Capture::default();
    let bytes = capture.encode(&mut text, LIMIT)?;
    let mut shared = native::Restored::new(text.export()?)?;
    let restored = Restored::decode(&bytes, LIMIT, &mut shared, &mut RestoreContext::default())?;
    ids.into_iter().map(|id| restored.cell(id)).collect()
}
fn same_views(original: &dyn HistoryCell, restored: &dyn HistoryCell) -> io::Result<()> {
    for width in [0, 5, 24, 97] {
        check(
            original.display_lines(width) == restored.display_lines(width),
            "native activity display changed after resize",
        )?;
        check(
            original.transcript_lines(width) == restored.transcript_lines(width),
            "native activity transcript changed after resize",
        )?;
    }
    check(
        original.raw_lines() == restored.raw_lines(),
        "native activity raw source changed",
    )
}
fn call(id: &str) -> McpToolCallCell {
    McpToolCallCell::new(
        id.into(),
        McpInvocation {
            server: "cua_repl".into(),
            tool: "js".into(),
            arguments: Some(json!({"title":"Actual title", "code":"return original"})),
        },
        false,
    )
}
fn result() -> Result<CallToolResult, String> {
    Ok(CallToolResult {
        content: vec![
            json!({"type":"text","text":"exact output\nsecond line"}),
            json!({"type":"not-standard","text":"node top-level text", "retained":4}),
        ],
        structured_content: None,
        is_error: Some(false),
        meta: None,
    })
}
fn dynamic(id: &str, status: DynamicToolCallStatus, text: &str) -> ThreadItemPayload {
    ThreadItemPayload::DynamicToolCall {
        id: id.into(),
        namespace: Some("actual".into()),
        tool: "tool".into(),
        arguments: json!({"argument":[1,2]}),
        status,
        content_items: Some(vec![
            codex_app_server_protocol::DynamicToolCallOutputContentItem::InputText {
                text: text.into(),
            },
        ]),
        success: None,
        duration_ms: Some(41),
    }
}

#[test]
fn real_exec_parser_continues_split_crlf_and_truncation_without_reconstructing_output()
-> io::Result<()> {
    let mut original: Arc<dyn HistoryCell> = Arc::new(new_active_exec_command(
        "exec-1".into(),
        vec!["sh".into(), "-c".into(), "cat source".into()],
        vec![ParsedCommand::Read {
            cmd: "cat source".into(),
            name: "source".into(),
            path: "/actual/source".into(),
        }],
        CommandExecutionSource::Agent,
        Some("original stdin".into()),
        false,
    ));
    let initial = format!("{}\r", "界".repeat(400_000));
    check(
        native_cell::<ExecCell>(&mut original)?.append_output("exec-1", &initial),
        "actual first output not accepted",
    )?;
    let mut restored = roundtrip(std::slice::from_ref(&original))?.remove(0);
    same_views(original.as_ref(), restored.as_ref())?;
    for suffix in ["\nnext\r", "\nlast partial", "\n"] {
        check(
            native_cell::<ExecCell>(&mut original)?.append_output("exec-1", suffix),
            "original suffix denied",
        )?;
        check(
            native_cell::<ExecCell>(&mut restored)?.append_output("exec-1", suffix),
            "restored suffix denied",
        )?;
        same_views(original.as_ref(), restored.as_ref())?;
    }
    let a = original
        .as_any()
        .downcast_ref::<ExecCell>()
        .ok_or_else(|| native::invalid("original exec missing"))?;
    let b = restored
        .as_any()
        .downcast_ref::<ExecCell>()
        .ok_or_else(|| native::invalid("restored exec missing"))?;
    let first = a
        .active_start_time()
        .ok_or_else(|| native::invalid("original clock missing"))?;
    let second = b
        .active_start_time()
        .ok_or_else(|| native::invalid("restored clock missing"))?;
    check(
        (if first >= second {
            first.duration_since(second)
        } else {
            second.duration_since(first)
        }) <= Duration::from_millis(1),
        "exec animation clock reset",
    )
}

#[test]
fn real_completed_group_keeps_non_utf8_parsed_path_and_full_argv() -> io::Result<()> {
    use std::os::unix::ffi::OsStringExt;
    let path = std::path::PathBuf::from(std::ffi::OsString::from_vec(
        b"/actual/source-\xff".to_vec(),
    ));
    let mut original = new_active_exec_command(
        "read".into(),
        vec!["bash".into(), "-lc".into(), "read original".into()],
        vec![ParsedCommand::Read {
            cmd: "read original".into(),
            name: "raw".into(),
            path,
        }],
        CommandExecutionSource::UnifiedExecStartup,
        None,
        false,
    );
    check(
        original.complete_call(
            "read",
            crate::exec_cell::CommandOutput::new(
                7,
                "exact final stdout\n\u{1b}[31mstderr\u{1b}[0m".into(),
            ),
            Duration::from_millis(12),
        ),
        "actual completion missing",
    )?;
    let original: Arc<dyn HistoryCell> = Arc::new(original);
    let restored = roundtrip(std::slice::from_ref(&original))?.remove(0);
    same_views(original.as_ref(), restored.as_ref())?;
    let original = original
        .as_any()
        .downcast_ref::<ExecCell>()
        .ok_or_else(|| native::invalid("original exec missing"))?;
    let restored = restored
        .as_any()
        .downcast_ref::<ExecCell>()
        .ok_or_else(|| native::invalid("restored exec missing"))?;
    check(
        restored.group.calls[0].command == original.group.calls[0].command
            && restored.group.calls[0].parsed == original.group.calls[0].parsed
            && restored.group.calls[0].duration == original.group.calls[0].duration
            && restored.group.calls[0].start_time.is_none(),
        "exec full original fields changed",
    )
}

#[test]
fn real_mcp_and_computer_groups_keep_validated_results_and_complete_pending_original()
-> io::Result<()> {
    let mut finished = call("done");
    finished.complete(Duration::from_millis(5), result());
    let finished: Arc<dyn HistoryCell> = Arc::new(finished);
    let mut computer = ComputerActivityCell::default();
    computer.complete(call("older"), Duration::from_millis(6), result());
    computer.start(call("pending"));
    let mut original: Arc<dyn HistoryCell> = Arc::new(computer);
    let roots = roundtrip(&[finished.clone(), original.clone()])?;
    same_views(finished.as_ref(), roots[0].as_ref())?;
    let mut restored = roots
        .into_iter()
        .nth(1)
        .ok_or_else(|| native::invalid("restored computer missing"))?;
    same_views(original.as_ref(), restored.as_ref())?;
    native_cell::<ComputerActivityCell>(&mut original)?.complete(
        call("pending"),
        Duration::from_millis(9),
        result(),
    );
    native_cell::<ComputerActivityCell>(&mut restored)?.complete(
        call("pending"),
        Duration::from_millis(9),
        result(),
    );
    same_views(original.as_ref(), restored.as_ref())
}

#[test]
fn dynamic_clones_share_one_actual_mutable_row_but_equal_distinct_rows_do_not_alias()
-> io::Result<()> {
    let source = DynamicToolCallCell::from_item(dynamic(
        "same-id",
        DynamicToolCallStatus::InProgress,
        "original",
    ))
    .ok_or_else(|| native::invalid("actual dynamic constructor refused"))?;
    let distinct = DynamicToolCallCell::from_item(dynamic(
        "same-id",
        DynamicToolCallStatus::InProgress,
        "original",
    ))
    .ok_or_else(|| native::invalid("actual dynamic constructor refused"))?;
    let originals: Vec<Arc<dyn HistoryCell>> = vec![
        Arc::new(source.clone()),
        Arc::new(source),
        Arc::new(distinct),
    ];
    // This is the genuine clone shape held by ChatWidget's active registry.
    let registry = originals[0]
        .as_any()
        .downcast_ref::<DynamicToolCallCell>()
        .ok_or_else(|| native::invalid("actual registry cell missing"))?
        .clone();
    let mut capture = Capture::default();
    let registry_data = capture.dynamic_cell(&registry)?;
    let ids = originals
        .iter()
        .map(|actual| capture.intern(actual))
        .collect::<io::Result<Vec<_>>>()?;
    let mut text = native::Capture::default();
    let bytes = capture.encode(&mut text, LIMIT)?;
    let mut shared = native::Restored::new(text.export()?)?;
    let mut resources = RestoreContext::default();
    let restored = Restored::decode(&bytes, LIMIT, &mut shared, &mut resources)?;
    let registry = resources.dynamic_cell(&registry_data)?;
    let roots = ids
        .into_iter()
        .map(|id| restored.cell(id))
        .collect::<io::Result<Vec<_>>>()?;
    check(
        !Arc::ptr_eq(&roots[0], &roots[1]),
        "distinct original outer cells were merged",
    )?;
    let first = roots[0]
        .as_any()
        .downcast_ref::<DynamicToolCallCell>()
        .ok_or_else(|| native::invalid("dynamic type changed"))?;
    let second = roots[1]
        .as_any()
        .downcast_ref::<DynamicToolCallCell>()
        .ok_or_else(|| native::invalid("dynamic type changed"))?;
    let distinct = roots[2]
        .as_any()
        .downcast_ref::<DynamicToolCallCell>()
        .ok_or_else(|| native::invalid("dynamic type changed"))?;
    check(
        registry.update_from_item(dynamic(
            "same-id",
            DynamicToolCallStatus::Completed,
            "exact fresh output",
        )),
        "actual restored registry update denied",
    )?;
    check(
        !first.is_active(),
        "registry update did not reach actual history row",
    )?;
    check(
        !second.is_active() && distinct.is_active(),
        "dynamic actual aliases lost or equal distinct rows merged",
    )?;
    same_views(first, second)
}

#[test]
fn original_terminal_search_and_loading_variants_restore_native_models() -> io::Result<()> {
    let mut search = WebSearchCell::new(
        "search".into(),
        "original query".into(),
        Some(WebSearchAction::FindInPage {
            url: Some("https://example.invalid/original".into()),
            pattern: Some("same pattern".into()),
        }),
        false,
    );
    search.complete();
    let roots: Vec<Arc<dyn HistoryCell>> = vec![
        Arc::new(new_unified_exec_interaction(
            Some("same command".into()),
            "exact stdin\nsecond line".into(),
        )),
        Arc::new(search),
        Arc::new(McpInventoryLoadingCell::new(false)),
    ];
    let restored = roundtrip(&roots)?;
    for (a, b) in roots.iter().zip(&restored) {
        same_views(a.as_ref(), b.as_ref())?;
    }
    check(
        restored[0].as_any().is::<UnifiedExecInteractionCell>()
            && restored[1].as_any().is::<WebSearchCell>()
            && restored[2].as_any().is::<McpInventoryLoadingCell>(),
        "native terminal/search/loading types replaced",
    )
}

#[test]
fn malformed_cycle_missing_detail_and_changed_dynamic_alias_are_refused() -> io::Result<()> {
    let detail: Arc<dyn HistoryCell> = Arc::new(ReasoningSummaryCell::new(
        "header".into(),
        "actual reasoning".into(),
        std::path::Path::new("/actual"),
        true,
    ));
    let mut exec = new_active_exec_command(
        "actual".into(),
        vec!["cat".into()],
        vec![],
        CommandExecutionSource::Agent,
        None,
        false,
    );
    exec.group.details.push(1, detail);
    let actual: Arc<dyn HistoryCell> = Arc::new(exec);
    let mut capture = Capture::default();
    capture.intern(&actual)?;
    let mut text = native::Capture::default();
    let bytes = capture.encode(&mut text, LIMIT)?;
    let graph = serde_json::from_slice::<serde_json::Value>(&bytes).map_err(native::invalid)?;
    for value in [0, 999999] {
        let mut changed = graph.clone();
        changed["cells"][0]["original"]["details"]["entries"][0][1] = json!(value);
        let changed = serde_json::to_vec(&changed).map_err(native::invalid)?;
        let mut shared = native::Restored::new(text.export()?)?;
        check(
            Restored::decode(&changed, LIMIT, &mut shared, &mut RestoreContext::default()).is_err(),
            "decoded cycle/missing edge was accepted",
        )?;
    }
    let row = DynamicToolCallCell::from_item(dynamic(
        "actual",
        DynamicToolCallStatus::InProgress,
        "original",
    ))
    .ok_or_else(|| native::invalid("dynamic constructor refused"))?;
    let root: Arc<dyn HistoryCell> = Arc::new(row);
    let mut capture = Capture::default();
    capture.intern(&root)?;
    let mut text = native::Capture::default();
    let mut graph: serde_json::Value =
        serde_json::from_slice(&capture.encode(&mut text, LIMIT)?).map_err(native::invalid)?;
    graph["cells"][0]["original"]["callId"] = json!("foreign");
    let mut shared = native::Restored::new(text.export()?)?;
    check(
        Restored::decode(
            &serde_json::to_vec(&graph).map_err(native::invalid)?,
            LIMIT,
            &mut shared,
            &mut RestoreContext::default(),
        )
        .is_err(),
        "dynamic alias changed its original call identity",
    )
}
