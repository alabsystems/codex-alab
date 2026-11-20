// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use unicode_width::UnicodeWidthChar;

fn text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn row(buf: &Buffer, y: u16) -> String {
    (buf.area.x..buf.area.right())
        .map(|x| buf[(x, y)].symbol())
        .collect()
}

#[test]
fn active_goal_has_its_own_row_and_preserves_account_targets() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    set_status(InterfaceStatus {
        account: "owner@example.test".into(),
        weekly: Some("weekly 70% left".into()),
        ..Default::default()
    });
    set_goal(Some("Build Trust with Trust"));
    assert_eq!(status_height(), 2);
    let area = Rect::new(3, 2, 100, status_height());
    let mut buf = Buffer::empty(area);
    render_status_current(area, &mut buf, Depth::Mono, Theme::default());
    assert_eq!(row(&buf, area.y).trim(), "Goal: Build Trust with Trust");
    assert!(row(&buf, area.y + 1).contains("owner@example.test"));
    assert!(row(&buf, area.y + 1).contains("weekly 70% left"));
    assert!(!account_mouse_hit(area.x, area.y));
    assert!(account_mouse_hit(area.x, area.y + 1));

    let clipped = Rect::new(3, 2, 100, 1);
    let mut buf = Buffer::empty(clipped);
    render_status_current(clipped, &mut buf, Depth::Mono, Theme::default());
    assert_eq!(row(&buf, clipped.y).trim(), "Goal: Build Trust with Trust");
    assert!(!account_mouse_hit(area.x, area.y + 1));
}

#[test]
fn replacing_and_clearing_a_goal_never_resurrects_the_old_objective() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    set_status(InterfaceStatus::default());
    set_goal(Some("First objective has a long obsolete suffix"));
    let area = Rect::new(0, 0, 80, status_height());
    let mut buf = Buffer::empty(area);
    render_status_current(area, &mut buf, Depth::Mono, Theme::default());
    set_goal(Some("Second objective"));
    render_status_current(area, &mut buf, Depth::Mono, Theme::default());
    assert_eq!(row(&buf, 0).trim(), "Goal: Second objective");
    assert!(!row(&buf, 0).contains("First objective"));

    set_goal(None);
    assert_eq!(status_height(), 1);
    set_enabled(false);
    assert_eq!(status_height(), 0);
    set_enabled(true);
    assert_eq!(status_height(), 1);
    let area = Rect::new(0, 0, 80, status_height());
    let mut buf = Buffer::empty(area);
    render_status_current(area, &mut buf, Depth::Mono, Theme::default());
    assert!(!row(&buf, 0).contains("objective"));
    assert!(row(&buf, 0).contains("Account unavailable"));
}

#[test]
fn goal_row_normalizes_multiline_text_and_clips_unicode_at_viewport_edges() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    set_status(InterfaceStatus::default());
    set_goal(Some("\nBuild\tTrust\nwith 👩🏽‍💻 and 世界\u{7}\n"));
    assert_eq!(
        current_goal().as_deref(),
        Some("Build Trust with 👩🏽‍💻 and 世界")
    );
    for width in 0..=80 {
        for height in 0..=2 {
            let area = Rect::new(4, 3, width, height);
            let mut buf = Buffer::empty(area);
            render_status_current(area, &mut buf, Depth::Mono, Theme::default());
            for y in area.y..area.bottom() {
                // Ratatui stores a wide grapheme in one cell and resets its
                // continuation cells to spaces. Read physical columns as the
                // terminal does; concatenating every cell double-counts them.
                let mut visible = String::new();
                let mut hidden = 0;
                for x in area.x..area.right() {
                    if hidden > 0 {
                        hidden -= 1;
                        continue;
                    }
                    let symbol = buf[(x, y)].symbol();
                    let columns = UnicodeWidthStr::width(symbol).max(1);
                    assert!(columns <= usize::from(area.right() - x));
                    visible.push_str(symbol);
                    hidden = columns - 1;
                }
                assert!(UnicodeWidthStr::width(visible.trim_end()) <= usize::from(width));
                if width == 26 && y == area.y {
                    assert_eq!(visible.trim_end(), "Goal: Build Trust with 👩🏽‍💻…");
                }
            }
        }
    }
    set_goal(Some("\n\t\u{7}"));
    assert_eq!(status_height(), 1);
}

#[test]
fn overline_keeps_brand_left_and_toggle_right_at_all_depths() {
    let _scene = scene(HarnessWord::Unknown, TaskFace::Open, None);
    for depth in [Depth::TrueColor, Depth::Ansi256, Depth::Ansi16, Depth::Mono] {
        for theme in [
            Theme::from_bg(Some((16, 20, 24))),
            Theme::from_bg(Some((255, 255, 255))),
        ] {
            let area = Rect::new(5, 3, 90, 5);
            let mut buf = Buffer::empty(area);
            render(area, &mut buf, depth, theme);
            let line = row(&buf, area.y);
            assert!(
                line.starts_with(&format!(
                    "   {} on Codex {} ",
                    display_release(codex_alab_core::version::alab()),
                    display_release(
                        codex_alab_core::version::display_baseline().unwrap_or("unreleased")
                    )
                )),
                "{line:?}"
            );
            assert!(line.ends_with("[ALab on]  "), "{line:?}");
            assert!(!line.contains("/alab help"), "{line:?}");
            assert!(line.contains('─'), "{line:?}");
            for y in area.y + 1..area.bottom() {
                assert!(row(&buf, y).trim().is_empty());
            }
            if depth == Depth::Mono {
                assert!(buf.content.iter().all(|cell| cell.fg == Color::Reset));
            }
        }
    }
}

#[test]
fn composer_labels_and_rule_stay_readable_on_both_themes() {
    use crate::banner::palette;
    let _scene = scene(HarnessWord::Unknown, TaskFace::Open, None);
    for width in [20, 40, 80, 120] {
        for background in [(16, 20, 24), (255, 255, 255)] {
            for depth in [Depth::TrueColor, Depth::Ansi256, Depth::Ansi16] {
                let area = Rect::new(0, 0, width, 3);
                let mut buf = Buffer::empty(area);
                let theme = Theme::from_bg(Some(background));
                render(area, &mut buf, depth, theme);
                if depth == Depth::Ansi16 {
                    let rule = Rect::new(2, 0, width - 4, 1);
                    let mut rainbow = Buffer::empty(rule);
                    crate::banner::tint_composer_rule(rule, &mut rainbow, depth, theme);
                    for x in rule.x..rule.right() {
                        let cell = &buf[(x, 0)];
                        if build_mouse_hit(x, 0) || mouse_hit(x, 0) {
                            assert_eq!(cell.fg, Color::Reset, "column {x} of {width}");
                            assert!(cell.modifier.contains(Modifier::BOLD));
                        } else {
                            assert_eq!(cell.fg, rainbow[(x, 0)].fg, "column {x} of {width}");
                        }
                    }
                    continue;
                }
                let mut colors = Vec::new();
                for x in 2..width - 2 {
                    let cell = &buf[(x, 0)];
                    if !cell.symbol().trim().is_empty() {
                        if !colors.contains(&cell.fg) {
                            colors.push(cell.fg);
                        }
                        let rgb = match cell.fg {
                            Color::Rgb(r, g, b) => Some(palette::pack((r, g, b))),
                            Color::Indexed(i) => Some(palette::xterm256(i)),
                            _ => None,
                        };
                        if let Some(rgb) = rgb {
                            assert!(
                                palette::contrast(rgb, palette::pack(background))
                                    >= palette::TEXT_CONTRAST_FLOOR
                            );
                        }
                    }
                }
                assert!(
                    colors.len() >= 5,
                    "rainbow is hidden at width {width}: {colors:?}"
                );
            }
        }
    }
}

#[test]
fn unreported_composer_ink_preserves_mode_weights_and_the_rainbow() {
    // The real layout reads the process's stamped identity. Give this case its own executable.
    const CASE: &str =
        "interface::tests::unreported_composer_ink_preserves_mode_weights_and_the_rainbow";
    if !std::env::args().skip(1).eq(["--exact", CASE]) {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CASE])
            .output()
            .unwrap();
        assert!(
            child.status.success() && String::from_utf8_lossy(&child.stdout).contains("1 passed"),
            "{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        return;
    }
    codex_alab_core::version::initialize_stamped_package(
        "0.170.0",
        Some("0.11.0"),
        Some("0.169.0"),
        None,
        None,
    )
    .unwrap();
    let (state, shared, _scope, _automation, _clock) =
        scene(HarnessWord::Land, TaskFace::Open, None);
    let full_update = Some("0.11 on Codex 0.169 ↑ update");
    let mut cases = vec![
        (
            120,
            true,
            Some((HarnessWord::Land, "⤓ land", HarnessWord::Off)),
            full_update,
            "[ALab on]",
        ),
        (80, true, None, full_update, "[ALab on]"),
        (120, false, None, None, "[ALab off]"),
        (19, false, None, None, "[off]"),
    ];
    // With the stamped release and an update pending, cross both exact fits:
    // the first picker at 11 columns and the first short version at 15.
    for mode in [
        (HarnessWord::Cruise, "∞", HarnessWord::Land),
        (HarnessWord::Land, "⤓", HarnessWord::Off),
        (HarnessWord::Off, "○", HarnessWord::Cruise),
    ] {
        for width in 10..=19 {
            cases.push((
                width,
                true,
                Some(mode),
                (width >= 15).then_some("0.11"),
                "[on]",
            ));
        }
        cases.push((34, true, Some(mode), Some("0.11 on Codex 0.169"), "[on]"));
    }
    for (width, on, mode, label, badge) in cases {
        set_enabled(on);
        let area = Rect::new(0, 0, width, 3);
        let harness = mode.map(|(word, _, _)| {
            shared.set_autopilot_with_word(Autopilot::Active, word);
            (shared, shared.harness())
        });
        let layout = ComposerControls::fit(
            area,
            on,
            button_for(area, on).unwrap(),
            harness,
            /*pending*/ true,
        );
        assert_eq!(layout.label.as_deref(), label);
        for depth in [Depth::TrueColor, Depth::Ansi256, Depth::Ansi16, Depth::Mono] {
            let paint = |theme| {
                let mut buf = Buffer::empty(area);
                buf.set_style(
                    area,
                    Style::default()
                        .fg(Color::Magenta)
                        .bg(Color::Blue)
                        .add_modifier(Modifier::DIM | Modifier::UNDERLINED),
                );
                let hits = render_border(area, &mut buf, &layout, depth, theme);
                (buf, hits)
            };
            let (ordinary, original_hits) = paint(Theme::default());
            let (buf, hits) = paint(Theme::default().with_terminal_foreground_if_unreported());
            let line = row(&buf, 0);
            assert_eq!(line, row(&ordinary, 0));
            let target = |value: &str| {
                Rect::new(column_of(&line, value).unwrap(), 0, value.width() as u16, 1)
            };
            assert_eq!(hits.build, label.map(target));
            assert_eq!(hits.button, Some(target(badge)));
            assert_eq!(hits.modes, original_hits.modes);
            let status = mode.map(|(word, text, next)| (target(text), word, next));
            let picker = (mode.is_some() && (11..=34).contains(&width)).then(|| target("▾"));
            assert_eq!(line.contains('▾'), picker.is_some(), "{line}");
            if let Some(rect) = picker {
                let (face, _, _) = status.unwrap();
                assert_eq!(rect.x, face.right(), "{line}");
                assert!(rect.right() <= hits.button.unwrap().x, "{line}");
            }
            if let Some((rect, word, _)) = status {
                let ordinary_cell = &ordinary[(rect.x, rect.y)];
                if depth == Depth::TrueColor {
                    assert!(matches!(ordinary_cell.fg, Color::Rgb(..)));
                }
                if depth == Depth::Mono && word == HarnessWord::Off {
                    assert!(ordinary_cell.modifier.contains(Modifier::DIM));
                }
            }
            if width == 120 && on {
                assert!(line.contains(PADDED_LAND_TRACK));
                if depth == Depth::TrueColor {
                    let rainbow: std::collections::HashSet<_> = buf
                        .content
                        .iter()
                        .filter(|cell| cell.symbol() == "─")
                        .map(|cell| cell.fg)
                        .collect();
                    assert!(rainbow.len() > 2, "{rainbow:?}");
                }
                for (word, mode) in [
                    ("Cruise", HarnessWord::Cruise),
                    ("Land", HarnessWord::Land),
                    ("Off", HarnessWord::Off),
                ] {
                    let point = target(&format!(" {word} "));
                    assert!(hits.modes.contains(&(point, Hit::Mode(mode))));
                }
            }
            let native_ansi16_track = hits.modes.iter().find_map(|&(rect, hit)| {
                (depth == Depth::Ansi16 && hit == Hit::MenuOnly).then_some(rect)
            });
            for pos in area.positions() {
                let cell = &buf[pos];
                let expected_bold = if hits.build.is_some_and(|r| r.contains(pos))
                    || hits.button.is_some_and(|r| r.contains(pos))
                    || picker.is_some_and(|r| r.contains(pos))
                {
                    Some(true)
                } else {
                    status.and_then(|(rect, word, _)| {
                        rect.contains(pos).then_some(word != HarnessWord::Off)
                    })
                };
                if let Some(bold) = expected_bold {
                    let expected_fg = if depth == Depth::Mono {
                        ordinary[pos].fg
                    } else {
                        Color::Reset
                    };
                    assert_eq!(
                        (cell.fg, cell.bg),
                        (expected_fg, Color::Blue),
                        "{depth:?} {line:?} at {pos:?}"
                    );
                    assert_eq!(
                        cell.modifier.contains(Modifier::BOLD),
                        bold,
                        "{line} at {pos:?}"
                    );
                    assert!(!cell.modifier.contains(Modifier::DIM));
                } else if native_ansi16_track.is_some_and(|r| r.contains(pos)) {
                    let mut expected = ordinary[pos].clone();
                    assert_ne!(expected.fg, Color::Reset);
                    expected.set_fg(Color::Reset).set_bg(Color::Reset);
                    assert_eq!(cell, &expected, "{line} at {pos:?}");
                } else {
                    assert_eq!(
                        cell, &ordinary[pos],
                        "rule, named track and ambient cells at {pos:?}"
                    );
                }
            }
            *state.overline_hits.lock().unwrap() = hits;
            if let Some((rect, _, next)) = status {
                assert_eq!(mode_mouse_hit(rect.x, rect.y), Some(next));
                assert!(!autopilot_disclosure_hit(rect.x, rect.y));
            }
            if let Some(rect) = picker {
                assert!(autopilot_disclosure_hit(rect.x, rect.y));
                assert_eq!(mode_mouse_hit(rect.x, rect.y), None);
            }
            for bg in [(0, 0, 0), (255, 255, 255)] {
                let reported = Theme::from_bg(Some(bg));
                assert_eq!(
                    paint(reported).0,
                    paint(reported.with_terminal_foreground_if_unreported()).0
                );
            }
        }
    }
}

#[test]
fn an_eighty_column_prompt_keeps_both_packaged_version_numbers() {
    let area = Rect::new(0, 0, 80, 3);
    let button = button_for(area, true).unwrap();
    let available = usize::from(button.x.saturating_sub(area.x + 3));
    let alab = "0.11.0-dev.123+codex.0.169.0.g012345678";
    let label = fit_brand_label(
        available,
        alab,
        release_pair(alab, "0.169.0+g987654321"),
        false,
        0,
    );
    assert_eq!(label.as_deref(), Some("0.11 on Codex 0.169"));
    assert_eq!(release_pair("v0.12.0", "0.169.1"), "0.12 on Codex 0.169.1");
    assert_eq!(release_pair("dev", "unreleased"), "dev on Codex unreleased");
}

#[test]
fn plain_title_rows_keep_release_numbers_whole_and_use_one_row_for_the_pair_when_it_fits() {
    for (alab, baseline, pair) in [
        ("0.11.0", "0.169.0", "0.11 on Codex 0.169"),
        ("0.12.1", "0.169.2", "0.12.1 on Codex 0.169.2"),
    ] {
        let token_width = display_release(baseline)
            .len()
            .max(display_release(alab).len());
        for width in token_width..=40 {
            let rows = plain_title_rows(alab, baseline, width);
            let expected_brand: &[&str] = if width < 10 {
                &["Codex", "ALab"]
            } else {
                &["Codex ALab"]
            };
            let (brand, release) = rows.split_at(expected_brand.len());
            for (line, word) in brand.iter().zip(expected_brand) {
                assert_eq!(line.to_string().trim(), *word, "{width}");
                assert!(
                    line.spans
                        .last()
                        .unwrap()
                        .style
                        .add_modifier
                        .contains(Modifier::BOLD)
                );
            }
            let release = release
                .iter()
                .map(|row| row.to_string().trim().to_owned())
                .collect::<Vec<_>>();
            assert!(rows.iter().all(|line| line.width() <= width));
            assert_eq!(release.join(" "), pair, "{width}");
            assert_eq!(release.len() == 1, width >= pair.width(), "{width}");
        }
    }
}

#[test]
fn plain_title_rows_fit_even_when_one_release_word_is_wider_than_the_pane() {
    assert!(plain_title_rows("0.11.0", "0.169.0", 0).is_empty());
    for (width, alab, baseline, expected) in [
        (
            1,
            "0.11",
            "0.169",
            &["0", ".", "1", "1", "…", "…", "0", ".", "1", "6", "9"][..],
        ),
        (
            4,
            "0.11.0-dev.4",
            "0.169.0+g123",
            &["0.11", "  on", "Cod…", "0.16", "9"][..],
        ),
        (
            5,
            "0.12.1",
            "0.169.1",
            &["0.12.", "1", "  on", "Codex", "0.169", ".1"][..],
        ),
        (
            6,
            "0.12",
            "0.169.2",
            &["  0.12", "  on", "Codex", "0.169.", "2"][..],
        ),
    ] {
        let rows = plain_title_rows(alab, baseline, width);
        assert!(rows.iter().all(|line| line.width() <= width));
        let brand_rows = if width < 5 { 1 } else { 2 };
        let release: Vec<_> = rows[brand_rows..].iter().map(Line::to_string).collect();
        assert_eq!(release, expected);
    }
    assert_eq!(
        plain_title_rows("0.11", "\u{200b}long", 1).last(),
        Some(&Line::from("…")),
    );
    assert_eq!(plain_title_rows("0.11", "0.169", 1)[0], Line::from("…"));
    assert_eq!(
        plain_title_rows("0.11", "0.169", 2)[0],
        Line::from(Span::styled("C…", Style::default().bold())),
    );
}

