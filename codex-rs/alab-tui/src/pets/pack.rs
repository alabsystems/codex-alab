// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALab's built-in pet: "Rainbow Kitty", aterm's codex calico riding aterm's spectrum ribbon.
//!
//! The art is the launch banner's: its 18×10 front-facing calico head (S117's split: coat
//! `#FAF4E8`, dark patch `#4B434D`, ginger patch `#B96F4F`, iris `#5D7B78`, pink `#D98D9A`) and
//! aterm's 511-entry spectrum, both in [`crate::banner`] with their aterm provenance. Each
//! sprite pixel is a 10×10 block, and the head rides a ribbon that runs the frame's width under
//! its chin, as the banner's kitty rides its trail: red at the front (the whiskers' side),
//! violet at the tail, a white-hot glint at the front end (aterm's
//! meteor), and a 2-pixel pale halo keeps the dark outline legible on dark terminals.
//!
//! The sheet uses 24 columns × 9 rows of 192×208 frames. Each semantic animation has
//! its own row, sampled evenly around a complete cycle. The loader supplies matching frame
//! geometry and timing instead of inheriting the stock pets' long, uneven keyframe holds.
//!
//! Generated, never fetched: upstream's built-ins download from OpenAI's CDN on first use;
//! this one is drawn by [`spritesheet_rgba`] from code in the binary. Every step is integer or
//! IEEE-exact `f32` arithmetic (no transcendental function), so the bytes are the same on
//! every target; `pack_tests.rs` pins them.

use crate::banner::palette;
use crate::banner::palette::Rgb;
use crate::banner::sprite::KITTY;
use crate::banner::sprite::KITTY_H;
use crate::banner::sprite::KITTY_W;

/// The pet's id in `/pets` and `[tui] pet` (no upstream built-in uses it).
pub const KITTY_ID: &str = "alab-kitty";
/// Its name in `/pets`.
pub const KITTY_NAME: &str = "Rainbow Kitty";
/// Its line in `/pets`.
pub const KITTY_DESCRIPTION: &str = "aterm's codex calico riding the spectrum";
/// The cached sheet's file name (codex keys its built-in cache by file name). Bump the
/// version whenever the art changes, or an install keeps the old sheet.
pub const KITTY_FILE: &str = "alab-kitty-spritesheet-v2.png";

/// One frame's width, codex's `DEFAULT_FRAME_WIDTH`.
pub const FRAME_W: usize = 192;
/// One frame's height, codex's `DEFAULT_FRAME_HEIGHT`.
pub const FRAME_H: usize = 208;
/// Smooth samples per animation, within Codex's 256-frame limit.
pub const COLUMNS: usize = 24;
/// Older Codex builds require a 1536×1872 sheet even for custom pets.
pub const LEGACY_COLUMNS: usize = 8;
/// Rows, codex's `DEFAULT_FRAME_ROWS`.
pub const ROWS: usize = 9;
/// The sheet's width in pixels (4608).
pub const SHEET_W: usize = FRAME_W * COLUMNS;
/// The sheet's height in pixels (1872).
pub const SHEET_H: usize = FRAME_H * ROWS;

/// Every sample in each row is played.
pub const ROW_FRAMES: [usize; ROWS] = [COLUMNS; ROWS];

/// Even frame times: a gentle idle/attention cycle and faster movement while working.
pub const ROW_FRAME_MS: [u64; ROWS] = [80, 50, 50, 80, 50, 80, 80, 50, 80];

/// codex's animation name for each row.
pub const ROW_NAMES: [&str; ROWS] = [
    "idle",
    "running-right",
    "running-left",
    "waving",
    "jumping",
    "failed",
    "waiting",
    "running",
    "review",
];

/// One sprite pixel's side in sheet pixels.
const SCALE: usize = 10;
const HEAD_W: usize = KITTY_W * SCALE;
const HEAD_H: usize = KITTY_H * SCALE;
/// The head, centred.
const HEAD_X: i32 = ((FRAME_W - HEAD_W) / 2) as i32;
/// Its resting top.
const HEAD_Y: i32 = 40;
/// The ribbon it rides: a band under its chin, the full width of the frame.
const RIBBON_Y: i32 = HEAD_Y + 86;
const RIBBON_H: i32 = 36;
/// The tail's fade into the sky, in pixels.
const TAIL_FADE: i32 = 24;
/// The white-hot glint at the ribbon's front end, in pixels.
const SHOULDER: i32 = 12;
/// The wave's period and height.
const WAVE_PERIOD: i32 = 48;
const WAVE_AMP: i32 = 4;
/// The pale halo around the head.
const HALO: i32 = 2;

/// The whole sheet, `SHEET_W × SHEET_H` straight-alpha RGBA, rows top to bottom.
pub fn spritesheet_rgba() -> Vec<u8> {
    sheet(COLUMNS)
}

