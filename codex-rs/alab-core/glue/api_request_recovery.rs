// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use codex_protocol::error::CodexErr;

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
}
