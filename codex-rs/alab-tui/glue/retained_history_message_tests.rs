// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original native message/render controls. No Core, Source/Native role or UI ACK.
use super::*;
use crate::history_cell::alab_retained_cells::Capture;
use crate::history_cell::alab_retained_cells::Restored;
use crate::terminal_hyperlinks::LogicalLineSource;
use pretty_assertions::assert_eq;
use std::sync::Arc;
const BYTES: usize = 8 * 1024 * 1024;

fn restore(original: Arc<dyn HistoryCell>) -> io::Result<Arc<dyn HistoryCell>> {
    let mut capture = Capture::default();
    let id = capture.intern(&original)?;
    let mut lines = native::Capture::default();
    let bytes = capture.encode(&mut lines, BYTES)?;
    Restored::decode(
        &bytes,
        BYTES,
        &mut native::Restored::new(lines.export()?)?,
        &mut RestoreContext::default(),
    )?
    .cell(id)
}
#[test]
fn original_reasoning_identity_transcript_policy_and_byte_cwd_survive() -> io::Result<()> {
    use std::os::unix::ffi::OsStringExt;
    let cwd = PathBuf::from(std::ffi::OsString::from_vec(
        b"/native/session-\xff".to_vec(),
    ));
    let mut original = ReasoningSummaryCell::new(
        "original header".into(),
        "**original** reasoning\n\n[local](file.rs)".into(),
        &cwd,
        true,
    );
    original.set_source_item_id("same-real-source-item".into());
    let original: Arc<dyn HistoryCell> = Arc::new(original);
    let restored = restore(original.clone())?;
    let actual = restored
        .as_any()
        .downcast_ref::<ReasoningSummaryCell>()
        .unwrap();
    assert_eq!(actual._header, "original header");
    assert_eq!(actual.source_item_id(), Some("same-real-source-item"));
    assert_eq!(actual.cwd, cwd);
    assert!(actual.is_transcript_only());
    for width in [16, 80] {
        assert!(restored.display_lines(width).is_empty());
        assert_eq!(
            restored.transcript_lines(width),
            original.transcript_lines(width)
        );
    }
    Ok(())
}
#[test]
fn exact_markdown_questions_copy_interrupted_phase_and_populated_cache_survive() -> io::Result<()> {
    let source =
        "Original question?\n- alpha\n- beta\n\n**second** paragraph [relative](src/lib.rs)";
    let original = AgentMarkdownCell::new(source.into(), Path::new("/native/original/cwd"))
        .with_questions(&[codex_protocol::items::AsyncUserInputQuestion {
            title: "Original question?".into(),
            options: Some(vec!["alpha".into(), "beta".into()]),
        }])
        .with_copy_source(Some("whole original private copy source".into()))
        .with_phase(Some(codex_protocol::models::MessagePhase::PartialAnswer));
    let mut original = original;
    original.interrupted_item = Some(("original-turn".into(), "original-item".into()));
    original.spoken_artifacts = true;
    let rendered = original.display_hyperlink_lines(40);
    let (key, cached) = original
        .rendered_lines
        .as_ref()
        .unwrap()
        .cached
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .clone();
    let original: Arc<dyn HistoryCell> = Arc::new(original);
    let restored = restore(original.clone())?;
    let actual = restored
        .as_any()
        .downcast_ref::<AgentMarkdownCell>()
        .unwrap();
    assert_eq!(
        actual.interrupted_item,
        Some(("original-turn".into(), "original-item".into()))
    );
    assert_eq!(
        actual.phase,
        Some(codex_protocol::models::MessagePhase::PartialAnswer)
    );
    assert_eq!(
        actual.copy_source(),
        Some("whole original private copy source")
    );
    assert_eq!(actual.cwd, Path::new("/native/original/cwd"));
    assert_eq!(actual.question_titles, vec![0.."Original question?".len()]);
    assert!(actual.spoken_artifacts);
    let restored_cache = actual
        .rendered_lines
        .as_ref()
        .unwrap()
        .cached
        .lock()
        .unwrap();
    let (after_key, after_lines) = restored_cache.as_ref().unwrap();
    assert_eq!(*after_key, key);
    assert!(crate::terminal_hyperlinks::lines_with_sources_eq(
        after_lines,
        &cached
    ));
    drop(restored_cache);
    assert_eq!(actual.display_hyperlink_lines(40), rendered);
    for width in [19, 71] {
        assert_eq!(actual.display_lines(width), original.display_lines(width));
    }
    Ok(())
}
#[test]
fn native_stream_and_agent_lines_share_exact_external_source_arcs() -> io::Result<()> {
    let mut line = HyperlinkLine::new(Line::from("same real rich source\t界"));
    line.source = Some(LogicalLineSource::from_line(&line.line));
    let agent: Arc<dyn HistoryCell> = Arc::new(AgentMessageCell::new_hyperlink_lines(
        vec![line.clone()],
        false,
    ));
    let tail: Arc<dyn HistoryCell> =
        Arc::new(StreamingAgentTailCell::new(vec![line.clone()], true));
    let mut capture = Capture::default();
    let agent_id = capture.intern(&agent)?;
    let tail_id = capture.intern(&tail)?;
    let mut shared = native::Capture::default();
    let external = shared.line(&line)?;
    let bytes = capture.encode(&mut shared, BYTES)?;
    let mut shared = native::Restored::new(shared.export()?)?;
    let restored = Restored::decode(&bytes, BYTES, &mut shared, &mut RestoreContext::default())?;
    let agent_after = restored.cell(agent_id)?;
    let tail_after = restored.cell(tail_id)?;
    let external = shared.line(&external)?.source.unwrap();
    let a = agent_after
        .as_any()
        .downcast_ref::<AgentMessageCell>()
        .unwrap()
        .lines[0]
        .source
        .as_ref()
        .unwrap();
    let b = tail_after
        .as_any()
        .downcast_ref::<StreamingAgentTailCell>()
        .unwrap()
        .lines[0]
        .source
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&a.text, &b.text));
    assert!(Arc::ptr_eq(&a.text, &external.text));
    assert!(Arc::ptr_eq(&a.styles, &external.styles));
    for width in [7, 80] {
        assert_eq!(agent_after.display_lines(width), agent.display_lines(width));
        assert_eq!(tail_after.display_lines(width), tail.display_lines(width));
    }
    assert!(agent_after.is_stream_continuation());
    assert!(!tail_after.is_stream_continuation());
    Ok(())
}
#[test]
fn malformed_message_ranges_phase_missing_fields_and_directive_cache_are_refused() -> io::Result<()>
{
    let original: Arc<dyn HistoryCell> =
        Arc::new(AgentMarkdownCell::new("é original".into(), Path::new("/")));
    let mut capture = Capture::default();
    capture.intern(&original)?;
    let mut lines = native::Capture::default();
    let bytes = capture.encode(&mut lines, BYTES)?;
    let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let cases = [
        ("questionTitles", serde_json::json!([{"start":1,"end":2}])),
        ("phase", serde_json::json!(9)),
        (
            "markdownSource",
            serde_json::json!("::codex-inline-vis{\"file\":\"chart.html\"}"),
        ),
    ];
    for (field, value) in cases {
        let mut data = original.clone();
        data["cells"][0]["original"]["original"][field] = value;
        let mut shared = native::Restored::new(lines.export()?)?;
        assert!(
            Restored::decode(
                &serde_json::to_vec(&data).unwrap(),
                BYTES,
                &mut shared,
                &mut RestoreContext::default()
            )
            .is_err(),
            "{field}"
        );
    }
    let mut data = original;
    data["cells"][0]["original"]["original"]
        .as_object_mut()
        .unwrap()
        .remove("copySource");
    assert!(
        Restored::decode(
            &serde_json::to_vec(&data).unwrap(),
            BYTES,
            &mut native::Restored::new(lines.export()?)?,
            &mut RestoreContext::default()
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn actual_message_context_requires_genuine_binding_before_any_native_cell_is_returned()
-> io::Result<()> {
    let home = tempfile::tempdir()?;
    let context = crate::inline_visualization::InlineVisualizationContext::new(
        home.path(),
        codex_protocol::ThreadId::new(),
    )
    .unwrap();
    let original: Arc<dyn HistoryCell> =
        Arc::new(AgentMarkdownCell::new_with_inline_visualizations(
            "**ordinary original source**".into(),
            home.path(),
            Some(context.clone()),
        ));
    let mut capture = Capture::default();
    let id = capture.intern(&original)?;
    let mut lines = native::Capture::default();
    let bytes = capture.encode(&mut lines, BYTES)?;
    assert!(
        Restored::decode(
            &bytes,
            BYTES,
            &mut native::Restored::new(lines.export()?)?,
            &mut RestoreContext::default()
        )
        .is_err()
    );
    let mut resources = RestoreContext::default();
    resources.bind_inline(&context)?;
    let restored = Restored::decode(
        &bytes,
        BYTES,
        &mut native::Restored::new(lines.export()?)?,
        &mut resources,
    )?
    .cell(id)?;
    assert!(
        restored
            .as_any()
            .downcast_ref::<AgentMarkdownCell>()
            .unwrap()
            .inline_visualization_context
            .is_some()
    );
    for width in [11, 80] {
        assert_eq!(restored.display_lines(width), original.display_lines(width));
    }
    Ok(())
}
