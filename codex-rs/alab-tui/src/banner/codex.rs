// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The enhanced launch card, with the Codex mark and the rainbow kitty flanking session facts.

use super::Banner;
use super::Depth;
use super::Mode;
use super::Theme;
use super::artwork::COMPACT_KITTY_COLUMN_WIDTH;
use super::artwork::COMPACT_KITTY_HEIGHT;
use super::artwork::COMPACT_MARK_HEIGHT;
use super::artwork::COMPACT_MARK_WIDTH;
use super::artwork::KITTY_COLUMN_WIDTH;
use super::artwork::KITTY_HEIGHT;
use super::artwork::TITLE_MARK_HEIGHT;
use super::artwork::TITLE_MARK_WIDTH;
use crate::model_layout::model_breaks;
use crate::model_layout::painted_width;
use ratatui::buffer::CellWidth;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;

const MAX_CARD_WIDTH: usize = 108;
const CAPTION_WIDTH: usize = 10;

#[derive(Clone, Copy)]
struct FactColumn {
    width: usize,
    beside_mark: bool,
}

impl FactColumn {
    fn fits_caption(self, value_width: usize) -> bool {
        (self.beside_mark || self.width >= 24)
            && self
                .width
                .checked_sub(CAPTION_WIDTH)
                .is_some_and(|space| value_width <= space)
    }
}

pub(super) fn render(banner: &Banner, width: u16, mode: Option<Mode>) -> Vec<Line<'static>> {
    render_session(banner, width, mode, Vec::new())
}

pub(super) fn render_session(
    banner: &Banner,
    width: u16,
    mode: Option<Mode>,
    details: Vec<Line<'static>>,
) -> Vec<Line<'static>> {
    render_session_with_deadline(banner, width, mode, details, None)
}

pub(super) fn render_session_with_deadline(
    banner: &Banner,
    width: u16,
    mode: Option<Mode>,
    details: Vec<Line<'static>>,
    deadline: Option<&str>,
) -> Vec<Line<'static>> {
    if width < 13 {
        return Vec::new();
    }
    let alab = banner.version.as_deref().unwrap_or("dev");
    let upstream = banner.upstream.as_deref().unwrap_or("unreleased");
    let pair = crate::interface::release_pair(alab, upstream);
    let pair_width = painted_width(&pair);
    // Keep the last column free in scrollback. Give the release line room
    // before adding the card's two-space margin, and align every row with it.
    let available = usize::from(width) - 1;
    let margin = "  ";
    let indent = &margin[..available.saturating_sub(pair_width).min(margin.len())];
    let room = (available - indent.len()).min(MAX_CARD_WIDTH);
    // Add the frame only after its padding fits beside the Codex mark and release pair.
    let identity_width = super::codex_icon::WIDTH + 2 + pair_width.max(18);
    let mark = room >= identity_width;
    let framed = room >= identity_width + 4;
    let inner = if framed { room - 4 } else { room };
    let mark_width = if mark {
        super::codex_icon::WIDTH + 2
    } else {
        0
    };
    // Reserve room for the release pair and the facts before admitting the kitty.
    let fact_room = inner - mark_width;
    let reserve_kitty = fact_room >= KITTY_COLUMN_WIDTH + pair_width.max(26);
    let fact_column = |row| FactColumn {
        width: if reserve_kitty && row < KITTY_HEIGHT {
            fact_room - KITTY_COLUMN_WIDTH
        } else {
            fact_room
        },
        beside_mark: mark,
    };
    let muted = Style::default().dim();
    let border = if banner.depth == Depth::Mono {
        muted
    } else {
        Style::default()
            .fg(detail_ink(banner.depth, banner.theme))
            .dim()
    };

    let mut facts = vec![Line::from(vec![
        Span::styled("Codex ", Style::default().bold()),
        Span::styled("ALab", bold_accent(banner.depth, banner.theme, 0xB7A2F1)),
    ])];
    let release_column = fact_column(facts.len());
    if pair_width <= release_column.width {
        facts.push(Line::from(pair));
    } else {
        for row in crate::interface::release_rows(alab, upstream, release_column.width) {
            facts.push(fact(banner, "", &row.text, release_column, border.fg));
        }
    }
    let mut details = details.into_iter().map(|line| line.to_string()).peekable();
    while let Some(text) = details.next() {
        let mut column = fact_column(facts.len());
        let (label, value) = if let Some(value) = text.strip_prefix("model: ") {
            if let Some(rows) = wrapped_model(banner, value, column, border.fg) {
                facts.extend(rows);
                continue;
            }
            ("Model", value)
        } else if let Some(value) = text.strip_prefix("directory: ") {
            let next_column = fact_column(facts.len() + 1);
            if column.width < next_column.width
                && let Some(permission) = details
                    .peek()
                    .and_then(|line| line.strip_prefix("permissions: "))
                && let Some(access) =
                    access_beside_kitty(banner, permission, value, column, next_column, border.fg)
            {
                facts.push(access);
                details.next();
                column = next_column;
            }
            ("Directory", value)
        } else if let Some(value) = text.strip_prefix("permissions: ") {
            ("Access", value)
        } else {
            ("", text.as_str())
        };
        facts.push(fact(banner, label, value, column, border.fg));
    }
    if let Some(mode) = mode {
        facts.push(fact(
            banner,
            "Autopilot",
            mode.label(),
            fact_column(facts.len()),
            border.fg,
        ));
    }

    let narrow = if mark {
        None
    } else {
        narrow_mark(
            room,
            indent.len(),
            pair_width,
            &facts,
            super::artwork::native_graphics(banner),
        )
    };
    let (kitty_width, kitty_height) = if framed {
        kitty_column(&facts, fact_room, reserve_kitty)
    } else {
        (0, 0)
    };
    // The mode has already reserved its caption and all artwork has its place.
    // Use only the padding left on that row for the whole local landing time.
    if mode == Some(Mode::Cruise)
        && let Some(deadline) = deadline
    {
        let row = facts.len() - 1;
        let width = match narrow {
            Some(NarrowMark::BesideFacts(left)) => available - left - COMPACT_MARK_WIDTH - 2,
            _ if row < kitty_height => fact_room - kitty_width,
            _ => fact_room,
        };
        let suffix = format!(" ⤓{deadline}");
        if used_cells(&facts[row]) + painted_width(&suffix) <= width {
            facts[row].push_span(suffix);
        }
    }
    let height = facts
        .len()
        .max(if mark { super::codex_icon::HEIGHT } else { 0 });
    let mut lines = Vec::with_capacity(height + 2);
    let rule = |left, right| {
        Line::from(vec![
            Span::raw(indent),
            Span::styled(format!("{left}{}{right}", "─".repeat(room - 2)), border),
        ])
    };
    if framed {
        lines.push(rule('╭', '╮'));
    }
    for row in 0..height {
        let prefix = match narrow {
            Some(NarrowMark::BesideFacts(cells)) => {
                Some((cells, super::codex_icon::compact_row(banner, row), "  "))
            }
            Some(NarrowMark::BesideTitle { indent, gap }) if row < TITLE_MARK_HEIGHT => {
                Some((indent, super::artwork::title_mark(row), gap))
            }
            _ => None,
        };
        let mut spans = if let Some((cells, icon, gap)) = prefix {
            vec![Span::raw(&margin[..cells]), icon, Span::raw(gap)]
        } else {
            vec![Span::raw(indent)]
        };
        if framed {
            spans.extend([Span::styled("│", border), Span::raw(" ")]);
        }
        if mark {
            spans.extend([super::codex_icon::row(banner, row), Span::raw("  ")]);
        }
        let line = facts.get(row).cloned().unwrap_or_default();
        let used = used_cells(&line);
        spans.extend(line.spans);
        if framed {
            let art_width = if row < kitty_height { kitty_width } else { 0 };
            spans.push(Span::raw(" ".repeat(fact_room - art_width - used)));
            if art_width > 0 {
                spans.extend(super::artwork::kitty(banner, art_width, row));
            }
            spans.extend([Span::raw(" "), Span::styled("│", border)]);
        }
        lines.push(Line::from(spans));
    }
    if framed {
        lines.push(rule('╰', '╯'));
    }
    lines
}

