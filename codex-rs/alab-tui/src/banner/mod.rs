// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The rainbow kitty launch banner: aterm's codex kitty riding aterm's spectrum, drawn once per
//! process above codex's session header.
//!
//! This module only renders: it knows nothing of codex-tui. `codex-rs/alab-tui/glue/
//! banner_cell.rs`, compiled into codex-tui with `#[path]` (codex's `HistoryCell` is
//! `pub(crate)`), wraps a [`Banner`] and upstream's session header in one history cell, part 0
//! of the launch `SessionInfoCell`. [`blossom_stage`] is the second hook's: it keeps codex's
//! empty-state blossom on screen under the longer transcript. [`render_composer_border`] is the
//! third's, through `glue/composer_border.rs`: the border along the composer's top row while
//! the codex-alab harness is on.
//!
//! The art is a pixel canvas, two pixels per cell row (`▀`/`▄`/`█`, top pixel in the
//! foreground, bottom in the background), redrawn for every width so it never wraps: each line
//! is at most `width - 1` columns, and narrow widths step down a ladder to the wordmark alone,
//! then nothing. Transparent pixels set no colour, so the terminal's own background shows
//! through (aterm's "sky, never a slab"). Every row opens with two spaces and the banner holds
//! none of the marks or phrases aterm's codex screen reader keys on.

pub mod palette;
mod render;
pub(crate) mod sprite;

use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::Line;
use std::fmt::Write as _;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Which trail the kitty rides.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Style {
    /// aterm's sweep: one spectrum position per column, red at the kitty, violet at the tail.
    #[default]
    Ribbon,
    /// Classic Nyan: seven one-pixel bands, red on top, stepping a pixel every eight columns.
    Stripes,
}

/// What the terminal can draw.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Depth {
    /// 24-bit colour.
    TrueColor,
    /// The xterm-256 fixed colours (16..=255; 0..=15 belong to the theme).
    Ansi256,
    /// The sixteen named ANSI colours, chosen by role.
    Ansi16,
    /// No colour at all (`NO_COLOR`, `TERM=dumb`): shape only, bold and dim.
    Mono,
}

/// The terminal's background, as far as it is known.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Theme {
    bg: Option<palette::Rgb>,
}

impl Theme {
    /// The theme of a terminal that reported `bg` (or nothing).
    pub fn from_bg(bg: Option<(u8, u8, u8)>) -> Self {
        Self {
            bg: bg.map(palette::pack),
        }
    }

    /// Whether the background is known and light (codex's `color::is_light`).
    pub fn is_light(self) -> bool {
        self.bg.is_some_and(palette::is_light)
    }

    /// The background to blend and measure against: the reported one, else codex's dark fallback.
    fn ground(self) -> palette::Rgb {
        self.bg.unwrap_or(palette::FALLBACK_DARK_BG)
    }
}

/// One banner: what it draws, for any width.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Banner {
    style: Style,
    depth: Depth,
    theme: Theme,
    sha7: Option<String>,
}

impl Banner {
    /// A banner for `build_commit` (codex's `BuildInfo::build_commit()`): the label names its
    /// first seven hex digits as `g<sha7>`, aterm's build-metadata convention, and names no
    /// commit for `dev`, `unknown` or anything else that is not a hex sha.
    pub fn new(style: Style, depth: Depth, theme: Theme, build_commit: &str) -> Self {
        let sha = build_commit.trim();
        let sha7 = (sha.len() >= 7 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| sha[..7].to_ascii_lowercase());
        Self {
            style,
            depth,
            theme,
            sha7,
        }
    }

    /// The banner's lines at `width` columns; each is at most `width - 1` columns wide. Below 13
    /// columns there are none.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        render::render(self, usize::from(width))
    }

    /// The banner's lines at `width` columns, appending the ALab harness status banner row when
    /// `harness_active` is `true`.
    pub fn lines_with_harness(&self, width: u16, harness_active: bool) -> Vec<Line<'static>> {
        render::render_with_harness(self, usize::from(width), harness_active)
    }
}

/// The running session's harness state (`Some(true)` when Autopilot is `Active`, `Some(false)`
/// when toggled off to `Passive`, `None` when unclaimed or under unit tests).
pub fn harness_state() -> Option<bool> {
    match crate::boxes::tick_shared()?.autopilot() {
        codex_alab_core::session::Autopilot::Active => Some(true),
        codex_alab_core::session::Autopilot::Passive => Some(false),
        codex_alab_core::session::Autopilot::Off => None,
    }
}

/// Draws the ALab rainbow kitty border and `codex-alab harness ● on` badge along the top row of
/// the composer block. The caller decides when: `glue/composer_border.rs` draws it while
/// [`harness_state`] is `Some(true)`.
pub fn render_composer_border(
    composer_rect: Rect,
    buf: &mut ratatui::buffer::Buffer,
    depth: Depth,
    theme: Theme,
) {
    render::render_composer_border(composer_rect, buf, depth, theme);
}

