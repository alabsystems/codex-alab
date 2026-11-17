// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual production local-admission transition. No Native/controller result.
use super::*;
#[test]
fn local_close_admission_failure_is_terminal_instead_of_an_unnotified_empty_result() {
    let mut state = Progress {
        started: false,
        result: None,
        pending: None,
    };
    state.reject_before_spawn(&io::Error::new(
        io::ErrorKind::BrokenPipe,
        "actual close gate refused local registration",
    ));
    assert!(matches!(&state.result,Some(Err(failure)) if failure.kind==io::ErrorKind::BrokenPipe));
    state.reject_before_spawn(&io::Error::other(
        "later observation must not replace original failure",
    ));
    assert!(matches!(&state.result,Some(Err(failure)) if failure.kind==io::ErrorKind::BrokenPipe));
    assert!(!state.started);
}
#[test]
fn begun_operation_cannot_be_reclassified_as_definitely_not_started_by_local_refusal() {
    let mut state = Progress {
        started: true,
        result: None,
        pending: None,
    };
    state.reject_before_spawn(&io::Error::other("late local close"));
    assert!(state.result.is_none());
    assert!(state.started);
}
