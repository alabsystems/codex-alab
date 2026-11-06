// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The complete current atpkg Store prelude, recognized as data, never executed.
//! Unknown producer statements retain direct-launch semantics. This is a format
//! boundary, not a shell interpreter or an authority to change the launch policy.

use std::path::Path;

pub(super) const STORE_PRELUDE: &str = include_str!("managed_primary_store_prelude.txt");

pub(super) fn statements(body: &str) -> Option<String> {
    if body
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return None;
    }
    let mut statements = String::new();
    for line in body.split('\n') {
        if !line.is_empty() && !line.trim_start_matches([' ', '\t']).starts_with('#') {
            statements.push_str(line);
            statements.push('\n');
        }
    }
    Some(statements)
}

pub(super) fn after_store_prelude<'a>(body: &'a str, prefix: &Path) -> Option<&'a str> {
    let template = statements(STORE_PRELUDE)?;
    let (head, after_atpkg) = template.split_once("{{ATPKG}}")?;
    let (between, tail) = after_atpkg.split_once("{{PREFIX}}")?;
    let after_head = body.strip_prefix(head)?;
    let (quoted_atpkg, _) = after_head.split_once('\n')?;
    let atpkg = super::shell_value(quoted_atpkg)?;
    // An empty binding is the producer's supported "no embedded atpkg" arm.
    // A nonempty one is one literal absolute path, never a command expansion.
    if atpkg.chars().any(char::is_control)
        || (!atpkg.is_empty() && !super::normalized_absolute(Path::new(&atpkg)))
    {
        return None;
    }
    let expected = format!(
        "{head}{quoted_atpkg}{between}{}{tail}",
        super::shell_quote(prefix)?,
    );
    body.strip_prefix(&expected)
}
