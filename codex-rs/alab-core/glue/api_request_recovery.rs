// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::TransportError;
use crate::error::ApiError;
use codex_protocol::error::CodexErr;

/// The internal opaque-HTTP marker is a reserved namespace, not provider text.
/// Reserve it for every ALab binary mode so a queued vanilla error cannot gain
/// recovery authority when the live experience later becomes enhanced.
pub(super) fn provider_error(mut error: ApiError) -> ApiError {
    if !codex_alab_core::is_alab_binary() {
        return error;
    }
    match &mut error {
        ApiError::Retryable { message, .. }
        | ApiError::Stream(message)
        | ApiError::RateLimit(message)
        | ApiError::Transport(TransportError::Network(message) | TransportError::Build(message))
            if message.starts_with(codex_alab_core::request_error::OPAQUE_BAD_REQUEST_PREFIX) =>
        {
            message.insert_str(0, "provider stream: ");
        }
        _ => {}
    }
    error
}

pub(super) fn opaque_bad_request(
    body: &str,
    headers: Option<&http::HeaderMap>,
) -> Option<CodexErr> {
    let error = classify(codex_alab_core::active(), body)?;
    // Keep correlation metadata locally without recording request content or
    // arbitrary headers. The transport error otherwise loses these IDs here.
    tracing::info!(
        request_id = correlation_id(headers, "x-request-id")
            .or_else(|| correlation_id(headers, "x-oai-request-id")),
        cf_ray = correlation_id(headers, "cf-ray"),
        "Opaque backend HTTP 400 classified as recoverable"
    );
    Some(error)
}

