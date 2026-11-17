// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original native field/alias controls only. No Core, callback/reply,
//! filesystem resource, Native/SDK grant or whole-App application fabricated.
use super::*;
use crate::history_cell::patches::alab_retained_patches as patch;
use crate::history_cell::request_user_input::alab_retained_request_user_input as questions;
use crate::history_cell::startup_warnings::StartupWarningsCell;
use crate::history_cell::startup_warnings::alab_retained_startup_warnings as startup;
use crate::history_cell::warnings::WarningHistoryCell;
use crate::history_cell::warnings::alab_retained_warnings as warning;
use crate::markdown_render::ListSpacing;
use serde::de::DeserializeOwned;
use std::ffi::OsString;

fn round_trip<D: Serialize + DeserializeOwned>(
    actual: &dyn HistoryCell,
    capture: fn(&dyn HistoryCell, &mut native::Capture, usize) -> io::Result<Option<D>>,
    restore: fn(D, &mut native::Restored, usize) -> io::Result<Box<dyn HistoryCell>>,
) -> io::Result<Box<dyn HistoryCell>> {
    let mut shared = native::Capture::default();
    let data = capture(actual, &mut shared, 0)?
        .ok_or_else(|| native::invalid("wrong actual native test type"))?;
    let before = serde_json::to_value(&data).map_err(native::invalid)?;
    let bytes = serde_json::to_vec(&data).map_err(native::invalid)?;
    let decoded = serde_json::from_slice(&bytes).map_err(native::invalid)?;
    let mut restored = native::Restored::new(shared.export()?)?;
    let cell = restore(decoded, &mut restored, 0)?;
    let mut after_shared = native::Capture::default();
    let after = capture(cell.as_ref(), &mut after_shared, 0)?
        .ok_or_else(|| native::invalid("native restored type changed"))?;
    assert_eq!(
        before,
        serde_json::to_value(after).map_err(native::invalid)?
    );
    Ok(cell)
}

#[test]
fn plan_update_preserves_activity_id_order_status_and_explicit_explanation() -> io::Result<()> {
    let actual = new_plan_update(UpdatePlanArgs {
        explanation: Some(String::new()),
        plan: vec![
            PlanItemArg {
                step: "already done".into(),
                status: StepStatus::Completed,
            },
            PlanItemArg {
                step: "next 界".into(),
                status: StepStatus::InProgress,
            },
            PlanItemArg {
                step: "last".into(),
                status: StepStatus::Pending,
            },
        ],
    });
    let restored = round_trip(&actual, Data::capture, Data::restore)?;
    assert_eq!(actual.activity_ids(), restored.activity_ids());
    assert_eq!(actual.raw_lines(), restored.raw_lines());
    for width in [1, 16, 80] {
        assert_eq!(
            actual.display_hyperlink_lines(width),
            restored.display_hyperlink_lines(width)
        );
    }
    let restored = restored
        .as_any()
        .downcast_ref::<PlanUpdateCell>()
        .ok_or_else(|| native::invalid("wrong restored plan type"))?;
    assert_eq!(restored.explanation, Some(String::new()));
    Ok(())
}

#[test]
fn proposed_plan_preserves_non_utf8_cwd_and_exact_cache_aliases_with_stream_tail() -> io::Result<()>
{
    let cwd = PathBuf::from(OsString::from_vec(vec![b'/', b'o', b'l', b'd', 0xff]));
    let actual = new_proposed_plan("**original revision**".into(), &cwd);
    let _ = actual.retained_hyperlink_lines(20, false);
    let (key, lines) = actual
        .rendered_lines
        .cached
        .lock()
        .map_err(|_| native::invalid("native cache poisoned"))?
        .clone()
        .ok_or_else(|| native::invalid("actual original renderer did not retain cache"))?;
    let stream = StreamingPlanTailCell::new(lines.clone(), true);
    let mut shared = native::Capture::default();
    let a = Data::capture(&actual, &mut shared, 0)?
        .ok_or_else(|| native::invalid("missing actual plan"))?;
    let b = Data::capture(&stream, &mut shared, 0)?
        .ok_or_else(|| native::invalid("missing actual stream"))?;
    let mut native = native::Restored::new(shared.export()?)?;
    let a = a.restore(&mut native, 0)?;
    let b = b.restore(&mut native, 0)?;
    let a = a
        .as_any()
        .downcast_ref::<ProposedPlanCell>()
        .ok_or_else(|| native::invalid("wrong restored source plan"))?;
    let b = b
        .as_any()
        .downcast_ref::<StreamingPlanTailCell>()
        .ok_or_else(|| native::invalid("wrong restored plan tail"))?;
    assert_eq!(a.cwd.as_os_str().as_bytes(), cwd.as_os_str().as_bytes());
    assert_eq!(a.plan_markdown, actual.plan_markdown);
    let after = a
        .rendered_lines
        .cached
        .lock()
        .map_err(|_| native::invalid("restored cache poisoned"))?;
    let (after_key, after_lines) = after
        .as_ref()
        .ok_or_else(|| native::invalid("missing restored cache revision"))?;
    assert_eq!(*after_key, key);
    assert!(lines_with_sources_eq(&lines, after_lines));
    let cached_source = after_lines
        .iter()
        .find_map(|line| line.source.as_ref())
        .ok_or_else(|| native::invalid("no actual source in cached plan"))?;
    let tail_source = b
        .lines
        .iter()
        .find_map(|line| line.source.as_ref())
        .ok_or_else(|| native::invalid("no actual source in plan tail"))?;
    assert!(std::sync::Arc::ptr_eq(
        &cached_source.text,
        &tail_source.text
    ));
    assert!(std::sync::Arc::ptr_eq(
        &cached_source.styles,
        &tail_source.styles
    ));
    assert!(b.is_stream_continuation);
    // The cached native key includes original terminal colors/theme revision and spacing.
    assert_eq!(after_key.list_spacing, ListSpacing::Uniform);
    Ok(())
}

