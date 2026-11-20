// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::history_cell::HistoryRenderMode;
use crate::pets::ImageProtocol;
use crate::thread_transcript::RawReasoningVisibility;
use crate::thread_transcript::thread_items_to_transcript_cells;
use crate::transcript_view::TranscriptView;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use codex_app_server_protocol::ImageReference;
use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::UserInput;
use codex_utils_absolute_path::AbsolutePathBuf;
use image::Rgba;
use image::RgbaImage;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn pasted_and_replayed_images_keep_their_transcript_thumbnails() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("pasted.png");
    let red = Rgba([240, 16, 8, 255]);
    let blue = Rgba([12, 32, 232, 255]);
    RgbaImage::from_pixel(96, 96, red).save(&path)?;
    let encoded = STANDARD.encode(std::fs::read(&path)?);
    RgbaImage::from_pixel(96, 96, blue).save(&path)?;
    let inline = |url| UserInput::Image {
        image: ImageReference::Inline { url },
        detail: None,
    };
    let cells = thread_items_to_transcript_cells(
        None,
        &AbsolutePathBuf::try_from(dir.path().to_owned())?,
        [ThreadItemPayload::UserMessage {
            id: "message".into(),
            client_id: None,
            content: vec![
                inline(format!("data:image/png;base64,{encoded}")),
                inline("data:image/png;base64,broken".into()),
                UserInput::LocalImage { path, detail: None },
            ],
        }],
        RawReasoningVisibility::Hidden,
        None,
    );
    use codex_alab_tui::interface;
    let _scope = interface::scoped(Box::leak(Box::new(interface::State::new())));
    interface::set_enabled(true);
    let mut view = TranscriptView::default();
    let area = Rect::new(0, 0, 60, 24);
    view.render(area, &mut Buffer::empty(area), &cells);
    let text_only = view.layout(&cells, 0).unwrap();
    assert!(view.image_placements().is_empty());
    for label in ["[Image #1]", "[Image #2]", "[Image #3]"] {
        assert!(text_only.text().contains(label));
    }

    view.image_protocol = Some(ImageProtocol::Kitty);
    view.render(area, &mut Buffer::empty(area), &cells);
    let layout = view.layout(&cells, 0).unwrap();
    let placements = view.image_placements();
    assert_eq!(placements.len(), 2);
    let first = &placements[0];
    let last = &placements[1];
    assert_eq!(first.image.pixels.get_pixel(0, 0), &red);
    assert_eq!(last.image.pixels.get_pixel(0, 0), &blue);
    assert!(first.area.bottom() <= last.area.y);
    assert_eq!(layout.text(), text_only.text());
    assert_eq!(
        layout.row_count() - text_only.row_count(),
        usize::from(first.area.height + last.area.height)
    );

    // At the bottom of a short viewport only the tail of the final image remains visible.
    let short = Rect::new(2, 3, 60, 2);
    view.render(short, &mut Buffer::empty(short), &cells);
    let clipped = view.image_placements();
    assert_eq!(clipped.len(), 1);
    assert_eq!(clipped[0].image.pixels.get_pixel(0, 0), &blue);
    assert!(clipped[0].first_row > 0);
    assert_eq!(clipped[0].area.y, short.y);
    assert_eq!(clipped[0].area.bottom(), short.bottom());

    interface::set_enabled(false);
    view.render(area, &mut Buffer::empty(area), &cells);
    assert!(view.image_placements().is_empty());
    interface::set_enabled(true);
    view.render(area, &mut Buffer::empty(area), &cells);
    assert_eq!(view.image_placements().len(), 2);

    view.set_presentation(/*detailed*/ false, HistoryRenderMode::Raw);
    view.render(area, &mut Buffer::empty(area), &cells);
    assert!(view.image_placements().is_empty());
    assert!(
        view.layout(&cells, 0)
            .unwrap()
            .text()
            .contains("[Image #3]")
    );
    Ok(())
}
