// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native layout/copy DATA controls only. No terminal placement, clipboard,
//! callback, Source/Native permission or successful whole-App restore fabricated.
use super::*;
use crate::history_cell::AgentMarkdownCell;
use crate::history_cell::HistoryCell;
use crate::markdown_copy::CopyLine;
use crate::pets::ImageProtocol;
use crate::transcript_image::TranscriptImage;
use image::RgbaImage;
use ratatui::style::Color;
use std::path::Path;

const BUDGET: usize = 8 * 1024 * 1024;
fn restored(layout: &Arc<TextLayout>) -> io::Result<Arc<TextLayout>> {
    let mut capture = Capture::default();
    let id = capture.intern(layout)?;
    Restored::decode(&capture.encode(BUDGET)?, BUDGET)?.layout(id)
}
fn rendered(layout: &TextLayout) -> io::Result<Buffer> {
    let area = Rect::new(
        0,
        0,
        layout.width,
        u16::try_from(layout.rows.len()).map_err(io::Error::other)?,
    );
    let mut buffer = Buffer::empty(area);
    layout.render(area, &mut buffer, 0);
    Ok(buffer)
}
fn payload(layout: &TextLayout) -> String {
    let mut lines = Vec::new();
    layout.copy_lines(0..layout.text.len(), "", &mut lines);
    crate::markdown_copy::selection(&lines, &layout.text).0
}

#[test]
fn exact_pinned_rows_tabs_disclosure_styles_and_hit_tests_round_trip() -> io::Result<()> {
    let style = Style::default()
        .fg(crate::terminal_palette::rgb_color((1, 2, 3)))
        .bg(crate::terminal_palette::indexed_color(144))
        .add_modifier(Modifier::BOLD | Modifier::ITALIC);
    assert!(matches!(style.fg, Some(Color::Rgb(1, 2, 3))));
    assert!(matches!(style.bg, Some(Color::Indexed(144))));
    let line = HyperlinkLine::new(Line::from(vec![ratatui::text::Span::styled(
        "a\t界 café\tend",
        style,
    )]));
    let actual = Arc::new(
        TextLayout::new(vec![line], 12)
            .with_turn_disclosure("Details".into())
            .with_leading_separator(),
    );
    let recovered = restored(&actual)?;
    assert_eq!(actual.text, recovered.text);
    assert_eq!(rendered(&actual)?, rendered(&recovered)?);
    assert_eq!(actual.disclosure_row(), recovered.disclosure_row());
    assert_eq!(actual.disclosure_columns(), recovered.disclosure_columns());
    for row in 0..actual.rows.len() {
        for column in 0..actual.width {
            assert_eq!(
                actual.position_at(row, column),
                recovered.position_at(row, column)
            );
            assert_eq!(
                actual.cursor_position_at(row, column),
                recovered.cursor_position_at(row, column)
            );
        }
    }
    for offset in 0..=actual.text.len() {
        assert_eq!(
            actual.row_for_offset(offset),
            recovered.row_for_offset(offset)
        );
        assert_eq!(
            actual.column_for_offset(offset),
            recovered.column_for_offset(offset)
        );
    }
    // Adjacent tabs can have equal unexpanded columns: keep their original stops.
    let adjacent = Arc::new(TextLayout::new(
        vec![HyperlinkLine::from(Line::from("\t\tx"))],
        24,
    ));
    let after = restored(&adjacent)?;
    assert_eq!(rendered(&adjacent)?, rendered(&after)?);
    Ok(())
}

