// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::app::App;
use crate::app::owned_transcript::tests::attach_thread;
use crate::app::owned_transcript::tests::buffer_text;
use crate::app::tests::make_test_app_with_channels;
use crate::app_event::AppEvent;
use crate::app_server_session::AppServerSession;
use crate::custom_terminal::test_support::last_rendered_buffer;
use crate::history_cell::SessionInfoCell;
use crate::tui::Tui;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use codex_alab_core::session::Autopilot;
use codex_alab_core::session::HarnessWord;
use codex_alab_core::session::Shared;
use codex_alab_tui::interface;
use codex_protocol::ThreadId;
use color_eyre::eyre::OptionExt;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::buffer::Cell;
use ratatui::layout::Position;
use ratatui::layout::Size;
use ratatui::style::Color;
use tokio::sync::mpsc::UnboundedReceiver;

#[path = "banner_selection_tests.rs"]
mod selection;

async fn in_direct_aterm(case: &str) -> color_eyre::Result<bool> {
    run_writer_case(case, "aterm", check_native_uploads).await
}

/// Terminal detection and package identity are process-wide. Each child writes through its host's
/// native scrollback policy, and the parent reads the bytes sent to that terminal.
async fn run_writer_case(
    case: &str,
    term_program: &str,
    check: impl FnOnce(&str) -> color_eyre::Result<()>,
) -> color_eyre::Result<bool> {
    const CHILD: &str = "ALAB_TEST_LIVE_KITTY";
    let (_, module) = module_path!()
        .split_once("::")
        .ok_or_eyre("crate module path")?;
    let name = format!("{module}::{case}");
    // Capture tools set CHILD on an exact libtest invocation. A suite inheriting the
    // same environment must still give each package identity its own process.
    let windows_terminal = term_program == "WindowsTerminal";
    if std::env::var_os(CHILD).is_some()
        && std::env::var("TERM_PROGRAM").is_ok_and(|host| host == term_program)
        && std::env::var_os("WT_SESSION").is_some() == windows_terminal
        && std::env::args_os().any(|arg| arg == "--exact")
        && std::env::args_os()
            .skip(1)
            .filter(|arg| !arg.as_encoded_bytes().starts_with(b"-"))
            .eq([std::ffi::OsString::from(&name)])
    {
        return Ok(true);
    }
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .args(["--exact", &name, "--nocapture"])
        .env(CHILD, "1")
        .env("TERM", "xterm-256color")
        .env("TERM_PROGRAM", term_program)
        .env("COLORTERM", "truecolor")
        .env_remove("NO_COLOR")
        .env_remove(codex_alab_core::session::ATERM_SESSION_ENV)
        .env_remove("ATERM_SESSION_ID")
        .env("RUST_MIN_STACK", "8388608")
        .kill_on_drop(true);
    if windows_terminal {
        command
            .env("WT_SESSION", "632aef5a-74ab-4bd6-a4a7-8e140662a10c")
            .env_remove("TERM_PROGRAM_VERSION");
    } else {
        command
            .env_remove("WT_SESSION")
            .env("TERM_PROGRAM_VERSION", "0.100.0");
    }
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
    let output =
        tokio::time::timeout(std::time::Duration::from_secs(60), command.output()).await??;
    let writer = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && writer.contains("test result: ok. 1 passed"),
        "{name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    check(&writer)?;
    Ok(false)
}

fn check_native_uploads(writer: &str) -> color_eyre::Result<()> {
    let (before_tiles, _) = writer
        .split_once('\u{10eeee}')
        .ok_or_eyre("native writer drew a tile")?;
    for (id, cols, rows, png) in [
        (
            0xa1ab01,
            10,
            5,
            include_bytes!("../src/banner/assets/codex-mark.png").as_slice(),
        ),
        (
            0xa1ab02,
            22,
            4,
            include_bytes!("../src/banner/assets/rainbow-kitty.png").as_slice(),
        ),
    ] {
        let upload = format!("\x1b_Ga=T,t=d,f=100,U=1,i={id},c={cols},r={rows},");
        let (_, transfer) = before_tiles
            .split_once(&upload)
            .ok_or_eyre("first-frame upload")?;
        let mut encoded = String::new();
        for command in transfer.split("\x1b\\") {
            let (header, data) = command.split_once(';').ok_or_eyre("Kitty PNG chunk")?;
            encoded.push_str(data);
            if header.ends_with("m=0") {
                break;
            }
            assert!(header.ends_with("m=1"), "unexpected chunk for {id:x}");
        }
        assert!(
            STANDARD.decode(encoded)? == png,
            "wrong PNG for image {id:x}"
        );
        assert_eq!(writer.matches(&upload).count(), 1, "reuploaded {id:x}");
    }
    Ok(())
}

