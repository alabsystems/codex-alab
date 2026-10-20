// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Canvas → half-block cells → merged spans, for one width.

use super::Banner;
use super::Depth;
use super::Style;
use super::palette;
use super::palette::Rgb;
use super::sprite::KITTY;
use super::sprite::KITTY_H;
use super::sprite::KITTY_W;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style as CellStyle;
use ratatui::text::Line;
use ratatui::text::Span;

/// Every row opens with this many blank columns: no row starts with a codex reader mark.
const INDENT: usize = 2;
/// The wordmark, one snapped spectrum stop per letter.
const WORDMARK: &str = "codex-alab";
/// The trail columns that fade into the sky (the release is the art).
const TAIL_FADE: usize = 6;
/// Below this alpha a pixel is cut where it cannot be blended.
const CUT_ALPHA: f32 = 0.5;

/// The named ANSI colour for each spectrum stop: 31 33 93 32 34 35 95.
const STOP16: [Color; palette::SPECTRUM_STOPS] = [
    Color::Red,
    Color::Yellow,
    Color::LightYellow,
    Color::Green,
    Color::Blue,
    Color::Magenta,
    Color::LightMagenta,
];

/// What a pixel is, which decides its 16-colour name and whether it inks in mono.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Role {
    /// A ribbon column at the named stop nearest its position.
    Ribbon(usize),
    /// Stripe band `k` (0 red .. 6 violet).
    Band(usize),
    /// A sprite pixel, by its letter.
    Kitty(u8),
}

#[derive(Clone, Copy, Debug)]
struct Px {
    rgb: Rgb,
    alpha: f32,
    role: Role,
}

/// A star: a glyph, its colour, and its 16-colour name.
#[derive(Clone, Copy, Debug)]
struct Star {
    glyph: char,
    rgb: Rgb,
    ansi16: Color,
}

#[derive(Clone, Debug, PartialEq)]
struct Cell {
    glyph: char,
    style: CellStyle,
}

impl Cell {
    const BLANK: Cell = Cell {
        glyph: ' ',
        style: CellStyle::new(),
    };
}

/// The art for one width: the trail length, how many sky stars, and whether stars fly ahead.
struct Layout {
    trail: usize,
    sky: usize,
    ahead: bool,
}

/// The width ladder. Every art row ends by `INDENT + trail + KITTY_W` (plus the ahead stars at
/// 64 and up), which keeps it inside `width - 1`.
fn layout(width: usize) -> Option<Layout> {
    if width >= 64 {
        Some(Layout {
            trail: (width - 2 - KITTY_W - 8).min(56),
            sky: 3,
            ahead: true,
        })
    } else if width >= 44 {
        Some(Layout {
            trail: width - 2 - KITTY_W - 2,
            sky: 2,
            ahead: false,
        })
    } else if width >= 28 {
        Some(Layout {
            trail: (width - 2 - KITTY_W - 2).max(4),
            sky: 0,
            ahead: false,
        })
    } else {
        None
    }
}

pub(super) fn render(banner: &Banner, width: usize) -> Vec<Line<'static>> {
    render_with_harness(banner, width, false)
}

pub(super) fn render_with_harness(
    banner: &Banner,
    width: usize,
    harness_active: bool,
) -> Vec<Line<'static>> {
    if width < INDENT + WORDMARK.len() + 1 {
        return Vec::new();
    }
    let painter = Painter::new(banner);
    let mut lines = vec![Line::default()];
    if let Some(layout) = layout(width) {
        lines.extend(painter.art(&layout).into_iter().map(into_line));
    }
    lines.push(painter.label(width));
    if harness_active && let Some(harness) = painter.harness_line(width) {
        lines.push(harness);
    }
    lines
}

struct Painter<'a> {
    banner: &'a Banner,
    light: bool,
    ground: Rgb,
    /// White-hot on a dark ground, aterm's ink on a light one.
    hot: Rgb,
}

impl<'a> Painter<'a> {
    fn new(banner: &'a Banner) -> Self {
        let light = banner.theme.is_light();
        Self {
            banner,
            light,
            ground: banner.theme.ground(),
            hot: if light {
                palette::INK
            } else {
                palette::WHITE_HOT
            },
        }
    }

