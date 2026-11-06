// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(pet-perf): what aterm holds of the ambient pet, one stored image per frame, placed
//! by id.
//!
//! Upstream deletes the pet and sends its whole PNG again on every draw (`a=d,d=I`, then
//! `a=T` with about 3.9 KB of base64), and a draw comes with every frame codex paints: about
//! one a second at rest from the pet's own animation, 31 a second during a turn. Inside
//! aterm codex-alab sends each frame's picture once (`a=t`: stored, not shown) under an id of
//! its own, and every draw after that only moves its one placement (`a=p`, about 60 bytes
//! with the cursor save and restore), so aterm no longer decodes a PNG per frame either. The
//! glue that writes the bytes is `codex-rs/alab-tui/glue/pet_kitty.rs`; this module keeps what
//! it needs that is not codex-tui's: the ids, the record of when the terminal may have lost
//! what it stored, and the reading of aterm's answer when it has.
//!
//! A stored frame is sent again only when the terminal may have lost it. A full clear or a
//! repaint codex cannot see into, and a resize ([`screen_lost`], the `ALAB(pet-perf)` hooks in
//! `tui/src/custom_terminal.rs`), and aterm answering a placement `ENOENT` ([`reply_lost`],
//! which `glue/kitty_replies.rs` reads off the input: aterm's restore after a seamless update
//! carries no kitty images) each move the [`epoch`]; the next draw sends its frame again, and
//! every other frame as it next comes up.
//!
//! Only an aterm whose kitty graphics take a placed frame ([`FIRST_ATERM`], read from the
//! `TERM_PROGRAM_VERSION` aterm sets for its children) gets this path; an older one keeps
//! upstream's whole picture per draw, which every aterm with kitty graphics draws.

use std::ops::RangeInclusive;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

/// The first id of the ambient pet's frames: upstream's own id for the ambient pet
/// (`0xC0DE`), shifted past every id codex-tui uses (`0xC0DE`, `0xC0DF`, and the transcript's
/// from `0xC100`), with room for [`FRAME_SLOTS`] frames under it.
const FRAME_ID_BASE: u32 = 0xC0DE << 12;
/// How many frames get an id: far more than a pet's sheet holds (codex's are 8 by 9).
const FRAME_SLOTS: u32 = 1 << 12;
/// Every id a frame of the ambient pet may be stored under.
pub const FRAME_IDS: RangeInclusive<u32> = FRAME_ID_BASE..=FRAME_ID_BASE + (FRAME_SLOTS - 1);

static EPOCH: AtomicU64 = AtomicU64::new(0);

/// The first aterm whose kitty graphics take a placed frame, 0.100.0 (aterm 6d20743955): a
/// put laid out by its own `c=`/`r=` that moves the placement its `i=`/`p=` name, `d=i` by
/// `p=`, `d=R` by id range, and a put's failure answered under `q=1`. Before it, a put drew
/// the stored frame at its own pixel size and every put was one more placement, so the pet
/// would come out the wrong size and leave copies behind.
const FIRST_ATERM: (u64, u64) = (0, 100);

/// Whether the ambient pet takes this path: the codex-alab binary inside aterm, where the
/// pet draws with kitty graphics on the direct medium ([`super::aterm_kitty_graphics`]), and
/// that aterm is [`FIRST_ATERM`] or later.
pub fn on() -> bool {
    static IN_ATERM: OnceLock<bool> = OnceLock::new();
    super::active()
        && *IN_ATERM.get_or_init(|| {
            codex_alab_core::inside_aterm()
                && places_frames(std::env::var("TERM_PROGRAM_VERSION").ok().as_deref())
        })
}

/// Whether aterm's `TERM_PROGRAM_VERSION` (`MAJOR.MINOR.0`) names [`FIRST_ATERM`] or later.
/// Absent or unreadable: no, and the pet keeps upstream's draw.
fn places_frames(version: Option<&str>) -> bool {
    let mut parts = version.unwrap_or_default().trim().split('.');
    let mut next = || parts.next().and_then(|part| part.parse::<u64>().ok());
    matches!((next(), next()), (Some(major), Some(minor)) if (major, minor) >= FIRST_ATERM)
}

