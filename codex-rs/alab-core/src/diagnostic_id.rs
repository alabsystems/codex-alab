// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Select whole, bounded correlation IDs from already decoded header values.

/// Select the first whole ID of 1..=128 ASCII letters, digits, hyphens or underscores.
/// Invalid values do not hide later usable metadata. Never trim, split or truncate
/// a value into a different identifier. Callers retain header-name precedence.
pub fn first_safe<'a>(values: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    values.into_iter().find(|value| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    })
}

#[cfg(test)]
mod tests {
    use super::first_safe;

    #[test]
    fn invalid_values_do_not_hide_the_first_safe_whole_id() {
        assert_eq!(
            first_safe(["", "invalid id", "req_first-1", "req_second-2"]),
            Some("req_first-1")
        );
        assert_eq!(
            first_safe(["req_first-1", "req_second-2", "invalid id"]),
            Some("req_first-1")
        );
        assert_eq!(first_safe(std::iter::empty()), None);
    }

    #[test]
    fn whitespace_punctuation_and_unicode_never_become_diagnostic_ids() {
        for value in [
            "",
            " ",
            " req",
            "req ",
            "req\t",
            "req\n",
            "req,other",
            "req.other",
            "req/other",
            "req:other",
            "réq",
        ] {
            assert_eq!(first_safe([value]), None, "{value:?}");
        }
    }

    #[test]
    fn byte_boundary_is_exact_and_oversize_values_are_not_truncated() {
        let maximum = "a".repeat(128);
        let oversized = format!("{maximum}b");
        assert_eq!(first_safe([maximum.as_str()]), Some(maximum.as_str()));
        assert_eq!(first_safe([oversized.as_str()]), None);
        assert_eq!(first_safe([oversized.as_str(), "later"]), Some("later"));
    }
}
