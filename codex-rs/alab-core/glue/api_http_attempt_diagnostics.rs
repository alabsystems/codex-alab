// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Completed HTTP operations, including auth/source work before transport I/O.

use super::RequestDiagnostic;
use super::correlation;
use codex_client::RequestTelemetry;
use codex_client::TransportError;
use http::StatusCode;
use std::sync::Arc;
use std::time::Duration;

struct HttpOperations {
    diagnostic: RequestDiagnostic,
    inner: Option<Arc<dyn RequestTelemetry>>,
}

pub(crate) fn request_telemetry(
    diagnostic: Option<&RequestDiagnostic>,
    inner: Option<Arc<dyn RequestTelemetry>>,
) -> Option<Arc<dyn RequestTelemetry>> {
    let Some(diagnostic) = diagnostic else {
        return inner;
    };
    Some(Arc::new(HttpOperations {
        diagnostic: diagnostic.clone(),
        inner,
    }))
}

impl RequestTelemetry for HttpOperations {
    fn on_request(
        &self,
        operation_attempt: u64,
        status: Option<StatusCode>,
        error: Option<&TransportError>,
        duration: Duration,
    ) {
        // The existing observer owns its original arguments and ambient context.
        if let Some(inner) = self.inner.as_ref() {
            inner.on_request(operation_attempt, status, error, duration);
        }
        if error.is_none() && operation_attempt == 0 {
            return;
        }

        // Typed metadata only: do not inspect payloads or sample retry clocks.
        let (kind, headers, retry_advised) = match error {
            Some(TransportError::Http {
                headers,
                retry_after,
                ..
            }) => (Some("http"), headers.as_ref(), Some(retry_after.is_some())),
            Some(TransportError::Policy(_)) => (Some("network_policy"), None, None),
            Some(TransportError::RetryLimit) => (Some("retry_limit"), None, None),
            Some(TransportError::Timeout) => (Some("timeout"), None, None),
            Some(TransportError::Connection(_)) => (Some("connection"), None, None),
            Some(TransportError::Network(_)) => (Some("network"), None, None),
            Some(TransportError::Build(_)) => (Some("request_build"), None, None),
            Some(TransportError::ResponseTooLarge { .. }) => {
                (Some("response_too_large"), None, None)
            }
            None => (None, None, None),
        };
        tracing::dispatcher::with_default(&self.diagnostic.dispatch, || {
            let _entered = self.diagnostic.span.enter();
            tracing::info!(
                local_attempt = self.diagnostic.attempt,
                transport = self.diagnostic.transport,
                operation_attempt,
                elapsed_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
                http_status = status.map(|status| status.as_u16()),
                failure_kind = kind,
                retry_advised,
                request_id = correlation(headers, "x-request-id")
                    .or_else(|| correlation(headers, "x-oai-request-id")),
                cf_ray = correlation(headers, "cf-ray"),
                "Responses HTTP operation completed"
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;

    #[derive(Debug, PartialEq, Eq)]
    struct Call {
        attempt: u64,
        status: Option<StatusCode>,
        error_address: Option<usize>,
        duration: Duration,
        ambient_dispatch: &'static str,
        ambient_span: Option<tracing::span::Id>,
    }

    struct Observer {
        calls: Arc<Mutex<Vec<Call>>>,
        ambient: Recorder,
    }

    impl RequestTelemetry for Observer {
        fn on_request(
            &self,
            attempt: u64,
            status: Option<StatusCode>,
            error: Option<&TransportError>,
            duration: Duration,
        ) {
            let ambient_dispatch = tracing::dispatcher::get_default(|dispatch| {
                dispatch.downcast_ref::<Recorder>().unwrap().label
            });
            let ambient_span = self.ambient.entered.lock().unwrap().last().cloned();
            self.calls.lock().unwrap().push(Call {
                attempt,
                status,
                error_address: error.map(|error| std::ptr::from_ref(error).addr()),
                duration,
                ambient_dispatch,
                ambient_span,
            });
            tracing::info!(target: "operation_contract_delegate", "delegate");
        }
    }

    #[derive(Default)]
    struct Fields(BTreeMap<String, String>);

    impl tracing::field::Visit for Fields {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.insert(field.name().to_owned(), format!("{value:?}"));
        }
    }

    #[derive(Debug)]
    struct Event {
        dispatch: &'static str,
        span: Option<tracing::span::Id>,
        target: &'static str,
        fields: BTreeMap<String, String>,
    }

    #[derive(Clone)]
    struct Recorder {
        label: &'static str,
        next: Arc<AtomicU64>,
        entered: Arc<Mutex<Vec<tracing::span::Id>>>,
        events: Arc<Mutex<Vec<Event>>>,
    }

    impl Recorder {
        fn new(label: &'static str, events: Arc<Mutex<Vec<Event>>>) -> Self {
            Self {
                label,
                next: Arc::new(AtomicU64::new(1)),
                entered: Arc::default(),
                events,
            }
        }
    }

    impl tracing::Subscriber for Recorder {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(self.next.fetch_add(1, Ordering::Relaxed))
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn enter(&self, id: &tracing::span::Id) {
            self.entered.lock().unwrap().push(id.clone());
        }
        fn exit(&self, id: &tracing::span::Id) {
            assert_eq!(self.entered.lock().unwrap().pop().as_ref(), Some(id));
        }
        fn event(&self, event: &tracing::Event<'_>) {
            let mut fields = Fields::default();
            event.record(&mut fields);
            self.events.lock().unwrap().push(Event {
                dispatch: self.label,
                span: self.entered.lock().unwrap().last().cloned(),
                target: event.metadata().target(),
                fields: fields.0,
            });
        }
    }

    #[test]
    fn inactive_request_observer_retains_arc_identity() {
        let observer: Arc<dyn RequestTelemetry> = Arc::new(Observer {
            calls: Arc::default(),
            ambient: Recorder::new("unused", Arc::default()),
        });
        let returned = request_telemetry(None, Some(observer.clone())).unwrap();
        assert!(Arc::ptr_eq(&observer, &returned));
        assert!(request_telemetry(None, None).is_none());
    }

    #[test]
    fn observer_arguments_and_context_survive_local_operation_diagnostics() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let request_recorder = Recorder::new("request", events.clone());
        let ambient_recorder = Recorder::new("ambient", events.clone());
        let request_dispatch = tracing::Dispatch::new(request_recorder);
        let ambient_dispatch = tracing::Dispatch::new(ambient_recorder.clone());
        let request_span = tracing::dispatcher::with_default(&request_dispatch, || {
            tracing::info_span!("captured_request")
        });
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observer: Arc<dyn RequestTelemetry> = Arc::new(Observer {
            calls: calls.clone(),
            ambient: ambient_recorder.clone(),
        });
        let diagnostic = RequestDiagnostic {
            attempt: 71,
            started: std::time::Instant::now(),
            transport: "responses_http",
            dispatch: request_dispatch,
            span: request_span.clone(),
            websocket_ids: None,
        };
        let wrapper = request_telemetry(Some(&diagnostic), Some(observer)).unwrap();
        let error = TransportError::Http {
            status: StatusCode::BAD_REQUEST,
            url: Some("https://never-log.example/private".into()),
            headers: Some(
                [
                    ("x-request-id", "req_contract"),
                    ("cf-ray", "contains private content"),
                ]
                .into_iter()
                .map(|(name, value)| (name.parse().unwrap(), value.parse().unwrap()))
                .collect(),
            ),
            body: Some("never-log-private-body".into()),
            retry_after: None,
        };
        let duration = Duration::from_micros(123_456);
        let ambient_span = tracing::dispatcher::with_default(&ambient_dispatch, || {
            let span = tracing::info_span!("ambient_callback");
            let _entered = span.enter();
            wrapper.on_request(0, Some(StatusCode::BAD_REQUEST), Some(&error), duration);
            wrapper.on_request(0, Some(StatusCode::OK), None, duration);
            wrapper.on_request(2, Some(StatusCode::CREATED), None, duration);
            tracing::info!(target: "operation_contract_after", "after");
            span.clone()
        });
        let expected = [
            (
                0,
                Some(StatusCode::BAD_REQUEST),
                Some(std::ptr::from_ref(&error).addr()),
            ),
            (0, Some(StatusCode::OK), None),
            (2, Some(StatusCode::CREATED), None),
        ]
        .map(|(attempt, status, error_address)| Call {
            attempt,
            status,
            error_address,
            duration,
            ambient_dispatch: "ambient",
            ambient_span: ambient_span.id(),
        });
        assert_eq!(*calls.lock().unwrap(), expected);
        assert!(ambient_recorder.entered.lock().unwrap().is_empty());
        let events = events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.dispatch)
                .collect::<Vec<_>>(),
            [
                "ambient", "request", "ambient", "ambient", "request", "ambient"
            ]
        );
        for index in [0, 2, 3, 5] {
            assert_eq!(events[index].span, ambient_span.id());
        }
        for index in [1, 4] {
            assert_eq!(events[index].span, request_span.id());
            assert_eq!(
                events[index].target,
                "codex_api::endpoint::alab_request_diagnostics::http_operations"
            );
            assert_eq!(events[index].fields["local_attempt"], "71");
            assert_eq!(events[index].fields["elapsed_ms"], "123");
        }
        assert_eq!(events[1].fields["request_id"], "\"req_contract\"");
        assert_eq!(events[1].fields["retry_advised"], "false");
        assert_eq!(events[4].fields["operation_attempt"], "2");
        for absent in ["request_id", "cf_ray", "failure_kind", "retry_advised"] {
            assert!(!events[4].fields.contains_key(absent));
        }
        assert!(!format!("{events:?}").contains("never-log"));
    }
}
