// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use codex_protocol::error::CodexErr;

pub(super) fn opaque_bad_request(body: &str) -> Option<CodexErr> {
    classify(codex_alab_core::active(), body)
}

fn classify(active: bool, body: &str) -> Option<CodexErr> {
    (active && codex_alab_core::request_error::opaque_bad_request(body)).then(|| {
        // Stream uses the existing bounded retry, transport fallback and exhausted
        // retry recovery paths. Preserve the backend's diagnostic in that channel.
        CodexErr::Stream(format!(
            "{}{body}",
            codex_alab_core::request_error::OPAQUE_BAD_REQUEST_PREFIX
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::error::CodexErrorDetails;

    #[test]
    fn opaque_backend_error_uses_bounded_transport_retry() {
        let body = r#"{"detail":"Bad Request"}"#;
        let error = classify(true, body).expect("opaque backend response");
        assert!(matches!(error.details(), CodexErrorDetails::Stream(_)));
        assert!(error.retry_delay(1).is_some());
        assert!(error.to_string().contains("HTTP 400"));
        assert!(classify(false, body).is_none());
        assert!(classify(true, r#"{"error":{"code":"invalid_prompt"}}"#).is_none());
    }

    #[test]
    fn http_bridge_retries_only_the_opaque_400_and_keeps_server_advice() {
        const CHILD: &str = "ALAB_OPAQUE_REQUEST_BRIDGE_TEST";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "api_bridge::alab_request_recovery::tests::http_bridge_retries_only_the_opaque_400_and_keeps_server_advice",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        // Activation lives in a child so upstream tests retain their own behavior.
        codex_alab_core::activate();
        let advice = codex_http_client::RetryAfter::from_delay(std::time::Duration::from_secs(7));
        for (body, opaque) in [
            (r#"{"detail":"Bad Request"}"#, true),
            (r#"{"error":{"code":"invalid_prompt"}}"#, false),
            (r#"{"detail":"Bad Request","code":"denied"}"#, false),
        ] {
            let error = crate::api_bridge::map_api_error(crate::error::ApiError::Transport(
                crate::TransportError::Http {
                    status: http::StatusCode::BAD_REQUEST,
                    url: None,
                    headers: None,
                    body: Some(body.into()),
                    retry_after: advice,
                },
            ));
            assert_eq!(
                matches!(error.details(), CodexErrorDetails::Stream(_)),
                opaque
            );
            assert_eq!(error.retry_delay(1).is_some(), opaque);
            assert_eq!(error.retry_after(), advice);
        }
    }
}
