// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The ALab experience switch and enhanced interface. Vanilla disables automatic
//! behavior and enhanced chrome, retaining a manual recovery switch. Native model,
//! approvals, conversation and draft remain intact.
//!
//! The caller resolves `[tui] alab_interface` from config layers and calls [`configure`].
//! A live toggle wins over subsequent frame refreshes. Persist a person's explicit choice
//! in the caller, then use [`set_enabled`] to apply that choice immediately.

use crate::banner::Depth;
use crate::banner::Theme;
use crate::landing::Landing;
use crate::model_layout::painted_width;
use crate::session::Autopilot;
use crate::session::HarnessWord;
use codex_alab_core::session::Shared;
use codex_alab_core::session::TaskFace;
use ratatui::buffer::Buffer;
use ratatui::buffer::CellWidth;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;
use std::cell::Cell;
use std::sync::Mutex;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use unicode_width::UnicodeWidthStr;

/// Config key for this optional presentation layer, exposed to TUI glue.
pub const SETTING: &str = codex_alab_core::INTERFACE_KEY;

// Unconfigured stays invisible to upstream's tests and non-interactive consumers.
const UNCONFIGURED: u8 = 0;
const OFF: u8 = 1;
const ON: u8 = 2;

/// The native app-server transport's presentation state, independent of task activity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum ConnectionState {
    #[default]
    Connected,
    Reconnecting,
    Offline,
}

impl ConnectionState {
    fn label(self) -> Option<&'static str> {
        match self {
            Self::Connected => None,
            Self::Reconnecting => Some("Reconnecting"),
            Self::Offline => Some("Offline"),
        }
    }
}

/// Mouse targets published together for the most recently painted composer row.
#[derive(Default)]
struct OverlineHits {
    button: Option<Rect>,
    build: Option<Rect>,
    modes: Vec<(Rect, Hit)>,
    reported: Option<HarnessWord>,
    hovered: Option<(Rect, HarnessWord)>,
}

impl OverlineHits {
    fn named_segment_at(&self, pos: Position) -> Option<(Rect, HarnessWord)> {
        // Only the three named segments share the full track's secondary-click target.
        // The separate cycling face can select the same mode.
        if !self
            .modes
            .iter()
            .any(|(rect, action)| *action == Hit::MenuOnly && rect.contains(pos))
        {
            return None;
        }
        self.modes.iter().find_map(|&(rect, action)| match action {
            Hit::Mode(mode) if rect.contains(pos) => Some((rect, mode)),
            _ => None,
        })
    }
}

/// Presentation state, separate from the harness's automation state.
pub struct State {
    choice: AtomicU8,
    connection: AtomicU8,
    overline_hits: Mutex<OverlineHits>,
    account_hits: Mutex<Option<PaintedAccount>>,
    status: Mutex<Option<InterfaceStatus>>,
    /// ONE WRITER for this interface's Autopilot ([`write_autopilot`]): writes go one at a
    /// time, and one a newer click or command overtook is dropped, so the last wins. Each
    /// state's own, so a test's scoped interface never drops another's write (review of gap
    /// 12, finding 14: two glue tests shared one process-wide writer).
    mode_write: tokio::sync::Mutex<()>,
    /// The newest write's generation ([`Self::mode_write`]).
    mode_latest: AtomicU64,
    /// The aterm a test sends this interface's requests to ([`route_aterm_for_tests`]).
    #[cfg(any(test, debug_assertions))]
    route: Mutex<Option<codex_alab_aterm::Endpoint>>,
    goal: Mutex<Option<String>>,
}

