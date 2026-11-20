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
    render_overline(composer, buf, cfg!(windows));
}

fn render_overline(composer: Rect, buf: &mut Buffer, windows: bool) {
    if interface::available() {
        // Keep the recovery switch reachable even while enhancements are off.
        // The native composer tints its fill before this hook runs. Use that
        // painted surface, including palette quantization, for text contrast.
        let visible = composer.intersection(buf.area);
        let background = buf.cell((visible.x, visible.y)).map(|cell| cell.bg);
        let (depth, theme) = palette(background);
        // A named surface belongs to the configured palette even when its RGB is unknown.
        let theme = if windows && matches!(background, None | Some(Color::Reset)) {
            theme.with_terminal_foreground_if_unreported()
        } else {
            theme
        };
        interface::render(composer, buf, depth, theme);
    }
}

pub(super) fn footer_layout(
    mut footer: super::FooterLayout,
    width: u16,
    unfocused: bool,
) -> super::FooterLayout {
    // The owning composer adjusts its allowance; passive panes keep their own status.
    // Native allocation still gives the draft and priority hints their space first.
    if let Some(status) = status_height(width, u16::MAX, unfocused) {
        footer.cap = footer.cap.max(status + footer.reserved_hints);
    }
    footer
}

pub(in crate::bottom_pane::chat_composer) fn status_height(
    width: u16,
    available: u16,
    unfocused: bool,
) -> Option<u16> {
    if !interface::enabled() || unfocused {
        return None;
    }
    if width <= 4 {
        return Some(0);
    }
    Some(interface::status_height().min(available))
}