const FLUSH_BOUNDARY: &str = "\x1b]777;alab-inline-flush\x07";

#[tokio::test]
async fn inline_launch_uses_the_reported_background_on_its_first_write() -> color_eyre::Result<()> {
    inline_launch_case(
        "inline_launch_uses_the_reported_background_on_its_first_write",
        /*report_before_write*/ true,
    )
    .await
}

#[tokio::test]
async fn inline_launch_uses_terminal_ink_when_first_written_before_the_report()
-> color_eyre::Result<()> {
    inline_launch_case(
        "inline_launch_uses_terminal_ink_when_first_written_before_the_report",
        /*report_before_write*/ false,
    )
    .await
}

async fn inline_launch_case(case: &str, report_before_write: bool) -> color_eyre::Result<()> {
    use crate::history_cell::PlainHistoryCell;
    use crate::terminal_palette::with_test_default_colors;
    use crate::terminal_probe::DefaultColors;
    use std::io::Write;

    if !run_writer_case(case, "WindowsTerminal", |writer| {
        check_inline_flush(writer, report_before_write)
    })
    .await?
    {
        return Ok(());
    }
    codex_alab_core::version::initialize_stamped_package(
        "0.170.0",
        Some("0.11.0"),
        Some("0.169.0"),
        None,
        None,
    )?;
    codex_alab_core::activate();
    let shared = Box::leak(Box::new(Shared::new()));
    let _host = codex_alab_tui::boxes::scoped(shared);
    let _edition = interface::scoped(Box::leak(Box::new(interface::State::new())));
    interface::configure(true);
    shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    SessionInfoCell::with_test_windows_launch(|| {
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.set_alt_screen_enabled(false);
        assert_eq!(
            codex_terminal_detection::terminal_info().name,
            codex_terminal_detection::TerminalName::WindowsTerminal
        );
        // A tall composer drives the earlier row into physical scrollback when the header is written.
        tui.terminal
            .set_viewport_area(ratatui::layout::Rect::new(0, 2, 80, 20));
        let text = |message: &'static str| Box::new(PlainHistoryCell::new(vec![message.into()]));
        let boundary = || std::io::stdout().write_all(FLUSH_BOUNDARY.as_bytes());

        app.insert_history_cell(&mut tui, text("previously printed row"));
        // With alternate screens disabled, opening an inline overlay flushes history and returns.
        tui.enter_alt_screen()?;
        boundary()?;
        let model = with_test_default_colors(None, || -> color_eyre::Result<_> {
            SessionInfoCell::with_test_launch_card(|| attach_thread(&mut app, ThreadId::new()));
            let header = std::iter::from_fn(|| events.try_recv().ok())
                .find_map(|event| match event {
                    AppEvent::InsertHistoryCell(cell)
                    | AppEvent::AlabInsertSideHistory { cell, .. }
                        if cell.as_any().is::<SessionInfoCell>() =>
                    {
                        Some(cell)
                    }
                    _ => None,
                })
                .ok_or_eyre("session header from the attached thread")?;
            let model = header
                .as_any()
                .downcast_ref::<SessionInfoCell>()
                .and_then(SessionInfoCell::live_banner_model)
                .ok_or_eyre("enriched launch card")?;
            app.insert_history_cell(&mut tui, text("before queued header"));
            app.insert_history_cell(&mut tui, header);
            app.insert_history_cell(&mut tui, text("after queued header"));
            Ok(model)
        })?;
        model
            .lock()
            .map_err(|_| color_eyre::eyre::eyre!("queued launch model lock poisoned"))?
            .update("changed-after-queue".into());
        shared.set_autopilot_with_word(Autopilot::Passive, HarnessWord::Off);
        boundary()?;
        let white = DefaultColors {
            fg: (32, 33, 36),
            bg: (255, 255, 255),
        };
        with_test_default_colors(report_before_write.then_some(white), || {
            tui.enter_alt_screen()
        })?;
        boundary()?;
        with_test_default_colors(white, || tui.enter_alt_screen())?;
        boundary()?;
        Ok(())
    })
}

