// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The backend's opaque 400 is not a diagnosis of an invalid model request.

pub const OPAQUE_BAD_REQUEST_PREFIX: &str = "opaque backend HTTP 400: ";

/// Distinguish a normalized backend error from a reply cut off mid-stream.
pub fn opaque_bad_request_stream(message: &str) -> bool {
    message
        .strip_prefix(OPAQUE_BAD_REQUEST_PREFIX)
        .is_some_and(opaque_bad_request)
}

/// Recognize only the observed backend envelope. A structured provider error,
/// validation detail, or additional field must retain its ordinary classification.
/// Parsing a single-field struct rejects duplicate keys as well as extra fields.
pub fn opaque_bad_request(message: &str) -> bool {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Envelope {
        detail: String,
    }
    serde_json::from_str::<Envelope>(message).is_ok_and(|envelope| envelope.detail == "Bad Request")
}

#[cfg(test)]
mod tests {
    use super::opaque_bad_request;

    #[test]
    fn only_the_opaque_backend_envelope_is_retryable() {
        for message in [
            r#"{"detail":"Bad Request"}"#,
            " { \"detail\" : \"Bad Request\" } ",
        ] {
            assert!(opaque_bad_request(message), "{message}");
        }
        for message in [
            "Bad Request",
            r#"{"detail":"invalid tools"}"#,
            r#"{"error":{"message":"Bad Request","code":"invalid_prompt"}}"#,
            r#"{"detail":"Bad Request","code":"access_denied"}"#,
            r#"{"detail":"access denied","detail":"Bad Request"}"#,
            r#"{"detail":"Bad Request","detail":"Bad Request"}"#,
            r#"{"detail":"Bad Request"} trailing"#,
        ] {
            assert!(!opaque_bad_request(message), "{message}");
        }
    }
}
