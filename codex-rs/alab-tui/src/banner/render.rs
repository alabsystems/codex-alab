// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Canvas → half-block cells → merged spans, for one width.

use super::Banner;
use super::Depth;
use super::Mode;
use super::Style;
use super::claims::Run;
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
use std::cell::RefCell;

/// Every row opens with this many blank columns, in line with upstream's `>_ OpenAI Codex`.
const INDENT: usize = 2;
/// The wordmark, one snapped spectrum stop per letter.
const WORDMARK: &str = "codex-alab";
/// The owner's tagline, in his words: one line where it fits, else a part a line.
const TAGLINE: [&str; 3] = ["bloatware-free", "nag-free", "optimized for aterm"];
/// What joins the tagline's parts, and the claims, on one line.
const JOIN: &str = " · ";
/// Text rows: one beside each art row, and no more without the art.
const TEXT_ROWS: usize = KITTY_H / 2;
/// Blank columns between the kitty and the text.
const GAP: usize = 3;
/// The shortest ribbon beside the text; with less room the kitty goes without one.
const TRAIL_MIN: usize = 8;
/// The longest ribbon beside the text.
const TRAIL_MAX: usize = 28;
/// The green of a measured figure: the harness row's.
const FIGURE_GREEN: Rgb = 0x4EE088;
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

pub(super) fn render(banner: &Banner, width: usize) -> Vec<Line<'static>> {
    render_with_harness(banner, width, None)
}

/// The banner at `width`; with `harness` (the harness on, under that Autopilot word) in the
/// word's palette and with the harness row under the text.
pub(super) fn render_with_harness(
    banner: &Banner,
    width: usize,
    harness: Option<Mode>,
) -> Vec<Line<'static>> {
    if width < INDENT + WORDMARK.len() + 1 {
        return Vec::new();
    }
    let ramp = match harness {
        Some(Mode::Off) => Ramp::Moonbow,
        Some(Mode::Cruise | Mode::Land) | None => Ramp::Spectrum,
    };
    let painter = Painter::new(banner, ramp);
    let mut lines = vec![Line::default()];
    // The room right of the kitty alone. The text column takes what it needs of it, and the
    // ribbon what is left, so every art row ends by `INDENT + trail + KITTY_W` and every text
    // row by `width - 1`.
    let room = width.saturating_sub(1 + INDENT + KITTY_W + GAP);
    // Beside the kitty only with the whole tagline in two lines at most; otherwise the text
    // stands alone, where it may take more.
    let text = painter.text(room, 2);
    if text.len() > 1 {
        let widest = text.iter().map(Line::width).max().unwrap_or(0);
        let trail = room.saturating_sub(widest).min(TRAIL_MAX);
        let trail = if trail >= TRAIL_MIN { trail } else { 0 };
        let sky = match trail {
            24.. => 3,
            16.. => 2,
            _ => 0,
        };
        let at = INDENT + trail + KITTY_W + GAP;
        let top = (TEXT_ROWS - text.len()) / 2;
        let mut text = text.into_iter();
        for (row, cells) in painter.art(trail, sky).into_iter().enumerate() {
            let beside = if row >= top { text.next() } else { None };
            lines.push(beside_art(cells, at, beside));
        }
    } else {
        let indent = || Span::raw(" ".repeat(INDENT));
        lines.extend(
            painter
                .text(width - 1 - INDENT, TEXT_ROWS - 1)
                .into_iter()
                .map(|line| merge([vec![indent()], line.spans].concat())),
        );
    }
    if let Some(mode) = harness
        && let Some(harness) = painter.autopilot_line(width, mode)
    {
        lines.push(harness);
    }
    lines
}

/// The colours the trail, its stars and the lettering run through.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ramp {
    /// aterm's spectrum, red to violet.
    Spectrum,
    /// aterm's moonbow, worn under Autopilot off: white to steel on a dark ground, pearl to
    /// graphite on a light one.
    Moonbow,
}

struct Painter<'a> {
    banner: &'a Banner,
    ramp: Ramp,
    light: bool,
    ground: Rgb,
    /// White-hot on a dark ground, aterm's ink on a light one.
    hot: Rgb,
}

impl<'a> Painter<'a> {
    fn new(banner: &'a Banner, ramp: Ramp) -> Self {
        let light = banner.theme.is_light();
        Self {
            banner,
            ramp,
            light,
            ground: banner.theme.ground(),
            hot: if light {
                palette::INK
            } else {
                palette::WHITE_HOT
            },
        }
    }

