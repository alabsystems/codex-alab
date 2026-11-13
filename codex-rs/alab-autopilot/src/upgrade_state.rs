// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Core/goal data codec layered over the generic terminal/runtime envelope.
//! No decoded record constructs a native SDK source or releases execution.
use crate::upgrade_goals::GoalForestState;
use crate::upgrade_source::Captured;
use codex_alab_tui::upgrade_startup;
use codex_protocol::ThreadId;

impl Captured {
    pub fn encode_state(&self) -> Result<Vec<u8>, String> {
        let startup = self
            .startup_snapshot()
            .ok_or("source capture lacks its actual TUI startup state")?;
        encode(self, startup)
    }
}
fn encode(source: &Captured, startup: &upgrade_startup::Snapshot) -> Result<Vec<u8>, String> {
    let goals = source.goals.encode_state(source.primary)?;
    codex_alab_tui::upgrade_state::encode(
        &source.primary.to_string(),
        &goals,
        source.input_snapshot(),
        source.mode_snapshot(),
        startup,
    )
    .map_err(|error| error.to_string())
}
/// Data validation only; callers admit fresh native SDK readers independently.
pub fn decode_state(bytes: &[u8], expected_primary: ThreadId) -> Result<GoalForestState, String> {
    let goals = codex_alab_tui::upgrade_state::goal_bytes(bytes, &expected_primary.to_string())
        .map_err(|error| error.to_string())?;
    GoalForestState::decode(goals, expected_primary)
}
// The Core-only native fixture has no renderer. Supply explicitly synthetic view
// data ONLY here to qualify framing; its public Captured::encode_state must still
// refuse missing actual TUI facts. The real TUI producer has its separate PTY test.
#[cfg(test)]
pub(crate) fn qualify_frame(source: &Captured) {
    let startup = upgrade_startup::Snapshot {
        version: 1,
        cursor: [0, 0],
        colors: None,
        enhanced_keys: false,
        terminal_app_over_ssh: false,
        reports_key_releases: false,
        da1_supported: false,
        tmux_image_base: None,
        viewport: [0; 4],
        saved_viewport: None,
        hidden_cursor: false,
        visible_history_rows: 0,
        screen: upgrade_startup::Screen {
            active: false,
            mouse_active: false,
            mouse_capture_disabled: false,
            input_configured: false,
            keyboard_active: false,
            parent_reports_key_releases: false,
            pointer: 0,
        },
        focused: true,
        owned_screen: false,
        alt_screen_enabled: true,
        mouse_capture: 0,
    };
    let encoded = encode(source, &startup).unwrap();
    let decoded = decode_state(&encoded, source.primary).unwrap();
    assert_eq!(decoded.goals, source.goals.goals);
    assert_eq!(decoded.forest.roots(), source.goals.forest.roots());
    let parsed = codex_alab_tui::upgrade_state::data(&encoded).unwrap();
    assert_eq!(
        parsed.terminal.input.get().as_bytes(),
        source.input_snapshot()
    );
    assert_eq!(parsed.terminal.startup, startup);
    assert_eq!(
        serde_json::to_value(parsed.terminal.modes).unwrap(),
        serde_json::to_value(source.mode_snapshot()).unwrap()
    );
    assert!(decode_state(&encoded, ThreadId::new()).is_err());
    let original: serde_json::Value = serde_json::from_slice(
        encoded
            .strip_prefix(codex_alab_tui::upgrade_state::MAGIC)
            .unwrap(),
    )
    .unwrap();
    for alteration in 0..6 {
        let mut changed = original.clone();
        match alteration {
            0 => changed["version"] = serde_json::json!(99),
            1 => changed["goals"]["primary"] = serde_json::json!(ThreadId::new()),
            2 => {
                changed.as_object_mut().unwrap().remove("terminal");
            }
            3 => changed["terminal"]["startup"]["version"] = serde_json::json!(99),
            4 => {
                changed["terminal"].as_object_mut().unwrap().remove("input");
            }
            5 => changed["grant"] = serde_json::json!(true),
            _ => unreachable!(),
        }
        let mut bytes = codex_alab_tui::upgrade_state::MAGIC.to_vec();
        serde_json::to_writer(&mut bytes, &changed).unwrap();
        assert!(decode_state(&bytes, source.primary).is_err());
    }
}
