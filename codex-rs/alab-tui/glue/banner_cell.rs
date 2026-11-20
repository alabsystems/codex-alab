// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Launch-card integration for codex-tui's private HistoryCell trait.
//!
//! The default Codex card integrates the header's model, directory and permissions into one
//! framed banner. The existing explicit ribbon/stripes styles keep the separate header.
//! Raw scrollback and export retain the original header. Visual selection copies the displayed
//! card text, with a blank cell for each native image tile. The first session
//! header consumes the launch slot even when artwork is disabled, so /new never adds art.
//! The cell tracks the live harness, displayed landing time and terminal width in its render cache. It remains
//! startup metadata, excluded from activity and export, and shares the existing blossom
//! placement hook. All drawing lives in codex_alab_tui::banner; this glue is ALab-owned.

use crate::history_cell::HistoryCell;
use crate::line_truncation::span_width;
use crate::local_settings::LocalSettings;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::plain_hyperlink_lines;
use crate::terminal_palette;
use crate::terminal_palette::StdoutColorLevel;
use codex_alab_tui::banner::Banner;
use codex_alab_tui::banner::Depth;
use codex_alab_tui::banner::Harness;
use codex_alab_tui::banner::Theme;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use std::path::Path;
use std::sync::Mutex;
use std::sync::PoisonError;

#[derive(Debug, Default)]
pub(crate) struct BannerModel {
    label: String,
    revision: u64,
    dirty: bool,
    resolved_banner: Option<Banner>,
}

impl BannerModel {
    pub(crate) fn update(&mut self, label: String) {
        if self.label != label {
            self.label = label;
            self.invalidate();
        }
    }

    fn invalidate(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.dirty = true;
    }
}

pub(crate) type LiveBannerModel = std::sync::Arc<Mutex<BannerModel>>;

fn windows_startup_palette() -> bool {
    #[cfg(test)]
    {
        tests::WINDOWS_LAUNCH.get()
    }
    #[cfg(not(test))]
    {
        cfg!(windows)
    }
}

fn terminal_colors() -> (Depth, Theme) {
    let depth = match terminal_palette::effective_stdout_color_level() {
        StdoutColorLevel::TrueColor => Depth::TrueColor,
        StdoutColorLevel::Ansi256 => Depth::Ansi256,
        StdoutColorLevel::Ansi16 => Depth::Ansi16,
        // NO_COLOR, TERM=dumb, not a terminal.
        StdoutColorLevel::Unknown => Depth::Mono,
    };
    let theme = Theme::from_bg(terminal_palette::default_bg());
    let theme = if windows_startup_palette() {
        theme.with_terminal_foreground_if_unreported()
    } else {
        theme
    };
    (depth, theme)
}

pub(super) fn plain_detail_style() -> Style {
    if !codex_alab_tui::interface::active() {
        return Style::default().dim();
    }
    let (depth, theme) = terminal_colors();
    codex_alab_tui::banner::detail_style(depth, theme)
}

pub(super) fn plain_permission(value: Span<'static>) -> Span<'static> {
    if !codex_alab_tui::interface::active() {
        return value;
    }
    let (depth, theme) = terminal_colors();
    value.style(codex_alab_tui::banner::permission_style(depth, theme))
}

/// Fit the directory of an enhanced plain header, keeping native center truncation
/// when it already identifies the checkout within the usual five-space indent.
pub(super) fn plain_directory(
    header: &super::SessionHeaderHistoryCell,
    width: usize,
    detail_style: Style,
) -> Option<Line<'static>> {
    if !codex_alab_tui::interface::active() {
        return None;
    }
    let margin = "     ";
    let row = |path: String, indent: usize| {
        Line::from(vec![
            Span::raw(&margin[..indent]),
            Span::styled(path, detail_style),
        ])
    };
    let path = header.format_directory(None);
    let cells = span_width(&Span::raw(path.as_str()));
    if cells <= width {
        return Some(row(path, (width - cells).min(margin.len())));
    }

    let budget = width.saturating_sub(margin.len());
    let centered = crate::text_formatting::center_truncate_path(&path, budget);
    let trimmed = path.trim_end_matches(std::path::is_separator);
    if let Some(name) = Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        && let Some(prefix) = trimmed.strip_suffix(name)
    {
        // Include any original trailing separators in the displayed checkout name.
        let tail = &path[prefix.len()..];
        let tail_width = span_width(&Span::raw(tail));
        if tail_width <= width
            && (!centered.ends_with(tail) || span_width(&Span::raw(centered.as_str())) > budget)
        {
            let spare = width - tail_width;
            let separator = prefix
                .chars()
                .next_back()
                .filter(|c| std::path::is_separator(*c));
            let (visible, marker_width) = match (spare, separator) {
                (0, _) => (tail.to_owned(), 0),
                (1, _) | (_, None) => (format!("…{tail}"), 1),
                (_, Some(separator)) => (format!("…{separator}{tail}"), 2),
            };
            return Some(row(visible, (spare - marker_width).min(margin.len())));
        }
    }
    Some(row(centered, margin.len()))
}

impl super::SessionInfoCell {
    fn codex_launch_card(&self) -> Option<&BannerCell> {
        let card = self
            .0
            .parts
            .first()?
            .as_any()
            .downcast_ref::<BannerCell>()?;
        card.banner.is_codex().then_some(card)
    }

    pub(crate) fn has_codex_launch_card(&self) -> bool {
        self.codex_launch_card().is_some()
    }

    #[cfg(any(windows, test))]
    pub(crate) fn pending_inline_art(
        cell: &dyn HistoryCell,
        width: u16,
        mode: crate::history_cell::HistoryRenderMode,
        preceded_by_history: bool,
    ) -> Option<crate::tui::alab_inline_launch::PendingLaunchArt> {
        if mode != crate::history_cell::HistoryRenderMode::Rich {
            return None;
        }
        let card = cell.as_any().downcast_ref::<Self>()?.codex_launch_card()?;
        if codex_alab_tui::interface::is_vanilla() || terminal_palette::default_bg().is_some() {
            return None;
        }
        let model = card.model.lock().unwrap_or_else(PoisonError::into_inner);
        let cache = card.cache.lock().unwrap_or_else(PoisonError::into_inner);
        let (cached_width, harness, art) = cache.as_ref()?;
        if model.dirty
            || model.resolved_banner.is_some()
            || *cached_width != width
            || art.is_empty()
        {
            return None;
        }
        let start = usize::from(preceded_by_history);
        Some(crate::tui::alab_inline_launch::PendingLaunchArt {
            rows: start..start + art.len(),
            banner: card.banner.clone(),
            width,
            harness: *harness,
            details: card.session_details(&model.label),
        })
    }

    pub(crate) fn live_banner_model(&self) -> Option<LiveBannerModel> {
        self.codex_launch_card().map(|card| card.model.clone())
    }
}