fn correlation_id<'a>(headers: Option<&'a http::HeaderMap>, name: &str) -> Option<&'a str> {
    let value = headers?.get(name)?.to_str().ok()?;
    // IDs are diagnostic data from an untrusted response. Reject oversized or
    // non-token values rather than truncate them into a misleading identifier.
    (!value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
    .then_some(value)
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
    use std::collections::BTreeMap;
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::Mutex;

    #[derive(Clone, Default)]
    struct Events(Arc<Mutex<Vec<BTreeMap<String, String>>>>);

    #[derive(Default)]
    struct Fields(BTreeMap<String, String>);

    impl tracing::field::Visit for Fields {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.insert(field.name().to_owned(), format!("{value:?}"));
        }
    }

    impl tracing::Subscriber for Events {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            let mut fields = Fields::default();
            event.record(&mut fields);
            self.0.lock().unwrap().push(fields.0);
        }
    }

    #[test]
    fn diagnostic_ids_are_bounded_tokens_from_named_headers() {
        let mut headers = http::HeaderMap::new();
        headers.insert("x-request-id", "req_ab12-CD34".parse().unwrap());
        headers.insert("cf-ray", "1234abcd-SJC".parse().unwrap());
        assert_eq!(
            correlation_id(Some(&headers), "x-request-id"),
            Some("req_ab12-CD34")
        );
        assert_eq!(
            correlation_id(Some(&headers), "cf-ray"),
            Some("1234abcd-SJC")
        );
        assert_eq!(correlation_id(None, "x-request-id"), None);
        assert_eq!(correlation_id(Some(&headers), "x-oai-request-id"), None);
        for value in ["".to_owned(), "contains spaces".to_owned(), "x".repeat(129)] {
            headers.insert("x-request-id", value.parse().unwrap());
            assert_eq!(correlation_id(Some(&headers), "x-request-id"), None);
        }
        headers.insert("x-request-id", "x".repeat(128).parse().unwrap());
        assert_eq!(
            correlation_id(Some(&headers), "x-request-id")
                .unwrap()
                .len(),
            128
        );
    }

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
            let name = format!(
                "{}::http_bridge_retries_only_the_opaque_400_and_keeps_server_advice",
                module_path!().split_once("::").unwrap().1
            );
            let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", &name, "--nocapture"])
                .env(CHILD, "1");
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test child parent runtime")
                .block_on(async {
                    crate::test_child_process::run(command, std::time::Duration::from_secs(30))
                        .await
                        .unwrap_or_else(|failure| panic!("{name}: {failure}"))
                        .assert_test_success(&name);
                });
            return;
        }
        // Activation lives in a child so upstream tests retain their own behavior.
        codex_alab_core::activate();
        let events = Events::default();
        let _subscriber = tracing::subscriber::set_default(events.clone());
        let advice = codex_http_client::RetryAfter::from_delay(std::time::Duration::from_secs(7));
        for (body, opaque) in [
            (r#"{"detail":"Bad Request"}"#, true),
            (r#"["Bad Request"]"#, false),
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
        assert_eq!(events.0.lock().unwrap().len(), 1);
        events.0.lock().unwrap().clear();
        for (request_id, expected) in [
            ("req_primary-1", "req_primary-1"),
            ("invalid identifier", "req_fallback-2"),
        ] {
            let mut headers = http::HeaderMap::new();
            headers.insert("x-request-id", request_id.parse().unwrap());
            headers.insert("x-oai-request-id", "req_fallback-2".parse().unwrap());
            headers.insert("cf-ray", "1234abcd-SJC".parse().unwrap());
            headers.insert("authorization", "Bearer never-log-this".parse().unwrap());
            headers.insert("x-unrelated", "never-log-this-either".parse().unwrap());
            let error = crate::api_bridge::map_api_error(crate::error::ApiError::Transport(
                crate::TransportError::Http {
                    status: http::StatusCode::BAD_REQUEST,
                    url: None,
                    headers: Some(headers),
                    body: Some(r#"{"detail":"Bad Request"}"#.into()),
                    retry_after: advice,
                },
            ));
            assert!(error.retry_delay(1).is_some());
            assert_eq!(error.retry_after(), advice);
            let fields = events.0.lock().unwrap().pop().unwrap();
            assert_eq!(fields.len(), 3);
            assert_eq!(fields["request_id"], format!("{expected:?}"));
            assert_eq!(fields["cf_ray"], "\"1234abcd-SJC\"");
            assert_eq!(
                fields["message"],
                "Opaque backend HTTP 400 classified as recoverable"
            );
        }
        assert!(events.0.lock().unwrap().is_empty());
    }

    const PROVIDER_MARKER_MESSAGE: &str = r#"opaque backend HTTP 400: {"detail":"Bad Request"}"#;
    const BRIDGE_CHILD: &str = "ALAB_PROVIDER_MARKER_BRIDGE_CHILD";
    const BRIDGE_COMPLETE: &str = "ALAB_PROVIDER_MARKER_BRIDGE_OK";

    fn marker_bridge_child(name: &str) -> bool {
        let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
        if let Some(child) = std::env::var_os(BRIDGE_CHILD) {
            assert_eq!(child.to_str(), Some(name));
            assert_eq!(
                std::env::args().skip(1).collect::<Vec<_>>(),
                ["--exact", &exact, "--nocapture", "--test-threads=1"]
            );
            assert!(!codex_alab_core::is_alab_binary());
            return false;
        }
        assert!(!codex_alab_core::is_alab_binary());
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
            .env_clear()
            .env(BRIDGE_CHILD, name);
        for key in ["DYLD_LIBRARY_PATH", "LD_LIBRARY_PATH"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let marker = format!("{BRIDGE_COMPLETE}:{name}\n");
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("provider marker parent runtime")
            .block_on(async {
                crate::test_child_process::run_with_stdout_marker(
                    command,
                    std::time::Duration::from_secs(30),
                    Some(&marker),
                )
                .await
                .unwrap_or_else(|failure| panic!("{exact}: {failure}"))
                .assert_test_success(&exact);
            });
        assert!(!codex_alab_core::is_alab_binary());
        true
    }

    fn marker_bridge_complete(name: &str) {
        std::io::stdout()
            .lock()
            .write_all(format!("{BRIDGE_COMPLETE}:{name}\n").as_bytes())
            .expect("provider marker child completion");
    }

    fn provider_stream_errors(
        message: &str,
        advice: codex_http_client::RetryAfter,
    ) -> [(
        crate::error::ApiError,
        Option<codex_http_client::RetryAfter>,
    ); 5] {
        use crate::error::ApiError;
        [
            (
                ApiError::Retryable {
                    message: message.into(),
                    retry_after: Some(advice),
                },
                Some(advice),
            ),
            (ApiError::Stream(message.into()), None),
            (
                ApiError::Transport(crate::TransportError::Network(message.into())),
                None,
            ),
            (
                ApiError::Transport(crate::TransportError::Build(message.into())),
                None,
            ),
            (ApiError::RateLimit(message.into()), None),
        ]
    }

    fn assert_provider_stream(
        mapped: &CodexErr,
        original: &str,
        advice: Option<codex_http_client::RetryAfter>,
        reserved: bool,
    ) {
        let CodexErrorDetails::Stream(message) = mapped.details() else {
            panic!("provider text retains its Stream identity: {mapped:?}");
        };
        assert_eq!(
            codex_alab_core::request_error::opaque_bad_request_stream(message),
            !reserved && original == PROVIDER_MARKER_MESSAGE,
            "provider text must not acquire the normalized HTTP400 marker"
        );
        let expected = if reserved
            && original.starts_with(codex_alab_core::request_error::OPAQUE_BAD_REQUEST_PREFIX)
        {
            format!("provider stream: {original}")
        } else {
            original.into()
        };
        assert_eq!(message, &expected);
        assert_eq!(
            mapped.to_string(),
            format!("stream disconnected before completion: {expected}")
        );
        assert_eq!(
            mapped.to_codex_protocol_error(),
            codex_protocol::protocol::CodexErrorInfo::Other
        );
        assert_eq!(mapped.http_status_code_value(), None);
        assert!(mapped.retry_delay(1).is_some());
        assert_eq!(mapped.retry_after(), advice, "retain the original deadline");
    }

    #[test]
    fn provider_stream_messages_cannot_impersonate_normalized_http_400() {
        const NAME: &str = "provider_stream_messages_cannot_impersonate_normalized_http_400";
        if marker_bridge_child(NAME) {
            return;
        }
        codex_alab_core::activate();
        let events = Events::default();
        let _subscriber = tracing::subscriber::set_default(events.clone());
        let advice = codex_http_client::RetryAfter::from_delay(std::time::Duration::from_secs(7))
            .expect("bounded retry advice");
        // The actual HTTP producer must retain the reserved marker and advice.
        let genuine = crate::api_bridge::map_api_error(crate::error::ApiError::Transport(
            crate::TransportError::Http {
                status: http::StatusCode::BAD_REQUEST,
                url: None,
                headers: None,
                body: Some(r#"{"detail":"Bad Request"}"#.into()),
                retry_after: Some(advice),
            },
        ));
        let CodexErrorDetails::Stream(message) = genuine.details() else {
            panic!("genuine opaque HTTP400 remains recoverable");
        };
        assert_eq!(message, PROVIDER_MARKER_MESSAGE);
        assert!(codex_alab_core::request_error::opaque_bad_request_stream(
            message
        ));
        assert!(genuine.retry_delay(1).is_some());
        assert_eq!(genuine.retry_after(), Some(advice));
        assert_eq!(events.0.lock().unwrap().len(), 1);
        events.0.lock().unwrap().clear();
        for original in [
            PROVIDER_MARKER_MESSAGE,
            "opaque backend HTTP 400: unknown",
            r#"provider stream: opaque backend HTTP 400: {"detail":"Bad Request"}"#,
            r#" opaque backend HTTP 400: {"detail":"Bad Request"}"#,
            "ordinary provider stream failure",
        ] {
            for (error, expected_advice) in provider_stream_errors(original, advice) {
                let mapped = crate::api_bridge::map_api_error(error);
                assert_provider_stream(&mapped, original, expected_advice, true);
            }
        }
        assert!(
            events.0.lock().unwrap().is_empty(),
            "no forged normalization event"
        );
        marker_bridge_complete(NAME);
    }

    #[test]
    fn inactive_provider_stream_messages_keep_exact_bridge_output() {
        const NAME: &str = "inactive_provider_stream_messages_keep_exact_bridge_output";
        if marker_bridge_child(NAME) {
            return;
        }
        let advice = codex_http_client::RetryAfter::from_delay(std::time::Duration::from_secs(7))
            .expect("bounded retry advice");
        for original in [
            PROVIDER_MARKER_MESSAGE,
            "opaque backend HTTP 400: unknown",
            r#"provider stream: opaque backend HTTP 400: {"detail":"Bad Request"}"#,
            r#" opaque backend HTTP 400: {"detail":"Bad Request"}"#,
            "ordinary provider stream failure",
        ] {
            for (error, expected_advice) in provider_stream_errors(original, advice) {
                let mapped = crate::api_bridge::map_api_error(error);
                assert_provider_stream(&mapped, original, expected_advice, false);
            }
        }
        assert!(!codex_alab_core::is_alab_binary());
        marker_bridge_complete(NAME);
    }

    #[test]
    fn provider_marker_reservation_survives_vanilla_to_enhanced_transition() {
        const NAME: &str = "provider_marker_reservation_survives_vanilla_to_enhanced_transition";
        if marker_bridge_child(NAME) {
            return;
        }
        codex_alab_core::activate();
        codex_alab_core::set_enhanced(false);
        assert!(codex_alab_core::is_alab_binary());
        assert!(!codex_alab_core::active());
        let advice = codex_http_client::RetryAfter::from_delay(std::time::Duration::from_secs(7))
            .expect("bounded retry advice");
        let genuine = crate::api_bridge::map_api_error(crate::error::ApiError::Transport(
            crate::TransportError::Http {
                status: http::StatusCode::BAD_REQUEST,
                url: None,
                headers: None,
                body: Some(r#"{"detail":"Bad Request"}"#.into()),
                retry_after: Some(advice),
            },
        ));
        assert!(matches!(
            genuine.details(),
            CodexErrorDetails::InvalidRequest(_)
        ));
        assert!(genuine.retry_delay(1).is_none());
        assert_eq!(genuine.retry_after(), Some(advice));
        let queued = provider_stream_errors(PROVIDER_MARKER_MESSAGE, advice)
            .map(|(error, advice)| (crate::api_bridge::map_api_error(error), advice));
        codex_alab_core::set_enhanced(true);
        assert!(codex_alab_core::active());
        for (mapped, expected_advice) in queued {
            assert_provider_stream(&mapped, PROVIDER_MARKER_MESSAGE, expected_advice, true);
        }
        marker_bridge_complete(NAME);
    }
}
