// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn autopilot_picker_keymap_preserves_defaults_and_resolves_native_shortcuts() {
    let action = keymap_action_id("chat", "open_autopilot").unwrap();
    assert_eq!(
        crate::keymap_setup::action_label(action.action),
        "Open Autopilot picker"
    );
    for (config, configured, primary) in [
        ("", false, None),
        ("[chat]\nopen_autopilot = []", true, None),
        ("[chat]\nopen_autopilot = 'f19'", true, Some("f19")),
        (
            "[chat]\nopen_autopilot = ['f20 a', 'f19']",
            true,
            Some("f20 a"),
        ),
    ] {
        let config: TuiKeymap = toml::from_str(config).unwrap();
        assert_eq!(
            configured_binding_for_action(&config, action)
                .unwrap()
                .is_some(),
            configured
        );
        let runtime = RuntimeKeymap::from_config(&config).unwrap();
        assert_eq!(
            runtime
                .primary_hint(action.context, action.action)
                .map(ShortcutHint::display_label)
                .as_deref(),
            primary
        );
        if primary.is_some() {
            assert!(
                runtime
                    .chat
                    .open_autopilot
                    .is_pressed(KeyEvent::from(KeyCode::F(19)))
            );
        } else {
            assert!(runtime.chat.open_autopilot.is_empty());
        }
    }
}

#[test]
fn autopilot_picker_keymap_protects_composer_input_and_yields_to_list_bindings() {
    for (binding, conflict) in [
        ("a", "printable"),
        ("ctrl-c", "fixed.interrupt_or_quit"),
        ("ctrl-k", "kill_line_end"),
        ("ctrl-t", "open_transcript"),
        #[cfg(unix)]
        ("ctrl-z", "reserved for suspend"),
    ] {
        let config: TuiKeymap =
            toml::from_str(&format!("[chat]\nopen_autopilot = '{binding}'")).unwrap();
        let error = RuntimeKeymap::from_config(&config).unwrap_err();
        assert!(error.contains(conflict), "{binding}: {error}");
    }
    for binding in ["f19", "f20 a"] {
        let config: TuiKeymap = toml::from_str(&format!(
            "[chat]\nopen_autopilot = '{binding}'\n[list]\naccept = '{binding}'"
        ))
        .unwrap();
        RuntimeKeymap::from_config(&config)
            .expect("the composer and a list cannot own input together");
        let config: TuiKeymap = toml::from_str(&format!(
            "[chat]\nopen_autopilot = '{binding}'\nopen_presets = '{binding}'"
        ))
        .unwrap();
        let error = RuntimeKeymap::from_config(&config).unwrap_err();
        assert!(
            error.contains("open_autopilot") && error.contains("open_presets"),
            "{error}"
        );
    }
}

#[test]
fn enhanced_keymap_appends_a_searchable_autopilot_action_and_preserves_vanilla_rows() {
    use codex_alab_tui::interface;

    let config = TuiKeymap::default();
    let runtime = RuntimeKeymap::defaults();
    for customized in [false, true] {
        let configured: TuiKeymap = toml::from_str("[chat]\nopen_autopilot = 'f19'").unwrap();
        let customized_runtime = RuntimeKeymap::from_config(&configured).unwrap();
        let (config, runtime) = if customized {
            (&configured, &customized_runtime)
        } else {
            (&config, &runtime)
        };
        let catalog = |enhanced| {
            let _edition = interface::scoped(Box::leak(Box::new(interface::State::new())));
            interface::configure(enhanced);
            crate::keymap_setup::build_keymap_picker_params(runtime, config).tabs
        };
        let old = catalog(false);
        let new = catalog(true);
        assert_eq!(old.len(), new.len());
        for (before, after) in old.iter().zip(&new) {
            let label = "Open Autopilot picker";
            assert!(!before.items.iter().any(|item| item.name == label));
            if let Some(index) = after.items.iter().position(|item| item.name == label) {
                let item = &after.items[index];
                assert!(
                    item.search_value
                        .as_ref()
                        .unwrap()
                        .contains("Cruise, Land or Off")
                );
                assert_eq!(
                    item.description.as_deref(),
                    Some(if customized { "f19" } else { "unbound" })
                );
                assert_eq!(
                    index,
                    after.items.len() - 1,
                    "existing selections keep their indices in {}",
                    after.label
                );
            }
        }
        let all = new.iter().find(|tab| tab.label == "All").unwrap();
        assert_eq!(all.items.last().unwrap().name, "Open Autopilot picker");
    }
}