    /// The five art rows: a 10-pixel canvas folded into half blocks, then the stars.
    fn art(&self, layout: &Layout) -> Vec<Vec<Cell>> {
        let trail = layout.trail;
        let width = INDENT + trail + KITTY_W;
        let mut px: Vec<Vec<Option<Px>>> = vec![vec![None; width]; KITTY_H];
        for c in 0..trail {
            let x = INDENT + c;
            let dist = trail - c; // 1 touches the kitty
            let alpha = if c >= TAIL_FADE {
                1.0
            } else {
                0.25 + 0.75 * c as f32 / TAIL_FADE as f32
            };
            match self.banner.style {
                Style::Stripes => {
                    let off = ((dist - 1) / 8) % 2;
                    for (k, &stop) in palette::SPECTRUM_ANCHORS.iter().enumerate() {
                        let y = 2 + k + off;
                        if y < KITTY_H {
                            px[y][x] = Some(Px {
                                rgb: stop,
                                alpha,
                                role: Role::Band(k),
                            });
                        }
                    }
                }
                Style::Ribbon => {
                    let wave = (c as f64 * 2.0 * std::f64::consts::PI / 16.0).sin() > 0.3;
                    let off = usize::from(wave);
                    let t = column_t(c, trail);
                    let mut rgb = palette::spectrum(t);
                    if dist <= 2 {
                        // aterm's meteor: a white-hot shoulder where the ribbon meets the head.
                        rgb = palette::mix(self.hot, rgb, (dist - 1) as f32 / 2.0);
                    }
                    let role = Role::Ribbon(palette::spectrum_snap_index(t));
                    for row in px.iter_mut().take((8 + off).min(KITTY_H)).skip(2 + off) {
                        row[x] = Some(Px { rgb, alpha, role });
                    }
                }
            }
        }
        for (y, sprite_row) in KITTY.iter().enumerate() {
            for (x, &letter) in sprite_row.iter().enumerate() {
                if let Some(rgb) = self.kitty_rgb(letter) {
                    px[y][INDENT + trail + x] = Some(Px {
                        rgb,
                        alpha: 1.0,
                        role: Role::Kitty(letter),
                    });
                }
            }
        }

        let mut rows: Vec<Vec<Cell>> = (0..KITTY_H / 2)
            .map(|r| {
                (0..width)
                    .map(|x| self.fold(px[2 * r][x], px[2 * r + 1][x]))
                    .collect()
            })
            .collect();

        // Stars: few and bright, in the sky, never on a stroke (RAINBOW-KITTY-V2 §5.1).
        let at = |f: f64| (f * trail as f64) as usize;
        for (k, f) in [0.14, 0.42, 0.71].into_iter().take(layout.sky).enumerate() {
            let c = at(f);
            let stop = palette::spectrum_snap_index(column_t(c, trail));
            let star = match k {
                0 => self.m2(stop),
                1 => self.hero(),
                _ => self.m3(stop),
            };
            self.put(&mut rows[0], INDENT + c, INDENT + trail, star);
        }
        if self.banner.style == Style::Ribbon {
            for (k, f) in [0.27, 0.58, 0.86].into_iter().take(layout.sky).enumerate() {
                let c = at(f);
                let star = if k == 1 {
                    gold()
                } else {
                    self.m3(palette::spectrum_snap_index(column_t(c, trail)))
                };
                self.put(&mut rows[4], INDENT + c, INDENT + trail, star);
            }
        }
        if layout.ahead {
            let a0 = width + 1;
            self.put(&mut rows[0], a0 + 1, a0 + 2, gold());
            self.put(&mut rows[2], a0 + 4, a0 + 5, self.m3(0));
            self.put(
                &mut rows[4],
                a0 + 2,
                a0 + 3,
                Star {
                    glyph: '⋆',
                    rgb: self.hot,
                    ansi16: self.hot16(),
                },
            );
        }
        rows
    }

    fn kitty_rgb(&self, letter: u8) -> Option<Rgb> {
        Some(match letter {
            b'O' if self.light => palette::INK,
            b'O' => palette::OUTLINE_PALE,
            b'W' => palette::COAT,
            b'D' => palette::PATCH_DARK,
            b'G' => palette::PATCH_GINGER,
            b'P' | b'N' => palette::INNER_EAR,
            b'M' => palette::MUZZLE,
            b'E' => palette::IRIS,
            b'H' => palette::WHITE_HOT,
            b'K' => palette::EYE_INK_DARK,
            _ => return None,
        })
    }

