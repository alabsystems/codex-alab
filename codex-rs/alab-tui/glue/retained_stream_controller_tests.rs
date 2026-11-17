// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Real native streaming continuation through one shared rich-text graph.
//! These CPU/data controls do not manufacture Native/renderer/clipboard grants.
use super::*;
use crate::terminal_hyperlinks::lines_with_sources_eq;
use std::sync::Arc;

fn captured(actual: &StreamController) -> io::Result<(AgentData, native::Restored)> {
    let mut text = native::Capture::default();
    let data = AgentData::capture(actual, &mut text, &mut cells::Capture::default())?;
    let data =
        serde_json::from_slice(&serde_json::to_vec(&data).map_err(invalid)?).map_err(invalid)?;
    Ok((data, native::Restored::new(text.export()?)?))
}
fn original_round_trip(actual: &StreamController) -> io::Result<StreamController> {
    let (data, mut text) = captured(actual)?;
    // SAME-process CPU continuation, not an entered-scene install. No resource
    // is fabricated: these fixtures have no inline contexts or live renderer.
    let current = crate::render::highlight::syntax_theme_revision();
    let revision = data
        .core
        .render
        .fence_revision()
        .filter(|old| *old == current)
        .map(|_| current);
    Ok(StreamController {
        core: data.core.restore_native(&mut text, None, revision)?,
        header_emitted: data.header_emitted,
    })
}
fn same_cell(a: &Option<Box<dyn HistoryCell>>, b: &Option<Box<dyn HistoryCell>>) {
    assert_eq!(a.is_some(), b.is_some());
    if let (Some(a), Some(b)) = (a, b) {
        assert!(lines_with_sources_eq(
            &a.display_hyperlink_lines(47),
            &b.display_hyperlink_lines(47)
        ));
    }
}
fn compare_tail(a: &StreamController, b: &StreamController) {
    assert_eq!(a.queued_lines(), b.queued_lines());
    assert!(lines_with_sources_eq(
        &a.current_tail_lines(),
        &b.current_tail_lines()
    ));
}

#[test]
fn retained_actual_agent_table_partial_row_survives_tick_resize_and_finalization() -> io::Result<()>
{
    let mut original =
        StreamController::new(Some(31), &std::env::temp_dir(), HistoryRenderMode::Rich);
    original
        .push("first paragraph\n\n| left | right |\n| --- | --- |\n| first | value |\n| partial");
    let _ = original.on_commit_tick();
    let mut restored = original_round_trip(&original)?;
    compare_tail(&original, &restored);
    for delta in [
        " | second |\n",
        "\nAfter table with [link](https://example.com/a#b).\n",
        "尾部 text",
    ] {
        assert_eq!(original.push(delta), restored.push(delta));
        original.set_width(Some(19));
        restored.set_width(Some(19));
        let (a, ai) = original.on_commit_tick();
        let (b, bi) = restored.on_commit_tick();
        assert_eq!(ai, bi);
        same_cell(&a, &b);
        compare_tail(&original, &restored);
    }
    let (a, source_a) = original.finalize();
    let (b, source_b) = restored.finalize();
    assert_eq!(source_a, source_b);
    same_cell(&a, &b);
    Ok(())
}

#[test]
fn retained_original_highlighted_plain_and_uninitialized_fences_resume_native_append()
-> io::Result<()> {
    for language in ["rust", "unknown-original-language"] {
        for initialize in [false, true] {
            let mut original =
                StreamController::new(Some(56), &std::env::temp_dir(), HistoryRenderMode::Rich);
            original.push(&format!("```{language}\nfn main() {{\n"));
            if initialize {
                original.push("    let original = 1;\n");
            }
            assert!(render::has_open_fence(&original.core.render));
            let mut restored = original_round_trip(&original)?;
            assert_eq!(
                render::has_open_fence(&original.core.render),
                render::has_open_fence(&restored.core.render)
            );
            for delta in [
                "    /* multi-line\n",
                "       comment */\n",
                "    println!(\"next\");\n",
                "}\n```\n\nend\n",
            ] {
                assert_eq!(original.push(delta), restored.push(delta));
                compare_tail(&original, &restored);
                let (a, ai) = original.on_commit_tick_batch(3);
                let (b, bi) = restored.on_commit_tick_batch(3);
                assert_eq!(ai, bi);
                same_cell(&a, &b);
            }
            let (a, sa) = original.finalize();
            let (b, sb) = restored.finalize();
            assert_eq!(sa, sb);
            same_cell(&a, &b);
        }
    }
    Ok(())
}

#[test]
fn retained_original_preview_waits_for_link_and_followup_closure() -> io::Result<()> {
    for prefix in [
        "Unicode α [label](https://private",
        "visible [Follow up]{prompt=\"private",
        "ordinary `inline",
        "$$\\sqrt{",
    ] {
        let mut original =
            StreamController::new(Some(36), &std::env::temp_dir(), HistoryRenderMode::Rich);
        original.push(prefix);
        let mut restored = original_round_trip(&original)?;
        compare_tail(&original, &restored);
        for delta in [" continuation", "\"} end)\n", "\nnext\n"] {
            assert_eq!(original.push(delta), restored.push(delta));
            compare_tail(&original, &restored);
        }
        let (a, sa) = original.finalize();
        let (b, sb) = restored.finalize();
        assert_eq!(sa, sb);
        same_cell(&a, &b);
    }
    Ok(())
}

