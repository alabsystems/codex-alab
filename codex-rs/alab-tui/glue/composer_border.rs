// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Edition chrome at the existing composer-border hook. This adapter maps Codex's
//! private terminal palette to ALab's renderer; all geometry and drawing live in
//! `codex_alab_tui::interface`. Unmodified Codex consumers leave every cell alone.

use crate::terminal_palette;
use crate::terminal_palette::StdoutColorLevel;
use codex_alab_tui::banner::Depth;
use codex_alab_tui::banner::Theme;
use codex_alab_tui::interface;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

fn palette(background: Option<Color>) -> (Depth, Theme) {
    let depth = match terminal_palette::effective_stdout_color_level() {
        StdoutColorLevel::TrueColor => Depth::TrueColor,
        StdoutColorLevel::Ansi256 => Depth::Ansi256,
        StdoutColorLevel::Ansi16 => Depth::Ansi16,
        StdoutColorLevel::Unknown => Depth::Mono,
    };
    let background = match background {
        Some(Color::Rgb(r, g, b)) => Some((r, g, b)),
        Some(Color::Indexed(index)) if index >= 16 => {
            Some(terminal_palette::XTERM_COLORS[usize::from(index)])
        }
        None | Some(Color::Reset) => terminal_palette::default_bg(),
        _ => None,
    };
    (depth, Theme::from_bg(background))
}

pub(super) fn render(composer: Rect, buf: &mut Buffer) {
    if interface::available() {
        // Keep the recovery switch reachable even while enhancements are off.
        // The native composer tints its fill before this hook runs. Use that
        // painted surface, including palette quantization, for text contrast.
        let visible = composer.intersection(buf.area);
        let background = buf.cell((visible.x, visible.y)).map(|cell| cell.bg);
        let (depth, theme) = palette(background);
        interface::render(composer, buf, depth, theme);
    }
}

pub(in crate::bottom_pane::chat_composer) fn render_status(area: Rect, buf: &mut Buffer) -> bool {
    if !interface::enabled() {
        return false;
    }
    let (depth, theme) = palette(None);
    let area = Rect {
        x: area.x.saturating_add(2).min(area.right()),
        width: area.width.saturating_sub(4),
        ..area
    };
    interface::render_status_current(area, buf, depth, theme);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bottom_pane::ChatComposer;
    use crate::bottom_pane::chat_composer::tests::new_test_composer;
    use crate::render::renderable::Renderable;
    use ratatui::buffer::Cell;

    fn buffer(composer: &ChatComposer) -> Buffer {
        let area = Rect::new(0, 0, 100, composer.desired_height(100));
        let mut buf = Buffer::empty(area);
        composer.render(area, &mut buf);
        buf
    }

    #[test]
    fn rainbow_rule_is_readable_on_the_painted_composer_surface() {
        use codex_alab_tui::banner::palette;
        use ratatui::style::Color;

        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(true);
        for background in [(16, 20, 24), (255, 255, 255), (120, 120, 120)] {
            terminal_palette::with_test_default_colors(
                crate::terminal_probe::DefaultColors {
                    fg: (222, 222, 222),
                    bg: background,
                },
                || {
                    for surface in [
                        crate::style::user_message_style().bg.unwrap(),
                        terminal_palette::indexed_color(236),
                        terminal_palette::indexed_color(254),
                    ] {
                        let area = Rect::new(5, 3, 100, 3);
                        let mut buf = Buffer::empty(area);
                        buf.set_style(area, ratatui::style::Style::default().bg(surface));
                        render(area, &mut buf);
                        let background = match surface {
                            Color::Rgb(r, g, b) => (r, g, b),
                            Color::Indexed(index) => {
                                terminal_palette::XTERM_COLORS[usize::from(index)]
                            }
                            _ => panic!("known composer background"),
                        };
                        for x in area.x + 2..area.right() - 2 {
                            let cell = &buf[(x, area.y)];
                            assert_eq!(cell.bg, surface);
                            if cell.symbol().trim().is_empty() {
                                continue;
                            }
                            let Color::Rgb(r, g, b) = cell.fg else {
                                panic!("true-color rainbow foreground");
                            };
                            let contrast = palette::contrast(
                                palette::pack((r, g, b)),
                                palette::pack(background),
                            );
                            assert!(
                                contrast >= palette::TEXT_CONTRAST_FLOOR,
                                "{contrast:.2}: {:?} on {background:?}",
                                cell.fg,
                            );
                        }
                    }
                },
            );
        }
    }

    #[test]
    fn edition_switch_preserves_native_composer_with_a_recovery_button() {
        let (composer, _rx) = new_test_composer();
        let native = buffer(&composer);
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(true);
        interface::set_status(interface::InterfaceStatus {
            account: "selected@example.test".into(),
            weekly: Some("weekly 82% left".into()),
            ..Default::default()
        });
        let on = buffer(&composer);
        let rendered: String = on.content.iter().map(Cell::symbol).collect();
        assert!(rendered.contains("codex-alab"));
        assert!(rendered.contains("selected@example.test"));
        assert!(rendered.contains("weekly 82% left"));
        interface::set_enabled(false);
        let off = buffer(&composer);
        let button = interface::button_rect(off.area).expect("recovery switch while off");
        let text: String = off.content.iter().map(Cell::symbol).collect();
        assert!(text.contains("[ALab off]"));
        assert!(!text.contains("selected@example.test"));
        assert!(!text.contains("codex-alab"));
        for y in native.area.y..native.area.bottom() {
            for x in native.area.x..native.area.right() {
                if !button.contains((x, y).into()) {
                    assert_eq!(native[(x, y)], off[(x, y)], "({x}, {y})");
                }
            }
        }
        assert!(interface::mouse_hit(button.x, button.y));
    }

    #[test]
    fn active_goal_reserves_a_row_without_moving_the_draft_or_stealing_account_hits() {
        let (mut composer, _rx) = new_test_composer();
        composer.set_text_content("A preserved draft".into(), Vec::new(), Vec::new());
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(true);
        interface::set_status(interface::InterfaceStatus {
            account: "selected@example.test".into(),
            weekly: Some("weekly 82% left".into()),
            ..Default::default()
        });
        let before = buffer(&composer);
        let cursor = composer.cursor_pos(before.area);
        interface::set_goal(Some("Compile Trust with Trust"));
        let with_goal = buffer(&composer);
        assert_eq!(with_goal.area.height, before.area.height + 1);
        assert_eq!(composer.cursor_pos(with_goal.area), cursor);
        assert_eq!(composer.current_text(), "A preserved draft");
        let row = |y| {
            (with_goal.area.x..with_goal.area.right())
                .map(|x| with_goal[(x, y)].symbol())
                .collect::<String>()
        };
        let goal_y = (with_goal.area.y..with_goal.area.bottom())
            .find(|y| row(*y).contains("Goal: Compile Trust with Trust"))
            .expect("the composer paints its reserved goal row");
        assert!(row(goal_y + 1).contains("selected@example.test"));
        let account_x = (with_goal.area.x..with_goal.area.right())
            .find(|x| with_goal[(*x, goal_y + 1)].symbol() == "s")
            .expect("account label");
        assert!(interface::account_mouse_hit(account_x, goal_y + 1));
        assert!(!interface::account_mouse_hit(account_x, goal_y));
        interface::set_goal(None);
        let cleared = buffer(&composer);
        assert_eq!(cleared.area, before.area);
        assert_eq!(composer.cursor_pos(cleared.area), cursor);
    }
}