#[test]
fn shared_sources_copies_styles_and_equal_distinct_arcs_keep_identity() -> io::Result<()> {
    let mut source = LogicalLineSource::new("same source".into());
    let code: Arc<str> = String::from("same source block").into();
    let mut copy = CopyLine::default();
    copy.code_source = Some(code.clone());
    copy.quote_source = Some(code);
    source.copy = Some(Arc::new(copy));
    let mut one = HyperlinkLine::from(Line::from("same source"));
    one.source = Some(source);
    let mut separate = one.clone();
    separate
        .source
        .as_mut()
        .ok_or_else(|| native::invalid("missing actual source"))?
        .text = String::from("same source").into();
    let first = Arc::new(TextLayout::new(vec![one.clone()], 40));
    let same = Arc::new(TextLayout::new(vec![one.clone()], 5));
    let equal_but_distinct = Arc::new(TextLayout::new(vec![separate], 40));
    let mut capture = Capture::default();
    // A non-layout whole-App line joins the SAME graph before any layout.
    let external = capture.lines().line(&one)?;
    let a = capture.intern(&first)?;
    assert_eq!(capture.intern(&first)?, a);
    let b = capture.intern(&same)?;
    let c = capture.intern(&equal_but_distinct)?;
    assert_ne!(a, b);
    let mut recovered = Restored::decode(&capture.encode(BUDGET)?, BUDGET)?;
    let a1 = recovered.layout(a)?;
    assert!(Arc::ptr_eq(&a1, &recovered.layout(a)?));
    let b1 = recovered.layout(b)?;
    let c1 = recovered.layout(c)?;
    let external = recovered
        .lines()
        .line(&external)?
        .source
        .ok_or_else(|| native::invalid("missing actual source"))?;
    let a_source = &a1.logical[0].origin;
    let b_source = &b1.logical[0].origin;
    let c_source = &c1.logical[0].origin;
    assert!(Arc::ptr_eq(&a_source.text, &external.text));
    assert!(Arc::ptr_eq(&a_source.text, &b_source.text));
    assert!(!Arc::ptr_eq(&a_source.text, &c_source.text));
    assert!(Arc::ptr_eq(&a_source.styles, &b_source.styles));
    let copy = a_source
        .copy
        .as_ref()
        .ok_or_else(|| native::invalid("missing actual copy"))?;
    assert!(Arc::ptr_eq(
        copy,
        b_source
            .copy
            .as_ref()
            .ok_or_else(|| native::invalid("missing actual copy"))?
    ));
    assert!(Arc::ptr_eq(
        copy.code_source
            .as_ref()
            .ok_or_else(|| native::invalid("missing actual code source"))?,
        copy.quote_source
            .as_ref()
            .ok_or_else(|| native::invalid("missing actual quote source"))?
    ));
    Ok(())
}

#[test]
fn original_markdown_table_labelled_copy_and_block_separation_survive() -> io::Result<()> {
    let cell = AgentMarkdownCell::new(
        "> **quoted** text\n>\n> ```rust\n> let x = 1;\n> ```\n\n| Left | Right |\n| :--- | ---: |\n| a `code` | **b** |\n\n```txt\nsame\n```\n\n```txt\nsame\n```".into(),
        Path::new("/"),
    );
    let actual = Arc::new(TextLayout::new(
        cell.retained_hyperlink_lines(22, false),
        22,
    ));
    let recovered = restored(&actual)?;
    assert_eq!(actual.text, recovered.text);
    assert_eq!(payload(&actual), payload(&recovered));
    assert_eq!(actual.copy_block_ranges(), recovered.copy_block_ranges());
    assert_eq!(rendered(&actual)?, rendered(&recovered)?);
    // The held layout's style/source revision, rather than a fresh renderer, supplies reflow.
    let later_actual = actual.rewrap(9);
    let later_recovered = recovered.rewrap(9);
    assert_eq!(rendered(&later_actual)?, rendered(&later_recovered)?);
    assert_eq!(payload(&later_actual), payload(&later_recovered));
    Ok(())
}

#[test]
fn clipped_plaintext_retains_exact_viewport_rows_instead_of_reflow() -> io::Result<()> {
    let actual = Arc::new(
        TextLayout::new(
            vec![HyperlinkLine::from(Line::from(
                "first line\twrap and more words",
            ))],
            8,
        )
        .visible_plaintext(1, 2),
    );
    let recovered = restored(&actual)?;
    assert_eq!(actual.text, recovered.text);
    assert_eq!(actual.rows.len(), 2);
    assert_eq!(rendered(&actual)?, rendered(&recovered)?);
    assert_eq!(payload(&actual), payload(&recovered));
    Ok(())
}

