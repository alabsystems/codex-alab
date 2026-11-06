// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(pet-perf): decorative motion follows the terminal's focus.
//!
//! codex paints a frame for every tick of its animations, focused or not: the ambient pet's
//! idle loop about once a second forever, during a turn the status row's shimmer every 32 ms,
//! and the loading spinner every 100 ms. A tab in the background or a window behind another
//! spends that CPU and those terminal bytes on frames nobody looks at. Inside aterm, which
//! reports focus in and out (`ESC [ I` / `ESC [ O`, mode 1004, which codex turns on),
//! codex-alab stops them while the terminal says focus is out: the pet holds its frame
//! ([`decorating`], the `ALAB(pet-perf)` hook in `tui/src/pets/ambient.rs`), and the status
//! row and the spinner tick with the row's 1 s elapsed-time clock ([`pace_ms`] in
//! `tui/src/status_indicator_widget.rs`, [`pace`] in `tui/src/motion.rs`). Focus in puts all
//! three back at once: it is a draw, and the draw schedules the next tick. Nothing else waits:
//! output, keys and every timer of real work schedule their own frames.
//!
//! The terminal's word is the only input: the two `ALAB(pet-perf)` lines in
//! `tui/src/tui/event_stream.rs` call [`focus`]. A terminal that never reports focus leaves
//! codex focused, as upstream assumes. Only in the codex binary inside aterm, whose focus
//! reports are a tab's real focus; elsewhere a terminal or multiplexer that reports focus out
//! and never in would freeze the pet for good.
//!
//! The periodic usage read follows the focus too ([`usage_poll`], the `ALAB(usage-poll)` hook in
//! `tui/src/chatwidget/rate_limits.rs`), slowing resting reads to five minutes while focus is
//! out. An active quota or subscription recovery keeps the existing poll at fifteen seconds
//! or faster, including in a background tab; it does not start a separate watcher.

use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_alab_core::active;

/// The slowest clock a row on screen still needs while focus is out: the status row's
/// elapsed seconds.
pub const UNFOCUSED_TICK: Duration = Duration::from_secs(1);

static UNFOCUSED: AtomicBool = AtomicBool::new(false);

/// The terminal reported focus in (`true`) or out.
pub fn focus(focused: bool) {
    UNFOCUSED.store(!focused, Ordering::Relaxed);
}

/// Whether the terminal last said focus is in (or said nothing).
pub fn focused() -> bool {
    !UNFOCUSED.load(Ordering::Relaxed)
}

fn on() -> bool {
    static IN_ATERM: OnceLock<bool> = OnceLock::new();
    active() && *IN_ATERM.get_or_init(codex_alab_core::inside_aterm)
}

/// Whether a decorative animation (the ambient pet) schedules its next frame.
pub fn decorating() -> bool {
    decorating_when(on(), focused())
}

fn decorating_when(on: bool, focused: bool) -> bool {
    !on || focused
}

/// The delay until a decorative tick that a clock on screen shares (the status row's
/// shimmer and its elapsed time): unchanged while focused, at least [`UNFOCUSED_TICK`] while not.
pub fn pace(delay: Duration) -> Duration {
    pace_when(on(), focused(), delay)
}

/// [`pace`] in milliseconds.
pub fn pace_ms(delay_ms: u64) -> u64 {
    u64::try_from(pace(Duration::from_millis(delay_ms)).as_millis()).unwrap_or(delay_ms)
}

fn pace_when(on: bool, focused: bool, delay: Duration) -> Duration {
    if decorating_when(on, focused) {
        delay
    } else {
        delay.max(UNFOCUSED_TICK)
    }
}

/// Upstream's usage poll below 75 % used, in every tab, focused or not
/// (`ChatWidget::rate_limit_refresh_interval`); it polls faster nearer the limit.
pub const RESTING_USAGE_POLL: Duration = Duration::from_secs(60);

/// The usage poll while the terminal's focus is out: the quota history's five-minute bucket.
pub const UNFOCUSED_USAGE_POLL: Duration = Duration::from_secs(5 * 60);

/// A suspended native task checks for restored account access without waiting
/// for the ordinary background-tab interval or sending a model request.
pub const RECOVERY_USAGE_POLL: Duration = Duration::from_secs(15);