#[test]
fn primary_version_formatting_preserves_build_numbers_without_mutating_source_versions() {
    for (raw, displayed) in [
        ("0.164.0-dev.12814+ga180bdb0c", "0.164.0-dev.12814"),
        ("0.163.0+ge8aa7c1678ca", "0.163.0"),
        ("0.164.0", "0.164.0"),
        ("dev", "dev"),
        ("unreleased", "unreleased"),
    ] {
        assert_eq!(readable_version(raw), displayed);
        assert!(raw.starts_with(displayed));
    }
    assert_ne!(
        readable_version("0.164.0-dev.12814+ga180bdb0c"),
        readable_version("0.164.0-dev.12815+gbb3dcfecf"),
        "automatic updates remain distinguishable by their readable build number"
    );
}

#[test]
fn routine_activity_and_permission_labels_leave_room_for_quota() {
    let status = Status {
        email: Some("person@example.test"),
        plan: Some("Pro 500"),
        weekly: Some("weekly 62% left"),
        mode: Some("Default / Full access"),
        activity: Some("Working"),
        ..Default::default()
    };
    let line = text(&status_line(200, status, Depth::Mono, Theme::default()));
    assert!(line.contains("Pro 500"));
    assert!(!line.contains("Working"));
    assert!(!line.contains("Default"));
    assert!(!line.contains("Full access"));
    let offline = Status {
        activity: Some("Offline"),
        mode: Some("Read only"),
        ..status
    };
    let line = text(&status_line(200, offline, Depth::Mono, Theme::default()));
    assert!(line.contains("Offline"));
    assert!(line.contains("Read only"));
}

#[test]
fn disconnected_footer_keeps_the_transport_label_ahead_of_clickable_cached_fields() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    set_status(InterfaceStatus {
        account: "owner@example.test".into(),
        plan: Some("Plus".into()),
        weekly: Some("weekly 62% left".into()),
        activity: Some("Reviewing".into()),
        ..Default::default()
    });

    for (connection, label) in [
        (ConnectionState::Reconnecting, "Reconnecting"),
        (ConnectionState::Offline, "Offline"),
    ] {
        set_connection_state(connection);
        let prefix = format!("{label} │ ");
        let prefix_width = prefix.width() as u16;
        for width in (0..=80).rev() {
            // The buffer clips five columns from the requested area's left edge.
            let area = Rect::new(2, 4, width + 5, 1);
            let viewport = Rect::new(7, 3, width + 2, 3);
            let mut buf = Buffer::empty(viewport);
            render_status_current(area, &mut buf, Depth::Mono, Theme::default());
            let line = row(&buf, area.y);
            let visible = line.trim_end();
            let account_x = viewport.x + prefix_width;
            let account_cells = if width <= prefix_width {
                assert_eq!(visible, fitted(label, width.into()), "{width} columns");
                0
            } else {
                let cached = visible
                    .strip_prefix(&prefix)
                    .unwrap_or_else(|| panic!("{width} columns: {line:?}"));
                assert!(!cached.is_empty(), "{width} columns: {line:?}");
                assert!(!cached.contains("Reviewing"), "{line:?}");
                if width == 80 {
                    assert_eq!(cached, "owner@example.test / Plus / weekly 62% left");
                }
                cached.width() as u16
            };
            for x in viewport.x - 1..viewport.right() {
                assert_eq!(
                    account_mouse_hit(x, area.y),
                    (account_x..account_x + account_cells).contains(&x),
                    "{label} at width {width}, column {x}: {line:?}"
                );
                assert!(!account_mouse_hit(x, area.y + 1));
            }
        }
    }
}

#[test]
fn transport_before_identity_and_latest_selected_activity_after_reconnect() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    let area = Rect::new(3, 2, 100, 1);
    let mut buf = Buffer::empty(area);
    for (connection, expected) in [
        (ConnectionState::Reconnecting, "Reconnecting"),
        (ConnectionState::Offline, "Offline"),
    ] {
        set_connection_state(connection);
        render_status_current(area, &mut buf, Depth::Mono, Theme::default());
        assert_eq!(row(&buf, area.y).trim_end(), expected);
        assert!((area.x..area.right()).all(|x| !account_mouse_hit(x, area.y)));
        assert!(connected_local_email().is_none());
    }

    for (account, task) in [
        ("first@example.test", "Inspecting"),
        ("second@example.test", "Reviewing"),
    ] {
        set_status(InterfaceStatus {
            account: account.into(),
            local_subscription: true,
            activity: Some(task.into()),
            ..Default::default()
        });
        render_status_current(area, &mut buf, Depth::Mono, Theme::default());
        let line = row(&buf, area.y);
        assert!(line.starts_with(&format!("Offline │ {account}")), "{line}");
        assert!(!line.contains(task));
        assert!(connected_local_email().is_none());
    }
    set_connection_state(ConnectionState::Connected);
    render_status_current(area, &mut buf, Depth::Mono, Theme::default());
    let line = row(&buf, area.y);
    assert!(line.starts_with("second@example.test"));
    assert!(line.trim_end().ends_with("Reviewing"));
    assert!(!line.contains("Offline"));
    assert!(account_mouse_hit(area.x, area.y));
    assert_eq!(
        connected_local_email().as_deref(),
        Some("second@example.test")
    );
}

#[test]
fn partial_individual_spend_totals_follow_the_selected_interface_mode() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    for (enabled, unknown) in [(true, Some("unreported")), (false, None)] {
        set_enabled(enabled);
        assert_eq!(individual_limit_amount(None).as_deref(), unknown);
        assert_eq!(
            individual_limit_amount(Some("0".into())).as_deref(),
            Some("0")
        );
    }
}

#[test]
fn settled_chat_usage_and_balance_take_priority_on_a_narrow_footer() {
    let _scope = scoped(Box::leak(Box::new(State::new())));
    configure(true);
    set_status(InterfaceStatus {
        account: "person@example.test".into(),
        weekly: Some("weekly 0% left".into()),
        credit_balance: Some(CreditBalance::Current("12.56 credits left".into())),
        thread_credit_usage: Some(ChatCreditUsage {
            summary: ChatCreditSummary {
                text: "2.5 credits used in this chat".into(),
                limit_comparison: None,
                credit_activity: true,
            },
            as_of: None,
            retained: false,
        }),
        ..Default::default()
    });
    for (width, expected) in [
        (30, "2.5 credits used in this chat"),
        (
            150,
            "2.5 credits used in this chat · 12.56 credits left · weekly 0% left",
        ),
    ] {
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        render_status_current(area, &mut buf, Depth::Mono, Theme::default());
        assert!(row(&buf, 0).contains(expected), "{}", row(&buf, 0));
    }
}

#[test]
fn credit_footer_keeps_the_weekly_allowance_with_delayed_chat_billing() {
    let _scope = scoped(Box::leak(Box::new(State::new())));
    configure(true);
    let mut history = crate::quota::History::default();
    assert!(history.observe(crate::quota::WeeklySnapshot {
        used_percent: 100.0,
        observed_at: 1_000,
        resets_at: 605_800,
        window_minutes: 10_080,
    }));
    let spend = "Last report · 2.5 credits used in this chat";
    let weekly = "weekly 0% left";
    let combined = format!("{spend} · {weekly}");
    for (available_count, stale, suffix) in [
        (0, false, ""),
        (1, false, " · resets ●"),
        (0, true, " · resets stale"),
    ] {
        set_status(InterfaceStatus {
            account: "person@example.test".into(),
            local_subscription: true,
            weekly: Some(weekly.into()),
            quota: history.view(1_000),
            thread_credit_usage: Some(ChatCreditUsage {
                summary: ChatCreditSummary {
                    text: "2.5 credits used in this chat".into(),
                    limit_comparison: None,
                    credit_activity: true,
                },
                as_of: None,
                retained: true,
            }),
            reset_credits: Some(ResetCredits {
                available_count,
                expiring_count: 0,
                expired_count: 0,
                next_expiry: None,
                stale,
                details_known: !stale,
            }),
            ..Default::default()
        });
        for (width, expected) in [
            (
                120,
                format!("{spend} · weekly [████████] 100% used{suffix}"),
            ),
            (combined.width() as u16, combined.clone()),
            (combined.width() as u16 - 1, spend.to_owned()),
        ] {
            let area = Rect::new(0, 0, width, 1);
            let mut buf = Buffer::empty(area);
            render_status_current(area, &mut buf, Depth::Mono, Theme::default());
            let line = row(&buf, 0);
            assert!(
                line.trim_end().ends_with(&expected),
                "width {width}: {line}"
            );
        }
    }
}

#[test]
fn credit_footer_keeps_an_expiring_reset_visible_when_the_meter_does_not_fit() {
    let _scope = scoped(Box::leak(Box::new(State::new())));
    configure(true);
    let mut history = crate::quota::History::default();
    assert!(history.observe(crate::quota::WeeklySnapshot {
        used_percent: 75.0,
        observed_at: 1_000,
        resets_at: 605_800,
        window_minutes: 10_080,
    }));
    let balance = "25 credits left";
    let weekly = "weekly 25% left";
    for (count, details_known, expiry) in [
        (1, true, "1 reset expires soon"),
        (2, true, "2 resets expire soon"),
        (1, false, "≥1 reset expires soon"),
    ] {
        let mut status = InterfaceStatus {
            account: "person@example.test".into(),
            credit_balance: Some(CreditBalance::Current(balance.into())),
            weekly: Some(weekly.into()),
            quota: history.view(1_000),
            reset_credits: Some(ResetCredits {
                available_count: 3,
                expiring_count: count,
                expired_count: 0,
                next_expiry: None,
                stale: false,
                details_known,
            }),
            ..Default::default()
        };
        for (stale, expected) in [
            (false, format!("{balance} · {expiry}")),
            (false, format!("{balance} · {weekly} · {expiry}")),
            (false, format!("{balance} · weekly 75% used")),
            (true, format!("{balance} · weekly 75% used")),
        ] {
            status.reset_credits.as_mut().unwrap().stale = stale;
            set_status(status.clone());
            let area = Rect::new(0, 0, expected.width() as u16, 1);
            let mut buf = Buffer::empty(area);
            render_status_current(area, &mut buf, Depth::Mono, Theme::default());
            assert_eq!(row(&buf, 0).trim_end(), expected);
        }
    }
}

#[test]
fn narrow_footer_keeps_the_credit_block_visible_ahead_of_prior_chat_spending() {
    let _scope = scoped(Box::leak(Box::new(State::new())));
    configure(true);
    set_status(InterfaceStatus {
        account: "person@example.test".into(),
        credit_balance: Some(CreditBalance::Unavailable(
            "Workspace out of credits · 12.56 credits".into(),
        )),
        thread_credit_usage: Some(ChatCreditUsage {
            summary: ChatCreditSummary {
                text: "2.5 credits used in this chat".into(),
                limit_comparison: None,
                credit_activity: true,
            },
            as_of: None,
            retained: false,
        }),
        ..Default::default()
    });
    for (width, expected) in [
        (25, "Workspace out of credits"),
        (
            150,
            "Workspace out of credits · 12.56 credits · 2.5 credits used in this chat",
        ),
    ] {
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        render_status_current(area, &mut buf, Depth::Mono, Theme::default());
        let shown = row(&buf, 0);
        assert!(shown.contains(expected), "{shown}");
    }
}

#[test]
fn weekly_usage_and_expiring_resets_are_visible_and_remain_clickable() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    let mut history = crate::quota::History::default();
    for (day, used_percent) in [(0, 10.0), (1, 20.0), (2, 30.0)] {
        assert!(history.observe(crate::quota::WeeklySnapshot {
            used_percent,
            observed_at: 1_000 + day * 86_400,
            resets_at: 605_800,
            window_minutes: 10_080,
        }));
    }
    set_status(InterfaceStatus {
        account: "person@example.test".into(),
        plan: Some("Pro 500".into()),
        weekly: Some("weekly 70% left".into()),
        weekly_reset: Some("Fri 16:00 PDT".into()),
        quota: history.view(173_800),
        reset_credits: Some(ResetCredits {
            available_count: 3,
            expiring_count: 1,
            expired_count: 0,
            next_expiry: Some("Thu 16:00 PDT".into()),
            stale: false,
            details_known: true,
        }),
        ..Default::default()
    });
    let area = Rect::new(3, 2, 220, 1);
    let mut buf = Buffer::empty(area);
    render_status_current(area, &mut buf, Depth::TrueColor, Theme::default());
    let line = row(&buf, area.y);
    for expected in [
        "Pro 500",
        "weekly [██░░░░░░] 30% used",
        "Fri 16:00 PDT",
        "resets ●●●",
        "1 expires soon",
        "est 80% at reset",
    ] {
        assert!(line.contains(expected), "{line:?}");
    }
    let x = (area.x..area.right())
        .find(|x| buf[(*x, area.y)].symbol() == "●")
        .unwrap();
    assert!(buf[(x, area.y)].modifier.contains(Modifier::BOLD));
    assert!(!buf[(x + 1, area.y)].modifier.contains(Modifier::BOLD));
    assert!(account_mouse_hit(x, area.y));
    for width in 0..=120 {
        let clip = Rect::new(7, 4, width, 1);
        let mut clipped = Buffer::empty(clip);
        render_status_current(clip, &mut clipped, Depth::Mono, Theme::default());
        assert!(UnicodeWidthStr::width(row(&clipped, clip.y).as_str()) <= usize::from(width));
    }
    render_status_current(
        Rect::new(0, 0, 0, 0),
        &mut buf,
        Depth::Mono,
        Theme::default(),
    );
    let native = buf.clone();
    set_enabled(false);
    render_status_current(area, &mut buf, Depth::TrueColor, Theme::default());
    assert_eq!(
        buf, native,
        "the disabled interface never recolors native dots"
    );
}

#[test]
fn narrow_weekly_footer_keeps_the_whole_percentage_and_its_account_target() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    for used_percent in [0.0, 9.0, 94.0, 99.0, 99.6, 100.0] {
        let mut history = crate::quota::History::default();
        assert!(history.observe(crate::quota::WeeklySnapshot {
            used_percent,
            observed_at: 1_000,
            resets_at: 605_800,
            window_minutes: 10_080,
        }));
        set_status(InterfaceStatus {
            account: "person@example.test".into(),
            weekly: Some("weekly unavailable".into()),
            weekly_reset: Some("Fri 16:00".into()),
            quota: history.view(1_000),
            ..Default::default()
        });
        let reading = format!("weekly {used_percent:.0}% used");
        let meter_width = reading.width() as u16 + 11; // Eight cells, brackets and a space.
        for width in reading.width() as u16 - 1..=meter_width + 1 {
            let area = Rect::new(2, 1, width, 1);
            let mut buf = Buffer::empty(Rect::new(0, 0, width + 4, 3));
            render_status_current(area, &mut buf, Depth::Mono, Theme::default());
            let text = row(&buf, area.y);
            if width < reading.width() as u16 {
                assert!(text.trim().starts_with("weekly ["), "{text}");
            } else if width < meter_width {
                assert_eq!(text.trim(), reading);
                for x in 0..buf.area.width {
                    assert_eq!(
                        account_mouse_hit(x, area.y),
                        (area.x..area.x + reading.len() as u16).contains(&x)
                    );
                    assert!(!account_mouse_hit(x, area.y + 1));
                }
            } else {
                assert!(
                    text.contains(&format!("] {used_percent:.0}% used")),
                    "{width}: {text}"
                );
            }
        }
    }
}

#[test]
fn connected_subscription_keeps_the_alias_and_weekly_reading_at_their_width_boundaries() {
    let _scope = scoped(Box::leak(Box::new(State::new())));
    configure(true);
    let paint = |width| {
        let area = Rect::new(2, 1, width, 1);
        let mut buf = Buffer::empty(area);
        render_status_current(area, &mut buf, Depth::Mono, Theme::default());
        let line = row(&buf, area.y).trim_end().to_owned();
        for x in area.x - 1..=area.right() {
            assert_eq!(
                account_mouse_hit(x, area.y),
                (area.x..area.x + line.width() as u16).contains(&x),
                "{width}: {line:?} at {x}"
            );
        }
        assert!(!account_mouse_hit(area.x, area.y + 1));
        line
    };
    for (used_percent, bar) in [
        (15.0, "█░░░░░░░"),
        (37.0, "███░░░░░"),
        (99.6, "████████"),
        (100.0, "████████"),
    ] {
        let mut history = crate::quota::History::default();
        assert!(history.observe(crate::quota::WeeklySnapshot {
            used_percent,
            observed_at: 1_000,
            resets_at: 605_800,
            window_minutes: 10_080,
        }));
        let reading = format!("weekly {used_percent:.0}% used");
        let meter = format!("weekly [{bar}] {used_percent:.0}% used");
        let detailed = format!("{meter} ↻ Fri 16:00 PDT · resets 0");
        for (account, alias) in [
            ("team.abc@example.test", "team…abc"),
            ("team.abc.2@example.test", "team…c.2"),
        ] {
            set_status(InterfaceStatus {
                account: account.into(),
                local_subscription: true,
                plan: Some("Pro 500".into()),
                five_hour: Some("5h 89% left".into()),
                weekly_reset: Some("Fri 16:00 PDT".into()),
                quota: history.view(1_000),
                reset_credits: Some(ResetCredits {
                    available_count: 0,
                    expiring_count: 0,
                    expired_count: 0,
                    next_expiry: None,
                    stale: false,
                    details_known: true,
                }),
                ..Default::default()
            });
            // Eight display cells for the alias and three for its separator.
            let pair_width = reading.width() as u16 + 11;
            assert_eq!(paint(pair_width - 1), reading);
            assert_eq!(paint(pair_width), format!("{alias} / {reading}"));
            assert_eq!(
                paint(meter.width() as u16 + 11),
                format!("{alias} / {meter}")
            );
            assert_eq!(
                paint(detailed.width() as u16 + 11),
                format!("{alias} / {detailed}")
            );
            assert_eq!(
                paint(115),
                format!("{account} / Pro 500 / {detailed} / 5h 89% left")
            );
        }
    }
}