#[test]
fn exact_image_raster_protocol_and_shared_alias_survive_without_file_reload() -> io::Result<()> {
    let image = Arc::new(TranscriptImage {
        protocol: ImageProtocol::KittyLocalFile,
        columns: 2,
        rows: 1,
        pixels: RgbaImage::from_raw(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8])
            .ok_or_else(|| native::invalid("test raster unavailable"))?,
        cell_width: 8,
        cell_height: 16,
    });
    let other = Arc::new(TranscriptImage {
        protocol: image.protocol,
        columns: 1,
        rows: 1,
        pixels: RgbaImage::from_pixel(1, 1, image::Rgba([4, 3, 2, 1])),
        cell_width: 8,
        cell_height: 16,
    });
    let mut first = TextLayout::new(vec![HyperlinkLine::from(Line::from("image"))], 4);
    first.images = vec![image.clone(), other.clone(), image.clone()];
    let mut second = TextLayout::new(Vec::new(), 4);
    second.images.push(other);
    let mut capture = Capture::default();
    let external = capture.lines().image(&image)?;
    let a = capture.intern(&Arc::new(first))?;
    let b = capture.intern(&Arc::new(second))?;
    let c = capture.intern(&Arc::new(TextLayout::new(Vec::new(), 4)))?;
    let bytes = capture.encode(BUDGET)?;
    let mut wire: serde_json::Value = serde_json::from_slice(&bytes).map_err(native::invalid)?;
    // Empty and single-image records retain the exact original nullable field.
    assert_eq!(wire["layouts"][1]["image"], serde_json::json!(1));
    assert_eq!(wire["layouts"][2]["image"], serde_json::Value::Null);
    for index in [1, 2] {
        assert!(wire["layouts"][index].get("additionalImages").is_none());
    }
    let mut recovered = Restored::decode(&bytes, BUDGET)?;
    let a = recovered.layout(a)?;
    let b = recovered.layout(b)?;
    assert!(recovered.layout(c)?.images.is_empty());
    assert_eq!(a.images.len(), 3);
    assert_eq!(b.images.len(), 1);
    let a_image = &a.images[0];
    assert!(Arc::ptr_eq(a_image, &recovered.lines().image(external)?));
    assert!(Arc::ptr_eq(a_image, &a.images[2]));
    assert!(Arc::ptr_eq(&a.images[1], &b.images[0]));
    assert_eq!(a.images[1].pixels.as_raw(), &[4, 3, 2, 1]);

    wire["layouts"][0]["additionalImages"][1] = serde_json::json!(99999);
    assert!(
        Restored::decode(&serde_json::to_vec(&wire).map_err(native::invalid)?, BUDGET).is_err()
    );
    assert_eq!(a_image.protocol, ImageProtocol::KittyLocalFile);
    assert_eq!(a_image.pixels.as_raw(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!((a_image.cell_width, a_image.cell_height), (8, 16));
    // ImagePlacement.file and any active terminal placement are deliberately not created.
    Ok(())
}

#[test]
fn malformed_required_fields_aliases_ranges_tabs_and_images_refuse() -> io::Result<()> {
    let actual = Arc::new(TextLayout::new(
        vec![HyperlinkLine::from(Line::from("a\tb"))],
        10,
    ));
    let mut capture = Capture::default();
    capture.intern(&actual)?;
    let bytes = capture.encode(BUDGET)?;
    let raw: serde_json::Value = serde_json::from_slice(&bytes).map_err(native::invalid)?;
    let mut missing = raw.clone();
    missing["layouts"][0]
        .as_object_mut()
        .ok_or_else(|| native::invalid("test object unavailable"))?
        .remove("image");
    assert!(
        Restored::decode(
            &serde_json::to_vec(&missing).map_err(native::invalid)?,
            BUDGET
        )
        .is_err()
    );
    let mut unknown = raw.clone();
    unknown["layouts"][0]["logical"][0]["origin"]["text"] = serde_json::json!(999999);
    assert!(
        Restored::decode(
            &serde_json::to_vec(&unknown).map_err(native::invalid)?,
            BUDGET
        )
        .is_err()
    );
    let mut range = raw.clone();
    range["layouts"][0]["rows"][0]["source"]["end"] = serde_json::json!(99999);
    assert!(
        Restored::decode(
            &serde_json::to_vec(&range).map_err(native::invalid)?,
            BUDGET
        )
        .is_err()
    );
    let mut underflow = raw.clone();
    underflow["layouts"][0]["rows"][0]["firstColumn"] = serde_json::json!(0);
    underflow["layouts"][0]["rows"][0]["prefixColumns"] = serde_json::json!(1);
    assert!(
        Restored::decode(
            &serde_json::to_vec(&underflow).map_err(native::invalid)?,
            BUDGET
        )
        .is_err()
    );
    let mut bad_prefix = raw.clone();
    let mut source = bad_prefix["layouts"][0]["logical"][0]["origin"].clone();
    source["range"] = serde_json::json!({"start":0,"end":1});
    source["prefix"] = serde_json::json!(1);
    let line = &mut bad_prefix["layouts"][0]["logical"][0]["line"];
    line["line"]["spans"][0]["text"] = serde_json::json!("éx");
    line["source"] = source;
    assert!(
        Restored::decode(
            &serde_json::to_vec(&bad_prefix).map_err(native::invalid)?,
            BUDGET
        )
        .is_err()
    );
    let mut tab = raw;
    tab["layouts"][0]["rows"][0]["tabs"][0]["width"] = serde_json::json!(0);
    assert!(Restored::decode(&serde_json::to_vec(&tab).map_err(native::invalid)?, BUDGET).is_err());
    assert!(Restored::decode(&bytes, bytes.len() - 1).is_err());
    assert!(capture.encode(0).is_err());
    let invalid = Arc::new(TranscriptImage {
        protocol: ImageProtocol::Sixel,
        columns: 1,
        rows: 1,
        pixels: RgbaImage::new(0, 0),
        cell_width: 8,
        cell_height: 16,
    });
    assert!(Capture::default().lines().image(&invalid).is_err());
    Ok(())
}

#[test]
fn original_unicode_label_uses_body_copy_coordinates_and_shared_label_arc() -> io::Result<()> {
    let cell = AgentMarkdownCell::new("**ab**".into(), Path::new("/"));
    let mut lines = crate::terminal_hyperlinks::prefix_hyperlink_lines(
        cell.retained_hyperlink_lines(40, false),
        ratatui::text::Span::raw("界 "),
        ratatui::text::Span::raw("   "),
    );
    crate::terminal_hyperlinks::retain_initial_prefix(&mut lines, "界 ");
    let first = Arc::new(TextLayout::new(lines.clone(), 40));
    let second = Arc::new(TextLayout::new(lines, 5));
    assert!(first.logical[0].origin.labelled_body.is_some());
    let mut capture = Capture::default();
    let a = capture.intern(&first)?;
    let b = capture.intern(&second)?;
    let recovered = Restored::decode(&capture.encode(BUDGET)?, BUDGET)?;
    let a = recovered.layout(a)?;
    let b = recovered.layout(b)?;
    assert_eq!(payload(&first), payload(&a));
    assert_eq!(rendered(&first)?, rendered(&a)?);
    let a_label = a.logical[0]
        .origin
        .labelled_body
        .as_ref()
        .ok_or_else(|| native::invalid("missing native label"))?;
    let b_label = b.logical[0]
        .origin
        .labelled_body
        .as_ref()
        .ok_or_else(|| native::invalid("missing native label"))?;
    assert!(Arc::ptr_eq(a_label, b_label));
    Ok(())
}

#[test]
fn impossible_stored_inline_marker_and_cyclic_source_graph_refuse() -> io::Result<()> {
    let mut copy = CopyLine::default();
    copy.push(1, &[crate::markdown_copy::Inline::Code]);
    let mut source = LogicalLineSource::new("a".into());
    source.copy = Some(Arc::new(copy));
    let mut line = HyperlinkLine::from(Line::from("a"));
    line.source = Some(source);
    let actual = Arc::new(TextLayout::new(vec![line], 10));
    let mut capture = Capture::default();
    capture.intern(&actual)?;
    let raw: serde_json::Value =
        serde_json::from_slice(&capture.encode(BUDGET)?).map_err(native::invalid)?;
    let mut impossible = raw.clone();
    impossible["native"]["copies"][0]["runs"][0][1] = serde_json::json!([{ "kind":"ignored" }]);
    assert!(
        Restored::decode(
            &serde_json::to_vec(&impossible).map_err(native::invalid)?,
            BUDGET
        )
        .is_err()
    );
    let mut cycle = raw;
    let source = cycle["layouts"][0]["logical"][0]["origin"].clone();
    cycle["native"]["tables"] = serde_json::json!([["---"]]);
    cycle["native"]["copies"][0]["table"] = serde_json::json!({
        "table":0,"fragments":[{"output":{"start":0,"end":1},"row":0,"column":0,"source":source}]
    });
    assert!(
        Restored::decode(
            &serde_json::to_vec(&cycle).map_err(native::invalid)?,
            BUDGET
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn actual_by_value_layout_keeps_same_global_sources_and_copy_arcs_without_a_fake_arc()
-> io::Result<()> {
    let mut source = LogicalLineSource::new("exact original warning/source".into());
    let mut copy = CopyLine::default();
    copy.code_source = Some(source.text.clone());
    copy.quote_source = Some(source.text.clone());
    source.copy = Some(Arc::new(copy));
    let mut line = HyperlinkLine::from(Line::from("exact original warning/source"));
    line.source = Some(source);
    let unique = TextLayout::new(vec![line.clone()], 7);
    let shared = Arc::new(TextLayout::new(vec![line], 40));
    let mut capture = Capture::default();
    let unique_data = capture.unique_layout(&unique)?;
    let reference = capture.intern(&shared)?;
    let wire = serde_json::to_vec(&unique_data).map_err(native::invalid)?;
    let unique_data: UniqueLayout = serde_json::from_slice(&wire).map_err(native::invalid)?;
    let mut restored = Restored::decode(&capture.encode(BUDGET)?, BUDGET)?;
    let shared = restored.layout(reference)?;
    let unique_restored = restored.restore_unique_layout(unique_data)?;
    assert_eq!(unique_restored.text, unique.text);
    assert_eq!(rendered(&unique_restored)?, rendered(&unique)?);
    let a = &unique_restored.logical[0].origin;
    let b = &shared.logical[0].origin;
    assert!(Arc::ptr_eq(&a.text, &b.text));
    assert!(Arc::ptr_eq(&a.styles, &b.styles));
    assert!(Arc::ptr_eq(
        a.copy
            .as_ref()
            .ok_or_else(|| native::invalid("unique original copy missing"))?,
        b.copy
            .as_ref()
            .ok_or_else(|| native::invalid("shared original copy missing"))?,
    ));
    Ok(())
}

#[test]
fn by_value_layout_requires_the_complete_nested_native_optional_shape() -> io::Result<()> {
    let actual = TextLayout::new(
        vec![HyperlinkLine::from(Line::from("actual native layout"))],
        12,
    );
    let mut graph = Capture::default();
    let data = graph.unique_layout(&actual)?;
    let original = serde_json::to_value(data).map_err(native::invalid)?;
    for field in ["image", "control"] {
        let mut value = original.clone();
        value
            .as_object_mut()
            .ok_or_else(|| native::invalid("actual layout not object"))?
            .remove(field);
        assert!(serde_json::from_value::<UniqueLayout>(value).is_err());
    }
    let mut value = original;
    value["logical"][0]["line"]
        .as_object_mut()
        .ok_or_else(|| native::invalid("actual native logical line not object"))?
        .remove("source");
    assert!(serde_json::from_value::<UniqueLayout>(value).is_err());
    Ok(())
}
