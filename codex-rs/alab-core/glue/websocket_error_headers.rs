// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve explicit WebSocket status and safe error correlation metadata.

use super::ApiError;
use super::WrappedWebsocketError;
use super::WrappedWebsocketErrorEvent;
use super::alab_request_diagnostics::correlation;
use crate::responses_headers::json_headers_to_http_headers;
use codex_http_client::RetryAfter;
use http::HeaderMap;
use http::StatusCode;
use serde::Deserialize;
use serde::Deserializer;
use serde::de::MapAccess;
use serde::de::Visitor;
use serde_json::Map;
use serde_json::Value;
use serde_json::value::RawValue;
use std::collections::BTreeMap;
use std::fmt;

/// Keep outer metadata intact and fill only missing, validated correlation IDs
/// from the already parsed error. Nested auth/recovery headers never propagate.
pub(super) fn http_headers(
    outer: Option<&Map<String, Value>>,
    nested: Option<&Value>,
) -> Option<HeaderMap> {
    let mut headers = outer.map(json_headers_to_http_headers);
    if !codex_alab_core::active() {
        return headers;
    }
    let Some(nested) = nested.and_then(Value::as_object) else {
        return headers;
    };
    // Use the existing converter's value types and case-collision ordering.
    let nested = json_headers_to_http_headers(nested);
    copy_correlation(
        &mut headers,
        &nested,
        &["x-request-id", "x-oai-request-id"],
        "x-request-id",
    );
    copy_correlation(&mut headers, &nested, &["cf-ray"], "cf-ray");
    headers
}

fn copy_correlation(
    headers: &mut Option<HeaderMap>,
    nested: &HeaderMap,
    names: &[&'static str],
    canonical: &'static str,
) {
    if names
        .iter()
        .any(|name| correlation(headers.as_ref(), name).is_some())
    {
        return;
    }
    if let Some(value) = names.iter().find_map(|name| {
        correlation(Some(nested), name)?;
        nested.get(*name).cloned()
    }) {
        headers
            .get_or_insert_with(HeaderMap::new)
            .insert(canonical, value);
    }
}

/// Keep the ordinary typed parse first. Only its failure can require a second
/// parse, so normal response deltas do not pay for envelope normalization.
pub(super) fn parse_for_stream(
    payload: &str,
) -> Result<Option<WrappedWebsocketErrorEvent>, ApiError> {
    if !codex_alab_core::active() {
        return Ok(super::parse_wrapped_websocket_error_event(payload));
    }
    if let Ok(event) = serde_json::from_str::<WrappedWebsocketErrorEvent>(payload) {
        return Ok((event.kind == "error").then_some(event));
    }
    let Ok(raw) = serde_json::from_str::<WireFields>(payload) else {
        return Ok(None);
    };
    if !raw.0.iter().any(|(name, value)| {
        name == "type"
            && serde_json::from_str::<String>(value.get()).ok().as_deref() == Some("error")
    }) {
        return Ok(None);
    }
    let fields: BTreeMap<&str, &RawValue> = raw
        .0
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_ref()))
        .collect();
    if fields.len() == raw.0.len()
        && let (Some(status), Some(alias)) = (
            fields.get("status").and_then(|value| http_status(value)),
            fields
                .get("status_code")
                .and_then(|value| http_status(value)),
        )
        && status == alias
    {
        // Both aliases unambiguously identify the same HTTP failure. Remove only
        // the alias in this parse copy. Raw values preserve nested duplicate keys
        // for strict decoding; the mapper retains the original wire body.
        let mut normalized = fields.clone();
        normalized.remove("status_code");
        if let Some(event) = serde_json::to_string(&normalized)
            .ok()
            .and_then(|normalized| serde_json::from_str(&normalized).ok())
        {
            return Ok(Some(event));
        }
    }
    if let Some(event) = recover_explicit_status(&fields, raw.0.len()) {
        return Ok(Some(event));
    }
    let errors: Vec<Value> = raw
        .0
        .iter()
        .filter(|(name, _)| name == "error")
        .filter_map(|(_, value)| serde_json::from_str(value.get()).ok())
        .collect();
    if raw.0.iter().any(|(name, value)| {
        matches!(name.as_str(), "status" | "status_code")
            && http_status(value).is_some_and(|status| {
                codex_alab_core::capacity::permanent_client_status(status.as_u16())
            })
    }) && errors.iter().any(|error| {
        error
            .get("code")
            .and_then(Value::as_str)
            .and_then(codex_alab_core::capacity::CapacityReason::from_code)
            .is_some()
    }) {
        // Invalid metadata must not erase a reported refusal and enter the
        // unbounded capacity loop. Use ordinary bounded retry/fallback without
        // inventing an HTTP identity, and retain any validated provider deadline.
        // Preserve repeated raw error/header fields until every deadline is
        // validated. Envelope advice remains a fallback to valid nested advice.
        let retry_after = raw
            .0
            .iter()
            .filter(|(name, _)| name == "error")
            .filter_map(|(_, value)| serde_json::from_str::<WireFields>(value.get()).ok())
            .flat_map(|fields| fields.0)
            .filter(|(name, _)| name == "headers")
            .filter_map(|(_, value)| retry_advice(&value))
            .max_by_key(|advice| advice.deadline())
            .or_else(|| {
                raw.0
                    .iter()
                    .filter(|(name, _)| name == "headers")
                    .filter_map(|(_, value)| retry_advice(value))
                    .max_by_key(|advice| advice.deadline())
            });
        return Err(ApiError::Retryable {
            message: "invalid WebSocket error metadata".into(),
            retry_after,
        });
    }
    Ok(None)
}

