// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! A chosen UI tip may outlive the build whose static inventory contained it.
//! Original selections borrow their literal; restored selections own exact text.
pub type Template = std::borrow::Cow<'static, str>;

pub fn literal(value: &'static str) -> Template {
    Template::Borrowed(value)
}