/// ALAB(usage-poll): the delay between the TUI's periodic usage reads. Each read is an HTTPS
/// request on a new connection, and upstream makes one a minute in every tab. aterm reports
/// focus in to one pane on the whole machine (the front pane of the window that has the
/// system's focus), so most tabs read as unfocused; a pane in a visible split, or a window
/// beside another app, does too, and its status line then shows a reading up to
/// [`UNFOCUSED_USAGE_POLL`] old. While focus is out, upstream's resting pace stretches to that.
/// The faster paces near the limit, and a read held for a usage limit's recovery (which an
/// accepted read releases), keep upstream's; the pace follows the last full read, so an
/// unfocused tab sees usage cross 75 % up to five minutes late (a hard stop still reads at
/// once, through the turn's error). Focus in is an event that recomputes the poll's deadline,
/// so a reading older than the resting pace is read at once.
pub fn usage_poll(interval: Duration, recovery_held: bool) -> Duration {
    let on = on();
    let recovering = on && {
        let shared = crate::session::shared();
        shared.autopilot() == codex_alab_core::session::Autopilot::Active
            && shared.recovery().is_some_and(|recovery| {
                matches!(
                    recovery.wall,
                    codex_alab_core::session::RecoveryWall::Usage { .. }
                        | codex_alab_core::session::RecoveryWall::Entitlement { .. }
                )
            })
    };
    recovery_poll_when(on, focused(), interval, recovery_held, recovering)
}

fn recovery_poll_when(
    on: bool,
    focused: bool,
    interval: Duration,
    recovery_held: bool,
    recovering: bool,
) -> Duration {
    if on && recovering {
        interval.min(RECOVERY_USAGE_POLL)
    } else {
        usage_poll_when(on, focused, interval, recovery_held)
    }
}

fn usage_poll_when(on: bool, focused: bool, interval: Duration, recovery_held: bool) -> Duration {
    if decorating_when(on, focused) || recovery_held || interval < RESTING_USAGE_POLL {
        interval
    } else {
        interval.max(UNFOCUSED_USAGE_POLL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const SHIMMER: Duration = Duration::from_millis(32);

    #[test]
    fn native_recovery_stays_responsive_without_accelerating_resting_tabs() {
        for focused in [false, true] {
            assert_eq!(
                recovery_poll_when(true, focused, RESTING_USAGE_POLL, false, true),
                RECOVERY_USAGE_POLL
            );
            assert_eq!(
                recovery_poll_when(true, focused, Duration::from_secs(5), false, true),
                Duration::from_secs(5)
            );
        }
        assert_eq!(
            recovery_poll_when(true, false, RESTING_USAGE_POLL, false, false),
            UNFOCUSED_USAGE_POLL
        );
        assert_eq!(
            recovery_poll_when(false, false, RESTING_USAGE_POLL, false, true),
            RESTING_USAGE_POLL
        );
    }

    #[test]
    fn focus_out_stops_the_animation_timer() {
        // The pet schedules no next frame; the shimmer waits for the elapsed-time clock.
        assert!(!decorating_when(/*on*/ true, /*focused*/ false));
        assert_eq!(
            pace_when(/*on*/ true, /*focused*/ false, SHIMMER),
            UNFOCUSED_TICK
        );
        let slow = Duration::from_secs(5);
        assert_eq!(pace_when(/*on*/ true, /*focused*/ false, slow), slow);
    }

    #[test]
    fn focus_in_and_upstream_keep_every_tick() {
        assert!(decorating_when(/*on*/ true, /*focused*/ true));
        assert_eq!(pace_when(/*on*/ true, /*focused*/ true, SHIMMER), SHIMMER);
        for focused in [true, false] {
            assert!(decorating_when(/*on*/ false, focused));
            assert_eq!(pace_when(/*on*/ false, focused, SHIMMER), SHIMMER);
        }
    }

    #[test]
    fn an_unfocused_tab_reads_usage_every_five_minutes_at_rest() {
        assert_eq!(
            usage_poll_when(true, /*focused*/ false, RESTING_USAGE_POLL, false),
            UNFOCUSED_USAGE_POLL
        );
        // Focused, outside aterm, near the limit, or holding a recovery: upstream's pace.
        assert_eq!(
            usage_poll_when(true, /*focused*/ true, RESTING_USAGE_POLL, false),
            RESTING_USAGE_POLL
        );
        assert_eq!(
            usage_poll_when(/*on*/ false, false, RESTING_USAGE_POLL, false),
            RESTING_USAGE_POLL
        );
        for near in [5, 15, 30].map(Duration::from_secs) {
            assert_eq!(usage_poll_when(true, false, near, false), near);
        }
        assert_eq!(
            usage_poll_when(true, false, RESTING_USAGE_POLL, /*recovery_held*/ true),
            RESTING_USAGE_POLL
        );
    }

    #[test]
    fn the_terminals_last_report_is_the_focus() {
        assert!(focused(), "no report: focused, as upstream assumes");
        focus(/*focused*/ false);
        assert!(!focused());
        focus(/*focused*/ true);
        assert!(focused());
    }
}