/// Sample the same art into the fixed eight-column layout required by codex-old.
pub fn legacy_spritesheet_rgba() -> Vec<u8> {
    sheet(LEGACY_COLUMNS)
}

fn sheet(columns: usize) -> Vec<u8> {
    let width = FRAME_W * columns;
    let mut sheet = vec![0u8; width * SHEET_H * 4];
    for row in 0..ROWS {
        for column in 0..columns {
            let frame = frame(row, column * COLUMNS / columns);
            for y in 0..FRAME_H {
                let at = ((row * FRAME_H + y) * width + column * FRAME_W) * 4;
                let from = y * FRAME_W * 4;
                sheet[at..at + FRAME_W * 4].copy_from_slice(&frame.px[from..from + FRAME_W * 4]);
            }
        }
    }
    sheet
}

/// One frame of one row, `FRAME_W × FRAME_H` RGBA.
pub fn frame_rgba(row: usize, k: usize) -> Vec<u8> {
    frame(row, k).px
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Eyes {
    Open,
    Blink,
    Happy,
}

/// What a frame shows.
struct Look {
    /// The head's (and ribbon's) vertical offset.
    lift: i32,
    eyes: Eyes,
    /// The spectrum's shift along the ribbon (0 = red at the head).
    phase: f32,
    /// The wave's phase, in pixels.
    wave: i32,
    /// The wave's height.
    amp: i32,
    /// How far the ribbon is drained toward grey, out of 256.
    drain: u32,
    stars: Vec<Star>,
    glyph: Option<Glyph>,
    mirrored: bool,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            lift: 0,
            eyes: Eyes::Open,
            phase: 0.0,
            wave: 0,
            amp: WAVE_AMP,
            drain: 0,
            stars: Vec::new(),
            glyph: None,
            mirrored: false,
        }
    }
}

#[derive(Clone, Copy)]
struct Star {
    x: i32,
    y: i32,
    arm: i32,
    rgb: Rgb,
}

#[derive(Clone, Copy)]
struct Glyph {
    bits: &'static [&'static [u8]],
    x: i32,
    y: i32,
    scale: i32,
    rgb: Rgb,
}

const QUESTION: &[&[u8]] = &[
    b".###.", b"#...#", b"....#", b"...#.", b"..#..", b".....", b"..#..",
];

const HEART: &[&[u8]] = &[
    b".##.##.", b"#######", b"#######", b".#####.", b"..###..", b"...#...",
];

/// Periodic eased motion, with zero velocity at both ends and across the loop seam.
fn pulse(phase: f32) -> f32 {
    let t = pingpong(phase * 2.0);
    t * t * (3.0 - 2.0 * t)
}

fn look(row: usize, k: usize) -> Look {
    let gold = palette::spectrum_stop(2);
    let hot = palette::WHITE_HOT;
    let phase = k as f32 / COLUMNS as f32;
    let bob = pulse(phase);
    let lift = |height: f32| -(height * bob).round() as i32;
    let shimmer = |rgb| palette::mix(rgb, hot, bob * 0.5);
    match row {
        0 => Look {
            lift: lift(2.0),
            eyes: if k == COLUMNS / 2 {
                Eyes::Blink
            } else {
                Eyes::Open
            },
            phase: bob * 0.2,
            stars: vec![
                star(10, 14, 2, shimmer(hot)),
                star(178, 190, 2, shimmer(gold)),
            ],
            ..Look::default()
        },
        1 | 2 => Look {
            lift: lift(3.0),
            wave: k as i32 * WAVE_PERIOD / COLUMNS as i32,
            phase: phase * 2.0,
            stars: (0..3)
                .map(|i| {
                    let x =
                        (170 - i * 64 - (k * FRAME_W / COLUMNS) as i32).rem_euclid(FRAME_W as i32);
                    star(x, 176 + i * 10, 2, palette::spectrum_stop(i as usize * 2))
                })
                .collect(),
            mirrored: row == 2,
            ..Look::default()
        },
        3 => Look {
            lift: lift(4.0),
            eyes: if bob > 0.75 { Eyes::Happy } else { Eyes::Open },
            glyph: Some(Glyph {
                bits: HEART,
                x: 4,
                y: 22 - (18.0 * bob).round() as i32,
                scale: 3,
                rgb: shimmer(palette::INNER_EAR),
            }),
            ..Look::default()
        },
        4 => Look {
            lift: lift(24.0),
            eyes: if bob > 0.8 { Eyes::Happy } else { Eyes::Open },
            phase: bob * 0.2,
            stars: vec![
                star(10, 10, 2, shimmer(gold)),
                star(182, 12, 2, shimmer(hot)),
            ],
            ..Look::default()
        },
        5 => {
            let progress = k as f32 / (COLUMNS - 1) as f32;
            Look {
                lift: (8.0 * progress).round() as i32,
                eyes: Eyes::Blink,
                amp: 1,
                drain: 120 + (112.0 * progress).round() as u32,
                ..Look::default()
            }
        }
        6 => Look {
            lift: lift(1.0),
            glyph: Some(Glyph {
                bits: QUESTION,
                x: 166,
                y: 4 + (3.0 * bob).round() as i32,
                scale: 4,
                rgb: shimmer(gold),
            }),
            ..Look::default()
        },
        7 => Look {
            lift: lift(2.0),
            phase: phase * 2.0,
            wave: k as i32 * WAVE_PERIOD / COLUMNS as i32,
            stars: vec![star(12, 18, 2, shimmer(gold))],
            ..Look::default()
        },
        8 => Look {
            lift: lift(3.0),
            eyes: Eyes::Happy,
            stars: [
                (12, 14, gold),
                (182, 22, hot),
                (16, 186, hot),
                (172, 192, gold),
            ]
            .into_iter()
            .map(|(x, y, rgb)| star(x, y, 2, shimmer(rgb)))
            .collect(),
            ..Look::default()
        },
        _ => Look::default(),
    }
}

