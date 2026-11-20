// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A compact Codex launch strip inspired by aterm's Claude harness presentation.
//!
//! The default card places OpenAI's Codex icon beside the ALab release, the upstream
//! baseline and session facts. The rainbow kitty occupies the remaining space on wide
//! terminals. The explicit ribbon and stripes styles retain their own renderers.

pub mod artwork;
pub mod claims;
mod codex;
pub use codex::detail_style;
pub use codex::permission_style;
mod codex_icon;
pub mod palette;
mod render;
pub mod retained;
pub(crate) mod sprite;

use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style as TextStyle;
use ratatui::text::Line;
use std::fmt::Write as _;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Whether the banner may draw: only in the codex-alab binary (for the glue; codex-tui does not
/// depend on alab-core).
pub use codex_alab_core::active;

/// When stdout reports no color, preserve an explicit monochrome choice in the
/// enhanced interface. A capable terminal can still advertise truecolor.
pub fn explicit_monochrome() -> bool {
    active()
        && (std::env::var("NO_COLOR").is_ok_and(|value| value != "0")
            || std::env::var("TERM").as_deref() == Ok("dumb"))
}

/// The stamped upstream source or the published snapshot's ledger baseline.
/// An unstamped source build has neither.
pub fn stamped_upstream() -> Option<String> {
    use codex_alab_core::version;
    match version::display_baseline() {
        Some(baseline) if version::upstream_release().is_none() => Some(baseline.to_owned()),
        Some(_) => Some(version::upstream()),
        None => version::upstream_commit().map(|_| version::upstream()),
    }
}

/// The opening banner design.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Style {
    /// aterm's sweep: one spectrum position per column, red at the kitty, violet at the tail.
    Ribbon,
    /// ALab welcome card with the OpenAI Codex mark and rainbow kitty.
    #[default]
    Codex,
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
    terminal_foreground: bool,
}

impl Theme {
    /// The theme of a terminal that reported `bg` (or nothing).
    pub fn from_bg(bg: Option<(u8, u8, u8)>) -> Self {
        Self {
            bg: bg.map(palette::pack),
            ..Self::default()
        }
    }

    /// Use the terminal's configured text ink until it reports a background.
    pub fn with_terminal_foreground_if_unreported(mut self) -> Self {
        self.terminal_foreground = self.bg.is_none();
        self
    }

    /// Keep foreground text legible on an unknown background, even on a tinted, faint row.
    pub(crate) fn unreported_foreground(self, depth: Depth) -> Option<TextStyle> {
        self.terminal_foreground.then(|| TextStyle {
            fg: (depth != Depth::Mono).then_some(Color::Reset),
            ..TextStyle::default().not_dim()
        })
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
    claims: Vec<retained::HistoricalClaim>,
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
            claims: Vec::new(),
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
        self.claims = claims
            .iter()
            .map(retained::HistoricalClaim::borrowed)
            .collect();
        self
    }

    /// Whether the launch cell should integrate its metadata into this card.
    pub fn is_codex(&self) -> bool {
        self.style == Style::Codex
    }

    /// Resolve an unknown background once a deferred terminal probe supplies its color.
    pub fn with_late_background(
        &self,
        report: impl FnOnce() -> Option<(u8, u8, u8)>,
    ) -> Option<Self> {
        if self.theme.bg.is_some() {
            return None;
        }
        Some(Self {
            theme: Theme::from_bg(Some(report()?)),
            ..self.clone()
        })
    }

    /// The live status used by this artwork. Only the Codex card displays deadlines.
    pub fn live_harness(&self) -> Option<Harness> {
        read_harness(self.is_codex())
    }

    /// Version the live transcript row by both its model and local harness presentation.
    pub fn transcript_tick(&self, model_revision: u64) -> Option<u64> {
        let harness = self.live_harness().map(Harness::tick);
        if model_revision == 0 {
            return harness;
        }
        // Eight day forms × 1,440 minutes plus the four simple harness words fit in 14 bits.
        Some((model_revision << 14) | harness.unwrap_or(0))
    }

