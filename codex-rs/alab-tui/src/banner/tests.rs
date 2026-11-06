// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::Banner;
use super::Depth;
use super::Harness;
use super::LaunchSlot;
use super::Mode;
use super::Style;
use super::Theme;
use super::claims::CLAIMS;
use super::claims::Claim;
use super::claims::Measure;
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
/// What the banner can be drawn under: no supervisor, the harness on under each word, and a
/// supervisor the harness does not supervise.
const HARNESSES: [Option<Harness>; 5] = [
    None,
    Some(Harness::On(Mode::Cruise)),
    Some(Harness::On(Mode::Land)),
    Some(Harness::On(Mode::Off)),
    Some(Harness::Unsupervised),
];
const BUILD_VERSION: &str = "0.164.0-dev.12814+ga180bdb0c";
/// `codex_alab_core::version::upstream()` for a packaged build.
const UPSTREAM: &str = "0.163.0+ge8aa7c1678ca";
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

/// The banner the glue draws for a packaged build: the upstream source and the claims.
fn packaged(style: Style, depth: Depth, theme: Theme) -> Banner {
    Banner::new(style, depth, theme, BUILD_VERSION)
        .with_upstream(Some(UPSTREAM.to_owned()))
        .with_claims(CLAIMS)
}

/// Every banner the switch can produce, packaged or bare, under every harness, at every width
/// from 0 to 240.
fn every_banner() -> impl Iterator<Item = (String, u16, Vec<Line<'static>>)> {
    STYLES.into_iter().flat_map(|style| {
        DEPTHS.into_iter().flat_map(move |depth| {
            themes().into_iter().flat_map(move |(theme_name, theme)| {
                [
                    ("packaged", packaged(style, depth, theme)),
                    ("bare", Banner::new(style, depth, theme, BUILD_VERSION)),
                ]
                .into_iter()
                .flat_map(move |(copy, banner)| {
                    HARNESSES.into_iter().flat_map(move |harness| {
                        let banner = banner.clone();
                        (0..=240u16).map(move |w| {
                            (
                                format!("{style:?}/{depth:?}/{theme_name}/{copy}/{harness:?}"),
                                w,
                                banner.lines_with_harness(w, harness),
                            )
                        })
                    })
                })
            })
        })
    })
}

