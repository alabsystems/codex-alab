// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Inspect native fixture payloads without changing production transport settings.
use std::io;
use std::io::Read;

/// A headerless fixture capture holds a JSON body or a standard complete zstd frame.
/// Decode every request independently; never carry a stream's history into another.
pub fn recorded_request_body(body: &[u8], limit: usize) -> io::Result<Vec<u8>> {
    let too_large = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "recorded request exceeds fixture limit",
        )
    };
    if !body.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        return if body.len() <= limit {
            Ok(body.to_vec())
        } else {
            Err(too_large())
        };
    }
    let mut decoder = zstd::stream::read::Decoder::new(body)?;
    decoder.window_log_max(23)?;
    let mut bytes = Vec::new();
    decoder
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(too_large());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_plain_and_compressed_requests_preserve_the_same_bytes() {
        let body = br#"{"input":"actual fixture payload"}"#;
        let frame = zstd::stream::encode_all(&body[..], 3).unwrap();
        assert_eq!(recorded_request_body(body, body.len()).unwrap(), body);
        assert_eq!(recorded_request_body(&frame, body.len()).unwrap(), body);
        assert_eq!(
            recorded_request_body(&frame, body.len()).unwrap(),
            body,
            "each decode starts fresh"
        );
    }

    #[test]
    fn corrupt_and_oversized_recorded_frames_are_refused() {
        assert!(recorded_request_body(&[0x28, 0xb5, 0x2f, 0xfd, 0xff], 1024).is_err());
        let body = vec![b'x'; 4096];
        let frame = zstd::stream::encode_all(&body[..], 3).unwrap();
        assert!(recorded_request_body(&body, body.len() - 1).is_err());
        assert!(recorded_request_body(&frame, body.len() - 1).is_err());
    }
}