/// The id frame `index` of the ambient pet is stored under, if it has one.
pub fn frame_id(index: usize) -> Option<u32> {
    u32::try_from(index)
        .ok()
        .filter(|index| *index < FRAME_SLOTS)
        .map(|index| FRAME_ID_BASE | index)
}

/// Whether `id` is one of the ambient pet's frames.
pub fn is_frame_id(id: u32) -> bool {
    FRAME_IDS.contains(&id)
}

/// The count of the moments the terminal may have dropped what it stored: what a draw holds
/// as sent is good only while this stays where it was when it was sent.
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Acquire)
}

/// The screen was cleared, repainted outside codex's diff or resized: send each frame again
/// the next time it is drawn.
pub fn screen_lost() {
    EPOCH.fetch_add(1, Ordering::AcqRel);
}

/// The text of a kitty graphics reply, between `ESC _ G` and `ESC \` (`i=<id>[,p=<n>…];<OK
/// or an error>`): whether it says one of the pet's stored frames is gone, which forgets
/// every frame sent (the terminal that lost one lost the store). `None` when it is no reply.
pub fn reply_lost(body: &str) -> Option<bool> {
    let (keys, verdict) = body.split_once(';')?;
    let id = keys
        .split(',')
        .find_map(|key| key.strip_prefix("i="))
        .and_then(|id| id.parse::<u32>().ok());
    if id.is_none() && !keys.split(',').any(|key| key.starts_with("I=")) {
        return None;
    }
    let lost = verdict != "OK" && id.is_some_and(is_frame_id);
    if lost {
        screen_lost();
    }
    Some(lost)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn frames_get_ids_of_their_own_past_every_codex_tui_id() {
        assert_eq!(frame_id(0), Some(0xC0DE << 12));
        assert_eq!(frame_id(71), Some((0xC0DE << 12) + 71));
        assert_eq!(frame_id(FRAME_SLOTS as usize), None);
        for upstream in [0xC0DE, 0xC0DF, 0xC100, 0xC100 + 4096] {
            assert!(!is_frame_id(upstream), "{upstream:#x}");
        }
        assert!(is_frame_id(frame_id(4095).unwrap()));
    }

    #[test]
    fn only_an_aterm_that_places_frames_takes_this_path() {
        for newer in ["0.100.0", "0.109.0", " 0.109.0 ", "0.100", "1.0.0"] {
            assert!(places_frames(Some(newer)), "{newer:?}");
        }
        // 0.98.0 is the last release before placements; 0.62 the retired internal lineage,
        // 0.15.<stamp> the archive's timestamp tags.
        for older in [
            "0.98.0",
            "0.62.0",
            "0.15.2607021856",
            "0",
            "",
            "aterm",
            "x.100.0",
        ] {
            assert!(!places_frames(Some(older)), "{older:?}");
        }
        assert!(!places_frames(/*version*/ None));
    }

    #[test]
    fn an_enoent_for_a_frame_moves_the_epoch() {
        let id = frame_id(3).unwrap();
        let before = epoch();
        assert_eq!(
            reply_lost(&format!("i={id},p=1;ENOENT:no such image")),
            Some(true)
        );
        assert!(epoch() > before);
    }

    #[test]
    fn other_replies_lose_nothing() {
        let id = frame_id(3).unwrap();
        assert_eq!(reply_lost(&format!("i={id},p=1;OK")), Some(false));
        assert_eq!(reply_lost("i=49374;ENOENT:no such image"), Some(false));
        assert_eq!(reply_lost("I=7;ENOSPC:full"), Some(false));
        for not_a_reply in ["", "i=5", "p=1;OK", "hello;world"] {
            assert_eq!(reply_lost(not_a_reply), None, "{not_a_reply:?}");
        }
    }

    #[test]
    fn a_clear_moves_the_epoch() {
        let before = epoch();
        screen_lost();
        assert!(epoch() > before);
    }
}