fn check_inline_flush(writer: &str, report_before_write: bool) -> color_eyre::Result<()> {
    use vt100::Color::Default;
    use vt100::Color::Rgb;

    assert!(!writer.contains("\x1b]10;?"));
    assert!(!writer.contains("\x1b]11;?"));
    let phases: Vec<_> = writer.split(FLUSH_BOUNDARY).collect();
    let [previous, queued, first, repeat, _] = phases.as_slice() else {
        color_eyre::eyre::bail!("missing inline flush boundaries");
    };
    assert!(previous.contains("previously printed row"));
    assert_eq!(writer.matches("previously printed row").count(), 1);
    assert!(!queued.contains("before queued header"));
    assert!(!queued.contains("0.11 on Codex 0.169"));
    assert!(repeat.is_empty());
    let mut remaining = *first;
    for word in [
        "before queued header",
        "0.11 on Codex 0.169",
        "Model",
        "gpt-test",
        "⤓ land",
        "after queued header",
    ] {
        assert_eq!(writer.matches(word).count(), 1, "{word}");
        remaining = remaining
            .split_once(word)
            .ok_or_eyre("ordered inline card")?
            .1;
    }
    assert!(!first.contains("changed-after-queue"));
    assert!(!first.contains("○ off"));
    let mut terminal = vt100::Parser::new(24, 80, 64);
    terminal.process(previous.as_bytes());
    assert!(
        terminal
            .screen()
            .contents()
            .contains("previously printed row")
    );
    terminal.process(queued.as_bytes());
    // The light palette arrives either side of the first write. Terminal-default ink delegates
    // critical details to the user's configured foreground until that report exists.
    let (title, face, caption, yellow, green) = if report_before_write {
        (
            Rgb(123, 108, 162),
            Rgb(142, 112, 92),
            Rgb(115, 117, 137),
            Rgb(132, 116, 11),
            Rgb(90, 130, 11),
        )
    } else {
        (
            Default,
            Default,
            Default,
            Rgb(255, 226, 0),
            Rgb(168, 255, 0),
        )
    };
    let mut remaining = *first;
    for (glyph, ink) in [
        ("ALab", title),
        ("•", face),
        ("•", face),
        ("Model", caption),
        ("▂", yellow),
        ("▃", green),
        ("▾", face),
        ("Directory", caption),
        ("Autopilot", caption),
    ] {
        let (prefix, suffix) = remaining
            .split_once(glyph)
            .ok_or_eyre("kitty or caption glyph")?;
        terminal.process(prefix.as_bytes());
        assert_eq!(terminal.screen().fgcolor(), ink, "{glyph}");
        assert_eq!(terminal.screen().bold(), glyph == "ALab", "{glyph}");
        assert!(!terminal.screen().dim(), "{glyph}");
        terminal.process(glyph.as_bytes());
        remaining = suffix;
    }
    terminal.process(remaining.as_bytes());
    assert!(
        !terminal
            .screen()
            .contents()
            .contains("previously printed row")
    );
    terminal.screen_mut().set_scrollback(24);
    assert!(
        terminal
            .screen()
            .contents()
            .contains("previously printed row")
    );
    Ok(())
}

async fn open_card(
    app: &mut App,
    events: &mut UnboundedReceiver<AppEvent>,
    size: Size,
) -> color_eyre::Result<(Tui, AppServerSession)> {
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.terminal = crate::tui::Terminal::with_screen_size_and_cursor_position_for_test(
        CrosstermBackend::new(std::io::stdout()),
        size,
        Position::default(),
    );
    tui.set_owned_screen(true)?;
    codex_alab_core::activate();
    crate::terminal_palette::with_test_default_colors(None, || {
        SessionInfoCell::with_test_launch_card(|| attach_thread(app, ThreadId::new()));
    });
    while let Ok(event) = events.try_recv() {
        if matches!(
            event,
            AppEvent::InsertHistoryCell(_) | AppEvent::AlabInsertSideHistory { .. }
        ) {
            Box::pin(app.handle_event(&mut tui, &mut server, event)).await?;
        }
    }
    Ok((tui, server))
}

