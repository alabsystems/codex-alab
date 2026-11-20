// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn title_check(items: Vec<String>) -> DoctorCheck {
    terminal_title_check_from_inputs(TerminalTitleInputs {
        configured_items: Some(items),
        cwd: PathBuf::from("/workspace/project"),
        project_root: None,
    })
}

#[test]
fn doctor_accepts_the_actual_packaged_title_defaults() {
    let defaults: toml::Value =
        toml::from_str(include_str!("../codex-rs/alab-core/defaults.toml")).unwrap();
    let items = defaults["tui"]["terminal_title"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect();
    let check = title_check(items);
    assert_eq!(check.status, CheckStatus::Ok, "{:?}", check.details);
    assert!(check.issues.is_empty());
    assert!(
        check
            .details
            .iter()
            .any(|line| line.contains("thread-name"))
    );
}

#[test]
fn accepting_a_named_thread_does_not_accept_unknown_items() {
    let check = title_check(vec!["thread-name".into(), "unknown-title-item".into()]);
    assert_eq!(check.status, CheckStatus::Warning);
    assert!(
        check
            .details
            .contains(&"terminal title items: thread-name".into())
    );
    assert!(
        check
            .details
            .contains(&"terminal title invalid items: \"unknown-title-item\"".into())
    );
    assert_eq!(check.issues.len(), 1);
}
