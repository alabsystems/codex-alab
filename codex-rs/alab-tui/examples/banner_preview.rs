// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Print the launch card and composer switch with sample session data, without starting a model session.
//! `targo --unverified run -p codex-alab-tui --example banner_preview -- 69 light --mode off`

use clap::Parser;
use clap::ValueEnum;
use codex_alab_tui::banner::Banner;
use codex_alab_tui::banner::Depth;
use codex_alab_tui::banner::Style;
use codex_alab_tui::banner::Theme;
use codex_alab_tui::banner::artwork::write_history_span;
use codex_alab_tui::banner::explicit_monochrome;
use codex_alab_tui::banner::harness_state;
use codex_alab_tui::interface;
use codex_alab_tui::session;
use codex_alab_tui::session::Autopilot;
use codex_alab_tui::session::HarnessWord;
use codex_terminal_detection::TerminalName;
use codex_terminal_detection::terminal_info;
use ratatui::backend::IntoCrossterm;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;
use std::io;
use std::io::IsTerminal;
use std::io::Write;

const ALAB: &str = "0.11.0";
const CODEX: &str = "0.169.0";

#[derive(Parser)]
#[command(
    name = "banner_preview",
    about = "Print the ALab launch card and Autopilot switch."
)]
struct PreviewArgs {
    /// Pane width in columns. Defaults to stdout's width, or 80 in a pipe.
    #[arg(value_name = "WIDTH", allow_negative_numbers = true)]
    width: Option<String>,
    /// Use the light palette and its sample model. May be passed alone.
    #[arg(value_name = "light")]
    palette: Option<String>,
    // The original sample used only the first two positional words.
    #[arg(hide = true)]
    _extra: Vec<String>,
    /// Autopilot mode to show in the card and switch.
    #[arg(long, value_enum, default_value = "land")]
    mode: PreviewMode,
}

#[derive(Clone, Copy, ValueEnum)]
enum PreviewMode {
    Cruise,
    Land,
    Off,
}

fn stdout_width() -> Option<u16> {
    #[cfg(any(unix, windows))]
    if let Some((terminal_size::Width(width), terminal_size::Height(height))) =
        terminal_size::terminal_size_of(io::stdout())
        && width > 0
        && height > 0
    {
        return Some(width);
    }
    None
}

fn stdout_depth(out: &io::StdoutLock<'_>) -> Depth {
    if !out.is_terminal() || std::env::var_os("NO_COLOR").is_some() {
        return Depth::Mono;
    }
    let reported = match supports_color::on(supports_color::Stream::Stdout) {
        Some(level) if level.has_16m => Depth::TrueColor,
        Some(level) if level.has_256 => Depth::Ansi256,
        Some(_) => Depth::Ansi16,
        None => Depth::Mono,
    };
    // Match the App's Windows Terminal hints after honoring the sample's plain-output contract.
    if std::env::var_os("FORCE_COLOR").is_some()
        || (reported == Depth::Mono && explicit_monochrome())
    {
        return reported;
    }
    if std::env::var_os("WT_SESSION").is_some()
        || (reported == Depth::Ansi16 && terminal_info().name == TerminalName::WindowsTerminal)
    {
        Depth::TrueColor
    } else {
        reported
    }
}

fn preview_theme(sample: Theme, windows: bool) -> Theme {
    if windows {
        sample.with_terminal_foreground_if_unreported()
    } else {
        sample
    }
}

fn main() -> io::Result<()> {
    match print_preview() {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

fn print_preview() -> io::Result<()> {
    let args = PreviewArgs::parse();
    let (width, palette) = match (args.width, args.palette) {
        (Some(palette), None) if palette == "light" => (None, Some(palette)),
        positional => positional,
    };
    let width = match width {
        Some(arg) => arg.parse().ok(),
        None => stdout_width(),
    }
    .unwrap_or(80);
    let mut out = io::stdout().lock();
    codex_alab_core::activate();
    let depth = stdout_depth(&out);
    codex_alab_core::version::initialize_package(CODEX, Some(ALAB), Some(CODEX), None)
        .map_err(io::Error::other)?;
    interface::configure(true);
    let (policy, word) = match args.mode {
        PreviewMode::Cruise => (Autopilot::Active, HarnessWord::Cruise),
        PreviewMode::Land => (Autopilot::Active, HarnessWord::Land),
        PreviewMode::Off => (Autopilot::Passive, HarnessWord::Off),
    };
    session::shared().set_autopilot_with_word(policy, word);
    let directory = "directory: ~/codex-alab";
    let (theme, details) = match palette.as_deref() {
        Some("light") => (
            Theme::from_bg(Some((255, 255, 255))),
            vec![
                "model: GPT-6.2-Astra · Ultrafast xhigh".into(),
                directory.into(),
                "permissions: YOLO mode".into(),
            ],
        ),
        _ => (
            Theme::default(),
            vec!["model: gpt-6.1".into(), directory.into()],
        ),
    };
    let theme = preview_theme(theme, cfg!(windows));
    let banner =
        Banner::new(Style::Codex, depth, theme, ALAB).with_upstream(Some(CODEX.to_owned()));
    let harness = harness_state();
    let mut lines = banner.session_lines(width, harness, details);
    if lines.is_empty() {
        lines = interface::plain_title_rows(ALAB, CODEX, usize::from(width));
        if let Some(label) = harness.and_then(|harness| harness.compact_mode_label(width)) {
            lines.push(Line::from(label));
        }
    }
    let composer = Rect::new(0, 0, width, 3);
    let mut buffer = Buffer::empty(composer);
    interface::render(composer, &mut buffer, depth, theme);
    lines.push(Line::default());
    lines.push(Line::from_iter((0..width).map(|x| {
        let cell = &buffer[(x, 0)];
        Span::styled(cell.symbol().to_owned(), cell.style())
    })));
    for line in lines {
        if depth == Depth::Mono {
            writeln!(out, "{line}")?;
            continue;
        }
        for span in line.spans {
            if !write_history_span(&mut out, &span)? {
                write!(out, "{}", span.style.into_crossterm().apply(span.content))?;
            }
        }
        writeln!(out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_alab_tui::banner::detail_style;
    use ratatui::style::Color;
    use ratatui::style::Modifier;

    #[test]
    fn only_the_unreported_windows_sample_uses_terminal_foreground() {
        let unreported = Theme::default();
        assert_eq!(preview_theme(unreported, false), unreported);
        let caption = detail_style(Depth::TrueColor, preview_theme(unreported, true));
        assert_eq!(caption.fg, Some(Color::Reset));
        assert!(caption.sub_modifier.contains(Modifier::DIM));

        let white = Theme::from_bg(Some((255, 255, 255)));
        assert_eq!(preview_theme(white, true), white);
    }
}
