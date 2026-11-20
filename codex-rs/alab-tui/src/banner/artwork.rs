// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The live launch card uses Kitty virtual placements so each cell addresses its
//! own tile. Inline history preserves the same pixels as anonymous one-row images.

use super::Banner;
use super::Depth;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Span;
use std::io;
use std::io::Write;

struct Art {
    id: u32,
    cols: usize,
    png: &'static [u8],
    history_rows: &'static [&'static [u8]],
}

macro_rules! history_rows {
    ($name:literal, $($row:literal),+) => {
        &[$(include_bytes!(concat!(env!("OUT_DIR"), "/", $name, "-", $row, ".png"))),+]
    };
}

const MARK: Art = Art {
    id: 0xa1ab01,
    cols: super::codex_icon::WIDTH,
    png: include_bytes!("assets/codex-mark.png"),
    history_rows: history_rows!("codex-mark", 0, 1, 2, 3, 4),
};

const COMPACT_MARK: Art = Art {
    id: 0xa1ab03,
    cols: 6,
    png: MARK.png,
    history_rows: history_rows!("codex-mark-compact", 0, 1, 2),
};

// Flying tabby adapted from aterm's Apache-2.0 pet_fly.toml at 802199655ee,
// Copyright 2026 Andrew Yates. The six-band trail is drawn for this aspect ratio.
const KITTY: Art = Art {
    id: 0xa1ab02,
    cols: 22,
    png: include_bytes!("assets/rainbow-kitty.png"),
    history_rows: history_rows!("rainbow-kitty", 0, 1, 2, 3),
};

const COMPACT_KITTY: Art = Art {
    id: 0xa1ab04,
    cols: 11,
    png: KITTY.png,
    history_rows: history_rows!("rainbow-kitty-compact", 0, 1),
};

const TITLE_MARK: Art = Art {
    id: 0xa1ab05,
    cols: 4,
    png: MARK.png,
    history_rows: history_rows!("codex-mark-title", 0, 1),
};

const ARTWORK: [&Art; 5] = [&MARK, &KITTY, &COMPACT_MARK, &COMPACT_KITTY, &TITLE_MARK];
pub(super) const TITLE_MARK_WIDTH: usize = TITLE_MARK.cols;
pub(super) const TITLE_MARK_HEIGHT: usize = TITLE_MARK.history_rows.len();
pub(super) const COMPACT_MARK_WIDTH: usize = COMPACT_MARK.cols;
pub(super) const COMPACT_MARK_HEIGHT: usize = COMPACT_MARK.history_rows.len();

/// One cell of padding on either side of the flying tabby.
pub(super) const KITTY_COLUMN_WIDTH: usize = KITTY.cols + 2;
pub(super) const KITTY_HEIGHT: usize = KITTY.history_rows.len();
pub(super) const COMPACT_KITTY_COLUMN_WIDTH: usize = COMPACT_KITTY.cols + 2;
pub(super) const COMPACT_KITTY_HEIGHT: usize = COMPACT_KITTY.history_rows.len();

// First 22 values of the Kitty graphics protocol's rowcolumn-diacritics table.
const COORDS: [char; 22] = [
    '\u{0305}', '\u{030D}', '\u{030E}', '\u{0310}', '\u{0312}', '\u{033D}', '\u{033E}', '\u{033F}',
    '\u{0346}', '\u{034A}', '\u{034B}', '\u{034C}', '\u{0350}', '\u{0351}', '\u{0352}', '\u{0357}',
    '\u{035B}', '\u{0363}', '\u{0364}', '\u{0365}', '\u{0366}', '\u{0367}',
];

fn native_placements_supported() -> bool {
    // Scaled U=1 placements need the same aterm version as stored pet frames.
    crate::pets::kitty::on() && crate::direct_aterm()
}

pub(super) fn native_graphics(banner: &Banner) -> bool {
    banner.depth == Depth::TrueColor && native_placements_supported()
}