/// The width and harness the art was last drawn for, with what was drawn.
type ArtCache = Option<(u16, Option<Harness>, Vec<Line<'static>>)>;

/// The banner over upstream's session header, as one history cell. The art is drawn for each
/// width it is asked for (cached for the last one, since owned mode lays out again on every
/// resize) and is absent from raw and export text; every view of the header is the
/// header's own. The trait's other methods keep their defaults, as the header's do.
#[derive(Debug)]
struct BannerCell {
    model: LiveBannerModel,
    banner: Banner,
    cache: Mutex<ArtCache>,
    header: Box<dyn HistoryCell>,
}

impl BannerCell {
    fn session_details(&self, model: &str) -> Vec<Line<'static>> {
        let mut details: Vec<_> = self.header.raw_lines().into_iter().skip(1).collect();
        if !model.is_empty()
            && let Some(line) = details.first_mut()
        {
            *line = Line::from(format!("model: {model}"));
        }
        details
    }

    fn live_model(&self) -> std::sync::MutexGuard<'_, BannerModel> {
        let mut model = self.model.lock().unwrap_or_else(PoisonError::into_inner);
        // On Windows, a resumed card can precede the protected startup color probe.
        if cfg!(any(windows, test))
            && model.resolved_banner.is_none()
            && let Some(banner) = self
                .banner
                .with_late_background(terminal_palette::default_bg)
        {
            model.resolved_banner = Some(banner);
            model.invalidate();
        }
        model
    }

    fn art(&self, width: u16, state: Option<Harness>) -> Vec<Line<'static>> {
        let mut model = self.live_model();
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((cached, cached_state, lines)) = cache.as_ref()
            && !model.dirty
            && *cached == width
            && *cached_state == state
        {
            return lines.clone();
        }
        // The card names the live Autopilot word; session details stay in the same frame.
        let banner = model.resolved_banner.as_ref().unwrap_or(&self.banner);
        let lines = if banner.is_codex() {
            banner.session_lines(width, state, self.session_details(&model.label))
        } else {
            banner.lines_with_harness(width, state)
        };
        model.dirty = false;
        *cache = Some((width, state, lines.clone()));
        lines
    }

    fn art_then<T: From<&'static str>>(
        &self,
        width: u16,
        art: impl FnOnce(Vec<Line<'static>>) -> Vec<T>,
        header: impl FnOnce() -> Vec<T>,
    ) -> Vec<T> {
        if codex_alab_tui::interface::is_vanilla() {
            return header();
        }
        let harness = self.banner.live_harness();
        let mut lines = art(self.art(width, harness));
        if !self.banner.is_codex() {
            lines.extend(header());
        } else if lines.is_empty() {
            lines = header();
            if let Some(label) = harness.and_then(|state| state.compact_mode_label(width)) {
                lines.push(label.into());
            }
        }
        lines
    }
}

impl HistoryCell for BannerCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.art_then(width, |art| art, || self.header.display_lines(width))
    }

    fn display_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.art_then(width, plain_hyperlink_lines, || {
            self.header.display_hyperlink_lines(width)
        })
    }

    fn compact_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.art_then(width, plain_hyperlink_lines, || {
            self.header.compact_hyperlink_lines(width)
        })
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.art_then(width, |art| art, || self.header.transcript_lines(width))
    }

    fn transcript_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.art_then(width, plain_hyperlink_lines, || {
            self.header.transcript_hyperlink_lines(width)
        })
    }

    /// Raw scrollback and export carry the original header.
    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.header.raw_lines()
    }

    fn live_raw_lines(&self) -> Vec<Line<'static>> {
        self.header.live_raw_lines()
    }

    fn transcript_animation_tick(&self) -> Option<u64> {
        let revision = self.live_model().revision;
        self.banner.transcript_tick(revision)
    }
}

/// Puts the banner over the header in part 0 of the launch session card's `parts`, if this is
/// the process's first session header and the switch says so. Then, banner or not, the folder
/// codex-alab trusted at launch, if it did (`ALAB(trust)`, `codex_alab_tui::trust`), as one info
/// line under the header: the person sees which folder was trusted under which aterm root.
pub(super) fn prepend(parts: &mut Vec<Box<dyn HistoryCell>>, settings: &LocalSettings) {
    prepend_when(
        parts,
        settings.tui.animations && settings.tui.effects.welcome,
        codex_alab_tui::session::version(),
        codex_alab_tui::banner::stamped_upstream(),
    );
    if let Some(notice) = codex_alab_tui::trust::take_notice() {
        parts.push(Box::new(crate::history_cell::new_info_event(
            notice.message,
            Some(notice.hint),
        )));
    }
}

/// [`prepend`] with upstream's welcome predicate (`tui.animations && tui.effects.welcome`)
/// already evaluated, and the upstream source the banner names.
fn prepend_when(
    parts: &mut Vec<Box<dyn HistoryCell>>,
    welcome: bool,
    product_version: &str,
    upstream: Option<String>,
) {
    let (suppressed, env) = switch();
    // The slot goes to the first header whatever the switch says, so a later one (after `/new`
    // with animations newly on) cannot draw a launch banner mid-session.
    if suppressed || !claim_launch_slot() {
        return;
    }
    let Some(style) = codex_alab_tui::banner::decide(env.as_deref(), welcome, suppressed) else {
        return;
    };
    if parts.is_empty() {
        return;
    }
    let (depth, theme) = terminal_colors();
    let banner = Banner::new(style, depth, theme, product_version)
        .with_upstream(upstream)
        .with_claims(codex_alab_tui::banner::claims::CLAIMS);
    let header = parts.remove(0);
    parts.insert(
        0,
        Box::new(BannerCell {
            banner,
            model: Default::default(),
            cache: Mutex::new(None),
            header,
        }),
    );
    note_drawn();
}

/// `(suppressed, ALAB_BANNER)`: suppressed outside codex-alab. The name sits outside the
/// `CODEX_*` prefix aterm strips from the shells it spawns.
#[cfg(not(test))]
fn switch() -> (bool, Option<String>) {
    (
        !codex_alab_tui::banner::active(),
        std::env::var("ALAB_BANNER").ok(),
    )
}

/// Under codex-tui's unit tests the banner is off unless a test here forces it, so upstream's
/// snapshots never see it; a forced test reads its switch from the force, not the environment.
#[cfg(test)]
fn switch() -> (bool, Option<String>) {
    match tests::forced() {
        Some(env) => (false, env.map(str::to_owned)),
        None => (true, None),
    }
}

#[cfg(not(test))]
fn claim_launch_slot() -> bool {
    codex_alab_tui::banner::claim_launch_slot()
}

/// A forced test has its own slot, so tests on other threads cannot take it.
#[cfg(test)]
fn claim_launch_slot() -> bool {
    tests::claim_slot()
}

/// Tells the blossom's placement the banner is on screen. Not under codex-tui's tests: the
/// record is process-wide and upstream's blossom tests share the process.
#[cfg(not(test))]
fn note_drawn() {
    codex_alab_tui::banner::note_drawn();
}

#[cfg(test)]
fn note_drawn() {}