#[test]
fn weekly_identity_fallback_preserves_other_account_and_connection_states() {
    let _scope = scoped(Box::leak(Box::new(State::new())));
    configure(true);
    let mut history = crate::quota::History::default();
    assert!(history.observe(crate::quota::WeeklySnapshot {
        used_percent: 37.0,
        observed_at: 1_000,
        resets_at: 605_800,
        window_minutes: 10_080,
    }));
    let mut status = InterfaceStatus {
        account: "team.abc.2@example.test".into(),
        weekly_reset: Some("Fri 16:00 PDT".into()),
        quota: history.view(1_000),
        ..Default::default()
    };
    let paint = |status: &InterfaceStatus, width| {
        set_status(status.clone());
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        render_status_current(area, &mut buf, Depth::TrueColor, Theme::default());
        row(&buf, 0).trim_end().to_owned()
    };
    let remote = paint(&status, 37);
    assert!(
        remote.starts_with("weekly [███░░░░░] 37% used ↻"),
        "{remote}"
    );
    status.local_subscription = true;
    for (connection, prefix) in [
        (ConnectionState::Reconnecting, "Reconnecting │ "),
        (ConnectionState::Offline, "Offline │ "),
    ] {
        set_connection_state(connection);
        assert_eq!(
            paint(&status, 37 + prefix.width() as u16),
            format!("{prefix}{remote}")
        );
    }
    set_connection_state(ConnectionState::Connected);
    let meter_reset = "weekly [███░░░░░] 37% used ↻ Fri 16:00 PDT";
    for (stale, expiring, suffix) in [
        (true, 0, "resets stale"),
        (false, 1, "resets ● (1 expires soon)"),
    ] {
        status.reset_credits = Some(ResetCredits {
            available_count: 1,
            expiring_count: expiring,
            expired_count: 0,
            next_expiry: None,
            stale,
            details_known: true,
        });
        let expected = format!("{meter_reset} · {suffix}");
        assert_eq!(paint(&status, expected.width() as u16), expected);
    }
    status.reset_credits = None;
    status.usage_stale = true;
    status.weekly = Some("weekly stale".into());
    assert_eq!(paint(&status, 45), "team.abc.2@example.test / weekly stale");
}

#[test]
fn widening_the_weekly_meter_retains_the_visible_account_and_five_hour_allowance() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    let mut history = crate::quota::History::default();
    for (day, used_percent) in [(0, 10.0), (1, 20.0), (2, 30.0)] {
        assert!(history.observe(crate::quota::WeeklySnapshot {
            used_percent,
            observed_at: 1_000 + day * 86_400,
            resets_at: 605_800,
            window_minutes: 10_080,
        }));
    }
    let quota = history.view(173_800).expect("current weekly readings");
    let sparkline = quota.sparkline(10);
    set_status(InterfaceStatus {
        account: "person@example.test".into(),
        plan: Some("Plus".into()),
        weekly: Some("weekly 70% left".into()),
        weekly_reset: Some("09:00 on 23 Nov".into()),
        five_hour: Some("5h 60% left".into()),
        branch: Some("main".into()),
        quota: Some(quota),
        ..Default::default()
    });

    for width in [84, 85, 86, 87, 99, 100, 103, 104, 118, 119] {
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        render_status_current(area, &mut buf, Depth::Mono, Theme::default());
        let line = row(&buf, area.y);
        assert!(
            line.contains("person@example.test / Plus"),
            "{width}: {line}"
        );
        assert_eq!(line.contains("5h 60% left"), width >= 87, "{width}: {line}");
        assert_eq!(line.contains(&sparkline), width >= 100, "{width}: {line}");
        assert_eq!(
            line.contains("est 80% at reset"),
            width >= 119,
            "{width}: {line}"
        );
    }
}

#[test]
fn light_footer_text_clears_the_contrast_floor_and_keeps_its_click_bounds() {
    use crate::banner::palette;
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    set_goal(Some("Review"));
    set_status(InterfaceStatus {
        account: "owner".into(),
        plan: Some("Plus".into()),
        weekly: Some("weekly".into()),
        five_hour: Some("5h".into()),
        activity: Some("Offline".into()),
        mode: Some("Manual".into()),
        branch: Some("main".into()),
        model: Some("GPT".into()),
        context: Some("ctx".into()),
        ..Default::default()
    });
    let area = Rect::new(2, 1, 100, 2);
    for ground in [(255, 252, 240), (250, 244, 237), (129, 129, 129)] {
        let theme = Theme::from_bg(Some(ground));
        for depth in [Depth::Ansi16, Depth::Mono, Depth::TrueColor, Depth::Ansi256] {
            let mut buf = Buffer::empty(area);
            render_status_current(area, &mut buf, depth, theme);
            let y = area.y + 1;
            let line = row(&buf, y);
            let end = area.x + column_of(&line, " │ ").unwrap();
            for (label, field_y, in_account) in [
                ("Goal: Review", area.y, false),
                ("owner", y, true),
                ("Plus", y, true),
                ("weekly", y, true),
                ("5h", y, true),
                ("Offline", y, false),
                ("Manual", y, false),
                ("main", y, false),
                ("GPT", y, false),
                ("ctx", y, false),
            ] {
                let x = area.x + column_of(&row(&buf, field_y), label).unwrap();
                let cell = &buf[(x, field_y)];
                assert_eq!(account_mouse_hit(x, field_y), in_account);
                assert!(!cell.modifier.contains(Modifier::DIM), "{depth:?} {label}");
                match cell.fg {
                    Color::Rgb(r, g, b) if depth == Depth::TrueColor => assert!(
                        palette::contrast(palette::pack((r, g, b)), palette::pack(ground))
                            >= palette::TEXT_CONTRAST_FLOOR,
                        "{ground:?} {label}"
                    ),
                    Color::Indexed(i) if depth == Depth::Ansi256 => assert!(
                        palette::contrast(palette::xterm256(i), palette::pack(ground))
                            >= palette::TEXT_CONTRAST_FLOOR,
                        "{ground:?} {label}"
                    ),
                    Color::Reset if matches!(depth, Depth::Mono | Depth::Ansi16) => {}
                    other => panic!("{depth:?} {label}: unexpected {other:?}"),
                }
            }
            assert!(account_mouse_hit(end - 1, y));
            assert!(!account_mouse_hit(end, y));
            let divider = &buf[(end + 1, y)];
            assert_eq!(
                divider.fg,
                Palette::new(depth, theme).muted.fg.unwrap_or_default()
            );
        }
    }
}

#[test]
fn reset_expiry_highlights_only_the_reset_credit_dots() {
    use crate::banner::palette;
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    set_status(InterfaceStatus {
        account: "person●@example.test".into(),
        weekly: Some("weekly 70% left".into()),
        branch: Some("topic/●".into()),
        reset_credits: Some(ResetCredits {
            available_count: 2,
            expiring_count: 1,
            expired_count: 0,
            next_expiry: Some("Thu 16:00 PDT".into()),
            stale: false,
            details_known: false,
        }),
        ..Default::default()
    });
    for depth in [Depth::TrueColor, Depth::Ansi256, Depth::Ansi16, Depth::Mono] {
        for background in [(16, 20, 24), (0xfa, 0xf4, 0xed), (255, 255, 255)] {
            let theme = Theme::from_bg(Some(background));
            let area = Rect::new(3, 2, 200, 1);
            let mut buf = Buffer::empty(area);
            render_status_current(area, &mut buf, depth, theme);
            let dots: Vec<_> = (area.x..area.right())
                .filter(|x| matches!(buf[(*x, area.y)].symbol(), "●" | "◉"))
                .collect();
            assert_eq!(dots.len(), 4, "{}", row(&buf, area.y));
            assert_eq!(buf[(dots[0], area.y)].fg, buf[(dots[0] - 1, area.y)].fg);
            assert_eq!(buf[(dots[3], area.y)].fg, buf[(dots[3] - 1, area.y)].fg);
            assert!(buf[(dots[1], area.y)].modifier.contains(Modifier::BOLD));
            assert!(!buf[(dots[2], area.y)].modifier.contains(Modifier::BOLD));
            let weekly = area.x + column_of(&row(&buf, area.y), "weekly").unwrap();
            if depth == Depth::Ansi16 && theme.is_light() {
                assert_eq!(buf[(weekly, area.y)].fg, Color::Reset);
                assert_eq!(buf[(dots[2], area.y)].fg, Color::Reset);
            }
            let expiring = &buf[(dots[1], area.y)];
            let marker = if depth == Depth::Mono || (depth == Depth::Ansi16 && theme.is_light()) {
                "◉"
            } else {
                "●"
            };
            assert_eq!(expiring.symbol(), marker);
            assert_eq!(expiring.symbol().cell_width(), 1);
            assert_eq!(buf[(dots[2], area.y)].symbol(), "●");
            let warning = expiring.fg;
            match (depth, theme.is_light()) {
                (Depth::Mono, _) | (Depth::Ansi16, true) => assert_eq!(warning, Color::Reset),
                (_, false) => assert_eq!(warning, Color::LightYellow),
                (_, true) => {
                    let rgb = match (depth, warning) {
                        (Depth::TrueColor, Color::Rgb(r, g, b)) => palette::pack((r, g, b)),
                        (Depth::Ansi256, Color::Indexed(index)) => palette::xterm256(index),
                        _ => panic!("unexpected warning ink at {depth:?}: {warning:?}"),
                    };
                    let ratio = palette::contrast(rgb, palette::pack(background));
                    assert!(
                        ratio >= palette::TEXT_CONTRAST_FLOOR,
                        "{background:?} {depth:?}: {ratio}"
                    );
                }
            }
            assert_eq!(buf[(dots[2], area.y)].fg, buf[(weekly, area.y)].fg);
        }
    }
}

#[test]
fn every_terminal_width_stays_inside_the_composer() {
    let _scene = scene(HarnessWord::Unknown, TaskFace::Open, None);
    for width in 0..=120 {
        for on in [true, false] {
            let buffer_area = Rect::new(0, 0, width + 10, 7);
            let area = Rect::new(5, 2, width, 3);
            let mut buf = Buffer::empty(buffer_area);
            set_enabled(on);
            let button = button_rect(area);
            assert_eq!(button.is_some(), width >= 8, "{width} columns, on={on}");
            if let Some(button) = button {
                let left_margin = if width == 8 && !on { 1 } else { 2 };
                assert!(button.x >= area.x + left_margin);
                assert_eq!(button.right(), area.right() - 2);
            }
            render(area, &mut buf, Depth::Mono, Theme::default());
            for y in buffer_area.y..buffer_area.bottom() {
                for x in buffer_area.x..buffer_area.right() {
                    if y != area.y || x < area.x || x >= area.right() {
                        assert_eq!(buf[(x, y)].symbol(), " ", "{width}: ({x}, {y})");
                    }
                }
            }
        }
    }
}

#[test]
fn off_preserves_native_cells_except_the_recovery_switch() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    // A word, a landing and a timer the enhanced line would wear (gap 12 of the parity sweep).
    let shared = Box::leak(Box::new(Shared::new()));
    let _automation_scope = crate::boxes::scoped(shared);
    shared.set_autopilot_with_word(crate::session::Autopilot::Active, HarnessWord::Land);
    shared.set_task_face("t1", TaskFace::Landed);
    shared.set_land_at(Some(an_hour_on().0));
    configure(false);
    let area = Rect::new(0, 0, 80, 6);
    let mut native = Buffer::empty(area);
    for cell in &mut native.content {
        cell.set_symbol("x")
            .set_style(Style::default().fg(Color::Magenta));
    }
    let mut off = native.clone();
    render(area, &mut off, Depth::Mono, Theme::default());
    let button = button_rect(area).expect("off switch remains reachable");
    assert!(row(&off, area.y).contains("[ALab off]"));
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if !button.contains((x, y).into()) {
                assert_eq!(off[(x, y)], native[(x, y)]);
            }
        }
    }
    assert!(session_title().is_none());
    for x in 0..80 {
        assert_eq!(mouse_hit(x, 0), button.contains((x, 0).into()));
        assert!(mode_mouse_hit(x, 0).is_none());
        assert!(!autopilot_menu_hit(x, 0));
    }
}

#[test]
fn mode_controls_use_the_reported_mode_and_clear_hits_when_disabled() {
    use crate::session::Autopilot;
    use crate::session::HarnessWord;
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    let shared = Box::leak(Box::new(codex_alab_core::session::Shared::new()));
    let _automation_scope = crate::boxes::scoped(shared);
    configure(true);
    let area = Rect::new(0, 0, 120, 4);
    let mut buf = Buffer::empty(area);
    let segments = [
        (" Cruise ", HarnessWord::Cruise),
        (" Land ", HarnessWord::Land),
        (" Off ", HarnessWord::Off),
    ];
    for (selected, expected) in [
        (HarnessWord::Cruise, "[ Cruise ] Land │ Off ▕"),
        (HarnessWord::Land, "▏ Cruise [ Land ] Off ▕"),
        (HarnessWord::Off, "▏ Cruise │ Land [ Off ]"),
    ] {
        shared.set_autopilot_with_word(Autopilot::Active, selected);
        for depth in [Depth::Mono, Depth::Ansi16, Depth::Ansi256, Depth::TrueColor] {
            // Redrawing into the same buffer must also move the selected highlight.
            render(area, &mut buf, depth, Theme::default());
            let text = row(&buf, 0);
            assert!(text.contains(expected), "{depth:?}: {text}");
            for (label, mode) in segments {
                let start = column_of(&text, label).unwrap();
                for x in start..start + label.len() as u16 {
                    assert_eq!(mode_mouse_hit(x, 0), Some(mode));
                    assert_eq!(
                        buf[(x, 0)].modifier.contains(Modifier::REVERSED),
                        mode == selected
                    );
                }
                for edge in [start - 1, start + label.len() as u16] {
                    assert_eq!(mode_mouse_hit(edge, 0), None);
                    assert!(autopilot_menu_hit(edge, 0));
                }
            }
        }
    }
    set_enabled(false);
    for x in 0..120 {
        assert!(mode_mouse_hit(x, 0).is_none());
    }
}

#[test]
fn ansi16_mode_switch_reverses_the_terminal_text_colors_on_light_and_unreported_surfaces() {
    let light = Theme::from_bg(Some((255, 255, 255)));
    let unreported = Theme::default().with_terminal_foreground_if_unreported();
    let area = Rect::new(0, 0, 120, 4);
    for (selected, face, track) in [
        (HarnessWord::Cruise, "∞", "[ Cruise ] Land │ Off ▕"),
        (HarnessWord::Land, "⤓ land", PADDED_LAND_TRACK),
        (HarnessWord::Off, "○ off", "▏ Cruise │ Land [ Off ]"),
    ] {
        let _scene = scene(selected, TaskFace::Open, None);
        for (theme, fill, edge_colors) in [
            (light, Color::White, (Color::DarkGray, Color::White)),
            (unreported, Color::Reset, (Color::Reset, Color::Reset)),
            (unreported, Color::Blue, (Color::Reset, Color::Reset)),
        ] {
            let mut buf = Buffer::empty(area);
            // The native composer paints its surface before the overline adds the switch.
            buf.set_style(area, Style::default().bg(fill).add_modifier(Modifier::DIM));
            render(area, &mut buf, Depth::Ansi16, theme);
            let text = row(&buf, 0);
            assert!(text.contains(track), "{text}");
            for (label, mode) in [
                (" Cruise ", HarnessWord::Cruise),
                (" Land ", HarnessWord::Land),
                (" Off ", HarnessWord::Off),
            ] {
                let start = column_of(&text, label).unwrap();
                let end = start + label.len() as u16;
                let modifiers = if mode == selected {
                    Modifier::BOLD | Modifier::REVERSED
                } else {
                    Modifier::empty()
                };
                for x in start..end {
                    let cell = &buf[(x, 0)];
                    assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset), "{label}");
                    assert_eq!(cell.modifier, modifiers);
                    assert_eq!(mode_mouse_hit(x, 0), Some(mode));
                }
                for divider in [start - 1, end] {
                    let cell = &buf[(divider, 0)];
                    assert!(autopilot_menu_hit(divider, 0));
                    assert_eq!(mode_mouse_hit(divider, 0), None);
                    assert_eq!((cell.fg, cell.bg), edge_colors);
                    assert_eq!(cell.modifier, Modifier::empty());
                }
            }
            let glyph = column_of(&text, face).unwrap();
            assert_eq!(buf[(glyph, 0)].fg, Color::Reset);
            assert_eq!(buf[(glyph, 0)].bg, fill);
        }
    }
}

#[test]
fn each_segment_is_readable_against_its_track() {
    use crate::banner::palette;
    let _scene = scene(HarnessWord::Land, TaskFace::Open, None);
    for depth in [Depth::Ansi256, Depth::TrueColor] {
        for ground in [(16, 20, 24), (255, 255, 255)] {
            let area = Rect::new(0, 0, 80, 3);
            let mut buf = Buffer::empty(area);
            render(area, &mut buf, depth, Theme::from_bg(Some(ground)));
            let rgb = |color| match color {
                Color::Rgb(r, g, b) => palette::pack((r, g, b)),
                Color::Indexed(index) => palette::xterm256(index),
                other => panic!("unexpected switch color: {other:?}"),
            };
            for label in [" Cruise ", " Land ", " Off "] {
                let x = column_of(&row(&buf, 0), label).unwrap();
                let cell = &buf[(x + 1, 0)];
                let contrast = palette::contrast(rgb(cell.fg), rgb(cell.bg));
                assert!(
                    contrast >= palette::TEXT_CONTRAST_FLOOR,
                    "{depth:?}/{ground:?}: {label} {contrast}"
                );
            }
        }
    }
}

#[test]
fn config_refresh_does_not_undo_live_toggles_or_touch_automation() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    let shared = Box::leak(Box::new(codex_alab_core::session::Shared::new()));
    let _automation_scope = crate::boxes::scoped(shared);
    shared.set_autopilot(codex_alab_core::session::Autopilot::Active);
    assert!(!enabled());
    assert!(button_rect(Rect::new(0, 0, 80, 3)).is_none());
    configure(true);
    assert!(enabled());
    assert!(!toggle());
    configure(true);
    assert!(!enabled());
    let area = Rect::new(0, 0, 80, 3);
    let mut buf = Buffer::empty(area);
    render(area, &mut buf, Depth::Mono, Theme::default());
    let off_button = button_rect(area).expect("recovery switch after turning off");
    assert!(mouse_hit(off_button.x, off_button.y));
    assert!(toggle());
    assert!(
        !mouse_hit(off_button.x, off_button.y),
        "stale frame rejected"
    );
    render(area, &mut buf, Depth::Mono, Theme::default());
    let button = button_rect(area).unwrap();
    assert!(mouse_hit(button.x, button.y));
    assert!(mouse_hit(button.right() - 1, button.y));
    assert!(!mouse_hit(button.right(), button.y));
    assert!(!mouse_hit(button.x, button.y + 1));
    assert!(!toggle());
    assert_eq!(
        shared.autopilot(),
        codex_alab_core::session::Autopilot::Active
    );
    set_enabled(false);
    configure(true);
    assert!(!enabled());
}

