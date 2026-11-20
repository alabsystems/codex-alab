// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

#[test]
fn command_failure_prints_only_an_observed_exit_code() {
    use super::alab_renderer::failed_command_line;

    assert_eq!(failed_command_line(None, ""), " failed:");
    assert_eq!(failed_command_line(Some(0), ""), " failed (exit 0):");
    assert_eq!(
        failed_command_line(Some(-1), " in 5ms"),
        " exited -1 in 5ms:"
    );
}
