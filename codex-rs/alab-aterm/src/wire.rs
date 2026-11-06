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

/// `value` decoded on the wire's strict reader: every `%` followed by two hex digits (either
/// case), the bytes UTF-8; `None` for a lone or bad escape, or bytes that are not UTF-8.
pub fn pct_decode_strict(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = bytes.get(i + 1).copied().and_then(hex_val)?;
            let lo = bytes.get(i + 2).copied().and_then(hex_val)?;
            out.push(hi << 4 | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
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

/// What a line too long for aterm puts where its words gave way ([`elide_before`]).
pub const ELISION: char = '…';

/// `head` then `tail` as one line ([`single_line`]) of at most `max_bytes`, cut on character
/// boundaries: whole where it fits; else the end of `head` gives way to [`ELISION`] and `tail`
/// stays whole, so what the line ends with survives (an act's fact and its why, CONTRACT §5
/// "aterm fact"). Where even `tail` and the mark pass `max_bytes`, the start of `head` and the
/// end of `tail` share the room around the mark.
pub fn elide_before(head: &str, tail: &str, max_bytes: usize) -> String {
    let whole = single_line(&format!("{head}{tail}"), usize::MAX);
    if whole.len() <= max_bytes {
        return whole;
    }
    let mark = ELISION.len_utf8();
    let Some(room) = max_bytes.checked_sub(mark) else {
        return String::new();
    };
    // `tail` keeps the one space it opens with: the seam's, which single_line drops at a start.
    let opens_spaced = tail
        .chars()
        .next()
        .is_some_and(|c| c.is_control() || is_format_char(c) || c.is_whitespace());
    let tail = single_line(tail, usize::MAX);
    let tail = if opens_spaced && !tail.is_empty() {
        format!(" {tail}")
    } else {
        tail
    };
    let (head_room, tail) = if tail.len() <= room {
        (room - tail.len(), tail.as_str())
    } else {
        let head_room = room / 2;
        (head_room, last_bytes(&tail, room - head_room))
    };
    let head = single_line(head, head_room);
    format!("{head}{ELISION}{tail}")
}

/// The longest end of `text` of at most `max_bytes`, starting on a character boundary.
fn last_bytes(text: &str, max_bytes: usize) -> &str {
    let from = text.len().saturating_sub(max_bytes);
    let from = (from..=text.len())
        .find(|&at| text.is_char_boundary(at))
        .unwrap_or(text.len());
    &text[from..]
}

/// Bidi controls and invisible formatting characters a terminal reorders or hides text around
/// (aterm's `aterm_messages::text::is_hidden_char`).
pub fn is_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'..='\u{1160}'
            | '\u{17B4}'..='\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FDD0}'..='\u{FDEF}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF0}'..='\u{FFFB}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E007F}'
    ) || (c as u32) & 0xfffe == 0xfffe
}

/// A canonical unsigned decimal integer: 1.. digits with no `+` or leading `0` (`"0"` allowed).
pub fn canonical_u64(s: &str) -> Option<u64> {
    if s.is_empty() || (s.len() > 1 && s.starts_with('0')) || !s.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    s.parse().ok()
}

/// A `retry_ms=<n>` token on a busy refusal, if any.
pub fn retry_ms(line: &str) -> Option<u64> {
    line.split_ascii_whitespace()
        .find_map(|t| t.strip_prefix("retry_ms="))
        .and_then(canonical_u64)
}