/// Fit the kitty into padding after the facts have taken their space.
fn kitty_column(facts: &[Line<'_>], room: usize, reserved: bool) -> (usize, usize) {
    let fits = |width, height| {
        facts
            .iter()
            .take(height)
            .all(|line| used_cells(line) + width <= room)
    };
    if reserved || fits(KITTY_COLUMN_WIDTH, KITTY_HEIGHT) {
        (KITTY_COLUMN_WIDTH, KITTY_HEIGHT)
    } else if fits(COMPACT_KITTY_COLUMN_WIDTH, COMPACT_KITTY_HEIGHT) {
        (COMPACT_KITTY_COLUMN_WIDTH, COMPACT_KITTY_HEIGHT)
    } else {
        (0, 0)
    }
}

fn used_cells(line: &Line<'_>) -> usize {
    line.spans
        .iter()
        .map(|span| painted_width(&span.content))
        .sum()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NarrowMark {
    BesideFacts(usize),
    BesideTitle { indent: usize, gap: &'static str },
}

/// A narrow mark may borrow the left margin, up to the cell before the autowrap boundary.
fn narrow_mark(
    room: usize,
    margin: usize,
    pair_width: usize,
    facts: &[Line<'_>],
    native: bool,
) -> Option<NarrowMark> {
    if room >= MAX_CARD_WIDTH || pair_width > room {
        return None;
    }
    let spare_cells = |rows: &[Line<'_>], width| {
        let widest = rows.iter().map(used_cells).max()?;
        (room + margin).checked_sub(widest + width)
    };
    if facts.len() >= COMPACT_MARK_HEIGHT
        && let Some(spare) = spare_cells(facts, COMPACT_MARK_WIDTH + 2)
    {
        return Some(NarrowMark::BesideFacts(spare.min(margin)));
    }
    // A long detail below the release can keep its original line and left margin.
    if native {
        let spare = spare_cells(facts.get(..TITLE_MARK_HEIGHT)?, TITLE_MARK_WIDTH)?;
        let gap = match spare {
            0 => return None,
            1 => " ",
            _ => "  ",
        };
        let indent = (spare - gap.len()).min(margin);
        return Some(NarrowMark::BesideTitle { indent, gap });
    }
    None
}

fn bold_accent(depth: Depth, theme: Theme, rgb: super::palette::Rgb) -> Style {
    if let Some(ink) = theme.unreported_foreground(depth) {
        return ink.bold();
    }
    let bold = Style::default().bold();
    match depth {
        Depth::Mono => bold,
        // Named ANSI colors and their bold variants both belong to the terminal palette.
        Depth::Ansi16 => bold.fg(Color::Reset),
        Depth::TrueColor | Depth::Ansi256 => {
            bold.fg(super::readable_text(rgb, Color::Reset, depth, theme))
        }
    }
}

/// The active permission warning in illustrated and plain enhanced session headers.
pub fn permission_style(depth: Depth, theme: Theme) -> Style {
    bold_accent(depth, theme, 0xCD00CD)
}

fn detail_ink(depth: Depth, theme: Theme) -> Color {
    super::readable_text(0x83869C, Color::Gray, depth, theme)
}

/// Readable secondary text for the enhanced session header's details.
pub fn detail_style(depth: Depth, theme: Theme) -> Style {
    caption_style(depth, theme, None)
}

fn caption_style(depth: Depth, theme: Theme, frame_ink: Option<Color>) -> Style {
    if let Some(ink) = theme.unreported_foreground(depth) {
        return ink;
    }
    if !theme.is_light() {
        return Style::default().dim();
    }
    let fg = match depth {
        Depth::Mono => None,
        Depth::Ansi16 => Some(Color::Reset),
        // The illustrated card already selected this neutral for its frame.
        Depth::Ansi256 | Depth::TrueColor => {
            Some(frame_ink.unwrap_or_else(|| detail_ink(depth, theme)))
        }
    };
    Style {
        fg,
        ..Style::default().not_dim()
    }
}

/// Include trailing separators in the displayed checkout, even at an exact fit.
fn checkout_name(path: &str) -> &str {
    let start = path
        .trim_end_matches(std::path::is_separator)
        .rfind(std::path::is_separator)
        .map_or(0, |separator| separator + 1);
    &path[start..]
}

/// Move Access beside the kitty when the wider row recovers the directory caption,
/// full path, or checkout name and the complete permission fits beside the artwork.
fn access_beside_kitty(
    banner: &Banner,
    permission: &str,
    directory: &str,
    column: FactColumn,
    next_column: FactColumn,
    frame_ink: Option<Color>,
) -> Option<Line<'static>> {
    let path_width = painted_width(directory);
    let recovers_caption = !column.fits_caption(path_width) && next_column.fits_caption(path_width);
    let recovered = |width| column.width < width && width <= next_column.width;
    if !recovers_caption
        && !recovered(path_width)
        && !recovered(painted_width(checkout_name(directory)))
    {
        return None;
    }
    let row = fact(banner, "Access", permission, column, frame_ink);
    row.spans
        .last()
        .is_some_and(|span| span.content == permission)
        .then_some(row)
}

fn fact(
    banner: &Banner,
    label: &str,
    value: &str,
    column: FactColumn,
    muted_ink: Option<Color>,
) -> Line<'static> {
    let width = column.width;
    let mut spans = Vec::new();
    let mut available = width;
    let cells = painted_width(value);
    let captioned_width = width.saturating_sub(CAPTION_WIDTH);
    let mut needs_full_row =
        matches!(label, "Model" | "Directory") && cells <= width && cells > captioned_width;
    if label == "Directory" && cells > width {
        let checkout = checkout_name(value);
        let checkout_width = painted_width(checkout);
        if checkout_width == width {
            return Line::from(checkout.to_owned());
        }
        // Lend the caption's cells when the name and an ellipsis can then fit together.
        needs_full_row = (captioned_width..width).contains(&checkout_width);
    }
    // Beside the full-size mark, fill spare cells with the caption only when
    // the whole value fits. Narrower mark-free rows can use that room for the compact icon.
    let show_caption = width >= 24 || column.fits_caption(cells);
    if !label.is_empty() && show_caption && !needs_full_row {
        spans.push(Span::styled(
            format!("{label:<CAPTION_WIDTH$}"),
            caption_style(banner.depth, banner.theme, muted_ink),
        ));
        available = captioned_width;
    }
    // The raw header carries the permission state as text. Restore the emphasis
    // that the upstream session header gives this specific value.
    let value_style = if label == "Access" && value == "YOLO mode" {
        permission_style(banner.depth, banner.theme)
    } else {
        Style::default()
    };
    let value = fit_value(value, cells, available, label == "Directory");
    spans.push(Span::styled(value, value_style));
    Line::from(spans)
}

/// Wrap whole words first. An oversized model keeps its prefix and the words that fit below it.
fn wrapped_model(
    banner: &Banner,
    value: &str,
    column: FactColumn,
    muted_ink: Option<Color>,
) -> Option<Vec<Line<'static>>> {
    let split = model_breaks(value, painted_width(value), column.width)?;
    let caption = column.fits_caption(split.longest);
    let rows = split.rows(value.len()).enumerate().map(|(index, range)| {
        let part = &value[range];
        if index == 0 {
            fact(
                banner,
                if caption { "Model" } else { "" },
                part,
                column,
                muted_ink,
            )
        } else if caption {
            Line::from(format!("{:CAPTION_WIDTH$}{part}", ""))
        } else {
            Line::from(part.to_owned())
        }
    });
    Some(rows.collect())
}

fn fit_value(value: &str, cells: usize, width: usize, keep_suffix: bool) -> String {
    if cells <= width {
        return value.to_owned();
    }
    let Some(room) = width.checked_sub(1) else {
        return String::new();
    };
    // Keep the end of a directory so the checkout name survives. Other facts
    // read from the beginning. Only take graphemes that fit in their entirety.
    let (start, end) = if keep_suffix {
        (cells - room, cells)
    } else {
        (0, room)
    };
    let mut result = String::new();
    if keep_suffix {
        result.push('…');
    }
    let mut position = 0;
    for grapheme in Span::raw(value).styled_graphemes(Style::default()) {
        let next = position + usize::from(grapheme.symbol.cell_width());
        if next > end {
            break;
        }
        if position >= start {
            result.push_str(grapheme.symbol);
        }
        position = next;
    }
    if !keep_suffix {
        result.push('…');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::banner::Style as BannerStyle;
    use crate::banner::Theme;

    fn marked_column(width: usize) -> FactColumn {
        FactColumn {
            width,
            beside_mark: true,
        }
    }

    #[test]
    fn compact_text_mark_uses_the_native_footprint_and_terminal_ink() {
        for depth in [Depth::Mono, Depth::Ansi16, Depth::Ansi256] {
            let banner = Banner::new(BannerStyle::Codex, depth, Theme::default(), "0.12.0")
                .with_upstream(Some("0.169.1".into()));
            let facts = vec![
                Line::from("model: GPT-6"),
                Line::from("directory: ~/codex-alab"),
                Line::from("permissions: YOLO mode"),
            ];
            for (width, margin) in [(31, 0), (32, 1), (33, 2), (35, 2)] {
                let rows = render_session(&banner, width, Some(Mode::Land), facts.clone());
                assert_eq!(rows.len(), 6);
                for (line, expected) in
                    rows.iter()
                        .zip(["⢠⠖⠋⠙⠲⡄", "⣏ ❯_ ⣹", "⠘⠦⣄⣠⠴⠃", "      ", "      ", "      "])
                {
                    assert_eq!(line.spans[0].width(), margin);
                    assert_eq!(line.spans[1], Span::raw(expected));
                    assert_eq!(line.spans[1].width(), COMPACT_MARK_WIDTH);
                    assert!(line.width() < usize::from(width));
                }
                assert!(rows[1].to_string().contains("0.12 on Codex 0.169.1"));
                assert!(rows[3].to_string().contains("Directory ~/codex-alab"));
            }
            for width in [30, 36] {
                let rows = render_session(&banner, width, Some(Mode::Land), facts.clone());
                assert!(rows.iter().all(|line| !line.to_string().contains("⣏ ❯_ ⣹")));
            }
        }
    }

    #[test]
    fn compact_kitty_uses_only_empty_title_and_release_padding() {
        let facts = [
            Line::from("Codex ALab"),
            Line::from("0.12 on Codex 0.169.1"),
            Line::from("Model     GPT-6"),
            Line::from("Directory ~/codex-alab"),
        ];
        // The 21-cell release needs 13 spare cells for the compact kitty.
        assert_eq!(kitty_column(&facts, 33, false), (0, 0));
        assert_eq!(kitty_column(&facts, 34, false), (13, 2));
        // The full kitty fits all four rows once the 22-cell directory has room.
        assert_eq!(kitty_column(&facts, 45, false), (13, 2));
        assert_eq!(kitty_column(&facts, 46, false), (24, 4));
    }

    #[test]
    fn compact_mark_shares_the_existing_rows_only_when_their_content_fits() {
        let rows = [
            Line::from("Codex ALab"),
            Line::from("0.12 on Codex 0.169.1"),
            Line::from("Directory ~/codex-alab"),
        ];
        for (width, margin) in [(30, None), (31, Some(0)), (32, Some(1)), (33, Some(2))] {
            assert_eq!(
                narrow_mark(width - 3, 2, 21, &rows, false),
                margin.map(NarrowMark::BesideFacts)
            );
        }
        assert_eq!(narrow_mark(33, 2, 21, &rows[..2], false), None);
        assert_eq!(narrow_mark(33, 2, 34, &rows, false), None);
        let long_pair = "9".repeat(98);
        let capped = [
            Line::from("Codex ALab"),
            Line::from(long_pair),
            Line::from("GPT-6"),
        ];
        assert_eq!(narrow_mark(MAX_CARD_WIDTH, 2, 98, &capped, false), None);
        let wide = [
            Line::from("Codex ALab"),
            Line::from("0.11 on Codex 0.169"),
            Line::from("界".repeat(12)),
        ];
        assert_eq!(narrow_mark(29, 2, 19, &wide, false), None);
        assert_eq!(
            narrow_mark(30, 2, 19, &wide, false),
            Some(NarrowMark::BesideFacts(0))
        );
    }

    #[test]
    fn title_mark_preserves_the_release_when_a_directory_gains_its_caption() {
        let facts = [
            Line::from("Codex ALab"),
            Line::from("0.12 on Codex 0.169.1"),
            Line::from("Model     GPT-6"),
            Line::from("Directory ~/projects/東京/猫"),
        ];
        for (columns, indent) in [(28, 0), (29, 1), (30, 2), (31, 2), (35, 2)] {
            assert_eq!(
                narrow_mark(columns - 3, 2, 21, &facts, true),
                Some(NarrowMark::BesideTitle { indent, gap: "  " })
            );
        }
        assert_eq!(
            narrow_mark(24, 2, 21, &facts, true),
            Some(NarrowMark::BesideTitle {
                indent: 0,
                gap: " "
            })
        );
        assert_eq!(narrow_mark(23, 2, 21, &facts, true), None);
        assert_eq!(narrow_mark(24, 2, 21, &facts, false), None);
        assert_eq!(narrow_mark(18, 2, 21, &facts, true), None);
        assert_eq!(narrow_mark(MAX_CARD_WIDTH, 2, 21, &facts, true), None);
        assert_eq!(
            narrow_mark(28, 2, 21, &facts[..2], true),
            Some(NarrowMark::BesideTitle {
                indent: 2,
                gap: "  "
            })
        );
        assert_eq!(
            narrow_mark(28, 2, 21, &facts[..3], true),
            Some(NarrowMark::BesideFacts(1))
        );
    }

    #[test]
    fn session_captions_keep_complete_values_beside_the_mark_and_frame() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.12.0")
            .with_upstream(Some("0.169.1".into()));
        for width in 35..=43 {
            let lines = render_session(
                &banner,
                width,
                Some(Mode::Land),
                vec![
                    "model: GPT-6".into(),
                    "directory: ~/codex-alab".into(),
                    "permissions: YOLO mode".into(),
                ],
            );
            let text = lines
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            for fact in [
                "0.12 on Codex 0.169.1",
                "Model     GPT-6",
                "Access    YOLO mode",
                "Autopilot ⤓ land",
                "~/codex-alab",
            ] {
                assert!(text.contains(fact), "{width}: {text}");
            }
            assert_eq!(
                text.contains("Directory ~/codex-alab"),
                !matches!(width, 36 | 40),
                "{width}: {text}"
            );
            assert_eq!(lines.len(), if width < 40 { 6 } else { 8 });
            assert!(lines.iter().all(|row| row.width() < usize::from(width)));
        }

        // Two eleven-cell words fit in the 21-cell fact column with a ten-cell caption.
        let rows = render_session(
            &banner,
            36,
            None,
            vec!["model: abcdefghijk lmnopqrstuv".into()],
        );
        let text = rows.iter().map(ToString::to_string).collect::<Vec<_>>();
        let (prefix, _) = text[2].split_once("Model     abcdefghijk").unwrap();
        let (continuation, _) = text[3].split_once("lmnopqrstuv").unwrap();
        assert_eq!(painted_width(continuation), painted_width(prefix) + 10);
    }

    #[test]
    fn release_pair_uses_the_package_baseline_at_every_supported_width() {
        for (baseline, visible) in [("0.169.0", "0.169"), ("0.169.1", "0.169.1")] {
            let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.11.0")
                .with_upstream(Some(format!("{baseline}+g987654321")));
            let pair = format!("0.11 on Codex {visible}");
            for width in 13..=120 {
                let rows = render_session(&banner, width, None, vec![Line::from("model: gpt-6")])
                    .into_iter()
                    .map(|line| line.to_string())
                    .collect::<Vec<_>>();
                assert!(rows.iter().any(|row| row.contains("0.11")));
                assert!(rows.iter().any(|row| row.contains(visible)));
                assert_eq!(
                    rows.iter().any(|row| row.contains(&pair)),
                    usize::from(width) > pair.len(),
                    "{width}: {rows:?}"
                );
                let wrapped: &[&str] = match (visible, width) {
                    ("0.169", 13 | 14) => &["0.11 on", "Codex 0.169"],
                    ("0.169", 15) => &["0.11", "on Codex 0.169"],
                    ("0.169.1", 13) => &["0.11", "on Codex", "0.169.1"],
                    ("0.169.1", 14..=16) => &["0.11 on", "Codex 0.169.1"],
                    ("0.169.1", 17) => &["0.11", "on Codex 0.169.1"],
                    _ => &[],
                };
                if !wrapped.is_empty() {
                    assert_eq!(&rows[1..rows.len() - 1], wrapped, "{width}");
                }
                // At the fit boundary, the title, release and details share a single margin.
                if let Some(spare @ 0..=2) = usize::from(width).checked_sub(pair.len() + 1) {
                    let indent = " ".repeat(spare);
                    assert_eq!(
                        rows,
                        [
                            format!("{indent}Codex ALab"),
                            format!("{indent}{pair}"),
                            format!("{indent}gpt-6")
                        ]
                    );
                    assert!(
                        rows.iter()
                            .all(|row| painted_width(row) < usize::from(width))
                    );
                }
            }
        }
    }

    #[test]
    fn illustrated_release_continues_a_long_numeric_baseline_before_session_facts() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "dev")
            .with_upstream(Some("0.12345678901.2".to_owned()));
        let rows = render(&banner, 13, Some(Mode::Land))
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            &rows[..4],
            ["Codex ALab", "dev on Codex", "0.1234567890", "1.2"]
        );
        assert!(rows.last().unwrap().contains("land"));
    }

    #[test]
    fn wrapped_release_measures_and_clips_whole_painted_graphemes() {
        for (baseline, visible) in [("لالالالا", "لالالالا"), ("猫猫猫猫猫猫猫", "猫猫猫猫猫…")]
        {
            let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "dev")
                .with_upstream(Some(baseline.to_owned()));
            let rows = render(&banner, 13, None)
                .into_iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>();
            assert_eq!(rows, ["Codex ALab", "dev on Codex", visible]);
        }
    }

    #[test]
    fn widening_keeps_the_mark_visible_when_the_frame_appears() {
        for (version, upstream, first_mark, release) in [
            ("0.11.0", Some("0.169.0"), 34, "0.11 on Codex 0.169"),
            ("dev", None, 38, "dev on Codex unreleased"),
        ] {
            let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), version)
                .with_upstream(upstream.map(str::to_owned));
            let mark = super::super::codex_icon::row(&banner, 2);
            for width in 32..=48 {
                let lines = render(&banner, width, None);
                assert_eq!(
                    lines
                        .iter()
                        .flat_map(|line| &line.spans)
                        .any(|span| *span == mark),
                    width >= first_mark,
                    "{version} at {width} columns",
                );
                assert_eq!(
                    lines[0].to_string().starts_with("  ╭"),
                    width >= first_mark + 4
                );
                assert!(lines.iter().any(|line| line.to_string().contains(release)));
                assert!(lines.iter().all(|line| line.width() < usize::from(width)));
            }
        }
    }

    #[test]
    fn reference_layout_is_a_compact_strip() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "dev");
        let lines = render_session(
            &banner,
            80,
            Some(Mode::Land),
            vec![
                Line::from("model: gpt-6.1"),
                Line::from("directory: ~/project"),
            ],
        );
        assert_eq!(lines.len(), 7);
        let rows = lines.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert!(rows[1].contains("Codex ALab"));
        assert!(rows[2].contains("dev on Codex unreleased"));
        assert!(rows[3].contains("gpt-6.1"));
        assert!(rows[4].contains("~/project"));
        assert!(rows[5].contains("⤓ land"));
        assert!(
            !rows
                .iter()
                .any(|row| row.contains('├') || row.contains("Welcome to"))
        );
    }

    #[test]
    fn yolo_permission_value_keeps_its_emphasis_with_and_without_a_caption() {
        use crate::banner::palette;
        use ratatui::style::Modifier;

        for bg in [(16, 20, 24), (120, 120, 120), (255, 255, 255)] {
            for depth in [Depth::Mono, Depth::Ansi16, Depth::Ansi256, Depth::TrueColor] {
                let banner = Banner::new(
                    BannerStyle::Codex,
                    depth,
                    Theme::from_bg(Some(bg)),
                    "0.11.0",
                )
                .with_upstream(Some("0.169.0".into()));
                for width in [20, 80] {
                    let lines = render_session(
                        &banner,
                        width,
                        None,
                        vec![
                            "model: YOLO mode".into(),
                            "directory: YOLO mode".into(),
                            "permissions: YOLO mode".into(),
                            "permissions: read-only".into(),
                        ],
                    );
                    let mut values = lines
                        .iter()
                        .flat_map(|line| &line.spans)
                        .filter(|span| span.content == "YOLO mode" || span.content == "read-only");
                    assert_eq!(values.next().unwrap().style, Style::default());
                    assert_eq!(values.next().unwrap().style, Style::default());
                    let yolo = values.next().unwrap().style;
                    assert_eq!(values.next().unwrap().style, Style::default());
                    assert!(values.next().is_none());
                    assert_eq!(yolo.add_modifier, Modifier::BOLD);
                    let rgb = match (depth, yolo.fg) {
                        (Depth::Mono, None) => None,
                        (Depth::Ansi16, Some(Color::Reset)) => None,
                        (Depth::Ansi256, Some(Color::Indexed(i))) => Some(palette::xterm256(i)),
                        (Depth::TrueColor, Some(Color::Rgb(r, g, b))) => {
                            Some(palette::pack((r, g, b)))
                        }
                        unexpected => panic!("unexpected permission color: {unexpected:?}"),
                    };
                    if let Some(rgb) = rgb {
                        assert!(
                            palette::contrast(rgb, palette::pack(bg))
                                >= palette::TEXT_CONTRAST_FLOOR
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn fact_captions_keep_readable_ink_in_light_terminal_cells() {
        use crate::banner::palette;
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;
        use ratatui::style::Modifier;
        use ratatui::widgets::Widget;

        for bg in [
            Some((255, 255, 255)),
            Some((140, 140, 140)),
            Some((16, 20, 24)),
            None,
        ] {
            let theme = Theme::from_bg(bg);
            for depth in [Depth::Mono, Depth::Ansi16, Depth::Ansi256, Depth::TrueColor] {
                let banner = Banner::new(BannerStyle::Codex, depth, theme, "0.11.0")
                    .with_upstream(Some("0.169.0".into()));
                let surrounding = Color::Green;
                let (ink, modifier) = match (theme.is_light(), depth) {
                    (false, _) => (surrounding, Modifier::DIM),
                    (true, Depth::Mono) => (surrounding, Modifier::empty()),
                    (true, Depth::Ansi16) => (Color::Reset, Modifier::empty()),
                    (true, _) => {
                        let ink = super::super::readable_text(0x83869C, Color::Gray, depth, theme);
                        let rgb = match ink {
                            Color::Indexed(i) => palette::xterm256(i),
                            Color::Rgb(r, g, b) => palette::pack((r, g, b)),
                            _ => panic!("unexpected muted text ink: {ink:?}"),
                        };
                        assert!(
                            palette::contrast(rgb, theme.ground()) >= palette::TEXT_CONTRAST_FLOOR
                        );
                        (ink, Modifier::empty())
                    }
                };
                let width = 80;
                let lines = render_session(
                    &banner,
                    width,
                    Some(Mode::Land),
                    vec![
                        "model: gpt-6.1".into(),
                        "directory: ~/dev".into(),
                        "permissions: read-only".into(),
                    ],
                );
                for label in ["Model", "Directory", "Access", "Autopilot"] {
                    let caption = format!("{label:<10}");
                    let (line, x) = lines
                        .iter()
                        .find_map(|line| {
                            let text = line.to_string();
                            let (prefix, _) = text.split_once(caption.as_str())?;
                            Some((line, painted_width(prefix) as u16))
                        })
                        .unwrap();
                    let area = Rect::new(0, 0, width, 1);
                    assert!(line.spans.iter().any(|span| {
                        span.content == caption && span.style == detail_style(depth, theme)
                    }));
                    for ambient in [Modifier::ITALIC, Modifier::ITALIC | Modifier::DIM] {
                        let mut buf = Buffer::empty(area);
                        buf.set_style(area, Style::default().fg(surrounding).add_modifier(ambient));
                        line.render(area, &mut buf);
                        for (i, ch) in caption.chars().enumerate() {
                            let cell = &buf[(x + i as u16, 0)];
                            assert_eq!(cell.symbol(), ch.to_string());
                            assert_eq!(
                                (cell.fg, cell.modifier),
                                (ink, modifier | Modifier::ITALIC),
                                "{label}, {depth:?}, {bg:?}, {ambient:?}"
                            );
                        }
                        let value = &buf[(x + 10, 0)];
                        assert_eq!((value.fg, value.modifier), (surrounding, ambient));
                    }
                }
            }
        }
    }

    #[test]
    fn unreported_foreground_preserves_captions_and_bold_accents() {
        use ratatui::style::Modifier;

        let theme = Theme::default().with_terminal_foreground_if_unreported();
        let ambient = Style::default().fg(Color::Green).dim().italic();
        for depth in [Depth::Mono, Depth::Ansi16, Depth::Ansi256, Depth::TrueColor] {
            let banner = Banner::new(BannerStyle::Codex, depth, theme, "0.11")
                .with_upstream(Some("0.169".into()));
            let rows = render_session(
                &banner,
                80,
                Some(Mode::Land),
                vec![
                    "model: gpt".into(),
                    "directory: ~/work".into(),
                    "permissions: YOLO mode".into(),
                ],
            );
            for (text, bold) in [
                ("Model", false),
                ("Directory", false),
                ("Access", false),
                ("Autopilot", false),
                ("ALab", true),
                ("YOLO mode", true),
            ] {
                let span = rows
                    .iter()
                    .flat_map(|row| &row.spans)
                    .find(|span| span.content.trim_end() == text)
                    .expect(text);
                let style = ambient.patch(span.style);
                let fg = if depth == Depth::Mono {
                    Color::Green
                } else {
                    Color::Reset
                };
                assert_eq!(style.fg, Some(fg), "{text}, {depth:?}");
                assert!(
                    !style.add_modifier.contains(Modifier::DIM),
                    "{text}, {depth:?}"
                );
                assert_eq!(
                    style.add_modifier.contains(Modifier::BOLD),
                    bold,
                    "{text}, {depth:?}"
                );
                assert!(style.add_modifier.contains(Modifier::ITALIC));
            }
        }
    }

    #[test]
    fn title_and_permission_ink_remain_readable_in_terminal_cells() {
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;
        use ratatui::style::Modifier;
        use ratatui::widgets::Widget;

        for bg in [Some((255, 255, 255)), Some((16, 20, 24)), None] {
            let theme = Theme::from_bg(bg);
            for depth in [Depth::Mono, Depth::Ansi16, Depth::Ansi256, Depth::TrueColor] {
                let banner = Banner::new(BannerStyle::Codex, depth, theme, "0.11.0")
                    .with_upstream(Some("0.169.0".into()));
                for width in [13, 20, 38, 69, 80] {
                    let lines =
                        render_session(&banner, width, None, vec!["permissions: YOLO mode".into()]);
                    for (word, rgb) in [("ALab", 0xB7A2F1), ("YOLO mode", 0xCD00CD)] {
                        let (line, x) = lines
                            .iter()
                            .find_map(|line| {
                                let text = line.to_string();
                                let (prefix, _) = text.split_once(word)?;
                                Some((line, painted_width(prefix) as u16))
                            })
                            .unwrap();
                        let area = Rect::new(0, 0, width, 1);
                        let mut buf = Buffer::empty(area);
                        buf.set_style(area, Style::default().fg(Color::Green).bg(Color::Blue));
                        line.render(area, &mut buf);
                        let ink = match depth {
                            Depth::Mono => Color::Green, // preserves the surrounding text ink
                            Depth::Ansi16 => Color::Reset,
                            _ => super::super::readable_text(rgb, Color::Reset, depth, theme),
                        };
                        for (i, ch) in word.chars().enumerate() {
                            let cell = &buf[(x + i as u16, 0)];
                            assert_eq!(cell.symbol(), ch.to_string(), "{word} at {width}");
                            assert_eq!(cell.fg, ink, "{word} at {width}, {depth:?}, {bg:?}");
                            assert_eq!(cell.bg, Color::Blue);
                            assert_eq!(cell.modifier, Modifier::BOLD);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn short_facts_share_four_padding_rows_with_the_kitty() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.11.0")
            .with_upstream(Some("0.169.0".into()));
        // Ratatui paints lam and alef in separate cells. The caption and path need 22 cells.
        let path = "/tmp/proj/لا";
        let access = "workspace-write (on request)";
        for width in [64, 65, 68] {
            let lines = render_session(
                &banner,
                width,
                Some(Mode::Land),
                vec![
                    "model: gpt-6".into(),
                    format!("directory: {path}").into(),
                    format!("permissions: {access}").into(),
                ],
            );
            assert_eq!(lines.len(), 8);
            for (row, expected) in [
                "Codex ALab",
                "0.11 on Codex 0.169",
                "Model     gpt-6",
                &format!("Directory {path}"),
                &format!("Access    {access}"),
                "Autopilot ⤓ land",
            ]
            .into_iter()
            .enumerate()
            {
                let line = &lines[row + 1];
                let text = line.to_string();
                let (before, _) = text.split_once(expected).expect("complete session fact");
                assert_eq!(painted_width(before), 16, "{width}: {text}");
                assert_eq!(painted_width(&text), usize::from(width) - 1);
                if row < KITTY_HEIGHT {
                    let kitty = super::super::artwork::kitty(&banner, KITTY_COLUMN_WIDTH, row);
                    let visible = line.spans.windows(kitty.len()).any(|spans| spans == kitty);
                    assert_eq!(visible, width >= 65, "{width}: {text}");
                }
            }
        }
    }

    #[test]
    fn full_model_and_checkout_survive_the_caption_and_kitty_breakpoints() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.11.0")
            .with_upstream(Some("0.169.0".into()));
        let model = "gpt-6.1-astra xhigh";
        let directory = "~/projects/codex-ui";
        for (width, caption) in [
            (42, false),
            (43, false),
            (68, true),
            (69, false),
            (72, true),
        ] {
            let lines = render_session(
                &banner,
                width,
                None,
                vec![
                    Line::from(format!("model: {model}")),
                    Line::from(format!("directory: {directory}")),
                ],
            );
            for (row, value, label) in [
                (&lines[3], model, "Model     "),
                (&lines[4], directory, "Directory "),
            ] {
                let text = row.to_string();
                assert!(text.contains(value), "{width}: {text}");
                assert_eq!(text.contains(label), caption, "{width}: {text}");
            }
        }
    }

    #[test]
    fn narrow_facts_keep_the_checkout_name_and_the_model_prefix() {
        let path = "~/work/clients/acme/platform/codex-alab";
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "dev");
        let lines = render_session(
            &banner,
            80,
            None,
            vec![
                Line::from(format!("directory: {path}")),
                Line::from("model: gpt-6.1-with-a-long-configuration-description"),
            ],
        );
        let directory = lines[3].to_string();
        assert!(directory.contains("Directory …") && directory.contains("/codex-alab"));
        assert!(
            lines[4]
                .to_string()
                .contains("Model     gpt-6.1-with-a-long-config…")
        );

        let path = "~/work/👩🏽‍💻/猫e\u{301}";
        for (width, expected) in [
            (0, ""),
            (1, "…"),
            (3, "猫e\u{301}"),
            (4, "…猫e\u{301}"),
            (6, "…/猫e\u{301}"),
            (7, "…👩🏽‍💻/猫e\u{301}"),
        ] {
            assert_eq!(
                fact(&banner, "Directory", path, marked_column(width), None).to_string(),
                expected
            );
        }
    }

    #[test]
    fn deep_checkout_borrows_caption_space_only_when_it_recovers_the_name() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "dev");
        // The 26-cell row has 15 cells for a clipped suffix beside its caption,
        // or 25 cells after the ellipsis without one.
        for (name, keep_caption) in [
            ("x".repeat(15), true),
            ("x".repeat(16), false),
            ("x".repeat(25), false),
            ("x".repeat(27), true),
            (format!("{}猫", "x".repeat(14)), false),
            (format!("{}e\u{301}", "x".repeat(14)), true),
            (format!("{}/", "x".repeat(15)), false),
        ] {
            let path = format!("/a/long/checkout/parent/{name}");
            let row = fact(&banner, "Directory", &path, marked_column(26), None).to_string();
            assert_eq!(row.starts_with("Directory "), keep_caption, "{path}: {row}");
            if !keep_caption {
                assert!(
                    row.starts_with('…') && row.ends_with(&name),
                    "{path}: {row}"
                );
            }
        }
    }

    #[test]
    fn exact_width_checkout_survives_the_kitty_breakpoint() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.11.0")
            .with_upstream(Some("0.169.0".into()));
        for checkout in [
            "codex-alab-compatible-test",
            "release-checkout-2026-猫e\u{301}/",
        ] {
            assert_eq!(painted_width(checkout), 26);
            let path = format!("/workspace/clients/public/projects/{checkout}");
            for (width, expected) in [
                (68, format!("/{checkout}")),
                (69, checkout.to_string()),
                (70, format!("…{checkout}")),
                (71, format!("…/{checkout}")),
            ] {
                let rows = render_session(
                    &banner,
                    width,
                    None,
                    vec![Line::from(format!("directory: {path}"))],
                );
                let row = rows[3].to_string();
                assert!(row.contains(&expected), "{width}: {row}");
                assert_eq!(row.contains("Directory "), width == 68, "{width}: {row}");
            }
        }
    }

    #[test]
    fn short_access_makes_room_for_a_clipped_checkout_below_the_kitty() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.11.0")
            .with_upstream(Some("0.169.0".into()));
        for checkout in [
            "aterm-native-release-s0.noindex",
            "codex-native-release-s0.noindex",
            "codex-native-猫👩🏽‍💻-release-2026/",
        ] {
            assert_eq!(painted_width(checkout), 31);
            for width in 68..=91 {
                let rows = render_session(
                    &banner,
                    width,
                    Some(Mode::Land),
                    vec![
                        "model: GPT-6".into(),
                        format!("directory: ~/.ubd/{checkout}").into(),
                        "permissions: YOLO mode".into(),
                    ],
                );
                let (directory, access) = if (69..=90).contains(&width) {
                    (5, 4)
                } else {
                    (4, 5)
                };
                assert_eq!(rows.len(), 8);
                assert!(
                    rows[directory].to_string().contains(checkout),
                    "{width}: {checkout}"
                );
                assert!(rows[access].to_string().contains("Access    YOLO mode"));
            }
        }
    }

    #[test]
    fn complete_directory_survives_the_basename_fitting_beside_the_kitty() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.12")
            .with_upstream(Some("0.169.1".into()));
        for checkout in [
            "codex-native-release-s0.noindex",
            "codex-native-猫👩🏽‍💻-release-2026/",
        ] {
            let path = format!("~/projects/production/{checkout}");
            assert_eq!(painted_width(&path), 53);
            for width in [71, 72, 73, 74, 81, 82, 95, 96, 105, 106] {
                let rows = render_session(
                    &banner,
                    width,
                    Some(Mode::Land),
                    vec![
                        "model: GPT-6".into(),
                        format!("directory: {path}").into(),
                        "permissions: YOLO mode".into(),
                    ],
                );
                assert_eq!(rows.len(), 8);
                let (directory, access) = if width < 106 { (5, 4) } else { (4, 5) };
                let directory = rows[directory].to_string();
                assert!(directory.contains(checkout), "{width}: {directory}");
                assert_eq!(
                    directory.contains(&path),
                    width >= 72,
                    "{width}: {directory}"
                );
                assert_eq!(
                    directory.contains("Directory "),
                    width == 71 || width >= 82,
                    "{width}: {directory}"
                );
                assert!(rows[access].to_string().contains("Access    YOLO mode"));
            }
        }
    }

    #[test]
    fn directory_caption_waits_until_the_entire_permission_fits_beside_the_kitty() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.12")
            .with_upstream(Some("0.169.1".into()));
        let path = "~/projects/production/codex-native-release-s0.noindex";
        let permission = "read-only with explicitly restricted workspace";
        for width in [96, 98, 99, 105, 106] {
            let rows = render_session(
                &banner,
                width,
                None,
                vec![
                    "model: GPT-6".into(),
                    format!("directory: {path}").into(),
                    format!("permissions: {permission}").into(),
                ],
            );
            let (directory, access) = if (99..106).contains(&width) {
                (5, 4)
            } else {
                (4, 5)
            };
            assert!(rows[directory].to_string().contains(path));
            assert_eq!(
                rows[directory].to_string().contains("Directory "),
                width >= 99
            );
            assert!(
                rows[access]
                    .to_string()
                    .contains(&format!("Access    {permission}"))
            );
        }
    }

    #[test]
    fn directory_stays_in_order_when_swapping_cannot_preserve_both_values() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.11.0")
            .with_upstream(Some("0.169.0".into()));
        for (checkout, following) in [
            (
                "codex-native-release-s0.noindex".to_owned(),
                "permissions: workspace-write (on request)",
            ),
            ("x".repeat(51), "permissions: read-only"),
            ("x".repeat(9), "permissions: read-only"),
            ("x".repeat(31), "extra session detail"),
        ] {
            let rows = render_session(
                &banner,
                69,
                None,
                vec![
                    "model: GPT-6".into(),
                    format!("directory: ~/work/{checkout}").into(),
                    following.into(),
                ],
            );
            assert!(!rows[4].to_string().contains("Access"));
            assert!(
                rows[5]
                    .to_string()
                    .contains(following.strip_prefix("permissions: ").unwrap_or(following))
            );
        }
    }

    #[test]
    fn mixed_script_facts_keep_the_frame_on_the_same_buffer_row() {
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;
        use ratatui::widgets::Paragraph;
        use ratatui::widgets::Widget;
        use ratatui::widgets::Wrap;

        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.11.0")
            .with_upstream(Some("0.169.0".into()));
        for width in [38, 43, 68, 69, 80] {
            let lines = render_session(
                &banner,
                width,
                None,
                vec![
                    "directory: ~/سلام/سلام".into(),
                    "model: سلام/سلام".into(),
                    "unknown سلام/سلام".into(),
                    "directory: ~/ｶﾞﾊﾟ/👩🏽‍💻".into(),
                ],
            );
            for line in &lines[1..lines.len() - 1] {
                let area = Rect::new(0, 0, width, 2);
                let mut buffer = Buffer::empty(area);
                Paragraph::new(line.clone())
                    .wrap(Wrap { trim: false })
                    .render(area, &mut buffer);
                assert_eq!(buffer[(width - 2, 0)].symbol(), "│", "{width}: {line}");
                assert!((0..width).all(|x| buffer[(x, 1)].symbol() == " "));
            }
        }
    }

    #[test]
    fn fact_clipping_and_captions_use_the_painted_grapheme_widths() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.11.0");
        for (label, value, width, expected) in [
            ("Directory", "~/مشروع/ｶﾞ👩🏽‍💻/سلام", 10, "…ｶﾞ👩🏽‍💻/سلام"),
            ("Directory", "~/مشروع/ｶﾞ👩🏽‍💻/سلام", 9, "…👩🏽‍💻/سلام"),
            ("Model", "سلام/👩🏽‍💻/بار", 8, "سلام/👩🏽‍💻…"),
            ("", "سلام/👩🏽‍💻/بار", 7, "سلام/…"),
            ("Model", "gpt-6.1-سلام-👩🏽‍💻", 24, "gpt-6.1-سلام-👩🏽‍💻"),
        ] {
            assert_eq!(
                fact(&banner, label, value, marked_column(width), None).to_string(),
                expected
            );
        }
    }

    #[test]
    fn session_details_stay_inside_one_card() {
        for bg in [(16, 20, 24), (255, 255, 255)] {
            let banner = Banner::new(
                BannerStyle::Codex,
                Depth::TrueColor,
                Theme::from_bg(Some(bg)),
                "dev",
            );
            for width in [20, 40, 42, 80, 120] {
                let lines = render_session(
                    &banner,
                    width,
                    Some(Mode::Land),
                    vec![
                        Line::from("model: gpt-6.1"),
                        Line::from("directory: ~/project"),
                        Line::from("permissions: YOLO mode"),
                        Line::from("directory: ~/你好/🦀/a-very-long-project-name"),
                        Line::from(format!("directory: ~/{}", "1️⃣".repeat(50))),
                    ],
                );
                let text = lines
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n");
                assert_eq!(text.matches("Codex ALab").count(), 1);
                assert!(text.contains("gpt-6.1"));
                assert!(text.contains("~/project"));
                assert!(lines.iter().all(|line| line.width() < usize::from(width)));
                assert!(text.contains("YOLO mode"));
                if width >= 42 {
                    assert_eq!(
                        text.lines().filter(|line| line.starts_with("  ╭")).count(),
                        1
                    );
                    assert_eq!(
                        text.lines().filter(|line| line.starts_with("  ╰")).count(),
                        1
                    );
                }
            }
        }
    }

    #[test]
    fn illustrated_model_keeps_its_tier_and_effort_as_the_kitty_appears() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.11.0")
            .with_upstream(Some("0.169.0".into()));
        let name = "GPT-6.2-Astra";
        let suffix = "· Ultrafast xhigh";
        let model = format!("{name} {suffix}");
        let checkout = "codex-alab-compatible-test";
        let directory = format!("~/src/{checkout}");
        let access = "workspace-write (on request)";
        let kitty = super::super::artwork::kitty(&banner, 24, 0);
        for width in (68..=74).chain((68..74).rev()) {
            let rows = render_session(
                &banner,
                width,
                None,
                vec![
                    format!("model: {model}").into(),
                    format!("directory: {directory}").into(),
                    format!("permissions: {access}").into(),
                ],
            );
            assert!(rows[2].to_string().contains("0.11 on Codex 0.169"));
            assert_eq!(
                rows[1].spans.windows(kitty.len()).any(|run| run == kitty),
                width >= 69
            );
            assert!(rows.iter().all(|row| row.width() < usize::from(width)));
            let first = rows[3].to_string();
            let next = rows[4].to_string();
            if (69..74).contains(&width) {
                let (name_prefix, _) = first.split_once(name).expect("complete model name");
                let (tier_prefix, _) = next.split_once(suffix).expect("tier and effort together");
                assert_eq!(painted_width(name_prefix), painted_width(tier_prefix));
                assert_eq!(name_prefix.contains("Model"), width >= 70);
                assert!(
                    rows[5]
                        .to_string()
                        .contains(&format!("Directory {directory}"))
                );
            } else {
                assert!(first.contains(&model));
                assert!(next.contains(checkout));
            }
            assert!(
                rows[rows.len() - 2]
                    .to_string()
                    .contains(&format!("Access    {access}"))
            );
        }
    }

    #[test]
    fn narrow_model_uses_a_third_row_only_to_recover_its_tier() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.11.0")
            .with_upstream(Some("0.169.0".into()));
        let qualified = "GPT-6.2-Astra · Ultrafast xhigh";
        for (width, value, expected) in [
            (
                14,
                qualified,
                &["GPT-6.2-Astra", "· Ultrafast", "xhigh"][..],
            ),
            (
                14,
                "GPT-6.2-Astra · Batch Plus high",
                &["GPT-6.2-Astra", "· Batch Plus", "high"],
            ),
            (
                14,
                "👩🏽‍💻猫猫猫猫猫猫猫 · Ultrafast xhigh",
                &["👩🏽‍💻猫猫猫猫猫…", "· Ultrafast", "xhigh"],
            ),
            (16, qualified, &["GPT-6.2-Astra ·", "Ultrafast xhigh"]),
            (18, qualified, &["GPT-6.2-Astra", "· Ultrafast xhigh"]),
            (14, "GPT-6.2-Astra xhigh", &["GPT-6.2-Astra", "xhigh"]),
            (13, "GPT-6.2-Astra xhigh", &["GPT-6.2-Ast…", "xhigh"]),
            (
                14,
                "GPT-6.2-Astra · oversized-tier-label xhigh",
                &["GPT-6.2-Astr…", "xhigh"],
            ),
        ] {
            let rows: Vec<_> =
                render_session(&banner, width, None, vec![format!("model: {value}").into()])
                    .into_iter()
                    .map(|line| line.to_string())
                    .collect();
            assert_eq!(&rows[3..], expected, "{width}: {value}");
        }
    }

    #[test]
    fn illustrated_model_wraps_only_complete_words_on_two_rows() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.11.0")
            .with_upstream(Some("0.169.0".into()));
        for (value, first, second) in [
            ("GPT-6.2-Sol high", "GPT-6.2-Sol high", None),
            ("abcdefghijklmnopqrstuvwxyz7890", "abcdefghijklmno…", None),
            (
                "org/production-model-name xhigh",
                "org/production-model-name",
                Some("xhigh"),
            ),
            (
                "研究👩🏽‍💻モデル研究👩🏽‍💻モデル xhigh",
                "研究👩🏽‍💻モデル研究👩🏽‍💻モデル",
                Some("xhigh"),
            ),
        ] {
            let rows = render_session(
                &banner,
                69,
                None,
                vec![format!("model: {value}").into(), "directory: ~/work".into()],
            );
            assert!(rows[3].to_string().contains(first), "{value}");
            assert!(
                rows[4].to_string().contains(second.unwrap_or("~/work")),
                "{value}"
            );
            if second.is_some() {
                assert!(rows[5].to_string().contains("~/work"));
            }
        }
    }

    #[test]
    fn long_model_keeps_its_prefix_and_complete_trailing_words_beside_the_kitty() {
        let banner = Banner::new(BannerStyle::Codex, Depth::Mono, Theme::default(), "0.11.0")
            .with_upstream(Some("0.169.0".into()));
        let name = "organization/deployment-with-long-name";
        let clipped = "organization/deployment-w…";
        let unicode = format!("{}👩🏽‍💻{}", "x".repeat(23), "猫".repeat(20));
        let clipped_unicode = format!("{}👩🏽‍💻…", "x".repeat(23));
        for (name, suffix, prefix) in [
            (name, "· Fast (high)", clipped),
            (name, "Reserve · Batch Plus ultra", clipped),
            (name, "extended", clipped),
            (unicode.as_str(), "· 速い ultra", clipped_unicode.as_str()),
        ] {
            let rows = render_session(
                &banner,
                69,
                Some(Mode::Land),
                vec![
                    format!("model: {name} {suffix}").into(),
                    "directory: ~/work".into(),
                ],
            );
            let first = rows[3].to_string();
            let next = rows[4].to_string();
            let (first_prefix, _) = first.split_once(prefix).expect("clipped model prefix");
            let (next_prefix, _) = next.split_once(suffix).expect("complete continuation");
            assert_eq!(painted_width(first_prefix), painted_width(next_prefix));
            assert!(rows[5].to_string().contains("~/work"));
            assert!(rows.iter().all(|row| row.width() < 69));
        }
        // No readable continuation when the final word itself exceeds the column.
        for value in [
            format!("prefix {name}"),
            format!("prefix {name}    "),
            format!("  {name} "),
        ] {
            assert!(wrapped_model(&banner, &value, marked_column(26), None).is_none());
        }
    }

    #[test]
    fn card_fits_every_terminal_width_and_color_depth() {
        for depth in [Depth::Mono, Depth::Ansi16, Depth::Ansi256, Depth::TrueColor] {
            let banner = Banner::new(BannerStyle::Codex, depth, Theme::default(), "2026.11.9");
            for width in 0..=200 {
                for mode in [None, Some(Mode::Land), Some(Mode::Cruise), Some(Mode::Off)] {
                    let lines = render(&banner, width, mode);
                    assert!(
                        lines.iter().all(|line| line.width() < usize::from(width)),
                        "width={width} depth={depth:?} lines={lines:?}"
                    );
                    if width >= 40 {
                        let text = lines
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join("\n");
                        assert!(text.contains("Codex ALab"));
                        assert!(text.contains("on Codex unreleased"));
                        if depth == Depth::Mono {
                            assert!(
                                lines.iter().flat_map(|line| &line.spans).all(|span| span
                                    .style
                                    .fg
                                    .is_none()
                                    && span.style.bg.is_none())
                            );
                        }
                    }
                }
            }
        }
    }
}
