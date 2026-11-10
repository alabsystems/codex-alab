// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The ambient pet occupies its own column beside the entire bottom pane. The same
//! geometry controls measurement, text rendering and the cursor; the composer's
//! existing mouse hit targets therefore use the text rectangle too. History keeps
//! the full terminal width because the image never rises above this dock. A visible image
//! schedules its next animation frame after the fit check; hidden docks need no redraw timer.

use super::ChatWidget;
use crate::pets::AmbientPet;
use crate::pets::AmbientPetDraw;
use crate::render::renderable::Renderable;
use crate::render::renderable::RenderableItem;
use crossterm::cursor::SetCursorStyle;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;

#[cfg(test)]
impl ChatWidget {
    pub(crate) fn alab_pet_picker_preview_visible_for_test(&self) -> bool {
        self.pet_picker_preview_image_visible.get()
    }
}

const MIN_TEXT_COLUMNS: u16 = 72;
const GAP_COLUMNS: u16 = 2;
// Upstream leaves one row below the sprite and allows two notification rows.
const VERTICAL_MARGIN: u16 = 3;

#[derive(Clone, Copy)]
struct Dock {
    columns: u16,
    rows: u16,
}

impl Dock {
    fn text_width(self, width: u16) -> u16 {
        if width.saturating_sub(self.columns.saturating_add(GAP_COLUMNS)) >= MIN_TEXT_COLUMNS {
            width.saturating_sub(self.columns.saturating_add(GAP_COLUMNS))
        } else {
            width
        }
    }

    fn split(self, area: Rect) -> (Rect, Option<Rect>) {
        let width = self.text_width(area.width);
        if width == area.width || area.height < self.rows {
            return (area, None);
        }
        (
            Rect { width, ..area },
            Some(Rect::new(
                area.right().saturating_sub(self.columns),
                area.y,
                self.columns,
                area.height,
            )),
        )
    }
}

/// Preserve native pre-draw timing while the ALab dock schedules only visible images.
pub(super) fn schedule_before_draw(pet: &AmbientPet) {
    if !codex_alab_tui::interface::enabled() {
        pet.schedule_next_frame();
    }
}

fn dock(chat: &ChatWidget) -> Option<Dock> {
    if !codex_alab_tui::interface::enabled()
        || !chat.bottom_pane.no_modal_or_popup_active()
        || chat.fork_in_progress
        || chat.external_writer_view
    {
        return None;
    }
    let pet = chat.ambient_pet.as_ref()?;
    let request = pet.draw_request(Rect::new(0, 0, u16::MAX, u16::MAX), u16::MAX)?;
    Some(Dock {
        columns: request.columns,
        rows: request.rows.saturating_add(VERTICAL_MARGIN),
    })
}

pub(super) fn wrap<'a>(chat: &ChatWidget, child: RenderableItem<'a>) -> RenderableItem<'a> {
    match dock(chat) {
        Some(dock) => RenderableItem::Owned(Box::new(DockedPane { child, dock })),
        None => child,
    }
}

pub(super) fn draw(chat: &ChatWidget, area: Rect, bottom_y: u16) -> Option<AmbientPetDraw> {
    let dock = dock(chat)?;
    // Only the last rows of the pane can contain the sprite. Keeping the clear
    // boundary here also prevents sixel cleanup from erasing full-width history.
    let bottom_y = bottom_y.min(area.bottom());
    let dock_area = Rect::new(
        area.x,
        bottom_y.saturating_sub(dock.rows).max(area.y),
        area.width,
        bottom_y.saturating_sub(area.y).min(dock.rows),
    );
    let (_, image_area) = dock.split(dock_area);
    let image_area = image_area?;
    let pet = chat.ambient_pet.as_ref()?;
    let mut request = pet.draw_request(image_area, image_area.bottom())?;
    let image = Rect::new(request.x, request.y, request.columns, request.rows);
    if image.intersection(image_area) != image {
        return None;
    }
    request.clear_top_y = image_area.y;
    pet.schedule_next_frame();
    Some(request)
}