#[test]
fn proposed_stream_retains_continuation_and_original_rich_lines() -> io::Result<()> {
    let mut line = HyperlinkLine::new(Line::from("stream 界"));
    line.source = Some(crate::terminal_hyperlinks::LogicalLineSource::from_line(
        &line.line,
    ));
    let actual = new_proposed_plan_stream(vec![line], true);
    let restored = round_trip(&actual, Data::capture, Data::restore)?;
    assert!(restored.is_stream_continuation());
    assert_eq!(actual.raw_lines(), restored.raw_lines());
    for width in [1, 9, 80] {
        assert_eq!(
            actual.display_hyperlink_lines(width),
            restored.display_hyperlink_lines(width)
        );
    }
    Ok(())
}

#[test]
fn patch_source_preserves_raw_paths_moves_change_variants_and_activity() -> io::Result<()> {
    let path = PathBuf::from(OsString::from_vec(vec![b'/', b'f', 0xff]));
    let moved = PathBuf::from(OsString::from_vec(vec![b'/', b'm', 0xfe]));
    let changes = HashMap::from([
        (
            path,
            FileChange::Update {
                unified_diff: "@@ -1 +1 @@\n-before\n+after\n".into(),
                move_path: Some(moved),
            },
        ),
        (
            PathBuf::from("added"),
            FileChange::Add {
                content: "new\n".into(),
            },
        ),
        (
            PathBuf::from("deleted"),
            FileChange::Delete {
                content: "old\n".into(),
            },
        ),
    ]);
    let actual = crate::history_cell::patches::new_patch_event(changes, Path::new("/original/cwd"));
    let restored = round_trip(&actual, patch::Data::capture, patch::Data::restore)?;
    assert_eq!(actual.activity_ids(), restored.activity_ids());
    assert_eq!(actual.raw_lines(), restored.raw_lines());
    assert_eq!(actual.tool_call_labels(), restored.tool_call_labels());
    // DATA recapture equality above checks every raw byte, not lossy display paths.
    Ok(())
}

#[test]
fn patch_failure_keeps_full_ansi_diagnostics_and_original_identity() -> io::Result<()> {
    let actual = crate::history_cell::patches::new_patch_apply_failure(
        "\x1b[31mfailed\x1b[0m\n\tdetail 界".into(),
    );
    let restored = round_trip(&actual, patch::Data::capture, patch::Data::restore)?;
    assert_eq!(actual.activity_ids(), restored.activity_ids());
    assert_eq!(actual.raw_lines(), restored.raw_lines());
    for width in [1, 12, 80] {
        assert_eq!(
            actual.transcript_hyperlink_lines(width),
            restored.transcript_hyperlink_lines(width)
        );
    }
    Ok(())
}