#[cfg(test)]
mod tests {
    use super::BannerCell;
    use crate::history_cell::CompositeHistoryCell;
    use crate::history_cell::HistoryCell;
    use crate::history_cell::HistoryRenderMode;
    use crate::history_cell::PlainHistoryCell;
    use crate::history_cell::SessionHeaderHistoryCell;
    use crate::history_cell::session::SessionInfoCell;
    use pretty_assertions::assert_eq;
    use ratatui::text::Line;
    use std::cell::Cell;

    thread_local! {
        static FORCE: Cell<Option<Option<&'static str>>> = const { Cell::new(None) };
        pub(super) static WINDOWS_LAUNCH: Cell<bool> = const { Cell::new(cfg!(windows)) };
        static CLAIMED: Cell<bool> = const { Cell::new(false) };
    }

    /// `Some(env)` inside [`forced`]: the banner may draw, with `env` as `ALAB_BANNER`.
    pub(super) fn forced() -> Option<Option<&'static str>> {
        FORCE.get()
    }

    pub(super) fn claim_slot() -> bool {
        !CLAIMED.replace(true)
    }

    /// Runs `f` with the banner allowed on this thread and a fresh launch slot.
    fn force<T>(env: Option<&'static str>, f: impl FnOnce() -> T) -> T {
        FORCE.set(Some(env));
        CLAIMED.set(false);
        let out = f();
        FORCE.set(None);
        out
    }

    fn with_startup_palette<T>(windows: bool, run: impl FnOnce() -> T) -> T {
        let previous = WINDOWS_LAUNCH.replace(windows);
        let result = run();
        WINDOWS_LAUNCH.set(previous);
        result
    }

    #[cfg(unix)]
    impl SessionInfoCell {
        pub(crate) fn with_test_windows_launch<T>(run: impl FnOnce() -> T) -> T {
            with_startup_palette(true, run)
        }

        pub(crate) fn with_test_launch_card(f: impl FnOnce()) {
            force(Some("codex"), f);
        }
    }

    fn header() -> SessionHeaderHistoryCell {
        SessionHeaderHistoryCell::new(
            "gpt-5".to_string(),
            /*reasoning_effort*/ None,
            std::path::PathBuf::from("/tmp/project"),
            "0.0.0",
        )
    }

    /// The upstream source of a packaged build.
    fn upstream() -> Option<String> {
        Some("0.163.0+ge8aa7c1678ca".to_owned())
    }

    fn prepend_when(
        parts: &mut Vec<Box<dyn HistoryCell>>,
        welcome: bool,
        upstream: Option<String>,
    ) {
        super::prepend_when(parts, welcome, "0.164.0-dev.12814+ga180bdb0c", upstream);
    }

    /// The parts `new_session_info` starts from, with the hook applied.
    fn launch_parts(welcome: bool) -> Vec<Box<dyn HistoryCell>> {
        let mut parts: Vec<Box<dyn HistoryCell>> = vec![Box::new(header())];
        prepend_when(&mut parts, welcome, upstream());
        parts
    }

    /// The banner cell at part 0, if the hook drew one.
    fn banner(parts: &[Box<dyn HistoryCell>]) -> Option<&BannerCell> {
        parts.first()?.as_any().downcast_ref::<BannerCell>()
    }

    fn text(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn plain_header_recovers_the_full_directory_or_checkout_in_a_split_pane() {
        use crate::line_truncation::line_width;
        use ratatui::style::Style;
        use ratatui::style::Stylize;
        use ratatui::text::Span;
        use std::path::MAIN_SEPARATOR as SEP;
        use std::path::PathBuf;

        let state = Box::leak(Box::new(codex_alab_tui::interface::State::new()));
        let _scope = codex_alab_tui::interface::scoped(state);
        codex_alab_tui::interface::set_enabled(true);
        let home = dirs::home_dir().unwrap();
        let name = "service-orchestration";
        let header_for = |path| SessionHeaderHistoryCell::new("gpt-5".into(), None, path, "test");
        let row = |header: &SessionHeaderHistoryCell, width: u16| {
            crate::terminal_palette::with_test_default_colors(None, || {
                with_startup_palette(false, || header.display_lines(width).pop().unwrap())
            })
        };
        let close = header_for(home.join(name));
        let full = format!("~{SEP}{name}");
        for width in 23..=29 {
            let indent = " ".repeat(usize::from(width - 23).min(5));
            let expected = Line::from(vec![Span::raw(indent), full.as_str().dim()]);
            assert_eq!(row(&close, width), expected, "{width} columns");
        }
        assert_eq!(
            close.raw_lines().last().unwrap().to_string(),
            format!("directory: {full}")
        );
        let deep = header_for(
            home.join("work")
                .join("clients")
                .join("platform")
                .join(name),
        );
        for (width, expected) in [
            (21, name.to_owned()),
            (22, format!("…{name}")),
            (23, format!("…{SEP}{name}")),
            (24, format!(" …{SEP}{name}")),
        ] {
            let line = row(&deep, width);
            assert_eq!(line.to_string(), expected);
            assert_eq!(line.spans.last().unwrap().style, Style::default().dim());
        }
        let native = |header: &SessionHeaderHistoryCell, width: u16| {
            format!(
                "     {}",
                header.format_directory(Some(usize::from(width) - 5))
            )
        };
        for width in [18, 28, 80] {
            assert_eq!(row(&deep, width).to_string(), native(&deep, width));
        }

        // Use the platform separator, including a trailing one outside a home-relative path.
        let path = PathBuf::from(format!(
            "C:{SEP}work{SEP}clients{SEP}platform{SEP}سلامｶﾞ👩🏽‍💻{SEP}"
        ));
        let directory = header_for(path);
        let tail = format!("سلامｶﾞ👩🏽‍💻{SEP}");
        let fit = line_width(&Line::from(tail.as_str())) as u16;
        assert_eq!(row(&directory, fit).to_string(), tail);

        codex_alab_tui::interface::set_enabled(false);
        assert_eq!(row(&close, 23).to_string(), native(&close, 23));
        assert_eq!(row(&deep, 24).to_string(), native(&deep, 24));
    }

    #[test]
    fn plain_session_details_use_readable_ink_on_light_backgrounds() {
        use crate::terminal_palette;
        use crate::terminal_probe::DefaultColors;
        use codex_protocol::openai_models::ReasoningEffort;
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;
        use ratatui::style::Color;
        use ratatui::style::Modifier;
        use ratatui::style::Style;
        use ratatui::widgets::Widget;

        let state = Box::leak(Box::new(codex_alab_tui::interface::State::new()));
        let _scope = codex_alab_tui::interface::scoped(state);
        let mut header = header().with_yolo_mode(true);
        header.reasoning_effort = Some(ReasoningEffort::High);
        let white = Some((255, 255, 255));
        for (enhanced, bg, windows) in [
            (true, white, true),
            (true, Some((16, 20, 24)), true),
            (true, None, true),
            (true, None, false),
            (false, white, true),
            (false, None, true),
        ] {
            codex_alab_tui::interface::set_enabled(enhanced);
            let colors = bg.map(|bg| DefaultColors {
                fg: (32, 33, 36),
                bg,
            });
            let rows = terminal_palette::with_test_default_colors(colors, || {
                with_startup_palette(windows, || header.display_lines(80))
            });
            let light = enhanced && bg == white;
            let unreported = enhanced && windows && bg.is_none();
            let expected_ink = if light {
                terminal_palette::rgb_color((115, 117, 137))
            } else if unreported {
                Color::Reset
            } else {
                Color::Green
            };
            let ambient = if light || unreported {
                Modifier::ITALIC | Modifier::DIM
            } else {
                Modifier::ITALIC
            };
            for needle in ["project", "permissions:", "(high)"] {
                if !enhanced && needle == "(high)" {
                    continue;
                }
                let (row, x) = rows
                    .iter()
                    .find_map(|row| {
                        let text = row.to_string();
                        let (prefix, _) = text.split_once(needle)?;
                        Some((row, Line::from(prefix).width() as u16))
                    })
                    .expect("visible session detail");
                let area = Rect::new(0, 0, 80, 1);
                let mut buf = Buffer::empty(area);
                buf.set_style(
                    area,
                    Style::default().fg(Color::Green).add_modifier(ambient),
                );
                row.render(area, &mut buf);
                for (i, ch) in needle.chars().enumerate() {
                    let cell = &buf[(x + i as u16, 0)];
                    assert_eq!(cell.symbol(), ch.to_string());
                    assert_eq!(cell.fg, expected_ink, "{needle} {bg:?}");
                    assert!(cell.modifier.contains(Modifier::ITALIC));
                    assert_eq!(
                        cell.modifier.contains(Modifier::DIM),
                        !(light || unreported),
                        "{needle} {bg:?} windows={windows}"
                    );
                }
            }
        }
    }

    #[test]
    fn plain_header_keeps_the_active_permission_visible_across_narrow_widths() {
        use crate::terminal_palette;
        use crate::terminal_probe::DefaultColors;
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;
        use ratatui::style::Color;
        use ratatui::style::Modifier;
        use ratatui::widgets::Widget;

        let state = Box::leak(Box::new(codex_alab_tui::interface::State::new()));
        let _scope = codex_alab_tui::interface::scoped(state);
        codex_alab_tui::interface::set_enabled(true);
        let header = header().with_yolo_mode(true);
        for width in 9..=25 {
            let line = terminal_palette::with_test_default_colors(
                DefaultColors {
                    fg: (32, 33, 36),
                    bg: (255, 255, 255),
                },
                || header.display_lines(width).pop().unwrap(),
            );
            let caption = match width {
                9 => "",
                10 => " ",
                11..=21 => "  ",
                22 => "permissions: ",
                23 => " permissions: ",
                _ => "  permissions: ",
            };
            assert_eq!(line.to_string(), format!("{caption}YOLO mode"), "{width}");
            let area = Rect::new(0, 0, width, 1);
            let mut buf = Buffer::empty(area);
            line.render(area, &mut buf);
            for (i, ch) in "YOLO mode".chars().enumerate() {
                let cell = &buf[((caption.len() + i) as u16, 0)];
                assert_eq!(cell.symbol(), ch.to_string());
                assert_eq!(cell.fg, terminal_palette::rgb_color((205, 0, 205)));
                assert!(cell.modifier.contains(Modifier::BOLD));
            }
        }
        assert_eq!(
            header.raw_lines().last().unwrap().to_string(),
            "permissions: YOLO mode"
        );
        codex_alab_tui::interface::set_enabled(false);
        assert_eq!(
            header.display_lines(16).last().unwrap().to_string(),
            "  permissions: …"
        );
        let vanilla = header.display_lines(80).pop().unwrap();
        assert_eq!(vanilla.to_string(), "  permissions: YOLO mode");
        let permission = vanilla.spans.last().unwrap();
        assert_eq!(permission.style.fg, Some(Color::Magenta));
        assert_eq!(permission.style.add_modifier, Modifier::BOLD);
    }

    #[test]
    fn cached_launch_card_uses_the_light_palette_when_the_startup_probe_arrives() {
        use crate::terminal_palette;
        use crate::terminal_probe::DefaultColors;
        use codex_alab_tui::interface;
        use codex_alab_tui::session::Shared;
        use ratatui::style::Style;

        let _mode = codex_alab_tui::boxes::scoped(Box::leak(Box::new(Shared::new())));
        let _edition = interface::scoped(Box::leak(Box::new(interface::State::new())));
        interface::set_enabled(true);
        let model_caption = |rows: &[Line<'_>]| {
            rows.iter()
                .flat_map(|line| &line.spans)
                .find(|span| span.content.trim_end() == "Model")
                .expect("illustrated model caption")
                .style
        };
        let prime = || {
            terminal_palette::with_test_default_colors(None, || {
                // Exercise the actual Windows startup foreground before the palette arrives.
                let parts = with_startup_palette(true, || {
                    force(Some("codex"), || launch_parts(/*welcome*/ true))
                });
                let before = parts[0].display_lines(80);
                let tick = parts[0].transcript_animation_tick();
                (parts, before, tick)
            })
        };
        let (card, before, initial_tick) = prime();
        let (redraw_first, _, _) = prime();
        assert_eq!(
            model_caption(&before),
            Style::default().fg(ratatui::style::Color::Reset).not_dim()
        );
        let cell = &card[0];
        let raw = cell.raw_lines();
        let white = DefaultColors {
            fg: (32, 33, 36),
            bg: (255, 255, 255),
        };
        terminal_palette::with_test_default_colors(white, || {
            let light = Style::default()
                .fg(terminal_palette::rgb_color((115, 117, 137)))
                .not_dim();
            assert_eq!(model_caption(&redraw_first[0].display_lines(80)), light);
            let resolved_tick = cell.transcript_animation_tick();
            assert_ne!(resolved_tick, initial_tick);
            for width in [80, 65, 80] {
                let rows = cell.display_lines(width);
                assert_eq!(model_caption(&rows), light);
                assert_eq!(cell.transcript_lines(width), rows);
                assert_eq!(cell.transcript_animation_tick(), resolved_tick);
                if width == 80 {
                    assert_eq!(text(&rows), text(&before));
                }
            }
            assert_eq!(cell.raw_lines(), raw);
        });
    }

    #[test]
    fn plain_and_illustrated_launches_share_permission_ink_on_the_reported_background() {
        use crate::terminal_palette;
        use crate::terminal_probe::DefaultColors;

        let state = Box::leak(Box::new(codex_alab_tui::interface::State::new()));
        let _scope = codex_alab_tui::interface::scoped(state);
        codex_alab_tui::interface::set_enabled(true);
        let permission = |welcome| {
            force(None, || {
                let mut parts: Vec<Box<dyn HistoryCell>> =
                    vec![Box::new(header().with_yolo_mode(true))];
                prepend_when(&mut parts, welcome, upstream());
                parts[0]
                    .display_lines(80)
                    .into_iter()
                    .flat_map(|line| line.spans)
                    .find(|span| span.content == "YOLO mode")
                    .unwrap()
                    .style
            })
        };
        for bg in [
            Some((16, 20, 24)),
            Some((120, 120, 120)),
            Some((255, 255, 255)),
            None,
        ] {
            let colors = bg.map(|bg| DefaultColors {
                fg: (32, 33, 36),
                bg,
            });
            terminal_palette::with_test_default_colors(colors, || {
                assert_eq!(permission(false), permission(true), "{bg:?}");
            });
        }
    }

    #[test]
    fn enhanced_headers_agree_and_vanilla_restores_the_native_header() {
        let header = header();
        let native = header.raw_lines();
        assert_eq!(native[0].to_string(), "OpenAI Codex (v0.0.0)");
        {
            let state = Box::leak(Box::new(codex_alab_tui::interface::State::new()));
            let _scope = codex_alab_tui::interface::scoped(state);
            for enabled in [true, false] {
                codex_alab_tui::interface::set_enabled(enabled);
                let raw = header.display_lines_for_mode(160, HistoryRenderMode::Raw);
                if !enabled {
                    assert_eq!(raw, native);
                    continue;
                }
                let title = raw[0].to_string();
                assert!(title.starts_with("Codex ALab  "));
                assert!(title.contains(" on Codex "));
                assert!(!title.contains("+g"));
                assert!(
                    text(&header.display_lines(160))
                        .iter()
                        .any(|line| line.trim() == title),
                    "raw and rendered headers must identify the same running sources",
                );
                assert_eq!(raw[1..], native[1..]);
            }
        }
        assert_eq!(header.raw_lines(), native);
    }

    #[test]
    fn reduced_motion_launch_reflows_only_the_visible_enhanced_title() {
        let parts = force(None, || {
            let mut parts: Vec<Box<dyn HistoryCell>> =
                vec![Box::new(header().with_yolo_mode(true))];
            prepend_when(&mut parts, /*welcome*/ false, upstream());
            parts
        });
        assert!(banner(&parts).is_none());
        let header = &parts[0];
        let vanilla = header.display_lines(28);
        let state = Box::leak(Box::new(codex_alab_tui::interface::State::new()));
        let _scope = codex_alab_tui::interface::scoped(state);
        codex_alab_tui::interface::set_enabled(true);
        let raw = header.raw_lines();
        let title = raw[0].to_string();
        let pair = title.strip_prefix("Codex ALab  ").expect("release pair");
        let wide = header.display_lines(120);
        assert_eq!(wide[1].to_string(), format!("  {title}"));
        let details = &wide[2..];
        assert!(text(details).iter().any(|line| line.contains("YOLO mode")));
        let first_width = pair.split_whitespace().map(str::len).max().unwrap();
        for width in 0..=wide[1].width() + 1 {
            let rows = header.display_lines(width as u16);
            assert!(rows.iter().all(|row| row.width() <= width));
            assert_eq!(rows[0], Line::default());
            let details_start = rows.len() - details.len();
            if details.iter().all(|row| row.width() <= width) {
                assert_eq!(&rows[details_start..], details);
            }
            let release_start = if (5..10).contains(&width) {
                let indent = if width >= 7 { "  " } else { "" };
                assert_eq!(rows[1].to_string(), format!("{indent}Codex"));
                assert_eq!(rows[2].to_string(), format!("{indent}ALab"));
                3
            } else {
                2
            };
            if width >= wide[1].width() {
                assert_eq!(rows[1], wide[1]);
            } else if width >= first_width {
                let release = text(&rows[release_start..details_start]);
                assert_eq!(
                    release
                        .iter()
                        .map(|row| row.trim())
                        .collect::<Vec<_>>()
                        .join(" "),
                    pair
                );
                assert_eq!(release.len() == 1, width >= pair.len());
            }
            assert_eq!(
                header.display_lines_for_mode(width as u16, HistoryRenderMode::Raw),
                raw
            );
        }
        codex_alab_tui::interface::set_enabled(false);
        assert_eq!(header.display_lines(28), vanilla);
    }

    #[test]
    fn default_card_contains_one_title_and_all_session_details() {
        let parts = force(None, || launch_parts(/*welcome*/ true));
        let cell = banner(&parts).expect("default banner");
        for width in [20, 40, 80, 120] {
            let rows = text(&cell.display_lines(width));
            let rendered = rows.join("\n");
            assert_eq!(rendered.matches("Codex ALab").count(), 1, "{rendered}");
            assert!(rendered.contains("gpt-5"), "{rendered}");
            assert!(rendered.contains("/tmp/project"), "{rendered}");
            assert!(rows.iter().all(
                |row| unicode_width::UnicodeWidthStr::width(row.as_str()) < usize::from(width)
            ));
            assert_eq!(cell.raw_lines(), cell.header.raw_lines());
        }
    }

    #[test]
    fn illustrated_card_keeps_the_checkout_name_when_the_directory_caption_crowds_it() {
        let checkout = "codex-ui-checkout-release-2026";
        let path = format!("/src/work/clients/acme/lab/{checkout}");
        let header = SessionHeaderHistoryCell::new(
            "gpt-5".into(),
            /*reasoning_effort*/ None,
            path.clone().into(),
            "0.0.0",
        )
        .with_yolo_mode(true);
        let mut parts: Vec<Box<dyn HistoryCell>> = vec![Box::new(header)];
        force(None, || {
            prepend_when(&mut parts, /*welcome*/ true, upstream())
        });
        let cell = banner(&parts).expect("default banner");

        // The 57-cell path is still one cell too wide below the kitty. Its basename fits beside it.
        let narrow = text(&cell.display_lines(75));
        assert!(
            narrow.iter().any(|row| {
                row.contains(checkout) && row.contains('…') && !row.contains("Directory")
            }),
            "{narrow:?}"
        );

        let complete = cell.display_lines(76);
        assert!(text(&complete).windows(2).any(|rows| {
            rows[0].contains("Access    YOLO mode") && rows[1].contains(path.as_str())
        }));
        assert_eq!(cell.transcript_lines(76), complete);
        // At 100 columns the path alone fits beside the kitty. At 110 its caption fits too.
        let captioned = format!("Directory {path}");
        for width in [99, 100, 109, 110, 120] {
            let display = cell.display_lines(width);
            let (directory, access) = if width < 110 { (1, 0) } else { (0, 1) };
            assert!(
                text(&display).windows(2).any(|rows| {
                    rows[directory].contains(&captioned)
                        && rows[access].contains("Access    YOLO mode")
                }),
                "{width}: {display:?}"
            );
            assert_eq!(cell.transcript_lines(width), display);
        }
        // With exactly 30 cells, the transcript gives the whole row to the checkout.
        let exact = cell.transcript_lines(73);
        assert!(
            exact
                .iter()
                .any(|line| line.spans.iter().any(|span| span.content == checkout))
        );
        assert_eq!(cell.display_lines(73), exact);
        let below_kitty = cell.display_lines(69);
        assert!(
            text(&below_kitty).windows(2).any(|rows| {
                rows[0].contains("Access    YOLO mode") && rows[1].contains(checkout)
            })
        );
        assert_eq!(cell.transcript_lines(69), below_kitty);
        assert_eq!(text(&cell.display_lines(75)), narrow);
    }

    #[test]
    fn unified_views_read_facts_once_and_only_format_a_visible_fallback() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering::Relaxed;

        #[derive(Debug, Default)]
        struct CountedHeader {
            raw: AtomicUsize,
            formatted: AtomicUsize,
        }

        impl HistoryCell for CountedHeader {
            fn raw_lines(&self) -> Vec<Line<'static>> {
                self.raw.fetch_add(1, Relaxed);
                vec![Line::from("title"), Line::from("model: counted-model")]
            }

            fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
                self.formatted.fetch_add(1, Relaxed);
                vec![Line::from("formatted header")]
            }
        }

        let state = Box::leak(Box::new(codex_alab_tui::interface::State::new()));
        let _scope = codex_alab_tui::interface::scoped(state);
        codex_alab_tui::interface::set_enabled(true);
        let mut parts: Vec<Box<dyn HistoryCell>> = vec![Box::<CountedHeader>::default()];
        force(None, || prepend_when(&mut parts, true, upstream()));
        let cell = banner(&parts).expect("default banner");
        let header = cell
            .header
            .as_any()
            .downcast_ref::<CountedHeader>()
            .unwrap();
        let lines = cell.display_lines(80);
        assert!(text(&lines).join(" ").contains("counted-model"));
        for len in [
            cell.display_hyperlink_lines(80).len(),
            cell.compact_hyperlink_lines(80).len(),
            cell.transcript_lines(80).len(),
            cell.transcript_hyperlink_lines(80).len(),
        ] {
            assert_eq!(len, lines.len());
        }
        assert_eq!(header.raw.load(Relaxed), 1);
        assert_eq!(header.formatted.load(Relaxed), 0);

        assert_eq!(cell.display_lines(12), vec![Line::from("formatted header")]);
        codex_alab_tui::interface::set_enabled(false);
        assert_eq!(cell.display_lines(80), vec![Line::from("formatted header")]);
        assert_eq!(header.formatted.load(Relaxed), 2);
    }

    #[test]
    fn scheduled_cruise_tracks_the_live_host_and_local_date_across_history_views() {
        use chrono::TimeZone;
        use codex_alab_tui::fixture_clock;
        use codex_alab_tui::session::Autopilot;
        use codex_alab_tui::session::HarnessWord;
        use codex_alab_tui::session::Shared;

        let shared = Box::leak(Box::new(Shared::new()));
        let _scope = codex_alab_tui::boxes::scoped(shared);
        let local = |day, hour, minute| {
            chrono::Local
                .with_ymd_and_hms(2026, 11, day, hour, minute, 0)
                .single()
                .unwrap()
        };
        let unix = |day, hour, minute| u64::try_from(local(day, hour, minute).timestamp()).unwrap();
        let _today = fixture_clock::scoped(local(19, 12, 0).to_utc());
        let parts = force(None, || launch_parts(true));
        let cell = banner(&parts).expect("default Codex banner");
        let raw = cell.raw_lines();
        cell.model.lock().unwrap().update("GPT-6".into());
        let status = || {
            let display = cell.display_lines(80);
            assert_eq!(display, cell.transcript_lines(80));
            assert_eq!(cell.raw_lines(), raw);
            assert_eq!(cell.live_raw_lines(), raw);
            assert_eq!(cell.copy_source(), cell.header.copy_source());
            assert_eq!(cell.display_lines_for_mode(80, HistoryRenderMode::Raw), raw);
            text(&display).join("\n")
        };
        shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Cruise);
        let ordinary = cell.transcript_animation_tick();
        assert!(status().contains("∞ cruise"));
        shared.set_land_at(Some(unix(19, 18, 0)));
        let same_day = cell.transcript_animation_tick();
        assert_ne!(same_day, ordinary);
        assert!(status().contains("∞ cruise ⤓18:00"));
        shared.set_land_at(Some(unix(20, 6, 0)));
        let tomorrow = cell.transcript_animation_tick();
        assert_ne!(tomorrow, same_day);
        assert!(status().contains("∞ cruise ⤓Fri 06:00"));
        let _midnight = fixture_clock::scoped(local(20, 0, 0).to_utc());
        assert_ne!(cell.transcript_animation_tick(), tomorrow);
        assert!(status().contains("∞ cruise ⤓06:00"));
        shared.set_autopilot_with_word(Autopilot::Passive, HarnessWord::Cruise);
        assert!(!status().contains("⤓06:00"));
        shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
        assert!(status().contains("⤓ land"));
        shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Cruise);
        assert!(status().contains("∞ cruise ⤓06:00"));
        shared.set_land_at(None);
        assert_eq!(cell.transcript_animation_tick(), ordinary);
        assert!(!status().contains("⤓06:00"));
    }

    #[test]
    fn model_and_effort_changes_redraw_a_cached_card_at_the_same_width() {
        let parts = force(None, || launch_parts(true));
        let cell = banner(&parts).expect("default banner");
        for label in [
            "gpt-6.1-sol ultra",
            "gpt-6.2-astra ultra",
            "gpt-6.2-astra high",
        ] {
            let tick = cell.transcript_animation_tick();
            cell.model.lock().unwrap().update(label.to_owned());
            assert_ne!(cell.transcript_animation_tick(), tick);
            assert!(text(&cell.display_lines(100)).join(" ").contains(label));
        }
    }

    #[test]
    fn resizing_one_card_restores_the_text_kitty_beside_its_release() {
        use codex_alab_tui::banner::Banner;
        use codex_alab_tui::banner::Depth;
        use codex_alab_tui::banner::Style;
        use codex_alab_tui::banner::Theme;
        use codex_alab_tui::session::Autopilot;
        use codex_alab_tui::session::HarnessWord;
        use codex_alab_tui::session::Shared;

        let shared = Box::leak(Box::new(Shared::new()));
        let _host = codex_alab_tui::boxes::scoped(shared);
        let _interface = codex_alab_tui::interface::scoped(Box::leak(Box::new(
            codex_alab_tui::interface::State::new(),
        )));
        codex_alab_tui::interface::configure(true);
        shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
        let raw = vec![
            Line::from("Codex ALab  0.11 on Codex 0.169"),
            Line::from("model: gpt-6.1"),
            Line::from("directory: ~/codex-alab"),
        ];
        let cell = BannerCell {
            banner: Banner::new(Style::Codex, Depth::Mono, Theme::default(), "0.11.0")
                .with_upstream(Some("0.169.0".into())),
            header: Box::new(PlainHistoryCell::new(raw.clone())),
            model: Default::default(),
            cache: Default::default(),
        };
        let compact = "▂▃▄▅▆▇(•▾•)";
        let full = "(  • •  )";
        for (width, kitty) in [
            (50, ""),
            (51, compact),
            (65, full),
            (64, compact),
            (50, ""),
            (51, compact),
        ] {
            let lines = cell.display_lines(width);
            let rows = text(&lines);
            let (_, after_release) = rows[2]
                .strip_suffix('│')
                .expect("right border")
                .split_once("0.11 on Codex 0.169")
                .expect("release row");
            assert_eq!(after_release.trim(), kitty, "{width}: {rows:?}");
            assert!(
                rows.iter().any(|row| row.contains("Autopilot ⤓ land")),
                "{width}: {rows:?}"
            );
            assert!(
                lines
                    .iter()
                    .all(|line| line.width() == usize::from(width) - 1)
            );
            assert_eq!(cell.transcript_lines(width), lines);
            assert_eq!(cell.raw_lines(), raw);
        }
    }

    #[test]
    fn resizing_the_live_card_keeps_the_model_tier_and_effort_in_the_transcript() {
        let parts = force(None, || launch_parts(true));
        let cell = banner(&parts).expect("default banner");
        let original = cell.raw_lines();
        let label = "GPT-6.2-Astra · Ultrafast xhigh";
        cell.model.lock().unwrap().update(label.into());
        for width in [68, 69, 70, 74, 14, 15, 69, 68] {
            let lines = cell.display_lines(width);
            let rows = text(&lines);
            assert!(
                rows.iter().any(|row| row.contains("GPT-6.2-Astra")),
                "{width}: {rows:?}"
            );
            if width <= 15 {
                assert!(
                    rows.windows(3)
                        .any(|model| model == ["GPT-6.2-Astra", "· Ultrafast", "xhigh"]),
                    "{width}: {rows:?}"
                );
            } else {
                assert!(
                    rows.iter().any(|row| row.contains("· Ultrafast xhigh")),
                    "{width}: {rows:?}"
                );
            }
            assert_eq!(cell.transcript_lines(width), lines);
            assert_eq!(cell.raw_lines(), original);
        }
    }

    #[test]
    fn forced_banner_leads_the_header() {
        let parts = force(None, || launch_parts(/*welcome*/ true));
        assert_eq!(parts.len(), 1);
        let cell = banner(&parts).unwrap_or_else(|| panic!("no banner in {parts:?}"));
        assert!(cell.header.as_any().is::<SessionHeaderHistoryCell>());
    }

    /// The negative control: without the force (upstream's own tests) nothing is inserted, even
    /// when the welcome predicate holds.
    #[test]
    fn unforced_is_a_noop() {
        let parts = launch_parts(/*welcome*/ true);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].as_any().is::<SessionHeaderHistoryCell>());
    }

    #[test]
    fn the_switch_reaches_the_cell() {
        let drew = |env, welcome| banner(&force(env, || launch_parts(welcome))).is_some();
        assert!(!drew(None, /*welcome*/ false));
        assert!(!drew(Some("off"), /*welcome*/ true));
        assert!(drew(Some("stripes"), /*welcome*/ false));
        assert!(drew(None, /*welcome*/ true));
    }

    #[test]
    fn once_per_process() {
        force(None, || {
            let mut parts: Vec<Box<dyn HistoryCell>> = vec![Box::new(header())];
            prepend_when(&mut parts, /*welcome*/ true, upstream());
            prepend_when(&mut parts, /*welcome*/ true, upstream());
            let later = launch_parts(/*welcome*/ true);
            let first = banner(&parts).unwrap_or_else(|| panic!("no banner in {parts:?}"));
            assert!(first.header.as_any().is::<SessionHeaderHistoryCell>());
            assert!(banner(&later).is_none());
        });
    }

    /// A first header the switch turned down still takes the slot: switching animations on and
    /// starting `/new` must not draw a launch banner mid-session.
    #[test]
    fn a_declined_launch_keeps_the_slot() {
        force(None, || {
            assert!(banner(&launch_parts(/*welcome*/ false)).is_none());
            assert!(banner(&launch_parts(/*welcome*/ true)).is_none());
        });
    }

    #[test]
    fn raw_mode_unchanged() {
        let plain = SessionInfoCell(CompositeHistoryCell::new(vec![
            Box::new(header()),
            Box::new(PlainHistoryCell {
                lines: vec![Line::from("  tip")],
            }),
        ]));
        let mut parts = force(None, || launch_parts(/*welcome*/ true));
        parts.push(Box::new(PlainHistoryCell {
            lines: vec![Line::from("  tip")],
        }));
        let with_banner = SessionInfoCell(CompositeHistoryCell::new(parts));
        for width in [20u16, 32, 80, 32, 120] {
            assert_eq!(
                with_banner.display_lines_for_mode(width, HistoryRenderMode::Raw),
                plain.display_lines_for_mode(width, HistoryRenderMode::Raw),
            );
            assert_eq!(with_banner.raw_lines(), plain.raw_lines());
            // The comparison means something: the rich view does carry the banner.
            let rich = text(&with_banner.display_lines_for_mode(width, HistoryRenderMode::Rich));
            assert!(
                rich.iter().any(|row| row.contains("Codex ALab")),
                "{rich:?}"
            );
            assert_eq!(with_banner.live_raw_lines(), plain.live_raw_lines());
        }
    }

    #[test]
    fn the_cell_follows_the_width() {
        let parts = force(Some("ribbon"), || launch_parts(/*welcome*/ true));
        let cell = banner(&parts).unwrap_or_else(|| panic!("no banner in {parts:?}"));
        for width in [0u16, 12, 13, 27, 28, 44, 80, 200, 80] {
            let lines = cell.art(width, cell.banner.live_harness());
            assert_eq!(
                text(&cell.display_lines(width)),
                text(&[lines.clone(), cell.header.display_lines(width)].concat()),
            );
            for row in text(&lines) {
                assert!(
                    unicode_width::UnicodeWidthStr::width(row.as_str()) < usize::from(width.max(1)),
                    "{width}: {row:?}"
                );
            }
            assert_eq!(lines.is_empty(), width <= 12, "{width}");
        }
    }

    #[test]
    fn narrow_launch_card_keeps_the_live_mode_and_native_header_projections() {
        use crate::history_cell::WebHyperlinkHistoryCell;
        use crate::terminal_hyperlinks::HyperlinkLine;
        use crate::terminal_hyperlinks::plain_hyperlink_lines;
        use codex_alab_tui::interface;
        use codex_alab_tui::session::Autopilot;
        use codex_alab_tui::session::HarnessWord;
        use codex_alab_tui::session::Shared;

        let shared = Box::leak(Box::new(Shared::new()));
        let _host = codex_alab_tui::boxes::scoped(shared);
        let _edition = interface::scoped(Box::leak(Box::new(interface::State::new())));
        interface::configure(true);
        // Give the native header a linked detail that would be lost by flattening its text.
        let mut link = HyperlinkLine::from("");
        link.push_span("project".into(), Some("https://example.test/project"));
        let header = CompositeHistoryCell::new(vec![
            Box::new(header()),
            Box::new(WebHyperlinkHistoryCell::new_hyperlink_lines(vec![link])),
        ]);
        let mut parts: Vec<Box<dyn HistoryCell>> = vec![Box::new(header)];
        force(Some("codex"), || prepend_when(&mut parts, true, upstream()));
        let cell = banner(&parts).expect("Codex launch card");
        let raw = cell.header.raw_lines();
        let live_raw = cell.header.live_raw_lines();

        for (policy, word, label) in [
            (Autopilot::Active, HarnessWord::Cruise, "∞ cruise"),
            (Autopilot::Active, HarnessWord::Land, "⤓ land"),
            (Autopilot::Passive, HarnessWord::Off, "○ off"),
        ] {
            shared.set_autopilot_with_word(policy, word);
            for width in [12, 13, 12] {
                let shown = cell.display_lines(width);
                assert_eq!(
                    text(&shown)
                        .iter()
                        .filter(|row| row.contains(label))
                        .count(),
                    1
                );
                assert_eq!(cell.transcript_lines(width), shown);
                let expected_links = if width < 13 {
                    let mut expected = cell.header.display_lines(width);
                    expected.push(Line::from(label));
                    assert_eq!(shown, expected);
                    let mut native = cell.header.display_hyperlink_lines(width);
                    assert!(native.iter().any(|row| !row.hyperlinks.is_empty()));
                    native.push(HyperlinkLine::from(label));
                    native
                } else {
                    plain_hyperlink_lines(shown)
                };
                assert_eq!(cell.display_hyperlink_lines(width), expected_links);
                assert_eq!(cell.compact_hyperlink_lines(width), expected_links);
                assert_eq!(cell.transcript_hyperlink_lines(width), expected_links);
            }
            assert_eq!(cell.raw_lines(), raw);
            assert_eq!(cell.live_raw_lines(), live_raw);
        }
        shared.set_autopilot_with_word(Autopilot::Passive, HarnessWord::Unknown);
        assert_eq!(cell.display_lines(12), cell.header.display_lines(12));
        shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
        interface::set_enabled(false);
        assert_eq!(
            cell.display_hyperlink_lines(12),
            cell.header.display_hyperlink_lines(12)
        );
    }

    /// The launch card as a truecolor dark terminal draws it at 80 columns: banner (the wordmark,
    /// the tagline, the claims and the upstream source beside the kitty), one blank row (the
    /// header's own), then upstream's header unchanged.
    #[test]
    #[cfg_attr(
        target_os = "windows",
        ignore = "snapshot path rendering differs on Windows"
    )]
    fn launch_card_snapshot() {
        let colors = crate::terminal_probe::DefaultColors {
            fg: (220, 220, 220),
            bg: (15, 20, 37),
        };
        let card = crate::terminal_palette::with_test_default_colors(colors, || {
            SessionInfoCell(CompositeHistoryCell::new(force(Some("ribbon"), || {
                launch_parts(/*welcome*/ true)
            })))
        });
        let lines = card.display_lines(80);
        let rows = text(&lines);
        let title = rows
            .iter()
            .position(|row| row.contains(">_ OpenAI Codex"))
            .unwrap_or_else(|| panic!("no header in {rows:?}"));
        assert!(
            rows[..title].iter().any(|row| row.contains("codex-alab")),
            "{rows:?}"
        );
        assert!(
            rows[title - 2].contains("based on OpenAI Codex 0.163.0"),
            "{rows:?}"
        );
        assert!(
            rows[..title]
                .iter()
                .any(|row| row.contains("0.164.0-dev.12814"))
        );
        assert!(rows[..title].iter().all(|row| !row.contains("+g")
            && !row.contains("a180bdb0c")
            && !row.contains("e8aa7c1678ca")));
        assert!(rows[title - 1].is_empty(), "{rows:?}");
        insta::assert_snapshot!(codex_alab_tui::banner::review_text(&lines));
    }

    /// The owner's ruling of 2026-10-24: "on" is the whole harness, not Autopilot alone. With
    /// the harness on the banner stays and its row names the real Autopilot word, Off
    /// included; unsupervised, the plain banner draws (as outside aterm), never an empty one.
    /// Every change wakes the TUI and moves the tick, so the cached layout redraws.
    #[test]
    fn the_harness_row_names_its_word_and_wakes_tui_seamlessly() {
        use codex_alab_tui::session::Autopilot;
        use codex_alab_tui::session::HarnessWord;
        use codex_alab_tui::session::Shared;
        use std::sync::Arc;
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering;

        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let wakes = Arc::new(AtomicUsize::new(0));
        let wakes_cl = Arc::clone(&wakes);
        assert!(shared.set_waker(Box::new(move || {
            wakes_cl.fetch_add(1, Ordering::SeqCst);
        })));
        let _scope = codex_alab_tui::boxes::scoped(shared);

        let parts = force(Some("ribbon"), || launch_parts(/*welcome*/ true));
        let info = SessionInfoCell(CompositeHistoryCell::new(parts));
        let cell = banner(&info.0.parts).expect("banner cell");
        assert_eq!(info.transcript_animation_tick(), None);
        let plain = text(&cell.display_lines(80));
        assert!(plain.iter().any(|r| r.contains("codex-alab")), "{plain:?}");
        assert!(!plain.iter().any(|r| r.contains("harness v")), "{plain:?}");

        let row = |word: &str| format!("Autopilot {word}");
        let mut cruise = None;
        for (word, autopilot, tick, expected) in [
            (
                HarnessWord::Cruise,
                Autopilot::Active,
                2,
                Some(row("∞ cruise")),
            ),
            (HarnessWord::Land, Autopilot::Active, 3, Some(row("⤓ land"))),
            // Autopilot off with the harness on: the claim is given back, the harness row stays.
            (HarnessWord::Off, Autopilot::Passive, 4, Some(row("○ off"))),
            // Unsupervised (the claim not proven, the harness itself off): the plain banner.
            (HarnessWord::Unknown, Autopilot::Passive, 1, None),
            (
                HarnessWord::Cruise,
                Autopilot::Active,
                2,
                Some(row("∞ cruise")),
            ),
        ] {
            let before = wakes.load(Ordering::SeqCst);
            shared.set_harness_word(word);
            shared.set_autopilot(autopilot);
            assert!(
                wakes.load(Ordering::SeqCst) > before,
                "{word:?}: wakes the TUI"
            );
            assert_eq!(info.transcript_animation_tick(), Some(tick), "{word:?}");
            let rows = text(&cell.display_lines(80));
            match expected {
                Some(expected) => {
                    assert!(
                        rows.iter().any(|r| r.trim() == format!("✦ {expected}")),
                        "{word:?}: {rows:?}"
                    );
                    assert_eq!(rows.len(), plain.len() + 1, "{word:?}: {rows:?}");
                    if word == HarnessWord::Cruise {
                        assert_eq!(cruise.get_or_insert_with(|| rows.clone()), &rows);
                    }
                }
                None => assert_eq!(rows, plain, "{word:?}"),
            }
        }
    }
}

#[cfg(unix)]
#[path = "retained_history_banner_cell.rs"]
pub(in crate::history_cell) mod alab_retained_banner;
