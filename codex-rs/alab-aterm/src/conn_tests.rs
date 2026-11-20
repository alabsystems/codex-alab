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

#[tokio::test]
async fn status_obeys_its_own_wire_limit_and_has_no_body() {
    // Five bytes before LF, including CR. The advertised-looking number is status text.
    let mut exact = &b"OK 3\r\nOK next\n"[..];
    let reply = read_reply_bounded(&mut exact, Framing::Status, 5, 0)
        .await
        .unwrap();
    assert_eq!(reply.status, "OK 3");
    assert!(reply.lines.is_empty() && reply.bytes.is_empty());
    assert_eq!(exact, b"OK next\n");

    let mut over = &b"OK 3\r\nOK next\n"[..];
    assert!(matches!(
        read_reply_bounded(&mut over, Framing::Status, 4, usize::MAX).await,
        Err(Error::Malformed(_))
    ));
    assert_eq!(over, b"\nOK next\n");
}

#[tokio::test]
async fn status_and_lines_stop_at_the_default_line_limit() {
    for framing in [Framing::Status, Framing::Lines] {
        let header = match framing {
            Framing::Lines => b"OK 1\n".as_slice(),
            _ => b"".as_slice(),
        };
        // Generate the first excessive byte on demand. The wire may contain more after it.
        let body = tokio::io::repeat(b'x').take((8 << 20) + 1);
        let mut reader = BufReader::new(header.chain(body).chain(b"\nOK next\n".as_slice()));
        assert!(matches!(
            read_reply(&mut reader, framing, MAX_LINE_BYTES).await,
            Err(Error::Malformed(_))
        ));
        assert_eq!(read_line(&mut reader).await.unwrap().as_deref(), Some(""));
        assert_eq!(
            read_line(&mut reader).await.unwrap().as_deref(),
            Some("OK next")
        );
    }
}

#[tokio::test]
async fn bytes_enforce_their_size_limit_and_consume_exactly_the_declared_body() {
    // The full, valid boundary body is binary and generated on demand; the Lines budget is zero.
    let body = tokio::io::repeat(0xff).take(1 << 20);
    let mut reader = BufReader::new(
        b"OK 1048576 field=value\r\n"
            .as_slice()
            .chain(body)
            .chain(b"OK next\n".as_slice()),
    );
    let reply = read_reply_bounded(&mut reader, Framing::Bytes, MAX_LINE_BYTES, 0)
        .await
        .unwrap();
    assert_eq!(reply.status, "OK 1048576 field=value");
    assert!(reply.lines.is_empty());
    assert_eq!(reply.bytes.len(), 1 << 20);
    assert!(reply.bytes.iter().all(|&byte| byte == 0xff));
    assert_eq!(
        read_line(&mut reader).await.unwrap().as_deref(),
        Some("OK next")
    );

    let mut over = &b"OK 1048577\nuntouched"[..];
    assert!(matches!(
        read_reply(&mut over, Framing::Bytes, MAX_LINE_BYTES).await,
        Err(Error::Malformed(_))
    ));
    assert_eq!(over, b"untouched");
    let mut truncated = &b"OK 3\nx"[..];
    assert!(matches!(
        read_reply(&mut truncated, Framing::Bytes, MAX_LINE_BYTES).await,
        Err(Error::Closed)
    ));
}

#[tokio::test]
async fn lines_enforce_the_announced_count_before_reading_any_body() {
    let mut exact = &b"OK 200000\n"[..];
    assert!(matches!(
        read_reply(&mut exact, Framing::Lines, MAX_LINE_BYTES).await,
        Err(Error::Closed)
    ));
    let mut over = &b"OK 200001\nuntouched"[..];
    assert!(matches!(
        read_reply(&mut over, Framing::Lines, MAX_LINE_BYTES).await,
        Err(Error::Malformed(_))
    ));
    assert_eq!(over, b"untouched");
}
