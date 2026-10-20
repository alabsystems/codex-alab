// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use pretty_assertions::assert_eq;

const UPSTREAM: &str = "a = 1\n[history]\npersistence = \"save-all\"\n";

#[test]
fn alab_tables_merge_into_upstreams() {
    let merged = crate::merge_defaults(UPSTREAM, "b = 2\n[history]\nmax_bytes = 9\n[tui]\nx = true\n")
        .unwrap_or_else(|| panic!("merge"));
    let merged: toml::Table = toml::from_str(&merged).unwrap_or_else(|err| panic!("{err}"));
    let expected: toml::Table = toml::from_str(
        "a = 1\nb = 2\n[history]\npersistence = \"save-all\"\nmax_bytes = 9\n[tui]\nx = true\n",
    )
    .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(merged, expected);
}

#[test]
fn alab_values_win_over_upstreams() {
    let merged = crate::merge_defaults(UPSTREAM, "a = 3\n").unwrap_or_else(|| panic!("merge"));
    let merged: toml::Table = toml::from_str(&merged).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(merged.get("a"), Some(&toml::Value::Integer(3)));
}

#[test]
fn alabs_own_defaults_parse() {
    let parsed: Result<toml::Table, _> = toml::from_str(crate::ALAB_DEFAULTS);
    assert!(parsed.is_ok(), "alab-core/defaults.toml: {parsed:?}");
}

#[test]
fn the_aterm_only_defaults_are_set_only_inside_aterm() {
    let tui = |in_aterm| {
        let merged = crate::alab_defaults(UPSTREAM, in_aterm).unwrap_or_else(|| panic!("merge"));
        let merged: toml::Table = toml::from_str(&merged).unwrap_or_else(|err| panic!("{err}"));
        merged.get("tui").and_then(toml::Value::as_table).cloned().unwrap_or_default()
    };
    let (inside, outside) = (tui(true), tui(false));
    assert_eq!(inside.get("pet").and_then(toml::Value::as_str), Some("alab-kitty"));
    assert_eq!(inside.get("notification_method").and_then(toml::Value::as_str), Some("osc9"));
    assert_eq!(inside.get("disable_paste_burst").and_then(toml::Value::as_bool), Some(true));
    assert_eq!(inside.get("alab_progress").and_then(toml::Value::as_bool), Some(true));
    // Elsewhere each stays upstream's: no pet (so nothing is written into CODEX_HOME for
    // one), the terminal's own notification method, paste-burst detection on, no OSC 9;4 bar.
    for key in ["pet", "notification_method", "disable_paste_burst", "alab_progress"] {
        assert_eq!(outside.get(key), None, "[tui] {key} outside aterm");
    }
    assert!(crate::is_aterm(" ATerm "));
    assert!(!crate::is_aterm("iTerm.app"));
}

#[test]
fn inactive_means_upstream() {
    // This test binary never activates: every switch reads upstream's behavior.
    assert!(!crate::active());
    assert_eq!(crate::packaged_defaults(UPSTREAM), UPSTREAM);
    assert!(crate::trace_diagnostics());
    assert!(crate::feedback_tags_logged());
    assert!(crate::turn_git_metadata());
}
