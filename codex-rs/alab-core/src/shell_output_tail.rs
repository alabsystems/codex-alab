// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use serde::Serializer;
use serde::ser::SerializeStruct;

pub const SHELL_OUTPUT_PREFIX_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SHELL_OUTPUT_TAIL_BYTES: usize = 4096;

/// A nonempty, nonoverlapping suffix of the same raw stream as the retained prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellOutputTail {
    start: u64,
    bytes: Vec<u8>,
}

impl ShellOutputTail {
    pub fn from_bytes(start_byte: u64, bytes: Vec<u8>) -> Option<Self> {
        if start_byte < SHELL_OUTPUT_PREFIX_BYTES as u64
            || bytes.is_empty()
            || bytes.len() > MAX_SHELL_OUTPUT_TAIL_BYTES
            || start_byte.checked_add(bytes.len() as u64).is_none()
        {
            return None;
        }
        Some(Self {
            start: start_byte,
            bytes,
        })
    }

    pub fn start_byte(&self) -> u64 {
        self.start
    }

    pub fn end_byte(&self) -> u64 {
        self.start + self.bytes.len() as u64
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// A single-line JSON record for tool text. Shell data cannot close a surrounding tag.
    pub fn json_text(&self) -> String {
        serde_json::json!(self)
            .to_string()
            .replace('<', "\\u003c")
            .replace('>', "\\u003e")
            .replace('&', "\\u0026")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029")
    }
}

impl Serialize for ShellOutputTail {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_struct("ShellOutputTail", 4)?;
        object.serialize_field("start_byte", &self.start_byte())?;
        object.serialize_field("end_byte", &self.end_byte())?;
        match std::str::from_utf8(&self.bytes) {
            Ok(text) if escaped_text_bytes(text) <= 2 * MAX_SHELL_OUTPUT_TAIL_BYTES => {
                object.serialize_field("encoding", "utf8")?;
                object.serialize_field("data", text)?;
            }
            _ => {
                object.serialize_field("encoding", "base64")?;
                object.serialize_field("data", &STANDARD.encode(&self.bytes))?;
            }
        }
        object.end()
    }
}

// Tagged tool data escapes markup, line separators, quotes and control characters.
// Keep control-heavy valid UTF-8 inside ordinary history budgets with padded base64.
fn escaped_text_bytes(text: &str) -> usize {
    text.chars()
        .map(|c| match c {
            '<' | '>' | '&' | '\u{2028}' | '\u{2029}' => 6,
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
            other if other < ' ' => 6,
            other => other.len_utf8(),
        })
        .sum()
}

#[cfg(test)]
#[path = "shell_output_tail_tests.rs"]
mod tests;