#[test]
fn retained_original_plan_header_padding_and_partial_queue_are_not_reemitted() -> io::Result<()> {
    let mut original =
        PlanStreamController::new(Some(33), &std::env::temp_dir(), HistoryRenderMode::Rich);
    original.push("# Original plan\n\n- first step\n- second step\n\npartial");
    let _ = original.on_commit_tick_batch(2);
    let mut text = native::Capture::default();
    let data = PlanData::capture(&original, &mut text, &mut cells::Capture::default())?;
    let data: PlanData =
        serde_json::from_slice(&serde_json::to_vec(&data).map_err(invalid)?).map_err(invalid)?;
    let mut text = native::Restored::new(text.export()?)?;
    let mut restored = PlanStreamController {
        core: data.core.restore_native(&mut text, None, None)?,
        header_emitted: data.header_emitted,
        top_padding_emitted: data.top_padding_emitted,
    };
    for delta in [" tail\n", "\nmore\n"] {
        assert_eq!(original.push(delta), restored.push(delta));
        original.set_width(Some(17));
        restored.set_width(Some(17));
        let (a, ai) = original.on_commit_tick();
        let (b, bi) = restored.on_commit_tick();
        assert_eq!(ai, bi);
        same_cell(&a, &b);
    }
    let (a, sa) = original.finalize();
    let (b, sb) = restored.finalize();
    assert_eq!(sa, sb);
    same_cell(&a, &b);
    Ok(())
}

#[test]
fn retained_shared_stream_graph_keeps_queued_and_rendered_source_aliases() -> io::Result<()> {
    let mut original =
        StreamController::new(Some(60), &std::env::temp_dir(), HistoryRenderMode::Rich);
    original.push("a paragraph with [link](https://example.com)\n\nnext\n");
    let mut text = native::Capture::default();
    let mut graph = cells::Capture::default();
    let a = AgentData::capture(&original, &mut text, &mut graph)?;
    let b = AgentData::capture(&original, &mut text, &mut graph)?;
    let bytes = serde_json::to_vec(&(a, b)).map_err(invalid)?;
    let (a, b): (AgentData, AgentData) = serde_json::from_slice(&bytes).map_err(invalid)?;
    let mut text = native::Restored::new(text.export()?)?;
    let a = a.core.restore_native(&mut text, None, None)?;
    let b = b.core.restore_native(&mut text, None, None)?;
    let a = a
        .render
        .lines
        .iter()
        .find_map(|x| x.source.as_ref())
        .ok_or_else(|| invalid("native source absent"))?;
    let b = b
        .render
        .lines
        .iter()
        .find_map(|x| x.source.as_ref())
        .ok_or_else(|| invalid("native source absent"))?;
    assert!(Arc::ptr_eq(&a.text, &b.text));
    Ok(())
}

#[test]
fn retained_original_stream_fifo_age_and_chunking_hysteresis_keep_monotonic_deadlines()
-> io::Result<()> {
    use crate::streaming::chunking::AdaptiveChunkingPolicy;
    use crate::streaming::chunking::QueueSnapshot;
    use crate::streaming::chunking::alab_retained_chunking;
    let mut original =
        StreamController::new(Some(30), &std::env::temp_dir(), HistoryRenderMode::Raw);
    original.push("one\ntwo\nthree\n");
    let restored = original_round_trip(&original)?;
    let now = Instant::now();
    let a = original
        .oldest_queued_age(now)
        .ok_or_else(|| invalid("original queue empty"))?;
    let b = restored
        .oldest_queued_age(now)
        .ok_or_else(|| invalid("restored queue empty"))?;
    assert!(a.abs_diff(b) < Duration::from_millis(1));
    let mut policy = AdaptiveChunkingPolicy::default();
    policy.decide(
        QueueSnapshot {
            queued_lines: 100,
            oldest_age: Some(Duration::from_secs(1)),
        },
        now,
    );
    let data = alab_retained_chunking::Data::capture(&policy)?;
    let mut resumed = data.restore()?;
    for seconds in [2, 4, 6] {
        let queue = QueueSnapshot {
            queued_lines: 2,
            oldest_age: Some(Duration::from_millis(1)),
        };
        assert_eq!(
            policy.decide(queue, now + Duration::from_secs(seconds)),
            resumed.decide(queue, now + Duration::from_secs(seconds))
        );
    }
    Ok(())
}