/// Whether to draw the banner, and in which style. First match wins:
/// 1. `suppressed` (upstream's own unit tests) → off;
/// 2. `ALAB_BANNER` (`env`), trimmed, any case: `off|0|false|no` → off, `on|1|true|yes` → the
///    default style, `ribbon|stripes` → that style; an explicit value beats rule 3, and anything
///    else is ignored;
/// 3. upstream's welcome-artwork predicate (`tui.animations && tui.effects.welcome`, which
///    already honours reduced motion and screen readers) → the default style, else off.
pub fn decide(env: Option<&str>, welcome: bool, suppressed: bool) -> Option<Style> {
    if suppressed {
        return None;
    }
    match env.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("off" | "0" | "false" | "no") => None,
        Some("on" | "1" | "true" | "yes") => Some(Style::default()),
        Some("ribbon") => Some(Style::Ribbon),
        Some("stripes") => Some(Style::Stripes),
        _ => welcome.then_some(Style::default()),
    }
}

/// A once-only claim: the first [`LaunchSlot::claim`] wins, every later one loses.
#[derive(Debug, Default)]
pub struct LaunchSlot(AtomicBool);

impl LaunchSlot {
    /// An unclaimed slot.
    pub const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    /// `true` for the first caller only.
    pub fn claim(&self) -> bool {
        !self.0.swap(true, Ordering::AcqRel)
    }
}

static LAUNCH_SLOT: LaunchSlot = LaunchSlot::new();

/// The process's launch slot: the first session header built (fresh or resumed) takes it, and
/// only that header may carry the banner, whether or not the switch let it; a later `/new`,
/// `/resume`, fork or thread switch never does, even after a settings change.
pub fn claim_launch_slot() -> bool {
    LAUNCH_SLOT.claim()
}

static DRAWN: AtomicBool = AtomicBool::new(false);

/// Records that this process drew the banner. The glue calls it when it inserts one (never
/// under codex-tui's own tests); [`blossom_stage`] reads it.
pub fn note_drawn() {
    DRAWN.store(true, Ordering::Release);
}

/// Where codex's empty-state blossom draws (`tui/src/empty_state_animation.rs`
/// `render_first_screen`, an `ALAB(banner)` hook): [`place_blossom`] with whether this process
/// drew the banner. Without the banner it is upstream's `stage`, unchanged.
pub fn blossom_stage(stage: Rect, available: Rect) -> Rect {
    place_blossom(stage, available, DRAWN.load(Ordering::Acquire))
}

/// The fewest rows upstream draws the blossom in (`render_first_screen`'s `MIN_STAGE_ROWS`).
const BLOSSOM_MIN_ROWS: u16 = 14;

/// Upstream centres the blossom's stage (21 rows at 60 columns) on the screen and draws nothing
/// when that overlaps the transcript. The banner adds rows above the header, so on a screen of
/// about 30 to 46 rows the centred stage meets the header and the blossom is gone. Once the
/// banner is drawn, a stage that does not fit centred moves to the middle of the free area
/// `available` (under the transcript, above the composer); where it is too tall for that area it
/// shrinks, keeping upstream's own proportion (rows = columns × height / width, as upstream sizes
/// it on narrow screens) and upstream's floor of 14 rows. Otherwise it stays as it was, and
/// upstream cancels it as before.
pub fn place_blossom(stage: Rect, available: Rect, banner_drawn: bool) -> Rect {
    if !banner_drawn
        || stage.width == 0
        || stage.height == 0
        || stage.intersection(available) == stage
        || stage.x < available.x
        || stage.right() > available.right()
    {
        return stage;
    }
    let rows_at = |width: u16| u32::from(width) * u32::from(stage.height) / u32::from(stage.width);
    let Some(width) = (1..=stage.width)
        .rev()
        .find(|&width| rows_at(width) <= u32::from(available.height))
    else {
        return stage;
    };
    let height = u16::try_from(rows_at(width)).unwrap_or(u16::MAX);
    if height < BLOSSOM_MIN_ROWS.min(stage.height) {
        return stage;
    }
    Rect {
        x: stage.x + (stage.width - width) / 2,
        y: available.y + (available.height - height) / 2,
        width,
        height,
    }
}

/// `lines` in the review format the snapshots use: each row's glyphs between `|` marks, then a
/// legend with one entry per styled run, `r<row> [<from>..<to>) fg=… bg=… bold dim`, in display
/// columns. A colour is `#RRGGBB`, `i<n>` for an xterm-256 index, or the ANSI name.
pub fn review_text(lines: &[Line<'_>]) -> String {
    let mut out = String::new();
    for (row, line) in lines.iter().enumerate() {
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        let _ = writeln!(out, "{row:>2}|{text}|");
    }
    out.push_str("--\n");
    for (row, line) in lines.iter().enumerate() {
        let mut col = 0usize;
        for span in &line.spans {
            let w = unicode_width::UnicodeWidthStr::width(span.content.as_ref());
            let style = span.style.patch(line.style);
            let mut desc = String::new();
            if let Some(fg) = style.fg {
                let _ = write!(desc, " fg={}", colour_name(fg));
            }
            if let Some(bg) = style.bg {
                let _ = write!(desc, " bg={}", colour_name(bg));
            }
            let m = style.add_modifier;
            if m.contains(ratatui::style::Modifier::BOLD) {
                desc.push_str(" bold");
            }
            if m.contains(ratatui::style::Modifier::DIM) {
                desc.push_str(" dim");
            }
            if !desc.is_empty() {
                let _ = writeln!(out, "r{row} [{col}..{}){desc}", col + w);
            }
            col += w;
        }
    }
    out
}

fn colour_name(c: Color) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("#{r:02X}{g:02X}{b:02X}"),
        Color::Indexed(i) => format!("i{i}"),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

#[cfg(test)]
mod tests;
