// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compact harness strip: Codex mark, identity and facts, rainbow kitty in the right sky.

use super::Banner;
use super::Depth;
use super::Mode;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

// The original full-body harness cat from aterm's harness_card.rs (KITTY_OPEN),
// read at ac52db025e (Apache-2.0, Copyright 2026 Andrew Yates),
// compressed vertically for terminal half-blocks. Keep the ears, tail and four feet.
const CAT: [&[u8; 24]; 14] = [
    b"..............o......o..",
    b".oo..........ono....ono.",
    b"oco.........onnoooooonno",
    b"oco.........occcccccccco",
    b".ocoooooooooocccccccccco",
    b"..occcccccccocekccccekco",
    b"..occcccccccocekccccekco",
    b"..occcccccccoccbbnnbbcco",
    b"..ocbbbbbbccoccbbbbbbcco",
    b"..ocbbbbbbcccoooooooooo.",
    b"..occccccccccccco.......",
    b"..occoccoccoccoo........",
    b"..occoccoccocco.........",
    b"...oo.oo.oo.oo..........",
];
const MARK: [&str; 5] = [
    " ▀▄       ",
    "   ▀▄     ",
    "     █    ",
    "   ▄▀     ",
    " ▄▀  ▄▄▄▄ ",
];

pub(super) fn render(banner: &Banner, width: u16, mode: Option<Mode>) -> Vec<Line<'static>> {
    render_session(banner, width, mode, Vec::new())
}

pub(super) fn render_session(
    banner: &Banner,
    width: u16,
    mode: Option<Mode>,
    details: Vec<Line<'static>>,
) -> Vec<Line<'static>> {
    let room = usize::from(width).saturating_sub(3);
    if room < 10 {
        return Vec::new();
    }
    if room < 31 {
        let title = if room >= 15 {
            ">_ OpenAI Codex"
        } else {
            "codex-alab"
        };
        let mut lines = vec![Line::from(vec![
            Span::raw("  "),
            Span::styled(title, Style::default().bold()),
        ])];
        if room >= 15 {
            lines.push(Line::from("  codex-alab"));
        }
        for detail in details {
            let text = detail.to_string();
            lines.push(Line::from(format!(
                "  {}",
                fit(text.strip_prefix("directory: ").unwrap_or(&text), room)
            )));
        }
        return lines;
    }
    let inner = room.min(112) - 2;
    let has_mark = inner >= 46;
    let art_width = if inner >= 69 {
        (inner - 46).clamp(26, 38)
    } else {
        0
    };
    let mark_width = if has_mark { 12 } else { 2 };
    let text_width = inner - mark_width - art_width - 1;
    let mut facts = vec![Line::from(vec![
        Span::styled("OpenAI Codex", Style::default().bold()),
        Span::styled(
            match banner.version.as_deref() {
                Some(version) if version != "dev" => format!("  v{version}"),
                _ => "  · dev".to_owned(),
            },
            Style::default().dim(),
        ),
    ])];
    facts.extend(details.into_iter().map(|line| {
        let text = line.to_string();
        let label = text
            .strip_prefix("model: ")
            .or_else(|| text.strip_prefix("directory: "))
            .unwrap_or(&text);
        Line::from(Span::styled(label.to_owned(), Style::default().dim()))
    }));
    if let Some(mode) = mode {
        facts.push(Line::from(Span::styled(
            format!("Autopilot {}", mode.label()),
            ink(banner, 0x72C9BB, Color::Cyan),
        )));
    }
    let height = facts.len().max(4) + 1;
    let mut lines = vec![rule(banner, inner, '╭', '╮')];
    for row in 0..height {
        let mut content = Vec::new();
        if has_mark {
            content.push(Span::styled(
                format!(" {} ", MARK.get(row).unwrap_or(&"          ")),
                ink(
                    banner,
                    if banner.theme.is_light() {
                        0x07695D
                    } else {
                        0xA7F3D0
                    },
                    Color::Cyan,
                )
                .bold(),
            ));
        } else {
            content.push(Span::raw("  "));
        }
        if row == height - 1 {
            let signature = fit("✦ enhanced by codex-alab", text_width + art_width);
            content.extend(rainbow(banner, &signature, true));
        } else {
            let fact = facts.get(row).cloned().unwrap_or_default();
            let text = fit(&fact.to_string(), text_width);
            let used = text.width();
            if fact.width() <= text_width {
                content.extend(fact.spans);
            } else {
                let style = fact
                    .spans
                    .first()
                    .map(|span| span.style)
                    .unwrap_or_default();
                content.push(Span::styled(text, style));
            }
            content.push(Span::raw(" ".repeat(text_width - used + 1)));
            if art_width > 0 {
                content.extend(kitty(banner, art_width, row));
            }
        }
        lines.push(frame_row(banner, inner, content));
    }
    lines.push(rule(banner, inner, '╰', '╯'));
    lines
}

fn ink(banner: &Banner, rgb: u32, ansi: Color) -> Style {
    if banner.depth == Depth::Mono {
        return Style::default();
    }
    Style::default().fg(super::render::readable_text(
        rgb,
        ansi,
        banner.depth,
        banner.theme,
    ))
}

fn rainbow(banner: &Banner, text: &str, bold: bool) -> Vec<Span<'static>> {
    let count = text.chars().count().max(2);
    text.chars()
        .enumerate()
        .map(|(i, ch)| {
            let rgb = super::palette::spectrum(i as f32 / (count - 1) as f32);
            let ansi = [
                Color::Red,
                Color::Yellow,
                Color::Green,
                Color::Cyan,
                Color::Blue,
                Color::Magenta,
                Color::LightMagenta,
            ][(i * 7 / count).min(6)];
            let style = ink(banner, rgb, ansi);
            Span::styled(
                ch.to_string(),
                if bold { style.bold() } else { style.dim() },
            )
        })
        .collect()
}

