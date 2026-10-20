// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! codex-alab's turn as aterm's progress bar (`ALAB(progress)`): ConEmu's OSC 9;4.
//!
//! aterm takes `ESC ] 9 ; 4 ; <state> [; <pct>] ST` ungated (`aterm-core`'s
//! `handler_osc_notify.rs`, `Terminal::taskbar_progress`). On macOS it draws no bar; its pet
//! watches it: a change of a value bar (`1` or `2`) while the command runs turns the rainbow
//! kitty to look (`PetAttention::Progress`); `3` (indeterminate) and `4` (paused) are quiet
//! levels; `0` hides the bar. aterm's Windows build mirrors it onto the taskbar button. RIS
//! clears it, and aterm's checkpoint carries it across a restore, so a bar left standing
//! outlives the process: every exit takes it down.
//!
//! What is written, each chat frame right after `pre_draw_tick`'s OSC 7501 report and before
//! the frame's draw (`glue/session_observe.rs`), on the same `stdout` through crossterm's
//! `execute!`, and only when it differs from what this process last wrote:
//!
//! | the displayed thread | bytes |
//! | --- | --- |
//! | working (or blocked on a box) with no plan this turn | `ESC ] 9 ; 4 ; 3 ST` |
//! | its turn running with this turn's `update_plan` checklist, `done` of `total` steps | `ESC ] 9 ; 4 ; 1 ; <done*100/total> ST` |
//! | the turn ended, but the thread still works (a background terminal, an active goal between turns) | `ESC ] 9 ; 4 ; 3 ST` |
//! | the turn failed | `ESC ] 9 ; 4 ; 2 ; <the plan's pct, or 100 with no plan or no step done> ST` |
//! | idle, or the turn ended | `ESC ] 9 ; 4 ; 0 ST` |
//!
//! The plan fill needs a plan: upstream made the `update_plan` tool opt-in (a9519cbcd), so
//! only with `[tools.update_plan] enabled = true` does a turn send one. Without it a turn is
//! indeterminate throughout, and the one value bar, the one the pet turns to, is a failed
//! turn's red one.
//!
//! The first frame writes its bar even at rest, so a bar a killed predecessor left in the tab
//! goes. Wherever the TUI takes its OSC 7501 report down (the exit, a panic, Ctrl+Z, an
//! external program; one `ALAB(progress)` line in `tui/src/terminal_program_status.rs`), a
//! bar that shows goes with it ([`clear`]), and the next frame puts the current one back.
//!
//! Only in the codex binary ([`codex_alab_core::active`]), only inside aterm
//! (`TERM_PROGRAM=aterm`, [`codex_alab_core::inside_aterm`]) with no multiplexer between
//! (tmux, zellij, screen's `STY`: their terminal is not aterm, and zellij and screen pass
//! `TERM_PROGRAM` on; upstream's OSC 21337 gate), only to a terminal, and only while
//! `[tui] alab_progress` is `true`:
//! `alab-core/aterm-defaults.toml` sets it inside aterm, and a person's `alab_progress = false`
//! (`~/.codex/config.toml`, or `-c tui.alab_progress=false`) turns it off. Anything else
//! writes nothing, ever. codex-alab's `--strict-config` accepts the key (the `ALAB(progress)`
//! line in `config/src/strict_config.rs`, [`codex_alab_core::own_tui_key`]); vendor codex's
//! refuses it, and an editor checking upstream's schema flags it (`Tui` denies unknown
//! fields), so it does not belong in a config a vendor codex reads strictly, such as a
//! checked-in project `.codex/config.toml`.

use codex_alab_core::session::ProgramState;
use ratatui::crossterm::Command;
use ratatui::crossterm::execute;
use std::fmt;
use std::io;
use std::io::IsTerminal;
use std::sync::Mutex;
use std::sync::OnceLock;

/// `[tui] alab_progress`: the bar's switch (ALab's own key: upstream's `Tui` has no such
/// field and ignores it). `true` in `alab-core/aterm-defaults.toml`.
pub const SETTING: &str = codex_alab_core::PROGRESS_KEY;

/// One OSC 9;4 state, as codex-alab writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bar {
    /// `9;4;0`: no bar (rest, a turn's end, the exit).
    Hidden,
    /// `9;4;3`: a turn works with no plan.
    Indeterminate,
    /// `9;4;1;<pct>`: this turn's plan, `pct` of its steps completed.
    Plan(u8),
    /// `9;4;2;<pct>`: the turn failed (never an empty red bar: 1 to 100).
    Error(u8),
}

impl Bar {
    /// The bar for a frame: the displayed thread's state and this turn's plan as
    /// (completed, total) steps.
    pub fn of(state: ProgramState, plan: Option<(usize, usize)>) -> Self {
        let pct = plan.and_then(|(done, total)| percent(done, total));
        match state {
            ProgramState::Working | ProgramState::Blocked(_) => {
                pct.map_or(Self::Indeterminate, Self::Plan)
            }
            // Where the plan stood, but never empty: a value renderer (aterm's Windows taskbar)
            // draws a red bar at 0 as no bar at all.
            ProgramState::Error => Self::Error(pct.filter(|pct| *pct > 0).unwrap_or(100)),
            ProgramState::Unknown | ProgramState::Idle | ProgramState::Done => Self::Hidden,
        }
    }

    /// The exact bytes: `ESC ] 9 ; 4 ; <state> [; <pct>] ESC \`.
    pub fn sequence(self) -> String {
        match self {
            Self::Hidden => "\x1b]9;4;0\x1b\\".to_owned(),
            Self::Indeterminate => "\x1b]9;4;3\x1b\\".to_owned(),
            Self::Plan(pct) => format!("\x1b]9;4;1;{pct}\x1b\\"),
            Self::Error(pct) => format!("\x1b]9;4;2;{pct}\x1b\\"),
        }
    }
}

