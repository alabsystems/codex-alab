// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep safe event IDs until the caller selects its actual terminal result.

use super::RequestDiagnostic;
use super::correlation;
use crate::error::ApiError;
use crate::responses_headers::json_headers_to_http_headers;
use crate::telemetry::WebsocketTelemetry;
use http::HeaderMap;
use serde::Deserialize;
use serde_json::Value;
use serde_json::value::RawValue;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub(super) struct SafeIds {
    pub(super) request_id: Option<String>,
    pub(super) cf_ray: Option<String>,
}

pub(super) type SharedIds = Arc<Mutex<SafeIds>>;

struct WebsocketDiagnostic {
    ids: SharedIds,
    inner: Option<Arc<dyn WebsocketTelemetry>>,
}

pub(super) fn wrap(
    diagnostic: Option<RequestDiagnostic>,
    inner: Option<Arc<dyn WebsocketTelemetry>>,
) -> (
    Option<RequestDiagnostic>,
    Option<Arc<dyn WebsocketTelemetry>>,
) {
    let Some(mut diagnostic) = diagnostic else {
        return (None, inner);
    };
    let ids = Arc::new(Mutex::new(SafeIds::default()));
    diagnostic.websocket_ids = Some(ids.clone());
    let observer = WebsocketDiagnostic { ids, inner };
    (Some(diagnostic), Some(Arc::new(observer)))
}

pub(super) fn selected_ids(diagnostic: Option<&RequestDiagnostic>) -> SafeIds {
    diagnostic
        .and_then(|diagnostic| diagnostic.websocket_ids.as_ref())
        .and_then(|ids| ids.lock().ok().map(|ids| ids.clone()))
        .unwrap_or_default()
}

// Only metadata fields are decoded. Optional containers keep arbitrary value
// types so malformed outer headers cannot erase usable nested correlation.
#[derive(Deserialize)]
struct ErrorMetadata<'a> {
    headers: Option<Value>,
    #[serde(borrow)]
    error: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct FailedMetadata<'a> {
    headers: Option<Value>,
    #[serde(borrow)]
    response: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct FailedResponse<'a> {
    #[serde(borrow)]
    error: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct ErrorHeaders {
    headers: Option<Value>,
}

fn event_ids(text: &str, kind: Option<&str>) -> SafeIds {
    let metadata = match kind {
        Some("error") => serde_json::from_str::<ErrorMetadata<'_>>(text)
            .ok()
            .map(|event| (event.headers, nested_headers(event.error))),
        Some("response.failed") => serde_json::from_str::<FailedMetadata<'_>>(text)
            .ok()
            .map(|event| (event.headers, failed_headers(event.response))),
        _ => None,
    };
    let Some((outer, nested)) = metadata else {
        return SafeIds::default();
    };
    let outer = outer
        .as_ref()
        .and_then(Value::as_object)
        .map(json_headers_to_http_headers);
    let nested = nested
        .as_ref()
        .and_then(Value::as_object)
        .map(json_headers_to_http_headers);
    SafeIds {
        request_id: request_id(outer.as_ref())
            .or_else(|| request_id(nested.as_ref()))
            .map(str::to_owned),
        cf_ray: correlation(outer.as_ref(), "cf-ray")
            .or_else(|| correlation(nested.as_ref(), "cf-ray"))
            .map(str::to_owned),
    }
}

// Optional nested errors need not have the metadata struct's shape. Borrow
// their wire slice only while extracting IDs, and never treat a positional
// error array as a positional header container. Failure here leaves outer IDs.
fn nested_headers(error: Option<&RawValue>) -> Option<Value> {
    let raw = error?.get();
    if !raw.trim_start().starts_with('{') {
        return None;
    }
    serde_json::from_str::<ErrorHeaders>(raw).ok()?.headers
}

fn failed_headers(response: Option<&RawValue>) -> Option<Value> {
    let raw = response?.get();
    if !raw.trim_start().starts_with('{') {
        return None;
    }
    let response = serde_json::from_str::<FailedResponse<'_>>(raw).ok()?;
    nested_headers(response.error)
}

fn request_id(headers: Option<&HeaderMap>) -> Option<&str> {
    correlation(headers, "x-request-id").or_else(|| correlation(headers, "x-oai-request-id"))
}

impl WebsocketTelemetry for WebsocketDiagnostic {
    fn on_ws_request(&self, duration: Duration, error: Option<&ApiError>, connection_reused: bool) {
        if let Some(inner) = self.inner.as_ref() {
            inner.on_ws_request(duration, error, connection_reused);
        }
    }

    fn on_ws_receive_failure(&self, duration: Duration) {
        if let Some(inner) = self.inner.as_ref() {
            inner.on_ws_receive_failure(duration);
        }
    }

    fn on_ws_text_event(&self, text: &str, kind: Option<&str>, duration: Duration) {
        // A new frame replaces the candidate, including malformed, unrelated,
        // missing-header and completed frames. Keep neither headers nor payload.
        let next = event_ids(text, kind);
        if let Ok(mut ids) = self.ids.lock() {
            *ids = next;
        }
        // Caller telemetry observes its original arguments and current context.
        // No state lock or captured request dispatch is held across the callback.
        if let Some(inner) = self.inner.as_ref() {
            inner.on_ws_text_event(text, kind, duration);
        }
    }
}

#[cfg(test)]
#[path = "api_websocket_diagnostics_tests.rs"]
mod tests;
