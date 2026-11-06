// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::bottom_pane::SelectionItem;
use crate::keymap::RuntimeKeymap;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::unbounded_channel;

fn params(names: &[&str]) -> SelectionViewParams {
    SelectionViewParams {
        title: Some("Tasks".into()),
        items: names
            .iter()
            .map(|name| SelectionItem {
                name: (*name).into(),
                dismiss_on_select: true,
                ..SelectionItem::default()
            })
            .collect(),
        ..SelectionViewParams::default()
    }
}

fn picker(now: Instant) -> (TasksView, UnboundedReceiver<AppEvent>) {
    let (tx, rx) = unbounded_channel();
    (
        TasksView::new_at(
            params(&["Build tests", "Deploy staging", "Review patch"]),
            ["build", "deploy", "review"].map(str::to_owned).into(),
            Some(ThreadId::from_u128(12)),
            AppEventSender::new(tx),
            RuntimeKeymap::defaults().list,
            now,
        ),
        rx,
    )
}

fn refresh(
    rx: &mut UnboundedReceiver<AppEvent>,
) -> (
    Option<ThreadId>,
    Option<String>,
    Option<String>,
    Option<usize>,
) {
    match rx.try_recv().expect("one due refresh") {
        AppEvent::AlabTasksRefresh {
            thread_id,
            selected_key,
            query,
            selected_index,
            ..
        } => (thread_id, selected_key, query, selected_index),
        event => panic!("expected Tasks refresh, got {event:?}"),
    }
}

#[test]
fn refreshes_once_per_interval_and_reports_the_filtered_source_key() {
    let now = Instant::now();
    let (mut view, mut rx) = picker(now);
    assert!(view.handle_paste("Deploy".into()));
    assert_eq!(view.selected_index(), Some(1));
    assert!(!view.pre_draw_tick(now + REFRESH_INTERVAL - Duration::from_nanos(1)));
    assert!(rx.try_recv().is_err());
    assert!(!view.pre_draw_tick(now + REFRESH_INTERVAL));
    let (thread_id, key, query, selected_index) = refresh(&mut rx);
    assert_eq!(thread_id, Some(ThreadId::from_u128(12)));
    assert_eq!(key.as_deref(), Some("deploy"));
    assert_eq!(query.as_deref(), Some("Deploy"));
    assert_eq!(selected_index, Some(1));
    view.pre_draw_tick(now + REFRESH_INTERVAL);
    view.pre_draw_tick(now + REFRESH_INTERVAL + Duration::from_millis(499));
    assert!(rx.try_recv().is_err());
    view.pre_draw_tick(now + REFRESH_INTERVAL * 2);
    assert_eq!(refresh(&mut rx).1.as_deref(), Some("deploy"));
    // A delayed frame emits one update, not a burst of missed polling intervals.
    view.pre_draw_tick(now + Duration::from_secs(30));
    refresh(&mut rx);
    assert!(rx.try_recv().is_err());
}

#[test]
fn parent_can_restore_query_and_stable_selection_after_rows_reorder() {
    let now = Instant::now();
    let (mut original, mut rx) = picker(now);
    original.handle_paste("Deploy".into());
    original.pre_draw_tick(now + REFRESH_INTERVAL);
    let (thread_id, key, query, _) = refresh(&mut rx);
    let keys = ["deploy", "review", "build"].map(str::to_owned).to_vec();
    let mut next = params(&["Deploy staging", "Review patch", "Build tests"]);
    next.initial_selected_idx = keys
        .iter()
        .position(|candidate| Some(candidate) == key.as_ref());
    next.initial_search_query = query;
    let (tx, _rx) = unbounded_channel();
    let replacement = TasksView::new_at(
        next,
        keys,
        thread_id,
        AppEventSender::new(tx),
        RuntimeKeymap::defaults().list,
        now,
    );
    assert_eq!(replacement.selected_index(), Some(0));
    assert_eq!(replacement.search_query(), Some("Deploy"));
}

#[test]
fn search_indexes_names_descriptions_categories_and_preserves_explicit_aliases() {
    for (query, expected) in [
        ("Build", Some(0)),
        ("verification", Some(0)),
        ("checklist", Some(0)),
        ("custom alias", Some(1)),
        ("Opaque", None),
    ] {
        let (tx, _rx) = unbounded_channel();
        let mut choices = params(&["Build tests", "Opaque row"]);
        choices.items[0].description = Some("Compile verification".into());
        choices.items[0].category_tag = Some("Checklist".into());
        choices.items[1].search_value = Some("custom alias".into());
        choices.initial_search_query = Some(query.into());
        let view = TasksView::new_at(
            choices,
            vec!["build".into(), "opaque".into()],
            None,
            AppEventSender::new(tx),
            RuntimeKeymap::defaults().list,
            Instant::now(),
        );
        // Selection is valid before any render or timer tick; paint is not a state sync.
        assert_eq!(view.selected_index(), expected, "{query}");
        assert_eq!(view.search_query(), Some(query));
    }
}

