// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A deterministic frame of the production composer chrome for a private terminal.
//! Run `targo --unverified run -p codex-alab-tui --example interface_preview --
//! 160 [on|off|stale] [dark|light]`. It waits for input after painting so the lab can
//! capture a settled frame, then ends with the lab's own process group.

use codex_alab_tui::banner::Depth;
use codex_alab_tui::banner::Theme;
use codex_alab_tui::interface;
use codex_alab_tui::interface::InterfaceStatus;
use codex_alab_tui::interface::ResetCredits;
use codex_alab_tui::quota::History;
use codex_alab_tui::quota::WeeklySnapshot;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::Widget;
use std::io;
use std::io::Read;
use std::io::Write;
use unicode_width::UnicodeWidthStr;

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let width = args
        .next()
        .and_then(|width| width.parse::<u16>().ok())
        .unwrap_or(160)
        .clamp(20, 240);
    let mode = args.next().unwrap_or_else(|| "on".to_string());
    let light = args.next().is_some_and(|theme| theme == "light");
    let theme = Theme::from_bg(Some(if light { (255, 255, 255) } else { (16, 20, 24) }));
    let state = Box::leak(Box::new(interface::State::new()));
    let _scope = interface::scoped(state);
    interface::configure(mode != "off");

    let mut history = History::default();
    for (day, used_percent) in [(0, 10.0), (1, 20.0), (2, 30.0)] {
        history.observe(WeeklySnapshot {
            used_percent,
            observed_at: 1_000 + day * 86_400,
            resets_at: 605_800,
            window_minutes: 10_080,
        });
    }
    interface::set_status(InterfaceStatus {
        account: "person@example.test".into(),
        plan: Some("Pro 500".into()),
        weekly: Some(if mode == "stale" {
            "weekly stale".into()
        } else {
            "weekly 70% left".into()
        }),
        weekly_reset: Some("Fri 16:00 PDT".into()),
        usage_stale: mode == "stale",
        mode: Some("Default / Full access".into()),
        activity: Some("Working".into()),
        quota: history.view(173_800),
        reset_credits: Some(ResetCredits {
            available_count: 3,
            expiring_count: 1,
            expired_count: 0,
            next_expiry: Some("Thu 16:00 PDT".into()),
            stale: mode == "stale",
            details_known: true,
        }),
        ..Default::default()
    });

    let area = Rect::new(0, 0, width, 8);
    let composer = Rect::new(0, 1, width, 5);
    let mut buf = Buffer::empty(area);
    Block::bordered()
        .border_style(Style::default().fg(Color::DarkGray))
        .render(composer, &mut buf);
    Line::from("> Describe the next change…").render(
        Rect::new(2, composer.y + 2, width.saturating_sub(4), 1),
        &mut buf,
    );
    interface::render(composer, &mut buf, Depth::TrueColor, theme);
    interface::render_status_current(
        Rect::new(2, composer.bottom(), width.saturating_sub(4), 1),
        &mut buf,
        Depth::TrueColor,
        theme,
    );

    let mut out = io::stdout().lock();
    write!(out, "\x1b[2J\x1b[H\x1b[?25l")?;
    for y in area.y..area.bottom() {
        write!(out, "\x1b[{};1H", y + 1)?;
        let mut x = area.x;
        while x < area.right() {
            let cell = &buf[(x, y)];
            write!(out, "\x1b[0m")?;
            color(&mut out, cell.fg, false)?;
            color(&mut out, cell.bg, true)?;
            if cell.modifier.contains(Modifier::BOLD) {
                write!(out, "\x1b[1m")?;
            }
            if cell.modifier.contains(Modifier::DIM) {
                write!(out, "\x1b[2m")?;
            }
            write!(out, "{}", cell.symbol())?;
            x = x.saturating_add(UnicodeWidthStr::width(cell.symbol()).max(1) as u16);
        }
    }
    write!(out, "\x1b[0m")?;
    out.flush()?;
    drop(out);
    let _ = io::stdin().read(&mut [0u8; 1])?;
    write!(io::stdout(), "\x1b[?25h")
}

fn color(out: &mut impl Write, color: Color, background: bool) -> io::Result<()> {
    let channel = if background { 48 } else { 38 };
    let index = match color {
        Color::Reset => return write!(out, "\x1b[{}m", if background { 49 } else { 39 }),
        Color::Rgb(r, g, b) => return write!(out, "\x1b[{channel};2;{r};{g};{b}m"),
        Color::Indexed(index) => index,
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
    };
    write!(out, "\x1b[{channel};5;{index}m")
}