#[test]
fn status_snapshots_replace_accounts_and_remain_current_while_off() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(false);
    set_status(InterfaceStatus {
        account: "first@example.com".into(),
        weekly: Some("weekly 62% left".into()),
        ..InterfaceStatus::default()
    });
    assert!(!enabled());
    assert_eq!(status_snapshot().unwrap().account, "first@example.com");
    set_status(InterfaceStatus::default());
    let current = status_snapshot().unwrap();
    assert_eq!(current.account, "Account unavailable");
    assert!(current.weekly.is_none());
    assert!(toggle());
    let area = Rect::new(0, 0, 80, 1);
    let mut buf = Buffer::empty(area);
    render_status_current(area, &mut buf, Depth::Mono, Theme::default());
    assert!(row(&buf, 0).starts_with("Account unavailable"));
    assert!(!row(&buf, 0).contains("first@example.com"));

    for (raw_account, expected) in [
        ("\0\u{7}\u{1b}", "Account unavailable"),
        (" \0\t\r\n ", "Account unavailable"),
        ("\u{200b}\u{200d}\u{2060}", "Account unavailable"),
        ("\u{301}", "Account unavailable"),
        ("ﾞ", "ﾞ"),
        ("  \0\t使\u{7}用者@例子.test\r\n  ", "使用者@例子.test"),
    ] {
        set_enabled(false);
        set_status(InterfaceStatus {
            account: raw_account.into(),
            plan: Some("Pro".into()),
            weekly: Some("weekly 62% left".into()),
            ..InterfaceStatus::default()
        });
        let snapshot = status_snapshot().unwrap();
        assert_eq!(snapshot.account, expected, "{raw_account:?}");
        assert_eq!(snapshot.plan.as_deref(), Some("Pro"));
        assert_eq!(snapshot.weekly.as_deref(), Some("weekly 62% left"));
        assert!(
            !account_mouse_hit(area.x, area.y),
            "Off retains native ownership"
        );

        set_enabled(true);
        render_status_current(area, &mut buf, Depth::Mono, Theme::default());
        let mut column = area.x;
        let expected_span = Span::raw(expected);
        for grapheme in expected_span.styled_graphemes(Style::default()) {
            assert_eq!(
                buf[(column, area.y)].symbol(),
                grapheme.symbol,
                "{raw_account:?}"
            );
            if !grapheme.symbol.chars().all(char::is_whitespace) {
                assert!(
                    account_mouse_hit(column, area.y),
                    "{raw_account:?} at {column}"
                );
            }
            column += grapheme.symbol.cell_width();
        }
        assert!(
            row(&buf, area.y)
                .trim_end()
                .ends_with(" / Pro / weekly 62% left"),
            "{raw_account:?}"
        );
        for join in column..column + 3 {
            assert!(
                account_mouse_hit(join, area.y),
                "the unit's own join is part of its one target: {join}"
            );
        }
        let end = column + " / Pro / weekly 62% left".len() as u16;
        assert!(account_mouse_hit(end - 1, area.y));
        assert!(
            !account_mouse_hit(end, area.y),
            "nothing past the unit is a target"
        );
        assert!(row(&buf, area.y).contains("weekly 62% left"));
        assert!(!row(&buf, area.y).chars().any(char::is_control));
    }
}

#[test]
fn scoped_state_is_isolated_and_restored() {
    let outer = Box::leak(Box::new(State::new()));
    let inner = Box::leak(Box::new(State::new()));
    let _scope = scoped(outer);
    configure(true);
    {
        let _scope = scoped(inner);
        configure(false);
        assert!(!enabled());
    }
    assert!(enabled());
}

#[test]
fn status_keeps_email_and_weekly_quota_before_secondary_fields() {
    let status = Status {
        email: Some("andrew@example.com"),
        weekly: Some("weekly 62% left"),
        branch: Some("main"),
        context: Some("context 28%"),
        model: Some("gpt-6"),
        ..Status::default()
    };
    let render = |width| text(&status_line(width, status, Depth::Mono, Theme::default()));
    assert_eq!(
        render(100),
        "andrew@example.com / weekly 62% left │ main · gpt-6 · context 28%"
    );
    let without_context = render(58);
    assert!(without_context.contains("andrew@example.com"));
    assert!(without_context.contains("weekly 62% left"));
    assert!(without_context.contains("gpt-6"));
    assert!(!without_context.contains("context"));
    let essentials = render(36);
    assert_eq!(essentials, "andrew@example.com / weekly 62% left");
    assert_eq!(render(27), "andrew@e… / weekly 62% left");
    assert_eq!(render(25), "weekly 62% left");
}

#[test]
fn narrow_status_keeps_the_end_of_similar_email_usernames() {
    let render = |email, width| {
        let status = Status {
            email: Some(email),
            weekly: Some("weekly 62% left"),
            ..Status::default()
        };
        text(&status_line(width, status, Depth::Mono, Theme::default()))
    };
    let main = "team.abc@example.test";
    let other = "team.abc.2@example.test";
    assert_eq!(render(main, 26), "team…abc / weekly 62% left");
    assert_eq!(render(other, 26), "team…c.2 / weekly 62% left");
    assert_eq!(render(main, 27), "team.abc… / weekly 62% left");
    assert_eq!(render(other, 27), "team…bc.2 / weekly 62% left");
    for width in 26..=80 {
        assert_ne!(render(main, width), render(other, width), "{width}");
    }
    assert_eq!(render(other, 80), format!("{other} / weekly 62% left"));
    assert_eq!(render(other, 25), "weekly 62% left");
}

#[test]
fn narrow_account_handles_fit_complete_graphemes_and_plain_labels() {
    let render = |email: &str, width| {
        let status = Status {
            email: Some(email),
            ..Status::default()
        };
        text(&status_line(width, status, Depth::Mono, Theme::default()))
    };
    for (account, expected) in [
        ("team.long.👩🏽‍💻@example.test", "team….👩🏽‍💻"),
        ("team.long.e\u{301}.2@example.test", "team…e\u{301}.2"),
        ("Account unavailable", "Account…"),
        ("team.abc.2@@example.test", "team.ab…"),
    ] {
        assert_eq!(render(account, 8), expected);
        for width in 0..=40 {
            assert!(painted_width(&render(account, width)) <= usize::from(width));
        }
    }
    for suffix in ["界", "語", "👩🏽‍💻"] {
        let account = format!("abcdef{suffix}@example.test");
        assert_eq!(render(&account, 4), format!("a…{suffix}"));
        assert_eq!(render(&account, 3), format!("…{suffix}"));
        assert_eq!(render(&account, 2), "a…");
    }
    assert_eq!(render("team.abc.2@example.test", 4), "te…2");
    assert_eq!(render("team.abc.2@example.test", 1), "…");
}

/// The account name, plan and allowances are one unit: joined by `/` inside and set apart by
/// `│` from the workspace fields, which keep `·` between them. The workspace fields go
/// first; then the unit shrinks as one: the five-hour allowance goes, the name shortens
/// beside the plan, the plan goes (the name taking its room back), the name goes, and the
/// weekly allowance, last, is cut to the width.
#[test]
fn status_shrinks_the_account_unit_as_one_keeping_weekly_last() {
    let status = Status {
        email: Some("andrew@example.com"),
        plan: Some("Pro"),
        weekly: Some("weekly 62% left"),
        five_hour: Some("5h 80% left"),
        branch: Some("main"),
        model: Some("gpt-6"),
        ..Status::default()
    };
    let render = |width| text(&status_line(width, status, Depth::Mono, Theme::default()));
    for (width, expected) in [
        (
            120,
            "andrew@example.com / Pro / weekly 62% left / 5h 80% left │ main · gpt-6",
        ),
        (
            70,
            "andrew@example.com / Pro / weekly 62% left / 5h 80% left │ main",
        ),
        (
            59,
            "andrew@example.com / Pro / weekly 62% left / 5h 80% left",
        ),
        (55, "andrew@example.com / Pro / weekly 62% left"),
        (41, "andrew@example.c… / Pro / weekly 62% left"),
        (32, "andrew@… / Pro / weekly 62% left"),
        (31, "andrew@examp… / weekly 62% left"),
        (26, "andrew@… / weekly 62% left"),
        (25, "weekly 62% left"),
        (10, "weekly 62…"),
        (1, "…"),
        (0, ""),
    ] {
        assert_eq!(render(width), expected, "{width}");
    }
    for width in 0..=120 {
        let line = render(width);
        assert!(
            UnicodeWidthStr::width(line.as_str()) <= usize::from(width),
            "{width}: {line:?}"
        );
        // Never a workspace field beside a shrunk unit, nor a unit without its weekly part.
        if line.contains('…') {
            assert!(
                !line.contains('│') && !line.contains('·'),
                "{width}: {line:?}"
            );
        }
        if width >= 7 {
            assert!(line.contains("weekly"), "{width}: {line:?}");
        }
        // The plan never outlives a name the unit could show in full.
        if line.contains("Pro") && !line.contains("andrew@example.com") {
            assert!(line.starts_with("andrew@"), "{width}: {line:?}");
        }
    }
    // Without a plan or a name the unit is the weekly allowance alone, still set apart.
    let alone = Status {
        weekly: Some("weekly 62% left"),
        branch: Some("main"),
        ..Status::default()
    };
    assert_eq!(
        text(&status_line(80, alone, Depth::Mono, Theme::default())),
        "weekly 62% left │ main"
    );
    // With no unit at all the workspace fields keep their own joins.
    let workspace = Status {
        branch: Some("main"),
        model: Some("gpt-6"),
        ..Status::default()
    };
    assert_eq!(
        text(&status_line(80, workspace, Depth::Mono, Theme::default())),
        "main · gpt-6"
    );
}

#[test]
fn status_width_is_bounded_with_missing_and_wide_unicode_values() {
    for divider in [ACCOUNT_JOIN, ACCOUNT_BOUNDARY, FIELD_JOIN] {
        assert_eq!(painted_width(divider), STATUS_JOIN_WIDTH);
    }
    for email in [
        None,
        Some("使用者@例子.test"),
        Some("لا@x.test"),
        Some("ｶﾞ@x.test"),
        Some("♥️@example.test"),
        Some("👩🏽‍💻@example.test"),
        Some("🇺🇳@example.test"),
        Some(""),
        Some("a\n\t@example.com"),
    ] {
        for weekly in [
            None,
            Some("weekly 90% left"),
            Some("週の残量 90%"),
            Some(""),
        ] {
            let status = Status {
                email,
                weekly,
                branch: Some("枝🌳"),
                context: Some("context 95%"),
                model: Some("model"),
                ..Status::default()
            };
            for width in 0..100 {
                let line = text(&status_line(width, status, Depth::Mono, Theme::default()));
                assert!(
                    painted_width(&line) <= usize::from(width),
                    "{width}: {line:?}"
                );
                assert!(!line.chars().any(char::is_control));
            }
        }
    }
}

#[test]
fn fitting_preserves_joined_emoji_and_combining_graphemes() {
    assert_eq!(fitted("لاxy", 3), "لا…");
    assert_eq!(fitted("ｶﾞxy", 3), "ｶﾞ…");
    assert_eq!(fitted("♥️@x", 2), "…");
    assert_eq!(fitted("👩‍💻@x", 3), "👩‍💻…");
    assert_eq!(fitted("🇺🇳@x", 3), "🇺🇳…");
    assert_eq!(fitted("a\u{301}@x", 2), "a\u{301}…");
}

/// One click range: every cell of the account unit, its own joins and spaces included, opens
/// `/alab account`; the boundary and the workspace fields never do.
#[test]
fn account_hits_cover_only_account_and_quota_text_cells() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    let area = Rect::new(7, 4, 160, 1);
    let mut buf = Buffer::empty(Rect::new(0, 0, 180, 8));
    let status = Status {
        email: Some("使用者@例子.test"),
        plan: Some("Max 5x"),
        weekly: Some("weekly 62% left"),
        five_hour: Some("5h 80% left"),
        activity: Some("Waiting"),
        mode: Some("Auto"),
        branch: Some("branch🌳"),
        model: Some("GPT-6"),
        context: Some("ctx 40%"),
    };
    render_status(area, &mut buf, status, Depth::Mono, Theme::default());
    assert_eq!(
        text(&status_line(
            area.width,
            status,
            Depth::Mono,
            Theme::default()
        )),
        "使用者@例子.test / Max 5x / weekly 62% left / 5h 80% left │ Waiting · Auto · branch🌳 \
         · GPT-6 · ctx 40%"
    );
    let mut column = area.x;
    let mut previous = None;
    for (value, in_unit) in [
        ("使用者@例子.test", true),
        ("Max 5x", true),
        ("weekly 62% left", true),
        ("5h 80% left", true),
        ("Waiting", false),
        ("Auto", false),
        ("branch🌳", false),
        ("GPT-6", false),
        ("ctx 40%", false),
    ] {
        if let Some(previous_in_unit) = previous {
            for _ in 0..3 {
                assert_eq!(
                    account_mouse_hit(column, area.y),
                    previous_in_unit && in_unit,
                    "join before {value} at {column}"
                );
                column += 1;
            }
        }
        previous = Some(in_unit);
        for ch in value.chars() {
            for _ in 0..UnicodeWidthChar::width(ch).unwrap_or(0) {
                assert_eq!(
                    account_mouse_hit(column, area.y),
                    in_unit,
                    "{value}: {column}"
                );
                column += 1;
            }
        }
    }
    assert!(!account_mouse_hit(area.x - 1, area.y));
    assert!(!account_mouse_hit(column, area.y));
    assert!(!account_mouse_hit(area.x, area.y - 1));
    assert!(!account_mouse_hit(area.x, area.y + 1));
}

#[test]
fn mixed_script_account_suffix_stays_visible_and_clickable() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    let area = Rect::new(3, 2, 40, 1);
    for account in ["لا@x.test", "ｶﾞ@x.test"] {
        for branch in [None, Some("main")] {
            set_status(InterfaceStatus {
                account: account.into(),
                weekly: Some(String::new()),
                branch: branch.map(str::to_owned),
                ..Default::default()
            });
            let mut buf = Buffer::empty(area);
            render_status_current(area, &mut buf, Depth::Mono, Theme::default());
            let last = area.x + 8;
            assert_eq!(buf[(last, area.y)].symbol(), "t", "{account} {branch:?}");
            assert!(account_mouse_hit(last, area.y), "{account} {branch:?}");
            assert!(!account_mouse_hit(last + 1, area.y));
            if branch.is_some() {
                assert_eq!(buf[(last + 2, area.y)].symbol(), "│");
            }
        }
    }
}

#[test]
fn mixed_script_weekly_reset_dots_keep_the_expiry_highlight() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    let area = Rect::new(3, 2, 110, 1);
    set_status(InterfaceStatus {
        account: "A".into(),
        weekly: Some("لا".repeat(24)),
        reset_credits: Some(ResetCredits {
            available_count: 2,
            expiring_count: 1,
            expired_count: 0,
            next_expiry: None,
            stale: false,
            details_known: true,
        }),
        ..Default::default()
    });
    let mut buf = Buffer::empty(area);
    render_status_current(area, &mut buf, Depth::Mono, Theme::default());
    let dots: Vec<_> = (area.x..area.right())
        .map(|x| &buf[(x, area.y)])
        .filter(|cell| matches!(cell.symbol(), "●" | "◉"))
        .map(|cell| cell.modifier.contains(Modifier::BOLD))
        .collect();
    assert_eq!(dots, [true, false]);
}

#[test]
fn responsive_account_hits_match_the_same_painted_status_line() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    let status = Status {
        email: Some("使用者👩‍💻@example.test"),
        weekly: Some("weekly62%"),
        branch: Some("topic/branch"),
        model: Some("GPT-6"),
        activity: Some("Working"),
        context: Some("ctx40%"),
        ..Default::default()
    };
    let palette = Palette::new(Depth::Mono, Theme::default());
    for width in 0..=120 {
        let area = Rect::new(7, 4, width, 1);
        let mut buf = Buffer::empty(Rect::new(0, 0, 140, 8));
        render_status(area, &mut buf, status, Depth::Mono, Theme::default());
        let line = status_line(width, status, Depth::Mono, Theme::default());
        // The unit is the name and the allowance as painted, and everything between them.
        let mut unit: Option<(usize, usize)> = None;
        let mut column = usize::from(area.x);
        for span in line.spans {
            let text_width = UnicodeWidthStr::width(span.content.as_ref());
            if text_width > 0 && (span.style == palette.brand || span.style == palette.accent) {
                let start = unit.map_or(column, |(start, _)| start);
                unit = Some((start, column + text_width));
            }
            column += text_width;
        }
        let mut expected = [false; 140];
        if let Some((start, end)) = unit {
            expected[start..end].fill(true);
        }
        for x in 0..140 {
            assert_eq!(
                account_mouse_hit(x, area.y),
                expected[usize::from(x)],
                "width {width}, cell {x}"
            );
        }
    }
}

#[test]
fn account_hits_follow_clipping_and_clear_after_empty_or_missing_status_paint() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    let status = Status {
        email: Some("owner@example.test"),
        weekly: Some("weekly62%"),
        ..Default::default()
    };
    let mut first = Buffer::empty(Rect::new(0, 0, 120, 10));
    render_status(
        Rect::new(3, 2, 100, 1),
        &mut first,
        status,
        Depth::Mono,
        Theme::default(),
    );
    assert!(account_mouse_hit(3, 2));
    let clip = Rect::new(70, 7, 12, 1);
    let mut clipped = Buffer::empty(clip);
    render_status(
        Rect::new(50, 7, 100, 1),
        &mut clipped,
        status,
        Depth::Mono,
        Theme::default(),
    );
    assert!(!account_mouse_hit(3, 2), "old position was invalidated");
    assert!(account_mouse_hit(clip.x, clip.y));
    for x in [clip.x - 1, clip.right()] {
        assert!(!account_mouse_hit(x, clip.y));
    }
    render_status(
        Rect::new(70, 7, 12, 0),
        &mut clipped,
        status,
        Depth::Mono,
        Theme::default(),
    );
    assert!(!account_mouse_hit(clip.x, clip.y));
    render_status(clip, &mut clipped, status, Depth::Mono, Theme::default());
    assert!(account_mouse_hit(clip.x, clip.y));
    render_status(
        Rect::new(1, 1, 10, 1),
        &mut clipped,
        status,
        Depth::Mono,
        Theme::default(),
    );
    assert!(
        !account_mouse_hit(clip.x, clip.y),
        "fully clipped paint clears old targets"
    );
    render_status(clip, &mut clipped, status, Depth::Mono, Theme::default());
    assert!(account_mouse_hit(clip.x, clip.y));
    render_status_current(clip, &mut clipped, Depth::Mono, Theme::default());
    assert!(
        !account_mouse_hit(clip.x, clip.y),
        "no current snapshot clears old targets"
    );
}

