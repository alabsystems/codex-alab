// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::text_selection::MULTI_CLICK_INTERVAL;
use crate::tui::TuiEvent;
use crossterm::event::MouseButton::Left;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Position;
use std::time::Instant;

/// Keep a multi-click gesture with its original control when the picker opens or closes.
#[derive(Clone, Copy, Default)]
pub(super) enum PickerClick {
    #[default]
    Idle,
    Pressed(Instant),
    Disclosure {
        pressed_at: Instant,
        cell: Position,
    },
    Transition {
        picker_open: bool,
        pressed_at: Instant,
        cell: Position,
    },
    HoldingSecondPress,
}

impl PickerClick {
    /// Only handled picker events get here. Closing on release proves the
    /// original stationary click activated a choice and exposed the composer.
    pub(super) fn observe(&mut self, event: MouseEvent, closed: bool, now: Instant) {
        match event.kind {
            MouseEventKind::Down(Left) if event.modifiers.is_empty() => *self = Self::Pressed(now),
            MouseEventKind::Up(Left) => {
                *self = match *self {
                    Self::Pressed(pressed_at) if closed => Self::Transition {
                        picker_open: false,
                        pressed_at,
                        cell: Position::new(event.column, event.row),
                    },
                    _ => Self::Idle,
                };
            }
            _ => {}
        }
    }

    pub(super) fn press_disclosure(&mut self, cell: Position, now: Instant) {
        *self = Self::Disclosure {
            pressed_at: now,
            cell,
        };
    }

    pub(super) fn disclosure_pressed(self, cell: Position) -> bool {
        matches!(self, Self::Disclosure { cell: pressed, .. } if pressed == cell)
    }

    pub(super) fn disclosure_opened(&mut self) {
        if let Self::Disclosure { cell, pressed_at } = *self {
            *self = Self::Transition {
                cell,
                pressed_at,
                picker_open: true,
            };
        }
    }

    pub(super) fn expects_picker(self) -> bool {
        matches!(
            self,
            Self::Transition {
                picker_open: true,
                ..
            }
        )
    }

    pub(super) fn pending(self) -> bool {
        matches!(
            self,
            Self::Disclosure { .. } | Self::Transition { .. } | Self::HoldingSecondPress
        )
    }

    pub(super) fn invalidate(&mut self) {
        // A new owner must never receive the drag or release of a press it did not see.
        if !matches!(self, Self::HoldingSecondPress) {
            *self = Self::Idle;
        }
    }

    pub(super) fn take_held_release(&mut self) -> Self {
        self.invalidate();
        std::mem::take(self)
    }

    pub(super) fn consume(&mut self, event: &TuiEvent, now: Instant) -> bool {
        if let Self::Transition { pressed_at, .. } = *self
            && now.saturating_duration_since(pressed_at) >= MULTI_CLICK_INTERVAL
        {
            *self = Self::Idle;
        }
        let TuiEvent::Mouse(mouse) = event else {
            return false;
        };
        match *self {
            Self::Disclosure { cell, .. } => {
                // The late dispatcher rechecks the target on release after a fresh paint.
                if cell != Position::new(mouse.column, mouse.row)
                    || !mouse.modifiers.is_empty()
                    || !matches!(mouse.kind, MouseEventKind::Moved | MouseEventKind::Up(Left))
                {
                    *self = Self::Idle;
                }
                false
            }
            Self::Transition { cell, .. } => {
                let same_plain_cell =
                    cell == Position::new(mouse.column, mouse.row) && mouse.modifiers.is_empty();
                match mouse.kind {
                    MouseEventKind::Down(Left) if same_plain_cell => {
                        *self = Self::HoldingSecondPress;
                        true
                    }
                    // Some terminals repeat the release that closed the picker.
                    MouseEventKind::Up(Left) if same_plain_cell => true,
                    MouseEventKind::Moved if same_plain_cell => false,
                    _ => {
                        *self = Self::Idle;
                        false
                    }
                }
            }
            Self::HoldingSecondPress => match mouse.kind {
                MouseEventKind::Drag(Left) => true,
                MouseEventKind::Up(Left) => {
                    *self = Self::Idle;
                    true
                }
                // The terminal may have lost the matching release while unfocused.
                MouseEventKind::Down(Left) => {
                    *self = Self::Idle;
                    false
                }
                _ => false,
            },
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use crossterm::event::MouseButton::Right;
    use crossterm::event::MouseEventKind::Down;
    use crossterm::event::MouseEventKind::Drag;
    use crossterm::event::MouseEventKind::Up;
    use std::time::Duration;

    fn mouse(kind: MouseEventKind, column: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row: 21,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn closed(pressed: Instant, released: Instant) -> PickerClick {
        let mut click = PickerClick::default();
        click.observe(mouse(Down(Left), 73), false, pressed);
        click.observe(mouse(Up(Left), 73), true, released);
        click
    }

    #[test]
    fn interval_is_measured_between_presses_and_only_a_closed_picker_arms_it() {
        let first = Instant::now();
        let boundary = first + MULTI_CLICK_INTERVAL;
        let second = TuiEvent::Mouse(mouse(Down(Left), 73));
        let mut click = closed(first, boundary);
        assert!(!click.consume(&second, boundary));
        let mut click = closed(first, first);
        assert!(click.consume(&second, boundary - Duration::from_millis(1)));

        let mut click = PickerClick::default();
        click.observe(mouse(Down(Left), 73), false, first);
        click.observe(mouse(Up(Left), 73), false, first);
        assert!(!click.consume(&second, first));
        let mut click = closed(first, first);
        click.invalidate();
        assert!(!click.consume(&second, first));
    }

    #[test]
    fn only_the_same_plain_left_press_and_its_drag_and_release_are_owned() {
        let now = Instant::now();
        for other in [
            mouse(Down(Left), 74),
            mouse(Down(Right), 73),
            MouseEvent {
                modifiers: KeyModifiers::CONTROL,
                ..mouse(Down(Left), 73)
            },
        ] {
            let mut click = closed(now, now);
            assert!(!click.consume(&TuiEvent::Mouse(other), now));
            assert!(!click.consume(&TuiEvent::Mouse(mouse(Down(Left), 73)), now));
        }

        let mut click = closed(now, now);
        assert!(!click.consume(&TuiEvent::Draw, now));
        assert!(click.consume(&TuiEvent::Mouse(mouse(Up(Left), 73)), now));
        assert!(click.consume(&TuiEvent::Mouse(mouse(Down(Left), 73)), now));
        click.invalidate();
        let mut replacement = click.take_held_release();
        assert!(!click.pending());
        let late = now + Duration::from_secs(30);
        // A release outside an unfocused terminal can be lost. Normal hover works
        // immediately on return, even if a late release still needs consuming.
        assert!(!replacement.consume(&TuiEvent::Mouse(mouse(MouseEventKind::Moved, 41)), late));
        assert!(replacement.consume(&TuiEvent::Mouse(mouse(Drag(Left), 11)), late));
        assert!(!replacement.consume(&TuiEvent::Mouse(mouse(Up(Right), 11)), late));
        assert!(replacement.consume(&TuiEvent::Mouse(mouse(Up(Left), 11)), late));
        assert!(!replacement.consume(&TuiEvent::Mouse(mouse(Down(Left), 73)), late));
    }
}