    /// The ramp's colour at trail position `t` (0 at the kitty).
    fn ramp_at(&self, t: f32) -> Rgb {
        match self.ramp {
            Ramp::Spectrum => palette::spectrum(t),
            Ramp::Moonbow => palette::moonbow(t, self.light),
        }
    }

    /// Named stop `k` (0..[`palette::SPECTRUM_STOPS`]) of the ramp: the spectrum's anchors, or
    /// the moonbow at even steps.
    fn stop(&self, k: usize) -> Rgb {
        let k = k.min(palette::SPECTRUM_STOPS - 1);
        match self.ramp {
            Ramp::Spectrum => palette::SPECTRUM_ANCHORS[k],
            Ramp::Moonbow => {
                palette::moonbow(k as f32 / (palette::SPECTRUM_STOPS - 1) as f32, self.light)
            }
        }
    }

    /// Named stop `k` of the ramp as an ANSI name: the spectrum's colours, or the moonbow's
    /// greys (bright white to grey on a dark ground, dark grey to black on a light one).
    fn stop16(&self, k: usize) -> Color {
        let k = k.min(palette::SPECTRUM_STOPS - 1);
        match (self.ramp, self.light, k < palette::SPECTRUM_STOPS / 2) {
            (Ramp::Spectrum, ..) => STOP16[k],
            (Ramp::Moonbow, false, true) => Color::White,
            (Ramp::Moonbow, false, false) => Color::Gray,
            (Ramp::Moonbow, true, true) => Color::DarkGray,
            (Ramp::Moonbow, true, false) => Color::Black,
        }
    }

    /// The gold star, or under the moonbow aterm's pearl glint.
    fn gold(&self) -> Star {
        match self.ramp {
            Ramp::Spectrum => Star {
                glyph: '✧',
                rgb: palette::GOLD,
                ansi16: Color::LightYellow,
            },
            Ramp::Moonbow => Star {
                glyph: '✧',
                rgb: palette::moonbow(1.0 / 3.0, self.light),
                ansi16: self.stop16(0),
            },
        }
    }