    /// Two stacked pixels as one cell. A pixel that has no colour sets none, so the terminal's
    /// background shows through.
    fn fold(&self, top: Option<Px>, bottom: Option<Px>) -> Cell {
        if self.banner.depth == Depth::Mono {
            let glyph = match (ink(top), ink(bottom)) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            };
            return Cell {
                glyph,
                style: CellStyle::new(),
            };
        }
        let (top, bottom) = (self.pixel_colour(top), self.pixel_colour(bottom));
        let style = CellStyle::new();
        match (top, bottom) {
            (None, None) => Cell::BLANK,
            (None, Some(b)) => Cell {
                glyph: '▄',
                style: style.fg(b),
            },
            (Some(t), None) => Cell {
                glyph: '▀',
                style: style.fg(t),
            },
            (Some(t), Some(b)) if t == b => Cell {
                glyph: '█',
                style: style.fg(t),
            },
            // A background of `Reset` is the terminal's background, not its foreground: keep the
            // theme-foreground pixel in the foreground.
            (Some(t), Some(Color::Reset)) => Cell {
                glyph: '▄',
                style: style.fg(Color::Reset).bg(t),
            },
            (Some(t), Some(b)) => Cell {
                glyph: '▀',
                style: style.fg(t).bg(b),
            },
        }
    }

    /// A pixel's colour at this depth, or none where it is cut.
    fn pixel_colour(&self, px: Option<Px>) -> Option<Color> {
        let px = px?;
        let mut rgb = px.rgb;
        if px.alpha < 1.0 {
            if self.banner.depth == Depth::TrueColor && self.banner.theme.bg.is_some() {
                rgb = palette::mix(self.ground, rgb, px.alpha);
            } else if px.alpha < CUT_ALPHA {
                return None;
            }
        }
        let ansi16 = match px.role {
            Role::Ribbon(stop) | Role::Band(stop) => STOP16[stop],
            Role::Kitty(letter) => self.kitty16(letter),
        };
        Some(self.colour(rgb, ansi16))
    }

    fn kitty16(&self, letter: u8) -> Color {
        match letter {
            // On a light theme the outline is ink and the coat a light grey, so the dark patch
            // (90) still reads inside the shape instead of the whole head going dark.
            b'O' if self.light => Color::Black,
            b'O' => Color::Gray,
            b'W' | b'M' if self.light => Color::Gray,
            b'D' => Color::DarkGray,
            b'G' => Color::Yellow,
            b'P' | b'N' => Color::LightMagenta,
            b'E' => Color::Cyan,
            b'H' => Color::White,
            b'K' => Color::Black,
            _ => Color::Reset, // coat and muzzle: the theme's own foreground
        }
    }

    /// `rgb` for this depth: exact, nearest of 256, or named. On a light ground every colour
    /// clears [`palette::LIGHT_CONTRAST_FLOOR`], after quantizing too.
    fn colour(&self, rgb: Rgb, ansi16: Color) -> Color {
        if self.light {
            return self.clearing(rgb, light16(ansi16), palette::LIGHT_CONTRAST_FLOOR);
        }
        match self.banner.depth {
            Depth::TrueColor => rgb_color(palette::channels(rgb)),
            Depth::Ansi256 => indexed_color(palette::nearest_xterm256(rgb)),
            Depth::Ansi16 | Depth::Mono => ansi16,
        }
    }

    /// A wordmark letter: [`Painter::colour`], held to [`palette::TEXT_CONTRAST_FLOOR`] on any
    /// ground, lifted toward white on a dark one and toward ink on a light one.
    fn text_colour(&self, rgb: Rgb, ansi16: Color) -> Color {
        let ansi16 = if self.light {
            match light16(ansi16) {
                Color::LightMagenta => Color::Magenta,
                other => other,
            }
        } else {
            match ansi16 {
                Color::Blue => Color::LightBlue,
                Color::Magenta => Color::LightMagenta,
                other => other,
            }
        };
        self.clearing(rgb, ansi16, palette::TEXT_CONTRAST_FLOOR)
    }

    /// `rgb` moved away from the ground until it clears `floor`, then at this depth; at 256
    /// colours the entry chosen clears it as well. A named colour is the theme's to draw.
    fn clearing(&self, rgb: Rgb, ansi16: Color, floor: f64) -> Color {
        let toward = if self.light {
            palette::INK
        } else {
            palette::WHITE_HOT
        };
        let rgb = palette::rescue(rgb, self.ground, toward, floor);
        match self.banner.depth {
            Depth::TrueColor => rgb_color(palette::channels(rgb)),
            Depth::Ansi256 => {
                indexed_color(palette::nearest_xterm256_clearing(rgb, self.ground, floor))
            }
            Depth::Ansi16 | Depth::Mono => ansi16,
        }
    }

    fn hot16(&self) -> Color {
        if self.light {
            Color::Black
        } else {
            Color::White
        }
    }

    /// The hero: the one white-hot star.
    fn hero(&self) -> Star {
        Star {
            glyph: '✦',
            rgb: self.hot,
            ansi16: self.hot16(),
        }
    }

    /// A medium star: its body 0.6 of the way from white-hot to its stop.
    fn m2(&self, stop: usize) -> Star {
        Star {
            glyph: '⋆',
            rgb: palette::mix(
                self.hot,
                palette::spectrum_stop(stop),
                palette::M2_BODY_TINT,
            ),
            ansi16: if self.light {
                STOP16[stop]
            } else {
                Color::White
            },
        }
    }

    /// A grain: the pure stop of the column it rose from.
    fn m3(&self, stop: usize) -> Star {
        Star {
            glyph: '∙',
            rgb: palette::spectrum_stop(stop),
            ansi16: STOP16[stop],
        }
    }

    /// Puts `star` at column `x` of `row`, or a column or two either side (left of `end`), where
    /// the cell and both its neighbours are sky: a star never sits on or against a stroke. With no
    /// such column nearby there is no star.
    fn put(&self, row: &mut Vec<Cell>, x: usize, end: usize, star: Star) {
        let sky = |row: &[Cell], x: usize| row.get(x).is_none_or(|cell| *cell == Cell::BLANK);
        let Some(x) = [
            Some(x),
            x.checked_sub(1),
            x.checked_add(1),
            x.checked_sub(2),
            x.checked_add(2),
        ]
        .into_iter()
        .flatten()
        .find(|&x| x >= INDENT && x < end && sky(row, x) && sky(row, x - 1) && sky(row, x + 1)) else {
            return;
        };
        if row.len() <= x {
            row.resize(x + 1, Cell::BLANK);
        }
        let style = match self.banner.depth {
            Depth::Mono => CellStyle::new(),
            _ => CellStyle::new().fg(self.colour(star.rgb, star.ansi16)),
        };
        row[x] = Cell {
            glyph: star.glyph,
            style,
        };
    }

    /// The label: the wordmark in snapped stops, bold, then dim provenance, shortened to fit.
    fn label(&self, width: usize) -> Line<'static> {
        let mut spans = vec![Span::raw(" ".repeat(INDENT))];
        let letters = WORDMARK.chars().count();
        for (i, ch) in WORDMARK.chars().enumerate() {
            let stop = (i * palette::SPECTRUM_STOPS / letters).min(palette::SPECTRUM_STOPS - 1);
            let mut style = CellStyle::new().add_modifier(Modifier::BOLD);
            if self.banner.depth != Depth::Mono {
                style = style.fg(self.text_colour(palette::SPECTRUM_ANCHORS[stop], STOP16[stop]));
            }
            spans.push(Span::styled(ch.to_string(), style));
        }
        let provenance = "  rainbow kitty · ALab build of OpenAI Codex";
        let mut tails = Vec::with_capacity(3);
        if let Some(sha7) = &self.banner.sha7 {
            tails.push(format!("{provenance} · g{sha7}"));
        }
        tails.push(provenance.to_string());
        tails.push("  rainbow kitty".to_string());
        let used = INDENT + letters;
        if let Some(tail) = tails
            .into_iter()
            .find(|tail| used + unicode_width::UnicodeWidthStr::width(tail.as_str()) < width)
        {
            spans.push(Span::styled(
                tail,
                CellStyle::new().add_modifier(Modifier::DIM),
            ));
        }
        merge(spans)
    }

    /// The harness banner line under the wordmark when codex-alab's autopilot is active.
    fn harness_line(&self, width: usize) -> Option<Line<'static>> {
        let candidates = [
            (
                "✦ ",
                "enhanced by aterm · ",
                "codex-alab harness",
                " · autopilot ∞ cruise",
            ),
            ("✦ ", "enhanced by aterm · ", "codex-alab harness", ""),
            ("✦ ", "", "codex-alab harness", " ● on"),
        ];
        let (star, lead, brand, tail) = candidates.into_iter().find(|(s, l, b, t)| {
            INDENT
                + unicode_width::UnicodeWidthStr::width(*s)
                + unicode_width::UnicodeWidthStr::width(*l)
                + unicode_width::UnicodeWidthStr::width(*b)
                + unicode_width::UnicodeWidthStr::width(*t)
                < width
        })?;
        let mut spans = vec![Span::raw(" ".repeat(INDENT))];
        let star_style = if self.banner.depth == Depth::Mono {
            CellStyle::new().add_modifier(Modifier::BOLD)
        } else {
            CellStyle::new()
                .fg(self.colour(palette::GOLD, Color::LightYellow))
                .add_modifier(Modifier::BOLD)
        };
        spans.push(Span::styled(star.to_string(), star_style));
        if !lead.is_empty() {
            spans.push(Span::styled(
                lead.to_string(),
                CellStyle::new().add_modifier(Modifier::DIM),
            ));
        }
        let brand_chars = brand.chars().count().max(1);
        for (i, ch) in brand.chars().enumerate() {
            let stop = (i * palette::SPECTRUM_STOPS / brand_chars).min(palette::SPECTRUM_STOPS - 1);
            let mut style = CellStyle::new().add_modifier(Modifier::BOLD);
            if self.banner.depth != Depth::Mono {
                style = style.fg(self.text_colour(palette::SPECTRUM_ANCHORS[stop], STOP16[stop]));
            }
            spans.push(Span::styled(ch.to_string(), style));
        }
        if !tail.is_empty() {
            let tail_style = if self.banner.depth == Depth::Mono {
                CellStyle::new().add_modifier(Modifier::BOLD)
            } else {
                CellStyle::new()
                    .fg(self.text_colour(0x4EE088, Color::Green))
                    .add_modifier(Modifier::BOLD)
            };
            spans.push(Span::styled(tail.to_string(), tail_style));
        }
        Some(merge(spans))
    }
}

