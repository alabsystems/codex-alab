// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The kitty: an 18×10-pixel front-facing calico head in the style of aterm's codex flagship
//! (S117, "a graphic calico split built from two broad shapes"), 16 pixels of head and two
//! whisker columns ahead of it. aterm's flying kitty is a head riding the ribbon, so this one is
//! too. New art: aterm draws its cats from vector paths and has no cell-sized one.
//!
//! ```text
//! O outline   W coat   D dark patch   G ginger patch   P/N inner ear, nose   M muzzle
//! E iris      H catch light           K pupil          . transparent
//! ```

/// Sprite width in pixels (= cells).
pub const KITTY_W: usize = 18;
/// Sprite height in pixels (= two per cell row).
pub const KITTY_H: usize = 10;

/// The pixel map, one byte per pixel.
pub const KITTY: [&[u8; KITTY_W]; KITTY_H] = [
    b".OO..........OO...",
    b"ODDO........OGGO..",
    b"ODPDOOOOOOOOGPGO..",
    b"ODDDDDWWWWGGGGGO..",
    b"ODDHEDWWWWGHEGGO..",
    b"ODDKKDWWWWGKKGGO.O",
    b"OWWWWWWNNWWWWWWOO.",
    b".OWWWMMMMMMWWWO.OO",
    b"..OOWWWWWWWWOO....",
    b"....OOOOOOOO......",
];