#[test]
fn completed_questions_keep_all_original_answers_and_secret_presentation() -> io::Result<()> {
    let actual = crate::history_cell::request_user_input::RequestUserInputResultCell {
        questions: vec![
            ToolRequestUserInputQuestion {
                id: "a".into(),
                header: "header".into(),
                question: "secret?".into(),
                is_other: true,
                is_secret: true,
                options: Some(vec![
                    codex_app_server_protocol::ToolRequestUserInputOption {
                        label: "choice".into(),
                        description: "description".into(),
                    },
                ]),
            },
            ToolRequestUserInputQuestion {
                id: "b".into(),
                header: "next".into(),
                question: "unanswered".into(),
                is_other: false,
                is_secret: false,
                options: None,
            },
        ],
        answers: HashMap::from([
            (
                "a".into(),
                ToolRequestUserInputAnswer {
                    answers: vec![
                        "private original value".into(),
                        "user_note: exact note".into(),
                    ],
                },
            ),
            (
                "extra original key".into(),
                ToolRequestUserInputAnswer {
                    answers: vec!["keep".into()],
                },
            ),
        ]),
        interrupted: true,
    };
    let restored = round_trip(&actual, questions::Data::capture, questions::Data::restore)?;
    assert_eq!(actual.raw_lines(), restored.raw_lines());
    for width in [1, 20, 80] {
        assert_eq!(
            actual.transcript_hyperlink_lines(width),
            restored.transcript_hyperlink_lines(width)
        );
    }
    // This restores a completed transcript result, never a live question/ReplyHandle.
    Ok(())
}

#[test]
fn hidden_warning_keeps_details_footer_identity_and_notice_flag() -> io::Result<()> {
    let actual = WarningHistoryCell {
        server_version_notice: true,
        visible_in_transcript: false,
        key: "stable warning".into(),
        diagnostic: "full\ndiagnostic".into(),
        details: PrefixedWrappedHistoryCell::new(
            ratatui::text::Text::from(vec![Line::from("first".red()), Line::from("second".bold())])
                .style(Style::default().bg(ratatui::style::Color::Blue)),
            Line::from("! ".fg(ratatui::style::Color::Yellow)),
            Line::from("  "),
        ),
    };
    let restored = round_trip(&actual, warning::Data::capture, warning::Data::restore)?;
    assert!(restored.live_raw_lines().is_empty());
    assert_eq!(actual.raw_lines(), restored.raw_lines());
    assert_eq!(actual.warning_entries(), restored.warning_entries());
    assert_eq!(actual.warning_keys(), restored.warning_keys());
    for width in [1, 12, 80] {
        assert_eq!(
            actual.transcript_hyperlink_lines(width),
            restored.transcript_hyperlink_lines(width)
        );
    }
    Ok(())
}

#[test]
fn startup_warning_keeps_mcp_signin_sets_details_and_pending_header() -> io::Result<()> {
    let mut actual = StartupWarningsCell::mcp(
        vec!["first".into(), "second".into()],
        ["server-b".into(), "server-a".into()],
        Some(codex_app_server_protocol::McpServerStartupFailureReason::ReauthenticationRequired),
    );
    actual.other_sources.insert("original other source".into());
    actual.mcp_details.insert(
        "server-a".into(),
        vec!["detail one".into(), "detail two".into()],
    );
    actual.pending_header = true;
    let restored = round_trip(&actual, startup::Data::capture, startup::Data::restore)?;
    assert_eq!(actual.raw_lines(), restored.raw_lines());
    assert_eq!(actual.warning_entries(), restored.warning_entries());
    assert_eq!(actual.warning_keys(), restored.warning_keys());
    assert!(restored.live_raw_lines().is_empty());
    Ok(())
}

#[test]
fn native_aux_schema_status_and_duplicate_path_negatives_refuse() -> io::Result<()> {
    let actual = new_plan_update(UpdatePlanArgs {
        explanation: None,
        plan: vec![PlanItemArg {
            step: "x".into(),
            status: StepStatus::Pending,
        }],
    });
    let mut shared = native::Capture::default();
    let data = Data::capture(&actual, &mut shared, 0)?
        .ok_or_else(|| native::invalid("no actual update"))?;
    let mut value = serde_json::to_value(data).map_err(native::invalid)?;
    value["body"]["plan"][0]["status"] = serde_json::json!({"kind":"pending","unexpected":true});
    assert!(serde_json::from_value::<Data>(value).is_err());
    let patch_actual = crate::history_cell::patches::new_patch_event(
        HashMap::from([(
            PathBuf::from("a"),
            FileChange::Add {
                content: "x".into(),
            },
        )]),
        Path::new("/"),
    );
    let patch_data = patch::Data::capture(&patch_actual, &mut shared, 0)?
        .ok_or_else(|| native::invalid("no actual patch"))?;
    let mut value = serde_json::to_value(patch_data).map_err(native::invalid)?;
    let duplicate = value["body"]["changes"][0].clone();
    value["body"]["changes"]
        .as_array_mut()
        .ok_or_else(|| native::invalid("missing test changes"))?
        .push(duplicate);
    let duplicated: patch::Data = serde_json::from_value(value).map_err(native::invalid)?;
    let mut native = native::Restored::new(shared.export()?)?;
    assert!(duplicated.restore(&mut native, 0).is_err());
    assert!(Data::capture(&actual, &mut native::Capture::default(), 256).is_err());
    Ok(())
}
