// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// Use a small aggregate budget through the same production reader, so the
/// boundary controls do not allocate a quarter GiB per test.
async fn lines(bytes: &[u8], maximum: usize) -> Result<Reply, Error> {
    let mut reader = bytes;
    read_reply_bounded(&mut reader, Framing::Lines, MAX_LINE_BYTES, maximum).await
}

#[tokio::test]
async fn aggregate_limit_counts_wire_bytes_and_preserves_following_reply() {
    // UTF-8 bytes, CRLF and empty lines all consume body budget; the header does not.
    let mut reader = &b"OK 3\n\xc3\xa9\r\n\nend\nOK next\n"[..];
    let reply = read_reply_bounded(&mut reader, Framing::Lines, MAX_LINE_BYTES, 9)
        .await
        .expect("exact aggregate limit");
    assert_eq!(reply.lines, ["é", "", "end"]);
    assert_eq!(
        read_line(&mut reader).await.unwrap().as_deref(),
        Some("OK next")
    );

    for maximum in 0..9 {
        assert!(
            matches!(
                lines(b"OK 3\n\xc3\xa9\r\n\nend\n", maximum).await,
                Err(Error::Malformed(_))
            ),
            "budget {maximum} must reject the same complete frame"
        );
    }

    // Preserve the reader's lossy decoding contract: budget the original wire
    // byte, even when its replacement character occupies more UTF-8 bytes.
    assert_eq!(lines(b"OK 1\n\xff\n", 2).await.unwrap().lines, ["�"]);
    assert!(matches!(
        lines(b"OK 1\n\xff\n", 1).await,
        Err(Error::Malformed(_))
    ));
}

#[tokio::test]
async fn aggregate_limit_stops_before_allocating_an_over_budget_line() {
    let mut reader = &b"OK 2\none\n01234567890123456789\n"[..];
    let error = read_reply_bounded(&mut reader, Framing::Lines, MAX_LINE_BYTES, 8)
        .await
        .expect_err("the second line exceeds remaining body budget");
    assert!(matches!(error, Error::Malformed(_)));
    assert_eq!(reader, b"4567890123456789\n");
}

#[tokio::test]
async fn an_empty_reply_needs_no_body_budget_and_errors_have_no_body() {
    assert!(lines(b"OK 0\n", 0).await.unwrap().lines.is_empty());
    assert_eq!(lines(b"OK 1\n\n", 1).await.unwrap().lines, [""]);
    assert!(matches!(lines(b"OK 1\n", 1).await, Err(Error::Closed)));
    assert!(matches!(lines(b"OK 1\nabc", 8).await, Err(Error::Closed)));
    assert_eq!(
        lines(b"ERR denied\n", 0).await.unwrap().status,
        "ERR denied"
    );
}

#[tokio::test]
async fn aggregate_budget_does_not_change_status_or_bytes_framing() {
    for framing in [Framing::Status, Framing::Bytes] {
        let mut reader = &b"OK 3\nabc"[..];
        let reply = read_reply_bounded(&mut reader, framing, MAX_LINE_BYTES, 0)
            .await
            .expect("the Lines aggregate budget belongs only to Lines framing");
        if framing == Framing::Bytes {
            assert_eq!(reply.bytes, b"abc");
            assert!(reader.is_empty());
        } else {
            assert_eq!(reader, b"abc");
        }
    }
}