#[test]
fn every_line_fits() {
    for (case, w, lines) in every_banner() {
        for line in &lines {
            let row = text(line);
            let width = UnicodeWidthStr::width(row.as_str());
            assert!(
                width <= usize::from(w.saturating_sub(1)),
                "{case} at {w}: a {width}-column line {row:?}"
            );
            assert!(
                row.is_empty() || row.starts_with("  "),
                "{case} at {w}: {row:?} opens indented"
            );
        }
        // A blank row, five beside the kitty, and the harness row while the harness is on.
        let rows = if case.contains("On(") { 7 } else { 6 };
        assert!(lines.len() <= rows, "{case} at {w}: {} rows", lines.len());
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
    for (style, harness) in STYLES
        .into_iter()
        .flat_map(|style| HARNESSES.map(|harness| (style, harness)))
    {
        for depth in [Depth::TrueColor, Depth::Ansi256] {
            let banner = packaged(style, depth, Theme::from_bg(Some(LIGHT)));
            for w in [12u16, 20, 28, 44, 64, 80, 120, 240] {
                for line in banner.lines_with_harness(w, harness) {
                    for span in &line.spans {
                        for colour in [span.style.fg, span.style.bg].into_iter().flatten() {
                            let ratio = palette::contrast(drawn(colour), bg);
                            assert!(
                                ratio >= palette::LIGHT_CONTRAST_FLOOR,
                                "{style:?}/{depth:?}/{harness:?} at {w}: {colour:?} is \
                                 {ratio:.2}:1 in {:?}",
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
        ("middle-gray", Some((120, 120, 120))),
        ("gray", Some((128, 128, 128))),
        ("light", Some(LIGHT)),
        ("unknown", None),
    ] {
        let ground = palette::pack(bg.unwrap_or(DARK));
        for (depth, harness) in [Depth::TrueColor, Depth::Ansi256]
            .into_iter()
            .flat_map(|depth| HARNESSES.map(|harness| (depth, harness)))
        {
            let lines =
                packaged(Style::Ribbon, depth, Theme::from_bg(bg)).lines_with_harness(80, harness);
            let label = lines
                .iter()
                .find(|line| text(line).contains("codex-alab"))
                .unwrap_or_else(|| panic!("no wordmark"));
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
            assert_eq!(wordmark, "codex-alab", "{name}/{depth:?}/{harness:?}");
            // The harness row's name and word are text too.
            let harness_row = lines
                .iter()
                .filter(|line| text(line).contains("Autopilot "))
                .flat_map(|line| line.spans.iter())
                .filter(|span| {
                    span.style
                        .add_modifier
                        .contains(ratatui::style::Modifier::BOLD)
                        && span.content.trim() != "✦"
                });
            for span in letters.into_iter().chain(harness_row) {
                let colour = span.style.fg.unwrap_or_else(|| panic!("{span:?}"));
                let ratio = palette::contrast(drawn(colour), ground);
                assert!(
                    ratio >= palette::TEXT_CONTRAST_FLOOR,
                    "{name}/{depth:?}/{harness:?}: {:?} {colour:?} is {ratio:.2}:1",
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
        BUILD_VERSION,
    );
    let label = banner
        .lines(80)
        .into_iter()
        .find(|line| text(line).contains("codex-alab"))
        .unwrap_or_default();
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
        let lines = packaged(style, Depth::Ansi16, Theme::from_bg(Some(LIGHT))).lines(80);
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

/// The text column of each row, art cut away: what the banner says at `w` columns.
fn copy(banner: &Banner, w: u16) -> Vec<String> {
    const ART: [char; 7] = ['▀', '▄', '█', '⋆', '✦', '∙', '✧'];
    banner
        .lines(w)
        .iter()
        .map(|line| {
            let row = text(line);
            let words = row.rfind(ART).map_or(row.as_str(), |at| &row[at..]);
            words.trim_start_matches(ART).trim().to_owned()
        })
        .filter(|words| !words.is_empty())
        .collect()
}

#[test]
fn the_copy_at_each_width() {
    let banner = packaged(Style::Ribbon, Depth::Mono, Theme::from_bg(Some(DARK)));
    let full = [
        "codex-alab  v0.164.0-dev.12814",
        "bloatware-free · nag-free · optimized for aterm",
        "Measured defaults vs Codex 0.164 · 2026-10-23",
        "6× less memory · no daemon · 1 idle socket vs 5",
        "based on OpenAI Codex 0.163.0",
    ];
    assert_eq!(copy(&banner, 120), full);
    assert_eq!(copy(&banner, 80), full);
    assert_eq!(copy(&banner, 79), full);
    // Beside the kitty with less room: the tagline in two lines, the claims shorter, and no
    // upstream line (the session header under the banner names it too).
    let shorter = [
        "codex-alab  v0.164.0-dev.12814",
        "bloatware-free · nag-free",
        "optimized for aterm",
        "Defaults vs 0.164 · 2026-10-23",
        "6× less memory · no daemon",
    ];
    assert_eq!(copy(&banner, 64), shorter);
    // The text alone, from where the tagline no longer fits beside the kitty in two lines.
    assert_eq!(copy(&banner, 40), shorter);
    assert_eq!(
        copy(&banner, 49),
        [
            "codex-alab",
            "bloatware-free · nag-free",
            "optimized for aterm",
            "based on Codex 0.163.0",
        ]
    );
    assert_eq!(
        copy(&banner, 28),
        [
            "codex-alab",
            "bloatware-free · nag-free",
            "optimized for aterm",
            "based on Codex 0.163.0",
        ]
    );
    assert_eq!(
        copy(&banner, 24),
        [
            "codex-alab",
            "bloatware-free",
            "nag-free",
            "optimized for aterm",
        ]
    );
    // The tagline whole or not at all, and the claims only with it.
    assert_eq!(copy(&banner, 20), ["codex-alab"]);
    assert_eq!(copy(&banner, 13), ["codex-alab"]);
    assert!(copy(&banner, 12).is_empty());
}

/// A bare banner says no figure and names no upstream. Its supplied ALab version stays
/// readable; unversioned builds say dev, and an unavailable version invents none.
#[test]
fn the_copy_says_only_what_it_was_given() {
    let dark = Theme::from_bg(Some(DARK));
    let bare = Banner::new(Style::Ribbon, Depth::Mono, dark, BUILD_VERSION);
    assert_eq!(
        copy(&bare, 80),
        [
            "codex-alab  v0.164.0-dev.12814",
            "bloatware-free · nag-free · optimized for aterm",
        ]
    );
    for (version, label) in [
        ("", "codex-alab"),
        ("unknown", "codex-alab"),
        ("dev", "codex-alab  vdev"),
        ("0.0.0", "codex-alab  vdev"),
    ] {
        let rows = copy(&Banner::new(Style::Ribbon, Depth::Mono, dark, version), 80);
        assert_eq!(rows[0], label, "{version:?}");
    }
    // An upstream with no commit draws the release alone; an empty one draws no line.
    let release = bare.clone().with_upstream(Some("0.163.0".to_owned()));
    assert_eq!(copy(&release, 80)[2], "based on OpenAI Codex 0.163.0");
    let empty = bare.with_upstream(Some(" ".to_owned()));
    assert_eq!(copy(&empty, 80).len(), 2);
    // A row whose numbers back nothing says nothing.
    static WORSE: [Claim; 3] = [
        Claim {
            measure: Measure::FootprintMb,
            alab: 252,
            upstream: 40,
            against: "0.163",
            measured: "2026-10-20",
        },
        Claim {
            measure: Measure::Daemons,
            alab: 1,
            upstream: 1,
            against: "0.163",
            measured: "2026-10-20",
        },
        Claim {
            measure: Measure::IdleConnections,
            alab: 5,
            upstream: 5,
            against: "0.163",
            measured: "2026-10-20",
        },
    ];
    let worse = Banner::new(Style::Ribbon, Depth::Mono, dark, BUILD_VERSION).with_claims(&WORSE);
    assert_eq!(copy(&worse, 80).len(), 2);
    // Below a halving the memory claim is a percentage, rounded down.
    for (alab, upstream, expected) in [
        (200, 252, "20% less memory"),
        (3_000_000_000, 4_000_000_000, "25% less memory"),
        (u32::MAX / 2 + 1, u32::MAX, "49% less memory"),
        (u32::MAX - 1, u32::MAX, "0% less memory"),
    ] {
        let near = Claim {
            alab,
            upstream,
            ..CLAIMS[0]
        };
        let words: String = near.forms()[2]
            .iter()
            .map(|run| match run {
                super::claims::Run::Figure(text) | super::claims::Run::Words(text) => text.as_str(),
            })
            .collect();
        assert_eq!(words, expected, "{alab} vs {upstream}");
    }
}

#[test]
fn every_visible_measurement_keeps_its_default_configuration_date_and_comparison() {
    let measured = |line: &str| {
        [
            "less memory",
            "no daemon",
            "background daemon",
            "connection",
            "socket",
        ]
        .iter()
        .any(|words| line.contains(words))
    };
    for (case, width, lines) in every_banner() {
        let rows: Vec<_> = lines.iter().map(text).collect();
        assert!(
            rows.iter().all(|row| !row.contains("+g")
                && !row.contains("a180bdb0c")
                && !row.contains("e8aa7c1678ca")),
            "{case} at {width}: primary banner leaked a source identifier: {rows:?}"
        );
        if let Some(first) = rows.iter().position(|row| measured(row)) {
            assert!(
                rows[..first].iter().any(|row| {
                    row.to_lowercase().contains("defaults")
                        && row.contains(CLAIMS[0].against)
                        && row.contains(CLAIMS[0].measured)
                }),
                "{case} at {width}: no dated default comparison before {rows:?}"
            );
        }
    }
    // One heading cannot qualify measurements of different versions or dates.
    static DIFFERENT_DATE: [Claim; 2] = [
        CLAIMS[0],
        Claim {
            measured: "2026-10-19",
            ..CLAIMS[1]
        },
    ];
    static DIFFERENT_VERSION: [Claim; 2] = [
        CLAIMS[0],
        Claim {
            against: "0.162",
            ..CLAIMS[1]
        },
    ];
    for claims in [&DIFFERENT_DATE, &DIFFERENT_VERSION] {
        let banner = Banner::new(Style::Ribbon, Depth::Mono, Theme::default(), BUILD_VERSION)
            .with_claims(claims);
        assert!(!copy(&banner, 120).iter().any(|row| measured(row)));
    }
}

/// The figures are bold, in the harness green where there is colour; the words around them are
/// the terminal's own.
#[test]
fn figures_are_bold() {
    let dark = Theme::from_bg(Some(DARK));
    for depth in DEPTHS {
        let lines = packaged(Style::Ribbon, depth, dark).lines(80);
        let bold: Vec<String> = lines[3..=4]
            .iter()
            .flat_map(|line| &line.spans)
            .filter(|span| {
                span.style
                    .add_modifier
                    .contains(ratatui::style::Modifier::BOLD)
            })
            .map(|span| span.content.to_string())
            .collect();
        assert_eq!(bold, ["6×", "1", "5"], "{depth:?}");
    }
}

#[test]
fn ladder_rows() {
    let banner = packaged(Style::Ribbon, Depth::Mono, Theme::default());
    let rows = |w: u16| banner.lines(w).len();
    assert_eq!(rows(240), 6);
    assert_eq!(rows(80), 6);
    assert_eq!(rows(49), 6);
    assert_eq!(rows(40), 6);
    assert_eq!(rows(22), 5);
    assert_eq!(rows(21), 2);
    assert_eq!(rows(13), 2);
    assert_eq!(rows(12), 0);
    assert_eq!(rows(0), 0);
    let bare = Banner::new(Style::Ribbon, Depth::Mono, Theme::default(), BUILD_VERSION);
    assert_eq!(bare.lines(80).len(), 6);
}

#[test]
fn snapshots() {
    let dark = Theme::from_bg(Some(DARK));
    let mut cases: Vec<(Style, Depth, &str, Theme, u16)> = Vec::new();
    for style in STYLES {
        for w in [120u16, 80, 64, 44, 28, 20, 12] {
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
        let lines = packaged(style, depth, theme).lines(w);
        let name = format!("{style:?}_{depth:?}_{theme_name}_w{w}").to_ascii_lowercase();
        insta::assert_snapshot!(name, review_text(&lines));
    }
}

/// The harness row names the session's real Autopilot word under aterm's harness, as aterm's
/// glass does: `∞ cruise`, `⤓ land`, and `○ off` for the harness on with Autopilot off.
/// The row disappears where the full mode will not fit. Negative controls: no row with no supervisor or unsupervised, and no
/// row ever names another word than its own.
#[test]
fn the_harness_row_names_the_real_word() {
    let banner = packaged(Style::Ribbon, Depth::TrueColor, Theme::from_bg(Some(DARK)));
    let plain = banner.lines(80);
    for mode in [Mode::Cruise, Mode::Land, Mode::Off] {
        let row = |w: u16| {
            let lines = banner.lines_with_harness(w, Some(Harness::On(mode)));
            (lines.len() == banner.lines(w).len() + 1).then(|| text(&lines[lines.len() - 1]))
        };
        let word = mode.label();
        assert_eq!(
            row(80).as_deref(),
            Some(format!("  ✦ Autopilot {word}").as_str()),
            "{mode:?}"
        );
        assert_eq!(
            row(60).as_deref(),
            Some(format!("  ✦ Autopilot {word}").as_str()),
            "{mode:?}"
        );
        assert_eq!(
            row(40).as_deref(),
            Some(format!("  ✦ Autopilot {word}").as_str()),
            "{mode:?}"
        );
        assert_eq!(row(12), None, "{mode:?}");
        for w in 0..=240u16 {
            let Some(row) = row(w) else { continue };
            assert!(row.ends_with(word), "{mode:?} at {w}: {row:?}");
            for other in [Mode::Cruise, Mode::Land, Mode::Off] {
                assert!(
                    other == mode || !row.contains(other.label()),
                    "{mode:?} at {w}: {row:?}"
                );
            }
        }
    }
    for harness in [None, Some(Harness::Unsupervised)] {
        assert_eq!(banner.lines_with_harness(80, harness), plain, "{harness:?}");
    }
}

/// The owner likes the white theme under Autopilot off: with the harness on and Autopilot off
/// the banner wears aterm's moonbow, white to steel on a dark ground and pearl to graphite on
/// a light one, where cruise wears the spectrum. Every cell the word recolours is a grey under
/// Off, and the recoloured cells include the wordmark on its art row, the trail's half blocks
/// and the harness row's name and word. Negative controls: the kitty's own colours stay, and
/// cruise and land draw alike but for the word.
#[test]
fn the_banner_wears_the_moonbow_under_autopilot_off() {
    assert_eq!(palette::moonbow(0.0, false), palette::MOONBOW[0]);
    for (k, &stop) in palette::MOONBOW.iter().enumerate() {
        assert_eq!(palette::moonbow(k as f32 / 3.0, false), stop, "stop {k}");
    }
    for (k, &stop) in palette::PRINT_OFF.iter().enumerate() {
        assert_eq!(
            palette::moonbow(k as f32 / 5.0, true),
            stop,
            "print stop {k}"
        );
    }
    assert_eq!(palette::moonbow(f32::NAN, false), 0xFF_FFFF);
    assert_eq!(palette::moonbow(-1.0, true), palette::PRINT_OFF[0]);
    assert_eq!(palette::moonbow(2.0, true), palette::PRINT_OFF[5]);
    let chroma = |rgb: palette::Rgb| {
        let (r, g, b) = palette::channels(rgb);
        r.max(g).max(b) - r.min(g).min(b)
    };
    for &stop in palette::MOONBOW.iter().chain(&palette::PRINT_OFF) {
        assert!(chroma(stop) <= 0x1C, "{stop:06X}");
    }

    type Cells = Vec<(usize, String, Option<Color>, Option<Color>)>;
    /// Each cell of `lines` as its row, glyph and colours.
    fn cells(lines: &[Line<'_>]) -> Cells {
        let mut out = Vec::new();
        for (row, line) in lines.iter().enumerate() {
            for span in &line.spans {
                for ch in span.content.chars() {
                    out.push((row, ch.to_string(), span.style.fg, span.style.bg));
                }
            }
        }
        out
    }
    let grey = |colour: Color| match colour {
        Color::Rgb(..) | Color::Indexed(_) => chroma(drawn(colour)) <= 0x24,
        named => matches!(
            named,
            Color::White | Color::Gray | Color::DarkGray | Color::Black
        ),
    };
    let kitty = [palette::PATCH_GINGER, palette::IRIS, palette::INNER_EAR];
    for (style, depth, theme) in STYLES.into_iter().flat_map(|style| {
        [Depth::TrueColor, Depth::Ansi256, Depth::Ansi16]
            .into_iter()
            .flat_map(move |depth| {
                [Theme::from_bg(Some(DARK)), Theme::from_bg(Some(LIGHT))]
                    .map(|theme| (style, depth, theme))
            })
    }) {
        let banner = packaged(style, depth, theme);
        let cruise = cells(&banner.lines_with_harness(120, Some(Harness::On(Mode::Cruise))));
        let land = cells(&banner.lines_with_harness(120, Some(Harness::On(Mode::Land))));
        let off = cells(&banner.lines_with_harness(120, Some(Harness::On(Mode::Off))));
        let case = format!("{style:?}/{depth:?}/{theme:?}");
        // Every cell up to the harness row's word, the row's last cells.
        let body = |cells: &Cells, word: &str| cells[..cells.len() - word.chars().count()].to_vec();
        assert_eq!(
            body(&cruise, Mode::Cruise.label()),
            body(&land, Mode::Land.label()),
            "{case}: only the word differs"
        );
        assert_eq!(
            body(&cruise, Mode::Cruise.label()).len(),
            body(&off, Mode::Off.label()).len(),
            "{case}"
        );
        let harness_row = off.last().map(|cell| cell.0).expect("the harness row");
        // What the word recoloured, row by row: each recoloured cell a grey.
        let mut recoloured = std::collections::BTreeMap::<usize, String>::new();
        for ((_, glyph, was, was_bg), (row, now_glyph, now, now_bg)) in cruise.iter().zip(&off) {
            if glyph != now_glyph || (was, was_bg) == (now, now_bg) {
                continue;
            }
            recoloured.entry(*row).or_default().push_str(glyph);
            for (was, now) in [(was, now), (was_bg, now_bg)] {
                if was != now {
                    let now = now.unwrap_or_else(|| panic!("{case}: {glyph:?} on row {row}"));
                    assert!(
                        grey(now),
                        "{case}: {glyph:?} on row {row} is {now:?} under Off"
                    );
                }
            }
        }
        let art = |wanted: &dyn Fn(&str) -> bool| {
            recoloured
                .iter()
                .any(|(row, glyphs)| *row != harness_row && wanted(glyphs))
        };
        assert!(
            art(&|glyphs| "codex-alab".chars().all(|ch| glyphs.contains(ch))),
            "{case}: the wordmark wears the moonbow: {recoloured:?}"
        );
        let trail: usize = recoloured
            .iter()
            .filter(|(row, _)| **row != harness_row)
            .map(|(_, glyphs)| {
                glyphs
                    .chars()
                    .filter(|ch| matches!(ch, '▀' | '▄' | '█'))
                    .count()
            })
            .sum();
        assert!(
            trail >= 8,
            "{case}: the trail wears the moonbow: {recoloured:?}"
        );
        assert!(
            recoloured
                .get(&harness_row)
                .is_some_and(|glyphs| "Autopilot".chars().all(|ch| glyphs.contains(ch))),
            "{case}: the harness row wears the moonbow: {recoloured:?}"
        );
        // The word itself, which cruise spells otherwise, is a grey too.
        let word = &off[off.len() - Mode::Off.label().chars().count()..];
        for (row, glyph, fg, _) in word.iter().filter(|cell| cell.1 != " ") {
            let fg = fg.unwrap_or_else(|| panic!("{case}: {glyph:?} on row {row}"));
            assert!(grey(fg), "{case}: the word's {glyph:?} is {fg:?}");
        }
        // The kitty is not the word's to recolour: its ginger patch, iris and inner ears keep
        // their colours, above and below each half block.
        if depth == Depth::TrueColor && !theme.is_light() {
            let mut seen = 0;
            for ((_, glyph, was, was_bg), (row, _, now, now_bg)) in cruise.iter().zip(&off) {
                let painted = |colour: &Option<Color>| {
                    colour.is_some_and(|colour| kitty.contains(&drawn(colour)))
                };
                if painted(was) || painted(was_bg) {
                    seen += 1;
                    assert_eq!(
                        (was, was_bg),
                        (now, now_bg),
                        "{case}: kitty {glyph:?} on row {row}"
                    );
                }
            }
            assert!(seen > 0, "{case}: the kitty's colours were found");
        }
    }
}
