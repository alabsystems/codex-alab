// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The text of aterm's control wire: the percent escape, `key=value` status lines,
//! owner tokens and single-line text.
//!
//! aterm's escape (`aterm_codec::pct`, re-exported as `aterm_control::wire::pct_encode`)
//! writes every byte outside `0x21..=0x7E`, and `%` itself, as uppercase `%XX`, so a value
//! is one space-free token. `-` (bare, unescaped) means unset; `%2D` is a literal `-`.

use std::collections::BTreeMap;

/// The total inverse of aterm's escape: `%XX` (either case) becomes its byte, anything
/// else (a lone `%`, `%zz`) passes through unchanged; bytes that are not UTF-8 become
/// U+FFFD.
pub fn pct_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(hi), Some(lo)) = (
                bytes.get(i + 1).copied().and_then(hex_val),
                bytes.get(i + 2).copied().and_then(hex_val),
            )
        {
            out.push(hi << 4 | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// aterm's escape: every byte outside `0x21..=0x7E`, and `%`, as uppercase `%XX`.
pub fn pct_encode(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if (0x21..=0x7E).contains(&b) && b != b'%' {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(b >> 4)]));
            out.push(char::from(HEX[usize::from(b & 0xF)]));
        }
    }
    out
}

/// The `key=value` tokens of a status line (the text after `OK `), raw (still escaped).
/// A token without `=` is kept with an empty value; a repeated key keeps its first value.
pub fn parse_fields(rest: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for tok in rest.split_ascii_whitespace() {
        let (k, v) = tok.split_once('=').unwrap_or((tok, ""));
        fields.entry(k.to_string()).or_insert_with(|| v.to_string());
    }
    fields
}

/// A raw field read as aterm means it: `None` when absent or bare `-`, else decoded.
pub fn field(fields: &BTreeMap<String, String>, key: &str) -> Option<String> {
    match fields.get(key).map(String::as_str) {
        None | Some("-") => None,
        Some(raw) => Some(pct_decode(raw)),
    }
}

/// An owner token as aterm's `valid_owner_token` takes it: 1..64 bytes of
/// `[A-Za-z0-9._:@/-]`, never `-` alone.
pub fn valid_owner_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s != "-"
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'@' | b'/' | b'-')
        })
}

/// `text` as one line aterm accepts for attention or a notice: every control, bidi or
/// invisible formatting character becomes a space, runs of spaces collapse, and the
/// result is cut on a character boundary to at most `max_bytes`.
pub fn single_line(text: &str, max_bytes: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max_bytes));
    let mut last_space = true;
    for c in text.chars() {
        let c = if c.is_control() || is_format_char(c) || c.is_whitespace() {
            ' '
        } else {
            c
        };
        if c == ' ' {
            if last_space {
                continue;
            }
            last_space = true;
        } else {
            last_space = false;
        }
        if out.len() + c.len_utf8() > max_bytes {
            break;
        }
        out.push(c);
    }
    out.trim_end().to_string()
}

/// Bidi controls and invisible formatting characters a terminal reorders text around.
fn is_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// A `retry_ms=<n>` token on a busy refusal, if any.
pub fn retry_ms(line: &str) -> Option<u64> {
    line.split_ascii_whitespace()
        .find_map(|t| t.strip_prefix("retry_ms="))
        .and_then(|v| v.parse().ok())
}