struct DockedPane<'a> {
    child: RenderableItem<'a>,
    dock: Dock,
}

impl Renderable for DockedPane<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let (text, image) = self.dock.split(area);
        if image.is_some() {
            Clear.render(
                Rect::new(text.right(), area.y, area.width - text.width, area.height),
                buf,
            );
        }
        self.child.render(text, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let text_width = self.dock.text_width(width);
        let height = self.child.desired_height(text_width);
        if text_width < width {
            height.max(self.dock.rows)
        } else {
            height
        }
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.child.cursor_pos(self.dock.split(area).0)
    }

    fn cursor_style(&self, area: Rect) -> SetCursorStyle {
        self.child.cursor_style(self.dock.split(area).0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bottom_pane::ComposerRenderOptions;
    use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
    use codex_alab_tui::interface;
    use pretty_assertions::assert_eq;

    fn scope(enabled: bool) -> interface::Scope {
        let state = Box::leak(Box::new(interface::State::new()));
        let scope = interface::scoped(state);
        interface::configure(enabled);
        scope
    }

    fn enable_pet(chat: &mut ChatWidget) {
        chat.set_pet_image_support_for_tests(crate::pets::PetImageSupport::Supported(
            crate::pets::ImageProtocol::Kitty,
        ));
        chat.install_test_ambient_pet_for_tests(false);
    }

    fn assert_blank(buf: &Buffer, area: Rect) {
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                assert_eq!(buf[(x, y)].symbol(), " ", "unexpected text at {x},{y}");
            }
        }
    }

    #[test]
    fn dock_geometry_preserves_text_before_hiding_or_clipping_the_image() {
        let dock = Dock {
            columns: 9,
            rows: 8,
        };
        for width in 0..150 {
            for height in 0..20 {
                let area = Rect::new(5, 7, width, height);
                let (text, image) = dock.split(area);
                assert_eq!(text.intersection(area), text);
                assert_eq!(text.height, area.height);
                if let Some(image) = image {
                    assert!(text.width >= MIN_TEXT_COLUMNS);
                    assert_eq!(image.right(), area.right());
                    assert_eq!(image.intersection(area), image);
                    assert!(text.right() + GAP_COLUMNS <= image.left());
                    assert_eq!(text.intersection(image).width, 0);
                } else {
                    assert_eq!(text, area);
                }
            }
        }
    }

    #[tokio::test]
    async fn multiline_prompt_cursor_and_footer_stay_outside_the_image() {
        let _scope = scope(true);
        let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
        enable_pet(&mut chat);
        chat.bottom_pane.set_composer_text(
            format!(
                "{}\nsecond logical line\nlast line",
                "long prompt words ".repeat(16)
            ),
            Vec::new(),
            Vec::new(),
        );
        // A native screen-bottom preference must not move the pet out of its dock.
        chat.local_settings.tui.pet_anchor = codex_config::types::TuiPetAnchor::ScreenBottom;
        let pane = chat.bottom_pane_renderable(ComposerRenderOptions::default());
        let width = 100;
        let height = pane.desired_height(width);
        let area = Rect::new(5, 7, width, height);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);
        let dock = dock(&chat).expect("supported ambient pet");
        let (text, image) = dock.split(area);
        let image = image.expect("wide enough for the dock");
        assert_blank(
            &buf,
            Rect::new(text.right(), area.y, area.width - text.width, height),
        );
        let cursor = pane.cursor_pos(area).expect("editable prompt cursor");
        assert!(text.contains(cursor.into()));
        let request = draw(
            &chat,
            Rect::new(5, 0, width, area.bottom() + 10),
            area.bottom(),
        )
        .expect("sprite inside the reserved pane");
        let sprite = Rect::new(request.x, request.y, request.columns, request.rows);
        assert_eq!(sprite.intersection(image), sprite);
        assert!(request.clear_top_y >= area.top());
        assert_eq!(chat.history_wrap_width(width), width);
        drop(pane);

        let click = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: cursor.0 - 1,
            row: cursor.1,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        assert!(chat.bottom_pane.prepare_composer_mouse(click));
        assert!(chat.bottom_pane.handle_composer_mouse(click));
        chat.bottom_pane.insert_str("X");
        assert!(chat.bottom_pane.composer_text().ends_with("last linXe"));
    }

    #[tokio::test]
    async fn narrow_or_short_terminals_hide_pet_and_keep_full_text_width() {
        let _scope = scope(true);
        let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
        enable_pet(&mut chat);
        let dock = dock(&chat).unwrap();
        assert!(
            chat.bottom_pane_renderable(ComposerRenderOptions::default())
                .desired_height(100)
                >= dock.rows
        );
        for area in [Rect::new(0, 0, 80, 24), Rect::new(0, 0, 100, dock.rows - 1)] {
            assert!(draw(&chat, area, area.bottom()).is_none());
            assert_eq!(dock.split(area).0, area);
            assert_eq!(chat.history_wrap_width(area.width), area.width);
        }
    }

    #[tokio::test]
    async fn hidden_pet_does_not_schedule_frames_and_visible_pet_resumes() {
        let _scope = scope(true);
        let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
        let (requester, mut frames) = crate::tui::FrameRequester::test_channel();
        chat.ambient_pet = Some(crate::pets::test_ambient_pet(requester, true));

        chat.pre_draw_tick();
        assert!(
            frames.try_recv().is_err(),
            "the pre-draw tick does not know whether the dock fits"
        );
        for area in [Rect::new(0, 0, 80, 24), Rect::new(0, 0, 100, 7)] {
            assert!(draw(&chat, area, area.bottom()).is_none());
            assert!(
                frames.try_recv().is_err(),
                "a hidden pet must not keep redrawing"
            );
        }

        let area = Rect::new(0, 0, 100, 24);
        chat.fork_in_progress = true;
        assert!(draw(&chat, area, area.bottom()).is_none());
        assert!(frames.try_recv().is_err());
        chat.fork_in_progress = false;
        assert!(draw(&chat, area, area.bottom()).is_some());
        assert!(
            frames.try_recv().is_ok(),
            "showing the pet restarts its timer"
        );
        assert!(frames.try_recv().is_err(), "one timer per visible frame");

        chat.ambient_pet
            .as_mut()
            .unwrap()
            .set_animations_enabled(false);
        assert!(draw(&chat, area, area.bottom()).is_some());
        assert!(frames.try_recv().is_err(), "reduced motion stays static");

        chat.ambient_pet
            .as_mut()
            .unwrap()
            .set_animations_enabled(true);
        interface::set_enabled(false);
        chat.pre_draw_tick();
        assert!(
            frames.try_recv().is_ok(),
            "the native layout keeps its own scheduling"
        );
    }

    #[tokio::test]
    async fn interface_off_keeps_native_rendering_and_pet_reserves() {
        let _scope = scope(false);
        let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
        enable_pet(&mut chat);
        let native = chat
            .bottom_pane
            .backdrop_with_options(ComposerRenderOptions::default());
        let width = 100;
        let area = Rect::new(0, 0, width, native.desired_height(width));
        let mut expected = Buffer::empty(area);
        native.render(area, &mut expected);
        let wrapped = wrap(&chat, native);
        let mut actual = Buffer::empty(area);
        wrapped.render(area, &mut actual);
        assert_eq!(actual, expected);
        assert_eq!(wrapped.desired_height(width), area.height);
        assert!(chat.history_wrap_width(width) < width);
        assert!(
            chat.ambient_pet_draw(Rect::new(0, 0, 100, 24), 24)
                .is_some()
        );
    }
}