impl Art {
    fn row(&self, row: usize) -> Span<'static> {
        let mut text = String::new();
        for col in &COORDS[..self.cols] {
            text.extend(['\u{10eeee}', COORDS[row], *col]);
        }
        let [_, r, g, b] = self.id.to_be_bytes();
        #[expect(
            clippy::disallowed_methods,
            reason = "the Kitty protocol encodes the image id in the foreground RGB bytes"
        )]
        Span::styled(text, Style::default().fg(Color::Rgb(r, g, b)))
    }

    fn native_row(&self, banner: &Banner, row: usize) -> Option<Span<'static>> {
        if row < self.history_rows.len() && native_graphics(banner) {
            Some(self.row(row))
        } else {
            None
        }
    }

    fn transmit(&self, out: &mut impl Write) -> io::Result<()> {
        write!(
            out,
            "\x1b_Ga=T,t=d,f=100,U=1,i={},c={},r={},C=1,q=2,",
            self.id,
            self.cols,
            self.history_rows.len()
        )?;
        write_payload(out, self.png)
    }
}

fn write_payload(out: &mut impl Write, png: &[u8]) -> io::Result<()> {
    let payload = STANDARD.encode(png);
    let mut chunks = payload.as_bytes().chunks(4096).peekable();
    while let Some(chunk) = chunks.next() {
        let more = chunks.peek().is_some();
        write!(out, "m={};", u8::from(more))?;
        out.write_all(chunk)?;
        out.write_all(b"\x1b\\")?;
        if more {
            out.write_all(b"\x1b_G")?;
        }
    }
    Ok(())
}

/// Write a banner image row directly to terminal-native scrollback. aterm archives
/// cursor placements as pixels. Its virtual placeholder tiles are resolved only
/// on the live screen. An anonymous row avoids binding archived pixels to a
/// stored id that a later session or a screen restore can replace.
pub fn write_history_span(out: &mut impl Write, span: &Span<'_>) -> io::Result<bool> {
    if !span.content.starts_with('\u{10eeee}') || !native_placements_supported() {
        return Ok(false);
    }
    write_history_tile(out, span)
}

fn image_row(span: &Span<'_>) -> Option<(&'static Art, usize)> {
    let Some(Color::Rgb(r, g, b)) = span.style.fg else {
        return None;
    };
    let id = u32::from_be_bytes([0, r, g, b]);
    let art = ARTWORK.into_iter().find(|art| art.id == id)?;
    let mark = span.content.chars().nth(1)?;
    let row = COORDS[..art.history_rows.len()]
        .iter()
        .position(|&c| c == mark)?;
    (span.content == art.row(row).content).then_some((art, row))
}

/// Cell count of a complete registered artwork span, also valid for a frozen transcript.
pub fn image_columns(span: &Span<'_>) -> Option<usize> {
    image_row(span).map(|(art, _)| art.cols)
}

fn write_history_tile(out: &mut impl Write, span: &Span<'_>) -> io::Result<bool> {
    let Some((art, row)) = image_row(span) else {
        return Ok(false);
    };
    write!(out, "\x1b_Ga=T,t=d,f=100,c={},r=1,C=1,q=2,", art.cols)?;
    write_payload(out, art.history_rows[row])?;
    write!(out, "\x1b[{}C", art.cols)?;
    Ok(true)
}

/// The launch images already registered or encountered in a terminal buffer.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ImageSet([bool; ARTWORK.len()]);

impl ImageSet {
    const ALL: Self = Self([true; ARTWORK.len()]);

    /// Include images referenced by these cells, stopping once all are known.
    pub fn with_tiles<'a>(mut self, cells: impl IntoIterator<Item = (&'a str, Color)>) -> Self {
        for (symbol, color) in cells {
            let Color::Rgb(r, g, b) = color else {
                continue;
            };
            if symbol.starts_with('\u{10eeee}') {
                let id = u32::from_be_bytes([0, r, g, b]);
                if let Some(index) = ARTWORK.iter().position(|art| art.id == id) {
                    self.0[index] = true;
                    if self == Self::ALL {
                        break;
                    }
                }
            }
        }
        self
    }
}

/// Image registrations for one terminal writer.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct Upload {
    epoch: Option<u64>,
    registered: ImageSet,
    retry_full_scan: bool,
}

impl Upload {
    /// Register graphics only when there are tiles to draw. At startup or after
    /// a possible store loss, inspect all visible cells, including a restored
    /// previous buffer. Otherwise only newly drawn cells can introduce artwork.
    pub fn prepare_visible(
        &mut self,
        out: &mut impl Write,
        visible: impl FnOnce(bool, ImageSet) -> ImageSet,
    ) -> io::Result<()> {
        if native_placements_supported() {
            self.prepare_epoch(out, crate::pets::kitty::epoch(), visible)?;
        }
        Ok(())
    }

