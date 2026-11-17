// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Ordinary static actions remain borrowed; historical native DATA can own them without leaks.
use std::borrow::Cow;
pub(super) fn static_actions(
    actual: &'static [(&'static str, &'static str)],
) -> Vec<(Cow<'static, str>, Cow<'static, str>)> {
    actual
        .iter()
        .map(|(label, target)| (Cow::Borrowed(*label), Cow::Borrowed(*target)))
        .collect()
}