#[test]
fn account_hits_reject_changed_cluster_without_losing_activity_only_updates() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    let initial = InterfaceStatus {
        account: "first@example.test".into(),
        plan: Some("Pro".into()),
        weekly: Some("weekly62%".into()),
        five_hour: Some("5h80%".into()),
        ..Default::default()
    };
    let area = Rect::new(6, 3, 110, 1);
    let mut buf = Buffer::empty(area);
    set_status(initial.clone());
    render_status_current(area, &mut buf, Depth::Mono, Theme::default());
    let mut activity = initial.clone();
    activity.activity = Some("Working".into());
    activity.context = Some("ctx70%".into());
    activity.usage_checked_at = Some("now".into());
    set_status(activity);
    configure(true);
    assert!(
        account_mouse_hit(area.x, area.y),
        "routine refresh keeps painted account targets"
    );
    for field in 0..4 {
        set_status(initial.clone());
        render_status_current(area, &mut buf, Depth::Mono, Theme::default());
        let mut changed = initial.clone();
        match field {
            0 => changed.account = "second@example.test".into(),
            1 => changed.plan = Some("Plus".into()),
            2 => changed.weekly = Some("weekly61%".into()),
            _ => changed.five_hour = Some("5h79%".into()),
        }
        set_status(changed);
        assert!(
            !account_mouse_hit(area.x, area.y),
            "changed cluster field {field}"
        );
        set_status(initial.clone());
        assert!(
            !account_mouse_hit(area.x, area.y),
            "old geometry cannot revive without paint"
        );
    }
    render_status_current(area, &mut buf, Depth::Mono, Theme::default());
    assert!(account_mouse_hit(area.x, area.y));
    // A renderer that captured the preceding snapshot cannot make it clickable again.
    let old = initial.values();
    let mut changed = initial.clone();
    changed.account = "second@example.test".into();
    set_status(changed);
    render_status(area, &mut buf, old, Depth::Mono, Theme::default());
    assert!(!account_mouse_hit(area.x, area.y));
}

#[test]
fn off_and_explicit_choices_cannot_reuse_account_targets() {
    let state = Box::leak(Box::new(State::new()));
    let _scope = scoped(state);
    configure(true);
    let area = Rect::new(3, 3, 90, 1);
    let mut buf = Buffer::empty(area);
    let status = Status {
        email: Some("owner@example.test"),
        weekly: Some("weekly62%"),
        ..Default::default()
    };
    render_status(area, &mut buf, status, Depth::Mono, Theme::default());
    assert!(account_mouse_hit(area.x, area.y));
    set_enabled(false);
    assert!(!account_mouse_hit(area.x, area.y));
    set_enabled(true);
    assert!(!account_mouse_hit(area.x, area.y));
    render_status(area, &mut buf, status, Depth::Mono, Theme::default());
    assert!(account_mouse_hit(area.x, area.y));
    assert!(!toggle());
    assert!(toggle());
    assert!(!account_mouse_hit(area.x, area.y));
    let native = buf.clone();
    set_enabled(false);
    render_status(area, &mut buf, status, Depth::Mono, Theme::default());
    assert_eq!(
        buf, native,
        "off does not paint or claim the native status row"
    );
}

#[test]
fn rainbow_overline_keeps_text_readable_on_middle_gray_backgrounds() {
    use crate::banner::palette;
    let _scene = scene(HarnessWord::Unknown, TaskFace::Open, None);
    for gray in [112, 116, 120, 124, 128, 132] {
        let background = (gray, gray, gray);
        for depth in [Depth::TrueColor, Depth::Ansi256] {
            let area = Rect::new(0, 0, 80, 3);
            let mut buf = Buffer::empty(area);
            render(area, &mut buf, depth, Theme::from_bg(Some(background)));
            for x in 2..area.width - 2 {
                let cell = &buf[(x, 0)];
                if cell.symbol().trim().is_empty() {
                    continue;
                }
                let rgb = match cell.fg {
                    Color::Rgb(r, g, b) => palette::pack((r, g, b)),
                    Color::Indexed(index) => palette::xterm256(index),
                    other => panic!("unexpected foreground {other:?}"),
                };
                let ratio = palette::contrast(rgb, palette::pack(background));
                assert!(
                    ratio >= palette::TEXT_CONTRAST_FLOOR,
                    "gray {gray}/{depth:?} at column {x}: {ratio:.2}:1"
                );
            }
            // At these backgrounds the indexed palette can lack five colored entries
            // at 4.5:1. Readability takes priority over the number of distinct colors.
        }
    }
}

/// THE LINE'S AUTOPILOT FACE (gap 12 of the parity sweep, 2026-10-29): an enhanced interface
/// over a `Shared` whose primary thread is `t1`, wearing `word` with `t1`'s task face `task` and
/// aterm's timer `land_at`. The scopes go with the returned guards.
fn scene(
    word: HarnessWord,
    task: TaskFace,
    land_at: Option<u64>,
) -> (
    &'static State,
    &'static Shared,
    Scope,
    crate::boxes::Scope,
    crate::fixture_clock::Scope,
) {
    let clock = crate::fixture_clock::scoped(scene_noon().to_utc());
    let state = Box::leak(Box::new(State::new()));
    let scope = scoped(state);
    let shared = Box::leak(Box::new(Shared::new()));
    let automation = crate::boxes::scoped(shared);
    configure(true);
    shared.observe(
        &codex_alab_core::session::Frame {
            primary_thread: Some("t1"),
            displayed_thread: Some("t1"),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: std::path::Path::new("/home/.codex"),
            embedded: true,
            state: codex_alab_core::session::ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        10,
    );
    shared.set_autopilot_with_word(Autopilot::Active, word);
    shared.set_task_face("t1", task);
    shared.set_land_at(land_at);
    (state, shared, scope, automation, clock)
}

fn scene_noon() -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    chrono::Local
        .with_ymd_and_hms(2026, 11, 19, 12, 0, 0)
        .unwrap()
}

/// A same-day timer in the shared rendering fixture, independent of the test runner's clock.
fn an_hour_on() -> (u64, String) {
    let at = scene_noon() + chrono::Duration::hours(1);
    (
        u64::try_from(at.timestamp()).unwrap(),
        at.format("%H:%M").to_string(),
    )
}

/// The six faces, each as its scene's facts and as drawn whole.
fn six_faces() -> Vec<(HarnessWord, TaskFace, Option<u64>, String)> {
    let (at, clock) = an_hour_on();
    vec![
        (HarnessWord::Cruise, TaskFace::Open, None, "∞".to_string()),
        (
            HarnessWord::Cruise,
            TaskFace::Open,
            Some(at),
            format!("∞ ⤓{clock}"),
        ),
        (
            HarnessWord::Land,
            TaskFace::Open,
            None,
            "⤓ land".to_string(),
        ),
        (
            HarnessWord::Land,
            TaskFace::Landed,
            None,
            "✓ landed".to_string(),
        ),
        (
            HarnessWord::Land,
            TaskFace::Theirs,
            None,
            "⤓ quiet".to_string(),
        ),
        (HarnessWord::Off, TaskFace::Open, None, "○ off".to_string()),
    ]
}

/// The cell `needle` starts at in `text`, a row of [`row`].
fn column_of(text: &str, needle: &str) -> Option<u16> {
    text.find(needle)
        .map(|at| UnicodeWidthStr::width(&text[..at]) as u16)
}

/// The drawn text in the version target on the composer's first row.
fn brand_of(text: &str) -> String {
    let Some(rect) = state().unwrap().overline_hits.lock().unwrap().build else {
        return String::new();
    };
    text.chars()
        .skip(usize::from(rect.x))
        .take(usize::from(rect.width))
        .collect()
}

/// One enhanced frame of `area`'s composer, in mono on the default ground.
fn paint(area: Rect) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, area.right(), area.bottom()));
    render(area, &mut buf, Depth::Mono, Theme::default());
    buf
}

/// EACH OF ATERM'S FACES ON THE LINE, from the facts it is read from: the task under Land only
/// and the timer under Cruise only, as aterm's `ChipFace::of` reads them. NEGATIVE CONTROL: a
/// landing under Cruise or Off, and a timer under Land, wear nothing of theirs.
#[test]
fn the_line_wears_atermss_autopilot_faces() {
    let (at, _) = an_hour_on();
    let pair_width = running_pair().width() as u16;
    let mut table = six_faces();
    table.extend([
        // NEGATIVE CONTROL.
        (HarnessWord::Cruise, TaskFace::Landed, None, "∞".to_string()),
        (
            HarnessWord::Off,
            TaskFace::Landed,
            None,
            "○ off".to_string(),
        ),
        (
            HarnessWord::Land,
            TaskFace::Open,
            Some(at),
            "⤓ land".to_string(),
        ),
    ]);
    for word in [
        HarnessWord::HarnessOff,
        HarnessWord::AgentOff,
        HarnessWord::Unhosted,
    ] {
        table.push((word, TaskFace::Open, None, "⊘ off".to_string()));
    }
    for (word, task, land_at, face) in table {
        let _scene = scene(word, task, land_at);
        for width in (pair_width + 22..=pair_width + 26).chain([120]) {
            let text = row(&paint(Rect::new(0, 0, width, 4)), 0);
            let face = if width < 120 && word == HarnessWord::Cruise && land_at.is_none() {
                "∞ cruise"
            } else {
                face.as_str()
            };
            let after = if width < 120 && selectable_mode(word) {
                '▾'
            } else {
                ' '
            };
            assert!(
                text.contains(&format!("─ {face}{after}─")),
                "{width}: {word:?} {task:?} {land_at:?}: {text}"
            );
            if width < 120 {
                assert!(text.ends_with("─[on]  "), "{text}");
            }
        }
    }
}

fn current_brand_label(available: usize, reserve: usize) -> Option<String> {
    fit_brand_label(
        available,
        codex_alab_core::version::alab(),
        running_pair(),
        crate::upgrade::pending(),
        reserve,
    )
}

fn running_pair() -> String {
    release_pair(
        codex_alab_core::version::alab(),
        codex_alab_core::version::display_baseline().unwrap_or("unreleased"),
    )
}

const COMPACT_LAND_TRACK: &str = "▏Cruise[Land]Off▕";
const PADDED_LAND_TRACK: &str = "▏ Cruise [ Land ] Off ▕";

#[test]
fn resizing_keeps_the_landed_status_and_retargets_each_drawn_control() {
    let _scene = scene(HarnessWord::Land, TaskFace::Landed, None);
    let pair = running_pair();
    // A patch release such as 0.12 on Codex 0.169.1 takes 21 cells.
    let label_width = pair.width() as u16;
    let layouts = [
        (label_width + 51, PADDED_LAND_TRACK, "[ALab on]"),
        (label_width + 50, PADDED_LAND_TRACK, "[on]"),
        (label_width + 46, PADDED_LAND_TRACK, "[on]"),
        (label_width + 45, COMPACT_LAND_TRACK, "[on]"),
        (label_width + 40, COMPACT_LAND_TRACK, "[on]"),
        (label_width + 39, COMPACT_LAND_TRACK, "[ALab on]"),
        (label_width + 38, COMPACT_LAND_TRACK, "[ALab on]"),
        (label_width + 34, COMPACT_LAND_TRACK, "[ALab on]"),
        (label_width + 33, COMPACT_LAND_TRACK, "[on]"),
        (label_width + 29, COMPACT_LAND_TRACK, "[on]"),
        (label_width + 28, COMPACT_LAND_TRACK, "[on]"),
        (label_width + 27, COMPACT_LAND_TRACK, "[on]"),
        (label_width + 26, "", "[on]"),
        (label_width + 22, "", "[on]"),
        (label_width + 21, "", "[ALab on]"),
    ];
    let right = 5 + layouts[0].0;
    for (width, track, toggle) in layouts.into_iter().rev().chain(layouts) {
        let area = Rect::new(5, 2, width, 4);
        let buf = paint(area);
        let text = row(&buf, area.y);
        let tight_track = matches!(width - label_width, 27 | 28);
        let version_x = area.x + if tight_track { 2 } else { 3 };
        assert_eq!(column_of(&text, &pair), Some(version_x), "{text}");
        assert!(text.ends_with(&format!("{toggle}  ")), "{text}");
        let toggle_x = column_of(&text, toggle).unwrap();
        assert_eq!(
            button_rect(area),
            Some(Rect::new(toggle_x, area.y, toggle.len() as u16, 1)),
            "{text}"
        );
        let mut expected_modes = vec![None; usize::from(right)];
        let mut track_cells = 0..0;
        if track.is_empty() {
            assert!(!text.contains('▏'), "{text}");
        } else {
            let track_x = column_of(&text, track).expect("the complete three-way switch");
            track_cells = track_x..track_x + track.width() as u16;
            let gap = u16::from(width != label_width + 27);
            assert_eq!(track_x + track.width() as u16 + gap, toggle_x, "{text}");
            for (part, mode) in track.trim_matches(['▏', '▕']).split(['[', ']']).zip([
                HarnessWord::Cruise,
                HarnessWord::Land,
                HarnessWord::Off,
            ]) {
                let start = track_x + column_of(track, part).unwrap();
                let cells = start..start + part.len() as u16;
                for x in cells {
                    expected_modes[usize::from(x)] = Some(mode);
                    assert_eq!(
                        buf[(x, area.y)].modifier.contains(Modifier::REVERSED),
                        mode == HarnessWord::Land,
                        "{text}"
                    );
                }
            }
        }
        let face = if text.contains("✓ landed") {
            "✓ landed"
        } else {
            "✓"
        };
        let x = usize::from(column_of(&text, face).unwrap_or_else(|| panic!("{width}: {text}")));
        expected_modes[x..x + face.width()].fill(Some(HarnessWord::Off));
        for x in 0..right {
            let selected = expected_modes[usize::from(x)];
            assert_eq!(mode_mouse_hit(x, area.y), selected, "{x}: {text}");
            assert_eq!(
                autopilot_menu_hit(x, area.y),
                selected.is_some()
                    || track_cells.contains(&x)
                    || (track.is_empty() && x == toggle_x - 2),
                "{x}: {text}"
            );
            assert_eq!(
                mouse_hit(x, area.y),
                (toggle_x..toggle_x + toggle.len() as u16).contains(&x),
                "{x}: {text}"
            );
            assert_eq!(
                build_mouse_hit(x, area.y),
                (version_x..version_x + label_width).contains(&x),
                "{x}: {text}"
            );
            assert!(
                !mouse_hit(x, area.y + 1)
                    && !build_mouse_hit(x, area.y + 1)
                    && !autopilot_menu_hit(x, area.y + 1)
            );
        }
    }
}

#[test]
fn narrow_release_keeps_mouse_access_to_autopilot_until_the_track_fits() {
    let _scene = scene(HarnessWord::Land, TaskFace::Open, None);
    let pair = running_pair();
    let pair_width = pair.width() as u16;
    let short = display_release(codex_alab_core::version::alab());
    // From the first complete short release through the full version pair and named modes.
    for width in short.width() as u16 + 11..=pair_width + 29 {
        let area = Rect::new(5, 2, width, 4);
        let text = row(&paint(area), area.y);
        assert_eq!(
            text.contains(&pair),
            width >= pair_width + 15,
            "{width}: {text}"
        );
        assert!(build_mouse_hit(area.x + 3, area.y));
        let (label, mode) = if width >= pair_width + 27 {
            ("Land", HarnessWord::Land)
        } else {
            ("⤓", HarnessWord::Off)
        };
        let x = column_of(&text, label).unwrap_or_else(|| panic!("{width}: {text}"));
        assert_eq!(mode_mouse_hit(x, area.y), Some(mode), "{width}: {text}");
        assert!(autopilot_menu_hit(x, area.y) && !autopilot_menu_hit(x, area.y + 1));
        assert!(!build_mouse_hit(x, area.y) && !mouse_hit(x, area.y));
        let button = button_rect(area).unwrap();
        assert_eq!(button.right(), area.right() - 2);
        assert!(mouse_hit(button.x, area.y) && mouse_hit(button.right() - 1, area.y));
        if width < pair_width + 27 {
            let face = if text.contains("⤓ land") {
                "⤓ land"
            } else {
                "⤓"
            };
            let arrow = column_of(&text, "▾").expect("collapsed switch has a picker");
            assert_eq!(arrow, x + face.width() as u16, "{text}");
            assert!(autopilot_disclosure_hit(arrow, area.y));
            assert_eq!(mode_mouse_hit(arrow, area.y), None);
        } else {
            assert!(!text.contains('▾'), "{text}");
            for (name, mode) in [
                ("Cruise", HarnessWord::Cruise),
                ("Land", HarnessWord::Land),
                ("Off", HarnessWord::Off),
            ] {
                let x = column_of(&text, name).expect("all three named modes");
                assert_eq!(mode_mouse_hit(x, area.y), Some(mode), "{text}");
            }
        }
    }
}

