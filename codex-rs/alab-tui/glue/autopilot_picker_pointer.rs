// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A pointer can accept only the visible Autopilot picker, through its own action and
//! the same completion path as keyboard input. Other views keep their native routing.

use codex_alab_tui::interface::AUTOPILOT_PICKER_ID;
use crossterm::event::MouseEvent;

#[derive(Default)]
pub(crate) struct PickerMouse {
    pub(crate) handled: bool,
    pub(crate) redraw: bool,
}

impl super::BottomPane {
    pub(crate) fn alab_cancel_picker_mouse(&mut self) -> bool {
        self.alab_reset_picker_mouse(false)
    }

    pub(crate) fn alab_reset_picker_mouse(&mut self, keep_geometry: bool) -> bool {
        if let Some(view) = self.view_stack.last_mut()
            && view.view_id() == Some(AUTOPILOT_PICKER_ID)
            && view.alab_cancel_picker_mouse(keep_geometry)
        {
            self.request_redraw();
            return true;
        }
        false
    }

    pub(crate) fn alab_picker_mouse(&mut self, event: MouseEvent) -> bool {
        if self.warnings_active() {
            return false;
        }
        let Some(view) = self.view_stack.last_mut() else {
            return false;
        };
        if view.view_id() != Some(AUTOPILOT_PICKER_ID) {
            return false;
        }
        let result = view.alab_picker_mouse(event);
        if result.handled && view.is_complete() {
            let completion = view.completion();
            self.pop_active_view_with_completion(completion);
        }
        if result.redraw {
            self.request_redraw();
        }
        result.handled
    }
}

#[cfg(test)]
mod tests {
    use super::AUTOPILOT_PICKER_ID;
    use crate::app_event_sender::AppEventSender;
    use crate::bottom_pane::SelectionItem;
    use crate::bottom_pane::SelectionViewParams;
    use crate::bottom_pane::tests::test_pane;
    use crate::render::renderable::Renderable;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    use crossterm::event::MouseButton;
    use crossterm::event::MouseEvent;
    use crossterm::event::MouseEventKind;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use tokio::sync::mpsc::unbounded_channel;

    #[tokio::test]
    async fn uncovering_the_picker_requires_a_new_foreground_paint() {
        let (tx, _rx) = unbounded_channel();
        let mut pane = test_pane(AppEventSender::new(tx));
        pane.show_selection_view(SelectionViewParams {
            title: Some("Autopilot".into()),
            view_id: Some(AUTOPILOT_PICKER_ID),
            items: vec![SelectionItem {
                name: "Cruise".into(),
                ..Default::default()
            }],
            ..SelectionViewParams::picker()
        });
        let area = Rect::new(4, 3, 60, 12);
        let mut buffer = Buffer::empty(area);
        pane.render(area, &mut buffer);
        let (column, row) = (area.top()..area.bottom())
            .find_map(|y| {
                (area.left()..area.right())
                    .find(|&x| buffer[(x, y)].symbol() == "C")
                    .map(|x| (x, y))
            })
            .expect("visible Cruise row");
        let press = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        for dismissal in ["Esc", "active id", "matching id"] {
            assert!(pane.alab_picker_mouse(press), "{dismissal}: fresh paint");
            pane.show_selection_view(SelectionViewParams {
                view_id: Some("test-confirmation"),
                title: Some("Confirm".into()),
                ..SelectionViewParams::confirmation()
            });
            // The centered dialog paints the picker again as its backdrop.
            pane.render(area, &mut buffer);
            match dismissal {
                "Esc" => {
                    pane.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
                }
                "active id" => assert!(pane.dismiss_active_view_if_id("test-confirmation")),
                _ => assert!(pane.dismiss_view_by_id("test-confirmation")),
            }
            assert_eq!(pane.active_view_id(), Some(AUTOPILOT_PICKER_ID));
            assert!(
                !pane.alab_picker_mouse(press),
                "{dismissal}: stale backdrop"
            );
            pane.render(area, &mut buffer);
        }
    }
}