pub(super) fn render_composer_border(
    composer_rect: ratatui::layout::Rect,
    buf: &mut ratatui::buffer::Buffer,
    depth: Depth,
    theme: super::Theme,
) {
    if composer_rect.height < 3 || composer_rect.width < 34 {
        return;
    }
    let banner = Banner::new(Style::Ribbon, depth, theme, "");
    let painter = Painter::new(&banner);
    let y = composer_rect.y;
    let x_start = composer_rect.x.saturating_add(INDENT as u16);
    let x_end = composer_rect.right().saturating_sub(2);
    if x_end <= x_start {
        return;
    }
    let span_w = usize::from(x_end - x_start);
    let full_badge = " ✦ alab kitty · codex-alab harness ● on ";
    let short_badge = " ✦ codex-alab harness ● on ";
    let badge = if span_w >= unicode_width::UnicodeWidthStr::width(full_badge) + 4 {
        full_badge
    } else if span_w >= unicode_width::UnicodeWidthStr::width(short_badge) + 2 {
        short_badge
    } else {
        return;
    };
    let badge_cols = unicode_width::UnicodeWidthStr::width(badge) as u16;
    let badge_start = x_end.saturating_sub(badge_cols);

    // Draw the ALab rainbow spectrum border rule across the top of the composer.
    let denom = (span_w.max(2) - 1) as f32;
    for i in 0..span_w {
        let x = x_start + i as u16;
        if x >= badge_start && x < x_end {
            continue;
        }
        let t = i as f32 / denom;
        let stop = palette::spectrum_snap_index(t);
        let rgb = palette::mix(painter.ground, palette::spectrum(t), 0.62);
        let cell = &mut buf[(x, y)];
        cell.set_symbol("─");
        if depth != Depth::Mono {
            cell.set_fg(painter.colour(rgb, STOP16[stop]));
        }
    }

    // Draw the right-aligned ALab rainbow kitty harness badge on the top border.
    let mut cx = badge_start;
    let badge_chars: Vec<char> = badge.chars().collect();
    let total_chars = badge_chars.len().max(1);
    for (idx, ch) in badge_chars.into_iter().enumerate() {
        if cx >= x_end {
            break;
        }
        let cell = &mut buf[(cx, y)];
        let mut sym = [0u8; 4];
        cell.set_symbol(ch.encode_utf8(&mut sym));
        if depth != Depth::Mono {
            if ch == '✦' {
                cell.set_fg(painter.colour(palette::GOLD, Color::LightYellow));
                cell.modifier.insert(Modifier::BOLD);
            } else if ch == '●' || idx + 4 >= total_chars {
                cell.set_fg(painter.text_colour(0x4EE088, Color::Green));
                cell.modifier.insert(Modifier::BOLD);
            } else if (3..13).contains(&idx) && badge == full_badge {
                let stop =
                    ((idx - 3) * palette::SPECTRUM_STOPS / 10).min(palette::SPECTRUM_STOPS - 1);
                cell.set_fg(painter.text_colour(palette::SPECTRUM_ANCHORS[stop], STOP16[stop]));
                cell.modifier.insert(Modifier::BOLD);
            } else {
                cell.set_fg(painter.text_colour(painter.hot, painter.hot16()));
            }
        } else if ch == '✦' || ch == '●' {
            cell.modifier.insert(Modifier::BOLD);
        }
        cx += 1;
    }
}

