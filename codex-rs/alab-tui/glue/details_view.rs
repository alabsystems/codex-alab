// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One local pager for account and task details. Measurement, scrolling and
//! rendering use the same wrapped paragraph; chrome yields before the content.
//! The native bottom pane retains drafts and owns key chords and cancellation.

use super::ChatWidget;
use super::interface;
use crate::app_event::AppEvent;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::CancellationEvent;
use crate::bottom_pane::ViewCompletion;
use crate::keymap::KeymapContext;
use crate::keymap::KeymapContextSet;
use crate::keymap::ListAction;
use crate::keymap::ListKeymap;
use crate::render::renderable::Renderable;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Margin;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;
use std::cell::Cell;

pub(super) struct DetailsView {
    view_id: &'static str,
    title: &'static str,
    dynamic_title: Option<Box<dyn Fn() -> &'static str>>,
    paragraph: Box<dyn Fn() -> Paragraph<'static>>,
    accept_label: &'static str,
    on_accept: Option<Box<dyn FnOnce()>>,
    on_refresh: Option<Box<dyn Fn()>>,
    keymap: ListKeymap,
    offset: Cell<usize>,
    page_size: Cell<usize>,
    max_offset: Cell<usize>,
    completion: Option<ViewCompletion>,
}

impl DetailsView {
    pub(super) fn new(
        view_id: &'static str,
        title: &'static str,
        paragraph: impl Fn() -> Paragraph<'static> + 'static,
        accept_label: &'static str,
        on_accept: Option<Box<dyn FnOnce()>>,
        keymap: ListKeymap,
    ) -> Self {
        Self {
            view_id,
            title,
            dynamic_title: None,
            paragraph: Box::new(paragraph),
            accept_label,
            on_accept,
            on_refresh: None,
            keymap,
            offset: Cell::new(0),
            page_size: Cell::new(1),
            max_offset: Cell::new(0),
            completion: None,
        }
    }

    /// Refresh the current view without completing it or resetting its scroll state.
    pub(super) fn with_refresh(mut self, refresh: impl Fn() + 'static) -> Self {
        self.on_refresh = Some(Box::new(refresh));
        self
    }

    /// Keep lifecycle notices visible while the independently scrolling body moves.
    pub(super) fn with_dynamic_title(mut self, title: impl Fn() -> &'static str + 'static) -> Self {
        self.dynamic_title = Some(Box::new(title));
        self
    }

    #[cfg(test)]
    pub(super) fn scroll_state(&self) -> (usize, usize, usize) {
        (
            self.offset.get(),
            self.page_size.get(),
            self.max_offset.get(),
        )
    }

    fn footer(&self) -> Paragraph<'static> {
        let mut spans = Vec::new();
        for (action, label) in [
            (ListAction::MoveDown, "scroll"),
            (ListAction::PageDown, "page"),
            (ListAction::Accept, self.accept_label),
            (ListAction::Cancel, "back"),
        ] {
            if let Some(hint) = self.keymap.primary_hint(action) {
                if !spans.is_empty() {
                    spans.push(" · ".dim());
                }
                spans.extend(hint.spans());
                spans.push(format!(" {label}").dim());
            }
        }
        Paragraph::new(Line::from(spans)).wrap(Wrap { trim: false })
    }
}

impl BottomPaneView for DetailsView {
    fn keymap_contexts(&self) -> KeymapContextSet {
        KeymapContextSet::new(KeymapContext::List)
    }

    fn accepts_input_when_disconnected(&self) -> bool {
        true
    }