#[test]
fn widening_a_short_release_keeps_the_autopilot_glyph_and_picker_clickable() {
    let short = display_release(codex_alab_core::version::alab());
    let short_width = short.width() as u16;
    for (word, task, glyph, target) in [
        (
            HarnessWord::Cruise,
            TaskFace::Open,
            "∞",
            Some(HarnessWord::Land),
        ),
        (
            HarnessWord::Land,
            TaskFace::Open,
            "⤓",
            Some(HarnessWord::Off),
        ),
        (
            HarnessWord::Land,
            TaskFace::Landed,
            "✓",
            Some(HarnessWord::Off),
        ),
        (
            HarnessWord::Off,
            TaskFace::Open,
            "○",
            Some(HarnessWord::Cruise),
        ),
        (HarnessWord::HarnessOff, TaskFace::Open, "⊘", None),
    ] {
        let _scene = scene(word, task, None);
        // Cross the first glyph, the short label and the full edition button in both directions.
        let max_width = short_width + 21;
        let widths = 8..=max_width;
        for width in widths.clone().chain(widths.rev()) {
            let area = Rect::new(5, 2, width, 4);
            let buf = paint(area);
            let text = row(&buf, area.y);
            let toggle = if width < 20 || (short_width + 15..short_width + 20).contains(&width) {
                "[on]"
            } else {
                "[ALab on]"
            };
            let button = button_rect(area).unwrap();
            assert_eq!(column_of(&text, toggle), Some(button.x), "{text}");
            assert_eq!(button.right(), area.right() - 2);
            let space = button.x.saturating_sub(area.x + 3);
            let has_release = space >= short_width + 2;
            let release_x = column_of(&text, short);
            assert_eq!(release_x.is_some(), has_release, "{text}");
            if let Some(start) = release_x {
                if target.is_some() && space == short_width + 2 {
                    assert!((area.x + 2..=area.x + 3).contains(&start), "{text}");
                } else {
                    assert_eq!(start, area.x + 3, "{text}");
                }
                assert_eq!(buf[(start + short_width, area.y)].symbol(), " ", "{text}");
            }
            for x in (area.x..area.x + 2).chain(area.right() - 2..area.right()) {
                assert_eq!(buf[(x, area.y)], buf[(x, area.y + 1)], "{text}");
            }
            let glyph_x = column_of(&text, glyph);
            assert_eq!(glyph_x.is_some(), width >= 10, "{text}");
            let bare_unavailable = target.is_none()
                && ((10..=12).contains(&width)
                    || (has_release && space - short_width - 2 <= GLYPH_SLOT));
            if width == 10 || bare_unavailable {
                assert!(text.contains(&format!(" {glyph}{toggle}")), "{text}");
            }
            let picker_x = (target.is_some() && width >= 11).then(|| glyph_x.unwrap() + 1);
            assert_eq!(column_of(&text, "▾"), picker_x, "{text}");
            if target.is_some() && width == short_width + 11 {
                assert!(text.contains(&format!("{short} {glyph}▾")), "{text}");
            }
            for x in 0..area.x + max_width {
                let on_glyph = glyph_x == Some(x);
                assert_eq!(mode_mouse_hit(x, area.y), target.filter(|_| on_glyph));
                assert_eq!(autopilot_words_hit(x, area.y), on_glyph && target.is_none());
                let disclosure = picker_x == Some(x);
                assert_eq!(autopilot_disclosure_hit(x, area.y), disclosure, "{text}");
                assert_eq!(autopilot_menu_hit(x, area.y), on_glyph || disclosure);
                assert_eq!(mouse_hit(x, area.y), button.contains((x, area.y).into()));
                assert_eq!(
                    build_mouse_hit(x, area.y),
                    release_x.is_some_and(|start| (start..start + short_width).contains(&x)),
                    "{text}"
                );
                assert!(!autopilot_menu_hit(x, area.y + 1));
            }
        }
    }
}

#[test]
fn an_unreported_mode_uses_the_available_space_for_both_versions() {
    let _scene = scene(HarnessWord::Unknown, TaskFace::Open, None);
    let pair = running_pair();
    let pair_width = pair.width() as u16;
    for width in pair_width + 10..=pair_width + 17 {
        let area = Rect::new(5, 2, width, 4);
        let text = row(&paint(area), area.y);
        let has_pair = width >= pair_width + 11;
        assert_eq!(text.contains(&pair), has_pair, "{width}: {text}");
        if has_pair {
            assert!(build_mouse_hit(area.x + 3 + pair_width - 1, area.y));
            let toggle = if width < pair_width + 16 {
                "[on]"
            } else {
                "[ALab on]"
            };
            let x = column_of(&text, toggle).expect("presentation button");
            assert!(mouse_hit(x, area.y) && !build_mouse_hit(x, area.y));
        }
        assert!((area.x..area.right()).all(|x| !autopilot_menu_hit(x, area.y)));
    }
}

#[test]
fn narrow_settings_face_explains_its_state_and_unknown_has_no_target() {
    let pair = running_pair();
    let area = Rect::new(5, 2, pair.width() as u16 + 16, 4);
    for word in [
        HarnessWord::HarnessOff,
        HarnessWord::AgentOff,
        HarnessWord::Unhosted,
        HarnessWord::Unknown,
    ] {
        let _scene = scene(word, TaskFace::Open, None);
        let text = row(&paint(area), area.y);
        assert!(text.contains(&pair), "{word:?}: {text}");
        let glyph = column_of(&text, "⊘");
        assert_eq!(
            glyph.is_some(),
            word != HarnessWord::Unknown,
            "{word:?}: {text}"
        );
        for x in area.x..area.right() {
            assert_eq!(mode_mouse_hit(x, area.y), None);
            assert_eq!(autopilot_words_hit(x, area.y), glyph == Some(x));
            assert_eq!(autopilot_menu_hit(x, area.y), glyph == Some(x));
        }
        let on = button_rect(area).unwrap();
        set_enabled(false);
        let vanilla = row(&paint(area), area.y);
        assert!(vanilla.contains("[ALab off]") && !vanilla.contains(&pair));
        assert!(mouse_hit(on.x, area.y));
        assert!((area.x..area.right()).all(|x| !autopilot_menu_hit(x, area.y)));
    }
}

/// The brand, the three label positions and the edition button stay put as the
/// reported mode or its status changes. Unknown leaves only the brand and edition.
#[test]
fn no_face_moves_the_brand_or_the_controls() {
    let minimum = running_pair().width() as u16 + 27;
    for width in 40..=200 {
        let area = Rect::new(0, 0, width, 4);
        let mut seen = None;
        for (word, task, land_at, face) in six_faces() {
            let _scene = scene(word, task, land_at);
            let buf = paint(area);
            let text = row(&buf, 0);
            let brand = brand_of(&text);
            let controls =
                ["Cruise", "Land", "Off", "[ALab on]", "[on]"].map(|label| column_of(&text, label));
            assert_eq!(controls[0].is_some(), width >= minimum, "{width} {face}");
            let now = (brand, controls);
            if let Some(seen) = &seen {
                assert_eq!(seen, &now, "{width} {face}");
            }
            seen = Some(now);
        }
    }
    // NEGATIVE CONTROL.
    let _scene = scene(HarnessWord::Unknown, TaskFace::Open, None);
    let text = row(&paint(Rect::new(0, 0, 120, 4)), 0);
    assert!(
        !brand_of(&text).trim().is_empty() && !text.contains("Cruise"),
        "{text}"
    );
}

/// The status always has a glyph beside the track. Spare cells provide padding,
/// then the full timer or landing text.
#[test]
fn the_face_uses_only_space_left_by_the_version_and_switch() {
    let pair = running_pair();
    for (word, task, land_at, face) in six_faces() {
        let _scene = scene(word, task, land_at);
        let glyph = if land_at.is_some() && compact_clock_supported() {
            '◷'
        } else {
            face.chars().next().unwrap()
        };
        for width in pair.width() as u16 + 27..=200 {
            let area = Rect::new(0, 0, width, 4);
            let text = row(&paint(area), 0);
            let edge = if word == HarnessWord::Cruise {
                "["
            } else {
                "▏"
            };
            let switch = column_of(&text, edge).unwrap();
            let label_x = column_of(&text, &pair).expect("full release beside the track");
            let available = usize::from(switch - label_x);
            let label = current_brand_label(available, 0).unwrap();
            assert_eq!(brand_of(&text).trim(), label, "{width}: {text}");
            let spare = available - label.width() - 2;
            let expected = if spare > usize::from(FACE_SLOT) {
                format!("─ {face} ─{edge}")
            } else if spare > usize::from(GLYPH_SLOT) {
                format!("─ {glyph} ─{edge}")
            } else {
                format!(" {glyph}{edge}")
            };
            assert!(text.contains(&expected), "{width} {face}: {text}");
        }
    }
}

#[test]
fn composer_weekday_uses_spare_rule_cells_and_preserves_existing_actions() {
    use chrono::TimeZone;
    let at = chrono::Local
        .with_ymd_and_hms(2026, 11, 20, 6, 0, 0)
        .unwrap();
    let timestamp = u64::try_from(at.timestamp()).unwrap();
    let (_state, shared, _scope, _automation, _clock) =
        scene(HarnessWord::Cruise, TaskFace::Open, Some(timestamp));
    let frame = |area: Rect, pending| {
        let policy = shared.harness();
        let harness = (policy.1 != HarnessWord::Unknown).then_some((shared, policy));
        let layout = ComposerControls::fit(
            area,
            true,
            button_for(area, true).unwrap(),
            harness,
            pending,
        );
        let mut buffer = Buffer::empty(Rect::new(0, 0, area.right(), area.bottom()));
        let hits = render_border(area, &mut buffer, &layout, Depth::Mono, Theme::default());
        (row(&buffer, area.y), hits, layout.track.map(|(x, _)| x))
    };
    for pending in [false, true] {
        let mut gaps = std::collections::BTreeSet::new();
        for width in 13..=128 {
            let area = Rect::new(width % 7, width % 3, width, 4);
            shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Cruise);
            shared.set_land_at(Some(timestamp));
            let (today, old, track) = {
                let _today = crate::fixture_clock::scoped(at.to_utc());
                frame(area, pending)
            };
            let (tomorrow, actual, new_track) = frame(area, pending);
            assert_eq!(
                (actual.button, actual.build, new_track),
                (old.button, old.build, track)
            );
            let mut expected_row = today.clone();
            let mut expected_modes = old.modes;
            let gap = track.zip(old.build).map(|(x, build)| x - build.right());
            if let Some(gap) = gap {
                gaps.insert(gap);
            }
            if let Some(clock) = column_of(&today, "∞ ⤓06:00") {
                let original_has_weekday_room =
                    today.split_once("∞ ⤓06:00").unwrap().0.ends_with("───── ");
                if gap.is_some_and(|gap| gap >= 13) || original_has_weekday_room {
                    let label = if gap == Some(13) {
                        "∞⤓Fri 06:00"
                    } else {
                        "∞ ⤓Fri 06:00"
                    };
                    let width = label.width() as u16;
                    let old_end = clock + "∞ ⤓06:00".width() as u16;
                    // The 13- and 14-cell gaps borrow the rule cell immediately before the switch.
                    let end = old_end + u16::from(matches!(gap, Some(13 | 14)));
                    let start = end - width;
                    let mut cells: Vec<_> = today.chars().collect();
                    cells[usize::from(start - 1)] = ' ';
                    for (index, ch) in label.chars().enumerate() {
                        cells[usize::from(start) + index] = ch;
                    }
                    if end != old_end {
                        cells[usize::from(end)] = ' ';
                    }
                    expected_row = cells.into_iter().collect();

                    let (face, action) = expected_modes
                        .iter_mut()
                        .find(|(rect, _)| rect.contains((clock, area.y).into()))
                        .unwrap();
                    assert_eq!(*action, Hit::Mode(HarnessWord::Land));
                    assert!(start <= face.x && end >= face.right());
                    *face = Rect::new(start, area.y, width, 1);
                }
            }
            assert_eq!(tomorrow, expected_row, "{width} columns, pending={pending}");
            assert_eq!(actual.modes, expected_modes, "{tomorrow}");

            for (policy, word, deadline) in [
                (Autopilot::Passive, HarnessWord::Cruise, Some(timestamp)),
                (Autopilot::Off, HarnessWord::Cruise, Some(timestamp)),
                (Autopilot::Active, HarnessWord::Land, Some(timestamp)),
                (Autopilot::Active, HarnessWord::Off, Some(timestamp)),
                (Autopilot::Passive, HarnessWord::Unhosted, Some(timestamp)),
                (Autopilot::Passive, HarnessWord::Unknown, Some(timestamp)),
                (Autopilot::Active, HarnessWord::Cruise, None),
            ] {
                shared.set_autopilot_with_word(policy, word);
                shared.set_land_at(deadline);
                let (future, future_hits, _) = frame(area, pending);
                let _today = crate::fixture_clock::scoped(at.to_utc());
                let (same_day, same_hits, _) = frame(area, pending);
                assert_eq!(future, same_day, "{width} {policy:?} {word:?}");
                assert!(!future.contains("Fri"), "{future}");
                assert_eq!(future_hits.modes, same_hits.modes);
                if word == HarnessWord::Cruise && deadline.is_some() {
                    assert_eq!(future, today);
                }
            }
        }
        assert!(gaps.iter().any(|&gap| gap < 13), "{gaps:?}");
        assert!((13..=18).all(|gap| gaps.contains(&gap)), "{gaps:?}");
    }
}

#[test]
fn a_full_cruise_face_names_the_mode_when_the_switch_is_hidden() {
    let _scene = scene(HarnessWord::Cruise, TaskFace::Open, None);
    let pair_width = running_pair().width() as u16;
    // Resize out of the full face, into the named switch, then back again.
    for (extra, label) in [
        (21, "∞"),
        (22, "∞ cruise"),
        (27, "∞"),
        (40, "∞"),
        (22, "∞ cruise"),
    ] {
        let area = Rect::new(5, 2, pair_width + extra, 4);
        let text = row(&paint(area), area.y);
        let start = column_of(&text, label).expect("current Cruise face");
        let end = start + label.width() as u16;
        assert_eq!(text.contains("∞ cruise"), label == "∞ cruise", "{text}");
        assert_eq!(text.contains("Cruise"), extra >= 27, "{text}");
        assert!(text.contains(&running_pair()), "{text}");
        for x in start..end {
            assert_eq!(mode_mouse_hit(x, area.y), Some(HarnessWord::Land), "{text}");
            assert!(autopilot_menu_hit(x, area.y));
            assert!(!autopilot_menu_hit(x, area.y + 1));
        }
        if label == "∞ cruise" {
            let button = button_rect(area).unwrap();
            // The former infinity cell is still part of the same primary and secondary target.
            assert_eq!(end - 1, button.x - 3);
            assert!(!autopilot_menu_hit(start - 1, area.y));
            assert!(autopilot_disclosure_hit(end, area.y));
            assert!(!autopilot_menu_hit(end + 1, area.y));
        }
    }
}

#[test]
fn compact_clock_distinguishes_scheduled_cruise_only_beside_the_named_switch() {
    let inks = FaceInks::of(Depth::Mono, Theme::default());
    let scheduled = Face::CruiseUntil(Landing {
        time: chrono::NaiveTime::parse_from_str("18:00", "%H:%M").unwrap(),
        day: None,
    });
    assert_eq!(scheduled.glyph(true), "◷");
    let clock = if compact_clock_supported() {
        "◷"
    } else {
        "∞"
    };
    for (face, slot, beside_switch, expected) in [
        (&scheduled, GLYPH_SLOT, true, clock),
        (&scheduled, GLYPH_SLOT, false, "∞"),
        (&scheduled, FACE_SLOT, true, "∞ ⤓18:00"),
        (&scheduled, FACE_SLOT, false, "∞ ⤓18:00"),
        (&Face::Cruise, GLYPH_SLOT, true, "∞"),
    ] {
        let line = face.line(slot, beside_switch, inks);
        assert_eq!(line.to_string(), expected);
        assert_eq!(line.width(), expected.chars().count());
        assert_eq!(line.spans[0].style, inks.accent);
        assert_eq!(face.hit(), Hit::Mode(HarnessWord::Land));
    }
}

/// A FACE CLICK MOVES THE MODE AS A CLICK ON ATERM'S CHIP DOES (`ChipFace::click`): land from
/// either Cruise (a timed one's "land now"), off from any Land, cruise from Off. NEGATIVE
/// CONTROL: the spaces either side of the face, and the rule, click nothing.
#[test]
fn a_face_click_moves_the_mode_as_atermss_chip_does() {
    let clicks = [
        HarnessWord::Land,
        HarnessWord::Land,
        HarnessWord::Off,
        HarnessWord::Off,
        HarnessWord::Off,
        HarnessWord::Cruise,
    ];
    for ((word, task, land_at, face), click) in six_faces().into_iter().zip(clicks) {
        let _scene = scene(word, task, land_at);
        let text = row(&paint(Rect::new(0, 0, 120, 4)), 0);
        let x = column_of(&text, &format!(" {face} ─")).unwrap() + 1;
        let face_width = UnicodeWidthStr::width(face.as_str()) as u16;
        for x in x..x + face_width {
            assert_eq!(mode_mouse_hit(x, 0), Some(click), "{face} at {x}");
            assert_eq!(mode_mouse_hit(x, 1), None);
        }
        // NEGATIVE CONTROL.
        assert_eq!(mode_mouse_hit(x - 1, 0), None, "{face}");
        assert_eq!(mode_mouse_hit(x - 2, 0), None, "{face}");
        assert_eq!(mode_mouse_hit(x + face_width, 0), None, "{face}");
        for (label, mode) in [
            (" Cruise ", HarnessWord::Cruise),
            (" Land ", HarnessWord::Land),
            (" Off ", HarnessWord::Off),
        ] {
            let at = column_of(&text, label).unwrap();
            assert_eq!(mode_mouse_hit(at, 0), Some(mode), "{face} {label}");
        }
    }
}

/// THE FACE'S INKS CLEAR THE TEXT FLOOR on dark, light and middle-gray grounds at every colour
/// depth that inks, as the overline's own labels do; mono keeps the terminal's ink, bold or dim.
#[test]
fn face_inks_clear_the_text_floor() {
    use crate::banner::palette;
    let grounds = [(16, 20, 24), (255, 255, 255), (236, 236, 236)]
        .into_iter()
        .chain((112..=132).step_by(4).map(|gray| (gray, gray, gray)));
    for ground in grounds {
        for depth in [Depth::TrueColor, Depth::Ansi256] {
            for (word, task, land_at, face) in six_faces() {
                let _scene = scene(word, task, land_at);
                let area = Rect::new(0, 0, 120, 4);
                let mut buf = Buffer::empty(area);
                render(area, &mut buf, depth, Theme::from_bg(Some(ground)));
                let text = row(&buf, 0);
                let x = column_of(&text, &format!(" {face} ─")).unwrap() + 1;
                for x in x..x + UnicodeWidthStr::width(face.as_str()) as u16 {
                    let cell = &buf[(x, 0)];
                    if cell.symbol() == " " {
                        continue;
                    }
                    // Each depth's own inks, never the last depth's (review of gap 12, finding 11).
                    let rgb = match (depth, cell.fg) {
                        (Depth::TrueColor, Color::Rgb(r, g, b)) => palette::pack((r, g, b)),
                        (Depth::Ansi256, Color::Indexed(index)) => palette::xterm256(index),
                        (_, other) => {
                            panic!("{face} at {depth:?}: unexpected foreground {other:?}")
                        }
                    };
                    let ratio = palette::contrast(rgb, palette::pack(ground));
                    assert!(
                        ratio >= palette::TEXT_CONTRAST_FLOOR,
                        "{face} on {ground:?}/{depth:?} at {x}: {ratio:.2}:1"
                    );
                }
            }
        }
    }
    for (word, task, land_at, face) in six_faces() {
        let _scene = scene(word, task, land_at);
        let buf = paint(Rect::new(0, 0, 120, 4));
        let text = row(&buf, 0);
        let x = column_of(&text, &format!(" {face} ─")).unwrap() + 1;
        for x in x..x + UnicodeWidthStr::width(face.as_str()) as u16 {
            assert_eq!(buf[(x, 0)].fg, Color::Reset, "{face} at {x}");
        }
    }
}