/// The trail position of column `c` (0 at the tail): 0 red where it touches the kitty, 1 violet
/// at the far tail.
fn column_t(c: usize, trail: usize) -> f32 {
    let dist = trail - c;
    (dist - 1) as f32 / (trail.max(2) - 1) as f32
}

/// Whether a pixel inks in mono: the shape only. The kitty's features (eyes, ears, nose) become
/// holes and stripes keep every other band (red, yellow, blue, violet), so the art still reads.
fn ink(px: Option<Px>) -> bool {
    match px {
        None => false,
        Some(px) if px.alpha < CUT_ALPHA => false,
        Some(Px {
            role: Role::Kitty(b'E' | b'H' | b'K' | b'N' | b'P'),
            ..
        }) => false,
        Some(Px {
            role: Role::Band(k),
            ..
        }) => k % 2 == 0,
        Some(_) => true,
    }
}

/// A named colour on a light theme: bright yellow (93) is near-invisible on white, so it takes
/// the theme's darker yellow (33).
fn light16(c: Color) -> Color {
    match c {
        Color::LightYellow => Color::Yellow,
        other => other,
    }
}

fn gold() -> Star {
    Star {
        glyph: '✧',
        rgb: palette::GOLD,
        ansi16: Color::LightYellow,
    }
}

