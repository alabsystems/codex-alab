// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Print the launch card without starting a model session.
//! `targo --unverified run -p codex-alab-tui --example banner_preview -- 80`

use codex_alab_tui::banner::Banner;
use codex_alab_tui::banner::Depth;
use codex_alab_tui::banner::Harness;
use codex_alab_tui::banner::Mode;
use codex_alab_tui::banner::Style;
use codex_alab_tui::banner::Theme;

fn main() {
    let width = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(80);
    let banner = Banner::new(Style::Codex, Depth::Mono, Theme::default(), "dev");
    for line in banner.session_lines(
        width,
        Some(Harness::On(Mode::Land)),
        vec!["model: gpt-6.1".into(), "directory: ~/project".into()],
    ) {
        println!("{line}");
    }
}