fn star(x: i32, y: i32, arm: i32, rgb: Rgb) -> Star {
    Star { x, y, arm, rgb }
}

struct Frame {
    px: Vec<u8>,
}

impl Frame {
    fn new() -> Self {
        Frame {
            px: vec![0; FRAME_W * FRAME_H * 4],
        }
    }

    fn put(&mut self, x: i32, y: i32, rgb: Rgb, alpha: u8) {
        if x < 0 || y < 0 || x >= FRAME_W as i32 || y >= FRAME_H as i32 {
            return;
        }
        let (r, g, b) = palette::channels(rgb);
        let at = (y as usize * FRAME_W + x as usize) * 4;
        self.px[at..at + 4].copy_from_slice(&[r, g, b, alpha]);
    }

    fn mirror(&mut self) {
        for y in 0..FRAME_H {
            for x in 0..FRAME_W / 2 {
                let a = (y * FRAME_W + x) * 4;
                let b = (y * FRAME_W + FRAME_W - 1 - x) * 4;
                for c in 0..4 {
                    self.px.swap(a + c, b + c);
                }
            }
        }
    }
}

fn frame(row: usize, k: usize) -> Frame {
    let look = look(row, k);
    let mut f = Frame::new();
    let head_y = HEAD_Y + look.lift;
    ribbon(&mut f, &look);
    for s in &look.stars {
        for d in -s.arm..=s.arm {
            for t in 0..2 {
                f.put(s.x + d, s.y + t, s.rgb, 255);
                f.put(s.x + t, s.y + d, s.rgb, 255);
            }
        }
    }
    if let Some(g) = look.glyph {
        glyph(&mut f, g);
    }
    head(&mut f, head_y, look.eyes);
    if look.mirrored {
        f.mirror();
    }
    f
}

/// The ribbon: one spectrum position per column, red at the front (the whiskers' side) and
/// violet at the tail, a white-hot glint at its front end (aterm's meteor). The head bobs and
/// hops on it; the ribbon itself stays put.
fn ribbon(f: &mut Frame, look: &Look) {
    let front = FRAME_W as i32 - 1;
    for x in 0..FRAME_W as i32 {
        let t = pingpong((front - x) as f32 / front as f32 + look.phase);
        let mut rgb = palette::spectrum(t);
        let near = front - x;
        if near < SHOULDER {
            rgb = palette::mix(palette::WHITE_HOT, rgb, near as f32 / SHOULDER as f32);
        }
        if look.drain > 0 {
            rgb = drain(rgb, look.drain);
        }
        let alpha = if x < TAIL_FADE {
            (64 + (255 - 64) * x / TAIL_FADE) as u8
        } else {
            255
        };
        let y0 = RIBBON_Y + wave(x + look.wave, look.amp);
        for y in y0..y0 + RIBBON_H {
            f.put(x, y, rgb, alpha);
        }
    }
}

/// `u` folded back and forth over 0..=1, so a shifted spectrum never jumps violet → red.
fn pingpong(u: f32) -> f32 {
    let u = u.rem_euclid(2.0);
    if u > 1.0 { 2.0 - u } else { u }
}

/// A triangle wave of [`WAVE_PERIOD`], `-amp..=amp`.
fn wave(x: i32, amp: i32) -> i32 {
    let half = WAVE_PERIOD / 2;
    let v = x.rem_euclid(WAVE_PERIOD);
    let tri = if v < half { v } else { WAVE_PERIOD - v };
    tri * 2 * amp / half - amp
}

