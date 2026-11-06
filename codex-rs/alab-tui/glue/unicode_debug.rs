// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep a render fixture independent of Rust's Unicode Debug escape tables.

use ratatui::buffer::Buffer;
use ratatui::text::Line;

pub(super) fn render_snapshot(buffer: &Buffer, lines: &[Line<'_>]) -> String {
    // The rendered cells, geometry and styles retain their exact original Debug.
    format!("{buffer:?}\n{}", unicode_debug(&format!("{lines:#?}")))
}

#[track_caller]
pub(super) fn assert_render_snapshot(buffer: &Buffer, lines: &[Line<'_>], reference: &str) {
    assert_eq!(
        render_snapshot(buffer, lines),
        reference_snapshot(reference)
    );
}

fn reference_snapshot(reference: &str) -> String {
    let reference = reference.replace("\r\n", "\n");
    let (_, body) = reference
        .strip_prefix("---\n")
        .and_then(|reference| reference.split_once("\n---\n"))
        .expect("the render reference has an insta metadata header");
    let (buffer, lines) = body
        .trim_end_matches('\n')
        .split_once("\n[")
        .expect("the render reference has separate Buffer and Line debug sections");
    // Upstream may regenerate its reference with another compiler's escaping.
    // Canonicalize only the Line section on both sides; Buffer remains exact.
    format!("{buffer}\n[{}", unicode_debug(lines))
}

fn unicode_debug(debug: &str) -> String {
    let mut output = String::with_capacity(debug.len());
    let mut characters = debug.chars();
    let mut quoted = false;
    while let Some(character) = characters.next() {
        if character == '"' {
            quoted = !quoted;
        } else if quoted && character == '\\' {
            let Some(escaped) = characters.next() else {
                output.push(character);
                break;
            };
            // Consume each escape as one token. In particular, `\\\\u{...}`
            // spells literal backslash text and must not become a scalar.
            if escaped == 'u' {
                let mut probe = characters.clone();
                if probe.next() == Some('{') {
                    let mut digits = String::new();
                    let mut closed = false;
                    for digit in probe.by_ref() {
                        if digit == '}' {
                            closed = true;
                            break;
                        }
                        if !digit.is_ascii_hexdigit() || digits.len() == 6 {
                            break;
                        }
                        digits.push(digit);
                    }
                    if let Some(scalar) = closed
                        .then(|| u32::from_str_radix(&digits, 16).ok())
                        .flatten()
                        .and_then(char::from_u32)
                        .filter(|scalar| !scalar.is_ascii() && !scalar.is_control())
                    {
                        output.push(scalar);
                        characters = probe;
                        continue;
                    }
                }
            }
            output.push(character);
            output.push(escaped);
            continue;
        }
        output.push(character);
    }
    output
}

#[test]
fn unicode_spelling_is_portable_without_a_character_registry() {
    assert_eq!(
        unicode_debug(r#"Span::from("ｶ\u{ff9e} ﾊ\u{ff9f} e\u{301}")"#),
        "Span::from(\"ｶﾞ ﾊﾟ e\u{301}\")"
    );
    let raw = "Span::from(\"ｶﾞ ﾊﾟ e\u{301}\")";
    assert_eq!(unicode_debug(raw), raw);
}

#[test]
fn literal_backslashes_quotes_and_control_escapes_stay_intact() {
    let literal = r#"Span::from("ｶ\\u{ff9e} \\\\u{ff9f} \n \u{85}")"#;
    assert_eq!(unicode_debug(literal), literal);
    assert_eq!(
        unicode_debug(r#"Span::from("quote: \"ｶ\u{ff9e}\", literal: \\u{ff9e}")"#),
        r#"Span::from("quote: \"ｶﾞ\", literal: \\u{ff9e}")"#
    );
    let invalid = r#"Span::from("\\u{not-a-scalar} \u{110000} \u{1234567}")"#;
    assert_eq!(unicode_debug(invalid), invalid);
}

#[test]
fn reference_normalization_keeps_glyph_cells_geometry_and_styles() {
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use ratatui::style::Style;

    let buffer = Buffer::empty(Rect::new(0, 0, 5, 1));
    let lines = [Line::from("abｶﾞ…")];
    let escaped_lines = format!("{lines:#?}").replace('\u{ff9e}', r"\u{ff9e}");
    let reference = format!("---\nsource: fixture\n---\n{buffer:?}\n{escaped_lines}\n");
    assert_render_snapshot(&buffer, &lines, &reference);
    let expected = reference_snapshot(&reference);
    assert!(expected.starts_with(&format!("{buffer:?}\n")));
    // Keep cells fixed while changing the line, so its glyph cannot disappear
    // merely because the separate Buffer snapshot still detects the mutation.
    assert_ne!(expected, render_snapshot(&buffer, &[Line::from("abﾊﾟ…")]));
    let narrower = Buffer::empty(Rect::new(0, 0, 4, 1));
    assert_ne!(expected, render_snapshot(&narrower, &lines));
    let mut wrong_cell = buffer.clone();
    wrong_cell.set_string(0, 0, "x", Style::default());
    assert_ne!(expected, render_snapshot(&wrong_cell, &lines));
    let mut wrong_style = buffer;
    wrong_style.set_style(wrong_style.area, Style::default().fg(Color::Cyan));
    assert_ne!(expected, render_snapshot(&wrong_style, &lines));
}
