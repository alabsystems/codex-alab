// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::initial_bound;

#[test]
fn delayed_reflog_writes_never_advance_the_initialization_bound() {
    assert_eq!(initial_bound(100, (100, 42)), Some((100, 42)));
    assert_eq!(initial_bound(100, (101, 42)), Some((100, 999_999_999)));
    assert_eq!(initial_bound(100, (200, 42)), Some((100, 999_999_999)));
    assert_eq!(initial_bound(101, (100, 42)), None);
    assert_eq!(initial_bound(0, (100, 42)), None);
}
