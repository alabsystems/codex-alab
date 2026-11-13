// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Narrow scoping for exact inline assertions in helpers called repeatedly by tests.

/// Allow a shared fixture assertion to run again while retaining its inline expectation.
/// `insta` resolves in the calling test crate; the product has no dependency on it.
#[cfg(feature = "test-support")]
#[macro_export]
macro_rules! allow_repeated_inline_snapshot {
    ($($assertion:tt)*) => {
        ::insta::allow_duplicates! { $($assertion)* }
    };
}
