// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The OSC 9;4 bar (`ALAB(progress)`): the bytes for each state, a plan's fill, the dedupe,
//! the gate and the clear every exit path takes.

use crate::progress::Bar;
use crate::progress::Emitter;
use crate::progress::enabled;
use crate::progress::percent;
use crate::progress::setting;
use codex_alab_core::session::BlockedKind;
use codex_alab_core::session::ProgramState;
use pretty_assertions::assert_eq;
use std::cell::RefCell;

/// An emitter and the bytes it wrote, frame by frame.
struct Tab {
    emitter: Emitter,
    bytes: RefCell<String>,
}

impl Tab {
    fn new() -> Self {
        Self {
            emitter: Emitter::new(),
            bytes: RefCell::new(String::new()),
        }
    }

    fn write(&self, bar: Bar) -> std::io::Result<()> {
        self.bytes.borrow_mut().push_str(&bar.sequence());
        Ok(())
    }

    /// One chat frame with the bar allowed.
    fn frame(&self, state: ProgramState, plan: Option<(usize, usize)>) {
        self.observe(/*enabled*/ true, state, plan);
    }

    fn observe(&self, enabled: bool, state: ProgramState, plan: Option<(usize, usize)>) {
        let result = self
            .emitter
            .observe(enabled, || Bar::of(state, plan), |bar| self.write(bar));
        assert!(result.is_ok(), "{result:?}");
    }

    /// The exit (or a panic, Ctrl+Z, an external program).
    fn clear(&self) {
        let result = self.emitter.clear(|bar| self.write(bar));
        assert!(result.is_ok(), "{result:?}");
    }

    /// The bytes written since the last take.
    fn take(&self) -> String {
        std::mem::take(&mut *self.bytes.borrow_mut())
    }
}

const HIDE: &str = "\x1b]9;4;0\x1b\\";
const WORKING: &str = "\x1b]9;4;3\x1b\\";

fn plan(pct: u8) -> String {
    format!("\x1b]9;4;1;{pct}\x1b\\")
}

fn error(pct: u8) -> String {
    format!("\x1b]9;4;2;{pct}\x1b\\")
}

#[test]
fn each_state_is_conemus_osc_9_4_with_st() {
    assert_eq!(Bar::Hidden.sequence(), "\x1b]9;4;0\x1b\\");
    assert_eq!(Bar::Indeterminate.sequence(), "\x1b]9;4;3\x1b\\");
    assert_eq!(Bar::Plan(42).sequence(), "\x1b]9;4;1;42\x1b\\");
    assert_eq!(Bar::Error(100).sequence(), "\x1b]9;4;2;100\x1b\\");
    // Negative control: a plan's bar is never the error state or the BEL-terminated form.
    assert_ne!(Bar::Plan(42).sequence(), Bar::Error(42).sequence());
    assert!(!Bar::Plan(42).sequence().ends_with('\x07'));
}

#[test]
fn a_plan_fills_in_whole_percents_rounded_down() {
    assert_eq!(
        (0..=3).map(|done| percent(done, 3)).collect::<Vec<_>>(),
        vec![Some(0), Some(33), Some(66), Some(100)]
    );
    // 100 only when every step is done; an empty plan is no plan; more done than planned
    // stays 100.
    assert_eq!(percent(99, 100), Some(99));
    assert_eq!(percent(0, 0), None);
    assert_eq!(percent(5, 4), Some(100));
    assert_eq!(percent(usize::MAX, usize::MAX), Some(100));
}

/// A whole turn with a plan of four steps: the bar at launch, indeterminate until the plan
/// comes, the plan's fill step by step (a box changes nothing), gone at the turn's end; each
/// state written once.
#[test]
fn a_plan_of_four_steps_fills_the_bar_once_per_change() {
    let tab = Tab::new();
    // Launch, at rest: the first frame hides whatever bar a killed predecessor left.
    tab.frame(ProgramState::Idle, None);
    assert_eq!(tab.take(), HIDE);
    tab.frame(ProgramState::Idle, None);
    assert_eq!(tab.take(), "", "rest stays unwritten");

    tab.frame(ProgramState::Working, None);
    tab.frame(ProgramState::Working, None);
    assert_eq!(
        tab.take(),
        WORKING,
        "a turn with no plan: indeterminate, once"
    );

    for done in 0..=4 {
        tab.frame(ProgramState::Working, Some((done, 4)));
        tab.frame(ProgramState::Working, Some((done, 4)));
        if done == 2 {
            // An approval box mid-plan: the turn still works, the bar holds.
            tab.frame(
                ProgramState::Blocked(BlockedKind::Permission),
                Some((done, 4)),
            );
        }
    }
    assert_eq!(tab.take(), [0, 25, 50, 75, 100].map(plan).concat());

    tab.frame(ProgramState::Done, Some((4, 4)));
    tab.frame(ProgramState::Idle, Some((4, 4)));
    assert_eq!(tab.take(), HIDE, "the turn's end takes the bar down");

    // Negative control: a frame that changes nothing writes nothing; one that does, writes.
    tab.frame(ProgramState::Idle, None);
    assert_eq!(tab.take(), "");
    tab.frame(ProgramState::Working, Some((1, 4)));
    assert_eq!(tab.take(), plan(25));
}