    /// The five art rows: a 10-pixel canvas folded into half blocks, then `sky` stars on the
    /// ribbon's `trail` columns.
    fn art(&self, trail: usize, sky: usize) -> Vec<Vec<Cell>> {
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
                    for k in 0..palette::SPECTRUM_STOPS {
                        let y = 2 + k + off;
                        if y < KITTY_H {
                            px[y][x] = Some(Px {
                                rgb: self.stop(k),
                                alpha,
                                role: Role::Band(k),
                            });
                        }
                    }
                }
                Style::Ribbon | Style::Codex => {
                    let wave = (c as f64 * 2.0 * std::f64::consts::PI / 16.0).sin() > 0.3;
                    let off = usize::from(wave);
                    let t = column_t(c, trail);
                    let mut rgb = self.ramp_at(t);
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
        for (k, f) in [0.14, 0.42, 0.71].into_iter().take(sky).enumerate() {
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
            for (k, f) in [0.27, 0.58, 0.86].into_iter().take(sky).enumerate() {
                let c = at(f);
                let star = if k == 1 {
                    self.gold()
                } else {
                    self.m3(palette::spectrum_snap_index(column_t(c, trail)))
                };
                self.put(&mut rows[4], INDENT + c, INDENT + trail, star);
            }
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
            Role::Ribbon(stop) | Role::Band(stop) => self.stop16(stop),
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
        // Luma's light/dark classification can select an endpoint below the text floor
        // on middle-gray backgrounds. White or black always reaches the 4.5:1 floor.
        let toward = if palette::contrast(toward, self.ground) >= floor {
            toward
        } else if palette::contrast(palette::WHITE_HOT, self.ground) >= floor {
            palette::WHITE_HOT
        } else {
            0x00_0000
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
            rgb: palette::mix(self.hot, self.stop(stop), palette::M2_BODY_TINT),
            ansi16: if self.light {
                self.stop16(stop)
            } else {
                Color::White
            },
        }
    }

    /// A grain: the pure stop of the column it rose from.
    fn m3(&self, stop: usize) -> Star {
        Star {
            glyph: '∙',
            rgb: self.stop(stop),
            ansi16: self.stop16(stop),
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

    /// The text for `room` columns, at most [`TEXT_ROWS`] lines no wider than the tagline on one
    /// line: the wordmark and build, the tagline (whole in at most `tagline_rows` lines, or not at
    /// all), the claims (as many as fit, in the table's order), and the upstream source while a
    /// row is left. Without room for the wordmark, nothing.
    fn text(&self, room: usize, tagline_rows: usize) -> Vec<Line<'static>> {
        let column = room.min(width(&TAGLINE.join(JOIN)));
        let Some(head) = self.wordmark(column) else {
            return Vec::new();
        };
        let mut rows = vec![head];
        let tagline: Vec<_> = TAGLINE.iter().map(|part| vec![Span::raw(*part)]).collect();
        let (lines, packed) = pack(&tagline, column, tagline_rows);
        if packed < tagline.len() {
            return rows;
        }
        rows.extend(lines);
        let upstream = self.upstream(column);
        let remaining = TEXT_ROWS - rows.len();
        // Keep source attribution beside a complete two-row measurement block where
        // all three fit; with less room the qualified measurements retain priority.
        let claim_rows = if upstream.is_some() && remaining >= 3 {
            remaining - 1
        } else {
            remaining
        };
        rows.extend(self.claims(column, claim_rows));
        if rows.len() < TEXT_ROWS
            && let Some(upstream) = upstream
        {
            rows.push(upstream);
        }
        rows
    }

    /// The wordmark in snapped stops, bold, then the readable ALab version where it fits.
    fn wordmark(&self, column: usize) -> Option<Line<'static>> {
        let letters = WORDMARK.chars().count();
        if column < letters {
            return None;
        }
        let mut spans = Vec::with_capacity(letters + 1);
        for (i, ch) in WORDMARK.chars().enumerate() {
            let stop = (i * palette::SPECTRUM_STOPS / letters).min(palette::SPECTRUM_STOPS - 1);
            let mut style = CellStyle::new().add_modifier(Modifier::BOLD);
            if self.banner.depth != Depth::Mono {
                style = style.fg(self.text_colour(self.stop(stop), self.stop16(stop)));
            }
            spans.push(Span::styled(ch.to_string(), style));
        }
        if let Some(version) = &self.banner.version {
            let build = format!("  v{version}");
            if letters + width(&build) <= column {
                spans.push(Span::styled(build, dim()));
            }
        }
        Some(merge(spans))
    }

    /// A dated comparison of default configurations, then the claims in the remaining rows.
    /// The qualification stays with every figure: these are measurements, not live facts
    /// about a session whose configuration may enable the daemon or other services again.
    fn claims(&self, column: usize, rows: usize) -> Vec<Line<'static>> {
        let forms: Vec<_> = self
            .banner
            .claims
            .iter()
            .filter_map(|claim| {
                let forms = claim.forms();
                (!forms.is_empty()).then_some((claim, forms))
            })
            .collect();
        let Some((comparison, _)) = forms.first() else {
            return Vec::new();
        };
        if rows < 2
            || forms.iter().any(|(claim, _)| {
                claim.against != comparison.against || claim.measured != comparison.measured
            })
        {
            return Vec::new();
        }
        let Some(qualification) = [
            format!(
                "Measured defaults vs Codex {} · {}",
                comparison.against, comparison.measured
            ),
            format!(
                "Defaults vs {} · {}",
                comparison.against, comparison.measured
            ),
        ]
        .into_iter()
        .find(|line| width(line) <= column) else {
            return Vec::new();
        };
        let longest = forms
            .iter()
            .map(|(_, forms)| forms.len())
            .max()
            .unwrap_or(0);
        let mut lines = Vec::new();
        for form in 0..longest {
            let items: Vec<_> = forms
                .iter()
                .map(|(_, forms)| self.claim(&forms[form.min(forms.len() - 1)]))
                .collect();
            let (fitted, packed) = pack(&items, column, rows - 1);
            lines = fitted;
            if packed == items.len() {
                break;
            }
        }
        if !lines.is_empty() {
            lines.insert(0, Line::from(Span::styled(qualification, dim())));
        }
        lines
    }

