// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn only_capacity_and_interruptions_survive_both_compiler_boundaries() {
    assert!(require_success("compiler", ExitStatus::from_raw(0)).is_ok());
    for (raw, expected) in [
        (2 << 8, 1),
        (101 << 8, 1),
        (128 << 8, 1),
        (75 << 8, 75),
        (libc::SIGTERM, 128 + libc::SIGTERM),
        (143 << 8, 143),
        (255 << 8, 255),
    ] {
        let worker = require_success("compiler", ExitStatus::from_raw(raw)).unwrap_err();
        let code = exit_code(worker.as_ref()).unwrap();
        assert_eq!(code, expected);
        let publisher = require_success("publisher", ExitStatus::from_raw(code << 8)).unwrap_err();
        assert_eq!(exit_code(publisher.as_ref()), Some(expected));
    }
}
