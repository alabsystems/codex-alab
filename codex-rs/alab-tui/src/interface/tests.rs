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
    for depth in [Depth::TrueColor, Depth::Ansi256, Depth::Ansi16, Depth::Mono] {
        for theme in [
            Theme::from_bg(Some((16, 20, 24))),
            Theme::from_bg(Some((255, 255, 255))),
        ] {
            let area = Rect::new(5, 3, 90, 5);
            let mut buf = Buffer::empty(area);
            render_border(
                area,
                &mut buf,
                true,
                button_for(area, true).unwrap(),
                depth,
                theme,
            );
            let line = row(&buf, area.y);
            assert!(
                line.starts_with(&format!(
                    "   codex-alab {} · based on OpenAI Codex {} ",
                    readable_version(codex_alab_core::version::alab()),
                    codex_alab_core::version::upstream_release().unwrap_or("unreleased")
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
fn rainbow_stays_visible_across_labels_and_readable_on_both_themes() {
    use crate::banner::palette;
    for width in [20, 40, 80, 120] {
        for background in [(16, 20, 24), (255, 255, 255)] {
            for depth in [Depth::TrueColor, Depth::Ansi256, Depth::Ansi16] {
                let area = Rect::new(0, 0, width, 3);
                let mut buf = Buffer::empty(area);
                render_border(
                    area,
                    &mut buf,
                    true,
                    button_for(area, true).unwrap(),
                    depth,
                    Theme::from_bg(Some(background)),
                );
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
fn an_eighty_column_prompt_keeps_both_packaged_version_numbers() {
    let area = Rect::new(0, 0, 80, 3);
    let button = button_for(area, true).unwrap();
    let available = usize::from(button.x.saturating_sub(area.x + 3));
    let alab = "0.164.0-dev.12706+g666cb760d";
    let upstream = "0.163.0+ge8aa7c1678ca";
    let label = brand_label(available, alab, upstream).unwrap();
    assert!(label.contains("0.164.0-dev.12706"));
    assert!(label.contains("0.163.0"));
    assert!(label.contains("based on"));
    assert!(!label.contains("666cb760d"));
    assert!(!label.contains("e8aa7c1678ca"));
    assert!(!label.contains("+g"));
    assert!(UnicodeWidthStr::width(label.as_str()) + 2 <= available);
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
fn reset_expiry_highlights_only_the_reset_credit_dots() {
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
        for theme in [
            Theme::from_bg(Some((16, 20, 24))),
            Theme::from_bg(Some((255, 255, 255))),
        ] {
            let area = Rect::new(3, 2, 200, 1);
            let mut buf = Buffer::empty(area);
            render_status_current(area, &mut buf, depth, theme);
            let dots: Vec<_> = (area.x..area.right())
                .filter(|x| buf[(*x, area.y)].symbol() == "●")
                .collect();
            assert_eq!(dots.len(), 4, "{}", row(&buf, area.y));
            let palette = Palette::new(depth, theme);
            assert_eq!(
                buf[(dots[0], area.y)].fg,
                palette.brand.fg.unwrap_or_default()
            );
            assert_eq!(
                buf[(dots[3], area.y)].fg,
                palette.muted.fg.unwrap_or_default()
            );
            assert!(buf[(dots[1], area.y)].modifier.contains(Modifier::BOLD));
            assert!(!buf[(dots[2], area.y)].modifier.contains(Modifier::BOLD));
            if depth != Depth::Mono {
                assert_eq!(
                    buf[(dots[1], area.y)].fg,
                    if theme.is_light() {
                        Color::Yellow
                    } else {
                        Color::LightYellow
                    }
                );
                assert_eq!(
                    buf[(dots[2], area.y)].fg,
                    palette.accent.fg.unwrap_or_default()
                );
            }
        }
    }
}

#[test]
fn every_terminal_width_stays_inside_the_composer() {
    for width in 0..=120 {
        for on in [true, false] {
            let buffer_area = Rect::new(0, 0, width + 10, 7);
            let area = Rect::new(5, 2, width, 3);
            let mut buf = Buffer::empty(buffer_area);
            if let Some(button) = button_for(area, on) {
                assert!(button.x >= area.x);
                assert!(button.right() <= area.right());
                render_border(area, &mut buf, on, button, Depth::Mono, Theme::default());
            }
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
    shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
    render(area, &mut buf, Depth::Mono, Theme::default());
    let text = row(&buf, 0);
    for (label, mode) in [
        ("[Cruise]", HarnessWord::Cruise),
        ("[Land]", HarnessWord::Land),
        ("[Off]", HarnessWord::Off),
    ] {
        let x = UnicodeWidthStr::width(&text[..text.find(label).unwrap()]) as u16;
        assert_eq!(mode_mouse_hit(x, 0), Some(mode));
        assert_eq!(
            buf[(x, 0)].modifier.contains(Modifier::REVERSED),
            mode == HarnessWord::Land
        );
    }
    set_enabled(false);
    for x in 0..120 {
        assert!(mode_mouse_hit(x, 0).is_none());
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
            column += UnicodeWidthStr::width(grapheme.symbol) as u16;
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
    for email in [
        None,
        Some("使用者@例子.test"),
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
                    UnicodeWidthStr::width(line.as_str()) <= usize::from(width),
                    "{width}: {line:?}"
                );
                assert!(!line.chars().any(char::is_control));
            }
        }
    }
}

#[test]
fn fitting_preserves_joined_emoji_and_combining_graphemes() {
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
    for gray in [112, 116, 120, 124, 128, 132] {
        let background = (gray, gray, gray);
        for depth in [Depth::TrueColor, Depth::Ansi256] {
            let area = Rect::new(0, 0, 80, 3);
            let mut buf = Buffer::empty(area);
            render_border(
                area,
                &mut buf,
                true,
                button_for(area, true).unwrap(),
                depth,
                Theme::from_bg(Some(background)),
            );
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
) -> (&'static State, &'static Shared, Scope, crate::boxes::Scope) {
    use crate::session::Autopilot;
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
    (state, shared, scope, automation)
}

/// A timer an hour from now, and the clock the line says for it.
fn an_hour_on() -> (u64, String) {
    let at = u64::try_from(chrono::Utc::now().timestamp()).unwrap() + 3_600;
    let lands = clock_in(at, &chrono::Local, chrono::Utc::now()).unwrap();
    (at, lands.clock)
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

/// The brand's cells of `text`, a row of a composer at column 0: from its third cell to the
/// rule.
fn brand_of(text: &str) -> String {
    text.chars().skip(2).take_while(|c| *c != '─').collect()
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
    for (word, task, land_at, face) in table {
        let _scene = scene(word, task, land_at);
        let text = row(&paint(Rect::new(0, 0, 120, 4)), 0);
        assert!(
            text.contains(&format!("─ {face} ─[Cruise]")),
            "{word:?} {task:?} {land_at:?}: {text}"
        );
    }
}

/// NO FACE MOVES THE BRAND OR THE CONTROLS: at every width, the brand's text and every cell
/// from `[Cruise]` on are the same under all six faces, and `[Cruise]` stands where it stood
/// before the face. NEGATIVE CONTROL: no word draws the brand alone.
#[test]
fn no_face_moves_the_brand_or_the_controls() {
    for width in 40..=200 {
        let area = Rect::new(0, 0, width, 4);
        let mut seen: Option<(String, Option<String>)> = None;
        for (word, task, land_at, face) in six_faces() {
            let _scene = scene(word, task, land_at);
            let buf = paint(area);
            let text = row(&buf, 0);
            let brand = brand_of(&text);
            let controls = column_of(&text, "[Cruise]").map(|x| {
                let toggle = button_rect(area).unwrap();
                assert_eq!(x, toggle.x - 24, "{width} {face}");
                (x..area.right())
                    .map(|x| buf[(x, 0)].symbol())
                    .collect::<String>()
            });
            assert_eq!(controls.is_some(), width >= 52, "{width} {face}");
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
        text.contains("codex-alab") && !text.contains("[Cruise]"),
        "{text}"
    );
}

/// THE FACE'S SLOT STEPS BY WIDTH ALONE, the brand's last rung kept beside it: none at 52 and 53
/// columns, the glyph alone from 54 to 60, the whole face from 61.
#[test]
fn the_face_slot_steps_by_width_alone() {
    for (word, task, land_at, face) in six_faces() {
        let _scene = scene(word, task, land_at);
        let glyph = face.chars().next().unwrap();
        for width in 52..=200 {
            let text = row(&paint(Rect::new(0, 0, width, 4)), 0);
            assert!(!brand_of(&text).trim().is_empty(), "{width} {face}: {text}");
            let drawn = match width {
                52..=53 => "─[Cruise]".to_string(),
                54..=60 => format!("─ {glyph} ─[Cruise]"),
                _ => format!("─ {face} ─[Cruise]"),
            };
            assert!(text.contains(&drawn), "{width} {face}: {text}");
            if width < 54 {
                assert!(!text.contains(glyph), "{width} {face}: {text}");
            }
        }
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
        let x = column_of(&text, &format!(" {face} ─[Cruise]")).unwrap() + 1;
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
            ("[Cruise]", HarnessWord::Cruise),
            ("[Land]", HarnessWord::Land),
            ("[Off]", HarnessWord::Off),
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
                let x = column_of(&text, &format!(" {face} ─[Cruise]")).unwrap() + 1;
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
        let x = column_of(&text, &format!(" {face} ─[Cruise]")).unwrap() + 1;
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
        Some(Lands {
            clock: clock.to_string(),
            day,
        })
    };
    // Thursday 2026-10-29, 13:00 local.
    let now = utc(29, 12, 0);
    assert_eq!(
        clock_in(secs(utc(29, 17, 0)), &zone, now),
        lands("18:00", None)
    );
    assert_eq!(
        clock_in(secs(utc(30, 17, 0)), &zone, now),
        lands("18:00", Some(Weekday::Fri))
    );
    // NEGATIVE CONTROL: 23:30 UTC is Friday 00:30 local.
    assert_eq!(
        clock_in(secs(utc(29, 23, 30)), &zone, now),
        lands("00:30", Some(Weekday::Fri))
    );
    // And from Friday 00:30 local (Thursday 23:30 UTC), Friday 01:30 local is today.
    assert_eq!(
        clock_in(secs(utc(30, 0, 30)), &zone, utc(29, 23, 30)),
        lands("01:30", None)
    );
}

/// NO WORD DRAWS NO FACE AND OFFERS NO MENU, whatever the landing and the timer hold: codex-alab
/// does not supervise the session. NEGATIVE CONTROL: the same facts under Land wear the face.
#[test]
fn unknown_word_draws_no_face_and_no_menu() {
    let (at, _) = an_hour_on();
    let (_state, _, _scope, _automation) = scene(HarnessWord::Unknown, TaskFace::Landed, Some(at));
    let text = row(&paint(Rect::new(0, 0, 120, 4)), 0);
    for glyph in ['∞', '⤓', '✓', '○'] {
        assert!(!text.contains(glyph), "{text}");
    }
    assert!(!text.contains("[Cruise]"), "{text}");
    for x in 0..120 {
        assert_eq!(mode_mouse_hit(x, 0), None);
        assert!(!autopilot_menu_hit(x, 0));
    }
    // NEGATIVE CONTROL.
    let _scene = scene(HarnessWord::Land, TaskFace::Landed, Some(at));
    let text = row(&paint(Rect::new(0, 0, 120, 4)), 0);
    assert!(text.contains("─ ✓ landed ─[Cruise]"), "{text}");
}

/// ATERM'S SENTENCES FOR EACH FACE (`ChipFace::spoken`), and what a click does, as `/alab
/// autopilot` says them; the weekday spelled out where the timer lands on another day.
#[test]
fn autopilot_words_say_atermss_sentences() {
    let said = [
        "Autopilot: cruise.\nClick: land.".to_string(),
        // The timed Cruise's, from its timer below.
        String::new(),
        "Autopilot: land.\nClick: off.".to_string(),
        "Autopilot: land. Landed.\nClick: off.".to_string(),
        "Autopilot: land. Quiet: a person is driving the session.\nClick: off.".to_string(),
        "Autopilot: off.\nClick: cruise.".to_string(),
    ];
    for ((word, task, land_at, face), said) in six_faces().into_iter().zip(said) {
        let said = match land_at {
            Some(at) => {
                let lands = clock_in(at, &chrono::Local, chrono::Utc::now()).unwrap();
                let day = lands
                    .day
                    .map_or(String::new(), |day| format!("{} ", weekday_long(day)));
                format!(
                    "Autopilot: cruise. Lands {day}at {}.\nClick: land now.",
                    lands.clock
                )
            }
            None => said,
        };
        let _scene = scene(word, task, land_at);
        assert_eq!(
            autopilot_words(),
            format!("{said} Right-click: Autopilot controls."),
            "{face}"
        );
    }
    let tuesday = Face::CruiseUntil(Lands {
        clock: "18:00".to_string(),
        day: Some(chrono::Weekday::Tue),
    });
    assert_eq!(
        tuesday.spoken(),
        "Autopilot: cruise. Lands Tuesday at 18:00."
    );
    let _scene = scene(HarnessWord::Unknown, TaskFace::Landed, None);
    assert_eq!(
        autopilot_words(),
        "Autopilot: no mode from aterm (codex-alab does not supervise this session)."
    );
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
    assert_eq!(set_harness_mode(HarnessWord::Land).await, Ok(()));
    assert_eq!(
        fake.requests().last(),
        Some(&format!("@{sid} meta set autopilot land"))
    );
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

/// `⊘ off` (aterm's `MasterOff`, `AgentOff` and `Unhosted`; review of gap 12, finding 3): the
/// harness held off by one of aterm's Settings switches, or with no host, wears its face where
/// every face stands, muted, its glyph alone on the glyph rung, and no mode buttons, whose cells
/// keep the rule; a click on the face selects no mode but says the face, naming where aterm
/// turns it, and a secondary click opens the native menu. NEGATIVE CONTROL: `○ off`, the harness on
/// with Autopilot off, keeps its buttons, and its click selects Cruise.
#[test]
fn a_settings_switch_wears_a_slashed_off_and_no_controls() {
    let area = Rect::new(0, 0, 120, 4);
    let off = {
        let _scene = scene(HarnessWord::Off, TaskFace::Open, None);
        let text = row(&paint(area), 0);
        let x = column_of(&text, " ○ off ─").unwrap() + 1;
        // NEGATIVE CONTROL.
        assert_eq!(mode_mouse_hit(x, 0), Some(HarnessWord::Cruise));
        assert!(!autopilot_words_hit(x, 0));
        assert!(text.contains("─ ○ off ─[Cruise]─[Land]─[Off]"), "{text}");
        (brand_of(&text), x)
    };
    let turn_on = "Turn it on in aterm's Settings ▸ Harness.";
    for (word, spoken, turned) in [
        (
            HarnessWord::HarnessOff,
            "Autopilot: off. The harness is off in Settings.",
            turn_on,
        ),
        (
            HarnessWord::AgentOff,
            "Autopilot: disabled for codex-alab in aterm's Settings.",
            turn_on,
        ),
        (
            HarnessWord::Unhosted,
            "Autopilot: off. This aterm has no control socket.",
            "Open aterm's Settings ▸ Harness.",
        ),
    ] {
        let _scene = scene(word, TaskFace::Landed, Some(an_hour_on().0));
        let buf = paint(area);
        let text = row(&buf, 0);
        let x = column_of(&text, " ⊘ off ─").unwrap() + 1;
        assert_eq!((brand_of(&text), x), off, "{word:?}: {text}");
        let start = button_rect(area).unwrap().x - 24;
        for at in start..start + 21 {
            assert_eq!(buf[(at, 0)].symbol(), "─", "{word:?} at {at}: {text}");
        }
        for label in ["[Cruise]", "[Land]", "[Off]"] {
            assert!(!text.contains(label), "{word:?}: {text}");
        }
        for at in 0..area.right() {
            let face = (x..x + 5).contains(&at);
            assert_eq!(mode_mouse_hit(at, 0), None, "{word:?} at {at}");
            assert_eq!(autopilot_words_hit(at, 0), face, "{word:?} at {at}");
            assert_eq!(autopilot_menu_hit(at, 0), face, "{word:?} at {at}");
            assert!(!autopilot_words_hit(at, 1));
        }
        assert_eq!(
            autopilot_words(),
            format!("{spoken}\n{turned} Right-click: Autopilot controls."),
        );
        for width in 52..=61 {
            let text = row(&paint(Rect::new(0, 0, width, 4)), 0);
            assert_eq!(
                text.contains("─ ⊘ ─"),
                (54..=60).contains(&width),
                "{width}: {text}"
            );
            assert_eq!(text.contains("─ ⊘ off ─"), width == 61, "{width}: {text}");
            assert!(!text.contains("[Cruise]"), "{width}: {text}");
        }
    }
}

/// EACH REDRAWN CELL WEARS ITS OWN STYLE'S MODIFIERS ALONE (review of gap 12, finding 2: at 80
/// columns the brand's bold, drawn wide and then refit, stayed on the rule, a muted face and a
/// muted button it had covered): at every width, in colour and in mono, the brand's label is
/// bold, its spaces and the rule bear none, an accent or amber face is bold and a muted one
/// bears none in colour and dim in mono, the selected button is bold and reversed and the
/// others muted.
#[test]
fn each_redrawn_cell_wears_its_own_modifiers_alone() {
    for (depth, muted) in [
        (Depth::TrueColor, Modifier::empty()),
        (Depth::Mono, Modifier::DIM),
    ] {
        for (word, task, land_at, face) in six_faces() {
            let (state, _, _scope, _automation) = scene(word, task, land_at);
            let quiet = word == HarnessWord::Off || task == TaskFace::Theirs;
            let face_ink = if quiet { muted } else { Modifier::BOLD };
            for width in 52..=200 {
                let area = Rect::new(0, 0, width, 4);
                let mut buf = Buffer::empty(area);
                render(area, &mut buf, depth, Theme::default());
                let text = row(&buf, 0);
                let toggle = button_rect(area).unwrap();
                let start = toggle.x - 24;
                let brand = UnicodeWidthStr::width(brand_of(&text).as_str()) as u16;
                let hits = state.mode_buttons.lock().unwrap().clone();
                for x in area.x + 2..toggle.x {
                    let at = hits
                        .iter()
                        .find(|(rect, _)| x >= rect.x && x < rect.right());
                    let want = match at {
                        Some((rect, _)) if rect.x < start => face_ink,
                        Some((_, Hit::Mode(mode))) if *mode == word => {
                            Modifier::BOLD | Modifier::REVERSED
                        }
                        Some(_) => muted,
                        None if x > 2 && x < 1 + brand => Modifier::BOLD,
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

/// EACH FACE'S INKS ARE ATERM'S TONES (`ChipFace::tones`; review of gap 12, finding 10): Cruise
/// accent, a timed Cruise's clock amber, Land and landed amber, quiet and every off muted, glyph
/// and rest, at every depth on dark and light grounds, and as each frame draws them, at true
/// colour on a dark ground and in mono, each cell's ink and modifiers. NEGATIVE CONTROL: the
/// three inks differ where the terminal draws colour, so no face could wear another's and
/// pass.
#[test]
fn each_faces_inks_are_atermss_tones() {
    let lands = Lands {
        clock: "18:00".to_string(),
        day: None,
    };
    for depth in [Depth::TrueColor, Depth::Ansi256, Depth::Ansi16, Depth::Mono] {
        for theme in [
            Theme::from_bg(Some((16, 20, 24))),
            Theme::from_bg(Some((255, 255, 255))),
        ] {
            let inks = FaceInks::new(depth, theme);
            let (accent, amber, muted) = (inks.accent, inks.amber, inks.muted);
            if depth != Depth::Mono {
                // NEGATIVE CONTROL.
                assert!(
                    accent != amber && amber != muted && accent != muted,
                    "{depth:?} {theme:?}: {inks:?}"
                );
            }
            for (face, glyph, rest) in [
                (Face::Cruise, accent, accent),
                (Face::CruiseUntil(lands.clone()), accent, amber),
                (Face::Land, amber, amber),
                (Face::Landed, amber, amber),
                (Face::Theirs, muted, muted),
                (Face::Off, muted, muted),
                (Face::HarnessOff, muted, muted),
                (Face::AgentOff, muted, muted),
                (Face::Unhosted, muted, muted),
            ] {
                let line = face.line(true, inks);
                let drawn: Vec<Style> = line.spans.iter().map(|span| span.style).collect();
                assert_eq!(drawn, [glyph, rest], "{face:?} {depth:?} {theme:?}");
                let alone = face.line(false, inks);
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
            let x = column_of(&text, &format!(" {face} ─[Cruise]")).unwrap() + 1;
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
        Some(Face::CruiseUntil(Lands {
            clock: clock.to_string(),
            day,
        }))
    };
    let (_, shared, _scope, _automation) = scene(
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
fn native_header_update_badge_preserves_the_brand_and_fits() {
    for width in 14..180 {
        let label = brand_label_with_update(width, "0.1.0-dev.14249", "0.166.0", true);
        if let Some(label) = label {
            assert!(
                UnicodeWidthStr::width(label.as_str()) + 2 <= width,
                "{width}: {label}"
            );
            assert!(label.contains("codex-alab") || label.starts_with("ALab "));
            if width >= 24 {
                assert!(label.ends_with("↑ update"), "{width}: {label}");
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
    let mode = row(&buffer, 0).find("[Cruise]").unwrap();
    let mode_col = row(&buffer, 0)[..mode].chars().count() as u16;
    assert!(!build_mouse_hit(mode_col, 0));
    set_enabled(false);
    assert!(!build_mouse_hit(3, 0));
}

#[test]
fn native_menu_hit_tracks_only_the_current_controls() {
    let _scene = scene(HarnessWord::Land, TaskFace::Open, None);
    let area = Rect::new(3, 2, 120, 4);
    paint(area);
    for x in 0..area.right() {
        assert_eq!(
            autopilot_menu_hit(x, 2),
            mode_mouse_hit(x, 2).is_some() || autopilot_words_hit(x, 2)
        );
        assert!(!autopilot_menu_hit(x, 3));
    }
    set_enabled(false);
    for x in 0..area.right() {
        assert!(!autopilot_menu_hit(x, 2));
    }
}
