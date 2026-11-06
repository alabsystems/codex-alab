// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The live Tasks picker is the native selection list with a refresh clock. Task rows,
//! actions and their stable keys come from the app's actual task/session state; this
//! wrapper owns no task model and starts no background worker. Each refresh hands the app
//! the selected source key and current query, so rebuilding a changing list preserves
//! what the person is inspecting. The app rejects refreshes after newer input, a closed
//! view or a thread change.

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::CancellationEvent;
use crate::bottom_pane::ListSelectionView;
use crate::bottom_pane::SelectionToggleState;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::ViewCompletion;
use crate::bottom_pane::ViewPresentation;
use crate::key_hint::KeyBinding;
use crate::keymap::KeymapContextSet;
use crate::keymap::ListKeymap;
use crate::render::renderable::Renderable;
use codex_protocol::ThreadId;
use crossterm::cursor::SetCursorStyle;
use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

pub(crate) const VIEW_ID: &str = "alab-tasks";
const REFRESH_INTERVAL: Duration = Duration::from_millis(500);

/// Build a live, searchable native picker. `keys` parallels the flat `params.items` list;
/// the parent supplies native row callbacks, configured key hints and restored selection.
pub(crate) fn view(
    params: SelectionViewParams,
    keys: Vec<String>,
    thread_id: Option<ThreadId>,
    tx: AppEventSender,
    keymap: ListKeymap,
) -> Box<dyn BottomPaneView> {
    Box::new(TasksView::new_at(
        params,
        keys,
        thread_id,
        tx,
        keymap,
        Instant::now(),
    ))
}

struct TasksView {
    lifetime: Arc<()>,
    list: ListSelectionView,
    keys: Vec<String>,
    thread_id: Option<ThreadId>,
    tx: AppEventSender,
    next_refresh: Instant,
}

impl TasksView {
    fn new_at(
        mut params: SelectionViewParams,
        keys: Vec<String>,
        thread_id: Option<ThreadId>,
        tx: AppEventSender,
        keymap: ListKeymap,
        now: Instant,
    ) -> Self {
        params.view_id = Some(VIEW_ID);
        params.is_searchable = true;
        params.allow_input_when_disconnected = true;
        // Native lists search only this explicit field. Since this wrapper enables
        // search, give ordinary task rows an index before the constructor filters an
        // initial query; keep any caller-provided aliases or intentional exclusions.
        for item in &mut params.items {
            if item.search_value.is_none() {
                item.search_value = Some(format!(
                    "{} {} {}",
                    item.name,
                    item.description.as_deref().unwrap_or_default(),
                    item.category_tag.as_deref().unwrap_or_default(),
                ));
            }
        }
        Self {
            lifetime: Arc::new(()),
            list: ListSelectionView::new(params, tx.clone(), keymap),
            keys,
            thread_id,
            tx,
            next_refresh: now + REFRESH_INTERVAL,
        }
    }
}

impl BottomPaneView for TasksView {
    fn presentation(&self) -> ViewPresentation {
        self.list.presentation()
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        self.list.handle_key_event(key_event);
    }

    fn handle_key_event_with_activation_key(
        &mut self,
        key_event: KeyEvent,
        activation_key: Option<KeyBinding>,
    ) {
        self.list
            .handle_key_event_with_activation_key(key_event, activation_key);
    }

    fn handle_key_release(&mut self, key_event: KeyEvent) {
        self.list.handle_key_release(key_event);
    }

    fn keymap_contexts(&self) -> KeymapContextSet {
        self.list.keymap_contexts()
    }

    fn accepts_input_when_disconnected(&self) -> bool {
        self.list.accepts_input_when_disconnected()
    }

    fn is_complete(&self) -> bool {
        self.list.is_complete()
    }

    fn completion(&self) -> Option<ViewCompletion> {
        self.list.completion()
    }

    fn dismiss_after_child_accept(&self) -> bool {
        self.list.dismiss_after_child_accept()
    }

    fn clear_dismiss_after_child_accept(&mut self) {
        self.list.clear_dismiss_after_child_accept();
    }

    fn view_id(&self) -> Option<&'static str> {
        Some(VIEW_ID)
    }

    fn selected_index(&self) -> Option<usize> {
        self.list.selected_index()
    }

    fn selection_toggle_states(&self) -> Vec<Option<SelectionToggleState>> {
        self.list.selection_toggle_states()
    }

    fn complete_selection_toggle(&mut self, index: usize) {
        self.list.complete_selection_toggle(index);
    }

    fn search_query(&self) -> Option<&str> {
        BottomPaneView::search_query(&self.list)
    }

    fn active_tab_id(&self) -> Option<&str> {
        BottomPaneView::active_tab_id(&self.list)
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.list.on_ctrl_c()
    }

    fn prefer_esc_to_handle_key_event(&self) -> bool {
        self.list.prefer_esc_to_handle_key_event()
    }

    fn handle_paste(&mut self, pasted: String) -> bool {
        self.list.handle_paste(pasted)
    }

    fn pre_draw_tick(&mut self, now: Instant) -> bool {
        if !self.is_complete() && now >= self.next_refresh {
            self.next_refresh = now + REFRESH_INTERVAL;
            let selected_index = self.selected_index();
            self.tx.send(AppEvent::AlabTasksRefresh {
                view: Arc::downgrade(&self.lifetime),
                thread_id: self.thread_id,
                selected_index,
                selected_key: selected_index.and_then(|index| self.keys.get(index).cloned()),
                query: self.search_query().map(str::to_owned),
            });
        }
        // Refresh is an app event, not a local mutation or a plan approval tick.
        false
    }

    fn next_frame_delay(&self) -> Option<Duration> {
        (!self.is_complete()).then(|| self.next_refresh.saturating_duration_since(Instant::now()))
    }
}

impl Renderable for TasksView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.list.render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.list.desired_height(width)
    }

    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        self.list.render_scrolled(area, buf, scroll_offset)
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.list.cursor_pos(area)
    }

    fn cursor_style(&self, area: Rect) -> SetCursorStyle {
        self.list.cursor_style(area)
    }
}

#[cfg(test)]
#[path = "tasks_view_tests.rs"]
mod tests;
