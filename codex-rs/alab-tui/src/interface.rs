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
use crate::session::HarnessWord;
use codex_alab_core::session::Shared;
use codex_alab_core::session::TaskFace;
use ratatui::buffer::Buffer;
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

/// Presentation state, separate from the harness's automation state.
pub struct State {
    choice: AtomicU8,
    connection: AtomicU8,
    last_button: Mutex<Option<Rect>>,
    build_button: Mutex<Option<Rect>>,
    /// The latest enhanced frame's Autopilot cells, the mode buttons and the face, and what a
    /// press on each does ([`Hit`]).
    mode_buttons: Mutex<Vec<(Rect, Hit)>>,
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
            last_button: Mutex::new(None),
            build_button: Mutex::new(None),
            mode_buttons: Mutex::new(Vec::new()),
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
        clear_switch_hits();
        clear_account_hits();
    }
}

/// Whether the enhanced interface is enabled. Autopilot mode is a separate choice.
pub fn enabled() -> bool {
    state().is_some_and(|state| state.choice.load(Ordering::Acquire) == ON)
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
pub fn set_enabled(enabled: bool) {
    if SCOPED.with(Cell::get).is_none() {
        codex_alab_core::set_enhanced(enabled);
    }
    if let Some(state) = state() {
        state
            .choice
            .store(if enabled { ON } else { OFF }, Ordering::Release);
        clear_switch_hits();
        clear_account_hits();
    }
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
                if SCOPED.with(Cell::get).is_none() {
                    codex_alab_core::set_enhanced(next == ON);
                }
                clear_switch_hits();
                clear_account_hits();
                return next == ON;
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
            (Depth::Ansi256, true) => (Color::Indexed(24), Color::Indexed(30), Color::Indexed(243)),
            (Depth::Ansi256, false) => {
                (Color::Indexed(255), Color::Indexed(80), Color::Indexed(245))
            }
            (_, true) => (Color::Black, Color::Cyan, Color::DarkGray),
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

fn button_label(width: u16, on: bool) -> &'static str {
    match (width >= 20, on) {
        (true, true) => "[ALab on]",
        (true, false) => "[ALab off]",
        (false, true) => "[on]",
        (false, false) => "[off]",
    }
}

fn button_for(area: Rect, on: bool) -> Option<Rect> {
    let width = UnicodeWidthStr::width(button_label(area.width, on)) as u16;
    (area.height >= 3 && area.width >= width + 4)
        .then(|| Rect::new(area.right() - width - 2, area.y, width, 1))
}

/// The clickable on/off button on this composer's top border, if it fits.
/// Use the same rectangle passed to [`render`].
pub fn button_rect(composer: Rect) -> Option<Rect> {
    let state = state()?;
    if state.choice.load(Ordering::Acquire) == UNCONFIGURED {
        return None;
    }
    button_for(composer, enabled())
}

/// Whether a terminal cell belongs to the button most recently painted by [`render`].
/// The caller owns mouse-button filtering and input ownership before calling [`toggle`].
pub fn mouse_hit(column: u16, row: u16) -> bool {
    state()
        .and_then(|state| state.last_button.lock().ok())
        .is_some_and(|button| {
            button.is_some_and(|area| column >= area.x && column < area.right() && row == area.y)
        })
}

/// Draw controls on the existing composer overline. When enhancements are off,
/// only the recovery switch is drawn; native content and automation stay unchanged.
pub fn render(composer: Rect, buf: &mut Buffer, depth: Depth, theme: Theme) {
    let Some(state) = state() else { return };
    let on = enabled();
    if let Ok(mut buttons) = state.mode_buttons.lock() {
        buttons.clear();
    }
    if let Ok(mut build) = state.build_button.lock() {
        *build = None;
    }
    let area = composer.intersection(buf.area);
    let button = button_rect(area);
    if let Ok(mut last) = state.last_button.lock() {
        *last = button;
    }
    let Some(button) = button else { return };
    render_border(area, buf, on, button, depth, theme);
    if on {
        render_mode_buttons(area, button, buf, depth, theme);
    }
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
}

/// The Autopilot cell of the latest enhanced frame at `(column, row)`.
fn hit_at(column: u16, row: u16) -> Option<Hit> {
    if !enabled() {
        return None;
    }
    let state = state()?;
    state
        .mode_buttons
        .lock()
        .ok()?
        .iter()
        .find_map(|(area, hit)| {
            (column >= area.x && column < area.right() && row == area.y).then_some(*hit)
        })
}

/// A mode click is accepted only from the latest enhanced frame: a mode button, or the
/// Autopilot face beside them, whose click selects what a click on aterm's chip does.
pub fn mode_mouse_hit(column: u16, row: u16) -> Option<HarnessWord> {
    match hit_at(column, row)? {
        Hit::Mode(mode) => Some(mode),
        Hit::Words => None,
    }
}

/// Whether a click at `(column, row)` falls on a face that selects no mode (`⊘ off`, the
/// harness off by one of aterm's Settings switches or with no host): the caller says
/// [`autopilot_words`], which name where it is turned.
pub fn autopilot_words_hit(column: u16, row: u16) -> bool {
    hit_at(column, row) == Some(Hit::Words)
}

/// Whether this cell belongs to a native Autopilot control in the latest frame.
pub fn autopilot_menu_hit(column: u16, row: u16) -> bool {
    hit_at(column, row).is_some()
}

/// ` codex-alab `: the brand's last rung with its spaces ([`brand_label`]).
const BRAND_FLOOR: u16 = 12;
/// THE WHOLE FACE'S SLOT: the widest face (`∞ ⤓18:00`, `✓ landed`: 8 cells) and a space each
/// side.
const FACE_SLOT: u16 = 10;
/// THE GLYPH-ONLY RUNG (aterm's `Keep::GlyphOnly`): one glyph and a space each side.
const GLYPH_SLOT: u16 = 3;

fn render_mode_buttons(area: Rect, toggle: Rect, buf: &mut Buffer, depth: Depth, theme: Theme) {
    let Some(shared) = crate::boxes::tick_shared() else {
        return;
    };
    // ONE LOAD OF THE WORD draws the face, the highlight and the hits, so no frame pairs the
    // face of one word with the highlight of another (review of gap 12, findings 5 and 9).
    let selected = shared.harness_word();
    if selected == HarnessWord::Unknown || area.width < 52 {
        return;
    }
    // The buttons select a mode where the harness runs one; a harness held off by a Settings
    // switch, or with no host, wears its face alone, the buttons' cells kept for the rule.
    let controls = matches!(
        selected,
        HarnessWord::Cruise | HarnessWord::Land | HarnessWord::Off
    );
    let labels = [
        ("[Cruise]", HarnessWord::Cruise),
        ("[Land]", HarnessWord::Land),
        ("[Off]", HarnessWord::Off),
    ];
    let width = 23;
    let start = toggle.x.saturating_sub(width + 1);
    // THE FACE'S SLOT, left of `[Cruise]` past the one `─` before it, goes by the width alone,
    // never by the face: no face moves the brand or the controls. The brand keeps its last
    // rung beside it.
    let slot = [FACE_SLOT, GLYPH_SLOT]
        .into_iter()
        .find(|slot| start.saturating_sub(area.x + 4 + slot) >= BRAND_FLOOR)
        .unwrap_or(0);
    // Refit the brand before the controls; no control overlays a version label. Beside the
    // slot a `─` stays between them.
    let available = if slot > 0 {
        start - (area.x + 4 + slot)
    } else {
        start.saturating_sub(area.x + 3)
    };
    // THE CELLS REDRAWN HERE CARRY THEIR OWN STYLES' MODIFIERS ALONE: ratatui's styles patch a
    // cell, adding modifiers and taking none away, so the cells the brand's bold label covered
    // before its refit kept its bold under the rule, a muted face and a muted button (review
    // of gap 12, finding 2). Their inks stay the rule's tint.
    let palette = Palette::new(depth, theme);
    for x in area.x + 2..toggle.x {
        let cell = &mut buf[(x, area.y)];
        cell.set_symbol("─");
        cell.modifier = Modifier::empty();
    }
    if let Some(label) = current_brand_label(
        usize::from(available),
        codex_alab_core::version::alab(),
        &codex_alab_core::version::upstream(),
    ) {
        note_build_button(area.x + 3, area.y, &label);
        let bold = Style::default().add_modifier(palette.brand.add_modifier);
        Line::from(vec![
            Span::raw(" "),
            Span::styled(label, bold),
            Span::raw(" "),
        ])
        .render(Rect::new(area.x + 2, area.y, available, 1), buf);
    }
    let mut hits = Vec::new();
    if slot > 0
        && let Some(face) = Face::read(shared, selected)
    {
        // Right-aligned in its slot, a space each side; the slot's rest keeps the rule. Only
        // the face's own cells click.
        let line = face.line(slot == FACE_SLOT, FaceInks::of(depth, theme));
        let face_width = line.width() as u16;
        let x = start - 2 - face_width;
        buf[(x - 1, area.y)].set_symbol(" ");
        buf[(start - 2, area.y)].set_symbol(" ");
        let rect = Rect::new(x, area.y, face_width, 1);
        line.render(rect, buf);
        hits.push((rect, face.hit()));
    }
    let mut x = start;
    for (label, mode) in labels.into_iter().filter(|_| controls) {
        let width = label.len() as u16;
        let rect = Rect::new(x, area.y, width, 1);
        let style = if selected == mode {
            palette.button.add_modifier(Modifier::REVERSED)
        } else {
            palette.muted
        };
        Line::styled(label, style).render(rect, buf);
        hits.push((rect, Hit::Mode(mode)));
        x += width + 1;
    }
    if let Some(state) = state()
        && let Ok(mut buttons) = state.mode_buttons.lock()
    {
        *buttons = hits;
    }
}

/// aterm's `AMBER` (`claude_lights.rs`): the chip's Land and landed ink.
const AMBER: (u8, u8, u8) = (0xE8, 0xB0, 0x3C);

/// THE FACE'S INKS (aterm's `ChipTone`s): accent, amber and muted, each floored against the
/// ground as aterm floors its chip's hue, as the overline's spectrum is
/// ([`crate::banner::readable_text`]), at true colour and 256 colours; on a 16-colour terminal a
/// named colour of the theme's own palette, which no floor can move, as the banner's are
/// (review of gap 12, finding 4: this said every ink was floored); bold and dim in mono. The
/// controls keep their own.
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
        // The interface's own accent and muted bases, and its named colours by theme.
        let (accent, accent16, muted, muted16, amber16) = if light {
            (
                (0, 105, 112),
                Color::Cyan,
                (101, 117, 126),
                Color::DarkGray,
                Color::Yellow,
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

/// A TIMED CRUISE'S LANDING as the line says it ([`clock_in`]): its local `HH:MM`, and its
/// weekday where it lands on another local date than today.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Lands {
    clock: String,
    day: Option<chrono::Weekday>,
}

/// THE TIMER'S LOCAL TIME, `at` (unix seconds) in `zone`, and its weekday where it lands on
/// another local date than `now`'s; `None` for a time chrono cannot place, whose face is plain
/// Cruise's.
fn clock_in<Tz: chrono::TimeZone>(
    at: u64,
    zone: &Tz,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<Lands> {
    use chrono::Datelike;
    use chrono::Timelike;
    let at = zone.timestamp_opt(i64::try_from(at).ok()?, 0).single()?;
    let today = now.with_timezone(zone).date_naive();
    Some(Lands {
        clock: format!("{:02}:{:02}", at.hour(), at.minute()),
        day: (at.date_naive() != today).then(|| at.weekday()),
    })
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
    /// `∞`: Cruise.
    Cruise,
    /// `∞ ⤓18:00`: Cruise until a time, then Land (aterm's timer).
    CruiseUntil(Lands),
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
    fn of(word: HarnessWord, task: TaskFace, lands: Option<Lands>) -> Option<Face> {
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
        Self::read_in(shared, word, &chrono::Local, chrono::Utc::now())
    }

    /// [`Self::read`], the timer's clock in `zone` as of `now` ([`clock_in`]).
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
            .and_then(|at| clock_in(at, zone, now));
        Face::of(word, task, lands)
    }

    /// The glyph: `∞`, `⤓`, `✓` or `○`, one cell each.
    fn glyph(&self) -> &'static str {
        match self {
            Face::Cruise | Face::CruiseUntil(_) => "∞",
            Face::Land | Face::Theirs => "⤓",
            Face::Landed => "✓",
            Face::Off => "○",
            Face::HarnessOff | Face::AgentOff | Face::Unhosted => "⊘",
        }
    }

    /// The whole face after its glyph: the word a space after it, and a timed Cruise's clock
    /// right after its `⤓`. Cruise has no word, as in aterm: the reversed `[Cruise]` says it.
    fn rest(&self) -> String {
        match self {
            Face::Cruise => String::new(),
            Face::CruiseUntil(lands) => format!(" ⤓{}", lands.clock),
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
    fn line(&self, whole: bool, inks: FaceInks) -> Line<'static> {
        let (glyph, rest) = self.tones(inks);
        let mut spans = vec![Span::styled(self.glyph(), glyph)];
        if whole {
            spans.push(Span::styled(self.rest(), rest));
        }
        Line::from(spans)
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

    /// What a click does, in words (aterm's `ChipFace::click`); `None` where it selects no
    /// mode ([`Self::hit`]).
    fn click_words(&self) -> Option<&'static str> {
        Some(match self {
            Face::Cruise => "land",
            Face::CruiseUntil(_) => "land now",
            Face::Land | Face::Landed | Face::Theirs => "off",
            Face::Off => "cruise",
            Face::HarnessOff | Face::AgentOff | Face::Unhosted => return None,
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

    /// The face in words (aterm's `ChipFace::spoken`).
    fn spoken(&self) -> String {
        match self {
            Face::Cruise => "Autopilot: cruise.".to_string(),
            Face::CruiseUntil(Lands {
                clock,
                day: Some(day),
            }) => format!(
                "Autopilot: cruise. Lands {} at {clock}.",
                weekday_long(*day)
            ),
            Face::CruiseUntil(Lands { clock, day: None }) => {
                format!("Autopilot: cruise. Lands at {clock}.")
            }
            Face::Land => "Autopilot: land.".to_string(),
            Face::Landed => "Autopilot: land. Landed.".to_string(),
            Face::Theirs => "Autopilot: land. Quiet: a person is driving the session.".to_string(),
            Face::Off => "Autopilot: off.".to_string(),
            Face::HarnessOff => "Autopilot: off. The harness is off in Settings.".to_string(),
            Face::AgentOff => "Autopilot: disabled for codex-alab in aterm's Settings.".to_string(),
            Face::Unhosted => "Autopilot: off. This aterm has no control socket.".to_string(),
        }
    }
}

/// The session's Autopilot in words (aterm's `ChipFace::spoken`) and what a click on the
/// line's face does: what `/alab autopilot` says, where the narrow line's face is its glyph
/// alone and none is drawn narrower still.
pub fn autopilot_words() -> String {
    let face =
        crate::boxes::tick_shared().and_then(|shared| Face::read(shared, shared.harness_word()));
    match face {
        Some(face) => match face.click_words() {
            Some(click) => format!(
                "{}\nClick: {click}. Right-click: Autopilot controls.",
                face.spoken()
            ),
            None => format!(
                "{}\n{} Right-click: Autopilot controls.",
                face.spoken(),
                face.settings_words()
            ),
        },
        None => "Autopilot: no mode from aterm (codex-alab does not supervise this session)."
            .to_string(),
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

/// `/alab cruise until=<when>`: aterm's timed Cruise, cruising until `when` (aterm's
/// `HH:MM|+DURATION`, which aterm reads and refuses), then landing. A plain Cruise drops it.
pub fn set_cruise_until(
    when: String,
) -> impl std::future::Future<Output = Result<(), String>> + Send {
    let words = if !when.is_empty() && when.bytes().all(|b| b.is_ascii_graphic()) {
        Ok(format!("cruise until={when}"))
    } else {
        Err("Use a time such as 18:00 or a duration such as +2h".to_string())
    };
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
    let state = state();
    let generation = state.map(|state| state.mode_latest.fetch_add(1, Ordering::SeqCst) + 1);
    let route = route();
    let on = move || state.is_some_and(|state| state.choice.load(Ordering::Acquire) == ON);
    async move {
        let (Some(state), Some(generation)) = (state, generation) else {
            return Err("ALab enhancements are off".into());
        };
        let _write = state.mode_write.lock().await;
        if state.mode_latest.load(Ordering::SeqCst) != generation {
            return Ok(());
        }
        if !on() {
            return Err("ALab enhancements are off".into());
        }
        let words = words?;
        let client = self_client(route).await?;
        if !on() {
            return Err("ALab enhancements are off".into());
        }
        let reply = client
            .request(&format!(
                "@{} meta set autopilot {words}",
                client.endpoint().sid
            ))
            .await
            .map_err(|error| error.to_string())?;
        if reply.ok().is_none() {
            return Err(reply.status);
        }
        Ok(())
    }
}

fn render_border(area: Rect, buf: &mut Buffer, on: bool, button: Rect, depth: Depth, theme: Theme) {
    let palette = Palette::new(depth, theme);
    if on {
        let start = area.x + 2;
        let available = usize::from(button.x.saturating_sub(start + 1));
        Line::from("─".repeat(usize::from(area.width.saturating_sub(4)))).render(
            Rect::new(start, area.y, area.width.saturating_sub(4), 1),
            buf,
        );
        let alab = codex_alab_core::version::alab();
        let upstream = codex_alab_core::version::upstream();
        let label = current_brand_label(available, alab, &upstream);
        if let Some(label) = label {
            note_build_button(start + 1, area.y, &label);
            Line::from(vec![
                Span::raw(" "),
                Span::styled(label, palette.brand),
                Span::raw(" "),
            ])
            .render(Rect::new(start, area.y, available as u16, 1), buf);
        }
    }
    Line::styled(button_label(area.width, on), palette.button).render(button, buf);
    if on {
        crate::banner::tint_composer_rule(
            Rect::new(area.x + 2, area.y, area.width.saturating_sub(4), 1),
            buf,
            depth,
            theme,
        );
    }
}

/// Primary UI versions retain their release/build number; exact source metadata belongs
/// in `/alab version`. This formatter never changes stored or compared build identities.
pub(crate) fn readable_version(version: &str) -> &str {
    version
        .split_once('+')
        .map_or(version, |(version, _)| version)
}

/// The native version label opens build and update details without a host overlay.
pub fn build_mouse_hit(column: u16, row: u16) -> bool {
    enabled()
        && state()
            .and_then(|s| s.build_button.lock().ok())
            .is_some_and(|hit| {
                hit.is_some_and(|r| row == r.y && column >= r.x && column < r.right())
            })
}

fn note_build_button(x: u16, y: u16, label: &str) {
    if let Some(state) = state()
        && let Ok(mut hit) = state.build_button.lock()
    {
        *hit = Some(Rect::new(x, y, UnicodeWidthStr::width(label) as u16, 1));
    }
}

fn current_brand_label(available: usize, alab: &str, upstream: &str) -> Option<String> {
    brand_label_with_update(available, alab, upstream, crate::upgrade::pending())
}

fn brand_label_with_update(
    available: usize,
    alab: &str,
    upstream: &str,
    pending: bool,
) -> Option<String> {
    if pending {
        let badge = " ↑ update";
        if let Some(label) = brand_label(
            available.saturating_sub(UnicodeWidthStr::width(badge)),
            alab,
            upstream,
        ) {
            return Some(format!("{label}{badge}"));
        }
    }
    brand_label(available, alab, upstream)
}

fn brand_label(available: usize, alab: &str, upstream: &str) -> Option<String> {
    let alab = readable_version(alab);
    let upstream = readable_version(upstream);
    [
        format!("codex-alab {alab} · based on OpenAI Codex {upstream}"),
        format!("codex-alab {alab} · based on Codex {upstream}"),
        format!("ALab {alab} · based on Codex {upstream}"),
        format!("codex-alab · based on Codex {upstream}"),
        "codex-alab".to_string(),
    ]
    .into_iter()
    .find(|label| UnicodeWidthStr::width(label.as_str()) + 2 <= available)
}

/// Identify the fork and its upstream source separately in launch and status cards.
pub fn session_title() -> Option<Vec<Span<'static>>> {
    enabled().then(|| {
        vec![
            Span::styled("codex-alab", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!(" v{}", readable_version(codex_alab_core::version::alab())),
                Style::default().add_modifier(Modifier::DIM),
            ),
            Span::raw(" · based on OpenAI Codex "),
            Span::styled(
                codex_alab_core::version::upstream_release()
                    .unwrap_or("unreleased")
                    .to_owned(),
                Style::default().add_modifier(Modifier::DIM),
            ),
        ]
    })
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

#[derive(Clone, Debug, PartialEq)]
pub struct InterfaceStatus {
    /// An email address, API-key label, or explicit account-unavailable text.
    pub account: String,
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
    /// Earned reset credits, kept separate from the weekly automatic reset.
    pub reset_credits: Option<ResetCredits>,
}

impl Default for InterfaceStatus {
    fn default() -> Self {
        Self {
            account: "Account unavailable".to_string(),
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
}

struct PaintedAccount {
    cluster: AccountCluster,
    rect: Option<Rect>,
}

fn clear_switch_hits() {
    if let Some(state) = state()
        && let Ok(mut button) = state.last_button.lock()
    {
        *button = None;
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
            .filter(|account| UnicodeWidthStr::width(account.as_str()) > 0)
            .unwrap_or_else(|| "Account unavailable".to_string());
        if current.as_ref().map(|old| AccountCluster::of(old.values()))
            != Some(AccountCluster::of(snapshot.values()))
            || current.as_ref().is_some_and(|old| {
                old.quota != snapshot.quota || old.reset_credits != snapshot.reset_credits
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
    let mut snapshot = state()?.status.lock().ok()?.clone()?;
    snapshot.activity = match connection_state() {
        ConnectionState::Connected => snapshot.activity,
        ConnectionState::Reconnecting => Some("Reconnecting".to_string()),
        ConnectionState::Offline => Some("Offline".to_string()),
    };
    Some(snapshot)
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
        Line::styled(label, Palette::new(depth, theme).accent)
            .render(Rect::new(area.x, area.y, area.width, 1), buf);
        area.y = area.y.saturating_add(1);
        area.height = area.height.saturating_sub(1);
    }
    if area.is_empty() {
        clear_account_hits();
        return;
    }
    let Some(snapshot) = status_snapshot() else {
        clear_account_hits();
        return;
    };
    let weekly = weekly_footer(&snapshot, usize::from(area.width.saturating_sub(28)));
    let mut values = snapshot.values();
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
        painted.cluster = AccountCluster::of(snapshot.values());
    }
    if let Some(credits) = snapshot
        .reset_credits
        .as_ref()
        .filter(|credits| !credits.stale && credits.expiring_count > 0)
    {
        let Some(area) = weekly_area else { return };
        let mut highlighted = 0;
        for x in area.x..area.right() {
            let cell = &mut buf[(x, area.y)];
            if cell.symbol() != "●" {
                continue;
            }
            if highlighted < credits.expiring_count {
                cell.modifier.insert(Modifier::BOLD);
                if depth != Depth::Mono {
                    cell.set_fg(if theme.is_light() {
                        Color::Yellow
                    } else {
                        Color::LightYellow
                    });
                }
            } else {
                cell.modifier.remove(Modifier::BOLD);
            }
            highlighted += 1;
        }
    }
}

fn weekly_footer(status: &InterfaceStatus, width: usize) -> Option<String> {
    let quota = status.quota.as_ref().filter(|_| !status.usage_stale);
    if quota.is_none() && status.reset_credits.is_none() {
        return None;
    }
    let mut text = if let Some(quota) = quota {
        let used = quota.used_percent.clamp(0.0, 100.0);
        let filled = (used * 8.0 / 100.0).round() as usize;
        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(8 - filled));
        let mut text = format!("weekly [{bar}] {used:.0}% used");
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
    if let Some(credits) = status.reset_credits.as_ref() {
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
                let label = if credits.details_known {
                    "expires soon"
                } else {
                    "known expiring"
                };
                text.push_str(&format!(" ({} {label})", credits.expiring_count));
            }
        }
    }
    let Some(quota) = quota else {
        return Some(text);
    };
    let mut trend = text.clone();
    if quota.sample_count >= 2 {
        trend.push_str(&format!(" · {}", quota.sparkline(10)));
    }
    if let Some(projection) = quota.projection.as_ref() {
        let projected = format!(
            "{trend} · est {:.0}% at reset",
            projection.projected_percent_at_reset
        );
        if UnicodeWidthStr::width(projected.as_str()) <= width {
            return Some(projected);
        }
    }
    if UnicodeWidthStr::width(trend.as_str()) <= width {
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

fn fitted(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
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
        let columns = UnicodeWidthStr::width(grapheme.symbol);
        if used + columns > width - 1 {
            break;
        }
        result.push_str(grapheme.symbol);
        used += columns;
    }
    result.push('…');
    result
}

fn fields_width(fields: &[(Field, String)]) -> usize {
    fields
        .iter()
        .map(|(_, text)| UnicodeWidthStr::width(text.as_str()))
        .sum::<usize>()
        + fields
            .windows(2)
            .map(|pair| UnicodeWidthStr::width(join(pair[0].0, pair[1].0)))
            .sum::<usize>()
}

/// The account unit shrunk as one into `width`, once the workspace fields have gone: the
/// five-hour allowance goes, the account name shortens (to [`ACCOUNT_NAME_FLOOR`]) beside
/// the plan, the plan goes and the name takes its room, then the name goes. The weekly
/// allowance goes last: only the part left alone is cut to the width.
fn shrink_account(fields: &mut Vec<(Field, String)>, width: usize, name: Option<&str>) {
    let fits = |fields: &[(Field, String)]| fields_width(fields) <= width;
    let drop_part = |fields: &mut Vec<(Field, String)>, part: Field| {
        if fields.len() > 1 {
            fields.retain(|(field, _)| *field != part);
        }
    };
    let shorten_name = |fields: &mut Vec<(Field, String)>| {
        let (Some(name), Some(at)) = (
            name,
            fields.iter().position(|(field, _)| *field == Field::Email),
        ) else {
            return;
        };
        let others = fields_width(fields) - UnicodeWidthStr::width(fields[at].1.as_str());
        let room = width.saturating_sub(others);
        if room >= ACCOUNT_NAME_FLOOR.min(UnicodeWidthStr::width(name)) {
            fields[at].1 = fitted(name, room);
        }
    };
    for step in 0..5 {
        if fits(fields) {
            return;
        }
        match step {
            0 => drop_part(fields, Field::FiveHour),
            1 => shorten_name(fields),
            2 => {
                drop_part(fields, Field::Plan);
                shorten_name(fields);
            }
            3 => drop_part(fields, Field::Email),
            _ => {
                if let [(_, value)] = fields.as_mut_slice() {
                    *value = fitted(value, width);
                }
            }
        }
    }
}

/// Build a bounded single status line. The account name, plan and allowances are one unit,
/// joined by `/` and set apart from the workspace fields by `│`. Context and the workspace
/// fields drop first; then the unit shrinks as one ([`shrink_account`]), keeping the weekly
/// allowance last. This is pure rendering; callers can measure it without changing live
/// interface state.
pub fn status_line(width: u16, status: Status<'_>, depth: Depth, theme: Theme) -> Line<'static> {
    status_layout(width, status, depth, theme).line
}

struct StatusLayout {
    line: Line<'static>,
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
            line: Line::default(),
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
    .filter_map(|(field, value)| display_text(value).map(|value| (field, value)))
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
        fields.retain(|(field, _)| *field != optional);
    }
    let name = display_text(status.email);
    shrink_account(&mut fields, width, name.as_deref());
    let palette = Palette::new(depth, theme);
    let mut spans = Vec::new();
    let mut account: Option<(usize, usize)> = None;
    let mut weekly_range = None;
    let mut column = 0;
    let mut previous = None;
    for (field, value) in fields {
        if let Some(previous) = previous {
            let join = join(previous, field);
            spans.push(Span::styled(join, palette.muted));
            column += UnicodeWidthStr::width(join);
        }
        previous = Some(field);
        let field_width = UnicodeWidthStr::width(value.as_str());
        if field == Field::Weekly {
            weekly_range = Some((column as u16, field_width as u16));
        }
        if field_width > 0 && field.in_account() {
            let start = account.map_or(column, |(start, _)| start);
            account = Some((start, column + field_width));
        }
        column += field_width;
        let style = match field {
            Field::Email => palette.brand,
            Field::Weekly => palette.accent,
            _ => palette.muted,
        };
        spans.push(Span::styled(value, style));
    }
    StatusLayout {
        line: Line::from(spans),
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
    layout.line.render(row, buf);
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
