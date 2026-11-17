// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original argv selection DATA only; no Native controller/acceptance fixture.
use super::*;
#[test]
fn only_first_native_prefix_selects_the_original_argument_tail() {
    for words in [
        vec!["codex"],
        vec!["codex", "resume"],
        vec!["codex", "--", "--alab-retained-service"],
    ] {
        let args = words.into_iter().map(OsString::from).collect::<Vec<_>>();
        assert!(original_arguments(&args).is_none());
    }
    let args = [
        "codex",
        "--alab-retained-service",
        "resume",
        "--last",
        "--config",
        "model=\"actual\"",
        "--",
        "--alab-retained-service",
    ]
    .map(OsString::from);
    let expected = [&args[..1], &args[2..]].concat();
    assert_eq!(original_arguments(&args), Some(expected));
}
#[test]
fn empty_original_launch_stays_a_real_fresh_cli_not_synthetic_start_params() {
    assert_eq!(
        original_arguments(&["codex".into(), BACKEND.into()]),
        Some(vec!["codex".into()])
    );
}
#[cfg(unix)]
#[test]
fn original_non_utf8_prompt_and_config_bytes_are_never_reconstructed() {
    use std::os::unix::ffi::OsStringExt;
    let original = vec![
        OsString::from("codex"),
        OsString::from_vec(vec![b'p', 0xff]),
        OsString::from("--config"),
        OsString::from_vec(vec![b'x', b'=', 0xfe]),
    ];
    let args = [&original[..1], &[OsString::from(BACKEND)], &original[1..]].concat();
    assert_eq!(original_arguments(&args), Some(original));
}