#[test]
fn a_failed_turn_turns_the_bar_red_where_its_plan_stood() {
    let tab = Tab::new();
    tab.frame(ProgramState::Working, Some((1, 4)));
    tab.frame(ProgramState::Error, Some((1, 4)));
    tab.frame(ProgramState::Error, Some((1, 4)));
    assert_eq!(tab.take(), plan(25) + &error(25));
    // A person's key acknowledges it: rest.
    tab.frame(ProgramState::Idle, None);
    assert_eq!(tab.take(), HIDE);
    // With no plan, the red bar is full.
    tab.frame(ProgramState::Working, None);
    tab.frame(ProgramState::Error, None);
    assert_eq!(tab.take(), format!("{WORKING}{}", error(100)));
    // Negative control: a turn that ends well never writes the error state.
    tab.frame(ProgramState::Working, Some((4, 4)));
    tab.frame(ProgramState::Done, Some((4, 4)));
    let bytes = tab.take();
    assert!(!bytes.contains("9;4;2"), "{bytes:?}");
    assert_eq!(bytes, plan(100) + HIDE);
}

/// Disabled (outside aterm, not the codex binary, not a terminal, or the setting off), a
/// whole turn writes nothing at all; switched off mid-turn, the bar it put up goes, once.
#[test]
fn nothing_is_written_while_the_bar_is_not_allowed() {
    let tab = Tab::new();
    for state in [
        ProgramState::Unknown,
        ProgramState::Idle,
        ProgramState::Working,
        ProgramState::Blocked(BlockedKind::Question),
        ProgramState::Error,
        ProgramState::Done,
    ] {
        tab.observe(/*enabled*/ false, state, Some((1, 2)));
        tab.observe(/*enabled*/ false, state, None);
    }
    tab.clear();
    assert_eq!(tab.take(), "", "not one byte");
    assert_eq!(tab.emitter.shown(), None);

    // Negative control: the same frames, allowed, do write.
    tab.observe(/*enabled*/ true, ProgramState::Working, Some((1, 2)));
    assert_eq!(tab.take(), plan(50));
    // Switched off mid-turn (`alab_progress = false` read on a reload): down once, then quiet.
    tab.observe(/*enabled*/ false, ProgramState::Working, Some((2, 2)));
    tab.observe(/*enabled*/ false, ProgramState::Working, Some((2, 2)));
    assert_eq!(tab.take(), HIDE);
}

/// Every exit path (`restore_after_exit`: `/exit`, a fatal error, the panic hook, the signal
/// watchdog) and every handoff (Ctrl+Z, an external program) takes a showing bar down; the
/// next frame after a handoff puts the current one back.
#[test]
fn the_exit_takes_a_showing_bar_down_once() {
    let tab = Tab::new();
    // Nothing ever written: the exit writes nothing.
    tab.clear();
    assert_eq!(tab.take(), "");

    tab.frame(ProgramState::Working, Some((2, 3)));
    assert_eq!(tab.take(), plan(66));
    tab.clear();
    assert_eq!(tab.take(), HIDE, "a mid-turn exit, panic or Ctrl+Z");
    assert_eq!(tab.emitter.shown(), Some(Bar::Hidden));
    tab.clear();
    assert_eq!(
        tab.take(),
        "",
        "a second clear (the panic hook after the exit) is quiet"
    );

    // Back from Ctrl+Z or the editor, still working: the bar is put back.
    tab.frame(ProgramState::Working, Some((2, 3)));
    assert_eq!(tab.take(), plan(66));

    // At rest the bar is already down: the exit writes nothing more.
    tab.frame(ProgramState::Idle, None);
    assert_eq!(tab.take(), HIDE);
    tab.clear();
    assert_eq!(tab.take(), "");

    // Negative control: an indeterminate or red bar is taken down too.
    for state in [ProgramState::Working, ProgramState::Error] {
        tab.frame(state, None);
        tab.take();
        tab.clear();
        assert_eq!(tab.take(), HIDE, "{state:?}");
    }
}