fn rule(banner: &Banner, inner: usize, left: char, right: char) -> Line<'static> {
    let mut spans = vec![Span::raw("  ")];
    spans.extend(rainbow(
        banner,
        &format!("{left}{}{right}", "─".repeat(inner)),
        false,
    ));
    Line::from(spans)
}

fn frame_row(banner: &Banner, inner: usize, content: Vec<Span<'static>>) -> Line<'static> {
    let used = content.iter().map(Span::width).sum::<usize>();
    let mut spans = vec![
        Span::raw("  "),
        Span::styled("│", ink(banner, 0xFF644D, Color::Red).dim()),
    ];
    spans.extend(content);
    spans.push(Span::raw(" ".repeat(inner.saturating_sub(used))));
    spans.push(Span::styled(
        "│",
        ink(banner, 0xAB6FE8, Color::Magenta).dim(),
    ));
    Line::from(spans)
}

fn kitty(banner: &Banner, width: usize, row: usize) -> Vec<Span<'static>> {
    let trail = width - 18;
    let mut spans = Vec::with_capacity(width);
    for x in 0..width {
        if row == 0 && (x == 2 || x == trail.saturating_sub(2)) {
            spans.push(Span::styled(
                if x == 2 { "✦" } else { "·" },
                ink(banner, 0xFFD68A, Color::Yellow),
            ));
            continue;
        }
        let pixel = |y: usize| -> Option<(u32, Color)> {
            if y >= 8 {
                return None;
            }
            if x < trail {
                if (2..=6).contains(&y) {
                    return Some((
                        super::palette::spectrum((y - 2) as f32 / 4.0),
                        [
                            Color::Red,
                            Color::Yellow,
                            Color::Green,
                            Color::Blue,
                            Color::Magenta,
                        ][y - 2],
                    ));
                }
                return None;
            }
            let sx = (x - trail) * 24 / 18;
            let sy = [0, 2, 4, 6, 8, 10, 12, 13][y];
            match CAT[sy][sx] {
                b'o' | b'k' => Some((
                    if banner.theme.is_light() {
                        0x30353F
                    } else {
                        0x677181
                    },
                    Color::DarkGray,
                )),
                b'c' => Some((if sx >= 20 { 0xD89C57 } else { 0xC4CAD0 }, Color::Gray)),
                b'b' | b'w' => Some((0xF5EFE2, Color::White)),
                b'e' => Some((0x59E5AD, Color::Green)),
                b'n' => Some((0xE898AF, Color::LightMagenta)),
                _ => None,
            }
        };
        let a = pixel(row * 2);
        let b = pixel(row * 2 + 1);
        if banner.depth == Depth::Mono {
            spans.push(Span::raw(match (a.is_some(), b.is_some()) {
                (true, true) => "█",
                (true, false) => "▀",
                (false, true) => "▄",
                _ => " ",
            }));
        } else {
            let (glyph, fg, bg) = match (a, b) {
                (Some(a), Some(b)) => ("▀", Some(a), Some(b)),
                (Some(a), None) => ("▀", Some(a), None),
                (None, Some(b)) => ("▄", Some(b), None),
                _ => (" ", None, None),
            };
            let mut style = Style::default();
            if let Some((rgb, ansi)) = fg {
                style = ink(banner, rgb, ansi);
            }
            if let Some((rgb, ansi)) = bg {
                style = style.bg(ink(banner, rgb, ansi).fg.unwrap_or(Color::Reset));
            }
            spans.push(Span::styled(glyph, style));
        }
    }
    spans
}

fn fit(text: &str, room: usize) -> String {
    if text.width() <= room {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let width = ch.width().unwrap_or(0);
        if used + width > room.saturating_sub(1) {
            break;
        }
        used += width;
        out.push(ch);
    }
    if room > 0 {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::banner::Style as BannerStyle;
    use crate::banner::Theme;

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
        assert!(rows[1].contains("OpenAI Codex"));
        assert!(rows[2].contains("gpt-6.1"));
        assert!(rows[3].contains("~/project"));
        assert!(rows[4].contains("Autopilot ⤓ land"));
        assert!(rows[5].contains("enhanced by codex-alab"));
        assert!(
            !rows
                .iter()
                .any(|row| row.contains('├') || row.contains("Welcome to"))
        );
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
            for width in [20, 40, 80, 120] {
                let lines = render_session(
                    &banner,
                    width,
                    Some(Mode::Land),
                    vec![
                        Line::from("model: gpt-6.1"),
                        Line::from("directory: ~/project"),
                        Line::from("permissions: YOLO mode"),
                        Line::from("directory: ~/你好/🦀/a-very-long-project-name"),
                    ],
                );
                let text = lines
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n");
                assert_eq!(text.matches("OpenAI Codex").count(), 1);
                assert!(text.contains("gpt-6.1"));
                assert!(text.contains("~/project"));
                assert!(lines.iter().all(|line| line.width() < usize::from(width)));
                if width >= 40 {
                    assert_eq!(text.matches('╭').count(), 1);
                    assert_eq!(text.matches('╰').count(), 1);
                    assert!(text.contains("YOLO mode"));
                }
            }
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
                        assert!(text.contains("OpenAI Codex"));
                        assert!(text.contains("codex-alab"));
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
