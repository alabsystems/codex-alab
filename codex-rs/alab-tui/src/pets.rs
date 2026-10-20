// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Codex's terminal pets inside aterm.
//!
//! Upstream draws a pet only in terminals it knows by name (Ghostty, kitty, WezTerm, iTerm2
//! 3.6+, sixel terminals), so in aterm `/pets` says "Pets aren't available in this terminal".
//! aterm's kitty graphics core takes every byte codex's direct-medium path sends: `a=T`,
//! `t=d`, `f=100` PNG, the `c=`/`r=` cell box, `C=1`, `q=2`, `m=1` chunks and `a=d,d=I`
//! deletes by id. The one hook (`tui/src/pets/image_protocol.rs`, after the tmux and Zellij
//! refusals) asks [`aterm_kitty_graphics`].
//!
//! Always [`ImageProtocol::Kitty`] there, never `KittyLocalFile`: `t=f` file transfer is off
//! in aterm by default (`allow_kitty_file_transfer = false`), and codex's `q=2` would hide the
//! refusal, so nothing would draw.
//!
//! [`ImageProtocol::Kitty`]: https://sw.kovidgoyal.net/kitty/graphics-protocol/

/// The codex-alab switch, for the pets glue (codex-tui does not depend on alab-core).
pub use codex_alab_core::active;

pub mod pack;

/// Whether codex's pets use the kitty graphics path in this terminal: the codex-alab binary
/// ([`active`]) inside aterm (`TERM_PROGRAM=aterm`, aterm's own identity, set for every child).
///
/// `TERM_PROGRAM` and not the proven claim: drawing is passive bytes the emulator renders or
/// ignores, not authority to act on the session, and a released claim or an unreachable
/// socket leaves kitty graphics working. A person's rc that overrides `TERM_PROGRAM` turns pets
/// off, the safe direction.
pub fn aterm_kitty_graphics(term_program: Option<&str>) -> bool {
    aterm_kitty_graphics_when(active(), term_program)
}

fn aterm_kitty_graphics_when(active: bool, term_program: Option<&str>) -> bool {
    active && term_program.is_some_and(|p| p.trim().eq_ignore_ascii_case("aterm"))
}

#[cfg(test)]
mod tests {
    use super::aterm_kitty_graphics_when;

    #[test]
    fn aterm_takes_the_kitty_path_only_in_codex_alab() {
        assert!(aterm_kitty_graphics_when(true, Some("aterm")));
        assert!(aterm_kitty_graphics_when(true, Some(" ATerm ")));
        // Upstream's behavior wherever the switch is off (codex-tui's own tests included).
        assert!(!aterm_kitty_graphics_when(false, Some("aterm")));
    }

    #[test]
    fn every_other_terminal_keeps_upstreams_verdict() {
        for other in [
            None,
            Some(""),
            Some("iTerm.app"),
            Some("Apple_Terminal"),
            Some("aterm-like"),
            Some("xterm"),
        ] {
            assert!(!aterm_kitty_graphics_when(true, other), "{other:?}");
        }
    }
}
