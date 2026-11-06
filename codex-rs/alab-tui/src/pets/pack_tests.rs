// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use pretty_assertions::assert_eq;

/// FNV-1a, 64-bit: enough to pin bytes without a hashing dependency.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn rgba_at(px: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
    let at = (y * width + x) * 4;
    [px[at], px[at + 1], px[at + 2], px[at + 3]]
}

/// The pinned sheet. A change to the art changes this hash: bump [`KITTY_FILE`]'s version in
/// the same commit, or every install keeps drawing its cached old sheet.
#[test]
fn the_sheet_is_pinned() {
    let sheet = spritesheet_rgba();
    assert_eq!(sheet.len(), SHEET_W * SHEET_H * 4);
    assert_eq!(
        format!("{:016x}", fnv1a(&sheet)),
        "af79b175f26332c9",
        "the art changed: bump KITTY_FILE's version too"
    );
    assert_eq!(KITTY_FILE, "alab-kitty-spritesheet-v2.png");
}

#[test]
fn the_geometry_fits_codexs_frame_budget() {
    assert_eq!((SHEET_W, SHEET_H), (4608, 1872));
    assert_eq!((FRAME_W, FRAME_H, COLUMNS, ROWS), (192, 208, 24, 9));
    assert_eq!(
        ROW_NAMES.iter().zip(ROW_FRAMES).collect::<Vec<_>>(),
        vec![
            (&"idle", 24),
            (&"running-right", 24),
            (&"running-left", 24),
            (&"waving", 24),
            (&"jumping", 24),
            (&"failed", 24),
            (&"waiting", 24),
            (&"running", 24),
            (&"review", 24),
        ]
    );
}

#[test]
fn every_frame_is_drawn() {
    let sheet = spritesheet_rgba();
    for row in 0..ROWS {
        for col in 0..COLUMNS {
            let opaque = (0..FRAME_H)
                .flat_map(|y| (0..FRAME_W).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    rgba_at(&sheet, SHEET_W, col * FRAME_W + x, row * FRAME_H + y)[3] > 0
                })
                .count();
            assert!(opaque > 10_000, "row {row} frame {col}: {opaque} pixels");
        }
    }
}

#[test]
fn each_animation_moves() {
    for (row, &frames) in ROW_FRAMES.iter().enumerate() {
        let first = frame_rgba(row, 0);
        assert!(
            (1..frames).any(|k| frame_rgba(row, k) != first),
            "{} never changes",
            ROW_NAMES[row]
        );
    }
}

#[test]
fn the_calico_wears_s117s_colours() {
    let px = frame_rgba(0, 0);
    let has = |rgb: Rgb| {
        let (r, g, b) = palette::channels(rgb);
        px.as_chunks::<4>().0.contains(&[r, g, b, 255])
    };
    for (name, rgb) in [
        ("coat", 0xFA_F4E8),
        ("dark patch", 0x4B_434D),
        ("ginger patch", 0xB9_6F4F),
        ("iris", 0x5D_7B78),
        ("pink", 0xD9_8D9A),
    ] {
        assert!(has(rgb), "{name} #{rgb:06X}");
    }
    // The ribbon is aterm's spectrum, red where it meets the head's shoulder, violet at the tail.
    let (r, g, b) = palette::channels(palette::spectrum(1.0));
    assert!(
        px.as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[..3] == [r, g, b] && p[3] > 0)
    );
}

#[test]
fn running_left_is_running_right_mirrored() {
    for k in 0..ROW_FRAMES[1] {
        let right = frame_rgba(1, k);
        let left = frame_rgba(2, k);
        for y in (0..FRAME_H).step_by(13) {
            for x in 0..FRAME_W {
                assert_eq!(
                    rgba_at(&right, FRAME_W, x, y),
                    rgba_at(&left, FRAME_W, FRAME_W - 1 - x, y)
                );
            }
        }
    }
}

#[test]
fn failed_drains_the_ribbon_and_shuts_the_eyes() {
    let failed = frame_rgba(5, COLUMNS - 1);
    // Mid-ribbon, left of the head: grey-ish (channels within a narrow band).
    let spread = |p: [u8; 4]| {
        let (lo, hi) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
        hi - lo
    };
    let idle = frame_rgba(0, 0);
    let (x, y) = (60, (RIBBON_Y + RIBBON_H / 2) as usize);
    assert!(spread(rgba_at(&failed, FRAME_W, x, y)) < spread(rgba_at(&idle, FRAME_W, x, y)));
    // No iris shows.
    let (r, g, b) = palette::channels(palette::IRIS);
    assert!(!failed.as_chunks::<4>().0.contains(&[r, g, b, 255]));
}

#[test]
fn looping_tracks_cross_the_seam_without_a_visual_jump() {
    let difference = |left: &[u8], right: &[u8]| -> u64 {
        left.iter()
            .zip(right)
            .map(|(a, b)| u64::from(a.abs_diff(*b)))
            .sum()
    };
    for row in [0, 1, 2, 3, 4, 6, 7, 8] {
        let frames: Vec<_> = (0..COLUMNS).map(|k| frame_rgba(row, k)).collect();
        let seam = difference(&frames[COLUMNS - 1], &frames[0]);
        let largest_step = frames
            .windows(2)
            .map(|pair| difference(&pair[0], &pair[1]))
            .max()
            .unwrap();
        assert!(
            seam <= largest_step,
            "{} jumps at its loop seam: {seam} > {largest_step}",
            ROW_NAMES[row]
        );
    }
}

#[test]
fn idle_blink_is_brief_and_movement_uses_even_samples() {
    assert_eq!(
        (0..COLUMNS)
            .filter(|&k| look(0, k).eyes == Eyes::Blink)
            .count(),
        1
    );
    for row in [0, 1, 2, 3, 4, 6, 7, 8] {
        for k in 0..COLUMNS {
            let next = (k + 1) % COLUMNS;
            assert!((look(row, k).lift - look(row, next).lift).abs() <= 3);
        }
    }
}