    /// One welcome card, including the session metadata supplied by the TUI.
    pub fn session_lines(
        &self,
        width: u16,
        harness: Option<Harness>,
        details: Vec<Line<'static>>,
    ) -> Vec<Line<'static>> {
        let mode = harness.and_then(Harness::mode);
        let deadline = match harness {
            Some(Harness::CruiseUntil(landing)) => Some(landing.to_string()),
            _ => None,
        };
        codex::render_session_with_deadline(self, width, mode, details, deadline.as_deref())
    }

    /// The banner's lines at `width` columns; each is at most `width - 1` columns wide. Below 13
    /// columns there are none.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.style == Style::Codex {
            return codex::render(self, width, None);
        }
        render::render(self, usize::from(width))
    }

    /// The banner's lines at `width` columns as `harness` shows it ([`harness_state`]): with the
    /// harness on, in its Autopilot word's palette (the moonbow under Off) and with the harness
    /// row under the text; otherwise the plain banner, as [`Banner::lines`].
    pub fn lines_with_harness(&self, width: u16, harness: Option<Harness>) -> Vec<Line<'static>> {
        if self.is_codex() {
            return self.session_lines(width, harness, Vec::new());
        }
        render::render_with_harness(self, usize::from(width), harness.and_then(Harness::mode))
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
    /// Supervised Cruise with a reported local landing time.
    CruiseUntil(crate::landing::Landing),
    /// A supervisor runs, but the harness does not supervise this session now: aterm's policy
    /// is off for a reason other than Autopilot (the harness disabled or off for this agent,
    /// unhosted, headless, a suspension, a hold over Cruise or Land codex-alab keeps no claim
    /// through, or an unreadable policy), or aterm did not prove codex-alab's claim, or
    /// another holds the session. Nothing says "on".
    Unsupervised,
}

impl Harness {
    fn mode(self) -> Option<Mode> {
        match self {
            Self::On(mode) => Some(mode),
            Self::CruiseUntil(_) => Some(Mode::Cruise),
            Self::Unsupervised => None,
        }
    }

    /// Fit the reported mode on one text row when the illustrated launch card collapses.
    pub fn compact_mode_label(self, width: u16) -> Option<&'static str> {
        let label = self.mode()?.label();
        let (glyph, word) = label.split_once(' ')?;
        [label, word, glyph]
            .into_iter()
            .find(|text| crate::model_layout::painted_width(text) <= usize::from(width))
    }

    fn tick(self) -> u64 {
        match self {
            Self::Unsupervised => 1,
            Self::On(Mode::Cruise) => 2,
            Self::On(Mode::Land) => 3,
            Self::On(Mode::Off) => 4,
            Self::CruiseUntil(landing) => 5 + landing.minute_key(),
        }
    }
}

/// The running session's harness ([`Harness`]); `None` with no supervisor at all (outside
/// aterm, an aterm with no harness policy, or under unit tests).
pub fn harness_state() -> Option<Harness> {
    read_harness(true)
}

fn supervised_landing(
    shared: &codex_alab_core::session::Shared,
    state: (
        codex_alab_core::session::Autopilot,
        codex_alab_core::session::HarnessWord,
    ),
) -> Option<u64> {
    use codex_alab_core::session::Autopilot;
    use codex_alab_core::session::HarnessWord;
    if state == (Autopilot::Active, HarnessWord::Cruise) {
        shared.land_at()
    } else {
        None
    }
}

fn read_harness(include_deadline: bool) -> Option<Harness> {
    use codex_alab_core::session::Autopilot;
    use codex_alab_core::session::HarnessWord;
    let shared = crate::boxes::tick_shared()?;
    // The supervisor publishes the policy and its word together.
    let state = shared.harness();
    if include_deadline
        && let Some(at) = supervised_landing(shared, state)
        && let Some(landing) =
            crate::landing::Landing::in_zone(at, &chrono::Local, crate::fixture_clock::utc_now())
    {
        return Some(Harness::CruiseUntil(landing));
    }
    Some(match state {
        (Autopilot::Off, _) => return None,
        (_, HarnessWord::Off) => Harness::On(Mode::Off),
        (Autopilot::Active, HarnessWord::Land) => Harness::On(Mode::Land),
        (Autopilot::Active, _) => Harness::On(Mode::Cruise),
        (Autopilot::Passive, _) => Harness::Unsupervised,
    })
}

/// Ask for a new frame when the reported landing day can change on an otherwise idle screen.
pub fn next_landing_day_change(
    now: &chrono::DateTime<chrono::Local>,
) -> Option<chrono::DateTime<chrono::Local>> {
    let shared = crate::boxes::tick_shared()?;
    let at = supervised_landing(shared, shared.harness())?;
    crate::landing::Landing::next_date_change(at, now)
}

/// A value that moves whenever what the banner draws for the harness does: the banner cell's
/// `transcript_animation_tick`, so a change redraws it at once. `None` with no supervisor.
pub fn harness_tick() -> Option<u64> {
    harness_state().map(Harness::tick)
}

/// Lay down the spectrum beneath the composer labels and controls.
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
///    default style, `codex|ribbon|stripes` → that style; an explicit value beats rule 3, and anything
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
        Some("codex") => Some(Style::Codex),
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