impl State {
    /// An unconfigured interface with no cached account information.
    pub const fn new() -> Self {
        Self {
            choice: AtomicU8::new(UNCONFIGURED),
            connection: AtomicU8::new(ConnectionState::Connected as u8),
            overline_hits: Mutex::new(OverlineHits {
                button: None,
                build: None,
                modes: Vec::new(),
                reported: None,
                hovered: None,
            }),
            account_hits: Mutex::new(None),
            status: Mutex::new(None),
            mode_write: tokio::sync::Mutex::const_new(()),
            mode_latest: AtomicU64::new(0),
            #[cfg(any(test, debug_assertions))]
            route: Mutex::new(None),
            goal: Mutex::new(None),
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

static SHARED: State = State::new();
thread_local! {
    static SCOPED: Cell<Option<&'static State>> = const { Cell::new(None) };
}

fn state() -> Option<&'static State> {
    SCOPED
        .with(Cell::get)
        .or_else(|| codex_alab_core::is_alab_binary().then_some(&SHARED))
}

/// Whether the enhanced interface hooks apply to this process or scoped test.
pub fn active() -> bool {
    state().is_some_and(|state| state.choice.load(Ordering::Acquire) != OFF)
        && (SCOPED.with(Cell::get).is_some() || codex_alab_core::active())
}

/// Whether the explicit mode command is available, including in vanilla mode.
pub fn available() -> bool {
    state().is_some()
}

/// Publish the native connection state, including while the optional interface is off.
/// The raw task status remains untouched so reconnecting restores its latest activity.
pub fn set_connection_state(connection: ConnectionState) {
    if let Some(state) = state() {
        state.connection.store(connection as u8, Ordering::Release);
    }
}

/// The transport state last supplied by the native app lifecycle.
pub fn connection_state() -> ConnectionState {
    match state().map(|state| state.connection.load(Ordering::Acquire)) {
        Some(value) if value == ConnectionState::Reconnecting as u8 => {
            ConnectionState::Reconnecting
        }
        Some(value) if value == ConnectionState::Offline as u8 => ConnectionState::Offline,
        _ => ConnectionState::Connected,
    }
}

/// Restores the previous thread-local interface state when dropped.
pub struct Scope(Option<&'static State>);

impl Drop for Scope {
    fn drop(&mut self) {
        SCOPED.with(|cell| cell.set(self.0));
    }
}

/// Let glue tests use their own interface state without activating ALab process-wide.
pub fn scoped(state: &'static State) -> Scope {
    Scope(SCOPED.with(|cell| cell.replace(Some(state))))
}

/// Apply the config-layer default once, without undoing a live toggle on later frames.
pub fn configure(enabled: bool) {
    let Some(state) = state() else { return };
    if state
        .choice
        .compare_exchange(
            UNCONFIGURED,
            if enabled { ON } else { OFF },
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
    {
        if SCOPED.with(Cell::get).is_none() {
            codex_alab_core::configure_enhanced(enabled);
        }
        clear_overline_hits(state);
        clear_account_hits();
    }
}

/// Whether the enhanced interface is enabled. Autopilot mode is a separate choice.
pub fn enabled() -> bool {
    state().is_some_and(|state| state.choice.load(Ordering::Acquire) == ON)
}

/// The native slash popup describes what Enter on a bare `/alab` will do.
pub fn slash_description() -> &'static str {
    if enabled() {
        "turn ALab off · /alab version · /alab menu for Autopilot"
    } else {
        "turn ALab on · /alab version"
    }
}

/// Whether the person explicitly selected vanilla, including during redraws of old cells.
pub fn is_vanilla() -> bool {
    state().is_some_and(|state| state.choice.load(Ordering::Acquire) == OFF)
}

/// The displayed thread's active objective, refreshed before each chat frame.
/// `None` clears paused, finished, disabled, or switched-away goals. This is
/// presentation only: the native goal owner retains every lifecycle decision.
pub fn set_goal(objective: Option<&str>) {
    if let Some(state) = state()
        && let Ok(mut goal) = state.goal.lock()
    {
        *goal = objective
            .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
            .and_then(|text| display_text(Some(&text)));
    }
}

fn current_goal() -> Option<String> {
    state()?.goal.lock().ok()?.clone()
}

/// Reserve a separate objective row while a goal is active, preserving the
/// account/quota row and the native composer geometry when presentation is off.
pub fn status_height() -> u16 {
    if enabled() {
        1 + u16::from(current_goal().is_some())
    } else {
        0
    }
}

/// Apply the experience choice immediately. Persistence belongs to the caller.
/// Returns the effective choice; a failed durable source revocation keeps the
/// previous mode rather than acknowledging an unsafe boundary.
pub fn set_enabled(enabled: bool) -> bool {
    let enabled = if SCOPED.with(Cell::get).is_none() {
        codex_alab_core::set_enhanced(enabled)
    } else {
        enabled
    };
    if let Some(state) = state() {
        state
            .choice
            .store(if enabled { ON } else { OFF }, Ordering::Release);
        clear_overline_hits(state);
        clear_account_hits();
    }
    enabled
}

/// Toggle the experience without changing the saved Cruise/Land/Off preference.
pub fn toggle() -> bool {
    let Some(state) = state() else { return false };
    let mut previous = state.choice.load(Ordering::Acquire);
    loop {
        let next = if previous == ON { OFF } else { ON };
        match state.choice.compare_exchange_weak(
            previous,
            next,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                let requested = next == ON;
                let effective = if SCOPED.with(Cell::get).is_none() {
                    codex_alab_core::set_enhanced(requested)
                } else {
                    requested
                };
                if effective != requested {
                    let _ = state.choice.compare_exchange(
                        next,
                        if effective { ON } else { OFF },
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                }
                clear_overline_hits(state);
                clear_account_hits();
                return effective;
            }
            Err(current) => previous = current,
        }
    }
}

#[derive(Clone, Copy)]
struct Palette {
    brand: Style,
    accent: Style,
    muted: Style,
    button: Style,
}

impl Palette {
    fn new(depth: Depth, theme: Theme) -> Self {
        if depth == Depth::Mono {
            return Self {
                brand: Style::default().add_modifier(Modifier::BOLD),
                accent: Style::default().add_modifier(Modifier::BOLD),
                muted: Style::default().add_modifier(Modifier::DIM),
                button: Style::default().add_modifier(Modifier::BOLD),
            };
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "each colour is picked for the depth the terminal reports, as the banner's \
                      rgb_color and indexed_color are"
        )]
        let (brand, accent, muted) = match (depth, theme.is_light()) {
            (Depth::TrueColor, true) => (
                Color::Rgb(30, 54, 71),
                Color::Rgb(0, 105, 112),
                Color::Rgb(101, 117, 126),
            ),
            (Depth::TrueColor, false) => (
                Color::Rgb(226, 235, 241),
                Color::Rgb(104, 220, 214),
                Color::Rgb(127, 151, 163),
            ),
            (Depth::Ansi256, true) => (Color::Indexed(24), Color::Indexed(23), Color::Indexed(243)),
            (Depth::Ansi256, false) => {
                (Color::Indexed(255), Color::Indexed(80), Color::Indexed(245))
            }
            (_, true) => (Color::Reset, Color::Reset, Color::DarkGray),
            (_, false) => (Color::White, Color::LightCyan, Color::Gray),
        };
        Self {
            brand: Style::default().fg(brand).add_modifier(Modifier::BOLD),
            accent: Style::default().fg(accent).add_modifier(Modifier::BOLD),
            muted: Style::default().fg(muted),
            button: Style::default().fg(accent).add_modifier(Modifier::BOLD),
        }
    }
}

#[derive(Clone, Copy)]
struct StatusPalette {
    account: Style,
    accent: Style,
    detail: Style,
    divider: Style,
}

impl StatusPalette {
    fn of(depth: Depth, theme: Theme) -> Self {
        // Quantizing to a readable fixed ANSI color searches the palette. Reuse the
        // result across draws, including the goal and account rows in the same frame.
        thread_local! {
            static CACHE: Cell<Option<(Depth, Theme, StatusPalette)>> = const { Cell::new(None) };
        }
        if let Some((d, t, palette)) = CACHE.with(Cell::get)
            && (d, t) == (depth, theme)
        {
            return palette;
        }
        let base = Palette::new(depth, theme);
        let text = |style: Style| {
            if !theme.is_light() {
                return style;
            }
            let rgb = match style.fg {
                Some(Color::Rgb(r, g, b)) => crate::banner::palette::pack((r, g, b)),
                Some(Color::Indexed(i)) => crate::banner::palette::xterm256(i),
                // ANSI slots can be pale, and bold can promote black to bright black.
                _ => return style.fg(Color::Reset).not_dim(),
            };
            style.fg(crate::banner::readable_text(
                rgb,
                Color::Reset,
                depth,
                theme,
            ))
        };
        let palette = Self {
            account: text(base.brand),
            accent: text(base.accent),
            detail: text(base.muted),
            divider: base.muted,
        };
        CACHE.with(|cache| cache.set(Some((depth, theme, palette))));
        palette
    }
}

fn button_label(compact: bool, on: bool) -> &'static str {
    match (compact, on) {
        (true, true) => "[on]",
        (true, false) => "[off]",
        (false, true) => "[ALab on]",
        (false, false) => "[ALab off]",
    }
}

fn button_for(area: Rect, on: bool) -> Option<Rect> {
    let compact = area.width < 20;
    let width = UnicodeWidthStr::width(button_label(compact, on)) as u16;
    // At eight columns [off] borrows a left margin cell so a click on [on] is reversible.
    let left_margin = if compact && !on { 1 } else { 2 };
    (area.height >= 3 && area.width >= left_margin + width + 2)
        .then(|| Rect::new(area.right() - width - 2, area.y, width, 1))
}

const PADDED_MODES: [&str; 3] = [" Cruise ", " Land ", " Off "];
const COMPACT_MODES: [&str; 3] = ["Cruise", "Land", "Off"];
const UPDATE_HINT: &str = " ↑ update";

struct ComposerControls {
    on: bool,
    button: Rect,
    harness: Option<(&'static Shared, (Autopilot, HarnessWord))>,
    track: Option<(u16, [&'static str; 3])>,
    label: Option<String>,
    label_x: u16,
    face_slot: u16,
}

impl ComposerControls {
    fn for_area(area: Rect) -> Option<Self> {
        let on = match state()?.choice.load(Ordering::Acquire) {
            ON => true,
            OFF => false,
            _ => return None,
        };
        let button = button_for(area, on)?;
        let harness = on
            .then(crate::boxes::tick_shared)
            .flatten()
            .map(|shared| (shared, shared.harness()))
            .filter(|(_, (_, word))| *word != HarnessWord::Unknown);
        Some(Self::fit(
            area,
            on,
            button,
            harness,
            on && crate::upgrade::pending(),
        ))
    }

    fn fit(
        area: Rect,
        on: bool,
        button: Rect,
        harness: Option<(&'static Shared, (Autopilot, HarnessWord))>,
        pending: bool,
    ) -> Self {
        // Two cells at each edge, the title inset, and two cells around the release.
        const BORDER_SPACE: usize = 7;
        let mut layout = Self {
            on,
            button,
            harness,
            track: None,
            label: None,
            label_x: area.x + 3,
            face_slot: 0,
        };
        if !on {
            return layout;
        }
        let (alab, baseline) = running_releases();
        let pair = release_pair(alab, baseline);
        let pair_width = pair.width();
        let short_width = display_release(alab).width();
        let selectable = harness.is_some_and(|(_, (_, word))| selectable_mode(word));
        // Keep the release pair first. When only the edition fits, keep mouse access to the
        // reported mode before expanding the presentation button from [on] to [ALab on].
        let width = usize::from(area.width);
        let full_face_space = usize::from(FACE_SLOT + 1);
        let edition_space = width.saturating_sub(pair_width + BORDER_SPACE + full_face_space);
        let edition_widths = button_label(true, on).len()..button_label(false, on).len();
        // When only the short edition button can keep the full status, reserve its
        // space regardless of the current word, task, timer or pending update.
        let lone_face_space = if harness.is_none() {
            0
        } else if edition_widths.contains(&edition_space) {
            full_face_space
        } else {
            usize::from(GLYPH_SLOT + 1)
        };
        // Fit the whole status and any pending update beside the named switch first.
        // Reserve the widest status across modes so a new timer or task cannot move a target.
        let status_and_hint = full_face_space + if pending { UPDATE_HINT.width() } else { 0 };
        let mut compact_hint = None;
        for (label_width, segments, compact_button, minimum_spare) in [
            (pair_width, Some(PADDED_MODES), false, status_and_hint),
            (pair_width, Some(PADDED_MODES), true, status_and_hint),
            (pair_width, Some(COMPACT_MODES), true, status_and_hint),
            (pair_width, Some(PADDED_MODES), false, 0),
            (pair_width, Some(COMPACT_MODES), false, 0),
            (pair_width, Some(COMPACT_MODES), true, 0),
            (pair_width, None, false, 0),
            (pair_width, None, true, 0),
            (short_width, None, false, 0),
            (short_width, None, true, 0),
        ] {
            // The space between the mode controls and the presentation button belongs here.
            let control_width = match segments {
                Some(_) if !selectable => continue,
                Some(labels) => 5 + labels.iter().map(|label| label.len()).sum::<usize>(),
                None => lone_face_space,
            };
            let button_width = button_label(compact_button || area.width < 20, on).len();
            let required = label_width + control_width + button_width + BORDER_SPACE;
            // With the shortest full track, use the title inset first and then the
            // rule between its right cap and [on]. Keep both two-cell outer margins.
            let shortfall = required.saturating_sub(width);
            let removable = if segments.is_some() && compact_button {
                2
            } else {
                0
            };
            let spare = width.saturating_sub(required);
            if shortfall > removable || spare < minimum_spare {
                continue;
            }
            if let Some((had_track, required)) = compact_hint {
                if label_width != pair_width || had_track != segments.is_some() {
                    break;
                }
                if spare < required {
                    continue;
                }
            }
            layout.button.width = button_width as u16;
            layout.button.x = area.right() - 2 - layout.button.width;
            layout.label_x = area.x + 3 - u16::from(shortfall > 0);
            layout.track = segments.map(|labels| {
                let x = layout.button.x - control_width as u16 + u16::from(shortfall == 2);
                (x, labels)
            });
            if compact_hint.is_some()
                || !pending
                || label_width != pair_width
                || spare >= UPDATE_HINT.width()
            {
                break;
            }
            // A shorter spelling can fund the hint, but keep the same controls and
            // their current face. Beside a lone button, leave room for a full face
            // (including a scheduled landing) before spending its cells on an update.
            let keep_face = if segments.is_none() && harness.is_some() {
                full_face_space - lone_face_space
            } else if segments.is_some() && spare > usize::from(GLYPH_SLOT) {
                usize::from(GLYPH_SLOT + 1)
            } else {
                0
            };
            compact_hint = Some((segments.is_some(), UPDATE_HINT.width() + keep_face));
        }
        let start = layout.track.map_or(layout.button.x, |(x, _)| x);
        let available = start.saturating_sub(layout.label_x);
        let reserve = if layout.track.is_none() {
            lone_face_space
        } else {
            0
        };
        layout.label = fit_brand_label(usize::from(available), alab, pair, pending, reserve);
        if let Some((_, policy)) = harness {
            let brand_width = layout.label.as_ref().map_or(0, |s| s.width() as u16 + 2);
            let room = available.saturating_sub(brand_width);
            // A weekday may use spare rule cells only after the release, update and switch fit.
            let supervised_cruise = policy == (Autopilot::Active, HarnessWord::Cruise);
            let compact_slot = if layout.track.is_none() && selectable {
                TIGHT_PICKER_SLOT
            } else {
                BARE_GLYPH_SLOT
            };
            layout.face_slot = [WEEKDAY_FACE_SLOT, FACE_SLOT, GLYPH_SLOT]
                .into_iter()
                .find(|&slot| room > slot && (slot != WEEKDAY_FACE_SLOT || supervised_cruise))
                .unwrap_or(available.min(compact_slot));
            // At an exact fit, borrow the label's extra inset to separate the release
            // from the face. The two outer margin cells remain available to the composer.
            if layout.face_slot == TIGHT_PICKER_SLOT && layout.label.is_some() && room == 0 {
                layout.label_x -= 1;
            }
        }
        layout
    }
}

/// The clickable on/off button on this composer's top border, if it fits.
pub fn button_rect(composer: Rect) -> Option<Rect> {
    ComposerControls::for_area(composer).map(|layout| layout.button)
}

/// Whether a terminal cell belongs to the button most recently painted by [`render`].
/// The caller owns mouse-button filtering and input ownership before calling [`toggle`].
pub fn mouse_hit(column: u16, row: u16) -> bool {
    state()
        .and_then(|state| state.overline_hits.lock().ok())
        .and_then(|hits| hits.button)
        .is_some_and(|area| area.contains((column, row).into()))
}

/// Draw controls on the existing composer overline. When enhancements are off,
/// only the recovery switch is drawn; native content and automation stay unchanged.
pub fn render(composer: Rect, buf: &mut Buffer, depth: Depth, theme: Theme) {
    let Some(state) = state() else { return };
    let area = composer.intersection(buf.area);
    let mut hits = ComposerControls::for_area(area)
        .map(|layout| render_border(area, buf, &layout, depth, theme))
        .unwrap_or_default();
    if let Ok(mut painted) = state.overline_hits.lock() {
        if let Some((rect, mode)) = painted.hovered
            && hits.reported == painted.reported
            && hits.named_segment_at((rect.x, rect.y).into()) == Some((rect, mode))
        {
            hits.hovered = painted.hovered;
            for pos in rect.positions() {
                if buf[pos].symbol() != " " {
                    buf[pos].modifier.insert(Modifier::UNDERLINED);
                }
            }
        }
        *painted = hits;
    }
}

/// Update hover from the last painted named switch. Returns whether its visual target changed.
pub fn hover_named_mode(pos: Option<Position>) -> bool {
    let Some(mut hits) = state().and_then(|state| state.overline_hits.lock().ok()) else {
        return false;
    };
    let next = pos.and_then(|pos| hits.named_segment_at(pos));
    let changed = hits.hovered != next;
    hits.hovered = next;
    changed
}

/// WHAT A PRESS ON ONE OF THE LINE'S AUTOPILOT CELLS DOES.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit {
    /// Select this mode: a mode button, or the face, whose click selects what a click on
    /// aterm's chip does.
    Mode(HarnessWord),
    /// Say the face in words ([`autopilot_words`]): `⊘ off`, which aterm's Settings turn and
    /// codex-alab cannot.
    Words,
    /// A secondary click opens the picker. A primary click here has no mode to select.
    MenuOnly,
    /// The painted arrow beside a collapsed switch opens the picker on either click.
    Disclosure,
}

/// The Autopilot cell of the latest enhanced frame at `(column, row)`.
fn hit_at(column: u16, row: u16) -> Option<Hit> {
    if !enabled() {
        return None;
    }
    let state = state()?;
    state
        .overline_hits
        .lock()
        .ok()?
        .modes
        .iter()
        .find_map(|(area, hit)| area.contains((column, row).into()).then_some(*hit))
}

/// A mode click is accepted only from the latest enhanced frame: a mode button, or the
/// Autopilot face beside them, whose click selects what a click on aterm's chip does.
pub fn mode_mouse_hit(column: u16, row: u16) -> Option<HarnessWord> {
    match hit_at(column, row)? {
        Hit::Mode(mode) => Some(mode),
        Hit::Words | Hit::MenuOnly | Hit::Disclosure => None,
    }
}

/// Whether a click at `(column, row)` falls on a face that selects no mode (`⊘ off`, the
/// harness off by one of aterm's Settings switches or with no host): the caller says
/// [`autopilot_words`], which name where it is turned.
pub fn autopilot_words_hit(column: u16, row: u16) -> bool {
    hit_at(column, row) == Some(Hit::Words)
}

/// The visible primary-click opener beside a collapsed three-way switch.
pub fn autopilot_disclosure_hit(column: u16, row: u16) -> bool {
    hit_at(column, row) == Some(Hit::Disclosure)
}

/// Whether this cell belongs to a native Autopilot control in the latest frame.
pub fn autopilot_menu_hit(column: u16, row: u16) -> bool {
    hit_at(column, row).is_some()
}

/// Eight cells for a full status such as `∞ ⤓18:00`, plus a space on each side.
const FACE_SLOT: u16 = 10;
/// A three-letter weekday and its following space, taken from the rule to the left.
const WEEKDAY_FACE_SLOT: u16 = FACE_SLOT + 4;
/// THE GLYPH-ONLY RUNG (aterm's `Keep::GlyphOnly`): one glyph and a space each side.
const GLYPH_SLOT: u16 = 3;
/// A selectable glyph and picker arrow directly before the edition button.
const TIGHT_PICKER_SLOT: u16 = 2;
/// One glyph directly before the next control, with the preceding rule cell left blank.
const BARE_GLYPH_SLOT: u16 = 1;

/// The macOS aterm renderer supplies a one-cell text clock, including font fallback.
fn compact_clock_supported() -> bool {
    cfg!(target_os = "macos") && crate::direct_aterm()
}

fn render_mode_buttons(
    layout: &ComposerControls,
    buf: &mut Buffer,
    palette: Palette,
    depth: Depth,
    theme: Theme,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let Some((shared, (_, selected))) = layout.harness else {
        return;
    };
    let y = layout.button.y;
    let start = layout.track.map_or(layout.button.x, |(x, _)| x);
    if layout.face_slot > 0
        && let Some(face) = Face::read(shared, selected)
    {
        // The status and picker arrow sit directly on the composer's background.
        let inks = match theme.unreported_foreground(depth) {
            Some(ink) => FaceInks {
                accent: ink.add_modifier(Modifier::BOLD),
                amber: ink.add_modifier(Modifier::BOLD),
                muted: ink,
            },
            None => FaceInks::of(depth, theme),
        };
        let (line, trailing) = face.composer_line(layout, start, inks);
        let face_width = line.width() as u16;
        let x = start - trailing - face_width;
        if trailing > 0 {
            buf[(start - trailing, y)].set_symbol(" ");
        }
        buf[(x - 1, y)].set_symbol(" ");
        let rect = Rect::new(x, y, face_width, 1);
        line.render(rect, buf);
        hits.push((rect, face.hit()));
        if layout.track.is_none() && trailing > 0 && selectable_mode(selected) {
            // The arrow uses the first trailing cell. Padded layouts keep a rule before the button.
            let arrow = Rect::new(rect.right(), y, 1, 1);
            Line::styled("▾", inks.accent).render(arrow, buf);
            hits.push((arrow, Hit::Disclosure));
        }
    }
    if let Some((mut x, segments)) = layout.track {
        #[expect(
            clippy::disallowed_methods,
            reason = "the switch track is chosen for each color depth and paired with the interface inks"
        )]
        let ground = match (depth, theme.is_light()) {
            (Depth::TrueColor, true) => Some(Color::Rgb(239, 244, 247)),
            (Depth::TrueColor, false) => Some(Color::Rgb(31, 41, 48)),
            (Depth::Ansi256, true) => Some(Color::Indexed(255)),
            (Depth::Ansi256, false) => Some(Color::Indexed(235)),
            _ => None,
        };
        let terminal_pair = Style::default().fg(Color::Reset).bg(Color::Reset);
        // ANSI16 has no independent track fill. Until Windows reports its default
        // background, draw the names and brackets with the terminal's own pair.
        let track_style = if depth == Depth::Ansi16 && theme.unreported_foreground(depth).is_some()
        {
            terminal_pair
        } else {
            ground.map_or(Style::default(), |bg| Style::default().bg(bg))
        };
        let edge = palette
            .muted
            .patch(track_style)
            .remove_modifier(Modifier::DIM);
        // Idle labels are actionable. Dimming normal text can put them below
        // the contrast floor on light monochrome terminals.
        let idle = palette.brand.remove_modifier(Modifier::BOLD);
        // The native composer may already have painted a fill. On light ANSI16
        // all three labels use the terminal's configured foreground/background pair.
        let label_track = if depth == Depth::Ansi16 && theme.is_light() {
            terminal_pair
        } else {
            track_style
        };
        let selected_ink = palette.button.add_modifier(Modifier::REVERSED);
        let modes = [HarnessWord::Cruise, HarnessWord::Land, HarnessWord::Off];
        // Bracket the reported mode in the existing border cells so copied text retains it.
        let boundaries = match selected {
            HarnessWord::Cruise => ["[", "]", "│", "▕"],
            HarnessWord::Land => ["▏", "[", "]", "▕"],
            HarnessWord::Off => ["▏", "│", "[", "]"],
            _ => ["▏", "│", "│", "▕"],
        };
        for ((label, mode), separator) in segments.into_iter().zip(modes).zip(boundaries) {
            Line::styled(separator, edge).render(Rect::new(x, y, 1, 1), buf);
            x += 1;
            let rect = Rect::new(x, y, label.len() as u16, 1);
            let ink = if selected == mode { selected_ink } else { idle };
            Line::styled(label, ink.patch(label_track)).render(rect, buf);
            hits.push((rect, Hit::Mode(mode)));
            x = rect.right();
        }
        Line::styled(boundaries[3], edge).render(Rect::new(x, y, 1, 1), buf);
        // Match the mode labels first. The whole painted track also opens their shared picker.
        hits.push((Rect::new(start, y, x - start + 1, 1), Hit::MenuOnly));
    }
}

/// aterm's `AMBER` (`claude_lights.rs`): the chip's Land and landed ink.
const AMBER: (u8, u8, u8) = (0xE8, 0xB0, 0x3C);

/// THE FACE'S INKS (aterm's `ChipTone`s): accent, amber and muted, each floored against the
/// ground as aterm floors its chip's hue, as the overline's spectrum is
/// ([`crate::banner::readable_text`]), at true colour and 256 colours; on a 16-colour terminal a
/// named colour on a dark ground or the terminal's text ink on a known light ground.
/// Mono uses bold and dim. The controls keep their own inks.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FaceInks {
    accent: Style,
    amber: Style,
    muted: Style,
}

thread_local! {
    /// The inks last made, for the depth and theme they were made at: a 256-colour terminal's
    /// pick searches the palette, which a frame need not repeat.
    static FACE_INKS: Cell<Option<(Depth, Theme, FaceInks)>> = const { Cell::new(None) };
}

impl FaceInks {
    /// The inks at `depth` on `theme`'s ground, made once per change of either.
    fn of(depth: Depth, theme: Theme) -> Self {
        if let Some((d, t, inks)) = FACE_INKS.with(Cell::get)
            && (d, t) == (depth, theme)
        {
            return inks;
        }
        let inks = Self::new(depth, theme);
        FACE_INKS.with(|cell| cell.set(Some((depth, theme, inks))));
        inks
    }