/// `done` of `total` steps as a whole percent, rounded down (100 only when every step is
/// done); `None` for an empty plan.
pub fn percent(done: usize, total: usize) -> Option<u8> {
    if total == 0 {
        return None;
    }
    // In u128, `done * 100` cannot overflow.
    let pct = done.min(total) as u128 * 100 / total as u128;
    Some(u8::try_from(pct).unwrap_or(100))
}

/// Whether `[tui] alab_progress` is on, given the config layers from highest precedence to
/// lowest (`ConfigLayerStack::layers_high_to_low`): the first layer that sets the key decides,
/// and only a boolean `true` turns the bar on (absent, or any other value, is off).
pub fn setting<'a>(layers: impl IntoIterator<Item = &'a toml::Value>) -> bool {
    layers
        .into_iter()
        .find_map(|layer| layer.get("tui")?.get(SETTING))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false)
}

/// What this process last wrote: `None` before its first write. The write and this memory
/// change together under one lock, so the exit's [`clear`] (a panic hook's included) never
/// interleaves with a frame's write.
pub struct Emitter {
    shown: Mutex<Option<Bar>>,
}

impl Default for Emitter {
    fn default() -> Self {
        Self::new()
    }
}

impl Emitter {
    pub const fn new() -> Self {
        Self {
            shown: Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Bar>> {
        self.shown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// What this process last wrote, if anything.
    pub fn shown(&self) -> Option<Bar> {
        *self.lock()
    }

    /// One frame's bar: written only when it differs from the last one written (the first
    /// frame's always). A failed write is not remembered, so the next frame tries again.
    pub fn frame(&self, bar: Bar, write: impl FnOnce(Bar) -> io::Result<()>) -> io::Result<()> {
        let mut shown = self.lock();
        if *shown == Some(bar) {
            return Ok(());
        }
        write(bar)?;
        *shown = Some(bar);
        Ok(())
    }

    /// The bar goes, if one shows: `9;4;0` once; nothing when none was ever written or the
    /// last write already hid it.
    pub fn clear(&self, write: impl FnOnce(Bar) -> io::Result<()>) -> io::Result<()> {
        let mut shown = self.lock();
        if matches!(*shown, None | Some(Bar::Hidden)) {
            return Ok(());
        }
        write(Bar::Hidden)?;
        *shown = Some(Bar::Hidden);
        Ok(())
    }

    /// One frame where the bar is allowed (`enabled`: codex-alab, inside aterm, to a
    /// terminal, the setting on) or not: when it is not, a bar this process put up still goes,
    /// and nothing else is written.
    pub fn observe(
        &self,
        enabled: bool,
        bar: impl FnOnce() -> Bar,
        write: impl FnOnce(Bar) -> io::Result<()>,
    ) -> io::Result<()> {
        if enabled {
            self.frame(bar(), write)
        } else {
            self.clear(write)
        }
    }
}

static EMITTER: Emitter = Emitter::new();

/// Whether the bar may show: the codex-alab binary (`active`), inside aterm (`inside_aterm`)
/// with no multiplexer between (`multiplexed`: tmux, zellij or screen, whose own terminal is
/// not aterm), writing to a terminal (`to_terminal`, as OSC 7501 is written only to one), and
/// `[tui] alab_progress` on in `layers` (highest precedence first). The setting is read only
/// when the rest holds.
pub fn enabled<'a>(
    active: bool,
    inside_aterm: bool,
    multiplexed: bool,
    to_terminal: bool,
    layers: impl IntoIterator<Item = &'a toml::Value>,
) -> bool {
    active && inside_aterm && !multiplexed && to_terminal && setting(layers)
}

/// Whether a terminal multiplexer stands between codex and the terminal: one upstream's
/// detection names (tmux, zellij), or GNU screen (`STY`), as upstream's OSC 21337 gate asks.
fn under_multiplexer() -> bool {
    codex_terminal_detection::terminal_info()
        .multiplexer
        .is_some()
        || std::env::var_os("STY").is_some()
}

/// ALAB(progress) glue, each chat frame right after the OSC 7501 report: this frame's bar,
/// written to the terminal if it changed. `layers` are the displayed thread's config layers,
/// highest precedence first.
pub fn observe<'a>(
    state: ProgramState,
    plan: Option<(usize, usize)>,
    layers: impl IntoIterator<Item = &'a toml::Value>,
) {
    static PLACE: OnceLock<(bool, bool, bool)> = OnceLock::new();
    if !codex_alab_core::active() {
        return;
    }
    let (inside_aterm, multiplexed, to_terminal) = *PLACE.get_or_init(|| {
        (
            codex_alab_core::inside_aterm(),
            under_multiplexer(),
            io::stdout().is_terminal(),
        )
    });
    let enabled = enabled(
        /*active*/ true,
        inside_aterm,
        multiplexed,
        to_terminal,
        layers,
    );
    let _ = EMITTER.observe(enabled, || Bar::of(state, plan), write_to_terminal);
}

/// ALAB(progress) hook, wherever the TUI takes its OSC 7501 report down (the exit, a panic,
/// Ctrl+Z, an external program): a bar this process put up goes. Inert otherwise.
pub fn clear() {
    let _ = EMITTER.clear(write_to_terminal);
}

/// The TUI's own path for OSC 7501: crossterm's `execute!` to `stdout`, flushed at once.
fn write_to_terminal(bar: Bar) -> io::Result<()> {
    execute!(io::stdout(), SetProgress(bar))
}

/// [`Bar`] as a crossterm command.
#[derive(Clone, Copy, Debug)]
struct SetProgress(Bar);

impl Command for SetProgress {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        f.write_str(&self.0.sequence())
    }
}