/// THE TIMER'S CLOCK IS LOCAL TIME, AND NAMES ITS DAY WHERE IT LANDS ON ANOTHER LOCAL DATE than
/// today's, at a zone an hour east of UTC. NEGATIVE CONTROL: the edges where the UTC date and the
/// local date differ, either way.
#[test]
fn the_timer_clock_is_local_and_names_another_day() {
    use chrono::TimeZone;
    use chrono::Weekday;
    let zone = chrono::FixedOffset::east_opt(3_600).unwrap();
    let utc = |d, h, m| chrono::Utc.with_ymd_and_hms(2026, 10, d, h, m, 0).unwrap();
    let secs = |at: chrono::DateTime<chrono::Utc>| u64::try_from(at.timestamp()).unwrap();
    let lands = |clock: &str, day| {
        Some(Landing {
            time: chrono::NaiveTime::parse_from_str(clock, "%H:%M").unwrap(),
            day,
        })
    };
    // Thursday 2026-10-29, 13:00 local.
    let now = utc(29, 12, 0);
    assert_eq!(
        Landing::in_zone(secs(utc(29, 17, 0)), &zone, now),
        lands("18:00", None)
    );
    assert_eq!(
        Landing::in_zone(secs(utc(30, 17, 0)), &zone, now),
        lands("18:00", Some(Weekday::Fri))
    );
    // NEGATIVE CONTROL: 23:30 UTC is Friday 00:30 local.
    assert_eq!(
        Landing::in_zone(secs(utc(29, 23, 30)), &zone, now),
        lands("00:30", Some(Weekday::Fri))
    );
    // And from Friday 00:30 local (Thursday 23:30 UTC), Friday 01:30 local is today.
    assert_eq!(
        Landing::in_zone(secs(utc(30, 0, 30)), &zone, utc(29, 23, 30)),
        lands("01:30", None)
    );
}

/// An unreported mode draws no face or mouse target, regardless of task or timer state.
/// Once Land is reported the same task shows its completed landing.
#[test]
fn unknown_word_draws_no_face_and_no_menu() {
    let (at, _) = an_hour_on();
    let (_state, _, _scope, _automation, _clock) =
        scene(HarnessWord::Unknown, TaskFace::Landed, Some(at));
    let text = row(&paint(Rect::new(0, 0, 120, 4)), 0);
    for glyph in ['∞', '⤓', '✓', '○'] {
        assert!(!text.contains(glyph), "{text}");
    }
    assert!(!text.contains("Cruise"), "{text}");
    for x in 0..120 {
        assert_eq!(mode_mouse_hit(x, 0), None);
        assert!(!autopilot_menu_hit(x, 0));
    }
    // NEGATIVE CONTROL.
    let _scene = scene(HarnessWord::Land, TaskFace::Landed, Some(at));
    let text = row(&paint(Rect::new(0, 0, 120, 4)), 0);
    assert!(text.contains("─ ✓ landed ─▏ Cruise "), "{text}");
}

/// The spoken status includes the keyboard route for every selectable face and the
/// weekday when a timer lands on another day.
#[test]
fn autopilot_words_name_the_keyboard_picker_for_selectable_modes() {
    let said = [
        "Autopilot: cruise.".to_string(),
        // The timed Cruise's, from its timer below.
        String::new(),
        "Autopilot: land.".to_string(),
        "Autopilot: land. Landed.".to_string(),
        "Autopilot: land. Quiet: a person is driving the session.".to_string(),
        "Autopilot: off.".to_string(),
    ];
    for ((word, task, land_at, face), said) in six_faces().into_iter().zip(said) {
        let said = match land_at {
            Some(at) => {
                let lands = Landing::in_zone(at, &chrono::Local, scene_noon().to_utc()).unwrap();
                let day = lands
                    .day
                    .map_or(String::new(), |day| format!("{} ", weekday_long(day)));
                format!(
                    "Autopilot: cruise. Switches to Land {day}at {}.",
                    lands.clock()
                )
            }
            None => said,
        };
        let _scene = scene(word, task, land_at);
        let subtitle = AutopilotStatus::read().picker_subtitle();
        let (_, details) = said.split_once('.').unwrap();
        let timer_hint = if land_at.is_some() {
            ""
        } else {
            " Timers switch to Land."
        };
        assert_eq!(subtitle, format!("{word:?}.{details}{timer_hint}"));
        assert_eq!(
            autopilot_words(),
            format!("{said}\nChoose a mode with /alab menu."),
            "{face}"
        );
    }
    let tuesday = Face::CruiseUntil(Landing {
        time: chrono::NaiveTime::parse_from_str("18:00", "%H:%M").unwrap(),
        day: Some(chrono::Weekday::Tue),
    });
    assert_eq!(
        tuesday.spoken(),
        "Autopilot: cruise. Switches to Land Tuesday at 18:00."
    );
    let _scene = scene(HarnessWord::Unknown, TaskFace::Landed, None);
    assert_eq!(autopilot_words(), "Autopilot: no mode reported by aterm.");
    assert_eq!(AutopilotStatus::read().picker_subtitle(), autopilot_words());
}

async fn routed_autopilot_host() -> (tempfile::TempDir, codex_alab_aterm::fake::FakeAterm) {
    use codex_alab_aterm::fake::FakeAterm;
    use codex_alab_aterm::fake::Mode;
    let dir = tempfile::tempdir().expect("scratch dir");
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .expect("fake aterm");
    route_aterm_for_tests(fake.endpoint());
    (dir, fake)
}

fn autopilot_writes(fake: &codex_alab_aterm::fake::FakeAterm) -> Vec<String> {
    fake.requests()
        .into_iter()
        .filter(|line| line.contains(" meta set autopilot "))
        .collect()
}

#[tokio::test]
async fn superseded_autopilot_selection_sends_nothing_after_its_handshake() {
    let _scene = scene(HarnessWord::Off, TaskFace::Open, None);
    let (_dir, fake) = routed_autopilot_host().await;
    // The host has accepted AUTH but the client still needs its version reply.
    fake.gate("version");
    let old = tokio::spawn(set_cruise_until("+1h".into()));
    assert!(
        fake.wait_for(|host| host.gated_requests().iter().any(|line| line == "version"))
            .await
    );
    let latest = set_harness_mode(HarnessWord::Land);
    fake.open_gate("version");
    assert_eq!(old.await.expect("older timer"), Ok(()));
    assert_eq!(latest.await, Ok(()));
    assert_eq!(
        autopilot_writes(&fake),
        [format!("@{} meta set autopilot land", fake.endpoint().sid)]
    );
}

#[tokio::test]
async fn only_the_current_autopilot_refusal_reaches_the_person() {
    let _scene = scene(HarnessWord::Off, TaskFace::Open, None);
    let (_dir, fake) = routed_autopilot_host().await;
    let sid = fake.endpoint().sid;
    let cruise = format!("@{sid} meta set autopilot cruise");
    let land = format!("@{sid} meta set autopilot land");
    let refusal = "ERR mode locked for this session\n";

    fake.reply_once(&cruise, refusal);
    fake.gate("meta set autopilot cruise");
    let old = tokio::spawn(set_harness_mode(HarnessWord::Cruise));
    assert!(
        fake.wait_for(|host| host.gated_requests().contains(&cruise))
            .await
    );
    let latest = set_harness_mode(HarnessWord::Land);
    fake.open_gate("meta set autopilot cruise");
    assert_eq!(old.await.expect("superseded Cruise refusal"), Ok(()));
    assert_eq!(latest.await, Ok(()));

    // The same refusal must be visible when Cruise is still the latest selection.
    fake.reply_once(&cruise, refusal);
    let error = set_harness_mode(HarnessWord::Cruise)
        .await
        .expect_err("current refusal");
    assert!(
        error.to_ascii_lowercase().contains("cruise")
            && error.contains("mode locked for this session"),
        "{error}"
    );
    assert_eq!(autopilot_writes(&fake), [cruise.clone(), land, cruise]);
}

#[tokio::test]
async fn missing_autopilot_acknowledgement_identifies_the_selection_without_retrying() {
    let _scene = scene(HarnessWord::Off, TaskFace::Open, None);
    let (_dir, fake) = routed_autopilot_host().await;
    let sid = fake.endpoint().sid;
    let land = format!("@{sid} meta set autopilot land");
    let off = format!("@{sid} meta set autopilot off");
    // Land is applied before its reply is dropped. Off gets a complete line outside the protocol.
    fake.drop_next_reply("meta set autopilot land");
    fake.reply_once(&off, "ERROR unexpected response\n");
    for (mode, word) in [(HarnessWord::Land, "land"), (HarnessWord::Off, "off")] {
        let error = set_harness_mode(mode)
            .await
            .expect_err("unconfirmed selection");
        let message = error.to_ascii_lowercase();
        assert!(
            message.contains(word) && message.contains("did not confirm"),
            "{error}"
        );
    }
    assert_eq!(autopilot_writes(&fake), [land, off]);
}

/// Mode and timer writes still use the session's shared policy service.
#[tokio::test]
async fn native_controls_write_modes_and_timers_to_this_session() {
    use codex_alab_aterm::fake::FakeAterm;
    use codex_alab_aterm::fake::Mode;
    let dir = tempfile::tempdir().expect("scratch dir");
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .expect("fake aterm");
    let _scene = scene(HarnessWord::Land, TaskFace::Open, None);
    route_aterm_for_tests(fake.endpoint());
    let sid = fake.endpoint().sid;
    // The timer and the mode.
    assert_eq!(set_cruise_until("18:00".to_string()).await, Ok(()));
    assert_eq!(
        fake.requests().last(),
        Some(&format!("@{sid} meta set autopilot cruise until=18:00"))
    );
    let text = row(&paint(Rect::new(0, 0, 100, 4)), 0);
    for (segment, expected) in [(" Cruise ", "cruise"), (" Off ", "off"), (" Land ", "land")] {
        let column = column_of(&text, segment).unwrap();
        let selected = mode_mouse_hit(column, 0).unwrap();
        assert_eq!(set_harness_mode(selected).await, Ok(()));
        assert_eq!(
            fake.requests().last(),
            Some(&format!("@{sid} meta set autopilot {expected}"))
        );
    }
    let before = fake.requests().len();
    for invalid in ["", "+1h\nmeta set autopilot off", "18:00 extra", "\t+1h"] {
        assert!(set_cruise_until(invalid.into()).await.is_err());
    }
    assert_eq!(
        fake.requests().len(),
        before,
        "malformed deadlines never reach the control socket"
    );
    let sent = fake.requests().len();
    for word in [
        HarnessWord::HarnessOff,
        HarnessWord::AgentOff,
        HarnessWord::Unhosted,
        HarnessWord::Unknown,
    ] {
        assert!(set_harness_mode(word).await.is_err(), "{word:?}");
    }
    assert_eq!(fake.requests().len(), sent, "{:?}", fake.requests());
    set_enabled(false);
    assert!(set_harness_mode(HarnessWord::Cruise).await.is_err());
    assert_eq!(
        fake.requests().last(),
        Some(&format!("@{sid} meta set autopilot land"))
    );
}

#[tokio::test]
async fn invalid_autopilot_inputs_report_errors_and_preserve_a_queued_mode_click() {
    let _scene = scene(HarnessWord::Cruise, TaskFace::Open, None);
    let (_dir, fake) = routed_autopilot_host().await;
    let sid = fake.endpoint().sid;
    fake.reply_once(
        &format!("@{sid} meta set autopilot cruise until=+0m"),
        "ERR invalid deadline\n",
    );

    let invalid_mode = set_harness_mode(HarnessWord::Unknown);
    let land = set_harness_mode(HarnessWord::Land);
    let invalid_timer = set_cruise_until("+0m".into());
    assert_eq!(land.await, Ok(()));
    let error = invalid_timer.await.expect_err("zero-minute Cruise");
    assert_eq!(
        autopilot_writes(&fake),
        [format!("@{sid} meta set autopilot land")]
    );
    assert!(error.contains("HH:MM") && error.contains("+24h"), "{error}");
    assert_eq!(invalid_mode.await, Err("unknown Autopilot mode".to_owned()));
}

#[test]
fn cruise_deadlines_match_aterms_clock_and_duration_syntax() {
    for valid in [
        "00:00",
        "07:05",
        "23:59",
        "+1m",
        "+2h",
        "+90m",
        "+1h30m",
        "+24h",
        "+1440m",
        "+23h60m",
        "+0h1m",
        "+0000h0001m",
    ] {
        assert_eq!(validate_cruise_until(valid), Ok(()), "{valid:?}");
    }
    for invalid in [
        "",
        "now",
        "6 pm",
        "7:00",
        "18:0",
        "24:00",
        "18:60",
        " 18:00",
        "18:00 ",
        "+7:00",
        "０７:００",
        "+",
        "+h",
        "+m",
        "+0m",
        "+0h",
        "+24h1m",
        "+1441m",
        "+1d",
        "+1h30",
        "2h",
        "+-1m",
        "++1m",
        "+1.5h",
        "+00001m",
        "+00000h1m",
        "+1h+2m",
        "+1m\noff",
    ] {
        assert!(validate_cruise_until(invalid).is_err(), "{invalid:?}");
    }
}

/// A disabled host's `⊘ off` uses the space left by the absent switch. Clicking it
/// explains the host setting. An actionable `○ off` still offers all three modes.
#[test]
fn a_settings_switch_wears_a_slashed_off_and_no_controls() {
    let area = Rect::new(0, 0, 120, 4);
    {
        let _scene = scene(HarnessWord::Off, TaskFace::Open, None);
        let text = row(&paint(area), 0);
        let x = column_of(&text, " ○ off ─").unwrap() + 1;
        // NEGATIVE CONTROL.
        assert_eq!(mode_mouse_hit(x, 0), Some(HarnessWord::Cruise));
        assert!(!autopilot_words_hit(x, 0));
        assert!(text.contains("─ ○ off ─▏ Cruise │ Land [ Off ]"), "{text}");
    }
    let turn_on = "Turn it on in aterm's Settings ▸ Harness.";
    for (word, spoken, reason, turned) in [
        (
            HarnessWord::HarnessOff,
            "Autopilot: off. The harness is off in Settings.",
            "The harness is off.",
            turn_on,
        ),
        (
            HarnessWord::AgentOff,
            "Autopilot: disabled for codex-alab in aterm's Settings.",
            "codex-alab Autopilot is off.",
            turn_on,
        ),
        (
            HarnessWord::Unhosted,
            "Autopilot: off. This aterm has no control socket.",
            "This aterm has no control socket.",
            "Open aterm's Settings ▸ Harness.",
        ),
    ] {
        let _scene = scene(word, TaskFace::Landed, Some(an_hour_on().0));
        let status = AutopilotStatus::read();
        assert_eq!(status.summary(), spoken);
        let subtitle = status.picker_subtitle();
        assert_eq!(subtitle, format!("{reason} {turned}"));
        // At 80 columns the picker insets its header two cells on each side.
        assert!(subtitle.width() <= 76, "{subtitle}");
        let buf = paint(area);
        let text = row(&buf, 0);
        let x = column_of(&text, " ⊘ off ─").unwrap() + 1;
        assert_eq!(x + 7, button_rect(area).unwrap().x);
        for label in ["▏ Cruise ", " Land ", " Off "] {
            assert!(!text.contains(label), "{word:?}: {text}");
        }
        for at in 0..area.right() {
            let face = (x..x + 5).contains(&at);
            assert_eq!(mode_mouse_hit(at, 0), None, "{word:?} at {at}");
            assert_eq!(autopilot_words_hit(at, 0), face, "{word:?} at {at}");
            assert_eq!(autopilot_menu_hit(at, 0), face, "{word:?} at {at}");
            assert!(!autopilot_words_hit(at, 1));
        }
        assert_eq!(autopilot_words(), format!("{spoken}\n{turned}"));
        for width in 52..=80 {
            let area = Rect::new(0, 0, width, 4);
            let text = row(&paint(area), 0);
            let available = usize::from(button_rect(area).unwrap().x - 3);
            assert_eq!(
                brand_of(&text).trim(),
                current_brand_label(available, usize::from(GLYPH_SLOT + 1)).unwrap()
            );
            assert!(text.contains("⊘ off ─[ALab on]"), "{width}: {text}");
            assert!(!text.contains("Cruise"), "{width}: {text}");
        }
    }
}

/// EACH REDRAWN CELL WEARS ITS OWN STYLE'S MODIFIERS ALONE (review of gap 12, finding 2: at 80
/// columns the brand's bold, drawn wide and then refit, stayed on the rule, a muted face and a
/// muted button it had covered): at every width, in colour and in mono, the brand's label is
/// bold, its spaces and the rule bear none, an accent or amber face is bold and a muted one
/// bears none in colour and dim in mono, the selected button is bold and reversed and the
/// others use normal text weight.
#[test]
fn each_redrawn_cell_wears_its_own_modifiers_alone() {
    for (depth, muted) in [
        (Depth::TrueColor, Modifier::empty()),
        (Depth::Mono, Modifier::DIM),
    ] {
        for (word, task, land_at, face) in six_faces() {
            let (state, _, _scope, _automation, _clock) = scene(word, task, land_at);
            let quiet = word == HarnessWord::Off || task == TaskFace::Theirs;
            let face_ink = if quiet { muted } else { Modifier::BOLD };
            for width in running_pair().width() as u16 + 27..=200 {
                let area = Rect::new(0, 0, width, 4);
                let mut buf = Buffer::empty(area);
                render(area, &mut buf, depth, Theme::default());
                let text = row(&buf, 0);
                let toggle = button_rect(area).unwrap();
                let start = column_of(&text, " Cruise ")
                    .or_else(|| column_of(&text, "Cruise"))
                    .expect("the first segment")
                    - 1;
                let hits = state.overline_hits.lock().unwrap();
                for x in area.x + 2..toggle.x {
                    let at = hits
                        .modes
                        .iter()
                        .find(|(rect, _)| x >= rect.x && x < rect.right());
                    let want = match at {
                        Some((rect, _)) if rect.x < start => face_ink,
                        Some((_, Hit::Mode(mode))) if *mode == word => {
                            Modifier::BOLD | Modifier::REVERSED
                        }
                        Some(_) => Modifier::empty(),
                        None if hits.build.is_some_and(|rect| rect.contains((x, 0).into())) => {
                            Modifier::BOLD
                        }
                        None => Modifier::empty(),
                    };
                    assert_eq!(
                        buf[(x, 0)].modifier,
                        want,
                        "{face} {depth:?} {width} at {x}: {text}"
                    );
                }
            }
        }
    }
}