    fn new(depth: Depth, theme: Theme) -> Self {
        if depth == Depth::Mono {
            return Self {
                accent: Style::default().add_modifier(Modifier::BOLD),
                amber: Style::default().add_modifier(Modifier::BOLD),
                muted: Style::default().add_modifier(Modifier::DIM),
            };
        }
        let light = theme.is_light();
        // Named ANSI colors can wash out on light palettes, especially when
        // bold promotes them to bright colors. The glyph and word identify
        // each mode; weight distinguishes Cruise and Land from quiet/Off.
        let (accent, accent16, muted, muted16, amber16) = if light {
            (
                (0, 105, 112),
                Color::Reset,
                (101, 117, 126),
                Color::Reset,
                Color::Reset,
            )
        } else {
            (
                (104, 220, 214),
                Color::LightCyan,
                (127, 151, 163),
                Color::Gray,
                Color::LightYellow,
            )
        };
        let ink = |rgb, ansi16| {
            crate::banner::readable_text(crate::banner::palette::pack(rgb), ansi16, depth, theme)
        };
        Self {
            accent: Style::default()
                .fg(ink(accent, accent16))
                .add_modifier(Modifier::BOLD),
            amber: Style::default()
                .fg(ink(AMBER, amber16))
                .add_modifier(Modifier::BOLD),
            muted: Style::default().fg(ink(muted, muted16)),
        }
    }
}

/// A weekday spelled out, as aterm's `Weekday::long` spells it.
fn weekday_long(day: chrono::Weekday) -> &'static str {
    match day {
        chrono::Weekday::Mon => "Monday",
        chrono::Weekday::Tue => "Tuesday",
        chrono::Weekday::Wed => "Wednesday",
        chrono::Weekday::Thu => "Thursday",
        chrono::Weekday::Fri => "Friday",
        chrono::Weekday::Sat => "Saturday",
        chrono::Weekday::Sun => "Sunday",
    }
}

/// AN AUTOPILOT FACE (aterm's `ChipFace`, `claude_lights.rs`): the faces a codex tab can wear,
/// drawn on codex-alab's line from its own facts (gap 12 of the parity sweep, 2026-10-29).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Face {
    /// `∞ cruise`, or `∞` beside the named switch or in a narrow slot.
    Cruise,
    /// `∞ ⤓18:00`: Cruise until a time, then Land (aterm's timer).
    CruiseUntil(Landing),
    /// `⤓ land`: Land, its task open.
    Land,
    /// `✓ landed`: Land, the work all on the remote main (`landed@v1`).
    Landed,
    /// `⤓ quiet`: Land, a point left to a person's own exchange.
    Theirs,
    /// `○ off`.
    Off,
    /// `⊘ off`: the harness off in aterm's Settings (aterm's `MasterOff`).
    HarnessOff,
    /// `⊘ off`: aterm's switch for codex-alab sessions off (aterm's `AgentOff`).
    AgentOff,
    /// `⊘ off`: aterm runs no harness host (aterm's `Unhosted`).
    Unhosted,
}