    /// One claim's words: its figures bold, in the harness green where there is colour.
    fn claim(&self, runs: &[Run]) -> Vec<Span<'static>> {
        let figure = match self.banner.depth {
            Depth::Mono => CellStyle::new(),
            _ => CellStyle::new().fg(self.text_colour(FIGURE_GREEN, Color::Green)),
        }
        .add_modifier(Modifier::BOLD);
        runs.iter()
            .map(|run| match run {
                Run::Figure(text) => Span::styled(text.clone(), figure),
                Run::Words(text) => Span::raw(text.clone()),
            })
            .collect()
    }

    /// The upstream release baseline, dim, with exact source metadata left in version details.
    fn upstream(&self, column: usize) -> Option<Line<'static>> {
        let release = crate::interface::readable_version(self.banner.upstream.as_deref()?);
        [
            format!("based on OpenAI Codex {release}"),
            format!("based on Codex {release}"),
        ]
        .into_iter()
        .find(|line| width(line) <= column)
        .map(|line| Line::from(Span::styled(line, dim())))
    }

    /// The native Autopilot status under the launch banner. The product version lives
    /// in the title; there is no second harness identity or version.
    fn autopilot_line(&self, width: usize, mode: Mode) -> Option<Line<'static>> {
        const STAR: &str = "✦ ";
        let brand = "Autopilot";
        let word = format!(" {}", mode.label());
        if INDENT + self::width(STAR) + self::width(brand) + self::width(&word) >= width {
            return None;
        }
        let mono = self.banner.depth == Depth::Mono;
        let bold = CellStyle::new().add_modifier(Modifier::BOLD);
        let mut spans = vec![Span::raw(" ".repeat(INDENT))];
        let star = match (mono, self.ramp) {
            (true, _) => bold,
            (false, Ramp::Spectrum) => bold.fg(self.colour(palette::GOLD, Color::LightYellow)),
            (false, Ramp::Moonbow) => bold.fg(self.colour(self.hot, self.hot16())),
        };
        spans.push(Span::styled(STAR, star));
        let brand_chars = brand.chars().count().max(1);
        for (i, ch) in brand.chars().enumerate() {
            let stop = (i * palette::SPECTRUM_STOPS / brand_chars).min(palette::SPECTRUM_STOPS - 1);
            let mut style = bold;
            if !mono {
                style = style.fg(self.text_colour(self.stop(stop), self.stop16(stop)));
            }
            spans.push(Span::styled(ch.to_string(), style));
        }
        let tail_style = match (mono, mode) {
            (true, _) => bold,
            // Pearl on a dark ground, graphite on a light one.
            (false, Mode::Off) if self.light => {
                bold.fg(self.text_colour(palette::moonbow(1.0, true), Color::Black))
            }
            (false, Mode::Off) => {
                bold.fg(self.text_colour(palette::moonbow(1.0 / 3.0, false), Color::White))
            }
            (false, Mode::Cruise | Mode::Land) => {
                bold.fg(self.text_colour(FIGURE_GREEN, Color::Green))
            }
        };
        spans.push(Span::styled(word, tail_style));
        Some(merge(spans))
    }
}

/// One palette per render thread: animation ticks reuse color quantization while a width,
/// depth or background change replaces the entry. Old resize palettes do not accumulate.
struct ComposerColors {
    key: (u16, Depth, super::Theme),
    values: Vec<Color>,
}

impl ComposerColors {
    fn new(key: (u16, Depth, super::Theme)) -> Self {
        let (width, depth, theme) = key;
        let banner = Banner::new(Style::Ribbon, depth, theme, "");
        let painter = Painter::new(&banner, Ramp::Spectrum);
        let denom = f32::from(width.max(2) - 1);
        let values = (0..width)
            .map(|column| {
                let t = f32::from(column) / denom;
                let stop = palette::spectrum_snap_index(t);
                painter.text_colour(palette::spectrum(t), STOP16[stop])
            })
            .collect();
        Self { key, values }
    }
}

thread_local! {
    static COMPOSER_COLORS: RefCell<Option<ComposerColors>> = const { RefCell::new(None) };
}

pub(super) fn tint_composer_rule(
    area: ratatui::layout::Rect,
    buf: &mut ratatui::buffer::Buffer,
    depth: Depth,
    theme: super::Theme,
) {
    let area = area.intersection(buf.area);
    if area.is_empty() || depth == Depth::Mono {
        return;
    }
    let key = (area.width, depth, theme);
    COMPOSER_COLORS.with_borrow_mut(|cached| {
        let colors = cached.get_or_insert_with(|| ComposerColors::new(key));
        if colors.key != key {
            *colors = ComposerColors::new(key);
        }
        for (x, color) in (area.x..area.right()).zip(&colors.values) {
            buf[(x, area.y)].set_fg(*color);
        }
    });
}

