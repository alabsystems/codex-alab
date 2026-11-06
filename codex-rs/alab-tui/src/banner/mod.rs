// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The rainbow kitty launch banner: aterm's codex kitty riding aterm's spectrum, drawn once per
//! process above codex's session header.
//!
//! This module only renders: it knows nothing of codex-tui. `codex-rs/alab-tui/glue/
//! banner_cell.rs`, compiled into codex-tui with `#[path]` (codex's `HistoryCell` is
//! `pub(crate)`), wraps a [`Banner`] and upstream's session header in one history cell, part 0
//! of the launch `SessionInfoCell`. [`blossom_stage`] keeps codex's empty-state blossom on
//! screen under the longer transcript. The composer's top border is rendered separately by
//! [`crate::interface::render`] through `glue/composer_border.rs`.
//!
//! One block: the kitty on a short ribbon, with the text beside it in the same five rows: the
//! wordmark and build, the owner's tagline, what [`claims::CLAIMS`] measured against upstream
//! codex, and the upstream source the build is based on. The art is a pixel canvas, two pixels
//! per cell row (`▀`/`▄`/`█`, top pixel in the foreground, bottom in the background), redrawn
//! for every width so it never wraps: each line is at most `width - 1` columns. Narrower, the
//! ribbon goes first, then the claims shorten and the upstream line goes, then the text stands
//! alone without the kitty, down to the wordmark alone, then nothing. Transparent pixels set no
//! colour, so the terminal's own background shows through (aterm's "sky, never a slab"). Every
//! row opens with two spaces, in line with upstream's `>_ OpenAI Codex` header.

pub mod claims;
pub mod palette;
mod render;
pub(crate) mod sprite;

use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::Line;
use std::fmt::Write as _;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Whether the banner may draw: only in the codex-alab binary (for the glue; codex-tui does not
/// depend on alab-core).
pub use codex_alab_core::active;

/// The upstream source this build includes, as `codex_alab_core::version::upstream()` spells
/// it, when the packager stamped it; an unstamped source build names none rather than its
/// `unreleased`.
pub fn stamped_upstream() -> Option<String> {
    use codex_alab_core::version;
    (version::upstream_release().is_some() || version::upstream_commit().is_some())
        .then(version::upstream)
}

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
    version: Option<String>,
    upstream: Option<String>,
    claims: &'static [claims::Claim],
}

impl Banner {
    /// A banner for the running ALab package version. The primary wordmark keeps its
    /// readable release/build number; `/alab version` carries exact source identities.
    pub fn new(style: Style, depth: Depth, theme: Theme, build_version: &str) -> Self {
        let version = crate::interface::readable_version(build_version.trim());
        let version = match version {
            "" | "unknown" => None,
            "0.0.0" => Some("dev".to_owned()),
            version => Some(version.to_owned()),
        };
        Self {
            style,
            depth,
            theme,
            version,
            upstream: None,
            claims: &[],
        }
    }

    /// The same banner naming the upstream source it was built from, as
    /// `codex_alab_core::version::upstream()` spells it (`<release>+g<commit>`). Its readable
    /// baseline is drawn; `None`, for an unstamped build, draws no upstream line.
    pub fn with_upstream(mut self, upstream: Option<String>) -> Self {
        self.upstream = upstream.filter(|upstream| !upstream.trim().is_empty());
        self
    }

    /// The same banner advertising `claims` ([`claims::CLAIMS`]), as many as fit.
    pub fn with_claims(mut self, claims: &'static [claims::Claim]) -> Self {
        self.claims = claims;
        self
    }

    /// The banner's lines at `width` columns; each is at most `width - 1` columns wide. Below 13
    /// columns there are none.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        render::render(self, usize::from(width))
    }

    /// The banner's lines at `width` columns as `harness` shows it ([`harness_state`]): with the
    /// harness on, in its Autopilot word's palette (the moonbow under Off) and with the harness
    /// row under the text; otherwise the plain banner, as [`Banner::lines`].
    pub fn lines_with_harness(&self, width: u16, harness: Option<Harness>) -> Vec<Line<'static>> {
        let mode = match harness {
            Some(Harness::On(mode)) => Some(mode),
            Some(Harness::Unsupervised) | None => None,
        };
        render::render_with_harness(self, usize::from(width), mode)
    }
}