    fn prepare_epoch(
        &mut self,
        out: &mut impl Write,
        epoch: u64,
        visible: impl FnOnce(bool, ImageSet) -> ImageSet,
    ) -> io::Result<()> {
        let new_epoch = self.epoch.replace(epoch) != Some(epoch);
        if new_epoch {
            self.registered = ImageSet::default();
        }
        if self.registered == ImageSet::ALL {
            return Ok(());
        }
        let needed = visible(new_epoch || self.retry_full_scan, self.registered);
        // After a partial write an unchanged buffer can still contain missing
        // pixels. Check the full buffer again and keep each completed registration.
        self.retry_full_scan = true;
        for (index, art) in ARTWORK.iter().enumerate() {
            if needed.0[index] && !self.registered.0[index] {
                art.transmit(out)?;
                self.registered.0[index] = true;
            }
        }
        self.retry_full_scan = false;
        Ok(())
    }
}

pub(super) fn title_mark(row: usize) -> Span<'static> {
    TITLE_MARK.row(row)
}

pub(super) fn native_compact_mark(banner: &Banner, row: usize) -> Option<Span<'static>> {
    COMPACT_MARK.native_row(banner, row)
}

pub(super) fn native_mark(banner: &Banner, row: usize) -> Option<Span<'static>> {
    MARK.native_row(banner, row)
}

