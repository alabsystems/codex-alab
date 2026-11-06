// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve explicit WebSocket status independently of envelope metadata.

use super::ApiError;
use super::WrappedWebsocketErrorEvent;
use crate::responses_headers::json_headers_to_http_headers;
use codex_http_client::RetryAfter;
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
    let mut fields: BTreeMap<&str, &RawValue> = raw
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
        fields.remove("status_code");
        if let Some(event) = serde_json::to_string(&fields)
            .ok()
            .and_then(|normalized| serde_json::from_str(&normalized).ok())
        {
            return Ok(Some(event));
        }
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