#[test]
fn only_a_painted_autopilot_layout_replaces_the_native_row_modifiers() {
    for (word, width, has_controls) in [
        (HarnessWord::Land, 100, true),
        (HarnessWord::Unknown, 100, false),
        (HarnessWord::Land, 9, false),
    ] {
        let _scene = scene(word, TaskFace::Open, None);
        let area = Rect::new(5, 3, width, 4);
        let mut buf = Buffer::empty(area);
        let native = Style::default()
            .bg(Color::Blue)
            .add_modifier(Modifier::UNDERLINED);
        buf.set_style(area, native);
        render(area, &mut buf, Depth::Mono, Theme::default());
        let button = button_rect(area).expect("presentation switch");
        for x in area.x..area.right() {
            let cell = &buf[(x, area.y)];
            let owned = has_controls && (area.x + 2..button.x).contains(&x);
            assert_eq!(
                cell.modifier.contains(Modifier::UNDERLINED),
                !owned,
                "{word:?} {width} at {x}"
            );
            assert_eq!(cell.bg, Color::Blue);
        }
    }
}

/// EACH FACE'S INKS ARE ATERM'S TONES (`ChipFace::tones`; review of gap 12, finding 10): Cruise
/// accent, a timed Cruise's clock amber, Land and landed amber, quiet and every off muted, glyph
/// and rest, at every depth on dark and light grounds, and as each frame draws them, at true
/// colour on a dark ground and in mono, each cell's ink and modifiers. NEGATIVE CONTROL: the
/// three inks differ on palettes where the face uses color. On light ANSI16,
/// the active tones share the configured foreground and keep their weight.
#[test]
fn each_faces_inks_are_atermss_tones() {
    let lands = Landing {
        time: chrono::NaiveTime::parse_from_str("18:00", "%H:%M").unwrap(),
        day: None,
    };
    for depth in [Depth::TrueColor, Depth::Ansi256, Depth::Ansi16, Depth::Mono] {
        for theme in [
            Theme::from_bg(Some((16, 20, 24))),
            Theme::from_bg(Some((255, 255, 255))),
        ] {
            let inks = FaceInks::new(depth, theme);
            let (accent, amber, muted) = (inks.accent, inks.amber, inks.muted);
            if depth == Depth::Ansi16 && theme.is_light() {
                assert_eq!(accent, amber);
                assert_ne!(amber, muted);
            } else if depth != Depth::Mono {
                // NEGATIVE CONTROL.
                assert!(
                    accent != amber && amber != muted && accent != muted,
                    "{depth:?} {theme:?}: {inks:?}"
                );
            }
            for (face, glyph, rest) in [
                (Face::Cruise, accent, accent),
                (Face::CruiseUntil(lands), accent, amber),
                (Face::Land, amber, amber),
                (Face::Landed, amber, amber),
                (Face::Theirs, muted, muted),
                (Face::Off, muted, muted),
                (Face::HarnessOff, muted, muted),
                (Face::AgentOff, muted, muted),
                (Face::Unhosted, muted, muted),
            ] {
                let line = face.line(FACE_SLOT, /*beside_switch*/ true, inks);
                let drawn: Vec<Style> = line.spans.iter().map(|span| span.style).collect();
                assert_eq!(drawn, [glyph, rest], "{face:?} {depth:?} {theme:?}");
                let alone = face.line(GLYPH_SLOT, /*beside_switch*/ false, inks);
                let drawn: Vec<Style> = alone.spans.iter().map(|span| span.style).collect();
                assert_eq!(
                    drawn,
                    [glyph],
                    "{face:?} {depth:?} {theme:?}: the glyph rung"
                );
            }
        }
    }
    for (depth, theme) in [
        (Depth::TrueColor, Theme::from_bg(Some((16, 20, 24)))),
        (Depth::Mono, Theme::default()),
    ] {
        let inks = FaceInks::new(depth, theme);
        let tones = [
            (inks.accent, inks.accent),
            (inks.accent, inks.amber),
            (inks.amber, inks.amber),
            (inks.amber, inks.amber),
            (inks.muted, inks.muted),
            (inks.muted, inks.muted),
        ];
        for ((word, task, land_at, face), (glyph, rest)) in six_faces().into_iter().zip(tones) {
            let _scene = scene(word, task, land_at);
            let area = Rect::new(0, 0, 120, 4);
            let mut buf = Buffer::empty(area);
            render(area, &mut buf, depth, theme);
            let text = row(&buf, 0);
            let x = column_of(&text, &format!(" {face} ─")).unwrap() + 1;
            let width = UnicodeWidthStr::width(face.as_str()) as u16;
            for at in x..x + width {
                let tone = if at == x { glyph } else { rest };
                let cell = &buf[(at, 0)];
                assert_eq!(
                    (cell.fg, cell.modifier),
                    (tone.fg.unwrap_or(Color::Reset), tone.add_modifier),
                    "{face} {depth:?} at {at}"
                );
            }
        }
    }
}

/// THE INKS ARE MADE AGAIN AT A CHANGE OF GROUND OR OF DEPTH (review of gap 12, finding 11):
/// the cache keys both, the ground itself and not its lightness alone, so two grounds back to
/// back at one depth, dark and light or two dark ones the floor inks apart, and one ground at
/// two depths back to back, each wear their own, a 256-colour terminal its palette's indices.
/// NEGATIVE CONTROL: the grounds' inks differ, and so do the depths'.
#[test]
fn the_inks_follow_the_ground_and_the_depth() {
    let dark = Theme::from_bg(Some((16, 20, 24)));
    let light = Theme::from_bg(Some((255, 255, 255)));
    let gray = Theme::from_bg(Some((120, 120, 120)));
    assert!(!dark.is_light() && !gray.is_light() && light.is_light());
    for depth in [Depth::TrueColor, Depth::Ansi256] {
        // NEGATIVE CONTROL.
        assert_ne!(
            FaceInks::new(depth, dark),
            FaceInks::new(depth, light),
            "{depth:?}"
        );
        assert_ne!(
            FaceInks::new(depth, dark),
            FaceInks::new(depth, gray),
            "{depth:?}"
        );
        for theme in [dark, light, dark, gray, dark] {
            assert_eq!(
                FaceInks::of(depth, theme),
                FaceInks::new(depth, theme),
                "{depth:?} {theme:?}"
            );
        }
    }
    // NEGATIVE CONTROL.
    assert_ne!(
        FaceInks::new(Depth::TrueColor, dark),
        FaceInks::new(Depth::Ansi256, dark)
    );
    for depth in [Depth::TrueColor, Depth::Ansi256, Depth::TrueColor] {
        let inks = FaceInks::of(depth, dark);
        assert_eq!(inks, FaceInks::new(depth, dark), "{depth:?}");
        for ink in [inks.accent, inks.amber, inks.muted] {
            assert!(
                matches!(
                    (depth, ink.fg),
                    (Depth::TrueColor, Some(Color::Rgb(..)))
                        | (Depth::Ansi256, Some(Color::Indexed(_)))
                ),
                "{depth:?}: {ink:?}"
            );
        }
    }
}

/// THE FACE READS THE TIMER IN THE ZONE IT IS GIVEN (review of gap 12, finding 19: the line's
/// zone was pinned by `clock_in`'s test alone): a timer at 17:00 UTC wears `⤓18:00` an hour east
/// of UTC and `⤓17:00` at UTC, and names Friday where it lands on the next local day. NEGATIVE
/// CONTROL: no timer under Land, and no task under Cruise.
#[test]
fn the_face_reads_the_timer_in_its_zone() {
    use chrono::TimeZone;
    let utc = |d, h, m| chrono::Utc.with_ymd_and_hms(2026, 10, d, h, m, 0).unwrap();
    let secs = |at: chrono::DateTime<chrono::Utc>| u64::try_from(at.timestamp()).unwrap();
    let east = chrono::FixedOffset::east_opt(3_600).unwrap();
    let now = utc(29, 12, 0);
    let until = |clock: &str, day| {
        Some(Face::CruiseUntil(Landing {
            time: chrono::NaiveTime::parse_from_str(clock, "%H:%M").unwrap(),
            day,
        }))
    };
    let (_, shared, _scope, _automation, _clock) = scene(
        HarnessWord::Cruise,
        TaskFace::Landed,
        Some(secs(utc(29, 17, 0))),
    );
    assert_eq!(
        Face::read_in(shared, HarnessWord::Cruise, &east, now),
        until("18:00", None)
    );
    assert_eq!(
        Face::read_in(shared, HarnessWord::Cruise, &chrono::Utc, now),
        until("17:00", None)
    );
    shared.set_land_at(Some(secs(utc(29, 23, 30))));
    assert_eq!(
        Face::read_in(shared, HarnessWord::Cruise, &east, now),
        until("00:30", Some(chrono::Weekday::Fri))
    );
    assert_eq!(
        Face::read_in(shared, HarnessWord::Cruise, &chrono::Utc, now),
        until("23:30", None)
    );
    // NEGATIVE CONTROL.
    assert_eq!(
        Face::read_in(shared, HarnessWord::Land, &east, now),
        Some(Face::Landed)
    );
}

/// EACH INTERFACE WRITES ITS OWN AUTOPILOT, THE LAST WRITE WINNING (review of gap 12, finding
/// 14: one process-wide writer let another scoped interface's write drop this one's), against
/// alab-aterm's in-process FAKE aterm (never a real one). NEGATIVE CONTROL: within one
/// interface, a newer write overtakes an older one not yet sent, which is dropped.
#[tokio::test]
async fn each_interface_writes_its_own_autopilot() {
    use codex_alab_aterm::fake::FakeAterm;
    use codex_alab_aterm::fake::Mode;
    let dir = tempfile::tempdir().expect("scratch dir");
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .expect("fake aterm");
    let sid = fake.endpoint().sid;
    let set = |word: &str| format!("@{sid} meta set autopilot {word}");
    let sets = |word: &str| fake.requests().iter().filter(|r| **r == set(word)).count();
    let _scene = scene(HarnessWord::Land, TaskFace::Open, None);
    route_aterm_for_tests(fake.endpoint());
    let first = set_harness_mode(HarnessWord::Off);
    {
        let _other = scene(HarnessWord::Land, TaskFace::Open, None);
        route_aterm_for_tests(fake.endpoint());
        assert_eq!(set_harness_mode(HarnessWord::Cruise).await, Ok(()));
    }
    assert_eq!(first.await, Ok(()));
    assert_eq!(
        (sets("cruise"), sets("off")),
        (1, 1),
        "{:?}",
        fake.requests()
    );
    assert_eq!(fake.requests().last(), Some(&set("off")));
    // NEGATIVE CONTROL.
    let older = set_harness_mode(HarnessWord::Cruise);
    let newer = set_harness_mode(HarnessWord::Land);
    assert_eq!(newer.await, Ok(()));
    assert_eq!(older.await, Ok(()));
    assert_eq!(
        (sets("cruise"), sets("land")),
        (1, 1),
        "{:?}",
        fake.requests()
    );
}

#[test]
fn pending_hint_keeps_the_release_modes_and_landing_time_when_they_fit() {
    let pair = running_pair();
    let pair_width = pair.width() as u16;
    for word in [HarnessWord::Cruise, HarnessWord::Unknown] {
        let (at, clock) = an_hour_on();
        let (_state, shared, _scope, _automation, _clock) = scene(word, TaskFace::Open, Some(at));
        let harness = (word != HarnessWord::Unknown).then_some((shared, shared.harness()));
        for extra in 15..=61 {
            let area = Rect::new(5, 2, pair_width + extra, 4);
            let switch = button_for(area, true).unwrap();
            let layout = ComposerControls::fit(area, true, switch, harness, true);
            let normal = ComposerControls::fit(area, true, switch, harness, false);
            let hint = extra >= if harness.is_some() { 40 } else { 20 };
            let mut buf = Buffer::empty(Rect::new(0, 0, area.right(), area.bottom()));
            let hits = render_border(area, &mut buf, &layout, Depth::Mono, Theme::default());
            let text = row(&buf, area.y);
            let label = if hint {
                format!("{pair}{UPDATE_HINT}")
            } else {
                pair.clone()
            };
            assert!(text.contains(&label), "{word:?}, {extra}: {text}");
            assert_eq!(text.contains(UPDATE_HINT), hint);
            assert_eq!(
                hits.build,
                Some(Rect::new(
                    column_of(&text, &label).unwrap(),
                    area.y,
                    label.width() as u16,
                    1,
                ))
            );
            assert_eq!(layout.track.is_some(), normal.track.is_some());
            if harness.is_some() && (22..=26).contains(&extra) {
                assert!(layout.face_slot >= FACE_SLOT, "{text}");
                assert!(text.ends_with("─[on]  "), "{text}");
            }
            if layout.track.is_some() {
                let expected = if layout.face_slot < FACE_SLOT && compact_clock_supported() {
                    "◷"
                } else {
                    "∞"
                };
                let glyph = column_of(&text, expected).expect("Cruise status beside the track");
                assert!(
                    hits.modes.iter().any(|(rect, hit)| {
                        rect.contains((glyph, area.y).into())
                            && *hit == Hit::Mode(HarnessWord::Land)
                    }),
                    "{extra}: {text}"
                );
                for (segment, mode) in COMPACT_MODES.into_iter().zip([
                    HarnessWord::Cruise,
                    HarnessWord::Land,
                    HarnessWord::Off,
                ]) {
                    let x = column_of(&text, segment).expect("named mode");
                    assert!(
                        hits.modes
                            .iter()
                            .any(|(rect, hit)| rect.contains((x, area.y).into())
                                && *hit == Hit::Mode(mode))
                    );
                }
            }
            if layout.track.is_some() {
                assert_eq!(normal.face_slot >= FACE_SLOT, extra >= 40, "{text}");
                assert_eq!(text.contains(&format!("∞ ⤓{clock}")), extra >= 49, "{text}");
            }
            if !hint {
                assert!(
                    layout.face_slot >= normal.face_slot,
                    "{word:?}, {extra}: {text}"
                );
            }
            assert!(text.contains(if layout.button.width == 4 {
                "[on]"
            } else {
                "[ALab on]"
            }));
            assert_eq!(hits.button, Some(layout.button));
        }
    }
}

#[test]
fn native_header_update_badge_preserves_the_brand_and_fits() {
    for reserve in [0, usize::from(GLYPH_SLOT + 1)] {
        for width in 14..180 {
            let label = fit_brand_label(
                width,
                "0.1.0-dev.14249",
                release_pair("0.1.0-dev.14249", "0.166.0"),
                true,
                reserve,
            );
            if let Some(label) = label {
                assert!(label.width() + 2 <= width, "{width}: {label}");
                assert!(label.starts_with("0.1"));
                assert_eq!(
                    label.ends_with("↑ update"),
                    width >= " 0.1 on Codex 0.166 ↑ update ".width() + reserve,
                    "{width}: {label}",
                );
            }
        }
    }
}

#[test]
fn native_version_hit_is_only_the_drawn_label_and_clears_with_the_interface() {
    let _scene = scene(HarnessWord::Land, TaskFace::Open, None);
    let area = Rect::new(0, 0, 120, 4);
    let mut buffer = Buffer::empty(area);
    render(area, &mut buffer, Depth::Mono, Theme::default());
    assert!(build_mouse_hit(3, 0));
    assert!(!build_mouse_hit(2, 0));
    assert!(!build_mouse_hit(3, 1));
    let mode = row(&buffer, 0).find("▏ Cruise ").unwrap();
    let mode_col = row(&buffer, 0)[..mode].chars().count() as u16;
    assert!(!build_mouse_hit(mode_col, 0));
    set_enabled(false);
    assert!(!build_mouse_hit(3, 0));
}

#[test]
fn reenabling_before_repaint_retires_all_previous_overline_targets() {
    let _scene = scene(HarnessWord::Land, TaskFace::Open, None);
    let old = Rect::new(0, 0, 120, 4);
    let old_row = row(&paint(old), old.y);
    let version = column_of(&old_row, &running_pair()).unwrap();
    let face = column_of(&old_row, "⤓ land").unwrap();
    let cruise = column_of(&old_row, " Cruise ").unwrap();
    assert!(build_mouse_hit(version, old.y));
    assert_eq!(mode_mouse_hit(face, old.y), Some(HarnessWord::Off));
    assert_eq!(mode_mouse_hit(cruise, old.y), Some(HarnessWord::Cruise));
    assert!(autopilot_menu_hit(cruise, old.y));

    set_enabled(false);
    assert!(toggle());
    for x in old.x..old.right() {
        assert!(!mouse_hit(x, old.y));
        assert!(!build_mouse_hit(x, old.y));
        assert_eq!(mode_mouse_hit(x, old.y), None);
        assert!(!autopilot_menu_hit(x, old.y));
    }

    let new = Rect::new(5, 2, 80, 4);
    let new_row = row(&paint(new), new.y);
    assert!(build_mouse_hit(new.x + 3, new.y));
    assert!(mouse_hit(column_of(&new_row, "[ALab on]").unwrap(), new.y));
    let cruise = column_of(&new_row, " Cruise ").unwrap();
    assert_eq!(mode_mouse_hit(cruise, new.y), Some(HarnessWord::Cruise));
    assert!(autopilot_menu_hit(cruise, new.y));
}

#[test]
fn native_menu_hit_tracks_only_the_current_controls() {
    let _scene = scene(HarnessWord::Land, TaskFace::Open, None);
    let area = Rect::new(3, 2, 120, 4);
    let text = row(&paint(area), area.y);
    let start = column_of(&text, PADDED_LAND_TRACK).expect("painted switch");
    let track = start..start + PADDED_LAND_TRACK.width() as u16;
    let start = column_of(&text, "⤓ land").expect("painted status");
    let face = start..start + "⤓ land".width() as u16;
    for x in 0..area.right() {
        assert_eq!(
            autopilot_menu_hit(x, 2),
            track.contains(&x) || face.contains(&x)
        );
        assert!(!autopilot_menu_hit(x, 3));
    }
    set_enabled(false);
    for x in 0..area.right() {
        assert!(!autopilot_menu_hit(x, 2));
    }
}
