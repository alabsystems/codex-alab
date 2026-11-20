// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::ColumnWidthConfig;
use super::GenericDisplayRow;
use super::ListSelectionView;
use super::SelectionItem;
use super::SelectionRowDisplay;
use crate::bottom_pane::alab_picker_pointer::PickerMouse;
use crate::bottom_pane::scroll_state::ScrollState;
use crate::bottom_pane::scroll_state::wheel_rows_in;
use crate::bottom_pane::selection_popup_common::adjust_start_for_wrapped_selection_visibility;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use std::cell::RefCell;
use unicode_width::UnicodeWidthStr;

impl GenericDisplayRow {
    pub(in crate::bottom_pane) fn alab_record_picker_rect(
        &self,
        area: Rect,
        buffer: Rect,
        y: u16,
        lines: usize,
    ) {
        if let Some(rect) = &self.alab_picker_rect {
            let height = lines.min(usize::from(area.bottom().saturating_sub(y))) as u16;
            rect.set(Rect::new(area.x, y, area.width, height).intersection(buffer));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Target {
    item: usize,
    rect: Rect,
}

impl Target {
    fn underline(self, buf: &mut Buffer) {
        for line in self.rect.rows() {
            let mut previous = None;
            for pos in line.positions() {
                if buf[pos].symbol().trim().is_empty() {
                    continue;
                }
                // Join words separated by one blank. Wider gaps separate the native columns.
                if previous.is_some_and(|x| pos.x - x == 2) {
                    buf[(pos.x - 1, pos.y)]
                        .modifier
                        .insert(Modifier::UNDERLINED);
                }
                buf[pos].modifier.insert(Modifier::UNDERLINED);
                previous = Some(pos.x);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Press {
    pos: Position,
    target: Option<Target>,
}

#[derive(Clone, Copy)]
struct Viewport {
    width: u16,
    height: u16,
    columns: ColumnWidthConfig,
}

impl Viewport {
    fn first_row(self, rows: &[GenericDisplayRow], state: ScrollState) -> usize {
        let capacity = usize::from(self.height).min(rows.len());
        adjust_start_for_wrapped_selection_visibility(
            rows,
            &state,
            capacity,
            capacity,
            self.width,
            self.height,
            self.columns,
        )
    }

    fn scroll(
        self,
        rows: &[GenericDisplayRow],
        state: ScrollState,
        steps: i16,
    ) -> Option<ScrollState> {
        let last = rows.len().checked_sub(1)?;
        let top = self.first_row(rows, state);
        // The last useful viewport is the first one that reveals the final row in full.
        // An oversized row is visible by itself, as in native keyboard navigation.
        let first_for = |scroll_top, selected| {
            self.first_row(
                rows,
                ScrollState {
                    scroll_top,
                    selected_idx: Some(selected),
                },
            )
        };
        let end = first_for(0, last);
        if steps > 0 && top >= end {
            return None;
        }
        let proposed = top.saturating_add_signed(isize::from(steps)).min(end);
        if proposed == top {
            return None;
        }
        let next = if proposed > top {
            // A small viewport may show fewer choices than a wheel tick. Start at the
            // first choice the previous viewport could not show in full.
            (top.saturating_add(1)..=proposed)
                .find(|&idx| first_for(top, idx) != top)
                .unwrap_or(proposed)
        } else {
            // Include the choice immediately above the old viewport even when it wraps.
            first_for(proposed, top.saturating_sub(1))
        };

        let previous = state.selected_idx.unwrap_or(next);
        let limit = last.min(next.saturating_add(usize::from(self.height.saturating_sub(1))));
        let visible_end = (next..=limit)
            .rev()
            .find(|&idx| first_for(next, idx) == next)
            .unwrap_or(next);
        // Retain keyboard focus wherever possible. A window containing only disabled
        // items keeps the native inert cursor so the view can still be scrolled.
        let nearest = previous.clamp(next, visible_end);
        let selected = (next..=visible_end)
            .filter(|&idx| rows.get(idx).is_some_and(|row| !row.is_disabled))
            .min_by_key(|idx| idx.abs_diff(nearest))
            .unwrap_or(nearest);
        Some(ScrollState {
            scroll_top: next,
            selected_idx: Some(selected),
        })
    }
}

#[derive(Default)]
struct Frame {
    area: Rect,
    viewport: Option<Viewport>,
    targets: Vec<Target>,
    hover: Option<(Position, Target)>,
    // Every left press begun on the modal owns its drag and release, even when
    // it starts on a blank cell or later loses its eligibility as a click.
    press: Option<Press>,
}

impl Frame {
    fn hit(&self, pos: Position) -> Option<Target> {
        self.targets
            .iter()
            .copied()
            .find(|target| target.rect.contains(pos))
    }
}

/// A newly opened or replaced picker starts without a painted frame or a pending gesture.
#[derive(Default)]
pub(super) struct Pointer(RefCell<Frame>);

impl Pointer {
    pub(super) fn for_view(id: Option<&str>) -> Option<Box<Self>> {
        (id == Some(codex_alab_tui::interface::AUTOPILOT_PICKER_ID)).then(Box::default)
    }
}

impl ListSelectionView {
    pub(super) fn alab_current_marker(
        &self,
        item: &SelectionItem,
        ordinal: usize,
        width: u16,
    ) -> (&'static str, char) {
        const CURRENT: &str = " (current)";
        if !item.is_current {
            return ("", ' ');
        }
        if self.view_id == Some(codex_alab_tui::interface::AUTOPILOT_PICKER_ID)
            && self.auto_number_shortcuts
            && !self.is_searchable
            && Self::item_is_enabled(item)
            && item.child_label.is_none()
        {
            let digits = ordinal.ilog10() as usize + 1;
            // The numbered gutter also contains the focus cell, a gap, a dot and a space.
            if 4 + digits + item.name.width() + CURRENT.len() > usize::from(width) {
                return ("", '✓');
            }
        }
        (CURRENT, ' ')
    }

    pub(super) fn alab_clear_pointer(&self, keep_geometry: bool) -> bool {
        let Some(pointer) = &self.alab_pointer else {
            return false;
        };
        let mut frame = pointer.0.borrow_mut();
        if keep_geometry {
            if let Some(press) = &mut frame.press {
                press.target = None;
            }
        } else {
            frame.targets.clear();
            frame.area = Rect::default();
            frame.viewport = None;
            frame.press = None;
        }
        frame.hover.take().is_some()
    }

    pub(super) fn alab_publish_pointer_rows(
        &self,
        rows: &[GenericDisplayRow],
        area: Rect,
        buf: &mut Buffer,
        width: u16,
        height: u16,
        columns: ColumnWidthConfig,
    ) {
        let Some(pointer) = &self.alab_pointer else {
            return;
        };
        let mut frame = pointer.0.borrow_mut();
        frame.area = area.intersection(buf.area);
        frame.viewport =
            (self.row_display == SelectionRowDisplay::Wrapped && width > 0 && height > 0)
                .then_some(Viewport {
                    width,
                    height,
                    columns,
                });
        frame.targets.clear();
        for (row, &item) in rows.iter().zip(&self.filtered_indices) {
            if let Some(cell) = &row.alab_picker_rect {
                let rect = cell.get();
                if !rect.is_empty() {
                    frame.targets.push(Target { item, rect });
                }
            }
        }
        if let Some((pos, target)) = frame.hover {
            if frame.hit(pos) == Some(target)
                && codex_alab_tui::interface::autopilot_selection_available()
            {
                target.underline(buf);
            } else {
                frame.hover = None;
            }
        }
        if let Some(Press {
            pos,
            target: Some(target),
        }) = frame.press
            && frame.hit(pos) != Some(target)
        {
            frame.press = Some(Press { pos, target: None });
        }
    }

    #[cfg(test)]
    pub(super) fn alab_pointer_event(&mut self, event: MouseEvent) -> bool {
        self.alab_pointer_result(event).handled
    }

    pub(super) fn alab_pointer_result(&mut self, event: MouseEvent) -> PickerMouse {
        let Some(pointer) = &self.alab_pointer else {
            return PickerMouse::default();
        };
        let mut frame = pointer.0.borrow_mut();
        let pos = Position::new(event.column, event.row);
        let target = if event.kind == MouseEventKind::Moved
            && event.modifiers.is_empty()
            && frame.press.is_none()
            && codex_alab_tui::interface::autopilot_selection_available()
        {
            frame.hit(pos)
        } else {
            None
        };
        let mut result = PickerMouse {
            handled: target.is_some(),
            redraw: frame.hover.map(|(_, target)| target) != target,
        };
        frame.hover = target.map(|target| (pos, target));
        if let Some(steps) = wheel_rows_in(event, frame.area) {
            if let Some(press) = &mut frame.press {
                press.target = None;
            }
            if let Some(viewport) = frame.viewport
                && let Some(state) =
                    viewport.scroll(&self.build_rows(viewport.width), self.state, steps)
            {
                // A second wheel tick can use the new state immediately. Clicks need a new paint.
                result.redraw = true;
                frame.targets.clear();
                drop(frame);
                let previous = self.state.selected_idx;
                self.state = state;
                if previous != state.selected_idx && !self.selected_visible_idx_is_disabled() {
                    self.fire_selection_changed();
                }
            }
            result.handled = true;
            return result;
        }
        let handled = match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                frame.press =
                    (event.modifiers.is_empty() && frame.area.contains(pos)).then(|| Press {
                        pos,
                        target: frame.hit(pos),
                    });
                frame.press.is_some()
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let Some(press) = frame.press.take() else {
                    return result;
                };
                if let Some(target) = press.target
                    && event.modifiers.is_empty()
                    && press.pos == pos
                    && frame.hit(pos) == Some(target)
                {
                    // Use the numbered shortcut's primary action, host availability check,
                    // and native parent/child completion rules.
                    drop(frame);
                    self.select_shortcut(target.item);
                    result.redraw = true;
                }
                true
            }
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Moved => {
                let Some(press) = &mut frame.press else {
                    return result;
                };
                if event.kind != MouseEventKind::Moved
                    || press.pos != pos
                    || !event.modifiers.is_empty()
                {
                    press.target = None;
                }
                true
            }
            _ => {
                if let Some(press) = &mut frame.press {
                    press.target = None;
                }
                false
            }
        };
        result.handled |= handled;
        result
    }
}

#[cfg(test)]
#[path = "autopilot_picker_hover_tests.rs"]
mod hover_tests;

#[cfg(test)]
#[path = "autopilot_picker_wheel_tests.rs"]
mod wheel_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use crate::app_event_sender::AppEventSender;
    use crate::bottom_pane::SelectionItem;
    use crate::bottom_pane::SelectionViewParams;
    use crate::render::renderable::Renderable;
    use codex_alab_tui::interface;
    use codex_alab_tui::session::Autopilot;
    use codex_alab_tui::session::HarnessWord;
    use codex_alab_tui::session::Shared;
    use crossterm::event::KeyModifiers;
    use ratatui::buffer::Buffer;

    fn row(buf: &Buffer, needle: &str) -> Position {
        let y = (buf.area.y..buf.area.bottom())
            .find(|y| {
                (buf.area.x..buf.area.right())
                    .map(|x| buf[(x, *y)].symbol())
                    .collect::<String>()
                    .contains(needle)
            })
            .expect(needle);
        Position::new(buf.area.x + 6, y)
    }

    #[test]
    fn wrapped_picker_rows_keep_the_original_owner_until_release() {
        let _interface = interface::scoped(Box::leak(Box::new(interface::State::new())));
        interface::configure(true);
        let shared = Box::leak(Box::new(Shared::new()));
        let _automation = codex_alab_tui::boxes::scoped(shared);
        shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);

        let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
        let params = SelectionViewParams {
            view_id: Some(interface::AUTOPILOT_PICKER_ID),
            title: Some("Autopilot".into()),
            items: vec![
                SelectionItem { name: "Cruise".into(), ..Default::default() },
                SelectionItem { name: "Unavailable option".into(), is_disabled: true, ..Default::default() },
                SelectionItem {
                    name: "Timed mode".into(),
                    description: Some("The description continues onto another displayed row before reaching the terminator.".into()),
                    actions: vec![Box::new(|tx| tx.send(AppEvent::UpdateModel("accepted".into())))],
                    ..Default::default()
                },
            ],
            ..SelectionViewParams::picker()
        };
        let mut view = ListSelectionView::new(
            params,
            AppEventSender::new(tx),
            crate::keymap::RuntimeKeymap::defaults().list,
        );
        let area = Rect::new(5, 3, 64, 18);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
        let choice = row(&buf, "terminator.");
        assert!(choice.y > row(&buf, "Timed mode").y, "description wraps");
        let disabled = row(&buf, "Unavailable option");
        let outside = Position::new(area.x - 1, choice.y);
        let down = MouseEventKind::Down(MouseButton::Left);
        let up = MouseEventKind::Up(MouseButton::Left);
        let drag = MouseEventKind::Drag(MouseButton::Left);
        let moved = MouseEventKind::Moved;
        let mouse = |kind, pos: Position| MouseEvent {
            kind,
            column: pos.x,
            row: pos.y,
            modifiers: KeyModifiers::NONE,
        };

        for (kind, pos, handled) in [
            (down, outside, false),
            (drag, choice, false),
            (up, choice, false),
            (down, choice, true),
            (drag, outside, true),
            (drag, choice, true),
            (up, choice, true),
            (down, choice, true),
            (moved, outside, true),
            (moved, choice, true),
            (up, choice, true),
            (down, disabled, true),
            (up, disabled, true),
        ] {
            assert_eq!(
                view.alab_pointer_event(mouse(kind, pos)),
                handled,
                "{kind:?} {pos:?}"
            );
        }

        // Removing and restoring the same painted row cannot rearm a held press.
        assert!(view.alab_pointer_event(mouse(down, choice)));
        view.items[2].is_disabled = true;
        view.render(area, &mut buf);
        view.items[2].is_disabled = false;
        view.render(area, &mut buf);
        assert!(view.alab_pointer_event(mouse(up, choice)));
        assert!(events.try_recv().is_err());

        // An identical redraw and a same-cell movement preserve the click.
        assert!(view.alab_pointer_event(mouse(down, choice)));
        view.render(area, &mut buf);
        assert!(view.alab_pointer_event(mouse(moved, choice)));
        assert!(view.alab_pointer_event(mouse(up, choice)));
        assert!(matches!(events.try_recv(), Ok(AppEvent::UpdateModel(text)) if text == "accepted"));
        assert!(events.try_recv().is_err());
    }
}