impl Face {
    /// The face `word` wears with the task's face `task` and a timed Cruise's `lands`: the task
    /// is read under Land only and the timer under Cruise only, as aterm's `ChipFace::of` reads
    /// them; none without a word.
    fn of(word: HarnessWord, task: TaskFace, lands: Option<Landing>) -> Option<Face> {
        Some(match word {
            HarnessWord::Cruise => lands.map_or(Face::Cruise, Face::CruiseUntil),
            HarnessWord::Land => match task {
                TaskFace::Landed => Face::Landed,
                TaskFace::Theirs => Face::Theirs,
                TaskFace::Open => Face::Land,
            },
            HarnessWord::Off => Face::Off,
            HarnessWord::HarnessOff => Face::HarnessOff,
            HarnessWord::AgentOff => Face::AgentOff,
            HarnessWord::Unhosted => Face::Unhosted,
            HarnessWord::Unknown => return None,
        })
    }

    /// The face the session wears now under `word`, the frame's one load of it, from what
    /// else the supervisor published: the primary thread's task face under Land, and aterm's
    /// timer under Cruise in local time (the clock read only while one is armed).
    fn read(shared: &Shared, word: HarnessWord) -> Option<Face> {
        Self::read_in(
            shared,
            word,
            &chrono::Local,
            crate::fixture_clock::utc_now(),
        )
    }

    /// [`Self::read`], the timer's clock in `zone` as of `now` ([`Landing::in_zone`]).
    fn read_in<Tz: chrono::TimeZone>(
        shared: &Shared,
        word: HarnessWord,
        zone: &Tz,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<Face> {
        let task = if word == HarnessWord::Land {
            shared.task_face()
        } else {
            TaskFace::Open
        };
        let lands = (word == HarnessWord::Cruise)
            .then(|| shared.land_at())
            .flatten()
            .and_then(|at| Landing::in_zone(at, zone, now));
        Face::of(word, task, lands)
    }

    /// The one-cell status. A clock can stand for the timer when the switch already names Cruise.
    fn glyph(&self, compact_clock: bool) -> &'static str {
        match self {
            Face::CruiseUntil(_) if compact_clock => "◷",
            Face::Cruise | Face::CruiseUntil(_) => "∞",
            Face::Land | Face::Theirs => "⤓",
            Face::Landed => "✓",
            Face::Off => "○",
            Face::HarnessOff | Face::AgentOff | Face::Unhosted => "⊘",
        }
    }

    /// The whole face after its glyph: the word a space after it, and a timed Cruise's clock
    /// right after its `⤓`. The selected Cruise segment already names the mode.
    fn rest(&self, beside_switch: bool, slot: u16) -> String {
        match self {
            Face::Cruise if beside_switch => String::new(),
            Face::Cruise => " cruise".to_string(),
            Face::CruiseUntil(lands) if slot == WEEKDAY_FACE_SLOT => format!(" ⤓{lands}"),
            Face::CruiseUntil(lands) => format!(" ⤓{}", lands.clock()),
            Face::Land => " land".to_string(),
            Face::Landed => " landed".to_string(),
            Face::Theirs => " quiet".to_string(),
            Face::Off | Face::HarnessOff | Face::AgentOff | Face::Unhosted => " off".to_string(),
        }
    }

    /// The glyph's and the rest's inks (aterm's `ChipFace::tones`).
    fn tones(&self, inks: FaceInks) -> (Style, Style) {
        match self {
            Face::Cruise => (inks.accent, inks.accent),
            Face::CruiseUntil(_) => (inks.accent, inks.amber),
            Face::Land | Face::Landed => (inks.amber, inks.amber),
            Face::Theirs | Face::Off | Face::HarnessOff | Face::AgentOff | Face::Unhosted => {
                (inks.muted, inks.muted)
            }
        }
    }

    /// The face as drawn: whole, or its glyph alone (aterm's glyph-only rung).
    fn line(&self, slot: u16, beside_switch: bool, inks: FaceInks) -> Line<'static> {
        let whole = slot >= FACE_SLOT;
        let (glyph, rest) = self.tones(inks);
        let mut spans = vec![Span::styled(
            self.glyph(!whole && beside_switch && compact_clock_supported()),
            glyph,
        )];
        if whole {
            spans.push(Span::styled(self.rest(beside_switch, slot), rest));
        }
        Line::from(spans)
    }

    /// Fit the status and its right inset between the painted release and the next control.
    fn composer_line(
        &self,
        layout: &ComposerControls,
        start: u16,
        inks: FaceInks,
    ) -> (Line<'static>, u16) {
        let beside_switch = layout.track.is_some();
        if beside_switch
            && layout.face_slot == FACE_SLOT
            && matches!(layout.harness, Some((_, (Autopilot::Active, _))))
            && let Self::CruiseUntil(landing) = self
            && landing.day.is_some()
            && let Some(label) = &layout.label
        {
            // A wider switch can spend the rule cells that held the weekday at the
            // preceding width. Keep one blank on each side before adding internal spacing.
            let gap =
                usize::from(start.saturating_sub(layout.label_x)).saturating_sub(label.width());
            let glyph = self.glyph(false);
            let mut deadline = format!("⤓{landing}");
            if let Some(spare) = gap.checked_sub(glyph.width() + deadline.width() + 2) {
                if spare > 0 {
                    deadline.insert(0, ' ');
                }
                let (accent, time) = self.tones(inks);
                let line = Line::from(vec![
                    Span::styled(glyph, accent),
                    Span::styled(deadline, time),
                ]);
                return (line, if spare >= 2 { 2 } else { 1 });
            }
        }
        let trailing = match layout.face_slot {
            BARE_GLYPH_SLOT => 0,
            TIGHT_PICKER_SLOT => 1,
            _ => 2,
        };
        (self.line(layout.face_slot, beside_switch, inks), trailing)
    }

    /// What a click does (aterm's `ChipFace::click`): the mode it selects, a timed Cruise's
    /// Land now, which drops its timer, as aterm keeps a timer with the cruise word only. aterm
    /// turns its own Settings switch at `⊘ off`, or opens its Settings, which codex-alab cannot:
    /// a click says the face in words, naming where it is turned ([`Self::settings_words`]).
    fn hit(&self) -> Hit {
        Hit::Mode(match self {
            Face::Cruise | Face::CruiseUntil(_) => HarnessWord::Land,
            Face::Land | Face::Landed | Face::Theirs => HarnessWord::Off,
            Face::Off => HarnessWord::Cruise,
            Face::HarnessOff | Face::AgentOff | Face::Unhosted => return Hit::Words,
        })
    }

    /// Where a face that selects no mode is turned, as aterm's `ChipFace::click` says it for
    /// `⊘ off` ("turn the harness on", "open Settings ▸ Harness"), which only aterm's Settings
    /// does.
    fn settings_words(&self) -> &'static str {
        match self {
            Face::Unhosted => "Open aterm's Settings ▸ Harness.",
            _ => "Turn it on in aterm's Settings ▸ Harness.",
        }
    }

    /// A complete status sentence, including any task or landing details.
    fn description(&self) -> String {
        match self {
            Face::Cruise => "Cruise.".to_string(),
            Face::CruiseUntil(lands) => {
                let day = lands
                    .day
                    .map_or(String::new(), |day| format!("{} ", weekday_long(day)));
                format!("Cruise. Switches to Land {day}at {}.", lands.clock())
            }
            Face::Land => "Land.".to_string(),
            Face::Landed => "Land. Landed.".to_string(),
            Face::Theirs => "Land. Quiet: a person is driving the session.".to_string(),
            Face::Off => "Off.".to_string(),
            Face::HarnessOff => "Off. The harness is off in Settings.".to_string(),
            Face::AgentOff => "Disabled for codex-alab in aterm's Settings.".to_string(),
            Face::Unhosted => "Off. This aterm has no control socket.".to_string(),
        }
    }

    /// The face in words (aterm's `ChipFace::spoken`).
    fn spoken(&self) -> String {
        let mut description = self.description();
        if let Some(initial) = description.get_mut(..1) {
            initial.make_ascii_lowercase();
        }
        format!("Autopilot: {description}")
    }
}

/// The reported mode and the task or deadline shown beside it.
#[derive(Debug, PartialEq, Eq)]
pub struct AutopilotStatus {
    pub mode: HarnessWord,
    face: Option<Face>,
}

/// Identifies the native picker whose actions write the reported host policy.
pub const AUTOPILOT_PICKER_ID: &str = "alab-autopilot";

fn selectable_mode(mode: HarnessWord) -> bool {
    matches!(
        mode,
        HarnessWord::Cruise | HarnessWord::Land | HarnessWord::Off
    )
}

/// Recheck the supervisor report at activation, including between a picker redraw and Enter.
pub fn autopilot_selection_available() -> bool {
    enabled()
        && crate::boxes::tick_shared().is_some_and(|shared| selectable_mode(shared.harness_word()))
}

/// Other native pickers retain their normal selection rules.
pub fn picker_accepts_selection(view_id: Option<&str>) -> bool {
    view_id != Some(AUTOPILOT_PICKER_ID) || autopilot_selection_available()
}

impl AutopilotStatus {
    pub fn read() -> Self {
        let shared = crate::boxes::tick_shared();
        let mode = shared.map_or(HarnessWord::Unknown, Shared::harness_word);
        let face = shared.and_then(|shared| Face::read(shared, mode));
        Self { mode, face }
    }

    pub fn summary(&self) -> String {
        self.face.as_ref().map_or_else(
            || "Autopilot: no mode reported by aterm.".into(),
            Face::spoken,
        )
    }

    /// Explain the reported state and the available next action above the whole picker.
    pub fn picker_subtitle(&self) -> String {
        let Some(face) = &self.face else {
            return self.summary();
        };
        let reason = match face {
            Face::HarnessOff => "The harness is off.",
            Face::AgentOff => "codex-alab Autopilot is off.",
            Face::Unhosted => "This aterm has no control socket.",
            Face::CruiseUntil(_) => return face.description(),
            Face::Cruise | Face::Land | Face::Landed | Face::Theirs | Face::Off => {
                return format!("{} Timers switch to Land.", face.description());
            }
        };
        format!("{reason} {}", face.settings_words())
    }
}

/// The reported Autopilot state and where to change it, including in narrow terminals.
pub fn autopilot_words() -> String {
    let status = AutopilotStatus::read();
    let summary = status.summary();
    match status.face {
        Some(_) if selectable_mode(status.mode) => {
            format!("{summary}\nChoose a mode with /alab menu.")
        }
        Some(face) => format!("{summary}\n{}", face.settings_words()),
        None => summary,
    }
}

/// Route this scoped interface's requests (mode and timer) to `endpoint`, a test's fake
/// aterm, instead of the session hosting the terminal, so a test that presses a control never
/// reaches a live aterm. The process's own state is never routed.
#[cfg(any(test, debug_assertions))]
pub fn route_aterm_for_tests(endpoint: codex_alab_aterm::Endpoint) {
    if let Some(state) = SCOPED.with(Cell::get)
        && let Ok(mut route) = state.route.lock()
    {
        *route = Some(endpoint);
    }
}

/// Where a request made now goes: a test's route, read as the request is made.
#[cfg(any(test, debug_assertions))]
fn route() -> Option<codex_alab_aterm::Endpoint> {
    state()?.route.lock().ok()?.clone()
}

#[cfg(not(any(test, debug_assertions)))]
fn route() -> Option<codex_alab_aterm::Endpoint> {
    None
}