#[test]
fn a_failed_write_is_tried_again_next_frame() {
    let emitter = Emitter::new();
    let failed = emitter.frame(Bar::Indeterminate, |_| Err(std::io::Error::other("EAGAIN")));
    assert!(failed.is_err());
    assert_eq!(emitter.shown(), None, "not remembered as written");
    let mut written = Vec::new();
    let ok = emitter.frame(Bar::Indeterminate, |bar| {
        written.push(bar);
        Ok(())
    });
    assert!(ok.is_ok());
    assert_eq!(written, vec![Bar::Indeterminate]);
    assert_eq!(emitter.shown(), Some(Bar::Indeterminate));
}

fn layer(text: &str) -> toml::Value {
    toml::from_str(text).unwrap_or_else(|err| panic!("{text}: {err}"))
}

#[test]
fn the_highest_layer_that_sets_the_key_decides() {
    let on = layer("[tui]\nalab_progress = true\n");
    let off = layer("[tui]\nalab_progress = false\n");
    let silent = layer("[tui]\npet = \"alab-kitty\"\n");
    let typo = layer("[tui]\nalab_progress = \"false\"\n");
    // Highest precedence first, as `layers_high_to_low` gives them.
    assert!(setting([&silent, &on]), "the packaged default inside aterm");
    assert!(!setting([&off, &silent, &on]), "a person's false wins");
    assert!(setting([&on, &off]));
    // Absent anywhere, or not a boolean where it is first set: off (fail closed).
    assert!(!setting([&silent]));
    assert!(!setting(std::iter::empty::<&toml::Value>()));
    assert!(!setting([&typo, &on]));
    // Negative control: the key is read under `[tui]` only.
    assert!(!setting([&layer("alab_progress = true\n")]));
}

#[test]
fn the_bar_is_allowed_only_in_codex_alab_inside_aterm_to_a_terminal_with_the_setting_on() {
    let on = layer("[tui]\nalab_progress = true\n");
    let off = layer("[tui]\nalab_progress = false\n");
    // (active, inside aterm, under a multiplexer, to a terminal, layers)
    assert!(enabled(true, true, false, true, [&on]));
    // Negative controls: each fact alone turns it off.
    assert!(
        !enabled(false, true, false, true, [&on]),
        "not the codex-alab binary"
    );
    assert!(!enabled(true, false, false, true, [&on]), "outside aterm");
    assert!(
        !enabled(true, true, true, true, [&on]),
        "screen or zellij inside aterm: their terminal is not aterm"
    );
    assert!(
        !enabled(true, true, false, false, [&on]),
        "stdout not a terminal"
    );
    assert!(
        !enabled(true, true, false, true, [&off]),
        "the person's off"
    );
    assert!(
        !enabled(true, true, false, true, std::iter::empty::<&toml::Value>()),
        "no default"
    );
}

#[test]
fn the_bar_follows_the_program_state() {
    let plan = Some((3, 4));
    assert_eq!(Bar::of(ProgramState::Working, None), Bar::Indeterminate);
    assert_eq!(Bar::of(ProgramState::Working, plan), Bar::Plan(75));
    assert_eq!(
        Bar::of(ProgramState::Blocked(BlockedKind::Auth), plan),
        Bar::Plan(75)
    );
    assert_eq!(
        Bar::of(ProgramState::Working, Some((0, 0))),
        Bar::Indeterminate
    );
    assert_eq!(Bar::of(ProgramState::Error, plan), Bar::Error(75));
    assert_eq!(Bar::of(ProgramState::Error, None), Bar::Error(100));
    // A turn that fails before its first step is done: a full red bar, never an empty one (a
    // value renderer draws 0 as no bar). Negative control: one step done stays where it stood.
    assert_eq!(Bar::of(ProgramState::Error, Some((0, 4))), Bar::Error(100));
    assert_eq!(Bar::of(ProgramState::Error, Some((1, 4))), Bar::Error(25));
    assert_eq!(Bar::of(ProgramState::Working, Some((0, 4))), Bar::Plan(0));
    for rest in [
        ProgramState::Unknown,
        ProgramState::Idle,
        ProgramState::Done,
    ] {
        assert_eq!(Bar::of(rest, plan), Bar::Hidden, "{rest:?}");
    }
}