    fn view_id(&self) -> Option<&'static str> {
        Some(self.view_id)
    }

    fn selected_index(&self) -> Option<usize> {
        Some(0)
    }

    fn handle_key_event(&mut self, event: KeyEvent) {
        if event.kind == KeyEventKind::Release || self.completion.is_some() {
            return;
        }
        let offset = self.offset.get().min(self.max_offset.get());
        let next = match self.keymap.action_for(event) {
            Some(ListAction::MoveUp) => offset.saturating_sub(1),
            Some(ListAction::MoveDown) => offset.saturating_add(1),
            Some(ListAction::PageUp) => offset.saturating_sub(self.page_size.get()),
            Some(ListAction::PageDown) => offset.saturating_add(self.page_size.get()),
            Some(ListAction::JumpTop) => 0,
            Some(ListAction::JumpBottom) => self.max_offset.get(),
            Some(ListAction::Accept) => {
                if let Some(refresh) = &self.on_refresh {
                    refresh();
                } else {
                    self.completion = Some(ViewCompletion::Accepted);
                    if let Some(on_accept) = self.on_accept.take() {
                        on_accept();
                    }
                }
                offset
            }
            Some(ListAction::Cancel) => {
                self.completion = Some(ViewCompletion::Cancelled);
                offset
            }
            _ => offset,
        };
        self.offset.set(next.min(self.max_offset.get()));
    }

    fn prefer_esc_to_handle_key_event(&self) -> bool {
        true
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.completion = Some(ViewCompletion::Cancelled);
        CancellationEvent::Handled
    }

    fn is_complete(&self) -> bool {
        self.completion.is_some()
    }

    fn completion(&self) -> Option<ViewCompletion> {
        self.completion
    }
}

impl Renderable for DetailsView {
    fn desired_height(&self, width: u16) -> u16 {
        let width = width.saturating_sub(4);
        (self.paragraph)()
            .desired_height(width)
            .saturating_add(self.footer().desired_height(width))
            .saturating_add(4)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        Widget::render(Clear, area, buf);
        Widget::render(
            Block::default().style(crate::style::user_message_style()),
            area,
            buf,
        );
        let inner = area.inner(Margin::new(2, 0));
        let footer = self.footer();
        let footer_height = footer
            .desired_height(inner.width)
            .min(inner.height.saturating_sub(3));
        let [title, _gap, body, accept, hints] = Layout::vertical([
            Constraint::Length(u16::from(inner.height >= 3)),
            Constraint::Length(u16::from(inner.height >= 5)),
            Constraint::Min(1),
            Constraint::Length(u16::from(inner.height >= 4)),
            Constraint::Length(footer_height),
        ])
        .areas(inner);
        let paragraph = (self.paragraph)();
        let content_height = paragraph.desired_height(body.width) as usize;
        let page_size = usize::from(body.height.max(1));
        self.page_size.set(page_size);
        self.max_offset
            .set(content_height.saturating_sub(page_size));
        let offset = self.offset.get().min(self.max_offset.get());
        self.offset.set(offset);
        let markers = match (offset > 0, offset < self.max_offset.get()) {
            (true, true) => " ↑↓",
            (true, false) => " ↑",
            (false, true) => " ↓",
            (false, false) => "",
        };
        let title_text = self
            .dynamic_title
            .as_ref()
            .map_or(self.title, |title| title());
        Widget::render(
            Line::from(format!("{title_text}{markers}")).bold(),
            title,
            buf,
        );
        Renderable::render(
            &paragraph.scroll((offset.min(u16::MAX as usize) as u16, 0)),
            body,
            buf,
        );
        Widget::render(
            Line::from(format!("› {}", self.accept_label)).style(crate::style::selection_style()),
            accept,
            buf,
        );
        Renderable::render(&footer, hints, buf);
    }
}

impl ChatWidget {
    /// A local snapshot of one native task; Back belongs to its displayed thread.
    pub(crate) fn alab_task_details_view(
        &self,
        title: &'static str,
        detail: String,
    ) -> Box<dyn BottomPaneView> {
        let tx = self.app_event_tx.clone();
        let thread_id = self.thread_id();
        Box::new(DetailsView::new(
            "alab-task-details",
            title,
            move || Paragraph::new(detail.clone()).wrap(Wrap { trim: false }),
            "Back to Tasks",
            Some(Box::new(move || {
                tx.send(AppEvent::AlabTasksBack { thread_id })
            })),
            self.bottom_pane.list_keymap(),
        ))
    }

    pub(crate) fn alab_show_task_details(&mut self, title: &'static str, detail: String) {
        if interface::enabled() {
            let view = self.alab_task_details_view(title, detail);
            self.show_bottom_pane_view(view);
        }
    }
}