/// ONE SELF CONNECTION per request, every one of the interface's (mode and timer): the
/// session hosting this terminal, or a test's route.
async fn self_client(
    route: Option<codex_alab_aterm::Endpoint>,
) -> Result<codex_alab_aterm::Client, String> {
    match route {
        Some(endpoint) => {
            codex_alab_aterm::Client::connect(endpoint, codex_alab_aterm::Options::default())
                .await
                .map_err(|error| error.to_string())
        }
        None => codex_alab_aterm::Client::connect_self()
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "this session is not hosted by aterm".to_string()),
    }
}

/// Apply a person's mode selection through aterm's owner-only control path.
/// The supervisor's next policy observation supplies the selected highlight.
pub fn set_harness_mode(
    mode: HarnessWord,
) -> impl std::future::Future<Output = Result<(), String>> + Send {
    write_autopilot(match mode {
        HarnessWord::Cruise => Ok("cruise".to_string()),
        HarnessWord::Land => Ok("land".to_string()),
        HarnessWord::Off => Ok("off".to_string()),
        HarnessWord::Unknown
        | HarnessWord::HarnessOff
        | HarnessWord::AgentOff
        | HarnessWord::Unhosted => Err("unknown Autopilot mode".to_string()),
    })
}

/// Validate aterm's timed Cruise syntax before a request can supersede another selection.
/// aterm resolves local clock times, including daylight-saving transitions.
pub fn validate_cruise_until(when: &str) -> Result<(), &'static str> {
    const USAGE: &str = "Use HH:MM (00:00–23:59) or a duration from +1m to +24h, such as +1h30m.";
    let number = |part: &str| -> Result<u32, &'static str> {
        if part.len() > 4 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(USAGE);
        }
        part.parse().map_err(|_| USAGE)
    };
    let valid = if let Some(span) = when.strip_prefix('+') {
        let (hours, rest) = match span.split_once('h') {
            Some((hours, rest)) => (number(hours)?, rest),
            None => (0, span),
        };
        let minutes = if rest.is_empty() {
            0
        } else {
            number(rest.strip_suffix('m').ok_or(USAGE)?)?
        };
        (1..=24 * 60).contains(&(hours * 60 + minutes))
    } else {
        let (hours, minutes) = when.split_once(':').ok_or(USAGE)?;
        hours.len() == 2 && minutes.len() == 2 && number(hours)? < 24 && number(minutes)? < 60
    };
    valid.then_some(()).ok_or(USAGE)
}

/// `/alab cruise until=<when>` starts timed Cruise. A plain Cruise drops the timer.
pub fn set_cruise_until(
    when: String,
) -> impl std::future::Future<Output = Result<(), String>> + Send {
    let words = validate_cruise_until(&when)
        .map(|()| format!("cruise until={when}"))
        .map_err(str::to_owned);
    write_autopilot(words)
}

/// The session's Autopilot as `words` ([`set_harness_mode`], [`set_cruise_until`]), through the
/// interface's ONE WRITER ([`State::mode_write`]): writes go one at a time, and one a newer
/// click or command overtook is dropped, so the last wins.
#[expect(
    clippy::await_holding_invalid_type,
    reason = "serialize mode writes so the last click cannot be overtaken by an older request"
)]
fn write_autopilot(
    words: Result<String, String>,
) -> impl std::future::Future<Output = Result<(), String>> + Send {
    // Only a valid selection can supersede a mode or timer that's waiting to be sent.
    let request = words.and_then(|words| {
        let state = state().ok_or_else(|| "ALab enhancements are off".to_owned())?;
        let generation = state.mode_latest.fetch_add(1, Ordering::SeqCst) + 1;
        Ok((state, generation, route(), words))
    });
    async move {
        let (state, generation, route, words) = request?;
        let _write = state.mode_write.lock().await;
        let superseded = || state.mode_latest.load(Ordering::SeqCst) != generation;
        if superseded() {
            return Ok(());
        }
        let on = || state.choice.load(Ordering::Acquire) == ON;
        if !on() {
            return Err("ALab enhancements are off".into());
        }
        let connection = self_client(route).await;
        // Authentication can span several clicks. Only the newest selection may write.
        if superseded() {
            return Ok(());
        }
        let client = connection
            .map_err(|error| format!("Could not connect to aterm to set {words} ({error})"))?;
        if !on() {
            return Err("ALab enhancements are off".into());
        }
        let reply = client
            .request(&format!(
                "@{} meta set autopilot {words}",
                client.endpoint().sid
            ))
            .await;
        // Once sent, a write may already have taken effect even if its reply is lost.
        if superseded() {
            return Ok(());
        }
        let reply = reply.map_err(|error| format!("aterm did not confirm {words} ({error})"))?;
        if reply.ok().is_some() {
            Ok(())
        } else if reply.is_err() {
            Err(format!("aterm refused {words} ({})", reply.status))
        } else {
            Err(format!(
                "aterm did not confirm {words} (unrecognized status reply)"
            ))
        }
    }
}

fn render_border(
    area: Rect,
    buf: &mut Buffer,
    layout: &ComposerControls,
    depth: Depth,
    theme: Theme,
) -> OverlineHits {
    let button = layout.button;
    let mut hits = OverlineHits {
        button: Some(button),
        reported: layout.harness.map(|(_, (_, mode))| mode),
        ..OverlineHits::default()
    };
    let palette = Palette::new(depth, theme);
    // The terminal owns the 16-color palette. Its configured foreground keeps
    // the release, update hint and edition readable over the composer's rainbow.
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let label_style = match (layout.on, depth) {
        (false, _) => palette.button,
        (true, Depth::Ansi16) => bold.fg(Color::Reset),
        (true, _) => bold,
    }
    .patch(theme.unreported_foreground(depth).unwrap_or_default());
    let rule = Rect::new(area.x + 2, area.y, area.width - 4, 1);
    if layout.on {
        let has_modes = layout.face_slot > 0 || layout.track.is_some();
        for x in rule.x..rule.right() {
            let cell = &mut buf[(x, rule.y)];
            cell.set_symbol("─");
            // Mode controls own their modifiers. The rest of the composer inherits
            // the native surface's styling, including its background.
            if has_modes && x < button.x {
                cell.modifier = Modifier::empty();
            }
        }
        crate::banner::tint_composer_rule(rule, buf, depth, theme);
        if let Some(label) = &layout.label {
            let rect = Rect::new(layout.label_x, rule.y, label.width() as u16, 1);
            if has_modes || area.width >= 32 {
                buf[(rule.x, rule.y)].set_symbol(" ");
            }
            Line::styled(label.as_str(), label_style).render(rect, buf);
            buf[(rect.right(), rule.y)].set_symbol(" ");
            hits.build = Some(rect);
        }
    }
    let compact = usize::from(button.width) < button_label(false, layout.on).len();
    Line::styled(button_label(compact, layout.on), label_style).render(button, buf);
    if layout.on {
        render_mode_buttons(layout, buf, palette, depth, theme, &mut hits.modes);
    }
    hits
}

/// Strip the source metadata for the legacy banner styles.
pub(crate) fn readable_version(version: &str) -> &str {
    version
        .split_once('+')
        .map_or(version, |(version, _)| version)
}

/// The native version label opens build and update details without a host overlay.
pub fn build_mouse_hit(column: u16, row: u16) -> bool {
    enabled()
        && state()
            .and_then(|state| state.overline_hits.lock().ok())
            .and_then(|hits| hits.build)
            .is_some_and(|area| area.contains((column, row).into()))
}

fn fit_brand_label(
    available: usize,
    alab: &str,
    mut full: String,
    pending: bool,
    reserve: usize,
) -> Option<String> {
    let fits = |label: &str| UnicodeWidthStr::width(label) + 2 <= available;
    if pending && full.width() + UPDATE_HINT.width() + 2 + reserve <= available {
        full.push_str(UPDATE_HINT);
        return Some(full);
    }
    if fits(&full) {
        return Some(full);
    }
    let short = display_release(alab);
    fits(short).then(|| short.to_owned())
}

/// The stamped release number without build metadata or a trailing zero patch.
/// Keep a nonzero patch: distinct upstream baselines can share a minor version.
pub(crate) fn display_release(version: &str) -> &str {
    let Some(core) = codex_alab_core::version::cli_version_core(version) else {
        return readable_version(version);
    };
    if core.split('.').count() == 3 {
        core.strip_suffix(".0").unwrap_or(core)
    } else {
        core
    }
}

pub(crate) fn release_pair(alab: &str, upstream: &str) -> String {
    format!(
        "{} on Codex {}",
        display_release(alab),
        display_release(upstream)
    )
}

pub(crate) struct ReleaseRow {
    pub(crate) text: String,
    // Keep every piece of a split number at the same left margin in the plain title.
    split_number: bool,
}

/// Group words from the end so the Codex name stays beside its baseline whenever it fits.
pub(crate) fn release_rows(alab: &str, baseline: &str, width: usize) -> Vec<ReleaseRow> {
    let mut groups = Vec::new();
    let mut row = display_release(baseline).to_owned();
    for word in ["Codex", "on", display_release(alab)] {
        if painted_width(word) + 1 + painted_width(&row) <= width {
            row = format!("{word} {row}");
        } else {
            groups.push(row);
            row = word.to_owned();
        }
    }
    groups.push(row);
    let mut rows = Vec::with_capacity(groups.len());
    for text in groups.into_iter().rev() {
        if width > 0
            && text.len() > width
            && codex_alab_core::version::cli_version_core(&text) == Some(text.as_str())
        {
            // A numeric release contains only ASCII digits and dots, each one cell.
            for start in (0..text.len()).step_by(width) {
                rows.push(ReleaseRow {
                    text: text[start..(start + width).min(text.len())].to_owned(),
                    split_number: true,
                });
            }
        } else {
            rows.push(ReleaseRow {
                text,
                split_number: false,
            });
        }
    }
    rows
}

fn running_releases() -> (&'static str, &'static str) {
    (
        codex_alab_core::version::alab(),
        codex_alab_core::version::display_baseline().unwrap_or("unreleased"),
    )
}

/// Release shown by the configured Codex version item in the footer and terminal title.
pub fn status_surface_version(product: &str) -> String {
    if enabled() {
        let (alab, baseline) = running_releases();
        release_pair(alab, baseline)
    } else {
        product.to_owned()
    }
}

const SESSION_BRAND: &str = "Codex ALab  ";

/// Plain and accessible session headers identify the edition and its upstream baseline.
pub fn session_title() -> Option<Vec<Span<'static>>> {
    enabled().then(|| {
        let (alab, baseline) = running_releases();
        vec![
            Span::styled(SESSION_BRAND, Style::default().bold()),
            Span::raw(release_pair(alab, baseline)),
        ]
    })
}

/// Keep the active permission value visible in the plain launch header. The
/// native renderer supplies its styling and handles values wider than the pane.
pub fn session_permission_caption(width: usize) -> &'static str {
    const CAPTION: &str = "  permissions: ";
    const INDENT: &str = "  ";
    if !enabled() {
        return CAPTION;
    }
    let room = width.saturating_sub("YOLO mode".len());
    if room >= CAPTION.len() - INDENT.len() {
        &CAPTION[CAPTION.len().saturating_sub(room)..]
    } else {
        &INDENT[..INDENT.len().min(room)]
    }
}

