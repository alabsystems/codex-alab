// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Launch-card integration for codex-tui's private HistoryCell trait.
//!
//! The default Codex card integrates the header's model, directory and permissions into one
//! framed banner. The existing explicit ribbon/stripes styles keep the separate header.
//! Raw scrollback, copy and export always retain the original header text. The first session
//! header consumes the launch slot even when artwork is disabled, so /new never adds art.
//! The cell tracks the live harness word and terminal width in its render cache. It remains
//! startup metadata, excluded from activity and export, and shares the existing blossom
//! placement hook. All drawing lives in codex_alab_tui::banner; this glue is ALab-owned.

use crate::history_cell::HistoryCell;
use crate::local_settings::LocalSettings;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::plain_hyperlink_lines;
use crate::terminal_palette;
use crate::terminal_palette::StdoutColorLevel;
use codex_alab_tui::banner::Banner;
use codex_alab_tui::banner::Depth;
use codex_alab_tui::banner::Harness;
use codex_alab_tui::banner::Theme;
use ratatui::text::Line;
use std::sync::Mutex;
use std::sync::PoisonError;

/// The width and harness the art was last drawn for, with what was drawn.
type ArtCache = Option<(u16, Option<Harness>, Vec<Line<'static>>)>;

/// The banner over upstream's session header, as one history cell. The art is drawn for each
/// width it is asked for (cached for the last one, since owned mode lays out again on every
/// resize) and is absent from raw, copy and export text; every view of the header is the
/// header's own. The trait's other methods keep their defaults, as the header's do.
#[derive(Debug)]
struct BannerCell {
    banner: Banner,
    cache: Mutex<ArtCache>,
    header: Box<dyn HistoryCell>,
}

impl BannerCell {
    fn art(&self, width: u16) -> Vec<Line<'static>> {
        if codex_alab_tui::interface::is_vanilla() {
            return Vec::new();
        }
        let state = codex_alab_tui::banner::harness_state();
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((cached, cached_state, lines)) = cache.as_ref()
            && *cached == width
            && *cached_state == state
        {
            return lines.clone();
        }
        // The card names the live Autopilot word; session details stay in the same frame.
        let lines = if self.banner.is_codex() {
            self.banner.session_lines(
                width,
                state,
                self.header.raw_lines().into_iter().skip(1).collect(),
            )
        } else {
            self.banner.lines_with_harness(width, state)
        };
        *cache = Some((width, state, lines.clone()));
        lines
    }

    fn art_then<T>(
        &self,
        width: u16,
        art: impl FnOnce(Vec<Line<'static>>) -> Vec<T>,
        header: Vec<T>,
    ) -> Vec<T> {
        let rendered = self.art(width);
        let unified = self.banner.is_codex() && !rendered.is_empty();
        let mut lines = art(rendered);
        if !unified {
            lines.extend(header);
        }
        lines
    }
}

impl HistoryCell for BannerCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.art_then(width, |art| art, self.header.display_lines(width))
    }

    fn display_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.art_then(
            width,
            plain_hyperlink_lines,
            self.header.display_hyperlink_lines(width),
        )
    }

    fn compact_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.art_then(
            width,
            plain_hyperlink_lines,
            self.header.compact_hyperlink_lines(width),
        )
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.art_then(width, |art| art, self.header.transcript_lines(width))
    }

    fn transcript_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.art_then(
            width,
            plain_hyperlink_lines,
            self.header.transcript_hyperlink_lines(width),
        )
    }

    /// Raw scrollback, copy and export carry no art: the header's text alone.
    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.header.raw_lines()
    }

    fn live_raw_lines(&self) -> Vec<Line<'static>> {
        self.header.live_raw_lines()
    }

    fn transcript_animation_tick(&self) -> Option<u64> {
        codex_alab_tui::banner::harness_tick()
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
fn prepend_when(parts: &mut Vec<Box<dyn HistoryCell>>, welcome: bool, upstream: Option<String>) {
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
    let depth = match terminal_palette::effective_stdout_color_level() {
        StdoutColorLevel::TrueColor => Depth::TrueColor,
        StdoutColorLevel::Ansi256 => Depth::Ansi256,
        StdoutColorLevel::Ansi16 => Depth::Ansi16,
        // NO_COLOR, TERM=dumb, not a terminal.
        StdoutColorLevel::Unknown => Depth::Mono,
    };
    let theme = Theme::from_bg(terminal_palette::default_bg());
    let banner = Banner::new(style, depth, theme, &build_version())
        .with_upstream(upstream)
        .with_claims(codex_alab_tui::banner::claims::CLAIMS);
    let header = parts.remove(0);
    parts.insert(
        0,
        Box::new(BannerCell {
            banner,
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

#[cfg(not(test))]
fn build_version() -> String {
    codex_alab_tui::session::version().to_owned()
}

#[cfg(test)]
fn build_version() -> String {
    "0.164.0-dev.12814+ga180bdb0c".to_owned()
}

#[cfg(test)]
mod tests {
    use super::BannerCell;
    use super::prepend_when;
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
                assert!(title.contains("codex-alab"));
                let alab_version = match env!("CARGO_PKG_VERSION") {
                    "0.0.0" => "dev",
                    version => version
                        .split_once('+')
                        .map_or(version, |(version, _)| version),
                };
                assert!(title.contains(alab_version));
                assert!(title.contains("OpenAI Codex"));
                assert!(title.contains("based on"));
                assert!(!title.contains("+g"));
                assert!(
                    title.contains(
                        option_env!("CODEX_ALAB_UPSTREAM_VERSION").unwrap_or("unreleased")
                    )
                );
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
    fn default_card_contains_one_title_and_all_session_details() {
        let parts = force(None, || launch_parts(/*welcome*/ true));
        let cell = banner(&parts).expect("default banner");
        for width in [20, 40, 80, 120] {
            let rows = text(&cell.display_lines(width));
            let rendered = rows.join("\n");
            assert_eq!(rendered.matches("OpenAI Codex").count(), 1, "{rendered}");
            assert!(rendered.contains("gpt-5"), "{rendered}");
            assert!(rendered.contains("/tmp/project"), "{rendered}");
            assert!(rows.iter().all(
                |row| unicode_width::UnicodeWidthStr::width(row.as_str()) < usize::from(width)
            ));
            assert_eq!(cell.raw_lines(), cell.header.raw_lines());
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
        for width in [20u16, 80, 120] {
            assert_eq!(
                with_banner.display_lines_for_mode(width, HistoryRenderMode::Raw),
                plain.display_lines_for_mode(width, HistoryRenderMode::Raw),
            );
            assert_eq!(with_banner.raw_lines(), plain.raw_lines());
            // The comparison means something: the rich view does carry the banner.
            let rich = text(&with_banner.display_lines_for_mode(width, HistoryRenderMode::Rich));
            assert!(
                rich.iter().any(|row| row.contains("codex-alab")),
                "{rich:?}"
            );
        }
    }

    #[test]
    fn the_cell_follows_the_width() {
        let parts = force(Some("ribbon"), || launch_parts(/*welcome*/ true));
        let cell = banner(&parts).unwrap_or_else(|| panic!("no banner in {parts:?}"));
        for width in [0u16, 12, 13, 27, 28, 44, 80, 200, 80] {
            let lines = cell.art(width);
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
