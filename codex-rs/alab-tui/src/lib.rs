// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! What codex-alab's TUI leaves out of upstream Codex, for the one-line `ALAB(<topic>):`
//! hooks in codex-tui. Each answer holds only in the codex binary
//! ([`codex_alab_core::active`]); everywhere else, upstream's tests included, it is
//! upstream's behavior. Config does most of the rest (`codex-rs/alab-core/defaults.toml`).
//! [`banner`] is the rainbow kitty launch banner's renderer; its codex-tui cell lives in
//! `glue/banner_cell.rs` and its composer border's codex side in `glue/composer_border.rs`
//! (both off under codex-tui's own tests unless a glue test forces them).

pub mod banner;

use codex_alab_core::active;

pub mod boxes;
pub mod context;
pub mod daemon_packages;
pub mod identity;
pub mod life;
pub mod login;
pub mod mail;
pub mod pets;
pub mod progress;
pub mod record;
pub mod resume_argv;
pub mod session;
pub mod trust;
pub mod upgrade;

/// Whether the TUI fetches OpenAI's announcement feed (`tui/src/tooltips.rs`): a GET to
/// raw.githubusercontent.com at every launch, made even with tips hidden. Not in codex-alab.
pub fn announcement_feed() -> bool {
    !active()
}

/// Whether the TUI and `codex archive|delete|queue` attach to a shared app-server daemon
/// already running on `$CODEX_HOME` (`tui/src/lib.rs`, the default-socket probe). Not in
/// codex-alab: a daemon runs every tab's tools with the environment of the tab that started
/// it (so an aterm tab's `aterm ctl @self` reaches another tab), keeps a copy of the codex
/// package per version under `$CODEX_HOME/packages`, never exits, holds threads' writer
/// locks, and may be a vendor build serving vendor defaults. Each TUI embeds its app-server
/// (and with `[features] daemon_auto_start = false` none is started either).
pub fn attach_shared_daemon() -> bool {
    !active()
}

/// A backend banner's call to action that only advertises a plan (`tui/src/backend_banners`):
/// codex-alab drops it, and the banner keeps its words and its working buttons. Buying
/// credits or a reset stays, since at a hard limit it is the only way to keep working.
/// Unknown actions are left to upstream, which already drops what it cannot route.
pub fn is_upsell_cta(action: &str) -> bool {
    active()
        && matches!(
            action,
            "open_plus_pricing_web" | "open_pro_pricing_web" | "open_pricing_dialog"
        )
}

/// Whether to skip the full-screen "try the new model" prompt when the current model can stay
/// (`tui/src/app/startup_prompts.rs`): codex-alab does. A forced migration off a retired
/// model still shows.
pub fn skip_optional_model_upgrade() -> bool {
    active()
}

#[cfg(test)]
#[path = "life_tests.rs"]
mod life_tests;
#[cfg(test)]
#[path = "login_tests.rs"]
mod login_tests;
#[cfg(test)]
#[path = "progress_tests.rs"]
mod progress_tests;
#[cfg(test)]
#[path = "session_tests.rs"]
mod session_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "trust_tests.rs"]
mod trust_tests;
#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod upgrade_tests;