/// Keep the release pair together in a wrapped status title. In very narrow panes
/// the native wrapper can show one long word across several lines without losing digits.
pub fn group_status_title(rows: &mut Vec<Line<'static>>, width: usize) {
    if rows.len() < 2 || !enabled() {
        return;
    }
    let (alab, baseline) = running_releases();
    let words_fit = SESSION_BRAND
        .split_ascii_whitespace()
        .chain([display_release(alab), display_release(baseline)])
        .all(|word| painted_width(word) <= width);
    if words_fit {
        *rows = plain_title_rows(alab, baseline, width);
    }
}

/// Reflow the plain launch title when its single-line presentation would clip the versions.
pub fn narrow_session_title(title: &Line<'_>, width: usize) -> Option<Vec<Line<'static>>> {
    if width == 0
        || !enabled()
        || title
            .spans
            .iter()
            .map(|span| painted_width(&span.content))
            .sum::<usize>()
            <= width
    {
        return None;
    }
    let (alab, baseline) = running_releases();
    Some(plain_title_rows(alab, baseline, width))
}

/// The plain launch identity, with the edition and each release fitted to the pane.
pub fn plain_title_rows(alab: &str, baseline: &str, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let indented = |mut span: Span<'static>, content_width: usize| {
        if content_width > width {
            // A lone ellipsis has no preceding glyph to supply its style.
            if width == 1 {
                return Line::from("…");
            }
            span.content = fitted(&span.content, width).into();
        }
        if content_width + 2 <= width {
            Line::from(vec![Span::raw("  "), span])
        } else {
            Line::from(span)
        }
    };
    let brand = SESSION_BRAND.trim_end();
    let brand_words = brand.split_ascii_whitespace();
    let brand_span = |word| Span::styled(word, Style::default().bold());
    let widest_word = brand_words
        .clone()
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0);
    let mut rows = if brand.width() > width && widest_word <= width {
        brand_words
            .map(|word| indented(brand_span(word), widest_word))
            .collect()
    } else {
        vec![indented(brand_span(brand), brand.width())]
    };
    rows.extend(release_rows(alab, baseline, width).into_iter().map(|row| {
        if row.split_number {
            return Line::from(row.text);
        }
        let cells = painted_width(&row.text);
        indented(Span::raw(row.text), cells)
    }));
    rows
}

/// Live values for the edition's single status row. The caller supplies truthful display
/// text, including any quota/context labels and placeholders when data is unavailable.
#[derive(Clone, Copy, Debug, Default)]
pub struct Status<'a> {
    /// Signed-in account email, the account unit's name: shortened before the plan goes.
    pub email: Option<&'a str>,
    /// Weekly quota and reset text, the account unit's last part to go.
    pub weekly: Option<&'a str>,
    /// Subscription plan display text, in the account unit.
    pub plan: Option<&'a str>,
    /// Short quota window, the account unit's first part to go.
    pub five_hour: Option<&'a str>,
    /// Current branch display text.
    pub branch: Option<&'a str>,
    /// Context usage display text; this is the first field dropped while shrinking.
    pub context: Option<&'a str>,
    /// Active model display text.
    pub model: Option<&'a str>,
    /// Current collaboration or permission mode display text.
    pub mode: Option<&'a str>,
    /// Ready/Working activity from the actual task state, independently of context usage.
    pub activity: Option<&'a str>,
}

/// An owned snapshot supplied by the TUI's native account, quota and workspace state.
/// Replacing the entire snapshot prevents stale fields surviving an account change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResetCredits {
    /// Backend-reported count, excluding credits whose known deadline has passed.
    pub available_count: usize,
    /// Known available credits expiring within the next day.
    pub expiring_count: usize,
    /// Known credits whose deadline has passed since the server read.
    pub expired_count: usize,
    /// Earliest known expiration, formatted in the person's local timezone.
    pub next_expiry: Option<String>,
    /// Aged backend data is shown as cached, without a fresh availability claim.
    pub stale: bool,
    /// Whether fresh detail rows cover every backend-reported available credit.
    pub details_known: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreditBalance {
    Current(String),
    LastReported(String),
    Unavailable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndividualCreditLimit {
    Current(String),
    AwaitingRefresh,
}

/// Preserve an individual limit's percentage even when its used or total amount is unreadable.
pub fn individual_limit_amount(formatted: Option<String>) -> Option<String> {
    formatted.or_else(|| active().then(|| "unreported".to_owned()))
}

/// For a validated decimal credit amount, only the significand determines zero.
pub fn credit_amount_is_nonzero(value: &str) -> bool {
    value
        .bytes()
        .take_while(|b| !matches!(b, b'e' | b'E'))
        .any(|b| matches!(b, b'1'..=b'9'))
}

impl CreditBalance {
    pub fn label(&self) -> &str {
        match self {
            Self::Current(label) | Self::LastReported(label) | Self::Unavailable(label) => label,
        }
    }
}

/// The billing service's cumulative report for the displayed conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatCreditUsage {
    pub summary: ChatCreditSummary,
    pub as_of: Option<i64>,
    /// A failed or out-of-order refresh left the previous billing report on screen.
    pub retained: bool,
}

impl ChatCreditUsage {
    pub fn label(&self) -> std::borrow::Cow<'_, str> {
        if self.retained {
            format!("Last report · {}", self.summary.text).into()
        } else {
            self.summary.text.as_str().into()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatCreditSummary {
    pub text: String,
    /// The provider's comparison of this chat's usage with current plan limits.
    pub limit_comparison: Option<String>,
    /// A nonzero debit or refund, or an explicit credit-backed usage source.
    pub credit_activity: bool,
}

pub const UNREPORTED_CHAT_SPENDING: &str = "Chat spending unreported";

/// The spending report available to the current local account in /usage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreditHistory {
    PersonalChats,
    WorkspaceCredits,
}

/// Result of refreshing one chat's billing report under its current identity.
#[derive(Debug)]
pub enum ChatCreditUpdate {
    Read {
        summary: ChatCreditSummary,
        as_of: Option<chrono::DateTime<chrono::Utc>>,
    },
    Clear,
    TemporarilyUnavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InterfaceStatus {
    /// An email address, API-key label, or explicit account-unavailable text.
    pub account: String,
    /// The displayed account came from a ChatGPT login on the selected local server.
    pub local_subscription: bool,
    /// Weekly quota and reset display text, or explicit unavailable text.
    pub weekly: Option<String>,
    /// Exact native weekly reset display text, for account details.
    pub weekly_reset: Option<String>,
    /// Subscription plan for the account the session actually uses.
    pub plan: Option<String>,
    /// Short quota window display text.
    pub five_hour: Option<String>,
    /// Exact native short-window reset display text, for account details.
    pub five_hour_reset: Option<String>,
    /// When the authoritative native usage snapshot was observed, for account details.
    pub usage_checked_at: Option<String>,
    /// Whether quota text represents an aged snapshot instead of a current percentage.
    pub usage_stale: bool,
    /// Whether the short window is stale independently of a current weekly cycle.
    pub five_hour_stale: bool,
    /// Current branch display text.
    pub branch: Option<String>,
    /// Current model display text.
    pub model: Option<String>,
    /// Context usage display text.
    pub context: Option<String>,
    /// Collaboration and permissions display text.
    pub mode: Option<String>,
    /// Actual task activity, such as Ready or Working.
    pub activity: Option<String>,
    /// Numeric weekly usage and the observed history for its current reset cycle.
    pub quota: Option<crate::quota::Weekly>,
    /// Available usage credits from the ordinary account snapshot.
    pub credit_balance: Option<CreditBalance>,
    /// Remaining monthly individual credit allowance and its reset, when reported.
    pub individual_credit_limit: Option<IndividualCreditLimit>,
    pub credit_history: Option<CreditHistory>,
    /// Settled usage charged to this conversation, when reported by the backend.
    pub thread_credit_usage: Option<ChatCreditUsage>,
    /// Inspector message while the selected chat has no billing report to display.
    pub thread_credit_notice: Option<&'static str>,
    /// Earned reset credits, kept separate from the weekly automatic reset.
    pub reset_credits: Option<ResetCredits>,
}

impl Default for InterfaceStatus {
    fn default() -> Self {
        Self {
            account: "Account unavailable".to_string(),
            local_subscription: false,
            weekly: None,
            weekly_reset: None,
            plan: None,
            five_hour: None,
            five_hour_reset: None,
            usage_checked_at: None,
            usage_stale: false,
            five_hour_stale: false,
            branch: None,
            model: None,
            context: None,
            mode: None,
            activity: None,
            quota: None,
            credit_balance: None,
            individual_credit_limit: None,
            credit_history: None,
            thread_credit_usage: None,
            thread_credit_notice: None,
            reset_credits: None,
        }
    }
}

impl InterfaceStatus {
    fn values(&self) -> Status<'_> {
        Status {
            email: Some(if self.account.trim().is_empty() {
                "Account unavailable"
            } else {
                &self.account
            }),
            weekly: Some(self.weekly.as_deref().unwrap_or("weekly unavailable")),
            plan: self.plan.as_deref(),
            five_hour: self.five_hour.as_deref(),
            branch: self.branch.as_deref(),
            context: self.context.as_deref(),
            model: self.model.as_deref(),
            mode: self.mode.as_deref(),
            activity: self.activity.as_deref(),
        }
    }
}

/// The displayed account/quota values, independent of workspace and task activity.
#[derive(PartialEq, Eq)]
struct AccountCluster {
    email: Option<String>,
    plan: Option<String>,
    weekly: Option<String>,
    five_hour: Option<String>,
}

impl AccountCluster {
    fn of(status: Status<'_>) -> Self {
        Self {
            email: display_text(status.email),
            plan: display_text(status.plan),
            weekly: display_text(status.weekly),
            five_hour: display_text(status.five_hour),
        }
    }