/// `rgb` moved `amount`/256 of the way to its own grey (Rec.601 luma).
fn drain(rgb: Rgb, amount: u32) -> Rgb {
    let (r, g, b) = palette::channels(rgb);
    let grey = (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000;
    let ch = |c: u8| -> u8 {
        let c = u32::from(c);
        let a = amount.min(256);
        ((c * (256 - a) + grey * a) / 256) as u8
    };
    palette::pack((ch(r), ch(g), ch(b)))
}

fn glyph(f: &mut Frame, g: Glyph) {
    // The glyph's pixels, then a one-pixel ink rim around them, then the glyph again on top.
    let mut cells = Vec::new();
    for (gy, line) in g.bits.iter().enumerate() {
        for (gx, &bit) in line.iter().enumerate() {
            if bit == b'#' {
                cells.push((g.x + gx as i32 * g.scale, g.y + gy as i32 * g.scale));
            }
        }
    }
    for &(x, y) in &cells {
        for dy in -1..=g.scale {
            for dx in -1..=g.scale {
                f.put(x + dx, y + dy, palette::INK, 255);
            }
        }
    }
    for &(x, y) in &cells {
        for dy in 0..g.scale {
            for dx in 0..g.scale {
                f.put(x + dx, y + dy, g.rgb, 255);
            }
        }
    }
}

/// The banner's head, each sprite pixel a 10×10 block, ringed by a pale halo.
fn head(f: &mut Frame, head_y: i32, eyes: Eyes) {
    let mut mask = vec![false; FRAME_W * FRAME_H];
    let mut blocks = Vec::new();
    for (sy, line) in KITTY.iter().enumerate() {
        for (sx, &letter) in line.iter().enumerate() {
            let Some(rgb) = kitty_rgb(eye_letter(letter, sx, sy, eyes)) else {
                continue;
            };
            let (x0, y0) = (HEAD_X + (sx * SCALE) as i32, head_y + (sy * SCALE) as i32);
            blocks.push((x0, y0, rgb));
            for y in y0..y0 + SCALE as i32 {
                for x in x0..x0 + SCALE as i32 {
                    if (0..FRAME_W as i32).contains(&x) && (0..FRAME_H as i32).contains(&y) {
                        mask[y as usize * FRAME_W + x as usize] = true;
                    }
                }
            }
        }
    }
    let (lo_y, hi_y) = (head_y - HALO, head_y + HEAD_H as i32 + HALO);
    let (lo_x, hi_x) = (HEAD_X - HALO, HEAD_X + HEAD_W as i32 + HALO);
    let masked = |x: i32, y: i32| {
        (0..FRAME_W as i32).contains(&x)
            && (0..FRAME_H as i32).contains(&y)
            && mask[y as usize * FRAME_W + x as usize]
    };
    for y in lo_y..hi_y {
        for x in lo_x..hi_x {
            if masked(x, y) {
                continue;
            }
            let near = (-HALO..=HALO).any(|dy| (-HALO..=HALO).any(|dx| masked(x + dx, y + dy)));
            if near {
                f.put(x, y, palette::OUTLINE_PALE, 255);
            }
        }
    }
    for (x0, y0, rgb) in blocks {
        for y in y0..y0 + SCALE as i32 {
            for x in x0..x0 + SCALE as i32 {
                f.put(x, y, rgb, 255);
            }
        }
    }
}

/// The sprite's letter with the eyes shut (`Blink`: lid line low) or smiling (`Happy`: arc
/// high). The eyes are rows 4 and 5, columns 3-4 (dark patch side) and 11-12 (ginger side).
fn eye_letter(letter: u8, sx: usize, sy: usize, eyes: Eyes) -> u8 {
    if eyes == Eyes::Open || !matches!(letter, b'H' | b'E' | b'K') {
        return letter;
    }
    let patch = if sx < KITTY_W / 2 { b'D' } else { b'G' };
    match (eyes, sy) {
        (Eyes::Blink, 4) | (Eyes::Happy, 5) => patch,
        _ => b'K',
    }
}

/// The banner's colours for a sprite letter, with aterm's ink outline: the pale halo carries
/// it on a dark terminal.
fn kitty_rgb(letter: u8) -> Option<Rgb> {
    Some(match letter {
        b'O' => palette::INK,
        b'W' => palette::COAT,
        b'D' => palette::PATCH_DARK,
        b'G' => palette::PATCH_GINGER,
        b'P' | b'N' => palette::INNER_EAR,
        b'M' => palette::MUZZLE,
        b'E' => palette::IRIS,
        b'H' => palette::WHITE_HOT,
        b'K' => palette::EYE_INK_DARK,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "pack_tests.rs"]
mod tests;
