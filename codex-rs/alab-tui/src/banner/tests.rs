// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::Banner;
use super::Depth;
use super::LaunchSlot;
use super::Style;
use super::Theme;
use super::decide;
use super::palette;
use super::place_blossom;
use super::review_text;
use pretty_assertions::assert_eq;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

const DEPTHS: [Depth; 4] = [Depth::TrueColor, Depth::Ansi256, Depth::Ansi16, Depth::Mono];
const STYLES: [Style; 2] = [Style::Ribbon, Style::Stripes];
const COMMIT: &str = "abc1234def5678";
const DARK: (u8, u8, u8) = (15, 20, 37);
const LIGHT: (u8, u8, u8) = (255, 255, 255);
/// aterm's own dark background.
const ATERM_DARK: (u8, u8, u8) = (0x11, 0x13, 0x18);

fn themes() -> [(&'static str, Theme); 3] {
    [
        ("dark", Theme::from_bg(Some(DARK))),
        ("light", Theme::from_bg(Some(LIGHT))),
        ("unknown", Theme::from_bg(None)),
    ]
}

fn text(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Every banner the switch can produce, at every width from 0 to 240.
fn every_banner() -> impl Iterator<Item = (String, u16, Vec<Line<'static>>)> {
    STYLES.into_iter().flat_map(|style| {
        DEPTHS.into_iter().flat_map(move |depth| {
            themes().into_iter().flat_map(move |(theme_name, theme)| {
                let banner = Banner::new(style, depth, theme, COMMIT);
                (0..=240u16).map(move |w| {
                    (
                        format!("{style:?}/{depth:?}/{theme_name}"),
                        w,
                        banner.lines(w),
                    )
                })
            })
        })
    })
}

#[test]
fn every_line_fits() {
    for (case, w, lines) in every_banner() {
        for line in &lines {
            let width = UnicodeWidthStr::width(text(line).as_str());
            assert!(
                width <= usize::from(w.saturating_sub(1)),
                "{case} at {w}: a {width}-column line {:?}",
                text(line)
            );
        }
        if w <= 12 {
            assert!(lines.is_empty(), "{case} at {w} draws nothing");
        } else {
            assert!(
                lines.iter().any(|l| text(l).contains("codex-alab")),
                "{case} at {w} keeps its wordmark"
            );
        }
    }
}

/// aterm's codex screen reader (`aterm-phase/src/codex.rs`, `anchors.rs`) reads a row by its
/// first mark and by a few phrases; the banner must never look like one of codex's own rows.
#[test]
fn no_row_opens_with_a_reader_mark() {
    const MARKS: [char; 10] = ['›', '»', '•', '■', '└', '─', '╭', '❯', '⏺', '●'];
    const PHRASES: [&str; 4] = ["Tip: ", "esc to interrupt", "Worked for", "% context left"];
    for (case, w, lines) in every_banner() {
        for line in &lines {
            let row = text(line);
            if row.is_empty() {
                continue;
            }
            assert!(
                row.starts_with("  "),
                "{case} at {w}: {row:?} opens indented"
            );
            let first = row.trim_start().chars().next();
            assert!(
                first.is_none_or(|c| !MARKS.contains(&c)),
                "{case} at {w}: {row:?}"
            );
            for phrase in PHRASES {
                assert!(!row.contains(phrase), "{case} at {w}: {row:?}");
            }
        }
    }
}

#[test]
fn palette_is_aterms() {
    assert_eq!(palette::SPECTRUM_LUT.len(), palette::SPECTRUM_LUT_LEN);
    for (i, &at) in palette::SPECTRUM_ANCHOR_AT.iter().enumerate() {
        assert_eq!(
            palette::SPECTRUM_LUT[at],
            palette::SPECTRUM_ANCHORS[i],
            "stop {i}"
        );
        assert_eq!(palette::spectrum_stop(i), palette::SPECTRUM_ANCHORS[i]);
        let t = at as f32 / (palette::SPECTRUM_LUT_LEN - 1) as f32;
        assert_eq!(
            palette::spectrum(t),
            palette::SPECTRUM_ANCHORS[i],
            "stop {i}"
        );
        assert_eq!(palette::spectrum_snap_index(t), i);
    }
    assert_eq!(
        palette::SPECTRUM_ANCHORS,
        [
            0xFF0000, 0xFF7F00, 0xFFFF00, 0x00FF00, 0x0000FF, 0x4B0082, 0x9400D3
        ]
    );
    // The green→blue leg turns through cyan in HSV: no sag to a dull teal.
    let cyanish = palette::SPECTRUM_LUT[237..=404]
        .iter()
        .any(|&c| c == 0x0000_FFFE || c == 0x0000_FDFF);
    assert!(cyanish, "the crossing passes #00FFFF");
}

#[test]
fn mono_sets_no_colour() {
    for (case, w, lines) in every_banner() {
        if !case.contains("Mono") {
            continue;
        }
        for line in &lines {
            assert_eq!(line.style.fg, None);
            assert_eq!(line.style.bg, None);
            for span in &line.spans {
                assert!(
                    span.style.fg.is_none() && span.style.bg.is_none(),
                    "{case} at {w}: {span:?}"
                );
            }
        }
    }
}

/// The colour a terminal draws for an emitted truecolor or xterm-256 colour.
fn drawn(colour: Color) -> palette::Rgb {
    match colour {
        Color::Rgb(r, g, b) => palette::pack((r, g, b)),
        Color::Indexed(i) => palette::xterm256(i),
        other => panic!("not a fixed colour: {other:?}"),
    }
}

#[test]
fn light_runs_clear_contrast() {
    let bg = palette::pack(LIGHT);
    for style in STYLES {
        for depth in [Depth::TrueColor, Depth::Ansi256] {
            let banner = Banner::new(style, depth, Theme::from_bg(Some(LIGHT)), COMMIT);
            for w in [12u16, 20, 28, 44, 64, 80, 120, 240] {
                for line in banner.lines(w) {
                    for span in &line.spans {
                        for colour in [span.style.fg, span.style.bg].into_iter().flatten() {
                            let ratio = palette::contrast(drawn(colour), bg);
                            assert!(
                                ratio >= palette::LIGHT_CONTRAST_FLOOR,
                                "{style:?}/{depth:?} at {w}: {colour:?} is {ratio:.2}:1 in {:?}",
                                span.content
                            );
                        }
                    }
                }
            }
        }
    }
    // The rescue keeps a hue family: yellow becomes a dark gold, not grey.
    let gold = palette::rescue_on_light(0xFFFF00, bg);
    let (r, g, b) = palette::channels(gold);
    assert!(r > b && g > b, "{gold:06X}");
}

/// The wordmark is text: every letter clears WCAG AA against the background, dark or light,
/// where the spectrum's blue, indigo and violet stops alone are 1.4 to 2.8:1 on aterm's dark.
#[test]
fn the_wordmark_reads_on_any_ground() {
    for (name, bg) in [
        ("dark", Some(DARK)),
        ("aterm", Some(ATERM_DARK)),
        ("light", Some(LIGHT)),
        ("unknown", None),
    ] {
        let ground = palette::pack(bg.unwrap_or(DARK));
        for depth in [Depth::TrueColor, Depth::Ansi256] {
            let lines = Banner::new(Style::Ribbon, depth, Theme::from_bg(bg), COMMIT).lines(80);
            let label = lines.last().unwrap_or_else(|| panic!("no label"));
            let letters: Vec<_> = label
                .spans
                .iter()
                .filter(|span| {
                    span.style
                        .add_modifier
                        .contains(ratatui::style::Modifier::BOLD)
                })
                .collect();
            let wordmark: String = letters.iter().map(|span| span.content.as_ref()).collect();
            assert_eq!(wordmark, "codex-alab", "{name}/{depth:?}");
            for span in letters {
                let colour = span.style.fg.unwrap_or_else(|| panic!("{span:?}"));
                let ratio = palette::contrast(drawn(colour), ground);
                assert!(
                    ratio >= palette::TEXT_CONTRAST_FLOOR,
                    "{name}/{depth:?}: {:?} {colour:?} is {ratio:.2}:1",
                    span.content
                );
            }
        }
    }
    // Still the spectrum: blue stays blue, it is only lifted.
    let banner = Banner::new(
        Style::Ribbon,
        Depth::TrueColor,
        Theme::from_bg(Some(ATERM_DARK)),
        COMMIT,
    );
    let label = banner.lines(80).pop().unwrap_or_default();
    let l = label
        .spans
        .iter()
        .find(|span| span.content.contains('l'))
        .and_then(|span| span.style.fg)
        .map(drawn)
        .unwrap_or_else(|| panic!("no l in {label:?}"));
    let (r, g, b) = palette::channels(l);
    assert!(b == 0xFF && r < 0xA0 && g < 0xA0, "{l:06X}");
}

/// Sixteen colours on a light theme: no bright yellow (93) on white, and the kitty keeps its
/// inside: an ink outline, a grey coat, the dark patch between them.
#[test]
fn ansi16_light_keeps_the_kitty() {
    for style in STYLES {
        let lines =
            Banner::new(style, Depth::Ansi16, Theme::from_bg(Some(LIGHT)), COMMIT).lines(80);
        let mut fgs = Vec::new();
        for line in &lines {
            for span in &line.spans {
                for colour in [span.style.fg, span.style.bg].into_iter().flatten() {
                    assert_ne!(colour, Color::LightYellow, "{style:?}: {span:?}");
                    assert_ne!(colour, Color::Reset, "{style:?}: {span:?}");
                    fgs.push(colour);
                }
            }
        }
        for colour in [Color::Black, Color::Gray, Color::DarkGray] {
            assert!(fgs.contains(&colour), "{style:?} draws {colour:?}");
        }
    }
}

/// Stars are few and bright and sit in open sky: never on a stroke, never against one.
#[test]
fn stars_keep_clear_of_strokes() {
    const STARS: [char; 4] = ['⋆', '✦', '∙', '✧'];
    for (case, w, lines) in every_banner() {
        for line in lines.iter().take(lines.len().saturating_sub(1)) {
            let row: Vec<char> = text(line).chars().collect();
            for (x, c) in row.iter().enumerate() {
                if !STARS.contains(c) {
                    continue;
                }
                let left = x.checked_sub(1).map(|x| row[x]);
                let right = row.get(x + 1).copied();
                assert!(
                    left.is_none_or(|c| c == ' ') && right.is_none_or(|c| c == ' '),
                    "{case} at {w}: {:?}",
                    text(line)
                );
            }
        }
    }
}

#[test]
fn the_blossom_moves_into_the_room_the_banner_leaves() {
    // A 120x40 owned screen: the stage centred at row 9, the transcript ending at row 10.
    let stage = Rect::new(30, 9, 60, 21);
    let available = Rect::new(0, 10, 120, 26);
    assert_eq!(
        place_blossom(stage, available, true),
        Rect::new(30, 12, 60, 21)
    );
    // Without the banner, or where the centred stage already fits, it is upstream's stage.
    assert_eq!(place_blossom(stage, available, false), stage);
    let roomy = Rect::new(0, 5, 120, 31);
    assert_eq!(place_blossom(stage, roomy, true), stage);
    // 120x36: 20 free rows. The stage shrinks in upstream's proportion (59 columns, 20 rows, as
    // upstream sizes it on a 63-column screen) and centres in the room.
    let shorter = Rect::new(0, 10, 120, 20);
    assert_eq!(
        place_blossom(Rect::new(30, 7, 60, 21), shorter, true),
        Rect::new(30, 10, 59, 20)
    );
    // 120x30: 14 free rows, upstream's smallest stage height (42 columns, as on a 46-column
    // screen).
    assert_eq!(
        place_blossom(Rect::new(30, 4, 60, 21), Rect::new(0, 10, 120, 14), true),
        Rect::new(39, 10, 42, 14)
    );
    // Under upstream's floor, or wider than the room: unchanged, and upstream cancels it.
    let short = Rect::new(0, 10, 120, 13);
    assert_eq!(place_blossom(stage, short, true), stage);
    let narrow = Rect::new(40, 10, 60, 26);
    assert_eq!(place_blossom(stage, narrow, true), stage);
    // Every placement fits its room and never grows.
    for rows in 0..40u16 {
        let room = Rect::new(0, 10, 120, rows);
        let placed = place_blossom(stage, room, true);
        assert!(
            placed == stage || placed.intersection(room) == placed,
            "{rows}: {placed:?}"
        );
        assert!(placed.width <= stage.width && placed.height <= stage.height);
    }
}

#[test]
fn decide_precedence() {
    // 1. upstream's own unit tests: off, whatever the switch says.
    assert_eq!(decide(Some("on"), true, true), None);
    // 2. ALAB_BANNER, trimmed and any case; an explicit value beats the welcome predicate.
    for off in ["off", "0", "false", "no", " OFF ", "No"] {
        assert_eq!(decide(Some(off), true, false), None, "{off:?}");
    }
    for on in ["on", "1", "true", "yes", "ON", " yes\n"] {
        assert_eq!(
            decide(Some(on), false, false),
            Some(Style::Ribbon),
            "{on:?}"
        );
    }
    assert_eq!(decide(Some("ribbon"), false, false), Some(Style::Ribbon));
    assert_eq!(decide(Some("Stripes"), false, false), Some(Style::Stripes));
    assert_eq!(decide(Some("stripes"), true, false), Some(Style::Stripes));
    // 3. anything else is ignored: upstream's welcome predicate decides.
    for ignored in [None, Some(""), Some("nyan"), Some("2")] {
        assert_eq!(
            decide(ignored, true, false),
            Some(Style::Ribbon),
            "{ignored:?}"
        );
        assert_eq!(decide(ignored, false, false), None, "{ignored:?}");
    }
}

#[test]
fn launch_slot_is_once() {
    let slot = LaunchSlot::new();
    assert!(slot.claim());
    assert!(!slot.claim());
    assert!(!slot.claim());
    // The process's own slot (no other test here claims it): the first claim wins, once.
    assert!(super::claim_launch_slot());
    assert!(!super::claim_launch_slot());
}

#[test]
fn label_names_the_build() {
    let dark = Theme::from_bg(Some(DARK));
    let label = |commit: &str, w: u16| {
        Banner::new(Style::Ribbon, Depth::Mono, dark, commit)
            .lines(w)
            .last()
            .map(text)
            .unwrap_or_default()
    };
    assert_eq!(
        label(COMMIT, 80),
        "  codex-alab  rainbow kitty · ALab build of OpenAI Codex · gabc1234"
    );
    assert_eq!(
        label("dev", 80),
        "  codex-alab  rainbow kitty · ALab build of OpenAI Codex"
    );
    assert_eq!(
        label(COMMIT, 60),
        "  codex-alab  rainbow kitty · ALab build of OpenAI Codex"
    );
    assert_eq!(label(COMMIT, 30), "  codex-alab  rainbow kitty");
    assert_eq!(label(COMMIT, 20), "  codex-alab");
    assert_eq!(label("unknown", 80).contains(" · g"), false);
}

#[test]
fn ladder_rows() {
    let banner = Banner::new(Style::Ribbon, Depth::Mono, Theme::default(), COMMIT);
    let rows = |w: u16| banner.lines(w).len();
    assert_eq!(rows(240), 7);
    assert_eq!(rows(80), 7);
    assert_eq!(rows(28), 7);
    assert_eq!(rows(27), 2);
    assert_eq!(rows(13), 2);
    assert_eq!(rows(12), 0);
    assert_eq!(rows(0), 0);
}

#[test]
fn snapshots() {
    let dark = Theme::from_bg(Some(DARK));
    let mut cases: Vec<(Style, Depth, &str, Theme, u16)> = Vec::new();
    for style in STYLES {
        for w in [80u16, 64, 44, 28, 20, 12] {
            cases.push((style, Depth::TrueColor, "dark", dark, w));
            cases.push((style, Depth::Mono, "dark", dark, w));
        }
        cases.push((style, Depth::Ansi256, "dark", dark, 80));
        cases.push((style, Depth::Ansi16, "dark", dark, 80));
        cases.push((
            style,
            Depth::Ansi256,
            "light",
            Theme::from_bg(Some(LIGHT)),
            80,
        ));
        cases.push((
            style,
            Depth::Ansi16,
            "light",
            Theme::from_bg(Some(LIGHT)),
            80,
        ));
        cases.push((
            style,
            Depth::TrueColor,
            "light",
            Theme::from_bg(Some(LIGHT)),
            80,
        ));
    }
    for (style, depth, theme_name, theme, w) in cases {
        let lines = Banner::new(style, depth, theme, COMMIT).lines(w);
        let name = format!("{style:?}_{depth:?}_{theme_name}_w{w}").to_ascii_lowercase();
        insta::assert_snapshot!(name, review_text(&lines));
    }
}

#[test]
fn harness_banner_and_composer_border_branding() {
    let dark = Theme::from_bg(Some(DARK));
    let banner = Banner::new(Style::Ribbon, Depth::TrueColor, dark, COMMIT);
    let lines = banner.lines_with_harness(80, true);
    let last = text(lines.last().expect("harness line"));
    assert_eq!(
        last,
        "  ✦ enhanced by aterm · codex-alab harness · autopilot ∞ cruise"
    );
    assert!(last.starts_with("  "));

    let area = Rect::new(0, 0, 80, 4);
    let mut buf = ratatui::buffer::Buffer::empty(area);
    super::render_composer_border(area, &mut buf, Depth::TrueColor, dark);
    let top_row: String = (0..area.width)
        .map(|x| buf[(x, 0)].symbol().chars().next().unwrap_or(' '))
        .collect();
    assert!(top_row.starts_with("  ─"), "top row: {top_row:?}");
    assert!(
        top_row.contains("✦ alab kitty · codex-alab harness ● on"),
        "top row: {top_row:?}"
    );
}