/// One row of cells as a line: trailing sky trimmed, equal styles merged into one span.
fn into_line(mut cells: Vec<Cell>) -> Line<'static> {
    while cells.last() == Some(&Cell::BLANK) {
        cells.pop();
    }
    let spans = cells
        .into_iter()
        .map(|cell| Span::styled(cell.glyph.to_string(), cell.style))
        .collect();
    merge(spans)
}

fn merge(spans: Vec<Span<'static>>) -> Line<'static> {
    let mut out: Vec<Span<'static>> = Vec::with_capacity(spans.len());
    for span in spans {
        match out.last_mut() {
            Some(last) if last.style == span.style => {
                last.content.to_mut().push_str(&span.content);
            }
            _ => out.push(span),
        }
    }
    Line::from(out)
}

// codex's clippy policy disallows these constructors because unchecked colours fight terminal
// themes; the banner checks its own (light-theme rescue, per-depth mapping), as upstream's
// `terminal_palette::{rgb_color, indexed_color}` do.
#[allow(clippy::disallowed_methods)]
fn rgb_color((r, g, b): (u8, u8, u8)) -> Color {
    Color::Rgb(r, g, b)
}

#[allow(clippy::disallowed_methods)]
fn indexed_color(index: u8) -> Color {
    Color::Indexed(index)
}
