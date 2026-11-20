// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Two complete keyboard actions in the spare row below the local timer input.

use super::PromptAreas;
use crate::bottom_pane::footer::max_left_width_for_right;
use crate::bottom_pane::footer::render_context_right;
use crate::key_hint;
use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;

pub(super) fn render(
    areas: &PromptAreas,
    buf: &mut Buffer,
    submit_label: &str,
    cancel_label: &str,
    inline_width: usize,
    vim_mode: Option<&Line<'static>>,
) -> bool {
    let footer = areas.footer;
    if inline_width <= usize::from(footer.width)
        || areas.input.is_empty()
        || areas.input.bottom() >= footer.y
    {
        return false;
    }
    let submit = Line::from(vec![
        key_hint::plain(KeyCode::Enter).into(),
        format!(" {submit_label}").dim(),
    ]);
    let cancel = Line::from(vec![
        key_hint::plain(KeyCode::Esc).into(),
        cancel_label.dim(),
    ]);
    let widest = submit.width().max(cancel.width());
    if widest > usize::from(areas.panel.width) {
        return false;
    }
    // Use the usual input inset whenever both complete actions fit there. Narrow
    // Vim panes can recover the outer padding to display the full “esc normal mode”.
    let width = footer.width.max(widest as u16);
    let bottom = Rect {
        x: footer.x.min(areas.panel.right() - width),
        width,
        ..footer
    };
    let top = Rect {
        y: bottom.y - 1,
        ..bottom
    };
    Clear.render(
        Rect {
            x: areas.panel.x,
            width: areas.panel.width,
            ..top
        },
        buf,
    );
    (&submit).render(top, buf);
    (&cancel).render(bottom, buf);
    if let Some(mode) = vim_mode {
        for (row, hint) in [(bottom, cancel), (top, submit)] {
            if max_left_width_for_right(row, mode.width() as u16)
                .is_some_and(|room| usize::from(room) >= hint.width())
            {
                render_context_right(row, buf, mode);
                break;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::super::CustomPromptView;
    use crate::bottom_pane::bottom_pane_view::BottomPaneView;
    use crate::render::renderable::Renderable;
    use crossterm::event::KeyCode;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn render(view: &CustomPromptView, area: Rect) -> Buffer {
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
        buf
    }

    #[test]
    fn narrow_timer_hints_follow_vim_escape_and_preserve_the_input() {
        let mut view = CustomPromptView::new(
            "Cruise until…".into(),
            "18:00 or +2h".into(),
            "+2h".into(),
            None,
            Box::new(|_| {}),
        );
        view.enable_vim_in_insert_mode();
        for (escape, inline_at) in [("esc normal mode", 34), ("esc back", 27)] {
            for (width, height) in [(16, 3), (16, 12), (26, 12), (27, 12), (34, 12), (80, 12)] {
                let area = Rect::new(5, 2, width, height);
                view.local_submit_available = None;
                let ordinary = render(&view, area);
                let cursor = view.cursor_pos(area);
                view.local_submit_available = Some(|| true);
                let local = render(&view, area);
                assert_eq!(cursor, view.cursor_pos(area));
                if height == 3 || width >= inline_at {
                    assert_eq!(local, ordinary, "{escape} {area:?}");
                    continue;
                }
                for y in area.top()..area.bottom() - 2 {
                    for x in area.left()..area.right() {
                        assert_eq!(local[(x, y)], ordinary[(x, y)], "{escape} {area:?}");
                    }
                }
                let row = |y| {
                    (area.left()..area.right())
                        .map(|x| local[(x, y)].symbol())
                        .collect::<String>()
                };
                assert!(row(area.bottom() - 2).contains("enter submit"));
                assert!(row(area.bottom() - 1).contains(escape));
            }
            if view.prefer_esc_to_handle_key_event() {
                view.handle_key_event(KeyCode::Esc.into());
            }
        }
    }
}