pub(in crate::bottom_pane::chat_composer) fn render_status(
    area: Rect,
    buf: &mut Buffer,
    unfocused: bool,
) -> bool {
    if !interface::enabled() || unfocused {
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
    use crate::bottom_pane::chat_composer::ComposerRenderOptions;
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
    fn windows_terminal_respects_explicit_monochrome() {
        const PROBE: &str = "ALAB_TEST_WINDOWS_TERMINAL_DEPTH";
        if let Ok(expected) = std::env::var(PROBE) {
            let native = terminal_palette::effective_stdout_color_level();
            let depth = match expected.as_str() {
                "mono" => Depth::Mono,
                "ansi16" => Depth::Ansi16,
                "truecolor" => Depth::TrueColor,
                _ => panic!("unknown test case {expected}"),
            };
            codex_alab_core::activate();
            assert_eq!(palette(None).0, depth);
            if depth == Depth::Mono {
                assert_eq!(native, StdoutColorLevel::TrueColor);
                assert_eq!(
                    terminal_palette::effective_stdout_color_level(),
                    StdoutColorLevel::Unknown
                );
                terminal_palette::with_test_default_colors(None, || {
                    assert_eq!(palette(None).0, Depth::TrueColor)
                });
            }
            return;
        }

        // The native color detector caches stdout, so give each case a fresh process.
        let thread = std::thread::current();
        for (term, no_color, force_color, expected) in [
            ("xterm", "1", None, "mono"),
            ("dumb", "0", None, "mono"),
            ("xterm", "0", None, "truecolor"),
            ("dumb", "1", Some("1"), "ansi16"),
        ] {
            let mut command =
                std::process::Command::new(std::env::current_exe().expect("test binary"));
            command
                .args(["--exact", thread.name().expect("test name")])
                .env(PROBE, expected)
                .env("WT_SESSION", "test")
                .env("TERM", term)
                .env("NO_COLOR", no_color)
                .env("IGNORE_IS_TERMINAL", "1")
                .env_remove("CLICOLOR_FORCE")
                .env_remove("FORCE_COLOR");
            if let Some(value) = force_color {
                command.env("FORCE_COLOR", value);
            }
            let output = command.output().expect("run fresh palette probe");
            let report = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && report.contains("1 passed"),
                "{expected}: {report}"
            );
        }
    }

    #[test]
    fn native_monochrome_composer_marks_the_selected_mode_in_plain_text() {
        use codex_alab_tui::session::HarnessWord;
        use codex_alab_tui::session::Shared;
        use ratatui::style::Modifier;
        use unicode_width::UnicodeWidthStr;

        const CHILD: &str = "ALAB_TEST_MONO_COMPOSER_MODES";
        if std::env::var_os(CHILD).is_none() {
            // The cached stdout detector must see each terminal configuration at startup.
            for (term, program, mux) in [
                ("xterm-256color", "aterm", false),
                ("xterm-256color", "aterm", true),
                ("dumb", "Apple_Terminal", false),
            ] {
                let mut command =
                    std::process::Command::new(std::env::current_exe().expect("test binary"));
                command
                    .args(["--exact", std::thread::current().name().expect("test name")])
                    .env(
                        CHILD,
                        if program == "aterm" && !mux {
                            "clock"
                        } else {
                            "glyph"
                        },
                    )
                    .env("TERM_PROGRAM", program)
                    .env("TERM", term)
                    .env("IGNORE_IS_TERMINAL", "1")
                    .env_remove("NO_COLOR")
                    .env_remove("COLORTERM")
                    .env_remove("CLICOLOR_FORCE")
                    .env_remove("FORCE_COLOR")
                    .env_remove("WT_SESSION");
                for key in [
                    "TMUX",
                    "TMUX_PANE",
                    "STY",
                    "ZELLIJ",
                    "ZELLIJ_SESSION_NAME",
                    "ZELLIJ_VERSION",
                ] {
                    command.env_remove(key);
                }
                if mux {
                    command.env("TMUX_PANE", "%1");
                }
                if term == "xterm-256color" {
                    command.env("NO_COLOR", "1");
                }
                let output = command.output().expect("run native monochrome composer");
                let report = String::from_utf8_lossy(&output.stdout);
                assert!(
                    output.status.success() && report.contains("1 passed"),
                    "{term}: {report}"
                );
            }
            return;
        }
        assert_eq!(palette(None).0, Depth::Mono);
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        let shared = Box::leak(Box::new(Shared::new()));
        let _automation = codex_alab_tui::boxes::scoped(shared);
        interface::configure(true);
        let release = interface::session_title().expect("edition title");
        let pair_width = release.last().expect("release pair").width() as u16;
        let (composer, _rx) = new_test_composer();
        let modes = [
            (HarnessWord::Cruise, "Cruise"),
            (HarnessWord::Land, "Land"),
            (HarnessWord::Off, "Off"),
        ];

        let compact = [
            "[Cruise]Land│Off▕",
            "▏Cruise[Land]Off▕",
            "▏Cruise│Land[Off]",
        ];
        let padded = [
            "[ Cruise ] Land │ Off ▕",
            "▏ Cruise [ Land ] Off ▕",
            "▏ Cruise │ Land [ Off ]",
        ];
        for (extra_width, padding, tracks) in [(29, 0, compact), (40, 0, compact), (46, 2, padded)]
        {
            let width = pair_width + extra_width;
            for ((selected, _), track) in modes.into_iter().zip(tracks) {
                shared.set_harness_word(selected);
                shared.set_land_at(Some(1_811_898_000));
                let area = Rect::new(5, 3, width, composer.desired_height(width));
                let mut buf = Buffer::empty(area);
                composer.render(area, &mut buf);
                let y = composer
                    .layout_with_options(area, ComposerRenderOptions::default())
                    .composer
                    .y;
                let row: String = (area.x..area.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect();
                let compact_clock = selected == HarnessWord::Cruise
                    && extra_width == 29
                    && cfg!(target_os = "macos")
                    && std::env::var(CHILD).as_deref() == Ok("clock");
                assert_eq!(row.contains('◷'), compact_clock, "{row}");
                if selected == HarnessWord::Cruise {
                    let symbol = if compact_clock { '◷' } else { '∞' };
                    let (before, _) = row.split_once(symbol).expect("Cruise status face");
                    let x = area.x + before.width() as u16;
                    assert_eq!(interface::mode_mouse_hit(x, y), Some(HarnessWord::Land));
                    assert!(interface::autopilot_menu_hit(x, y));
                    assert_eq!(row.contains("∞ ⤓"), extra_width >= 40, "{row}");
                }
                if selected == HarnessWord::Off {
                    assert_eq!(row.contains("○ off"), extra_width >= 40, "{row}");
                }
                let (prefix, _) = row
                    .split_once(track)
                    .unwrap_or_else(|| panic!("missing {track:?} in {row:?}"));
                let mut x = area.x + prefix.width() as u16;
                let check_separator = |x| {
                    assert!(interface::autopilot_menu_hit(x, y));
                    assert_eq!(interface::mode_mouse_hit(x, y), None);
                    assert!(!interface::autopilot_words_hit(x, y));
                    assert!(!interface::build_mouse_hit(x, y));
                    assert!(!interface::mouse_hit(x, y));
                };
                for (mode, word) in modes {
                    check_separator(x);
                    x += 1;
                    let end = x + word.width() as u16 + padding;
                    for column in x..end {
                        assert_eq!(interface::mode_mouse_hit(column, y), Some(mode));
                        let cell = &buf[(column, y)];
                        assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset));
                        assert!(!cell.modifier.contains(Modifier::DIM));
                        assert_eq!(cell.modifier.contains(Modifier::BOLD), selected == mode);
                        assert_eq!(cell.modifier.contains(Modifier::REVERSED), selected == mode);
                    }
                    x = end;
                }
                check_separator(x);
            }
        }
    }

    #[test]
    fn native_ansi16_composer_preserves_label_ink_and_surface() {
        const CHILD: &str = "ALAB_TEST_ANSI16_COMPOSER_LABELS";
        if std::env::var_os(CHILD).is_none() {
            // Stdout depth is process-cached; the scoped default-color fixture forces truecolor.
            let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args(["--exact", std::thread::current().name().expect("test name")])
                .env(CHILD, "1")
                .env("FORCE_COLOR", "1")
                .env("TERM", "dumb")
                .env_remove("COLORTERM")
                .output()
                .expect("run fresh ANSI16 composer probe");
            let report = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && report.contains("1 passed"),
                "{report}"
            );
            return;
        }

        assert_eq!(
            terminal_palette::effective_stdout_color_level(),
            StdoutColorLevel::Ansi16
        );
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(true);
        let area = Rect::new(5, 3, 100, 3);
        codex_alab_tui::upgrade::update_pending("Update available");
        let light = terminal_palette::indexed_color(255);
        let dark = terminal_palette::indexed_color(235);
        // Named slots have no known RGB value; the terminal owns their palette.
        for (on, surface, ink) in [
            (true, light, Color::Reset),
            (true, dark, Color::Reset),
            (true, Color::Blue, Color::Reset),
            (false, light, Color::Reset),
            (false, dark, Color::LightCyan),
            (false, Color::Blue, Color::LightCyan),
        ] {
            interface::set_enabled(on);
            let mut buf = Buffer::empty(area);
            buf.set_style(
                area,
                ratatui::style::Style::default()
                    .fg(Color::Yellow)
                    .bg(surface),
            );
            render(area, &mut buf);
            let mut build = String::new();
            let mut switch = String::new();
            for x in area.x..area.right() {
                let cell = &buf[(x, area.y)];
                if interface::build_mouse_hit(x, area.y) {
                    build.push_str(cell.symbol());
                } else if interface::mouse_hit(x, area.y) {
                    switch.push_str(cell.symbol());
                } else {
                    continue;
                }
                assert_eq!(cell.fg, ink, "column {x} on {surface:?}");
                assert_eq!(cell.bg, surface, "column {x}");
                assert!(cell.modifier.contains(ratatui::style::Modifier::BOLD));
            }
            assert_eq!(switch, if on { "[ALab on]" } else { "[ALab off]" });
            if on {
                assert!(build.contains(" on Codex "), "{build}");
                assert!(build.ends_with(" ↑ update"), "{build}");
            } else {
                assert!(build.is_empty());
            }
        }
    }

    #[test]
    fn only_an_unreported_windows_default_surface_uses_terminal_ink_for_composer_labels() {
        use codex_alab_tui::session::HarnessWord;
        use codex_alab_tui::session::Shared;
        use ratatui::style::Modifier;
        use ratatui::style::Style;

        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        let shared = Box::leak(Box::new(Shared::new()));
        let _automation = codex_alab_tui::boxes::scoped(shared);
        interface::configure(true);
        let draw = |area: Rect, surface, windows| {
            let mut buf = Buffer::empty(area);
            buf.set_style(
                area,
                Style::default()
                    .fg(Color::Green)
                    .bg(surface)
                    .add_modifier(Modifier::DIM),
            );
            render_overline(area, &mut buf, windows);
            buf
        };
        for reported in [None, Some((255, 255, 255)), Some((16, 20, 24))] {
            let colors = reported.map(|bg| crate::terminal_probe::DefaultColors {
                fg: (32, 33, 36),
                bg,
            });
            terminal_palette::with_test_default_colors(colors, || {
                for surface in [
                    Color::Reset,
                    Color::Blue,
                    terminal_palette::indexed_color(5),
                    terminal_palette::indexed_color(254),
                    terminal_palette::rgb_color((235, 240, 245)),
                ] {
                    for (width, enabled, word) in [
                        (80, true, HarnessWord::Land),
                        (28, true, HarnessWord::Off),
                        (80, false, HarnessWord::Land),
                    ] {
                        interface::set_enabled(enabled);
                        shared.set_harness_word(word);
                        let area = Rect::new(5, 3, width, 3);
                        let mut expected = draw(area, surface, false);
                        let actual = draw(area, surface, true);
                        if reported.is_none() && surface == Color::Reset {
                            let mut labels = 0;
                            let mut compact_controls = String::new();
                            for x in area.x..area.right() {
                                let cell = &mut expected[(x, area.y)];
                                if interface::build_mouse_hit(x, area.y)
                                    || interface::mouse_hit(x, area.y)
                                {
                                    labels += 1;
                                    assert!(cell.modifier.contains(Modifier::BOLD));
                                } else if interface::autopilot_menu_hit(x, area.y)
                                    && cell.bg == Color::Reset
                                {
                                    // The face and its arrow sit on the terminal surface. The track paints its own.
                                    if width == 28 {
                                        compact_controls.push_str(cell.symbol());
                                    }
                                } else {
                                    continue;
                                }
                                assert_ne!(cell.fg, Color::Reset);
                                cell.set_fg(Color::Reset);
                                cell.modifier.remove(Modifier::DIM);
                            }
                            assert!(labels > 0);
                            if width == 28 {
                                assert_eq!(compact_controls, "○▾");
                            }
                        }
                        assert_eq!(
                            actual, expected,
                            "{reported:?} {surface:?} enabled={enabled} width={width}"
                        );
                    }
                }
            });
        }
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
        let title = interface::session_title().expect("enhanced session title");
        let release = title.last().expect("release pair").content.as_ref();
        assert!(release.contains(" on Codex "));
        assert!(rendered.contains(release));
        assert!(rendered.contains("selected@example.test"));
        assert!(rendered.contains("weekly 82% left"));
        interface::set_enabled(false);
        let off = buffer(&composer);
        let button = interface::button_rect(off.area).expect("recovery switch while off");
        let text: String = off.content.iter().map(Cell::symbol).collect();
        assert!(text.contains("[ALab off]"));
        assert!(!text.contains("selected@example.test"));
        assert!(!text.contains(release));
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

    #[test]
    fn goal_account_rows_follow_footer_layout_with_constrained_draft_and_focus() {
        let (mut composer, _rx) = new_test_composer();
        composer.set_text_content("a\nb\nc".into(), Vec::new(), Vec::new());
        composer.move_cursor_to_end();
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(true);
        interface::set_status(interface::InterfaceStatus {
            account: "selected@example.test".into(),
            weekly: Some("weekly 82% left".into()),
            ..Default::default()
        });
        interface::set_goal(Some("Keep the draft visible"));

        for separate_status_line in [false, true] {
            for (height, status_rows) in [(5, 0), (6, 0), (7, 1), (8, 2), (10, 2)] {
                let area = Rect::new(4, 3, 100, height);
                let options = ComposerRenderOptions {
                    separate_status_line,
                    max_height: Some(height),
                    ..Default::default()
                };
                let layout = composer.layout_with_options(area, options);
                let mut buf = Buffer::empty(area);
                composer.render_with_options(area, &mut buf, None, options);
                assert_eq!(layout.status.height, status_rows);
                assert!(layout.textarea.height >= 3);
                let draft =
                    [0, 1, 2].map(|dy| buf[(layout.textarea.x, layout.textarea.y + dy)].symbol());
                assert_eq!(draft, ["a", "b", "c"]);
                assert_eq!(
                    composer.cursor_pos_with_options(area, options),
                    Some((layout.textarea.x + 1, layout.textarea.y + 2))
                );
                let rows: Vec<String> = (layout.status.y..layout.status.bottom())
                    .map(|y| {
                        (area.x..area.right())
                            .map(|x| buf[(x, y)].symbol())
                            .collect()
                    })
                    .collect();
                if status_rows > 0 {
                    assert!(rows[0].contains("Goal: Keep the draft visible"));
                }
                if status_rows == 2 {
                    assert!(rows[1].contains("selected@example.test"));
                    let account_x = (area.x..area.right())
                        .find(|x| buf[(*x, layout.status.y + 1)].symbol() == "s")
                        .expect("account label");
                    assert!(interface::account_mouse_hit(account_x, layout.status.y + 1));
                    assert!(!interface::account_mouse_hit(account_x, layout.status.y));
                } else {
                    assert!(!area.rows().any(|row| {
                        (row.x..row.right()).any(|x| interface::account_mouse_hit(x, row.y))
                    }));
                }
            }
        }
        let find = crate::bottom_pane::TranscriptFooter {
            text: vec![
                ratatui::text::Line::from("Find: needle"),
                ratatui::text::Line::from("Enter next · Esc close"),
            ]
            .into(),
            cursor_column: Some(8),
            is_interactive: true,
        };
        for (height, status_rows, hint_rows) in [(6, 0, 1), (7, 0, 2), (8, 1, 2), (9, 2, 2)] {
            let area = Rect::new(4, 3, 100, height);
            let options = ComposerRenderOptions {
                separate_status_line: true,
                max_height: Some(height),
                footer: Some(&find),
                ..Default::default()
            };
            let layout = composer.layout_with_options(area, options);
            let mut buf = Buffer::empty(area);
            composer.render_with_options(area, &mut buf, None, options);
            assert_eq!(
                (layout.status.height, layout.footer.height),
                (status_rows, hint_rows)
            );
            let draft =
                [0, 1, 2].map(|dy| buf[(layout.textarea.x, layout.textarea.y + dy)].symbol());
            assert_eq!(draft, ["a", "b", "c"]);
            assert_eq!(buf[(layout.footer.x + 2, layout.footer.y)].symbol(), "F");
            assert_eq!(
                composer.cursor_pos_with_options(area, options),
                Some((layout.footer.x + 10, layout.footer.y))
            );
            if hint_rows == 2 {
                let row: String = (area.x..area.right())
                    .map(|x| buf[(x, layout.footer.y + 1)].symbol())
                    .collect();
                assert!(row.contains("Enter next · Esc close"));
            }
            let status: String = (layout.status.y..layout.status.bottom())
                .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
                .map(|position| buf[position].symbol())
                .collect();
            assert_eq!(
                status.contains("Goal: Keep the draft visible"),
                status_rows > 0
            );
            assert_eq!(status.contains("selected@example.test"), status_rows == 2);
            let account_y = layout.status.y + 1;
            if status_rows == 2 {
                let account_x = (area.x..area.right())
                    .find(|x| buf[(*x, account_y)].symbol() == "s")
                    .expect("account label below the goal");
                assert!(interface::account_mouse_hit(account_x, account_y));
                assert!(!interface::account_mouse_hit(account_x, layout.footer.y));
            }
        }
        assert_eq!(composer.current_text(), "a\nb\nc");
    }

    #[test]
    fn passive_footer_keeps_its_native_status_without_replacing_focused_account_hits() {
        let (mut composer, _rx) = new_test_composer();
        composer.set_text_content("draft".into(), Vec::new(), Vec::new());
        composer.set_status_line_enabled(true);
        let status: crate::ui_tree::UiTree =
            r#""passive one\npassive two\npassive three""#.parse().unwrap();
        composer.set_status_line(Some(status.into()));
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(true);
        interface::set_status(interface::InterfaceStatus {
            account: "selected@example.test".into(),
            ..Default::default()
        });
        interface::set_goal(Some("Focused objective"));
        let focused_area = Rect::new(4, 2, 100, 8);
        let focused_options = ComposerRenderOptions::default();
        let focused_layout = composer.layout_with_options(focused_area, focused_options);
        let mut focused = Buffer::empty(focused_area);
        composer.render_with_options(focused_area, &mut focused, None, focused_options);
        let account_y = focused_layout.status.y + 1;
        let account_x = (focused_area.x..focused_area.right())
            .find(|x| focused[(*x, account_y)].symbol() == "s")
            .expect("focused account label");
        assert!(interface::account_mouse_hit(account_x, account_y));

        let passive_area = Rect::new(110, 2, 40, 20);
        let passive_options = ComposerRenderOptions {
            separate_status_line: true,
            max_height: Some(passive_area.height),
            unfocused: true,
            ..Default::default()
        };
        let layout = composer.layout_with_options(passive_area, passive_options);
        assert_eq!(layout.status.height, 3);
        let mut passive = Buffer::empty(passive_area);
        composer.render_with_options(passive_area, &mut passive, None, passive_options);
        let text: String = passive.content.iter().map(Cell::symbol).collect();
        for line in ["passive one", "passive two", "passive three"] {
            assert!(text.contains(line), "{text}");
        }
        assert!(!text.contains("Focused objective"));
        assert!(!text.contains("selected@example.test"));
        assert!(interface::account_mouse_hit(account_x, account_y));
        for y in layout.status.y..layout.status.bottom() {
            assert!(
                passive[(layout.status.x + 2, y)]
                    .modifier
                    .contains(ratatui::style::Modifier::DIM)
            );
        }

        // Native content can change while ALab owns the focused status. Toggling
        // the edition must use its current layout rather than an old measurement.
        composer.set_status_line(Some(
            ratatui::text::Line::from("updated native status").into(),
        ));
        interface::set_enabled(false);
        let layout = composer.layout_with_options(passive_area, passive_options);
        assert_eq!(layout.status.height, 1);
        let mut native = Buffer::empty(passive_area);
        composer.render_with_options(passive_area, &mut native, None, passive_options);
        let text: String = native.content.iter().map(Cell::symbol).collect();
        assert!(text.contains("updated native status"));
        assert!(!text.contains("passive one"));
        interface::set_enabled(true);
        composer.render_with_options(focused_area, &mut focused, None, focused_options);
        assert!(interface::account_mouse_hit(account_x, account_y));
        assert_eq!(composer.current_text(), "draft");
    }
}
