// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Reject cookie framing characters while preserving upstream opaque comma tokens.

use codex_model_provider_info::GatewayOAuthDelivery;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;

pub(super) fn validate(delivery: &GatewayOAuthDelivery, token: &str) -> Result<()> {
    if matches!(delivery, GatewayOAuthDelivery::Cookie { .. })
        && !token
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b';' | b'"' | b'\\'))
    {
        return Err(CodexErr::InvalidRequest(
            "gateway access token contains invalid cookie framing characters".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_cookie_rejects_injected_pairs_without_echoing_credentials() {
        let cookie = GatewayOAuthDelivery::Cookie {
            name: "gateway".into(),
        };
        for token in [
            "secret;admin=true",
            "secret; admin=true",
            "secret\r\nCookie: admin=true",
            "secret\\value",
            "\"secret\"",
            "secret value",
            "secret\tvalue",
            "secreté",
        ] {
            let error = super::super::gateway_header(&cookie, token)
                .unwrap_err()
                .to_string();
            assert!(!error.contains(token));
            assert!(error.contains("cookie framing"));
        }
    }

    #[test]
    fn gateway_cookie_preserves_opaque_commas_and_sensitive_delivery() {
        let cookie = GatewayOAuthDelivery::Cookie {
            name: "gateway".into(),
        };
        let (name, value) = super::super::gateway_header(&cookie, "opaque,token=payload").unwrap();
        assert_eq!(name, http::header::COOKIE);
        assert_eq!(value, "gateway=opaque,token=payload");
        assert!(value.is_sensitive());
        let header = GatewayOAuthDelivery::Header {
            name: "x-gateway-auth".into(),
            scheme: "Bearer".into(),
        };
        let (_, value) = super::super::gateway_header(&header, "opaque;token").unwrap();
        assert_eq!(value, "Bearer opaque;token");
        assert!(value.is_sensitive());
    }
}