    /// Each visible companion consumes its text plus one join beside the weekly meter.
    fn width_beside_weekly(&self) -> usize {
        [&self.email, &self.plan, &self.five_hour]
            .into_iter()
            .flatten()
            .map(|value| painted_width(value) + STATUS_JOIN_WIDTH)
            .sum()
    }
}

struct PaintedAccount {
    cluster: AccountCluster,
    rect: Option<Rect>,
}

/// A presentation change retires every target painted on the old composer row.
fn clear_overline_hits(state: &State) {
    if let Ok(mut hits) = state.overline_hits.lock() {
        *hits = OverlineHits::default();
    }
}

fn clear_account_hits() {
    if let Some(state) = state()
        && let Ok(mut hits) = state.account_hits.lock()
    {
        *hits = None;
    }
}

/// Whether this cell was painted inside the account unit (email, plan and quota, with the
/// unit's own joins): one target. The boundary after it and the workspace fields are never
/// targets. A changed account/quota snapshot rejects old geometry until it is painted again;
/// task-activity updates keep valid targets.
pub fn account_mouse_hit(column: u16, row: u16) -> bool {
    let Some(state) = state() else { return false };
    if state.choice.load(Ordering::Acquire) != ON {
        return false;
    }
    let Ok(current) = state.status.lock() else {
        return false;
    };
    let cluster = current
        .as_ref()
        .map(|snapshot| AccountCluster::of(snapshot.values()));
    drop(current);
    state.account_hits.lock().ok().is_some_and(|hits| {
        hits.as_ref().is_some_and(|painted| {
            cluster
                .as_ref()
                .is_none_or(|current| *current == painted.cluster)
                && painted
                    .rect
                    .is_some_and(|rect| row == rect.y && column >= rect.x && column < rect.right())
        })
    })
}

/// Replace the complete live status snapshot. This also updates while presentation is off.
pub fn set_status(mut snapshot: InterfaceStatus) {
    if let Some(state) = state()
        && let Ok(mut current) = state.status.lock()
    {
        snapshot.account = display_text(Some(&snapshot.account))
            .filter(|account| painted_width(account) > 0)
            .unwrap_or_else(|| "Account unavailable".to_string());
        if current.as_ref().map(|old| AccountCluster::of(old.values()))
            != Some(AccountCluster::of(snapshot.values()))
            || current.as_ref().is_some_and(|old| {
                old.quota != snapshot.quota
                    || old.reset_credits != snapshot.reset_credits
                    || old.credit_balance != snapshot.credit_balance
                    || old.thread_credit_usage != snapshot.thread_credit_usage
            })
        {
            clear_account_hits();
        }
        *current = Some(snapshot);
    }
}

/// The latest status with native transport availability overlaid on its task activity.
/// The stored task activity remains current while disconnected and returns on reconnect.
pub fn status_snapshot() -> Option<InterfaceStatus> {
    let mut snapshot = stored_status()?;
    if let Some(label) = connection_state().label() {
        snapshot.activity = Some(label.to_owned());
    }
    Some(snapshot)
}

fn stored_status() -> Option<InterfaceStatus> {
    state()?.status.lock().ok()?.clone()
}

/// The selected server's address for the local accounts pane. The roster caches its
/// own observations separately, so it cannot retain this address across a disconnect.
pub fn connected_local_email() -> Option<String> {
    if !active() || connection_state() != ConnectionState::Connected {
        return None;
    }
    let current = state()?.status.lock().ok()?;
    let status = current.as_ref()?;
    status.local_subscription.then(|| status.account.clone())
}

/// Paint the active objective above the account/quota snapshot. In a clipped
/// one-row viewport the objective remains visible; account targets exist only
/// where an account row was actually painted.
pub fn render_status_current(area: Rect, buf: &mut Buffer, depth: Depth, theme: Theme) {
    if !enabled() {
        clear_account_hits();
        return;
    }
    let mut area = area.intersection(buf.area);
    if let Some(goal) = current_goal()
        && !area.is_empty()
    {
        for x in area.x..area.right() {
            buf[(x, area.y)].reset();
        }
        let label = fitted(&format!("Goal: {goal}"), usize::from(area.width));
        Line::styled(label, StatusPalette::of(depth, theme).accent)
            .render(Rect::new(area.x, area.y, area.width, 1), buf);
        area.y = area.y.saturating_add(1);
        area.height = area.height.saturating_sub(1);
    }
    if area.is_empty() {
        clear_account_hits();
        return;
    }
    let mut snapshot = stored_status();
    let connection = connection_state();
    if let Some(label) = connection.label() {
        if let Some(snapshot) = &mut snapshot {
            snapshot.activity = None;
        }
        area = render_connection_prefix(area, buf, label, snapshot.is_some(), depth, theme);
    }
    let Some(snapshot) = snapshot.filter(|_| !area.is_empty()) else {
        render_status(area, buf, Status::default(), depth, theme);
        return;
    };
    let mut values = snapshot.values();
    let cluster = AccountCluster::of(values);
    let width = usize::from(area.width);
    let weekly = quota_footer(&snapshot, &cluster, width, connection);
    if let Some(weekly) = weekly.as_deref() {
        values.weekly = Some(weekly);
    }
    let weekly_area = render_status_row(area, buf, values, depth, theme);
    // Hit geometry belongs to the rendered summary, but its validity belongs to the
    // native account snapshot. A subsequent quota/history change invalidates it above.
    if let Some(state) = state()
        && let Ok(mut hits) = state.account_hits.lock()
        && let Some(painted) = hits.as_mut()
    {
        painted.cluster = cluster;
    }
    if let Some(credits) = snapshot
        .reset_credits
        .as_ref()
        .filter(|credits| !credits.stale && credits.expiring_count > 0)
    {
        let Some(area) = weekly_area else { return };
        let warning = match (depth, theme.is_light()) {
            (Depth::Mono, _) => Style::default().bold(),
            (_, false) => Style::default().fg(Color::LightYellow).bold(),
            (_, true) => FaceInks::of(depth, theme).amber,
        };
        let mut highlighted = 0;
        for x in area.x..area.right() {
            let cell = &mut buf[(x, area.y)];
            if cell.symbol() != "●" {
                continue;
            }
            if highlighted < credits.expiring_count {
                // Some terminal fonts paint the same filled circle at both weights.
                if depth == Depth::Mono || (depth == Depth::Ansi16 && theme.is_light()) {
                    cell.set_symbol("◉");
                }
                cell.set_style(warning);
            } else {
                cell.modifier.remove(Modifier::BOLD);
            }
            highlighted += 1;
        }
    }
}

/// Reserve the leading cells for the transport label before measuring the cached quota.
/// The account renderer receives only the remaining cells, including for mouse hits.
fn render_connection_prefix(
    area: Rect,
    buf: &mut Buffer,
    label: &str,
    has_snapshot: bool,
    depth: Depth,
    theme: Theme,
) -> Rect {
    let palette = StatusPalette::of(depth, theme);
    let mut line = Line::styled(fitted(label, usize::from(area.width)), palette.detail);
    let prefix_width = line.width() + STATUS_JOIN_WIDTH;
    let width = if has_snapshot && prefix_width < usize::from(area.width) {
        line.push_span(Span::styled(ACCOUNT_BOUNDARY, palette.divider));
        prefix_width as u16
    } else {
        area.width
    };
    let prefix = Rect::new(area.x, area.y, width, 1);
    for x in prefix.x..prefix.right() {
        buf[(x, prefix.y)].reset();
    }
    line.render(prefix, buf);
    Rect::new(prefix.right(), area.y, area.width - width, 1)
}

/// Put settled chat spend and the account balance ahead of the weekly graph.
/// The account inspector keeps the exact figures at every terminal width.
fn quota_footer(
    status: &InterfaceStatus,
    cluster: &AccountCluster,
    width: usize,
    connection: ConnectionState,
) -> Option<String> {
    let report = status
        .thread_credit_usage
        .as_ref()
        .filter(|report| report.summary.credit_activity);
    let spend = report.map(ChatCreditUsage::label);
    let balance = status.credit_balance.as_ref().map(CreditBalance::label);
    let balance_first = match &status.credit_balance {
        Some(CreditBalance::Unavailable(_)) => true,
        Some(CreditBalance::Current(_)) => report.is_some_and(|report| report.retained),
        _ => false,
    };
    let fields = if balance_first {
        [balance, spend.as_deref()]
    } else {
        [spend.as_deref(), balance]
    };
    let mut fields = fields.into_iter().flatten();
    let resets = status.reset_credits.as_ref();
    let Some(first) = fields.next() else {
        return weekly_footer(status, cluster, width, connection, resets);
    };
    let mut text = first.to_owned();
    if let Some(next) = fields.next() {
        text.push_str(" · ");
        text.push_str(next);
    }
    let quota_width = width.saturating_sub(painted_width(&text) + STATUS_JOIN_WIDTH);
    // The inspector retains the empty inventory when the footer has credit figures.
    let resets = resets.filter(|credits| credits.stale || credits.available_count > 0);
    let detailed = weekly_footer(status, cluster, quota_width, connection, resets);
    if let Some(weekly) = [detailed.as_deref(), status.weekly.as_deref()]
        .into_iter()
        .flatten()
        .find(|reading| painted_width(reading) <= quota_width)
    {
        text.push_str(" · ");
        text.push_str(weekly);
    }
    Some(text)
}

fn weekly_footer(
    status: &InterfaceStatus,
    cluster: &AccountCluster,
    row_width: usize,
    connection: ConnectionState,
    resets: Option<&ResetCredits>,
) -> Option<String> {
    let quota = status.quota.as_ref().filter(|_| !status.usage_stale);
    if quota.is_none() && resets.is_none() {
        return None;
    }
    let compact_expiry = resets
        .filter(|credits| !credits.stale && credits.expiring_count > 0)
        .map(|credits| {
            let minimum = if credits.details_known { "" } else { "≥" };
            let count = credits.expiring_count;
            let action = if count == 1 {
                "reset expires"
            } else {
                "resets expire"
            };
            format!("{minimum}{count} {action} soon")
        })
        .filter(|notice| painted_width(notice) <= row_width);
    let mut meter_with_alias = None;
    let mut text = if let Some(quota) = quota {
        let used = quota.used_percent.clamp(0.0, 100.0);
        let filled = (used * 8.0 / 100.0).round() as usize;
        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(8 - filled));
        let usage = format!("{used:.0}% used");
        let mut text = format!("weekly [{bar}] {usage}");
        let meter_width = painted_width(&text);
        let compact_width = "weekly ".len() + usage.len();
        // Reserve the same minimum name and join as shrink_account only when a
        // complete numeric reading fits beside them. Credit warnings keep priority.
        let alias_budget = (connection == ConnectionState::Connected
            && status.local_subscription
            && !resets.is_some_and(|credits| credits.stale || credits.expiring_count > 0))
        .then(|| {
            let name = painted_width(&status.account).min(ACCOUNT_NAME_FLOOR);
            row_width.saturating_sub(name + STATUS_JOIN_WIDTH)
        })
        .filter(|width| *width >= compact_width);
        let width = alias_budget.unwrap_or(row_width);
        // Weekly is the last field to be clipped. At an exact fit, adding the reset
        // would replace the last letter of the reading with an ellipsis.
        if compact_expiry.is_none() && width == meter_width {
            return Some(text);
        }
        if compact_expiry.is_none() && width < meter_width && compact_width <= width {
            return Some(format!("weekly {usage}"));
        }
        meter_with_alias = alias_budget.map(|width| (width, text.len()));
        if let Some(reset) = status.weekly_reset.as_deref() {
            text.push_str(&format!(" ↻ {reset}"));
        }
        text
    } else {
        status
            .weekly
            .as_deref()
            .unwrap_or("weekly unavailable")
            .to_owned()
    };
    if let Some(credits) = resets {
        if credits.stale {
            text.push_str(" · resets stale");
        } else {
            let count = credits.available_count;
            let dots = "●".repeat(count.min(5));
            text.push_str(&format!(" · resets {dots}"));
            if count == 0 {
                text.push('0');
            }
            if count > 5 {
                text.push_str(&format!("+{}", count - 5));
            }
            if credits.expiring_count > 0 {
                let label = match (credits.details_known, credits.expiring_count) {
                    (false, _) => "known expiring",
                    (true, 1) => "expires soon",
                    (true, _) => "expire soon",
                };
                text.push_str(&format!(" ({} {label})", credits.expiring_count));
            }
        }
    }
    if let Some(expiry) = compact_expiry
        && painted_width(&text) > row_width
    {
        if let Some(weekly) = status.weekly.as_deref()
            && painted_width(weekly) + STATUS_JOIN_WIDTH + painted_width(&expiry) <= row_width
        {
            return Some(format!("{weekly} · {expiry}"));
        }
        return Some(expiry);
    }
    let Some(quota) = quota else {
        return Some(text);
    };
    if let Some((budget, meter_end)) = meter_with_alias
        && painted_width(&text) > budget
    {
        text.truncate(meter_end);
        return Some(text);
    }
    let width = row_width.saturating_sub(cluster.width_beside_weekly());
    let mut trend = text.clone();
    if quota.sample_count >= 2 {
        trend.push_str(&format!(" · {}", quota.sparkline(10)));
    }
    if let Some(projection) = quota.projection.as_ref() {
        let projected = format!(
            "{trend} · est {:.0}% at reset",
            projection.projected_percent_at_reset
        );
        if painted_width(&projected) <= width {
            return Some(projected);
        }
    }
    if painted_width(&trend) <= width {
        Some(trend)
    } else {
        Some(text)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Email,
    Weekly,
    Plan,
    FiveHour,
    Mode,
    Activity,
    Branch,
    Model,
    Context,
}

impl Field {
    /// Whether this is a part of the account unit: the account, its plan and allowances.
    fn in_account(self) -> bool {
        matches!(
            self,
            Field::Email | Field::Plan | Field::Weekly | Field::FiveHour
        )
    }
}

/// What joins two parts of the account unit.
const ACCOUNT_JOIN: &str = " / ";
/// What sets the account unit apart from the workspace fields beside it.
const ACCOUNT_BOUNDARY: &str = " │ ";
/// What joins two workspace fields.
const FIELD_JOIN: &str = " · ";
/// All three status dividers occupy the same number of terminal cells.
const STATUS_JOIN_WIDTH: usize = ACCOUNT_JOIN.len();
/// The fewest columns the account name shortens to (`andrew@…`) before it goes.
const ACCOUNT_NAME_FLOOR: usize = 8;

fn join(before: Field, after: Field) -> &'static str {
    match (before.in_account(), after.in_account()) {
        (true, true) => ACCOUNT_JOIN,
        (false, false) => FIELD_JOIN,
        _ => ACCOUNT_BOUNDARY,
    }
}