/// `rgb` as overline text at `depth` on `theme`'s ground, floored as the spectrum through the
/// overline's labels is ([`Painter::clearing`] at [`palette::TEXT_CONTRAST_FLOOR`]); `ansi16`
/// where the terminal draws named colours.
pub(super) fn readable_text(rgb: Rgb, ansi16: Color, depth: Depth, theme: super::Theme) -> Color {
    let banner = Banner::new(Style::Ribbon, depth, theme, "");
    Painter::new(&banner, Ramp::Spectrum).clearing(rgb, ansi16, palette::TEXT_CONTRAST_FLOOR)
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

/// `items` in order, in at most `rows` lines of `column`, joined by a dim [`JOIN`] where two
/// share a line; it stops before the first item that does not fit. Returns the lines and how
/// many items they hold.
fn pack(items: &[Vec<Span<'static>>], column: usize, rows: usize) -> (Vec<Line<'static>>, usize) {
    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    let mut used = 0;
    let mut packed = 0;
    for item in items {
        let w: usize = item.iter().map(Span::width).sum();
        let row_left = lines.len() < rows;
        match lines.last_mut() {
            Some(line) if used + width(JOIN) + w <= column => {
                line.push(Span::styled(JOIN, dim()));
                line.extend(item.iter().cloned());
                used += width(JOIN) + w;
            }
            _ if w <= column && row_left => {
                lines.push(item.clone());
                used = w;
            }
            _ => break,
        }
        packed += 1;
    }
    (lines.into_iter().map(merge).collect(), packed)
}

fn width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

fn dim() -> CellStyle {
    CellStyle::new().add_modifier(Modifier::DIM)
}

/// An art row with `text` beside it from column `at`, which the art never reaches; with no text,
/// the art row alone.
fn beside_art(mut cells: Vec<Cell>, at: usize, text: Option<Line<'static>>) -> Line<'static> {
    let Some(text) = text else {
        return into_line(cells);
    };
    cells.resize(at, Cell::BLANK);
    let mut spans: Vec<Span<'static>> = cells
        .into_iter()
        .map(|cell| Span::styled(cell.glyph.to_string(), cell.style))
        .collect();
    spans.extend(text.spans);
    merge(spans)
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
pub(super) fn rgb_color((r, g, b): (u8, u8, u8)) -> Color {
    Color::Rgb(r, g, b)
}

#[allow(clippy::disallowed_methods)]
pub(super) fn indexed_color(index: u8) -> Color {
    Color::Indexed(index)
}

#[cfg(test)]
mod composer_tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    #[test]
    fn cached_composer_colors_match_fresh_paints_after_terminal_changes() {
        let dark = super::super::Theme::from_bg(Some((16, 20, 24)));
        let light = super::super::Theme::from_bg(Some((255, 255, 255)));
        let gray = super::super::Theme::from_bg(Some((120, 120, 120)));
        let area = Rect::new(3, 1, 80, 1);
        COMPOSER_COLORS.with_borrow_mut(|cached| *cached = None);
        for (depth, theme, area) in [
            (Depth::TrueColor, dark, area),
            (Depth::TrueColor, dark, Rect::new(12, 3, 80, 1)),
            (Depth::TrueColor, dark, Rect::new(3, 1, 32, 1)),
            (Depth::TrueColor, dark, area),
            (Depth::Ansi256, dark, area),
            (Depth::Ansi16, dark, area),
            (Depth::TrueColor, dark, area),
            (Depth::TrueColor, light, area),
            (Depth::TrueColor, gray, area),
            (Depth::TrueColor, super::super::Theme::default(), area),
            (Depth::TrueColor, dark, area),
            (Depth::Mono, dark, area),
            (Depth::TrueColor, dark, Rect::new(3, 1, 0, 1)),
            (Depth::TrueColor, dark, Rect::new(3, 1, 80, 0)),
            (Depth::TrueColor, dark, Rect::new(60, 2, 80, 1)),
        ] {
            let mut original = Buffer::empty(Rect::new(0, 0, 100, 5));
            for cell in &mut original.content {
                cell.set_symbol("x").set_fg(Color::Green);
            }
            let mut cached = original.clone();
            tint_composer_rule(area, &mut cached, depth, theme);
            COMPOSER_COLORS.with_borrow_mut(|cached| *cached = None);
            tint_composer_rule(area, &mut original, depth, theme);
            assert_eq!(cached, original, "{depth:?}/{theme:?}/{area:?}");
        }
    }
}