#[test]
fn queued_refresh_keeps_input_snapshot_so_parent_can_reject_stale_events() {
    let now = Instant::now();
    let (mut view, mut rx) = picker(now);
    view.pre_draw_tick(now + REFRESH_INTERVAL);
    view.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    view.handle_paste("Deploy".into());
    let (_, old_key, old_query, old_index) = refresh(&mut rx);
    assert_eq!(old_key.as_deref(), Some("build"));
    assert_eq!(old_index, Some(0));
    assert_eq!(old_query.as_deref(), Some(""));
    assert_ne!(old_index, view.selected_index());
    assert_ne!(old_query.as_deref(), view.search_query());
    view.pre_draw_tick(now + REFRESH_INTERVAL * 2);
    let (_, key, query, index) = refresh(&mut rx);
    assert_eq!(index, view.selected_index());
    assert_eq!(key.as_deref(), Some("deploy"));
    assert_eq!(query.as_deref(), Some("Deploy"));
}

#[test]
fn configured_keys_and_native_actions_stay_in_charge() {
    let now = Instant::now();
    let (tx, mut rx) = unbounded_channel();
    let mut keymap = RuntimeKeymap::defaults().list;
    keymap.move_down = vec![crate::key_hint::plain(KeyCode::F(6))];
    keymap.accept = vec![crate::key_hint::plain(KeyCode::F(7))];
    let mut choices = params(&["Build tests", "Deploy staging"]);
    choices.items[1].actions = vec![Box::new(|tx| {
        tx.send(AppEvent::UpdateModel("selected deploy".into()));
    })];
    let mut view = TasksView::new_at(
        choices,
        vec!["build".into(), "deploy".into()],
        None,
        AppEventSender::new(tx),
        keymap,
        now,
    );
    view.handle_key_event_with_activation_key(
        KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE),
        None,
    );
    assert_eq!(view.selected_index(), Some(1));
    view.handle_key_event(KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE));
    assert!(
        matches!(rx.try_recv(), Ok(AppEvent::UpdateModel(model)) if model == "selected deploy")
    );
    assert_eq!(view.completion(), Some(ViewCompletion::Accepted));
    view.pre_draw_tick(now + Duration::from_secs(1));
    assert!(rx.try_recv().is_err());
    assert!(view.next_frame_delay().is_none());
}

#[test]
fn cancellation_stops_refresh_and_the_picker_never_blocks_agent_work() {
    let now = Instant::now();
    for ctrl_c in [false, true] {
        let (mut view, mut rx) = picker(now);
        assert_eq!(view.view_id(), Some(VIEW_ID));
        assert!(view.accepts_input_when_disconnected());
        assert!(!view.terminal_title_requires_action());
        assert!(view.program_status_blocked_kind().is_none());
        assert!(view.prefer_esc_to_handle_key_event());
        if ctrl_c {
            assert!(matches!(view.on_ctrl_c(), CancellationEvent::Handled));
        } else {
            view.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
        assert_eq!(view.completion(), Some(ViewCompletion::Cancelled));
        view.pre_draw_tick(now + Duration::from_secs(1));
        assert!(rx.try_recv().is_err());
        assert!(view.next_frame_delay().is_none());
    }
}

#[test]
fn native_picker_rendering_stays_inside_the_allocated_rectangle() {
    let now = Instant::now();
    let (view, _rx) = picker(now);
    for width in [1, 12, 30, 80] {
        for height in [0, 1, 3, 8] {
            let area = Rect::new(3, 2, width, height);
            let bounds = Rect::new(0, 0, width + 6, height + 4);
            let mut buf = Buffer::empty(bounds);
            for cell in &mut buf.content {
                cell.set_symbol(".");
            }
            view.render(area, &mut buf);
            for y in bounds.y..bounds.bottom() {
                for x in bounds.x..bounds.right() {
                    if x < area.x || x >= area.right() || y < area.y || y >= area.bottom() {
                        assert_eq!(buf[(x, y)].symbol(), ".", "{width}x{height}: ({x}, {y})");
                    }
                }
            }
        }
    }
}