fn display_text(value: Option<&str>) -> Option<String> {
    let value = value?;
    let text: String = value.chars().filter(|ch| !ch.is_control()).collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

pub(crate) fn fitted(text: &str, width: usize) -> String {
    if painted_width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut used = 0;
    // Fit the same graphemes the terminal renderer paints. Scalar widths can
    // split joined emoji or undercount a variation selector's display width.
    let span = Span::raw(text);
    for grapheme in span.styled_graphemes(Style::default()) {
        let columns = usize::from(grapheme.symbol.cell_width());
        if used + columns > width - 1 {
            break;
        }
        result.push_str(grapheme.symbol);
        used += columns;
    }
    result.push('…');
    result
}

/// Keep the distinguishing end of a long email username when even the whole username and
/// an ellipsis cannot fit. Once they fit, show as much of the full address as usual.
fn fitted_account(account: &str, width: usize) -> String {
    let Some((local, domain)) = account.split_once('@') else {
        return fitted(account, width);
    };
    if width < 2
        || domain.is_empty()
        || domain.contains('@')
        || account.contains(char::is_whitespace)
    {
        return fitted(account, width);
    }

    let local_span = Span::raw(local);
    let mut local_width = 0;
    let mut last_width = 0;
    for grapheme in local_span.styled_graphemes(Style::default()) {
        let cells = usize::from(grapheme.symbol.cell_width());
        if cells > 0 {
            local_width += cells;
            last_width = cells;
        }
    }
    if local_width < width || last_width >= width {
        return fitted(account, width);
    }

    // Reserve half the space around the ellipsis for the end of the username, or a whole
    // final grapheme when it needs more. Use the cell widths Ratatui paints for both ends.
    let tail_width = ((width - 1) / 2).max(last_width);
    let mut result = fitted(local, width - tail_width);
    let mut remaining = local_width;
    for grapheme in local_span.styled_graphemes(Style::default()) {
        if remaining <= tail_width {
            result.push_str(grapheme.symbol);
        }
        remaining -= usize::from(grapheme.symbol.cell_width());
    }
    result
}

struct StatusField {
    kind: Field,
    text: String,
    width: usize,
}

impl StatusField {
    fn new(kind: Field, text: String) -> Self {
        let width = painted_width(&text);
        Self { kind, text, width }
    }

    fn replace(&mut self, text: String) {
        *self = Self::new(self.kind, text);
    }
}

// Responsive fitting can evaluate the same fields several times in one frame.
fn fields_width(fields: &[StatusField]) -> usize {
    fields.iter().map(|field| field.width).sum::<usize>()
        + fields.len().saturating_sub(1) * STATUS_JOIN_WIDTH
}

/// The account unit shrunk as one into `width`, once the workspace fields have gone: the
/// five-hour allowance goes, the account name shortens (to [`ACCOUNT_NAME_FLOOR`]) beside
/// the plan, the plan goes and the name takes its room, then the name goes. The weekly
/// allowance goes last: only the part left alone is cut to the width.
fn shrink_account(fields: &mut Vec<StatusField>, width: usize) {
    if fields_width(fields) <= width {
        return;
    }
    let name = fields
        .iter()
        .find(|field| field.kind == Field::Email)
        .map(|field| (field.text.clone(), field.width));
    let drop_part = |fields: &mut Vec<StatusField>, part: Field| {
        if fields.len() > 1 {
            fields.retain(|field| field.kind != part);
        }
    };
    let shorten_name = |fields: &mut Vec<StatusField>| {
        let (Some((name, name_width)), Some(at)) = (
            name.as_ref(),
            fields.iter().position(|field| field.kind == Field::Email),
        ) else {
            return;
        };
        let others = fields_width(fields) - fields[at].width;
        let room = width.saturating_sub(others);
        if fields.len() == 1 || room >= ACCOUNT_NAME_FLOOR.min(*name_width) {
            fields[at].replace(fitted_account(name, room));
        }
    };
    for step in 0..5 {
        match step {
            0 => drop_part(fields, Field::FiveHour),
            1 => shorten_name(fields),
            2 => {
                drop_part(fields, Field::Plan);
                shorten_name(fields);
            }
            3 => drop_part(fields, Field::Email),
            _ => {
                if let [field] = fields.as_mut_slice() {
                    field.replace(fitted(&field.text, width));
                }
            }
        }
        if fields_width(fields) <= width {
            break;
        }
    }
}

/// Build a bounded single status line. The account name, plan and allowances are one unit,
/// joined by `/` and set apart from the workspace fields by `│`. Context and the workspace
/// fields drop first; then the unit shrinks as one ([`shrink_account`]), keeping the weekly
/// allowance last. This is pure rendering; callers can measure it without changing live
/// interface state.
pub fn status_line(width: u16, status: Status<'_>, depth: Depth, theme: Theme) -> Line<'static> {
    status_layout(width, status, depth, theme)
        .spans
        .into_iter()
        .map(|(span, _)| span)
        .collect()
}

struct StatusLayout {
    spans: Vec<(Span<'static>, u16)>,
    /// The account unit's one range relative to the row's left edge, its own joins included,
    /// measured after responsive fitting.
    account_range: Option<(u16, u16)>,
    /// The weekly field alone owns reset-credit adornments; other text may contain dots.
    weekly_range: Option<(u16, u16)>,
}

fn status_layout(width: u16, status: Status<'_>, depth: Depth, theme: Theme) -> StatusLayout {
    let width = usize::from(width);
    if width == 0 {
        return StatusLayout {
            spans: Vec::new(),
            account_range: None,
            weekly_range: None,
        };
    }
    let mut fields: Vec<_> = [
        (Field::Email, status.email),
        (Field::Plan, status.plan),
        (Field::Weekly, status.weekly),
        (Field::FiveHour, status.five_hour),
        (
            Field::Activity,
            status
                .activity
                .filter(|value| !matches!(*value, "Ready" | "Working" | "Thinking")),
        ),
        (
            Field::Mode,
            status.mode.filter(|value| {
                !matches!(
                    value.to_ascii_lowercase().as_str(),
                    "default" | "full access" | "default / full access"
                )
            }),
        ),
        (Field::Branch, status.branch),
        (Field::Model, status.model),
        (Field::Context, status.context),
    ]
    .into_iter()
    .filter_map(|(field, value)| display_text(value).map(|value| StatusField::new(field, value)))
    .collect();
    for optional in [
        Field::Context,
        Field::Mode,
        Field::Model,
        Field::Branch,
        Field::Activity,
    ] {
        if fields_width(&fields) <= width {
            break;
        }
        fields.retain(|field| field.kind != optional);
    }
    shrink_account(&mut fields, width);
    let palette = StatusPalette::of(depth, theme);
    let mut spans = Vec::new();
    let mut account: Option<(usize, usize)> = None;
    let mut weekly_range = None;
    let mut column = 0;
    let mut previous = None;
    for field in fields {
        if let Some(previous) = previous {
            let join = join(previous, field.kind);
            spans.push((
                Span::styled(join, palette.divider),
                STATUS_JOIN_WIDTH as u16,
            ));
            column += STATUS_JOIN_WIDTH;
        }
        previous = Some(field.kind);
        if field.kind == Field::Weekly {
            weekly_range = Some((column as u16, field.width as u16));
        }
        if field.width > 0 && field.kind.in_account() {
            let start = account.map_or(column, |(start, _)| start);
            account = Some((start, column + field.width));
        }
        column += field.width;
        let style = match field.kind {
            Field::Email => palette.account,
            Field::Weekly => palette.accent,
            _ => palette.detail,
        };
        spans.push((Span::styled(field.text, style), field.width as u16));
    }
    StatusLayout {
        spans,
        account_range: account.map(|(start, end)| (start as u16, (end - start) as u16)),
        weekly_range,
    }
}

/// Paint the live status row when the edition is enabled. The supplied area owns exactly
/// this row, so clearing it prevents stale characters after an account or branch change.
pub fn render_status(area: Rect, buf: &mut Buffer, status: Status<'_>, depth: Depth, theme: Theme) {
    let _ = render_status_row(area, buf, status, depth, theme);
}

fn render_status_row(
    area: Rect,
    buf: &mut Buffer,
    status: Status<'_>,
    depth: Depth,
    theme: Theme,
) -> Option<Rect> {
    clear_account_hits();
    if !enabled() {
        return None;
    }
    let area = area.intersection(buf.area);
    if area.height == 0 {
        return None;
    }
    let row = Rect::new(area.x, area.y, area.width, 1);
    for x in row.x..row.right() {
        buf[(x, row.y)].set_symbol(" ").set_style(Style::default());
    }
    let layout = status_layout(row.width, status, depth, theme);
    let weekly_area = layout
        .weekly_range
        .map(|(column, width)| Rect::new(row.x.saturating_add(column), row.y, width, 1));
    // Ratatui's Line advances by each span's string width, while Span paints graphemes.
    // Advance by the painted cells so subsequent fields and their hit targets stay aligned.
    let mut x = row.x;
    for (span, width) in layout.spans {
        span.render(Rect::new(x, row.y, width, 1), buf);
        x = x.saturating_add(width);
    }
    if let Some(state) = state()
        && let Ok(mut hits) = state.account_hits.lock()
    {
        *hits = Some(PaintedAccount {
            cluster: AccountCluster::of(status),
            rect: layout
                .account_range
                .map(|(column, width)| Rect::new(row.x.saturating_add(column), row.y, width, 1)),
        });
    }
    weekly_area
}

#[cfg(test)]
mod tests;
