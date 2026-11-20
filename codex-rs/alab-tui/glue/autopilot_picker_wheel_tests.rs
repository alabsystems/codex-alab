// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::render::renderable::Renderable;
use codex_alab_tui::interface::AUTOPILOT_PICKER_ID;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEventKind::Down;
use crossterm::event::MouseEventKind::ScrollDown;
use crossterm::event::MouseEventKind::ScrollUp;
use crossterm::event::MouseEventKind::Up;
use ratatui::buffer::Buffer;

const NAMES: [&str; 8] = [
    "Jade", "Ochre", "Mint", "Sage", "Fern", "Pearl", "Dusk", "Amber",
];

fn picker(count: usize) -> ListSelectionView {
    let (tx, _events) = tokio::sync::mpsc::unbounded_channel();
    let params = SelectionViewParams {
        view_id: Some(AUTOPILOT_PICKER_ID),
        items: NAMES[..count]
            .iter()
            .enumerate()
            .map(|(i, name)| SelectionItem {
                name: (*name).into(),
                is_disabled: i == 1,
                require_explicit_confirmation: true,
                ..Default::default()
            })
            .collect(),
        max_visible_rows: 3,
        initial_selected_idx: Some(0),
        ..SelectionViewParams::picker()
    };
    ListSelectionView::new(
        params,
        AppEventSender::new(tx),
        crate::keymap::RuntimeKeymap::defaults().list,
    )
}

fn paint(view: &ListSelectionView) -> Buffer {
    paint_in(view, Rect::new(5, 3, 50, 12))
}

fn paint_in(view: &ListSelectionView, area: Rect) -> Buffer {
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    buf
}

fn find_row(buf: &Buffer, name: &str) -> Option<Position> {
    (buf.area.top()..buf.area.bottom()).find_map(|y| {
        let line: String = (buf.area.left()..buf.area.right())
            .map(|x| buf[(x, y)].symbol())
            .collect();
        line.contains(name)
            .then(|| Position::new(buf.area.x + 6, y))
    })
}

fn mouse(kind: MouseEventKind, pos: Position) -> MouseEvent {
    MouseEvent {
        kind,
        column: pos.x,
        row: pos.y,
        modifiers: KeyModifiers::NONE,
    }
}

fn click(view: &mut ListSelectionView, pos: Position) {
    for kind in [Down(MouseButton::Left), Up(MouseButton::Left)] {
        view.alab_pointer_event(mouse(kind, pos));
    }
}

#[test]
fn fully_painted_choices_keep_selection_and_cancel_held_clicks_at_both_limits() {
    let mut view = picker(3);
    let buf = paint(&view);
    let mint = find_row(&buf, "Mint").expect("last choice is visible");
    let ochre = find_row(&buf, "Ochre").expect("disabled choice is visible");
    let outside = Position::new(buf.area.x - 1, mint.y);
    for kind in [MouseEventKind::ScrollLeft, MouseEventKind::ScrollRight] {
        assert!(!view.alab_pointer_event(mouse(kind, mint)));
    }
    for kind in [ScrollDown, ScrollUp] {
        assert!(!view.alab_pointer_event(mouse(kind, outside)));
        assert!(!view.alab_pointer_event(MouseEvent {
            modifiers: KeyModifiers::SHIFT,
            ..mouse(kind, ochre)
        }));
        assert!(view.alab_pointer_event(mouse(Down(MouseButton::Left), mint)));
        assert!(view.alab_pointer_event(mouse(kind, ochre)));
        assert!(view.alab_pointer_event(mouse(Up(MouseButton::Left), mint)));
        assert_eq!(view.selected_actual_idx(), Some(0));
    }
    let buf = paint(&view);
    click(&mut view, find_row(&buf, "Ochre").unwrap());
    assert_eq!(view.selected_actual_idx(), Some(0));
    click(&mut view, find_row(&buf, "Mint").unwrap());
    assert_eq!(view.selected_actual_idx(), Some(2));
}

#[test]
fn wheel_bursts_reach_both_ends_and_stale_clicks_cannot_select() {
    let mut view = picker(NAMES.len());
    let buf = paint(&view);
    let first = find_row(&buf, "Jade").expect("first choice is visible");
    let third = find_row(&buf, "Mint").expect("third choice is visible");
    assert!(find_row(&buf, "Amber").is_none());
    for _ in 0..NAMES.len() {
        assert!(view.alab_pointer_event(mouse(ScrollDown, first)));
    }
    let selected = view.selected_actual_idx();
    click(&mut view, if selected == Some(0) { third } else { first });
    assert_eq!(view.selected_actual_idx(), selected);

    let buf = paint(&view);
    let last = find_row(&buf, "Amber").expect("wheel burst reaches the last choice");
    assert!(find_row(&buf, "Jade").is_none());
    for _ in 0..NAMES.len() {
        assert!(view.alab_pointer_event(mouse(ScrollUp, last)));
    }
    let buf = paint(&view);
    assert!(find_row(&buf, "Jade").is_some());
    assert!(find_row(&buf, "Ochre").is_some());
    assert!(find_row(&buf, "Amber").is_none());
}

#[test]
fn wheel_reveals_the_last_wrapped_line_and_retains_visible_focus() {
    let mut view = picker(3);
    view.items[1].is_disabled = false;
    view.items[2].description =
        Some("Continue with every remaining choice before the afterword.".into());
    let area = Rect::new(5, 3, 50, 4);
    let buf = paint_in(&view, area);
    click(&mut view, find_row(&buf, "Ochre").unwrap());
    let buf = paint_in(&view, area);
    let clipped = find_row(&buf, "Mint").expect("the final choice starts in view");
    assert!(find_row(&buf, "afterword.").is_none());
    assert_eq!(view.selected_actual_idx(), Some(1));

    assert!(view.alab_pointer_event(mouse(ScrollDown, clipped)));
    let buf = paint_in(&view, area);
    let mint = find_row(&buf, "Mint").unwrap();
    let tail = find_row(&buf, "afterword.").expect("the end of the description is visible");
    assert!(tail.y > mint.y);
    assert_eq!(view.selected_actual_idx(), Some(1));
}

#[test]
fn a_single_visible_row_scrolls_through_every_choice_without_wrapping() {
    let mut view = picker(NAMES.len());
    let area = Rect::new(5, 3, 50, 1);
    let mut buf = paint_in(&view, area);
    let pos = find_row(&buf, NAMES[0]).expect("first choice is visible");
    let mut expected = NAMES;
    for direction in [ScrollDown, ScrollUp] {
        let mut painted = Vec::new();
        for _ in 0..NAMES.len() {
            let name = NAMES
                .into_iter()
                .find(|name| find_row(&buf, name).is_some())
                .expect("a choice is visible on each wheel tick");
            painted.push(name);
            assert!(view.alab_pointer_event(mouse(direction, pos)));
            buf = paint_in(&view, area);
        }
        assert_eq!(painted, expected);
        assert!(find_row(&buf, expected.last().unwrap()).is_some());
        expected.reverse();
    }
}