// Optional body fields must not erase an independently valid HTTP route.
// Keep strict decoding first; only unique, unambiguous wrappers reach this copy.
fn recover_explicit_status(
    fields: &BTreeMap<&str, &RawValue>,
    field_count: usize,
) -> Option<WrappedWebsocketErrorEvent> {
    if fields.len() != field_count
        || serde_json::from_str::<String>(fields.get("type")?.get()).ok()? != "error"
    {
        return None;
    }
    let status = match (fields.get("status"), fields.get("status_code")) {
        (Some(status), None) | (None, Some(status)) => http_status(status)?,
        (Some(status), Some(alias)) => {
            let status = http_status(status)?;
            if status != http_status(alias)? {
                return None;
            }
            status
        }
        (None, None) => return None,
    };
    if status.is_success() {
        return None;
    }
    let error = match fields.get("error") {
        Some(raw) if raw.get().trim_start().starts_with('{') => {
            let body = serde_json::from_str::<WireFields>(raw.get()).ok()?;
            let body_fields: BTreeMap<&str, &RawValue> = body
                .0
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_ref()))
                .collect();
            if body_fields.len() != body.0.len() {
                return None;
            }
            let headers = body_fields
                .get("headers")
                .and_then(|headers| serde_json::from_str::<Value>(headers.get()).ok());
            Some(WrappedWebsocketError {
                code: None,
                message: None,
                headers,
            })
        }
        _ => None,
    };
    let mut normalized = fields.clone();
    normalized.remove("error");
    if normalized.contains_key("status") && normalized.contains_key("status_code") {
        normalized.remove("status_code");
    }
    let mut event: WrappedWebsocketErrorEvent = serde_json::to_string(&normalized)
        .ok()
        .and_then(|normalized| serde_json::from_str(&normalized).ok())?;
    // Reattach only optional header metadata. The original payload stays with
    // the caller, and malformed code/message cannot select a special retry.
    event.error = error;
    Some(event)
}

fn http_status(value: &RawValue) -> Option<StatusCode> {
    let status = serde_json::from_str::<u16>(value.get()).ok()?;
    StatusCode::from_u16(status).ok()
}

fn retry_advice(headers: &RawValue) -> Option<RetryAfter> {
    serde_json::from_str::<WireFields>(headers.get())
        .ok()?
        .0
        .into_iter()
        .filter_map(|(name, value)| {
            let value = serde_json::from_str::<Value>(value.get()).ok()?;
            let headers = Map::from_iter([(name, value)]);
            RetryAfter::from_headers(&json_headers_to_http_headers(&headers))
        })
        .max_by_key(|advice| advice.deadline())
}

// A Value map would discard earlier status fields. Capture every raw top-level
// field, and keep nested JSON untouched until the typed decoder validates it.
struct WireFields(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for WireFields {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FieldsVisitor;

        impl<'de> Visitor<'de> for FieldsVisitor {
            type Value = WireFields;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON error envelope")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut fields = Vec::new();
                while let Some(field) = map.next_entry()? {
                    fields.push(field);
                }
                Ok(WireFields(fields))
            }
        }

        deserializer.deserialize_map(FieldsVisitor)
    }
}

pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Option<Map<String, Value>>, D::Error>
where
    D: Deserializer<'de>,
{
    if !codex_alab_core::active() {
        return Option::<Map<String, Value>>::deserialize(deserializer);
    }
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::Object(headers)) => Some(headers),
        _ => None,
    })
}