#[tokio::test]
async fn model_changes_reuse_the_native_kitty_at_68_columns() -> color_eyre::Result<()> {
    if !in_direct_aterm("model_changes_reuse_the_native_kitty_at_68_columns").await? {
        return Ok(());
    }
    codex_alab_core::version::initialize_package("0.164.0-dev.12814+ga180bdb0c", None, None, None)?;
    let size = Size::new(68, 24);
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let (mut tui, mut server) = Box::pin(open_card(&mut app, &mut events, size)).await?;
    let card = app
        .transcript_cells
        .iter()
        .find(|cell| cell.as_any().is::<SessionInfoCell>())
        .cloned()
        .ok_or_eyre("attached conversation's launch card")?;
    selection::check(&card)?;
    let raw = card.raw_lines();
    assert!(
        raw.iter()
            .any(|line| line.to_string().contains("model: gpt-test"))
    );
    let long = "production-deployment-with-long-name";
    for (model, kitty_cells) in [("gpt-test", 88), (long, 0), ("gpt-test", 88)] {
        if app.chat_widget.current_model() != model {
            Box::pin(app.handle_event(&mut tui, &mut server, AppEvent::UpdateModel(model.into())))
                .await?;
        }
        app.render_chat_widget_frame(&mut tui, size)?;
        let buffer = last_rendered_buffer(&tui.terminal);
        let screen = buffer_text(buffer);
        assert!(
            screen
                .lines()
                .any(|row| row.contains("Model") && row.contains(model)),
            "{screen}"
        );
        let mut card_rows = screen.lines().skip_while(|row| !row.contains("Codex ALab"));
        assert!(
            card_rows
                .nth(1)
                .is_some_and(|row| row.contains("0.164 on Codex unreleased")),
            "{screen}"
        );
        let tiles = |id| {
            buffer
                .content()
                .iter()
                .filter(|cell| image_tile(cell, id))
                .count()
        };
        assert_eq!(tiles(1), 50, "{model}");
        assert_eq!(tiles(2), kitty_cells, "{model}");
        assert_eq!(card.raw_lines(), raw);
    }
    tui.set_owned_screen(false)?;
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn stamped_launch_shows_both_images_and_all_three_modes() -> color_eyre::Result<()> {
    if !in_direct_aterm("stamped_launch_shows_both_images_and_all_three_modes").await? {
        return Ok(());
    }
    // A packaged source snapshot carries its Codex baseline independently of the client version.
    codex_alab_core::version::initialize_stamped_package(
        "0.170.0",
        Some("0.11.0"),
        Some("0.169.0"),
        None,
        None,
    )?;
    let shared = Box::leak(Box::new(Shared::new()));
    let _host = codex_alab_tui::boxes::scoped(shared);
    let _interface = interface::scoped(Box::leak(Box::new(interface::State::new())));
    interface::configure(true);
    let size = Size::new(80, 24);
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let (mut tui, server) = Box::pin(open_card(&mut app, &mut events, size)).await?;
    let release = "0.11 on Codex 0.169";
    let card = app
        .transcript_cells
        .iter()
        .find(|cell| cell.as_any().is::<SessionInfoCell>())
        .ok_or_eyre("attached conversation's launch card")?;
    let narrow: Vec<_> = card
        .display_lines(4)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(
        narrow.windows(3).any(|rows| rows == ["Cod…", "0.16", "9"]),
        "{narrow:?}"
    );
    assert!(
        card.raw_lines()
            .iter()
            .any(|row| row.to_string().contains(release))
    );
    for (policy, mode, face, track) in [
        (
            Autopilot::Active,
            HarnessWord::Cruise,
            "∞ cruise",
            "[Cruise]Land│Off▕",
        ),
        (
            Autopilot::Active,
            HarnessWord::Land,
            "⤓ land",
            "▏Cruise[Land]Off▕",
        ),
        (
            Autopilot::Passive,
            HarnessWord::Off,
            "○ off",
            "▏Cruise│Land[Off]",
        ),
    ] {
        shared.set_autopilot_with_word(policy, mode);
        app.render_chat_widget_frame(&mut tui, size)?;
        let buffer = last_rendered_buffer(&tui.terminal);
        let screen = buffer_text(buffer);
        let mut rows = screen.lines().enumerate();
        let (title_y, _) = rows
            .find(|(_, row)| row.contains("Codex ALab"))
            .ok_or_eyre("launch title")?;
        assert_illustrated_row(buffer, title_y, "Codex ALab")?;
        assert_illustrated_row(buffer, title_y + 1, release)?;
        let card_mode = rows
            .by_ref()
            .take_while(|(_, row)| !row.contains('╰'))
            .find_map(|(_, row)| row.split_once("Autopilot").map(|(_, value)| value))
            .ok_or_eyre("Autopilot fact inside the launch card")?;
        assert_eq!(card_mode.trim_matches([' ', '│']), face, "{mode:?}");
        let (_, composer) = rows
            .find(|(_, row)| row.contains("[ALab on]"))
            .ok_or_eyre("composer overline")?;
        assert!(composer.contains(release), "{mode:?}: {composer}");
        assert!(
            composer.replace(' ', "").contains(track),
            "{mode:?}: {composer}"
        );
    }
    app.chat_widget
        .dispatch_command(crate::slash_command::SlashCommand::Status);
    let status = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| match event {
            AppEvent::InsertHistoryCell(cell)
                if cell
                    .raw_lines()
                    .first()
                    .is_some_and(|line| line.to_string() == "/status") =>
            {
                Some(cell)
            }
            _ => None,
        })
        .ok_or_eyre("the /status command created its card")?;
    assert_status_title(status.as_ref());
    assert_configured_release(&mut app.chat_widget, release);
    tui.set_owned_screen(false)?;
    server.shutdown().await?;
    Ok(())
}

