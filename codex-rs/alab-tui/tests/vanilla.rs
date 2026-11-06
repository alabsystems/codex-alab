// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise the real process switch, separate from thread-scoped rendering fixtures.

use codex_alab_tui::banner::Depth;
use codex_alab_tui::banner::Theme;
use codex_alab_tui::interface;
use codex_alab_tui::session;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn vanilla_keeps_restart_bookkeeping_and_only_a_recovery_switch() {
    codex_alab_core::activate();
    interface::configure(true);
    assert!(interface::enabled());
    interface::set_enabled(false);
    assert!(!codex_alab_core::active());
    assert!(!interface::active());
    assert!(
        session::active(),
        "draft and queue tracking must survive mode changes"
    );
    assert!(
        interface::available(),
        "the explicit /alab escape remains available"
    );
    assert!(interface::session_title().is_none());
    let area = Rect::new(0, 0, 100, 4);
    let native = Buffer::empty(area);
    let mut actual = native.clone();
    interface::render(area, &mut actual, Depth::Mono, Theme::default());
    let button = interface::button_rect(area).expect("recovery switch");
    let text: String = (button.x..button.right())
        .map(|x| actual[(x, button.y)].symbol())
        .collect();
    assert_eq!(text, "[ALab off]");
    assert!(interface::mouse_hit(button.x, button.y));
    assert!(!interface::active());
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if !button.contains((x, y).into()) {
                assert_eq!(actual[(x, y)], native[(x, y)]);
            }
        }
    }
    interface::set_enabled(true);
    assert!(codex_alab_core::active());
    assert!(interface::enabled());
}