pub(super) fn kitty(banner: &Banner, width: usize, row: usize) -> Vec<Span<'static>> {
    // Keep the cell drawing and native image on the same rows and columns.
    const COMPACT: [(&str, &str); COMPACT_KITTY_HEIGHT] =
        [("  ✧   ", "/\\_/\\"), ("▂▃▄▅▆▇", "(•▾•)")];
    const FULL: [(&str, &str); KITTY_HEIGHT] = [
        ("      ✧   ", "   /\\___/\\  "),
        ("          ", "  (  • •  ) "),
        ("  ▂▃▄▅▆▇██", " ≋(   ▾   )≋"),
        ("▂▃▄▅▆▇██▀ ", "    ╰─┴─╯   "),
    ];
    let (art, text) = if width == COMPACT_KITTY_COLUMN_WIDTH {
        (&COMPACT_KITTY, COMPACT.as_slice())
    } else {
        (&KITTY, FULL.as_slice())
    };
    if width < art.cols || row >= text.len() {
        return vec![Span::raw(" ".repeat(width))];
    }
    let pad = width - art.cols;
    let mut spans = vec![Span::raw(" ".repeat(pad / 2))];
    if let Some(native) = art.native_row(banner, row) {
        spans.push(native);
    } else {
        let (trail, cat) = text[row];
        let trail_cells = trail.chars().count();
        for (x, ch) in trail.chars().enumerate() {
            let style = if banner.depth == Depth::Mono {
                Style::default()
            } else {
                let colors = [
                    Color::Red,
                    Color::Yellow,
                    Color::Green,
                    Color::Cyan,
                    Color::Blue,
                    Color::Magenta,
                ];
                let rgb = super::palette::spectrum(x as f32 / (trail_cells - 1) as f32);
                Style::default().fg(super::readable_text(
                    rgb,
                    colors[x * colors.len() / trail_cells],
                    banner.depth,
                    banner.theme,
                ))
            };
            spans.push(Span::styled(ch.to_string(), style));
        }
        // On a light ground, ANSI yellow can wash out the entire silhouette.
        // The terminal's configured text ink keeps its eyes and whiskers readable.
        let ink = if let Some(ink) = banner.theme.unreported_foreground(banner.depth) {
            ink
        } else if banner.depth == Depth::Mono
            || (banner.depth == Depth::Ansi16 && banner.theme.is_light())
        {
            Style::default()
        } else {
            Style::default().fg(super::readable_text(
                0xf0bd93,
                Color::Yellow,
                banner.depth,
                banner.theme,
            ))
        };
        spans.push(Span::styled(cat, ink));
    }
    spans.push(Span::raw(" ".repeat(pad - pad / 2)));
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_text_kitty_fits_two_rows_with_six_ribbon_colors() {
        use super::super::Style as BannerStyle;
        use super::super::Theme;
        use ratatui::text::Line;

        for depth in [Depth::Ansi16, Depth::Mono] {
            let banner = Banner::new(
                BannerStyle::Codex,
                depth,
                Theme::from_bg(Some((255, 255, 255))),
                "0.11",
            );
            for (row, expected) in ["   ✧   /\\_/\\ ", " ▂▃▄▅▆▇(•▾•) ", "             "]
                .into_iter()
                .enumerate()
            {
                let line = Line::from(kitty(&banner, COMPACT_KITTY_COLUMN_WIDTH, row));
                assert_eq!(line.width(), COMPACT_KITTY_COLUMN_WIDTH);
                assert_eq!(line.to_string(), expected);
                if depth == Depth::Mono {
                    assert!(line.spans.iter().all(|span| span.style == Style::default()));
                } else if row == 1 {
                    let colors = [
                        Color::Red,
                        Color::Yellow,
                        Color::Green,
                        Color::Cyan,
                        Color::Blue,
                        Color::Magenta,
                    ];
                    assert_eq!(
                        line.spans
                            .iter()
                            .filter_map(|span| span.style.fg)
                            .collect::<Vec<_>>(),
                        colors
                    );
                }
            }
        }
    }

    #[test]
    fn unreported_foreground_changes_only_the_text_kitty_silhouette() {
        use super::super::Style as BannerStyle;
        use super::super::Theme;

        for depth in [Depth::Mono, Depth::Ansi16, Depth::Ansi256] {
            let original = Banner::new(BannerStyle::Codex, depth, Theme::default(), "0.11");
            let fallback = Banner::new(
                BannerStyle::Codex,
                depth,
                Theme::default().with_terminal_foreground_if_unreported(),
                "0.11",
            );
            for (width, row) in [(COMPACT_KITTY_COLUMN_WIDTH, 1), (KITTY_COLUMN_WIDTH, 2)] {
                let mut expected = kitty(&original, width, row);
                let face = expected
                    .iter_mut()
                    .find(|span| span.content.contains('▾'))
                    .unwrap();
                face.style = if depth == Depth::Mono {
                    Style::default().not_dim()
                } else {
                    Style::default().fg(Color::Reset).not_dim()
                };
                assert_eq!(kitty(&fallback, width, row), expected, "{width}, {depth:?}");
            }
        }
    }

    #[test]
    fn light_ansi_kitty_inherits_text_ink_while_the_trail_keeps_its_colors() {
        use super::super::Style as BannerStyle;
        use super::super::Theme;

        let light = Theme::from_bg(Some((255, 255, 255)));
        let dark = Theme::from_bg(Some((16, 20, 24)));
        for (depth, theme, face_ink) in [
            (Depth::Ansi16, light, None),
            (Depth::Ansi16, dark, Some(Color::Yellow)),
            (Depth::Ansi16, Theme::default(), Some(Color::Yellow)),
            (Depth::Mono, light, None),
            (
                Depth::Ansi256,
                light,
                Some(super::super::readable_text(
                    0xf0bd93,
                    Color::Yellow,
                    Depth::Ansi256,
                    light,
                )),
            ),
        ] {
            let banner = Banner::new(BannerStyle::Codex, depth, theme, "0.11");
            for (row, feature) in ['/', '•', '▾', '╰'].into_iter().enumerate() {
                let spans = kitty(&banner, KITTY.cols, row);
                let face = spans
                    .iter()
                    .find(|span| span.content.contains(feature))
                    .unwrap();
                assert_eq!(face.style.fg, face_ink);
                if row == 2 {
                    let trail = spans.iter().find(|span| span.content == "▂").unwrap();
                    assert_eq!(trail.style.fg.is_some(), depth != Depth::Mono);
                }
            }
        }
    }

    #[test]
    fn explicit_tile_coordinates_keep_every_row_cell_aligned() {
        for art in ARTWORK {
            for (row, coordinate) in COORDS[..art.history_rows.len()].iter().enumerate() {
                let span = art.row(row);
                assert_eq!(span.width(), art.cols);
                assert_eq!(span.content.matches('\u{10eeee}').count(), art.cols);
                for (col, cell) in span.styled_graphemes(Style::default()).enumerate() {
                    assert_eq!(
                        cell.symbol,
                        format!("\u{10eeee}{}{}", coordinate, COORDS[col])
                    );
                }
            }
        }
    }

    fn read_payload<'a>(commands: &mut impl Iterator<Item = &'a str>, header: &str) -> Vec<u8> {
        let mut prefix = header;
        let mut encoded = String::new();
        loop {
            let (control, chunk) = commands.next().unwrap().split_once(';').unwrap();
            assert!(chunk.len() <= 4096);
            encoded.push_str(chunk);
            match control.strip_prefix(prefix).unwrap() {
                "m=0" => return STANDARD.decode(encoded).unwrap(),
                "m=1" => assert_eq!(chunk.len(), 4096),
                other => panic!("unexpected Kitty continuation: {other}"),
            }
            prefix = "\x1b_G";
        }
    }

    fn decode_rgba(png: &[u8]) -> (png::OutputInfo, Option<png::SrgbRenderingIntent>, Vec<u8>) {
        let mut reader = png::Decoder::new(io::Cursor::new(png)).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut pixels).unwrap();
        assert_eq!(frame.color_type, png::ColorType::Rgba);
        assert_eq!(frame.bit_depth, png::BitDepth::Eight);
        pixels.truncate(frame.buffer_size());
        (frame, reader.info().srgb, pixels)
    }

    #[test]
    fn inline_history_places_lossless_anonymous_rows_with_stable_cursor_advance() {
        for (art, width, heights) in [
            (&MARK, 160, &[32; super::super::codex_icon::HEIGHT][..]),
            (&KITTY, 528, &[48; 4][..]),
            (&COMPACT_MARK, 160, &[53, 53, 54][..]),
            (&COMPACT_KITTY, 528, &[96; 2][..]),
            (&TITLE_MARK, 160, &[80; 2][..]),
        ] {
            let (full, intent, pixels) = decode_rgba(art.png);
            assert_eq!((full.width, full.height), (width, heights.iter().sum()));
            assert_eq!(art.history_rows.len(), heights.len());
            let mut restored = Vec::new();
            for (row, &height) in heights.iter().enumerate() {
                let mut bytes = Vec::new();
                assert!(write_history_tile(&mut bytes, &art.row(row)).unwrap());
                let output = std::str::from_utf8(&bytes).unwrap();
                let mut commands = output.split_terminator("\x1b\\");
                let header = format!("\x1b_Ga=T,t=d,f=100,c={},r=1,C=1,q=2,", art.cols);
                let payload = read_payload(&mut commands, &header);
                let (strip, strip_intent, rgba) = decode_rgba(&payload);
                assert_eq!((strip.width, strip.height), (width, height));
                assert_eq!(strip_intent, intent);
                restored.extend(rgba);
                assert_eq!(commands.next().unwrap(), format!("\x1b[{}C", art.cols));
                assert_eq!(commands.next(), None);
            }
            assert!(
                restored == pixels,
                "archived rows must reconstruct the complete source"
            );
        }
    }

    #[test]
    fn inline_history_leaves_ordinary_and_partial_spans_to_the_text_writer() {
        let mut bytes = Vec::new();
        assert!(!write_history_tile(&mut bytes, &Span::raw("ordinary transcript")).unwrap());
        let broken = Span::styled("\u{10eeee}\u{305}", MARK.row(0).style);
        assert!(!write_history_tile(&mut bytes, &broken).unwrap());
        assert!(bytes.is_empty());
    }

    #[test]
    fn uploading_preserves_pngs_across_chunk_boundaries_and_repaints() {
        let mut upload = Upload::default();
        let mut bytes = Vec::new();
        upload
            .prepare_epoch(&mut bytes, 0, |_, _| ImageSet::ALL)
            .unwrap();
        let first = bytes.clone();
        let mut commands = std::str::from_utf8(&bytes)
            .unwrap()
            .split_terminator("\x1b\\");
        for art in ARTWORK {
            let header = format!(
                "\x1b_Ga=T,t=d,f=100,U=1,i={},c={},r={},C=1,q=2,",
                art.id,
                art.cols,
                art.history_rows.len()
            );
            assert_eq!(read_payload(&mut commands, &header), art.png);
        }
        assert_eq!(commands.next(), None);
        bytes.clear();
        upload
            .prepare_epoch(&mut bytes, 0, |_, _| panic!("all registered"))
            .unwrap();
        assert!(bytes.is_empty());
        upload
            .prepare_epoch(&mut bytes, 1, |full, registered| {
                assert!(full);
                assert_eq!(registered, ImageSet::default());
                registered
            })
            .unwrap();
        assert!(
            bytes.is_empty(),
            "do not resend a scrolled-away banner after resize"
        );
        upload
            .prepare_epoch(&mut bytes, 1, |full, _| {
                assert!(!full);
                ImageSet::ALL
            })
            .unwrap();
        assert_eq!(bytes, first);
    }

    #[test]
    fn registers_only_visible_images_and_rescans_after_store_loss() {
        let mut upload = Upload::default();
        let mut bytes = Vec::new();
        let mut mark_bytes = Vec::new();
        let mut kitty_bytes = Vec::new();
        MARK.transmit(&mut mark_bytes).unwrap();
        KITTY.transmit(&mut kitty_bytes).unwrap();
        let mark = MARK.row(0);
        let kitty = KITTY.row(0);
        let compact = COMPACT_MARK.row(0);
        let compact_cell = (compact.content.as_ref(), compact.style.fg.unwrap());
        let mark_cell = (mark.content.as_ref(), mark.style.fg.unwrap());
        let kitty_cell = (kitty.content.as_ref(), kitty.style.fg.unwrap());
        upload
            .prepare_epoch(&mut bytes, 0, |full, registered| {
                assert!(full);
                registered.with_tiles([("ordinary text", kitty_cell.1), mark_cell])
            })
            .unwrap();
        assert_eq!(bytes, mark_bytes);
        bytes.clear();
        upload
            .prepare_epoch(&mut bytes, 0, |full, registered| {
                assert!(!full);
                registered.with_tiles([mark_cell])
            })
            .unwrap();
        assert!(bytes.is_empty());
        upload
            .prepare_epoch(&mut bytes, 0, |full, registered| {
                assert!(!full);
                registered.with_tiles([kitty_cell])
            })
            .unwrap();
        assert_eq!(bytes, kitty_bytes);
        bytes.clear();
        upload
            .prepare_epoch(&mut bytes, 0, |full, registered| {
                assert!(!full);
                registered.with_tiles([compact_cell, mark_cell])
            })
            .unwrap();
        let mut compact_bytes = Vec::new();
        COMPACT_MARK.transmit(&mut compact_bytes).unwrap();
        assert_eq!(bytes, compact_bytes);
        bytes.clear();
        let small = COMPACT_KITTY.row(0);
        let small_cell = (small.content.as_ref(), small.style.fg.unwrap());
        upload
            .prepare_epoch(&mut bytes, 0, |full, registered| {
                assert!(!full);
                registered.with_tiles([small_cell, kitty_cell])
            })
            .unwrap();
        let mut small_bytes = Vec::new();
        COMPACT_KITTY.transmit(&mut small_bytes).unwrap();
        assert_eq!(bytes, small_bytes);
        bytes.clear();
        upload
            .prepare_epoch(&mut bytes, 1, |full, registered| {
                assert!(full);
                registered.with_tiles([mark_cell])
            })
            .unwrap();
        assert_eq!(bytes, mark_bytes);
    }

    #[test]
    fn failed_second_registration_retries_only_the_missing_image() {
        let mut upload = Upload::default();
        let mut mark_bytes = Vec::new();
        MARK.transmit(&mut mark_bytes).unwrap();
        let mut limited = vec![0; mark_bytes.len()];
        assert!(
            upload
                .prepare_epoch(&mut limited.as_mut_slice(), 0, |_, _| ImageSet::ALL)
                .is_err()
        );
        assert_eq!(limited, mark_bytes);
        let mut retried = Vec::new();
        upload
            .prepare_epoch(&mut retried, 0, |full, registered| {
                assert!(full);
                let mark = MARK.row(0);
                assert_eq!(
                    registered,
                    ImageSet::default()
                        .with_tiles([(mark.content.as_ref(), mark.style.fg.unwrap())])
                );
                ImageSet::ALL
            })
            .unwrap();
        let mut missing = Vec::new();
        KITTY.transmit(&mut missing).unwrap();
        COMPACT_MARK.transmit(&mut missing).unwrap();
        COMPACT_KITTY.transmit(&mut missing).unwrap();
        TITLE_MARK.transmit(&mut missing).unwrap();
        assert_eq!(retried, missing);
    }
}