fn assert_configured_release(chat: &mut crate::chatwidget::ChatWidget, release: &str) {
    use crate::bottom_pane::StatusLineItem;
    use crate::bottom_pane::TerminalTitleItem;
    use crate::chatwidget::tests::render_bottom_popup;
    use crate::slash_command::SlashCommand;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    chat.setup_status_line(vec![StatusLineItem::CodexVersion], false);
    chat.setup_terminal_title(vec![TerminalTitleItem::CodexVersion]);
    for (enhanced, expected) in [(true, release), (false, "0.11.0")] {
        interface::set_enabled(enhanced);
        chat.refresh_status_surfaces();
        assert_eq!(chat.status_line_text().as_deref(), Some(expected));
        assert_eq!(chat.last_terminal_title.as_deref(), Some(expected));
        for command in [SlashCommand::Statusline, SlashCommand::Title] {
            chat.dispatch_command(command);
            let popup = render_bottom_popup(chat, 100);
            assert!(popup.lines().any(|line| line.trim() == expected), "{popup}");
            chat.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
    }
}

/// The command echo and body each have their own paragraph in the real status cell.
fn assert_status_title(status: &dyn crate::history_cell::HistoryCell) {
    let title = |width| {
        status
            .display_lines(width)
            .into_iter()
            .skip_while(|line| line.width() != 0)
            // The native hard wrap can leave an empty title row after the echo's separator.
            .skip_while(|line| line.width() == 0)
            .take_while(|line| line.width() != 0)
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(title(33), ["  Codex ALab  0.11 on Codex 0.169"]);
    assert_eq!(title(28), ["  Codex ALab", "  0.11 on Codex 0.169"]);
    assert_eq!(title(19), ["  Codex ALab", "0.11 on Codex 0.169"]);
    assert_eq!(title(13), ["  Codex ALab", "  0.11 on", "  Codex 0.169"]);
    for width in 1..=33 {
        let rows = title(width);
        assert!(
            rows.iter().all(|row| row.len() <= usize::from(width)),
            "{width}: {rows:?}"
        );
        assert_eq!(
            rows.concat().replace(' ', ""),
            "CodexALab0.11onCodex0.169",
            "{width}: {rows:?}"
        );
    }
}

/// Both rows share the upper-left Codex mark and the right-hand rainbow kitty.
fn assert_illustrated_row(buffer: &Buffer, y: usize, text: &str) -> color_eyre::Result<()> {
    let row = buffer
        .content()
        .chunks_exact(usize::from(buffer.area.width))
        .nth(y)
        .ok_or_eyre("complete row")?;
    let mark = row
        .iter()
        .rposition(|cell| image_tile(cell, 1))
        .ok_or_eyre("Codex mark")?;
    let kitty = row
        .iter()
        .position(|cell| image_tile(cell, 2))
        .ok_or_eyre("rainbow kitty")?;
    assert!(mark < kitty, "{text}: logo must precede kitty");
    let between: String = row[mark + 1..kitty].iter().map(Cell::symbol).collect();
    assert_eq!(between.trim(), text);
    Ok(())
}

fn image_tile(cell: &Cell, id: u8) -> bool {
    cell.symbol().starts_with('\u{10eeee}')
        && matches!(cell.fg, Color::Rgb(0xa1, 0xab, image_id) if image_id == id)
}
