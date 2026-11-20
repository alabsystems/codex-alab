// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde_json::Value;

#[test]
fn shell_output_tail_encoding_is_lossless_and_fits_ordinary_history_budgets() {
    let start = SHELL_OUTPUT_PREFIX_BYTES as u64;
    for (bytes, encoding) in [
        ("compiler: <error>\n\u{2028}".as_bytes().to_vec(), "utf8"),
        (vec![0xff; MAX_SHELL_OUTPUT_TAIL_BYTES], "base64"),
        (vec![0; MAX_SHELL_OUTPUT_TAIL_BYTES], "base64"),
        (vec![b'<'; MAX_SHELL_OUTPUT_TAIL_BYTES], "base64"),
    ] {
        let tail = ShellOutputTail::from_bytes(start, bytes.clone()).unwrap();
        let text = tail.json_text();
        assert!(!text.contains(['\n', '<', '>', '\u{2028}']));
        assert!(text.len() < 8192);
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["encoding"], encoding);
        assert_eq!(value["start_byte"], start);
        assert_eq!(value["end_byte"], start + bytes.len() as u64);
        let data = value["data"].as_str().unwrap();
        let actual = if encoding == "base64" {
            STANDARD.decode(data).unwrap()
        } else {
            data.as_bytes().to_vec()
        };
        assert_eq!(actual, bytes);
    }
    assert!(ShellOutputTail::from_bytes(start - 1, vec![b'x']).is_none());
    assert!(ShellOutputTail::from_bytes(start, Vec::new()).is_none());
    assert!(ShellOutputTail::from_bytes(u64::MAX, vec![b'x']).is_none());
    assert!(
        ShellOutputTail::from_bytes(start, vec![b'x'; MAX_SHELL_OUTPUT_TAIL_BYTES + 1]).is_none()
    );
}