#[test]
fn retained_stream_decoder_refuses_missing_options_and_invalid_utf8_cursor_without_panicking()
-> io::Result<()> {
    let mut original =
        StreamController::new(Some(30), &std::env::temp_dir(), HistoryRenderMode::Rich);
    original.push("α pending");
    let (data, _) = captured(&original)?;
    let wire = serde_json::to_value(data).map_err(invalid)?;
    for field in [
        "width",
        "inline_visualization_context",
        "stable_prefix_len_cache",
    ] {
        let mut bad = wire.clone();
        bad["core"].as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<AgentData>(bad).is_err(), "{field}");
    }
    for field in [
        "pending_math_start",
        "mutable_fence_start",
        "open_code_fence",
    ] {
        let mut bad = wire.clone();
        bad["core"]["render"].as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<AgentData>(bad).is_err(), "{field}");
    }
    let mut bad = wire;
    bad["core"]["state"]["collector"]["committed_source_len"] = serde_json::json!(1);
    let data: AgentData = serde_json::from_value(bad).map_err(invalid)?;
    let mut text = native::Restored::new(native::Capture::default().export()?)?;
    assert!(data.core.restore_native(&mut text, None, None).is_err());
    Ok(())
}

#[test]
fn retained_stale_highlight_cache_keeps_historical_rows_until_native_next_update() -> io::Result<()>
{
    let mut original =
        StreamController::new(Some(50), &std::env::temp_dir(), HistoryRenderMode::Rich);
    original.push("```rust\nlet original = 1;\n");
    original.push("let second = 2;\n");
    let theme = crate::render::highlight::current_syntax_theme();
    crate::render::highlight::set_syntax_theme(theme);
    let mut restored = original_round_trip(&original)?;
    assert!(render::has_open_fence(&original.core.render));
    assert!(!render::has_open_fence(&restored.core.render));
    assert!(lines_with_sources_eq(
        &original.core.render.lines,
        &restored.core.render.lines
    ));
    original.push("let third = 3;\n");
    restored.push("let third = 3;\n");
    compare_tail(&original, &restored);
    let (a, sa) = original.finalize();
    let (b, sb) = restored.finalize();
    assert_eq!(sa, sb);
    same_cell(&a, &b);
    Ok(())
}

#[test]
fn retained_any_original_fence_cache_invalidates_when_actual_grammar_identity_differs()
-> io::Result<()> {
    for language in ["rust", "unknown-original-language"] {
        for initialized in [false, true] {
            let mut original =
                StreamController::new(Some(50), &std::env::temp_dir(), HistoryRenderMode::Rich);
            original.push(&format!("```{language}\nlet original = 1;\n"));
            if initialized {
                original.push("let second = 2;\n");
            }
            let (data, mut text) = captured(&original)?;
            let mut wire = serde_json::to_value(data).map_err(invalid)?;
            wire["core"]["render"]["open_code_fence"]["syntax_identity"] =
                serde_json::json!(vec![0; 32]);
            let data: AgentData = serde_json::from_value(wire).map_err(invalid)?;
            let restored = data.core.restore_native(
                &mut text,
                None,
                Some(crate::render::highlight::syntax_theme_revision()),
            )?;
            assert!(!render::has_open_fence(&restored.render));
            assert!(lines_with_sources_eq(
                &original.core.render.lines,
                &restored.render.lines
            ));
        }
    }
    Ok(())
}

#[test]
fn retained_original_fifo_count_must_match_its_actual_emitted_and_enqueued_boundaries()
-> io::Result<()> {
    for remove_queue_row in [false, true] {
        let mut original =
            StreamController::new(Some(30), &std::env::temp_dir(), HistoryRenderMode::Raw);
        original.push("one\ntwo\n");
        let (data, mut text) = captured(&original)?;
        let mut wire = serde_json::to_value(data).map_err(invalid)?;
        if remove_queue_row {
            wire["core"]["state"]["queued_lines"]
                .as_array_mut()
                .unwrap()
                .pop();
        } else {
            wire["core"]["enqueued_stable_len"] = serde_json::json!(0);
            wire["core"]["emitted_stable_len"] = serde_json::json!(0);
        }
        let data: AgentData = serde_json::from_value(wire).map_err(invalid)?;
        assert!(data.core.restore_native(&mut text, None, None).is_err());
    }
    Ok(())
}

#[test]
fn retained_stream_lines_cannot_default_missing_shared_native_metadata() -> io::Result<()> {
    let mut original =
        StreamController::new(Some(30), &std::env::temp_dir(), HistoryRenderMode::Rich);
    original.push("actual markdown\n\nnext\n");
    let (data, _) = captured(&original)?;
    let wire = serde_json::to_value(data).map_err(invalid)?;
    assert!(
        !wire["core"]["render"]["lines"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for (level, field) in [
        (0, "source"),
        (1, "alignment"),
        (2, "fg"),
        (2, "bg"),
        (2, "underline"),
    ] {
        let mut bad = wire.clone();
        let line = &mut bad["core"]["render"]["lines"][0];
        let object = match level {
            0 => line,
            1 => &mut line["line"],
            _ => &mut line["line"]["style"],
        };
        object.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<AgentData>(bad).is_err(), "{field}");
    }
    Ok(())
}
