// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPane;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::tests::test_pane;
use crate::render::renderable::Renderable;
use crate::tui::FrameRequester;
use codex_alab_tui::boxes;
use codex_alab_tui::interface;
use codex_alab_tui::session::Autopilot;
use codex_alab_tui::session::HarnessWord;
use codex_alab_tui::session::Shared;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use crossterm::event::MouseEventKind::Moved;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

fn picker() -> (BottomPane, interface::Scope, boxes::Scope) {
    let interface = interface::scoped(Box::leak(Box::new(interface::State::new())));
    interface::configure(true);
    let shared = Box::leak(Box::new(Shared::new()));
    let automation = boxes::scoped(shared);
    shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
    let (tx, _events) = tokio::sync::mpsc::unbounded_channel();
    let mut pane = test_pane(AppEventSender::new(tx));
    pane.show_selection_view(SelectionViewParams {
        view_id: Some(interface::AUTOPILOT_PICKER_ID),
        title: Some("Autopilot".into()),
        items: vec![
            SelectionItem {
                name: "Land".into(),
                is_current: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Unavailable option".into(),
                is_disabled: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Timed mode".into(),
                description: Some(
                    "The description continues onto another displayed row before reaching the terminator."
                        .into(),
                ),
                ..Default::default()
            },
        ],
        ..SelectionViewParams::picker()
    });
    (pane, interface, automation)
}

fn paint(pane: &BottomPane) -> Buffer {
    let area = Rect::new(5, 3, 64, 18);
    let mut buf = Buffer::empty(area);
    pane.render(area, &mut buf);
    buf
}

fn point(buf: &Buffer, text: &str) -> Position {
    buf.area
        .positions()
        .find(|p| {
            (p.x..buf.area.right())
                .map(|x| buf[(x, p.y)].symbol())
                .collect::<String>()
                .starts_with(text)
        })
        .expect(text)
}

fn mouse(kind: MouseEventKind, pos: Position) -> MouseEvent {
    MouseEvent {
        kind,
        column: pos.x,
        row: pos.y,
        modifiers: KeyModifiers::NONE,
    }
}

#[tokio::test]
async fn wrapped_hover_preserves_native_styles_and_leaves_gutters_clear() {
    let (mut pane, _interface, _automation) = picker();
    let before = paint(&pane);
    let name = point(&before, "Timed mode");
    let description = point(&before, "The description");
    let tail = point(&before, "terminator.");
    let selected = point(&before, "Land (current)");
    assert!(tail.y > name.y);
    pane.alab_picker_mouse(mouse(Moved, tail));
    let hovered = paint(&pane);
    for pos in [name, Position::new(name.x + 5, name.y), description, tail] {
        let mut expected = before[pos].clone();
        expected.modifier.insert(Modifier::UNDERLINED);
        assert_eq!(hovered[pos], expected);
    }
    for x in name.x + 10..description.x {
        assert_eq!(hovered[(x, name.y)], before[(x, name.y)]);
    }
    for pos in before.area.positions() {
        if !(name.y..=tail.y).contains(&pos.y)
            || (pos.y == tail.y && pos.x < tail.x && before[pos].symbol() == " ")
        {
            assert_eq!(
                hovered[pos], before[pos],
                "unexpected hover style at {pos:?}"
            );
        }
    }
    pane.alab_picker_mouse(mouse(Moved, selected));
    let hovered = paint(&pane);
    let mut expected = before[selected].clone();
    expected.modifier.insert(Modifier::UNDERLINED);
    assert_eq!(hovered[selected], expected);
    assert_eq!(hovered[name], before[name]);
}

#[tokio::test]
async fn only_entering_or_leaving_a_choice_requests_a_hover_frame() {
    let (mut pane, _interface, _automation) = picker();
    let before = paint(&pane);
    let name = point(&before, "Timed mode");
    let tail = point(&before, "terminator.");
    let (requester, mut frames) = FrameRequester::test_channel();
    pane.frame_requester = requester;
    for (kind, point) in [
        (Moved, point(&before, "Unavailable option")),
        (Moved, point(&before, "Autopilot")),
        (Moved, Position::new(before.area.x - 1, name.y)),
        (MouseEventKind::ScrollDown, name),
    ] {
        assert!(pane.alab_picker_mouse(mouse(Moved, name)));
        assert!(frames.try_recv().is_ok(), "entering the timer");
        assert!(pane.alab_picker_mouse(mouse(Moved, name)));
        assert!(pane.alab_picker_mouse(mouse(Moved, tail)));
        assert!(frames.try_recv().is_err(), "same wrapped choice");
        assert!(paint(&pane)[name].modifier.contains(Modifier::UNDERLINED));

        let handled = pane.alab_picker_mouse(mouse(kind, point));
        assert_eq!(handled, kind == MouseEventKind::ScrollDown);
        assert!(
            frames.try_recv().is_ok(),
            "clearing hover on {kind:?} at {point:?}"
        );
        assert!(frames.try_recv().is_err(), "one redraw for the transition");
        assert_eq!(paint(&pane), before);
    }
}