/// The session's Autopilot word under aterm's harness, as its glass names it: the harness row
/// ends with it, and the banner wears its palette (the spectrum, and aterm's white moonbow
/// under Off).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Mode {
    /// `∞ cruise`.
    Cruise,
    /// `⤓ land`.
    Land,
    /// `○ off`: the harness on, Autopilot off.
    Off,
}

impl Mode {
    /// The word and its glyph, as aterm's glass spells them.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Cruise => "∞ cruise",
            Mode::Land => "⤓ land",
            Mode::Off => "○ off",
        }
    }
}

/// aterm's harness, as this session's chrome shows it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Harness {
    /// The whole harness is on: aterm hosts the session and its policy supports codex-alab,
    /// under this Autopilot word. With [`Mode::Cruise`] or [`Mode::Land`] codex-alab holds
    /// the proven claim and acts (a person's hold pauses it, the claim kept); with
    /// [`Mode::Off`] it acts on nothing, and the harness is on all the same, a person's hold
    /// over that Off included (the owner's ruling of 2026-10-24: "on" is the entire harness,
    /// not Autopilot alone).
    On(Mode),
    /// A supervisor runs, but the harness does not supervise this session now: aterm's policy
    /// is off for a reason other than Autopilot (the harness disabled or off for this agent,
    /// unhosted, headless, a suspension, a hold over Cruise or Land codex-alab keeps no claim
    /// through, or an unreadable policy), or aterm did not prove codex-alab's claim, or
    /// another holds the session. Nothing says "on".
    Unsupervised,
}

/// The running session's harness ([`Harness`]); `None` with no supervisor at all (outside
/// aterm, an aterm with no harness policy, or under unit tests).
pub fn harness_state() -> Option<Harness> {
    use codex_alab_core::session::Autopilot;
    use codex_alab_core::session::HarnessWord;
    let shared = crate::boxes::tick_shared()?;
    // One load: the supervisor publishes the Autopilot and its word in one store, so a frame
    // never pairs a new state with the word it replaced.
    Some(match shared.harness() {
        (Autopilot::Off, _) => return None,
        (_, HarnessWord::Off) => Harness::On(Mode::Off),
        (Autopilot::Active, HarnessWord::Land) => Harness::On(Mode::Land),
        (Autopilot::Active, _) => Harness::On(Mode::Cruise),
        (Autopilot::Passive, _) => Harness::Unsupervised,
    })
}

/// A value that moves whenever what the banner draws for the harness does: the banner cell's
/// `transcript_animation_tick`, so a change redraws it at once. `None` with no supervisor.
pub fn harness_tick() -> Option<u64> {
    harness_state().map(|harness| match harness {
        Harness::Unsupervised => 1,
        Harness::On(Mode::Cruise) => 2,
        Harness::On(Mode::Land) => 3,
        Harness::On(Mode::Off) => 4,
    })
}

/// Tint the complete composer overline, including its labels, with a readable spectrum.
pub(crate) fn tint_composer_rule(
    area: ratatui::layout::Rect,
    buf: &mut ratatui::buffer::Buffer,
    depth: Depth,
    theme: Theme,
) {
    render::tint_composer_rule(area, buf, depth, theme);
}

/// `rgb` as text on the composer overline at `depth` on `theme`'s ground: moved away from the
/// ground until it clears [`palette::TEXT_CONTRAST_FLOOR`], as the spectrum through the
/// overline's labels is (toward white, else black, where the theme's own ink cannot clear it
/// on a middle gray); `ansi16` where the terminal draws named colours, and in mono.
pub(crate) fn readable_text(rgb: palette::Rgb, ansi16: Color, depth: Depth, theme: Theme) -> Color {
    render::readable_text(rgb, ansi16, depth, theme)
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
